//! External references: finding and reading attached drawings, XATTACH's
//! Attach External Reference dialog, and the External References palette
//! (EXTERNALREFERENCES, XR) to reload, unload, detach and bind them.

use std::path::{Component, Path, PathBuf};

use eframe::egui::{self, Align, Layout, RichText, Ui};
use oxidraft_geom::Point;
use oxidraft_model::{Document, Xref, XrefStatus, check_name};

use crate::{app::OxidraftApp, theme};

/// Attached drawings nested deeper than this aren't read.
const MAX_NESTING: usize = 8;

/// How XATTACH saves where the file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathType {
    Full,
    Relative,
    None,
}

/// The Attach External Reference dialog.
pub(crate) struct AttachDialog {
    file: PathBuf,
    content: Document,
    name: String,
    path_type: PathType,
    overlay: bool,
    on_screen: bool,
    at: Point,
    scale: f64,
    rotation: f64,
}

/// Where an xref's file is: its saved path (Windows separators too), taken
/// from the drawing's folder unless it's absolute; failing that, a file of
/// that name in the drawing's folder, where AutoCAD looks next.
pub(crate) fn resolve(host: Option<&Path>, saved: &str) -> Option<PathBuf> {
    let saved = Path::new(&saved.replace('\\', "/")).to_path_buf();
    let dir = host.and_then(Path::parent);
    let mut candidates = Vec::new();
    if saved.is_absolute() {
        candidates.push(saved.clone());
    } else if let Some(d) = dir {
        candidates.push(d.join(&saved));
    }
    if let (Some(d), Some(name)) = (dir, saved.file_name()) {
        candidates.push(d.join(name));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// `to` written from the folder `from`, like "../survey/site.dwg".
pub(crate) fn relative(from: &Path, to: &Path) -> Option<String> {
    let (from, to): (Vec<Component>, Vec<Component>) = (from.components().collect(), to.components().collect());
    // Different drives (Windows) have no relative path.
    if from.first() != to.first() {
        return None;
    }
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(to[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    Some(parts.join(std::path::MAIN_SEPARATOR_STR))
}

/// Reads a DWG or DXF drawing.
pub(crate) fn read_drawing(path: &Path) -> Result<Document, String> {
    if oxidraft_io_dwg::is_dwg(path) {
        oxidraft_io_dwg::load_file(path).map(|o| o.imported.document)
    } else {
        oxidraft_io_dwg::load_dxf(path).map(|i| i.document)
    }
}

/// Reads an attached drawing with the drawings it attaches itself (not its
/// overlays), never going round in a circle.
pub(crate) fn read_xref(path: &Path, chain: &mut Vec<PathBuf>) -> Result<Document, String> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if chain.contains(&canonical) {
        return Err("it attaches itself, directly or through another drawing".into());
    }
    if chain.len() > MAX_NESTING {
        return Err("attached drawings are nested too deep".into());
    }
    chain.push(canonical);
    let mut doc = read_drawing(path)?;
    let nested: Vec<Xref> = doc.xrefs().cloned().collect();
    for x in nested {
        if x.overlay {
            continue;
        }
        match resolve(Some(path), &x.path).map(|p| read_xref(&p, chain)) {
            Some(Ok(content)) => doc.load_xref(&x.name, &content),
            _ => doc.set_xref_status(&x.name, XrefStatus::NotFound),
        }
    }
    chain.pop();
    Ok(doc)
}

/// REFCLOSE's Save for an xref, waiting to be confirmed: the xref's file
/// as read again, to put the edits into and write back.
pub(crate) struct XrefEditSave {
    name: String,
    path: PathBuf,
    site: Document,
    /// The file's DWG version, or `None` for DXF.
    version: Option<oxidraft_io_dwg::Version>,
    /// Objects in the file of types Oxidraft can't keep.
    skipped: usize,
}

/// The backup AutoCAD makes when it writes over a drawing: "site.bak".
fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("bak")
}

/// A long path cut to fit the palette, whole on hover.
fn path_label(ui: &mut Ui, path: &str, width: f32) {
    ui.add_sized([width, 18.0], egui::Label::new(path).truncate()).on_hover_text(path);
}

/// A file's size, as the palette shows it.
fn size_label(path: &Path) -> String {
    match std::fs::metadata(path).map(|m| m.len()) {
        Ok(b) if b >= 1 << 20 => format!("{:.1} MB", b as f64 / f64::from(1 << 20)),
        Ok(b) => format!("{} KB", b.div_ceil(1024)),
        Err(_) => String::new(),
    }
}

impl OxidraftApp {
    /// The drawing's own folder chain, to read xrefs from: where it is.
    fn host_chain(&self) -> Vec<PathBuf> {
        self.path.iter().map(|p| p.canonicalize().unwrap_or_else(|_| p.clone())).collect()
    }

    /// Reads one xref's file into the drawing, or marks it not found.
    /// Returns where it was found.
    fn read_into(&mut self, name: &str) -> Option<PathBuf> {
        let x = self.doc.xref(name)?.clone();
        let found = resolve(self.path.as_deref(), &x.path);
        let read = found.as_ref().map(|p| read_xref(p, &mut self.host_chain()));
        match read {
            Some(Ok(content)) => {
                self.doc.load_xref(&x.name, &content);
                let nested = content.xrefs().map(|n| (n.name.clone(), n.path.clone(), n.status)).collect();
                self.nested_xrefs.insert(x.name.to_ascii_uppercase(), nested);
                self.engine.log(format!("Resolve Xref \"{}\": {}", x.name, found.as_ref()?.display()));
                self.engine.log(format!("\"{}\" loaded.", x.name));
                found
            }
            Some(Err(e)) => {
                self.doc.set_xref_status(&x.name, XrefStatus::NotFound);
                self.engine.log(format!("Unable to load \"{}\": {e}", x.name));
                None
            }
            None => {
                self.doc.set_xref_status(&x.name, XrefStatus::NotFound);
                self.engine.log(format!("Xref \"{}\" not found: {}", x.name, x.path));
                None
            }
        }
    }

    /// REFCLOSE's Save for an xref: reads its file again and asks to write
    /// the edits into it.
    pub(crate) fn prepare_xref_edit_save(&mut self) {
        let Some(name) = self.doc.refedit().map(|r| r.block.clone()) else { return };
        let Some(x) = self.doc.xref(&name).cloned() else { return };
        let Some(path) = resolve(self.path.as_deref(), &x.path) else {
            self.engine.log(format!("Xref \"{name}\" not found: {}; the edits aren't saved.", x.path));
            return;
        };
        let read = if oxidraft_io_dwg::is_dwg(&path) {
            oxidraft_io_dwg::load_file(&path).map(|o| (o.imported, Some(o.version)))
        } else {
            oxidraft_io_dxf::load_file(&path).map(|i| (i, None)).map_err(|e| e.to_string())
        };
        match read {
            Ok((imported, version)) => {
                self.xref_edit_save =
                    Some(XrefEditSave { name, path, site: imported.document, version, skipped: imported.skipped });
            }
            Err(e) => self.engine.log(format!("Could not read {}: {e}; the edits aren't saved.", path.display())),
        }
    }

    /// Asks before REFCLOSE writes an xref's file, as AutoCAD does, saying
    /// what in it Oxidraft can't keep.
    pub(crate) fn xref_edit_save_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = self.xref_edit_save.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let file = d.path.file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let backup = backup_path(&d.path).file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned());
        egui::Window::new("Save Reference Edits")
            .id(egui::Id::unique("xref_edit_save"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([360.0, 200.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(format!("All reference edits will be saved to {file}.")).strong());
                ui.add_space(4.0);
                if d.skipped > 0 {
                    ui.colored_label(
                        theme::warning(),
                        format!(
                            "Oxidraft rewrites the file and can't keep {} object(s) of types it doesn't support; they'll be left out.",
                            d.skipped
                        ),
                    );
                }
                ui.colored_label(theme::text_dim(), format!("The file as it is now is kept as {backup}."));
                ui.add_space(6.0);
                let row = egui::vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok {
            self.save_xref_edit(d);
        } else if open && !cancel {
            self.xref_edit_save = Some(d);
        } else {
            self.engine.log("Reference edits not saved; still editing.".to_string());
        }
    }

    /// Writes the edits into the xref's file (keeping the old file as .bak),
    /// ends the reference edit and reads the xref again.
    fn save_xref_edit(&mut self, mut d: XrefEditSave) {
        self.engine.cancel(&mut self.doc);
        self.doc.store_refedit(&mut d.site);
        let written =
            std::fs::copy(&d.path, backup_path(&d.path)).map_err(|e| e.to_string()).and_then(|_| match d.version {
                Some(v) => oxidraft_io_dwg::save_file(&d.site, &d.path, v),
                None => oxidraft_io_dwg::save_dxf(&d.site, &d.path),
            });
        if let Err(e) = written {
            self.engine.log(format!("Could not save {}: {e}; still editing.", d.path.display()));
            return;
        }
        self.doc.begin("REFCLOSE");
        self.doc.close_refedit(true);
        self.read_into(&d.name);
        self.doc.commit();
        self.engine.log(format!("Xref \"{}\" saved to {}.", d.name, d.path.display()));
    }

    /// Reads every attached drawing, right after opening.
    pub(crate) fn load_xrefs(&mut self) {
        self.nested_xrefs.clear();
        let names: Vec<String> = self.doc.xrefs().map(|x| x.name.clone()).collect();
        for name in names {
            self.read_into(&name);
        }
    }

    /// The palette's Open: the attached drawing in a window of its own.
    pub fn open_xref(&mut self, name: &str) {
        let Some(x) = self.doc.xref(name) else { return };
        let Some(path) = resolve(self.path.as_deref(), &x.path) else {
            self.engine.log(format!("Xref \"{}\" not found: {}", x.name, x.path));
            return;
        };
        let launched = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg(&path).spawn());
        match launched {
            Ok(_) => self.engine.log(format!("Opening {} in a new window.", path.display())),
            Err(e) => self.engine.log(format!("Could not open {}: {e}", path.display())),
        }
    }

    /// The palette's Browse: picks where an attached drawing's file is now,
    /// saves that path and reads it from there.
    pub(crate) fn browse_xref(&mut self, name: &str) {
        let Some(x) = self.doc.xref(name).cloned() else { return };
        let picked = rfd::FileDialog::new()
            .set_title(format!("Find \"{}\"", x.name))
            .add_filter("Drawings", &["dwg", "dxf"])
            .pick_file();
        if let Some(file) = picked {
            self.set_xref_file(name, &file);
        }
    }

    /// Points xref `name` at `file` and reads it from there.
    pub fn set_xref_file(&mut self, name: &str, file: &Path) {
        let Some(x) = self.doc.xref(name).cloned() else { return };
        let kind = if self.path.is_some() { PathType::Relative } else { PathType::Full };
        self.engine.cancel(&mut self.doc);
        self.doc.begin("XREF");
        self.doc.set_xref(Xref { path: self.saved_path(file, kind), ..x });
        self.read_into(name);
        self.doc.commit();
    }

    /// The palette's Reload: reads the file again.
    pub fn reload_xref(&mut self, name: &str) {
        self.engine.cancel(&mut self.doc);
        self.doc.begin("XREF");
        self.read_into(name);
        self.doc.commit();
    }

    pub fn unload_xref(&mut self, name: &str) {
        self.engine.cancel(&mut self.doc);
        self.doc.begin("XREF");
        self.doc.set_xref_status(name, XrefStatus::Unloaded);
        self.doc.commit();
    }

    pub fn detach_xref(&mut self, name: &str) {
        self.engine.cancel(&mut self.doc);
        self.doc.begin("XREF");
        self.doc.detach_xref(name);
        self.doc.commit();
        self.engine.log(format!("Detach Xref \"{name}\""));
        self.xref_selected = None;
    }

    pub fn bind_xref(&mut self, name: &str, insert: bool) {
        self.engine.cancel(&mut self.doc);
        self.doc.begin("XBIND");
        self.doc.bind_xref(name, insert);
        self.doc.commit();
        self.engine.log(format!("\"{name}\" bound{}.", if insert { " (inserted)" } else { "" }));
        self.xref_selected = None;
    }

    /// XATTACH: picks the drawing, then opens the Attach dialog.
    pub(crate) fn xattach(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Drawing (DWG, DXF)", &["dwg", "dxf"]);
        if let Some(path) = dialog.pick_file() {
            self.attach_file(path);
        }
    }

    /// Reads `path` and opens the Attach External Reference dialog for it.
    pub fn attach_file(&mut self, path: PathBuf) {
        let content = match read_xref(&path, &mut self.host_chain()) {
            Ok(c) => c,
            Err(e) => {
                self.engine.log(format!("Can't attach {}: {e}", path.display()));
                return;
            }
        };
        let name = path.file_stem().map_or("Xref".into(), |s| s.to_string_lossy().into_owned());
        let at = Point::ORIGIN;
        self.attach_dialog = Some(AttachDialog {
            file: path,
            content,
            name,
            path_type: if self.path.is_some() { PathType::Relative } else { PathType::Full },
            overlay: false,
            on_screen: true,
            at,
            scale: 1.0,
            rotation: 0.0,
        });
    }

    /// How a file attached here is saved: its full path, its path from
    /// this drawing's folder (when the drawing has been saved), or its name.
    pub(crate) fn saved_path(&self, file: &Path, kind: PathType) -> String {
        let full = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let host_dir =
            self.path.as_ref().and_then(|p| p.canonicalize().ok()).and_then(|p| p.parent().map(Path::to_path_buf));
        match (kind, host_dir) {
            (PathType::Relative, Some(dir)) => relative(&dir, &full).unwrap_or_else(|| full.display().to_string()),
            (PathType::None, _) => full.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()),
            _ => full.display().to_string(),
        }
    }

    /// The Attach dialog's OK: checks the name, then places the drawing.
    fn finish_attach(&mut self, d: AttachDialog) -> Result<(), String> {
        check_name(&d.name)?;
        if let Some(existing) = self.doc.xref(&d.name)
            && resolve(self.path.as_deref(), &existing.path).and_then(|p| p.canonicalize().ok())
                != d.file.canonicalize().ok()
        {
            return Err(format!("\"{}\" is already attached from another file.", d.name));
        }
        if self.doc.xref(&d.name).is_none() && self.doc.block(&d.name).is_some() {
            return Err(format!("A block named \"{}\" already exists.", d.name));
        }
        let path = self.saved_path(&d.file, d.path_type);
        let xref = Xref { name: d.name.clone(), path, overlay: d.overlay, status: XrefStatus::Loaded };
        let at = (!d.on_screen).then_some(d.at);
        self.engine.attach_xref(xref, d.content, d.scale, d.rotation, at, &mut self.doc);
        Ok(())
    }

    pub(crate) fn attach_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.attach_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Attach External Reference")
            .id(egui::Id::unique("attach_xref"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([320.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                egui::Grid::new("attach_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Name:");
                    let field = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(240.0));
                    self.text_fields.push(field.id);
                    ui.end_row();
                    ui.label("Found in:");
                    ui.colored_label(theme::text_dim(), d.file.display().to_string());
                    ui.end_row();
                    ui.label("Path type:");
                    egui::ComboBox::from_id_salt("attach_path_type")
                        .selected_text(match d.path_type {
                            PathType::Full => "Full path",
                            PathType::Relative => "Relative path",
                            PathType::None => "No path",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.path_type, PathType::Full, "Full path");
                            ui.add_enabled_ui(self.path.is_some(), |ui| {
                                ui.selectable_value(&mut d.path_type, PathType::Relative, "Relative path")
                                    .on_disabled_hover_text("Save this drawing first.");
                            });
                            ui.selectable_value(&mut d.path_type, PathType::None, "No path");
                        });
                    ui.end_row();
                    ui.label("Reference type:");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut d.overlay, false, "Attachment");
                        ui.radio_value(&mut d.overlay, true, "Overlay");
                    });
                    ui.end_row();
                    ui.label("Insertion point:");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut d.on_screen, "Specify on-screen");
                        ui.add_enabled_ui(!d.on_screen, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("X:");
                                ui.add(egui::DragValue::new(&mut d.at.x).speed(1.0).max_decimals(4));
                                ui.label("Y:");
                                ui.add(egui::DragValue::new(&mut d.at.y).speed(1.0).max_decimals(4));
                            });
                        });
                    });
                    ui.end_row();
                    ui.label("Scale:");
                    ui.add(egui::DragValue::new(&mut d.scale).speed(0.01).range(0.0001..=f64::MAX).max_decimals(4));
                    ui.end_row();
                    ui.label("Rotation:");
                    ui.add(egui::DragValue::new(&mut d.rotation).speed(1.0).suffix("°").max_decimals(4));
                    ui.end_row();
                });
                ui.separator();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if !open || cancel {
            return;
        }
        if ok {
            if let Err(e) = self.finish_attach(d) {
                self.engine.log(e);
            }
            return;
        }
        self.attach_dialog = Some(d);
    }

    /// The External References palette: this drawing, then what it attaches.
    pub(crate) fn xref_palette(&mut self, ctx: &egui::Context) {
        if !self.xref_palette_open {
            return;
        }
        let mut open = true;
        let mut action: Option<(&'static str, String)> = None;
        egui::Window::new("External References")
            .id(egui::Id::unique("xref_palette"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([640.0, 300.0])
            .default_pos([300.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Attach DWG...").clicked() {
                        action = Some(("attach", String::new()));
                    }
                    let image = ui
                        .button("Attach Image...")
                        .on_hover_text("One image, or many GeoTIFFs at once (left unloaded)");
                    if image.clicked() {
                        action = Some(("attach image", String::new()));
                    }
                    if ui.button("Attach PDF...").clicked() {
                        action = Some(("attach pdf", String::new()));
                    }
                    ui.separator();
                    if ui.button("Reload All").clicked() {
                        action = Some(("reload all", String::new()));
                    }
                    let view = ui
                        .button("Load Images in View")
                        .on_hover_text("Load the unloaded images the drawing area shows");
                    if view.clicked() {
                        action = Some(("load in view", String::new()));
                    }
                });
                ui.separator();
                self.xref_table(ui, &mut action);
                ui.separator();
                self.xref_details(ui, &mut action);
            });
        if !open {
            self.xref_palette_open = false;
        }
        match action {
            Some(("attach", _)) => self.xattach(),
            Some(("attach image", _)) => self.image_attach(),
            Some(("attach pdf", _)) => self.pdf_attach(),
            Some(("load in view", _)) => self.load_images_in_view(),
            Some(("open", n)) => self.open_xref(&n),
            Some(("browse image", n)) => self.browse_image(&n),
            Some(("find images", _)) => self.browse_images_folder(&self.image_selected.clone()),
            Some(("browse xref", n)) => self.browse_xref(&n),
            Some(("load images", _)) => {
                for n in self.image_selected.clone() {
                    self.load_image(&n);
                }
            }
            Some(("unload images", _)) => {
                for n in self.image_selected.clone() {
                    self.unload_image(&n);
                }
            }
            Some(("detach images", _)) => {
                for n in self.image_selected.clone() {
                    self.detach_image(&n);
                }
            }
            Some(("reload all", _)) => {
                let names: Vec<String> = self.doc.xrefs().map(|x| x.name.clone()).collect();
                for n in names {
                    self.reload_xref(&n);
                }
            }
            Some(("reload", n)) => self.reload_xref(&n),
            Some(("unload", n)) => self.unload_xref(&n),
            Some(("detach", n)) => self.detach_xref(&n),
            Some(("bind", n)) => self.bind_xref(&n, false),
            Some(("insert", n)) => self.bind_xref(&n, true),
            Some(("attach type", n)) | Some(("overlay type", n)) => {
                if let Some(mut x) = self.doc.xref(&n).cloned() {
                    x.overlay = !x.overlay;
                    self.doc.set_xref(x);
                }
            }
            _ => {}
        }
    }

    fn xref_table(&mut self, ui: &mut Ui, action: &mut Option<(&'static str, String)>) {
        let host = self.path.clone();
        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
            egui::Grid::new("xref_table").num_columns(5).striped(true).spacing([16.0, 4.0]).show(ui, |ui| {
                for h in ["Reference Name", "Status", "Size", "Type", "Saved Path"] {
                    ui.label(RichText::new(h).color(theme::text_dim()));
                }
                ui.end_row();
                ui.label(RichText::new(self.file_name()).strong());
                ui.label("Opened");
                ui.label(host.as_deref().map(size_label).unwrap_or_default());
                ui.label("Current");
                path_label(ui, &host.as_ref().map(|p| p.display().to_string()).unwrap_or_default(), 300.0);
                ui.end_row();
                let xrefs: Vec<Xref> = self.doc.xrefs().cloned().collect();
                for x in xrefs {
                    let selected = self.xref_selected.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&x.name));
                    let row = ui.selectable_label(selected, format!("   {}", x.name));
                    if row.clicked() {
                        self.xref_selected = Some(x.name.clone());
                        self.image_selected.clear();
                    }
                    if row.double_clicked() {
                        *action = Some(("reload", x.name.clone()));
                    }
                    let status = if x.status == XrefStatus::Loaded && self.doc.xref_references(&x.name) == 0 {
                        "Unreferenced"
                    } else {
                        x.status.label()
                    };
                    ui.label(status);
                    ui.label(resolve(host.as_deref(), &x.path).as_deref().map(size_label).unwrap_or_default());
                    ui.label(if x.overlay { "Overlay" } else { "Attach" });
                    path_label(ui, &x.path, 300.0);
                    ui.end_row();
                    // What it attaches itself, read along with it.
                    for (name, path, status) in
                        self.nested_xrefs.get(&x.name.to_ascii_uppercase()).cloned().unwrap_or_default()
                    {
                        ui.colored_label(theme::text_dim(), format!("      └ {name}"));
                        ui.colored_label(theme::text_dim(), status.label());
                        ui.label("");
                        ui.colored_label(theme::text_dim(), "Nested");
                        path_label(ui, &path, 300.0);
                        ui.end_row();
                    }
                }
                let defs: Vec<oxidraft_model::ImageDef> = self.doc.image_defs().cloned().collect();
                for d in defs {
                    let selected = self.image_selected.iter().any(|s| s.eq_ignore_ascii_case(&d.name));
                    let row = ui.selectable_label(selected, format!("   {}", d.name));
                    if row.clicked() {
                        // Cmd- or Shift-click picks several images.
                        let many = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                        if !many {
                            self.image_selected.clear();
                        }
                        if selected && many {
                            self.image_selected.retain(|s| !s.eq_ignore_ascii_case(&d.name));
                        } else {
                            self.image_selected.push(d.name.clone());
                        }
                        self.xref_selected = None;
                    }
                    let status = match self.rasters.get(&d.name.to_ascii_uppercase()) {
                        Some(crate::underlay::Raster::Loading) => "Loading…",
                        Some(crate::underlay::Raster::Failed(_)) => "Not Found",
                        _ if d.status == XrefStatus::Loaded && self.doc.image_references(&d.name) == 0 => {
                            "Unreferenced"
                        }
                        _ => d.status.label(),
                    };
                    ui.label(status);
                    ui.label(resolve(host.as_deref(), &d.path).as_deref().map(size_label).unwrap_or_default());
                    ui.label(if d.is_pdf() { "PDF" } else { "Image" });
                    path_label(ui, &d.path, 300.0);
                    ui.end_row();
                }
            });
        });
    }

    fn xref_details(&mut self, ui: &mut Ui, action: &mut Option<(&'static str, String)>) {
        if !self.image_selected.is_empty() {
            self.image_details(ui, action);
            return;
        }
        let Some(x) = self.xref_selected.as_deref().and_then(|n| self.doc.xref(n)).cloned() else {
            ui.colored_label(
                theme::text_dim(),
                "Select a reference to reload, unload, detach or bind it; Cmd- or Shift-click picks several images.",
            );
            return;
        };
        let found = resolve(self.path.as_deref(), &x.path);
        egui::Grid::new("xref_details").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            ui.label("Reference Name");
            ui.label(RichText::new(&x.name).strong());
            ui.end_row();
            ui.label("Status");
            ui.label(x.status.label());
            ui.end_row();
            ui.label("Type");
            ui.horizontal(|ui| {
                if ui.radio(!x.overlay, "Attach").clicked() && x.overlay {
                    *action = Some(("attach type", x.name.clone()));
                }
                if ui.radio(x.overlay, "Overlay").clicked() && !x.overlay {
                    *action = Some(("overlay type", x.name.clone()));
                }
            });
            ui.end_row();
            ui.label("Found At");
            path_label(ui, &found.as_ref().map_or("Not found".into(), |p| p.display().to_string()), 460.0);
            ui.end_row();
            ui.label("Saved Path");
            path_label(ui, &x.path, 460.0);
            ui.end_row();
        });
        ui.horizontal(|ui| {
            for (label, what, tip) in [
                ("Open", "open", "Open the attached drawing in a window of its own"),
                ("Reload", "reload", "Read the file again"),
                ("Unload", "unload", "Keep it attached, but don't show it"),
                ("Detach", "detach", "Remove it and its references"),
                ("Bind", "bind", "Make it part of this drawing, its layers named NAME$0$layer"),
                ("Bind (Insert)", "insert", "Make it part of this drawing, merging its layers into this drawing's"),
                ("Browse...", "browse xref", "Pick where its file is now, and read it from there"),
            ] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    *action = Some((what, x.name.clone()));
                }
            }
        });
    }
}

impl OxidraftApp {
    /// The selected images: one's details, and Load, Unload, Detach for all.
    fn image_details(&mut self, ui: &mut Ui, action: &mut Option<(&'static str, String)>) {
        if let [one] = self.image_selected.as_slice()
            && let Some(d) = self.doc.image_def(one).cloned()
        {
            let found = resolve(self.path.as_deref(), &d.path);
            egui::Grid::new("image_details").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.label("Reference Name");
                ui.label(RichText::new(&d.name).strong());
                ui.end_row();
                ui.label(if d.is_pdf() { "Page" } else { "Size" });
                ui.label(match d.page {
                    Some(p) => format!("{}", p + 1),
                    None => format!("{} x {} pixels", d.width, d.height),
                });
                ui.end_row();
                ui.label("Found At");
                path_label(ui, &found.as_ref().map_or("Not found".into(), |p| p.display().to_string()), 460.0);
                ui.end_row();
                ui.label("Saved Path");
                path_label(ui, &d.path, 460.0);
                ui.end_row();
            });
        } else {
            ui.label(format!("{} images selected", self.image_selected.len()));
        }
        let one = match self.image_selected.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        };
        ui.horizontal(|ui| {
            for (label, what, tip) in [
                ("Load", "load images", "Read the files and show the pictures"),
                ("Unload", "unload images", "Keep them attached, but show only their frames"),
                ("Detach", "detach images", "Remove them and their pictures"),
            ] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    *action = Some((what, String::new()));
                }
            }
            ui.separator();
            let tip = "Pick where its file is now; other missing images in that folder are found too";
            match one {
                Some(name) => {
                    if ui.button("Browse...").on_hover_text(tip).clicked() {
                        *action = Some(("browse image", name));
                    }
                }
                None => {
                    let tip = "Pick the folder their files are in now";
                    if ui.button("Find in Folder...").on_hover_text(tip).clicked() {
                        *action = Some(("find images", String::new()));
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_and_resolving() {
        let sep = std::path::MAIN_SEPARATOR_STR;
        let rel = relative(Path::new("/jobs/plan"), Path::new("/jobs/survey/site.dwg")).unwrap();
        assert_eq!(rel, ["..", "survey", "site.dwg"].join(sep));
        let dir = std::env::temp_dir().join(format!("oxidraft-xref-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("survey")).unwrap();
        std::fs::write(dir.join("survey").join("site.dxf"), "").unwrap();
        let host = dir.join("plan.dxf");
        assert_eq!(resolve(Some(&host), "survey\\site.dxf"), Some(dir.join("survey/site.dxf")));
        // A path from another computer: found by name next to the drawing, if it's there.
        assert_eq!(resolve(Some(&host), "C:\\jobs\\survey\\site.dxf"), None);
        std::fs::write(dir.join("site.dxf"), "").unwrap();
        assert_eq!(resolve(Some(&host), "C:\\jobs\\survey\\site.dxf"), Some(dir.join("site.dxf")));
        std::fs::remove_dir_all(&dir).ok();
    }
}

//! SHEETSET: the Sheet Set Manager. A sheet set is a list of sheets —
//! layouts of drawings, each with its number and title — kept in a sheet
//! set file (.oxss, JSON) beside them. The palette adds a drawing's
//! layouts, removes and reorders sheets, opens one's drawing in a tab,
//! publishes the set to one PDF, and ARCHIVE copies the set with its
//! drawings and the files they use into a folder.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_plot::{PlotArea, PlotSettings, Sheet};
use serde::{Deserialize, Serialize};

use crate::app::OxidraftApp;
use crate::theme;

/// A sheet: a layout of a drawing (its path relative to the set's file).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetRef {
    pub number: String,
    pub title: String,
    pub drawing: String,
    pub layout: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetSet {
    pub name: String,
    pub description: String,
    pub sheets: Vec<SheetRef>,
}

/// The Sheet Set Manager: the set open, where its file is, the sheet picked.
pub(crate) struct SheetSetPalette {
    pub(crate) path: Option<PathBuf>,
    pub(crate) set: SheetSet,
    selected: Option<usize>,
}

impl SheetSet {
    pub fn load(path: &Path) -> Result<SheetSet, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{} isn't a sheet set: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("Could not write {}: {e}", path.display()))
    }
}

/// `file` as the set at `set` names it: relative to its folder when it's in it.
fn relative(set: &Path, file: &Path) -> String {
    let dir = set.parent().unwrap_or(Path::new(""));
    file.strip_prefix(dir).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

/// Where a sheet's drawing is.
fn drawing_path(set: Option<&Path>, sheet: &SheetRef) -> PathBuf {
    let p = PathBuf::from(&sheet.drawing);
    match set.and_then(Path::parent) {
        Some(dir) if p.is_relative() => dir.join(p),
        _ => p,
    }
}

impl OxidraftApp {
    pub(crate) fn toggle_sheet_set(&mut self) {
        if self.sheet_set.take().is_none() {
            self.sheet_set = Some(SheetSetPalette { path: None, set: SheetSet::default(), selected: None });
        }
    }

    /// Opens the sheet set file at `path` in the Sheet Set Manager.
    pub fn open_sheet_set(&mut self, path: &Path) -> Result<(), String> {
        let set = SheetSet::load(path)?;
        self.sheet_set = Some(SheetSetPalette { path: Some(path.to_path_buf()), set, selected: None });
        Ok(())
    }

    /// A new sheet set named `name`, kept at `path`.
    pub fn new_sheet_set(&mut self, path: &Path, name: &str) -> Result<(), String> {
        let set = SheetSet { name: name.into(), ..Default::default() };
        set.save(path)?;
        self.sheet_set = Some(SheetSetPalette { path: Some(path.to_path_buf()), set, selected: None });
        Ok(())
    }

    /// Adds `drawing`'s layouts to the set as sheets, numbered on from its last.
    pub fn add_sheets_from(&mut self, drawing: &Path) -> Result<usize, String> {
        let doc = crate::xref::read_drawing(drawing)?;
        let p = self.sheet_set.as_mut().ok_or("No sheet set is open.")?;
        let set_path = p.path.clone().ok_or("Save the sheet set first.")?;
        let name = relative(&set_path, drawing);
        let mut added = 0;
        for l in doc.layouts() {
            let number = format!("{:02}", p.set.sheets.len() + 1);
            p.set.sheets.push(SheetRef {
                number,
                title: l.name.clone(),
                drawing: name.clone(),
                layout: l.name.clone(),
            });
            added += 1;
        }
        p.set.save(&set_path)?;
        Ok(added)
    }

    /// The set's sheets plotted, each on its layout's page setup.
    fn sheet_set_sheets(&self) -> Result<Vec<Sheet>, String> {
        let p = self.sheet_set.as_ref().ok_or("No sheet set is open.")?;
        let settings = PlotSettings { area: PlotArea::Layout, ..self.plot_settings };
        let mut sheets = Vec::new();
        for s in &p.set.sheets {
            let file = drawing_path(p.path.as_deref(), s);
            // This drawing as it is now, else its file.
            let read;
            let doc = if self.path.as_deref() == Some(file.as_path()) {
                &self.doc
            } else {
                read = crate::xref::read_drawing(&file)?;
                &read
            };
            let layout = doc
                .layouts()
                .into_iter()
                .find(|l| l.name.eq_ignore_ascii_case(&s.layout))
                .ok_or_else(|| format!("{} has no layout \"{}\".", file.display(), s.layout))?
                .id;
            sheets.extend(oxidraft_plot::render(doc, layout, &settings));
        }
        Ok(sheets)
    }

    /// Publishes the set's sheets to one PDF at `path`; how many pages.
    pub fn publish_sheet_set(&mut self, path: &Path) -> Result<usize, String> {
        let sheets = self.sheet_set_sheets()?;
        if sheets.is_empty() {
            return Err("The sheet set has no sheets to publish.".into());
        }
        let name = self.sheet_set.as_ref().map(|p| p.set.name.clone()).unwrap_or_default();
        std::fs::write(path, oxidraft_plot::pdf_pages(&sheets, &name))
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(sheets.len())
    }

    /// ARCHIVE: the set's file, its drawings and the files they use
    /// (attached drawings, images, PDFs) copied into `folder`; how many
    /// files.
    pub fn archive_sheet_set(&mut self, folder: &Path) -> Result<usize, String> {
        let p = self.sheet_set.as_ref().ok_or("No sheet set is open.")?;
        let set_path = p.path.clone().ok_or("Save the sheet set first.")?;
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let mut copied: Vec<PathBuf> = Vec::new();
        let mut report = vec![format!("Archive of sheet set {}", p.set.name), String::new()];
        let mut copy = |from: &Path, to: &Path, report: &mut Vec<String>| -> Result<(), String> {
            if copied.iter().any(|c| c == from) {
                return Ok(());
            }
            if let Some(dir) = to.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::copy(from, to).map_err(|e| format!("{}: {e}", from.display()))?;
            report.push(format!("  {}", relative(&folder.join("x"), to)));
            copied.push(from.to_path_buf());
            Ok(())
        };
        copy(&set_path, &folder.join(set_path.file_name().unwrap_or_default()), &mut report)?;
        let drawings: Vec<String> = {
            let mut d: Vec<String> = p.set.sheets.iter().map(|s| s.drawing.clone()).collect();
            d.dedup();
            d
        };
        for name in drawings {
            let file = drawing_path(Some(&set_path), &SheetRef { drawing: name.clone(), ..Default::default() });
            copy(&file, &folder.join(&name), &mut report)?;
            // What it uses, beside it in the archive as it was beside it.
            let doc = crate::xref::read_drawing(&file)?;
            let saved: Vec<String> =
                doc.xrefs().map(|x| x.path.clone()).chain(doc.image_defs().map(|d| d.path.clone())).collect();
            for s in saved {
                if let Some(found) = crate::xref::resolve(Some(&file), &s) {
                    let dir = file.parent().unwrap_or(Path::new(""));
                    let rel = found
                        .strip_prefix(dir)
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|_| PathBuf::from(found.file_name().unwrap_or_default()));
                    let to = folder.join(&name).parent().unwrap_or(folder).join(rel);
                    copy(&found, &to, &mut report)?;
                }
            }
        }
        let n = copied.len();
        report.push(String::new());
        report.push(format!("{n} file(s)."));
        std::fs::write(folder.join("archive report.txt"), report.join("\n")).map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// ARCHIVE: picks the folder the set's archive goes to.
    pub(crate) fn archive_command(&mut self) {
        if self.sheet_set.as_ref().and_then(|p| p.path.as_ref()).is_none() {
            self.engine.log("Open a sheet set in the Sheet Set Manager (SHEETSET) first.");
            self.toggle_sheet_set();
            return;
        }
        if let Some(folder) = rfd::FileDialog::new().set_title("Archive a Sheet Set: folder").pick_folder() {
            match self.archive_sheet_set(&folder) {
                Ok(n) => self.engine.log(format!("Sheet set archived: {n} file(s) in {}", folder.display())),
                Err(e) => self.engine.log(e),
            }
        }
    }

    pub(crate) fn sheet_set_palette(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.sheet_set.take() else { return };
        let mut open = true;
        let (mut new, mut load, mut add, mut publish, mut archive, mut open_drawing) =
            (false, false, false, false, false, None);
        let mut changed = false;
        egui::Window::new("SHEET SET MANAGER")
            .id(egui::Id::unique("sheet_set"))
            .open(&mut open)
            .collapsible(true)
            .resizable(true)
            .default_pos([60.0, 200.0])
            .default_width(300.0)
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    new = ui.button("New Sheet Set...").clicked();
                    load = ui.button("Open...").clicked();
                });
                if p.path.is_none() {
                    ui.colored_label(theme::text_dim(), "Make a sheet set, or open one (.oxss).");
                    return;
                }
                ui.label(RichText::new(&p.set.name).strong());
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (i, s) in p.set.sheets.iter().enumerate() {
                        let label = format!("{} - {}", s.number, s.title);
                        let r = ui.selectable_label(p.selected == Some(i), &label);
                        let r = r.on_hover_text(format!("{} / {}", s.drawing, s.layout));
                        if r.clicked() {
                            p.selected = Some(i);
                        }
                        if r.double_clicked() {
                            open_drawing = Some(i);
                        }
                    }
                });
                if let Some(i) = p.selected.filter(|i| *i < p.set.sheets.len()) {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Number:");
                        let r = ui.add(egui::TextEdit::singleline(&mut p.set.sheets[i].number).desired_width(50.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Sheet number"));
                        changed |= r.changed();
                        ui.label("Title:");
                        let r = ui.add(egui::TextEdit::singleline(&mut p.set.sheets[i].title).desired_width(150.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Sheet title"));
                        changed |= r.changed();
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() {
                            open_drawing = Some(i);
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("Move Up")).clicked() {
                            p.set.sheets.swap(i, i - 1);
                            p.selected = Some(i - 1);
                            changed = true;
                        }
                        if ui.add_enabled(i + 1 < p.set.sheets.len(), egui::Button::new("Move Down")).clicked() {
                            p.set.sheets.swap(i, i + 1);
                            p.selected = Some(i + 1);
                            changed = true;
                        }
                        if ui.button("Remove").clicked() {
                            p.set.sheets.remove(i);
                            p.selected = None;
                            changed = true;
                        }
                    });
                }
                ui.separator();
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
                    add = ui.button("Add Sheets...").on_hover_text("Adds a drawing's layouts").clicked();
                    publish = ui.button("Publish to PDF...").clicked();
                    archive = ui.button("Archive...").clicked();
                });
            });
        if changed
            && let Some(path) = &p.path
            && let Err(e) = p.set.save(path)
        {
            self.engine.log(e);
        }
        let sheet = open_drawing.and_then(|i| p.set.sheets.get(i)).map(|s| drawing_path(p.path.as_deref(), s));
        self.sheet_set = open.then_some(p);
        if new
            && let Some(path) =
                rfd::FileDialog::new().add_filter("Sheet set", &["oxss"]).set_file_name("Sheets.oxss").save_file()
        {
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if let Err(e) = self.new_sheet_set(&path, &name) {
                self.engine.log(e);
            }
        }
        if load
            && let Some(path) = rfd::FileDialog::new().add_filter("Sheet set", &["oxss"]).pick_file()
            && let Err(e) = self.open_sheet_set(&path)
        {
            self.engine.log(e);
        }
        if add {
            for file in rfd::FileDialog::new().add_filter("Drawing", &["dwg", "dxf"]).pick_files().unwrap_or_default() {
                match self.add_sheets_from(&file) {
                    Ok(n) => self.engine.log(format!("{n} sheet(s) added from {}", file.display())),
                    Err(e) => self.engine.log(e),
                }
            }
        }
        if publish && let Some(path) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).save_file() {
            match self.publish_sheet_set(&path) {
                Ok(n) => self.engine.log(format!("{n} sheet(s) published to {}", path.display())),
                Err(e) => self.engine.log(e),
            }
        }
        if archive {
            self.archive_command();
        }
        if let Some(file) = sheet {
            self.tab_request = Some(crate::drawings::TabRequest::Open(file));
        }
    }
}

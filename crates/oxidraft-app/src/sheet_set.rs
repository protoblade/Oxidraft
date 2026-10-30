//! SHEETSET: the Sheet Set Manager. A sheet set is a list of sheets —
//! layouts of drawings, each with its number and title — kept in a sheet
//! set file (.oxss, JSON) beside them. The palette adds a drawing's
//! layouts, removes and reorders sheets, opens one's drawing in a tab,
//! publishes the set (or the sheets ticked, a sheet selection saved by
//! name) to one PDF, places a sheet list table, and ARCHIVE copies the set
//! with its drawings and the files they use into a folder. Custom
//! properties of the set and of each sheet show in fields
//! (`%<\AcSm SheetSet.Client>%`, `%<\AcSm Sheet.Number>%`).

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
    /// Its own custom properties: name and value.
    pub properties: Vec<(String, String)>,
}

/// Sheets kept together under a name, to publish together.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetSelection {
    pub name: String,
    /// Each sheet's drawing and layout.
    pub sheets: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetSet {
    pub name: String,
    pub description: String,
    pub sheets: Vec<SheetRef>,
    /// The set's custom properties: name and value.
    pub properties: Vec<(String, String)>,
    pub selections: Vec<SheetSelection>,
}

impl SheetSet {
    /// The sheets in selection `name`, by number in the list.
    pub fn selection(&self, name: &str) -> Vec<usize> {
        let Some(s) = self.selections.iter().find(|s| s.name == name) else { return Vec::new() };
        self.sheets
            .iter()
            .enumerate()
            .filter(|(_, r)| s.sheets.iter().any(|(d, l)| *d == r.drawing && l.eq_ignore_ascii_case(&r.layout)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Saves the sheets `ticked` as selection `name` (in place of one so named).
    pub fn save_selection(&mut self, name: &str, ticked: &[usize]) {
        let sheets =
            ticked.iter().filter_map(|i| self.sheets.get(*i)).map(|r| (r.drawing.clone(), r.layout.clone())).collect();
        self.selections.retain(|s| s.name != name);
        self.selections.push(SheetSelection { name: name.into(), sheets });
    }
}

/// The Sheet Set Manager: the set open, where its file is, the sheet picked.
pub(crate) struct SheetSetPalette {
    pub(crate) path: Option<PathBuf>,
    pub(crate) set: SheetSet,
    selected: Option<usize>,
    /// The sheets ticked to publish.
    ticked: Vec<usize>,
    /// The name a new sheet selection gets.
    selection_name: String,
}

impl SheetSetPalette {
    fn new(path: Option<PathBuf>, set: SheetSet) -> Self {
        Self { path, set, selected: None, ticked: Vec::new(), selection_name: String::new() }
    }
}

/// Custom properties, editable: name and value, removable, one to add.
fn property_rows(ui: &mut egui::Ui, id: &str, properties: &mut Vec<(String, String)>) -> bool {
    let mut changed = false;
    let mut remove = None;
    egui::Grid::new(id).num_columns(3).spacing([4.0, 2.0]).show(ui, |ui| {
        for (i, (name, value)) in properties.iter_mut().enumerate() {
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(90.0));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Property name"));
            changed |= r.changed();
            let r = ui.add(egui::TextEdit::singleline(value).desired_width(130.0));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Property value"));
            changed |= r.changed();
            if ui.small_button("✕").on_hover_text("Remove the property").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        properties.remove(i);
        changed = true;
    }
    if ui.small_button("Add Property").clicked() {
        properties.push((format!("Property{}", properties.len() + 1), String::new()));
        changed = true;
    }
    changed
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
            self.sheet_set = Some(SheetSetPalette::new(None, SheetSet::default()));
        }
    }

    /// Opens the sheet set file at `path` in the Sheet Set Manager.
    pub fn open_sheet_set(&mut self, path: &Path) -> Result<(), String> {
        let set = SheetSet::load(path)?;
        self.sheet_set = Some(SheetSetPalette::new(Some(path.to_path_buf()), set));
        Ok(())
    }

    /// A new sheet set named `name`, kept at `path`.
    pub fn new_sheet_set(&mut self, path: &Path, name: &str) -> Result<(), String> {
        let set = SheetSet { name: name.into(), ..Default::default() };
        set.save(path)?;
        self.sheet_set = Some(SheetSetPalette::new(Some(path.to_path_buf()), set));
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
                properties: Vec::new(),
            });
            added += 1;
        }
        p.set.save(&set_path)?;
        Ok(added)
    }

    /// The sheet set open in the Sheet Set Manager.
    pub fn sheet_set_mut(&mut self) -> Option<&mut SheetSet> {
        self.sheet_set.as_mut().map(|p| &mut p.set)
    }

    /// Ticks sheets (by number in the list) for publishing; none: all.
    pub fn tick_sheets(&mut self, which: &[usize]) {
        if let Some(p) = &mut self.sheet_set {
            p.ticked = which.to_vec();
        }
    }

    /// Keeps the sheets ticked as sheet selection `name`, in the set's file.
    pub fn save_sheet_selection(&mut self, name: &str) -> Result<(), String> {
        let p = self.sheet_set.as_mut().ok_or("No sheet set is open.")?;
        let ticked = p.ticked.clone();
        p.set.save_selection(name, &ticked);
        match &p.path {
            Some(path) => p.set.save(path),
            None => Ok(()),
        }
    }

    /// Ticks the sheets of selection `name`.
    pub fn choose_sheet_selection(&mut self, name: &str) {
        if let Some(p) = &mut self.sheet_set {
            p.ticked = p.set.selection(name);
        }
    }

    /// The set's sheet set fields: its Name, Description and properties.
    pub(crate) fn sheet_set_fields(&self) -> Vec<(String, String)> {
        let Some(p) = &self.sheet_set else { return Vec::new() };
        let set = &p.set;
        [("Name".to_string(), set.name.clone()), ("Description".to_string(), set.description.clone())]
            .into_iter()
            .chain(set.properties.iter().cloned())
            .collect()
    }

    /// The sheet fields of the sheet this drawing's layout `space` is.
    pub(crate) fn sheet_fields(&self, space: oxidraft_model::Space) -> Vec<(String, String)> {
        let (Some(p), Some(file), Some(layout)) = (&self.sheet_set, &self.path, self.doc.layout(space)) else {
            return Vec::new();
        };
        let sheet = p
            .set
            .sheets
            .iter()
            .find(|s| drawing_path(p.path.as_deref(), s) == *file && s.layout.eq_ignore_ascii_case(&layout.name));
        sheet.map_or_else(Vec::new, |s| {
            [("Number".to_string(), s.number.clone()), ("Title".to_string(), s.title.clone())]
                .into_iter()
                .chain(s.properties.iter().cloned())
                .collect()
        })
    }

    /// The set's sheets plotted, each on its layout's page setup: those
    /// ticked, or all when none are.
    fn sheet_set_sheets(&self) -> Result<Vec<Sheet>, String> {
        let p = self.sheet_set.as_ref().ok_or("No sheet set is open.")?;
        let settings = PlotSettings { area: PlotArea::Layout, ..self.plot_settings };
        let mut sheets = Vec::new();
        let env = oxidraft_model::field::env();
        for (i, s) in p.set.sheets.iter().enumerate() {
            if !p.ticked.is_empty() && !p.ticked.contains(&i) {
                continue;
            }
            // Its sheet fields while it plots.
            let fields = [("Number".to_string(), s.number.clone()), ("Title".to_string(), s.title.clone())];
            oxidraft_model::field::set_env(oxidraft_model::field::FieldEnv {
                sheet_set: self.sheet_set_fields(),
                sheet: fields.into_iter().chain(s.properties.iter().cloned()).collect(),
                ..env.clone()
            });
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
        oxidraft_model::field::set_env(env);
        Ok(sheets)
    }

    /// The Sheet List Table: the set's sheets' numbers and titles, as a
    /// table to place.
    pub fn insert_sheet_list(&mut self) -> Result<(), String> {
        let p = self.sheet_set.as_ref().ok_or("No sheet set is open.")?;
        if p.set.sheets.is_empty() {
            return Err("The sheet set has no sheets to list.".into());
        }
        let rows: Vec<Vec<String>> = std::iter::once(vec!["Sheet Number".to_string(), "Sheet Title".to_string()])
            .chain(p.set.sheets.iter().map(|s| vec![s.number.clone(), s.title.clone()]))
            .collect();
        let title = if p.set.name.is_empty() { "Sheet List".to_string() } else { format!("{} Sheet List", p.set.name) };
        let text_height = self.doc.table_style().text_height;
        let (widths, heights, cells) = crate::datalink::table_parts(&title, &rows, text_height);
        let table = oxidraft_model::Geometry::Table {
            position: oxidraft_geom::Point::ORIGIN,
            rotation: 0.0,
            widths,
            heights,
            cells,
            text_height,
            formats: Vec::new(),
        };
        self.engine.insert_table(table, &mut self.doc);
        Ok(())
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
        let mut sheet_list = false;
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
                        let mut ticked = p.ticked.contains(&i);
                        let r = ui
                            .horizontal(|ui| {
                                let t = ui.checkbox(&mut ticked, "").on_hover_text("Publish this sheet");
                                t.widget_info(|| egui::WidgetInfo::labeled(egui::Role::CheckBox, true, &label));
                                if t.changed() {
                                    if ticked {
                                        p.ticked.push(i);
                                    } else {
                                        p.ticked.retain(|x| *x != i);
                                    }
                                }
                                ui.selectable_label(p.selected == Some(i), &label)
                            })
                            .inner;
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
                            p.ticked.clear();
                            changed = true;
                        }
                    });
                    if let Some(s) = p.set.sheets.get_mut(i) {
                        egui::CollapsingHeader::new("Sheet Properties").id_salt("sheet_props").show(ui, |ui| {
                            changed |= property_rows(ui, "sheet_props_grid", &mut s.properties);
                        });
                    }
                }
                egui::CollapsingHeader::new("Sheet Set Properties").id_salt("set_props").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Description:");
                        let r = ui.text_edit_singleline(&mut p.set.description);
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Description"));
                        changed |= r.changed();
                    });
                    changed |= property_rows(ui, "set_props_grid", &mut p.set.properties);
                });
                // Sheet selections: the sheets ticked, kept by name.
                ui.horizontal(|ui| {
                    ui.label("Sheet Selections:");
                    let mut chosen = None;
                    let r =
                        egui::ComboBox::from_id_salt("sheet_selections").selected_text("Choose...").show_ui(ui, |ui| {
                            for s in &p.set.selections {
                                if ui.selectable_label(false, &s.name).clicked() {
                                    chosen = Some(s.name.clone());
                                }
                            }
                            if ui.selectable_label(false, "All sheets").clicked() {
                                chosen = Some(String::new());
                            }
                        });
                    r.response
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Sheet selections"));
                    if let Some(name) = chosen {
                        p.ticked = if name.is_empty() { Vec::new() } else { p.set.selection(&name) };
                    }
                });
                ui.horizontal(|ui| {
                    let r = ui
                        .add(egui::TextEdit::singleline(&mut p.selection_name).hint_text("Name").desired_width(120.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Selection name"));
                    let can = !p.ticked.is_empty() && !p.selection_name.trim().is_empty();
                    if ui.add_enabled(can, egui::Button::new("Save Selection")).clicked() {
                        let name = p.selection_name.trim().to_string();
                        let ticked = p.ticked.clone();
                        p.set.save_selection(&name, &ticked);
                        p.selection_name.clear();
                        changed = true;
                    }
                });
                ui.separator();
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
                    add = ui.button("Add Sheets...").on_hover_text("Adds a drawing's layouts").clicked();
                    let what = if p.ticked.is_empty() {
                        "the sheet set".to_string()
                    } else {
                        format!("{} sheet(s) ticked", p.ticked.len())
                    };
                    publish = ui.button("Publish to PDF...").on_hover_text(format!("Publishes {what}")).clicked();
                    archive = ui.button("Archive...").clicked();
                });
                sheet_list = ui.button("Insert Sheet List").on_hover_text("A table of the sheets, to place").clicked();
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
        if sheet_list && let Err(e) = self.insert_sheet_list() {
            self.engine.log(e);
        }
        if let Some(file) = sheet {
            self.tab_request = Some(crate::drawings::TabRequest::Open(file));
        }
    }
}

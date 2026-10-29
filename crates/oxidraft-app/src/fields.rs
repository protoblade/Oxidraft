//! FIELD's dialog (AutoCAD's Field dialog: a field by category and name,
//! its format, its value and its code) and the text editor TEXTEDIT and
//! double-click open, which inserts fields too.

use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Datelike, Local, Timelike};
use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::field::{self, FieldEnv, Moment};
use oxidraft_model::{EntityId, Geometry};

use crate::{app::OxidraftApp, theme};

fn moment(t: DateTime<Local>) -> Moment {
    Moment { year: t.year(), month: t.month(), day: t.day(), hour: t.hour(), minute: t.minute(), second: t.second() }
}

fn file_moment(t: std::io::Result<SystemTime>) -> Option<Moment> {
    t.ok().map(|t| moment(DateTime::<Local>::from(t)))
}

/// When the drawing's file was made and last saved.
#[derive(Default, Clone, Copy)]
pub(crate) struct FileTimes {
    created: Option<Moment>,
    saved: Option<Moment>,
}

impl FileTimes {
    pub(crate) fn of(path: &Path) -> FileTimes {
        match std::fs::metadata(path) {
            Ok(m) => FileTimes { created: file_moment(m.created()), saved: file_moment(m.modified()) },
            Err(_) => FileTimes::default(),
        }
    }
}

/// The fields, by category.
const FIELDS: [(&str, &str); 12] = [
    ("Author", "Document"),
    ("Comments", "Document"),
    ("CreateDate", "Date & Time"),
    ("Date", "Date & Time"),
    ("Filename", "Document"),
    ("Keywords", "Document"),
    ("Login", "Other"),
    ("Object", "Objects"),
    ("PlotDate", "Date & Time"),
    ("SaveDate", "Date & Time"),
    ("Subject", "Document"),
    ("Title", "Document"),
];
const CATEGORIES: [&str; 5] = ["All", "Date & Time", "Document", "Objects", "Other"];
const DATE_FORMATS: [&str; 10] = [
    "M/d/yyyy",
    "dddd, MMMM d, yyyy",
    "MMMM d, yyyy",
    "d MMMM yyyy",
    "MM/dd/yy",
    "yyyy-MM-dd",
    "d-MMM-yy",
    "M/d/yyyy h:mm tt",
    "h:mm tt",
    "HH:mm",
];
const CASES: [&str; 5] = ["(none)", "Uppercase", "Lowercase", "First capital", "Title case"];
const FILE_PARTS: [&str; 3] = ["Filename only", "Path only", "Path and filename"];
const UNITS: [&str; 6] = ["Current units", "Scientific", "Decimal", "Engineering", "Architectural", "Fractional"];

fn is_date(name: &str) -> bool {
    name.ends_with("Date")
}

/// FIELD: the field being chosen, and where it goes.
pub(crate) struct FieldDialog {
    category: &'static str,
    name: &'static str,
    date_format: String,
    case: u32,
    file_part: usize,
    extension: bool,
    object: Option<EntityId>,
    property: String,
    units: u32,
    precision: u32,
    /// Into the text editor, rather than as text of its own.
    into_editor: bool,
    /// Hidden while the object is picked (SELECT).
    picking: bool,
}

impl FieldDialog {
    fn new(into_editor: bool) -> FieldDialog {
        FieldDialog {
            category: "All",
            name: "Date",
            date_format: DATE_FORMATS[0].into(),
            case: 0,
            file_part: 0,
            extension: true,
            object: None,
            property: String::new(),
            units: 0,
            precision: 2,
            into_editor,
            picking: false,
        }
    }

    /// The field's code, once there's all it needs.
    fn code(&self) -> Option<String> {
        let case = if self.case > 0 { format!(" \\f \"%tc{}\"", self.case) } else { String::new() };
        Some(match self.name {
            name if is_date(name) => format!("%<\\AcVar {name} \\f \"{}\">%", self.date_format.trim()),
            "Filename" => {
                let bits = [2, 1, 3][self.file_part] | if self.extension && self.file_part != 1 { 4 } else { 0 };
                format!("%<\\AcVar Filename \\f \"%fn{bits}\">%")
            }
            "Object" => {
                let format = match self.property.as_str() {
                    "Layer" | "TextString" => {
                        if self.case > 0 {
                            format!("%tc{}", self.case)
                        } else {
                            String::new()
                        }
                    }
                    _ if self.units > 0 => format!("%lu{}%pr{}", self.units, self.precision),
                    _ => format!("%pr{}", self.precision),
                };
                field::object_code(self.object?, &self.property, &format)
            }
            name => format!("%<\\AcVar {name}{case}>%"),
        })
    }
}

/// TEXTEDIT: a text's content being edited, paragraphs as lines.
pub(crate) struct TextEditor {
    id: EntityId,
    text: String,
    /// Where the cursor was (in characters), for Insert Field.
    cursor: Option<usize>,
}

impl OxidraftApp {
    /// What fields show for the time, the file and the user, now.
    pub(crate) fn field_env(&self) -> FieldEnv {
        let path = self.path.as_ref().map_or_else(|| self.file_name(), |p| p.display().to_string());
        let login = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
        FieldEnv {
            now: moment(Local::now()),
            path,
            created: self.file_times.created,
            saved: self.file_times.saved,
            plotted: None,
            login,
        }
    }

    /// After a save: the drawing was saved just now.
    pub(crate) fn just_saved(&mut self) {
        self.file_times.saved = Some(moment(Local::now()));
        if self.file_times.created.is_none() {
            self.file_times.created = self.file_times.saved;
        }
    }

    pub(crate) fn open_field_dialog(&mut self, into_editor: bool) {
        self.field_dialog = Some(FieldDialog::new(into_editor));
    }

    pub(crate) fn field_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.field_dialog.take() else { return };
        if d.picking {
            // Back once SELECT is done, with the object picked.
            if !self.engine.is_active() {
                d.picking = false;
                if let Some(id) = self.engine.previous_selection().first() {
                    d.object = Some(*id);
                    let props = self.doc.entity(*id).map(|e| field::object_properties(&e.geometry)).unwrap_or_default();
                    d.property = props.first().map(|p| p.to_string()).unwrap_or_default();
                }
            }
            self.field_dialog = Some(d);
            return;
        }
        let (mut open, mut ok, mut cancel, mut select) = (true, false, false, false);
        let mut text_fields = Vec::new();
        let preview = d.code().map(|c| field::evaluate(&c, &self.doc));
        let object_kind = d.object.and_then(|id| self.doc.entity(id)).map(|e| e.geometry.clone());
        egui::Window::new("Field")
            .id(egui::Id::unique("field_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 140.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        ui.label("Field category:");
                        let combo =
                            egui::ComboBox::from_id_salt("field_category").width(160.0).selected_text(d.category);
                        combo
                            .show_ui(ui, |ui| {
                                for c in CATEGORIES {
                                    ui.selectable_value(&mut d.category, c, c);
                                }
                            })
                            .response
                            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Field category"));
                        ui.label("Field names:");
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(vec2(150.0, 200.0));
                            for (name, category) in FIELDS {
                                if (d.category == "All" || d.category == category)
                                    && ui.selectable_label(d.name == name, name).clicked()
                                {
                                    d.name = name;
                                }
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        ui.label(RichText::new("Field value:").strong());
                        let shown = preview.clone().unwrap_or_else(|| "Select an object".into());
                        ui.label(RichText::new(shown).monospace());
                        ui.add_space(6.0);
                        if is_date(d.name) {
                            ui.label("Date format:");
                            let r = ui.add(egui::TextEdit::singleline(&mut d.date_format).desired_width(220.0));
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Date format"));
                            text_fields.push(r.id);
                            ui.label("Examples:");
                            let now = moment(Local::now());
                            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                                for f in DATE_FORMATS {
                                    let picked = d.date_format == f;
                                    if ui.selectable_label(picked, field::format_date(now, f)).clicked() {
                                        d.date_format = f.into();
                                    }
                                }
                            });
                        } else if d.name == "Filename" {
                            for (i, part) in FILE_PARTS.iter().enumerate() {
                                ui.radio_value(&mut d.file_part, i, *part);
                            }
                            ui.add_enabled(
                                d.file_part != 1,
                                egui::Checkbox::new(&mut d.extension, "Display file extension"),
                            );
                        } else if d.name == "Object" {
                            ui.horizontal(|ui| {
                                let kind = object_kind.as_ref().map_or("(none)", Geometry::name);
                                ui.label(format!("Object type: {kind}"));
                                select = ui.button("Select object").clicked();
                            });
                            if let Some(g) = &object_kind {
                                ui.label("Property:");
                                for p in field::object_properties(g) {
                                    if ui.selectable_label(d.property == p, p).clicked() {
                                        d.property = p.into();
                                    }
                                }
                                if matches!(d.property.as_str(), "Layer" | "TextString") {
                                    case_list(ui, &mut d.case);
                                } else {
                                    ui.horizontal(|ui| {
                                        ui.label("Format:");
                                        egui::ComboBox::from_id_salt("field_units")
                                            .selected_text(UNITS[d.units as usize])
                                            .show_ui(ui, |ui| {
                                                for (i, u) in UNITS.iter().enumerate() {
                                                    ui.selectable_value(&mut d.units, i as u32, *u);
                                                }
                                            });
                                        ui.label("Precision:");
                                        egui::ComboBox::from_id_salt("field_precision")
                                            .width(40.0)
                                            .selected_text(d.precision.to_string())
                                            .show_ui(ui, |ui| {
                                                for p in 0..=8 {
                                                    ui.selectable_value(&mut d.precision, p, p.to_string());
                                                }
                                            });
                                    });
                                }
                            }
                        } else {
                            case_list(ui, &mut d.case);
                        }
                    });
                });
                ui.separator();
                ui.label("Field expression:");
                ui.label(RichText::new(d.code().unwrap_or_default()).monospace().color(theme::text_dim()));
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.add_enabled(d.code().is_some(), egui::Button::new("OK")).clicked();
                });
            });
        self.text_fields.extend(text_fields);
        if select {
            d.picking = true;
            self.run("SELECT");
            self.field_dialog = Some(d);
            return;
        }
        if ok && let Some(code) = d.code() {
            if d.into_editor {
                if let Some(ed) = &mut self.text_editor {
                    let at = ed.cursor.unwrap_or(ed.text.chars().count());
                    let byte = ed.text.char_indices().nth(at).map_or(ed.text.len(), |(i, _)| i);
                    ed.text.insert_str(byte, &code);
                    ed.cursor = Some(at + code.chars().count());
                }
            } else {
                // FIELD asks where its text goes.
                self.engine.run("FIELD", &mut self.doc);
                let tolerance = self.pick_tolerance();
                self.engine.submit(&code, &mut self.doc, oxidraft_geom::Point::ORIGIN, tolerance);
            }
            return;
        }
        if open && !cancel {
            self.field_dialog = Some(d);
        }
    }

    /// TEXTEDIT: opens a text in the editor.
    pub(crate) fn open_text_editor(&mut self, id: EntityId) {
        if let Some(Geometry::Text { content, .. }) = self.doc.entity(id).map(|e| &e.geometry) {
            let text = content.replace("\\P", "\n");
            self.text_editor = Some(TextEditor { id, text, cursor: None });
        }
    }

    pub(crate) fn text_editor(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.text_editor.take() else { return };
        if self.doc.entity(ed.id).is_none() {
            return;
        }
        let (mut open, mut ok, mut cancel, mut insert) = (true, false, false, false);
        let mut text_fields = Vec::new();
        let shown = field::has_fields(&ed.text).then(|| field::evaluate(&ed.text, &self.doc));
        egui::Window::new("Text Editor")
            .id(egui::Id::unique("text_editor"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([360.0, 180.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                let output = egui::TextEdit::multiline(&mut ed.text).desired_width(420.0).desired_rows(5).show(ui);
                output.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Text"));
                if let Some(r) = output.cursor_range {
                    ed.cursor = Some(r.primary.index.0);
                }
                text_fields.push(output.response.id);
                if let Some(s) = &shown {
                    ui.colored_label(theme::text_dim(), format!("Shows: {}", s.replace("\\P", " ")));
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
                    insert = ui
                        .button("Insert Field...")
                        .on_hover_text("A field: the date, a property, an object's area")
                        .clicked();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        cancel = ui.button("Cancel").clicked();
                        ok = ui.button("OK").clicked();
                    });
                });
            });
        self.text_fields.extend(text_fields);
        if insert {
            self.open_field_dialog(true);
        }
        if ok {
            let content = ed.text.replace("\r\n", "\n").replace('\n', "\\P");
            self.doc.begin("TEXTEDIT");
            self.doc.update(ed.id, |e| {
                if let Geometry::Text { content: c, .. } = &mut e.geometry {
                    *c = content;
                }
            });
            self.doc.commit();
            self.field_dialog = None;
            return;
        }
        if open && !cancel {
            self.text_editor = Some(ed);
        } else {
            self.field_dialog = None;
        }
    }
}

/// Text fields' case formats.
fn case_list(ui: &mut egui::Ui, case: &mut u32) {
    ui.label("Format:");
    for (i, c) in CASES.iter().enumerate() {
        ui.selectable_value(case, i as u32, *c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_as_autocad_writes_them() {
        let mut d = FieldDialog::new(false);
        assert_eq!(d.code().unwrap(), r#"%<\AcVar Date \f "M/d/yyyy">%"#);
        d.name = "Filename";
        assert_eq!(d.code().unwrap(), r#"%<\AcVar Filename \f "%fn6">%"#);
        d.extension = false;
        d.file_part = 2;
        assert_eq!(d.code().unwrap(), r#"%<\AcVar Filename \f "%fn3">%"#);
        d.name = "Title";
        d.case = 1;
        assert_eq!(d.code().unwrap(), r#"%<\AcVar Title \f "%tc1">%"#);
        d.name = "Object";
        assert_eq!(d.code(), None);
        d.object = Some(EntityId(7));
        d.property = "Area".into();
        d.units = 2;
        assert_eq!(d.code().unwrap(), r#"%<\AcObjProp Object(%<\_ObjId 7>%).Area \f "%lu2%pr2">%"#);
    }
}

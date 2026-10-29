//! Closing a drawing with care, and the file commands around it: NEW,
//! OPEN, CLOSE and QUIT ask to save changes first (the window's close
//! button too); RECOVER opens a drawing and fixes what AUDIT finds;
//! SCRIPT runs a script's commands; DWGPROPS edits the drawing's
//! properties.

use std::path::Path;

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::DrawingProperties;

use crate::app::OxidraftApp;

/// What happens once the drawing's changes are saved or let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Then {
    New,
    Open,
    Recover,
    Close,
    Quit,
}

impl OxidraftApp {
    /// Does `then`, after asking to save the drawing if it has changes.
    pub(crate) fn after_saving(&mut self, then: Then) {
        if self.is_dirty() {
            self.save_prompt = Some(then);
        } else {
            self.proceed(then);
        }
    }

    fn proceed(&mut self, then: Then) {
        match then {
            Then::New | Then::Close => self.new_drawing(),
            Then::Open => self.open_dialog(),
            Then::Recover => self.recover_dialog(),
            Then::Quit => self.quit_now = true,
        }
    }

    /// "Save changes to …?": Yes, No or Cancel.
    pub(crate) fn save_prompt_dialog(&mut self, ctx: &egui::Context) {
        // The window's close button asks too.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_now && self.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.save_prompt = Some(Then::Quit);
        }
        if self.quit_now {
            self.quit_now = false;
            self.saved_revision = self.doc.revision();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let Some(then) = self.save_prompt else { return };
        let (mut yes, mut no, mut cancel) = (false, false, false);
        egui::Window::new("Oxidraft")
            .id(egui::Id::unique("save_prompt"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -60.0))
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("Save changes to {}?", self.file_name())).strong());
                let row = vec2(260.0, 26.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    no = ui.button("No").clicked();
                    yes = ui.button("Yes").clicked();
                });
            });
        if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.save_prompt = None;
        } else if yes {
            self.save_prompt = None;
            self.save(false);
            // Saved (not cancelled in the file dialog): go on.
            if !self.is_dirty() {
                self.proceed(then);
            }
        } else if no {
            self.save_prompt = None;
            self.proceed(then);
        }
    }

    /// RECOVER: a drawing opened and AUDIT's fixes made.
    fn recover_dialog(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Drawing (DWG, DXF)", &["dwg", "dxf"]);
        if let Some(path) = dialog.pick_file() {
            self.recover_path(&path);
        }
    }

    /// Opens `path` and fixes what AUDIT finds, as RECOVER does.
    pub fn recover_path(&mut self, path: &Path) {
        self.open_path(path.to_path_buf());
        if self.path.as_deref() != Some(path) {
            return;
        }
        let found = self.doc.audit(true);
        for line in &found {
            self.engine.log(line.clone());
        }
        self.engine.log(format!("Total errors found {} fixed {}", found.len(), found.len()));
    }

    /// SCRIPT (SCR): a script's commands, run as typed: each space or line
    /// end is Enter, an empty line too.
    pub(crate) fn script_dialog(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Script", &["scr"]);
        if let Some(path) = dialog.pick_file() {
            match std::fs::read_to_string(&path) {
                Ok(text) => self.run_script(&text),
                Err(e) => self.engine.log(format!("Could not read {}: {e}", path.display())),
            }
        }
    }

    pub fn run_script(&mut self, script: &str) {
        let mut word = String::new();
        let mut quoted = false;
        for c in script.chars().filter(|c| *c != '\r') {
            match c {
                '"' => quoted = !quoted,
                ' ' | '\n' if !quoted => {
                    self.type_command(&word);
                    word.clear();
                }
                c => word.push(c),
            }
        }
        if !word.is_empty() {
            self.type_command(&word);
        }
    }

    pub(crate) fn open_drawing_properties(&mut self) {
        self.properties_dialog = Some(self.doc.properties().clone());
    }

    /// DWGPROPS: title, subject, author, keywords and comments.
    pub(crate) fn drawing_properties_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.properties_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new(format!("{} Properties", self.file_name()))
            .id(egui::Id::unique("dwgprops"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                egui::Grid::new("dwgprops_fields").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    for (label, value) in [
                        ("Title:", &mut p.title),
                        ("Subject:", &mut p.subject),
                        ("Author:", &mut p.author),
                        ("Keywords:", &mut p.keywords),
                    ] {
                        ui.label(label);
                        let r = ui.add(egui::TextEdit::singleline(value).desired_width(280.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                        fields.push(r.id);
                        ui.end_row();
                    }
                    ui.label("Comments:");
                    let r = ui.add(egui::TextEdit::multiline(&mut p.comments).desired_width(280.0).desired_rows(3));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Comments:"));
                    fields.push(r.id);
                    ui.end_row();
                });
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            self.doc.begin("DWGPROPS");
            self.doc.set_properties(p);
            self.doc.commit();
            return;
        }
        if open && !cancel {
            self.properties_dialog = Some(p);
        }
    }
}

/// So the app's field reads plainly.
pub(crate) type PropertiesDialog = DrawingProperties;

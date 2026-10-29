//! LAYERSTATE (LAS): the Layer States Manager — named snapshots of the
//! layers' settings, made, restored, renamed and deleted.

use eframe::egui::{self, Align, Layout, RichText, vec2};

use crate::{app::OxidraftApp, theme};

#[derive(Default)]
pub(crate) struct LayerStatesDialog {
    selected: Option<String>,
    /// A new state's name and description being typed.
    new: Option<(String, String)>,
    /// The selected state's new name being typed.
    renaming: Option<String>,
    error: Option<String>,
}

impl OxidraftApp {
    pub(crate) fn open_layer_states(&mut self) {
        self.layer_states_dialog = Some(LayerStatesDialog::default());
    }

    pub(crate) fn layer_states_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.layer_states_dialog.take() else { return };
        let (mut open, mut close) = (true, false);
        let (mut save, mut restore, mut delete, mut new_ok, mut rename_ok) = (false, false, false, false, false);
        let mut fields = Vec::new();
        let states = self.doc.layer_states().to_vec();
        egui::Window::new("Layer States Manager")
            .id(egui::Id::unique("layer_states"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([400.0, 150.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_min_size(vec2(320.0, 160.0));
                        egui::Grid::new("layer_states_list").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                            ui.label(RichText::new("Name").strong());
                            ui.label(RichText::new("Description").strong());
                            ui.end_row();
                            for s in &states {
                                let picked = d.selected.as_ref().is_some_and(|n| n == &s.name);
                                if ui.selectable_label(picked, &s.name).clicked() {
                                    d.selected = Some(s.name.clone());
                                    d.renaming = None;
                                }
                                ui.label(&s.description);
                                ui.end_row();
                            }
                        });
                        if states.is_empty() {
                            ui.colored_label(theme::text_dim(), "No layer states saved yet.");
                        }
                    });
                    ui.vertical(|ui| {
                        let size = vec2(90.0, 22.0);
                        let picked = d.selected.is_some();
                        if ui.add_sized(size, egui::Button::new("New...")).clicked() {
                            d.new = Some((String::new(), String::new()));
                        }
                        save = ui.add_enabled(picked, egui::Button::new("Save").min_size(size)).clicked();
                        if ui.add_enabled(picked, egui::Button::new("Rename").min_size(size)).clicked() {
                            d.renaming = d.selected.clone();
                        }
                        delete = ui.add_enabled(picked, egui::Button::new("Delete").min_size(size)).clicked();
                    });
                });
                if let Some((name, description)) = &mut d.new {
                    ui.label(RichText::new("New Layer State to Save").strong());
                    egui::Grid::new("new_layer_state").num_columns(2).show(ui, |ui| {
                        for (label, value) in [("New layer state name:", name), ("Description:", description)] {
                            ui.label(label);
                            let r = ui.add(egui::TextEdit::singleline(value).desired_width(200.0));
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                            fields.push(r.id);
                            ui.end_row();
                        }
                    });
                    new_ok = ui.button("Save New").clicked();
                }
                if let Some(name) = &mut d.renaming {
                    ui.horizontal(|ui| {
                        ui.label("Rename to:");
                        let r = ui.add(egui::TextEdit::singleline(name).desired_width(200.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Rename to"));
                        fields.push(r.id);
                        rename_ok = ui.button("Done").clicked();
                    });
                }
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                    restore = ui.add_enabled(d.selected.is_some(), egui::Button::new("Restore")).clicked();
                });
            });
        self.text_fields.extend(fields);
        let run = |app: &mut OxidraftApp, f: &dyn Fn(&mut oxidraft_model::Document) -> Result<(), String>| {
            app.doc.begin("LAYERSTATE");
            let r = f(&mut app.doc);
            if r.is_ok() {
                app.doc.commit()
            } else {
                app.doc.rollback()
            }
            r.err()
        };
        if new_ok && let Some((name, description)) = d.new.clone() {
            d.error = run(self, &|doc| doc.save_layer_state(&name, &description));
            if d.error.is_none() {
                (d.new, d.selected) = (None, Some(name.trim().to_string()));
            }
        }
        if let Some(name) = d.selected.clone() {
            let description = states.iter().find(|s| s.name == name).map_or(String::new(), |s| s.description.clone());
            if save {
                d.error = run(self, &|doc| doc.save_layer_state(&name, &description));
            }
            if restore {
                d.error = run(self, &|doc| doc.restore_layer_state(&name));
                if d.error.is_none() {
                    self.engine.log(format!("Layer state \"{name}\" restored."));
                }
            }
            if delete {
                run(self, &|doc| {
                    doc.delete_layer_state(&name);
                    Ok(())
                });
                d.selected = None;
            }
            if rename_ok && let Some(new) = d.renaming.clone() {
                d.error = run(self, &|doc| doc.rename_layer_state(&name, &new));
                if d.error.is_none() {
                    (d.renaming, d.selected) = (None, Some(new.trim().to_string()));
                }
            }
        }
        if open && !close {
            self.layer_states_dialog = Some(d);
        }
    }
}

//! VIEW's View Manager: named views to restore, save from what's on
//! screen, or delete.

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_commands::ViewRequest;
use oxidraft_model::{MODEL, NamedView};

use crate::{app::OxidraftApp, theme};

/// The View Manager while it's open: the view picked, and a new view's
/// name being typed.
#[derive(Default)]
pub(crate) struct ViewManager {
    picked: Option<String>,
    new_name: String,
}

impl OxidraftApp {
    pub(crate) fn open_view_manager(&mut self) {
        self.view_manager = Some(ViewManager::default());
    }

    /// Shows a named view: its space, centered on its center, as tall as it is.
    pub(crate) fn restore_view(&mut self, view: &NamedView) {
        if view.space != self.shown_space() && (view.space == MODEL || self.doc.layout(view.space).is_some()) {
            self.switch_space(view.space);
        }
        self.pending_views.push(ViewRequest::Center { center: view.center, height: view.height });
    }

    /// What's on screen now, as a view called `name`.
    fn current_view(&self, name: &str) -> NamedView {
        let r = self.canvas_rect;
        let (height, width) = (self.view.units(r.height()), self.view.units(r.width()));
        NamedView { name: name.trim().into(), center: self.view.center, height, width, space: self.doc.space() }
    }

    pub(crate) fn view_manager_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.view_manager.take() else { return };
        let views: Vec<NamedView> = self.doc.views().to_vec();
        let (mut open, mut close, mut set_current, mut save, mut delete) = (true, false, false, false, false);
        let mut field = None;
        egui::Window::new("View Manager")
            .id(egui::Id::unique("view_manager"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 200.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Named views").strong());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(300.0);
                    if views.is_empty() {
                        ui.colored_label(theme::text_dim(), "None yet: save what's on screen below.");
                    }
                    for v in &views {
                        let place = self.doc.layout(v.space).map_or("Model".to_string(), |l| l.name.clone());
                        let text = format!("{}   ({place}, {:.2} x {:.2})", v.name, v.width, v.height);
                        let picked = d.picked.as_deref() == Some(v.name.as_str());
                        let row = ui.selectable_label(picked, text);
                        if row.clicked() {
                            d.picked = Some(v.name.clone());
                        }
                        if row.double_clicked() {
                            set_current = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    set_current |= ui.add_enabled(d.picked.is_some(), egui::Button::new("Set Current")).clicked();
                    delete = ui.add_enabled(d.picked.is_some(), egui::Button::new("Delete")).clicked();
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("New view name:");
                    let response = ui.add(egui::TextEdit::singleline(&mut d.new_name).desired_width(140.0));
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "New view name"));
                    field = Some(response.id);
                    let ready = !d.new_name.trim().is_empty();
                    save = ui.add_enabled(ready, egui::Button::new("Save Current")).clicked();
                });
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
        self.text_fields.extend(field);
        let picked = d.picked.as_ref().and_then(|n| views.iter().find(|v| &v.name == n)).cloned();
        if save {
            let view = self.current_view(&d.new_name);
            self.doc.begin("VIEW");
            let saved = self.doc.save_view(view.clone());
            self.doc.commit();
            match saved {
                Ok(()) => {
                    d.picked = Some(view.name);
                    d.new_name.clear();
                }
                Err(e) => self.engine.log(e),
            }
        }
        if delete && let Some(v) = &picked {
            self.doc.begin("VIEW");
            self.doc.delete_view(&v.name);
            self.doc.commit();
            d.picked = None;
        }
        if set_current && let Some(v) = &picked {
            self.restore_view(v);
            return;
        }
        if open && !close {
            self.view_manager = Some(d);
        }
    }
}

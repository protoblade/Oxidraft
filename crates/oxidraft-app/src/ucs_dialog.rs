//! UCSMAN: AutoCAD's UCS dialog — the named user coordinate systems
//! (and World) to make current, rename or delete, with each one's
//! origin and angle; and whether the UCS icon shows.

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::ucs::{Ucs, WORLD};

use crate::{app::OxidraftApp, theme};

pub(crate) struct UcsDialog {
    /// The one picked in the list: `None` for World.
    picked: Option<String>,
    rename: Option<(String, String)>,
    error: Option<String>,
}

impl OxidraftApp {
    pub(crate) fn open_ucs_dialog(&mut self) {
        let ucs = self.doc.ucs();
        let picked = self.doc.named_ucs().iter().find(|n| n.ucs == ucs).map(|n| n.name.clone());
        self.ucs_dialog = Some(UcsDialog { picked, rename: None, error: None });
    }

    pub(crate) fn ucs_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.ucs_dialog.take() else { return };
        let (mut open, mut ok, mut cancel, mut set, mut delete) = (true, false, false, false, false);
        let mut fields = Vec::new();
        let current = self.doc.ucs();
        let named: Vec<(String, Ucs)> = self.doc.named_ucs().iter().map(|n| (n.name.clone(), n.ucs)).collect();
        let mut icon = self.engine.vars.ucs_icon;
        egui::Window::new("UCS")
            .id(egui::Id::unique("ucs_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Named UCSs").strong());
                ui.label(format!("Current UCS:  {}", oxidraft_commands::ucs_cmd::ucs_name(&self.doc)));
                ui.horizontal_top(|ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_min_size(vec2(220.0, 160.0));
                        let mark = |u: &Ucs| if *u == current { "* " } else { "   " };
                        if ui.selectable_label(d.picked.is_none(), format!("{}World", mark(&WORLD))).clicked() {
                            d.picked = None;
                        }
                        for (name, ucs) in &named {
                            let picked = d.picked.as_deref() == Some(name.as_str());
                            if d.rename.as_ref().is_some_and(|r| &r.0 == name) {
                                let (_, new) = d.rename.as_mut().unwrap();
                                let r = ui.add(egui::TextEdit::singleline(new).desired_width(180.0));
                                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "UCS name"));
                                r.request_focus();
                                fields.push(r.id);
                            } else if ui.selectable_label(picked, format!("{}{name}", mark(ucs))).clicked() {
                                d.picked = Some(name.clone());
                            }
                        }
                    });
                    ui.vertical(|ui| {
                        set = ui.button("Set Current").clicked();
                        let is_named = d.picked.is_some();
                        if ui.add_enabled(is_named, egui::Button::new("Rename")).clicked()
                            && let Some(name) = d.picked.clone()
                        {
                            d.rename = Some((name.clone(), name));
                        }
                        delete = ui.add_enabled(is_named, egui::Button::new("Delete")).clicked();
                    });
                });
                let shown = d.picked.as_ref().and_then(|n| named.iter().find(|(m, _)| m == n)).map_or(WORLD, |x| x.1);
                ui.colored_label(
                    theme::text_dim(),
                    format!(
                        "Origin: {}, {}    X axis at {}",
                        oxidraft_model::units::length(shown.origin.x),
                        oxidraft_model::units::length(shown.origin.y),
                        oxidraft_model::units::direction(shown.angle)
                    ),
                );
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.separator();
                ui.checkbox(&mut icon, "Show the UCS icon");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        self.engine.vars.ucs_icon = icon;
        // Enter (or OK) finishes a rename.
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some((old, new)) = d.rename.clone()
            && (enter || ok)
        {
            match self.doc.rename_ucs(&old, &new) {
                Ok(()) => {
                    d.picked = Some(new.trim().to_string());
                    d.rename = None;
                    d.error = None;
                }
                Err(e) => d.error = Some(e),
            }
        }
        if set || ok {
            let ucs = d.picked.as_ref().and_then(|n| named.iter().find(|(m, _)| m == n)).map_or(WORLD, |x| x.1);
            self.doc.set_ucs(ucs);
        }
        if delete && let Some(name) = d.picked.take() {
            self.doc.delete_ucs(&name);
        }
        if open && !ok && !cancel {
            self.ucs_dialog = Some(d);
        }
    }
}

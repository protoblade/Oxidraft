//! DIMINSPECT: AutoCAD's Inspection Dimension dialog — the shape, label
//! and inspection rate put around the selected dimensions' text, or
//! removed from them.

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::annotation::{Inspection, InspectionShape};
use oxidraft_model::{EntityId, Geometry};

use crate::app::OxidraftApp;

pub(crate) struct InspectDialog {
    dimensions: Vec<EntityId>,
    shape: InspectionShape,
    label: Option<String>,
    rate: Option<String>,
    /// Hidden while dimensions are selected (SELECT).
    picking: bool,
}

impl OxidraftApp {
    /// The linear dimensions among `ids`.
    fn dimensions(&self, ids: &[EntityId]) -> Vec<EntityId> {
        let linear =
            |id: &&EntityId| matches!(self.doc.entity(**id).map(|e| &e.geometry), Some(Geometry::Dimension { .. }));
        ids.iter().filter(linear).copied().collect()
    }

    pub(crate) fn open_inspect_dialog(&mut self) {
        let dimensions = self.dimensions(self.engine.selection());
        self.inspect_dialog = Some(InspectDialog {
            dimensions,
            shape: InspectionShape::Round,
            label: None,
            rate: Some("100%".into()),
            picking: false,
        });
    }

    fn set_inspection(&mut self, ids: &[EntityId], inspection: Option<Inspection>) {
        self.doc.begin("DIMINSPECT");
        for id in ids {
            self.doc.update(*id, |e| {
                if let Geometry::Dimension { extra, .. } = &mut e.geometry {
                    extra.inspection.clone_from(&inspection);
                }
            });
        }
        self.doc.commit();
    }

    pub(crate) fn inspect_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.inspect_dialog.take() else { return };
        if d.picking {
            if !self.engine.is_active() {
                d.picking = false;
                d.dimensions = self.dimensions(self.engine.previous_selection());
            }
            self.inspect_dialog = Some(d);
            return;
        }
        let (mut open, mut ok, mut cancel, mut select, mut remove) = (true, false, false, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Inspection Dimension")
            .id(egui::Id::unique("inspect_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    select = ui.button("Select dimensions").clicked();
                    ui.label(format!("{} selected", d.dimensions.len()));
                });
                remove = ui.add_enabled(!d.dimensions.is_empty(), egui::Button::new("Remove Inspection")).clicked();
                ui.add_space(4.0);
                ui.label(RichText::new("Shape").strong());
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.shape, InspectionShape::Round, "Round");
                    ui.radio_value(&mut d.shape, InspectionShape::Angular, "Angular");
                    ui.radio_value(&mut d.shape, InspectionShape::None, "None");
                });
                ui.label(RichText::new("Label/Inspection rate").strong());
                for (name, value) in [("Label", &mut d.label), ("Inspection rate", &mut d.rate)] {
                    ui.horizontal(|ui| {
                        let mut on = value.is_some();
                        if ui.checkbox(&mut on, name).changed() {
                            *value = on.then(String::new);
                        }
                        if let Some(text) = value {
                            let r = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, name));
                            fields.push(r.id);
                        }
                    });
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.add_enabled(!d.dimensions.is_empty(), egui::Button::new("OK")).clicked();
                });
            });
        self.text_fields.extend(fields);
        if select {
            d.picking = true;
            self.run("SELECT");
            self.inspect_dialog = Some(d);
            return;
        }
        if remove {
            self.set_inspection(&d.dimensions, None);
            return;
        }
        if ok {
            let text = |v: &Option<String>| v.as_deref().map(str::trim).unwrap_or_default().to_string();
            let inspection = Inspection { shape: d.shape, label: text(&d.label), rate: text(&d.rate) };
            self.set_inspection(&d.dimensions, Some(inspection));
            return;
        }
        if open && !cancel {
            self.inspect_dialog = Some(d);
        }
    }
}

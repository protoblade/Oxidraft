//! MLEADERSTYLE and TABLESTYLE: how multileaders look (arrowhead and its
//! size, text height, landing) and how new tables look (text height, cell
//! margin).

use eframe::egui::{self, Align, ComboBox, Layout, RichText, Ui, vec2};
use oxidraft_model::{
    annotation::{LeaderArrow, LeaderStyle, TagShape},
    table::TableStyle,
};

use crate::app::OxidraftApp;

/// Which style the dialog edits, as it's being changed.
pub(crate) enum StyleDialog {
    Leader(LeaderStyle),
    Table(TableStyle),
}

fn number(ui: &mut Ui, label: &str, v: &mut f64, fields: &mut Vec<egui::Id>) {
    ui.horizontal(|ui| {
        ui.label(label);
        let r = ui.add(egui::DragValue::new(v).range(0.0..=1e6).speed(0.05).max_decimals(4));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, label));
        fields.push(r.id);
    });
}

impl OxidraftApp {
    pub(crate) fn open_leader_style(&mut self) {
        self.style_dialog = Some(StyleDialog::Leader(self.doc.leader_style()));
    }

    pub(crate) fn open_table_style(&mut self) {
        self.style_dialog = Some(StyleDialog::Table(self.doc.table_style()));
    }

    pub(crate) fn annotation_style_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.style_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        let title = match d {
            StyleDialog::Leader(_) => "Modify Multileader Style: Standard",
            StyleDialog::Table(_) => "Modify Table Style: Standard",
        };
        egui::Window::new(title)
            .id(egui::Id::unique("annotation_style"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.set_min_width(280.0);
                match &mut d {
                    StyleDialog::Leader(s) => {
                        ui.label(RichText::new("Arrowhead").strong());
                        ui.horizontal(|ui| {
                            ui.label("Symbol:");
                            let r = ComboBox::from_id_salt("leader_arrow")
                                .width(120.0)
                                .selected_text(s.arrowhead.name())
                                .show_ui(ui, |ui| {
                                    for a in LeaderArrow::ALL {
                                        ui.selectable_value(&mut s.arrowhead, a, a.name());
                                    }
                                });
                            r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Symbol"));
                        });
                        number(ui, "Size:", &mut s.arrow_size, &mut fields);
                        ui.label(RichText::new("Content").strong());
                        ui.horizontal(|ui| {
                            ui.label("Multileader type:");
                            let shown = s.block.map_or("Mtext", |_| "Block");
                            let r = ComboBox::from_id_salt("leader_content").width(120.0).selected_text(shown).show_ui(
                                ui,
                                |ui| {
                                    if ui.selectable_label(s.block.is_none(), "Mtext").clicked() {
                                        s.block = None;
                                    }
                                    if ui.selectable_label(s.block.is_some(), "Block").clicked() && s.block.is_none() {
                                        s.block = Some(TagShape::Circle);
                                    }
                                },
                            );
                            r.response.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Multileader type")
                            });
                        });
                        if let Some(shape) = &mut s.block {
                            ui.horizontal(|ui| {
                                ui.label("Source block:");
                                let r = ComboBox::from_id_salt("leader_block")
                                    .width(120.0)
                                    .selected_text(shape.label())
                                    .show_ui(ui, |ui| {
                                        for t in TagShape::ALL {
                                            ui.selectable_value(shape, t, t.label());
                                        }
                                    });
                                r.response.widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Source block")
                                });
                            });
                        } else {
                            number(ui, "Text height:", &mut s.text_height, &mut fields);
                        }
                        ui.label(RichText::new("Landing").strong());
                        ui.checkbox(&mut s.landing_on, "Automatically include landing");
                        ui.checkbox(&mut s.annotative, "Annotative")
                            .on_hover_text("New multileaders are sized for the annotation scale");
                        ui.add_enabled_ui(s.landing_on, |ui| {
                            number(ui, "Set landing distance:", &mut s.landing, &mut fields)
                        });
                    }
                    StyleDialog::Table(s) => {
                        ui.label(RichText::new("Text").strong());
                        number(ui, "Text height:", &mut s.text_height, &mut fields);
                        ui.label(RichText::new("Margins").strong());
                        number(ui, "Cell margin:", &mut s.margin, &mut fields);
                        ui.label("New tables (TABLE) take these.");
                    }
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            let (mut leader, mut table) = (self.doc.leader_style(), self.doc.table_style());
            match d {
                StyleDialog::Leader(s) => leader = s,
                StyleDialog::Table(s) => table = s,
            }
            leader.text_height = leader.text_height.max(0.01);
            table.text_height = table.text_height.max(0.01);
            self.doc.begin("STYLE");
            self.doc.set_annotation_styles(leader, table);
            self.doc.commit();
            return;
        }
        if open && !cancel {
            self.style_dialog = Some(d);
        }
    }
}

//! DSETTINGS (DS, OSNAP): AutoCAD's Drafting Settings dialog — snap and
//! grid, polar tracking, object snaps and dynamic input; and PTYPE's Point
//! Style dialog.

use eframe::egui::{self, Align, Layout, Sense, Stroke, vec2};
use oxidraft_model::snap::SnapKind;

use crate::{app::OxidraftApp, canvas::paint_point, theme};

/// Polar tracking increments offered, as AutoCAD lists them.
const INCREMENTS: [f64; 9] = [90.0, 60.0, 45.0, 30.0, 22.5, 18.0, 15.0, 10.0, 5.0];

/// The settings as they're being changed; OK puts them in place.
pub(crate) struct DraftingSettings {
    tab: usize,
    snap: bool,
    snap_spacing: f64,
    grid: bool,
    adaptive: bool,
    grid_spacing: f64,
    polar: bool,
    ortho: bool,
    increment: f64,
    osnap: bool,
    modes: Vec<SnapKind>,
    dynamic: bool,
}

/// PTYPE: the point style and size being chosen.
pub(crate) struct PointStyle {
    mode: i32,
    size: f64,
    /// Size as a percentage of the screen, not in drawing units.
    relative: bool,
}

/// PDMODE values in the dialog's order: each figure, plain, with a
/// circle, a square, and both.
const POINT_MODES: [i32; 20] = [0, 1, 2, 3, 4, 32, 33, 34, 35, 36, 64, 65, 66, 67, 68, 96, 97, 98, 99, 100];

impl OxidraftApp {
    pub(crate) fn open_drafting_settings(&mut self, tab: usize) {
        self.drafting_settings = Some(DraftingSettings {
            tab,
            snap: self.engine.vars.snap,
            snap_spacing: self.engine.vars.snap_spacing,
            grid: self.grid,
            adaptive: self.grid_spacing.is_none(),
            grid_spacing: self.grid_spacing.unwrap_or(10.0),
            polar: self.engine.polar(),
            ortho: self.engine.ortho(),
            increment: self.engine.polar_increment,
            osnap: self.osnap,
            modes: self.osnap_modes.clone(),
            dynamic: self.engine.dynamic_input,
        });
    }

    pub(crate) fn drafting_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.drafting_settings.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Drafting Settings")
            .id(egui::Id::unique("drafting_settings"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([400.0, 140.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, name) in ["Snap and Grid", "Polar Tracking", "Object Snap", "Dynamic Input"].iter().enumerate() {
                        ui.selectable_value(&mut d.tab, i, *name);
                    }
                });
                ui.separator();
                ui.set_min_size(vec2(420.0, 220.0));
                let mut number = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let r = ui.add(egui::DragValue::new(v).range(1e-6..=1e9).speed(0.1).max_decimals(6));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, label));
                        fields.push(r.id);
                    });
                };
                match d.tab {
                    0 => {
                        ui.checkbox(&mut d.snap, "Snap On (F9)");
                        number(ui, "Snap X spacing:", &mut d.snap_spacing);
                        ui.add_space(8.0);
                        ui.checkbox(&mut d.grid, "Grid On (F7)");
                        ui.checkbox(&mut d.adaptive, "Adaptive grid");
                        ui.add_enabled_ui(!d.adaptive, |ui| number(ui, "Grid X spacing:", &mut d.grid_spacing));
                        ui.colored_label(theme::text_dim(), "Every fifth grid line is a major one.");
                    }
                    1 => {
                        ui.checkbox(&mut d.polar, "Polar Tracking On (F10)");
                        ui.horizontal(|ui| {
                            ui.label("Increment angle:");
                            let r = egui::ComboBox::from_id_salt("polar_increment")
                                .width(80.0)
                                .selected_text(format!("{}", d.increment))
                                .show_ui(ui, |ui| {
                                    for a in INCREMENTS {
                                        ui.selectable_value(&mut d.increment, a, format!("{a}"));
                                    }
                                });
                            r.response.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Increment angle")
                            });
                        });
                        ui.checkbox(&mut d.ortho, "Ortho Mode (F8)");
                    }
                    2 => {
                        ui.checkbox(&mut d.osnap, "Object Snap On (F3)");
                        egui::Grid::new("osnap_modes").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                            for (i, kind) in SnapKind::ALL.iter().enumerate() {
                                let mut on = d.modes.contains(kind);
                                if ui.checkbox(&mut on, kind.label()).changed() {
                                    if on {
                                        d.modes.push(*kind);
                                    } else {
                                        d.modes.retain(|k| k != kind);
                                    }
                                }
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Select All").clicked() {
                                d.modes = SnapKind::ALL.to_vec();
                            }
                            if ui.button("Clear All").clicked() {
                                d.modes.clear();
                            }
                        });
                    }
                    _ => {
                        ui.checkbox(&mut d.dynamic, "Enable Dynamic Input (F12)");
                        ui.colored_label(
                            theme::text_dim(),
                            "Input and prompts at the cursor; typed coordinates are relative to the last point unless # comes first.",
                        );
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
            self.engine.vars.snap = d.snap;
            self.engine.vars.snap_spacing = d.snap_spacing;
            self.grid = d.grid;
            self.grid_spacing = (!d.adaptive).then_some(d.grid_spacing);
            self.engine.set_polar(d.polar);
            self.engine.set_ortho(d.ortho);
            // Ortho and polar tracking don't go together.
            if d.polar && !d.ortho {
                self.engine.set_polar(true);
            }
            self.engine.polar_increment = d.increment;
            self.osnap = d.osnap;
            self.osnap_modes = SnapKind::ALL.iter().copied().filter(|k| d.modes.contains(k)).collect();
            self.engine.dynamic_input = d.dynamic;
        }
        if open && !ok && !cancel {
            self.drafting_settings = Some(d);
        }
    }

    pub(crate) fn open_point_style(&mut self) {
        let (mode, size) = (self.doc.pdmode(), self.doc.pdsize());
        let relative = size <= 0.0;
        let size = if relative { if size == 0.0 { 5.0 } else { -size } } else { size };
        self.point_style = Some(PointStyle { mode, size, relative });
    }

    pub(crate) fn point_style_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.point_style.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Point Style")
            .id(egui::Id::unique("point_style"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([440.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                egui::Grid::new("point_modes").spacing([4.0, 4.0]).show(ui, |ui| {
                    for (i, mode) in POINT_MODES.iter().enumerate() {
                        let (rect, r) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::click());
                        let picked = d.mode == *mode;
                        let fill = if picked { theme::accent() } else { theme::model_bg() };
                        ui.painter().rect_filled(rect, 2.0, fill);
                        let ink = if picked { egui::Color32::WHITE } else { theme::text() };
                        paint_point(ui.painter(), rect.center(), *mode, 24.0, Stroke::new(1.2, ink));
                        r.widget_info(|| {
                            egui::WidgetInfo::selected(egui::Role::Button, true, picked, format!("PDMODE {mode}"))
                        });
                        if r.clicked() {
                            d.mode = *mode;
                        }
                        if i % 5 == 4 {
                            ui.end_row();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Point Size:");
                    let r = ui.add(egui::DragValue::new(&mut d.size).range(0.0..=1e9).speed(0.1).max_decimals(4));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "Point Size"));
                    fields.push(r.id);
                    ui.label(if d.relative { "%" } else { "Units" });
                });
                ui.radio_value(&mut d.relative, true, "Set Size Relative to Screen");
                ui.radio_value(&mut d.relative, false, "Set Size in Absolute Units");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            // PDSIZE: negative is a percentage of the screen.
            let size = if d.relative { -d.size } else { d.size };
            self.doc.begin("PTYPE");
            self.doc.set_point_style(d.mode, size);
            self.doc.commit();
        }
        if open && !ok && !cancel {
            self.point_style = Some(d);
        }
    }
}

//! The Layer Properties Manager and the layer state icons (on/off bulb,
//! thaw/freeze sun and snowflake, lock padlock) shared with the ribbon's
//! layer dropdown.

use eframe::egui::{self, Color32, Id, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};
use oxidraft_model::Layer;

use crate::{app::OxidraftApp, theme};

/// The ACI colors offered in color menus, with AutoCAD's names.
pub const NAMED_COLORS: &[(u8, &str)] = &[
    (1, "Red"),
    (2, "Yellow"),
    (3, "Green"),
    (4, "Cyan"),
    (5, "Blue"),
    (6, "Magenta"),
    (7, "White"),
    (8, "Gray"),
    (9, "Light Gray"),
];

pub fn color_name(index: u8) -> String {
    NAMED_COLORS.iter().find(|c| c.0 == index).map_or(format!("Color {index}"), |c| c.1.to_string())
}

#[derive(Clone, Copy)]
pub enum LayerToggle {
    On,
    Thawed,
    Unlocked,
}

/// A clickable layer state icon. Returns the click response.
pub fn state_icon(ui: &mut Ui, kind: LayerToggle, on: bool, tip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 2.0, theme::RIBBON_HOVER);
    }
    let p = ui.painter();
    let c = rect.center();
    let dim = theme::TEXT_DISABLED;
    match kind {
        LayerToggle::On => {
            let bulb = if on { Color32::from_rgb(250, 210, 60) } else { dim };
            if on {
                p.circle_filled(c - vec2(0.0, 2.0), 5.0, bulb);
            } else {
                p.circle_stroke(c - vec2(0.0, 2.0), 5.0, Stroke::new(1.2, bulb));
            }
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, 5.0), vec2(5.0, 3.0)), 0.5, theme::TEXT_DIM);
        }
        LayerToggle::Thawed => {
            if on {
                let sun = Color32::from_rgb(250, 190, 50);
                p.circle_filled(c, 3.0, sun);
                for i in 0..8 {
                    let a = i as f32 * std::f32::consts::TAU / 8.0;
                    let d = vec2(a.cos(), a.sin());
                    p.line_segment([c + d * 4.5, c + d * 6.5], Stroke::new(1.2, sun));
                }
            } else {
                let ice = Color32::from_rgb(120, 190, 255);
                for i in 0..3 {
                    let a = i as f32 * std::f32::consts::PI / 3.0 + std::f32::consts::FRAC_PI_2;
                    let d = vec2(a.cos(), a.sin()) * 6.5;
                    p.line_segment([c - d, c + d], Stroke::new(1.3, ice));
                }
            }
        }
        LayerToggle::Unlocked => {
            let color = if on { theme::TEXT_DIM } else { Color32::from_rgb(230, 180, 60) };
            let body = Rect::from_center_size(c + vec2(0.0, 2.5), vec2(10.0, 7.0));
            p.rect_filled(body, 1.0, color);
            // The shackle swings open when unlocked.
            let shift = if on { 3.0 } else { 0.0 };
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 / 12.0 * std::f32::consts::PI;
                    pos2(c.x + shift + 3.2 * a.cos(), body.top() - 0.5 + 4.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(1.5, color)));
        }
    }
    response.on_hover_text(tip)
}

/// A filled color square for an ACI color.
pub fn swatch(ui: &mut Ui, aci: u8) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::click());
    ui.painter().rect(rect, 1.0, theme::aci(aci), Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
    response
}

/// The on/freeze/lock icons for a layer. Returns the changed layer, if any.
pub fn layer_state_icons(ui: &mut Ui, layer: &Layer) -> Option<Layer> {
    let mut changed = None;
    if state_icon(ui, LayerToggle::On, layer.on, if layer.on { "Turn off" } else { "Turn on" }).clicked() {
        changed = Some(Layer { on: !layer.on, ..layer.clone() });
    }
    let thawed = !layer.frozen;
    if state_icon(ui, LayerToggle::Thawed, thawed, if thawed { "Freeze" } else { "Thaw" }).clicked() {
        changed = Some(Layer { frozen: !layer.frozen, ..layer.clone() });
    }
    let unlocked = !layer.locked;
    if state_icon(ui, LayerToggle::Unlocked, unlocked, if unlocked { "Lock" } else { "Unlock" }).clicked() {
        changed = Some(Layer { locked: !layer.locked, ..layer.clone() });
    }
    changed
}

/// A color picker menu for ACI colors. Returns the chosen index.
pub fn color_menu(ui: &mut Ui, current: u8, id_salt: impl std::hash::Hash + std::fmt::Debug) -> Option<u8> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        swatch(ui, current);
        let menu = egui::ComboBox::from_id_salt(id_salt).width(96.0).selected_text(color_name(current));
        let shown = menu.show_ui(ui, |ui| {
            for &(index, name) in NAMED_COLORS {
                ui.horizontal(|ui| {
                    swatch(ui, index);
                    if ui.selectable_label(index == current, name).clicked() {
                        chosen = Some(index);
                    }
                });
            }
        });
        shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Color"));
    });
    chosen
}

impl OxidraftApp {
    /// LAYER: the Layer Properties Manager window.
    pub(crate) fn layer_manager(&mut self, ctx: &egui::Context) {
        if !self.layer_manager_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Layer Properties Manager")
            .open(&mut open)
            .default_size([620.0, 320.0])
            .default_pos([260.0, 210.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                self.layer_toolbar(ui);
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| self.layer_table(ui));
                ui.separator();
                let shown = self.doc.layers().iter().filter(|l| l.is_shown()).count();
                ui.colored_label(
                    theme::TEXT_DIM,
                    format!("All: {shown} layers displayed of {} total layers", self.doc.layers().len()),
                );
            });
        if !open {
            self.layer_manager_open = false;
        }
    }

    fn layer_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("New Layer").on_hover_text("Creates a new layer").clicked() {
                let name = self.doc.unused_layer_name();
                self.doc.add_layer(Layer::new(name.clone(), 7));
                self.selected_layer = Some(name.clone());
                self.renaming_layer = Some((name.clone(), name));
            }
            let selected = self.selected_layer.clone();
            let can_delete = selected.as_deref().is_some_and(|n| n != "0" && n != self.doc.current_layer());
            if ui.add_enabled(can_delete, egui::Button::new("Delete Layer")).clicked()
                && let Some(name) = &selected
            {
                match self.doc.delete_layer(name) {
                    Ok(()) => self.selected_layer = None,
                    Err(e) => self.engine.log(e),
                }
            }
            if ui.add_enabled(selected.is_some(), egui::Button::new("Set Current")).clicked()
                && let Some(name) = &selected
            {
                self.doc.set_current_layer(name);
            }
        });
    }

    fn layer_table(&mut self, ui: &mut Ui) {
        let layers: Vec<Layer> = self.doc.layers().to_vec();
        let current = self.doc.current_layer().to_string();
        egui::Grid::new("layer_table").striped(true).num_columns(5).spacing([12.0, 4.0]).show(ui, |ui| {
            for title in ["Status", "Name", "On  Freeze  Lock", "Color", "Objects"] {
                ui.colored_label(theme::TEXT_DIM, title);
            }
            ui.end_row();
            for layer in layers {
                let is_current = layer.name == current;
                let selected = self.selected_layer.as_deref() == Some(layer.name.as_str());
                // Status: a green check on the current layer.
                let (rect, status) = ui.allocate_exact_size(vec2(36.0, 18.0), Sense::click());
                if is_current {
                    let c = rect.center();
                    let pts = vec![c + vec2(-5.0, 0.0), c + vec2(-1.5, 4.0), c + vec2(6.0, -5.0)];
                    ui.painter().add(Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(90, 200, 90))));
                }
                if status.double_clicked() {
                    self.doc.set_current_layer(&layer.name);
                }
                self.layer_name_cell(ui, &layer, selected);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    if let Some(changed) = layer_state_icons(ui, &layer) {
                        self.doc.update_layer(changed);
                    }
                });
                if let Some(color) = color_menu(ui, layer.color, ("layer_color", &layer.name)) {
                    self.doc.update_layer(Layer { color, ..layer.clone() });
                }
                ui.label(self.doc.layer_usage(&layer.name).to_string());
                ui.end_row();
            }
        });
    }

    /// The name column: click to select, double-click to rename, Enter to commit.
    fn layer_name_cell(&mut self, ui: &mut Ui, layer: &Layer, selected: bool) {
        if let Some((old, buffer)) = &mut self.renaming_layer
            && *old == layer.name
        {
            let id = Id::unique(("layer_rename", &layer.name));
            let response = ui.add(egui::TextEdit::singleline(buffer).id(id).desired_width(160.0));
            self.text_fields.push(id);
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            if response.lost_focus() {
                let (old, new) = (old.clone(), buffer.trim().to_string());
                self.renaming_layer = None;
                if new != old {
                    self.doc.begin("LAYER");
                    let result = self.doc.rename_layer(&old, &new);
                    self.doc.commit();
                    match result {
                        Ok(()) => self.selected_layer = Some(new),
                        Err(e) => self.engine.log(e),
                    }
                }
            }
            return;
        }
        let (rect, label) = ui.allocate_exact_size(vec2(160.0, 18.0), Sense::click());
        if selected {
            ui.painter().rect_filled(rect, 2.0, theme::ACCENT.gamma_multiply(0.35));
        } else if label.hovered() {
            ui.painter().rect_filled(rect, 2.0, theme::RIBBON_HOVER);
        }
        ui.painter().text(
            rect.left_center() + vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            &layer.name,
            egui::FontId::proportional(12.5),
            theme::TEXT,
        );
        let label = label.on_hover_text("Double-click to rename");
        if label.clicked() {
            self.selected_layer = Some(layer.name.clone());
        }
        if label.double_clicked() && layer.name != "0" {
            self.renaming_layer = Some((layer.name.clone(), layer.name.clone()));
        }
    }
}

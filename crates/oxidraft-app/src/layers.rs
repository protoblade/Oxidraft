//! The Layer Properties Manager and the layer state icons (on/off bulb,
//! thaw/freeze sun and snowflake, lock padlock) shared with the ribbon's
//! layer dropdown.

use eframe::egui::{self, Color32, Id, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};
use oxidraft_model::{Layer, Lineweight};

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
        ui.painter().rect_filled(rect, 2.0, theme::ribbon_hover());
    }
    let p = ui.painter();
    let c = rect.center();
    let dim = theme::text_disabled();
    match kind {
        LayerToggle::On => {
            let bulb = if on { Color32::from_rgb(250, 210, 60) } else { dim };
            if on {
                p.circle_filled(c - vec2(0.0, 2.0), 5.0, bulb);
            } else {
                p.circle_stroke(c - vec2(0.0, 2.0), 5.0, Stroke::new(1.2, bulb));
            }
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, 5.0), vec2(5.0, 3.0)), 0.5, theme::text_dim());
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
            let color = if on { theme::text_dim() } else { Color32::from_rgb(230, 180, 60) };
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
    ui.painter().rect(rect, 1.0, theme::swatch(aci), Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
    response
}

/// The on/freeze/lock icons for a layer. Returns the changed layer, if any.
pub fn layer_state_icons(ui: &mut Ui, layer: &Layer) -> Option<Layer> {
    // Named for screen readers by what a click does, and to which layer.
    let icon = |ui: &mut Ui, kind, on, tip: &str| {
        let response = state_icon(ui, kind, on, tip);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{tip}: {}", layer.name)));
        response.clicked()
    };
    let mut changed = None;
    if icon(ui, LayerToggle::On, layer.on, if layer.on { "Turn off" } else { "Turn on" }) {
        changed = Some(Layer { on: !layer.on, ..layer.clone() });
    }
    let thawed = !layer.frozen;
    if icon(ui, LayerToggle::Thawed, thawed, if thawed { "Freeze" } else { "Thaw" }) {
        changed = Some(Layer { frozen: !layer.frozen, ..layer.clone() });
    }
    let unlocked = !layer.locked;
    if icon(ui, LayerToggle::Unlocked, unlocked, if unlocked { "Lock" } else { "Unlock" }) {
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

/// A dropdown of linetype names, `width` wide (a long name cut short),
/// named `label` for screen readers. Returns the one picked, if any.
pub(crate) fn linetype_menu(
    ui: &mut Ui,
    current: Option<&str>,
    names: &[String],
    salt: impl std::hash::Hash + std::fmt::Debug,
    width: f32,
    label: &str,
) -> Option<String> {
    let mut chosen = None;
    let menu = egui::ComboBox::from_id_salt(salt).width(width).truncate();
    let shown = menu.selected_text(current.unwrap_or("*VARIES*")).show_ui(ui, |ui| {
        for name in names {
            let on = current.is_some_and(|c| c.eq_ignore_ascii_case(name));
            if ui.selectable_label(on, name).clicked() {
                chosen = Some(name.clone());
            }
        }
    });
    shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
    chosen
}

/// A dropdown of lineweights, starting with `special` (ByLayer, ByBlock,
/// Default as fits), named `label`. Returns the one picked, if any.
pub(crate) fn lineweight_menu(
    ui: &mut Ui,
    current: Option<Lineweight>,
    special: &[Lineweight],
    salt: impl std::hash::Hash + std::fmt::Debug,
    width: f32,
    label: &str,
) -> Option<Lineweight> {
    let mut chosen = None;
    let text = current.map_or("*VARIES*".into(), Lineweight::label);
    let shown = egui::ComboBox::from_id_salt(salt).width(width).selected_text(text).show_ui(ui, |ui| {
        let all = special.iter().copied().chain(Lineweight::STANDARD.into_iter().map(Lineweight::Hundredths));
        for lw in all {
            if ui.selectable_label(current == Some(lw), lw.label()).clicked() {
                chosen = Some(lw);
            }
        }
    });
    shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
    chosen
}

fn name_font() -> egui::FontId {
    egui::FontId::proportional(12.5)
}

/// The Layer Properties Manager window's id.
pub const LAYER_MANAGER: &str = "layer_manager";

impl OxidraftApp {
    /// LAYER: the Layer Properties Manager window.
    pub(crate) fn layer_manager(&mut self, ctx: &egui::Context) {
        if !self.layer_manager_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Layer Properties Manager")
            .id(egui::Id::unique(LAYER_MANAGER))
            .open(&mut open)
            .default_size([860.0, 320.0])
            .default_pos([260.0, 210.0])
            .constrain_to(self.dialog_area(ctx))
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                self.layer_toolbar(ui);
                ui.separator();
                // The count stays at the bottom and the table scrolls above it,
                // so a long table can't push the window past its limit.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let shown = self.doc.layers().iter().filter(|l| l.is_shown()).count();
                    ui.colored_label(
                        theme::text_dim(),
                        format!("All: {shown} layers displayed of {} total layers", self.doc.layers().len()),
                    );
                    ui.separator();
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| self.layer_table(ui));
                    });
                });
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
                self.selected_layers = vec![name.clone()];
                self.renaming_layer = Some((name.clone(), name));
            }
            let selected = self.selected_layers.clone();
            let current = self.doc.current_layer().to_string();
            let can_delete = selected.iter().any(|n| n != "0" && *n != current);
            let delete = egui::Button::new("Delete Layer");
            if ui.add_enabled(can_delete, delete).on_hover_text("Deletes the selected layers").clicked() {
                self.delete_layers(&selected);
            }
            if let [one] = selected.as_slice() {
                if ui.button("Set Current").clicked() {
                    self.doc.set_current_layer(one);
                }
            } else {
                ui.add_enabled(false, egui::Button::new("Set Current"));
            }
            if selected.len() > 1 {
                ui.colored_label(theme::text_dim(), format!("{} layers selected", selected.len()));
            }
        });
    }

    /// Delete Layer: deletes the selected layers it can, as one undo step,
    /// and says why it can't delete the others.
    fn delete_layers(&mut self, names: &[String]) {
        self.doc.begin("LAYER");
        for name in names {
            match self.doc.delete_layer(name) {
                Ok(()) => self.selected_layers.retain(|s| s != name),
                Err(e) => self.engine.log(e),
            }
        }
        self.doc.commit();
    }

    /// A change made on `name`'s row: to every selected layer if it's one of
    /// them, as in AutoCAD, else to it alone (which becomes the selection).
    /// One undo step.
    fn change_layers(&mut self, name: &str, f: impl Fn(&mut Layer)) {
        if !self.selected_layers.iter().any(|s| s == name) {
            self.selected_layers = vec![name.to_string()];
        }
        self.doc.begin("LAYER");
        for n in self.selected_layers.clone() {
            if let Some(mut layer) = self.doc.layer(&n).cloned() {
                f(&mut layer);
                self.doc.update_layer(layer);
            }
        }
        self.doc.commit();
    }

    /// A click on a layer's name: selects it alone; with Cmd (Ctrl) adds or
    /// removes it; with Shift selects the run from the last one clicked.
    fn click_layer(&mut self, ui: &Ui, name: &str, order: &[String]) {
        let (toggle, range) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        let anchor = self.layer_anchor.clone().filter(|a| order.contains(a));
        if range && let Some(anchor) = anchor {
            let (a, b) = (order.iter().position(|n| *n == anchor), order.iter().position(|n| n == name));
            if let (Some(a), Some(b)) = (a, b) {
                self.selected_layers = order[a.min(b)..=a.max(b)].to_vec();
            }
            return;
        }
        if toggle {
            if self.selected_layers.iter().any(|s| s == name) {
                self.selected_layers.retain(|s| s != name);
            } else {
                self.selected_layers.push(name.to_string());
            }
        } else {
            self.selected_layers = vec![name.to_string()];
        }
        self.layer_anchor = Some(name.to_string());
    }

    fn layer_table(&mut self, ui: &mut Ui) {
        let layers: Vec<Layer> = self.doc.layers().to_vec();
        let order: Vec<String> = layers.iter().map(|l| l.name.clone()).collect();
        let current = self.doc.current_layer().to_string();
        let linetypes: Vec<String> = self.doc.linetypes().into_iter().map(|l| l.name).collect();
        // Columns as wide as what's in them, up to a limit; longer names are
        // cut short and shown whole on hover.
        let text_width = |ui: &Ui, s: &str| ui.painter().layout_no_wrap(s.into(), name_font(), theme::text()).size().x;
        let name_width = layers.iter().map(|l| text_width(ui, &l.name)).fold(0.0, f32::max).clamp(112.0, 360.0) + 10.0;
        let in_use = layers.iter().map(|l| l.linetype.as_str());
        let linetype_width = in_use.map(|n| text_width(ui, n)).fold(0.0, f32::max).clamp(96.0, 300.0) + 12.0;
        egui::Grid::new("layer_table").striped(true).num_columns(7).spacing([12.0, 4.0]).show(ui, |ui| {
            for title in ["Status", "Name", "On  Freeze  Lock", "Color", "Linetype", "Lineweight", "Objects"] {
                ui.colored_label(theme::text_dim(), title);
            }
            ui.end_row();
            for layer in layers {
                let is_current = layer.name == current;
                let selected = self.selected_layers.contains(&layer.name);
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
                self.layer_name_cell(ui, &layer, selected, name_width, &order);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    if let Some(changed) = layer_state_icons(ui, &layer) {
                        let name = &layer.name;
                        if changed.on != layer.on {
                            self.change_layers(name, |l| l.on = changed.on);
                        } else if changed.frozen != layer.frozen {
                            self.change_layers(name, |l| l.frozen = changed.frozen);
                        } else if changed.locked != layer.locked {
                            self.change_layers(name, |l| l.locked = changed.locked);
                        }
                    }
                });
                if let Some(color) = color_menu(ui, layer.color, ("layer_color", &layer.name)) {
                    self.change_layers(&layer.name, |l| l.color = color);
                }
                let salt = ("layer_linetype", &layer.name);
                if let Some(linetype) =
                    linetype_menu(ui, Some(&layer.linetype), &linetypes, salt, linetype_width, "Linetype")
                {
                    self.change_layers(&layer.name, |l| l.linetype = linetype.clone());
                }
                let salt = ("layer_lineweight", &layer.name);
                if let Some(lineweight) =
                    lineweight_menu(ui, Some(layer.lineweight), &[Lineweight::Default], salt, 90.0, "Lineweight")
                {
                    self.change_layers(&layer.name, |l| l.lineweight = lineweight);
                }
                ui.label(self.doc.layer_usage(&layer.name).to_string());
                ui.end_row();
            }
        });
    }

    /// The name column, `width` wide: click to select (Cmd and Shift for
    /// several), double-click to rename, Enter to commit.
    fn layer_name_cell(&mut self, ui: &mut Ui, layer: &Layer, selected: bool, width: f32, order: &[String]) {
        if let Some((old, buffer)) = &mut self.renaming_layer
            && *old == layer.name
        {
            let id = Id::unique(("layer_rename", &layer.name));
            let response = ui.add(egui::TextEdit::singleline(buffer).id(id).desired_width(width));
            self.text_fields.push(id);
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            if response.lost_focus() {
                let (old, new) = (old.clone(), buffer.trim().to_string());
                self.renaming_layer = None;
                // Esc keeps the old name.
                if new != old && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.doc.begin("LAYER");
                    let result = self.doc.rename_layer(&old, &new);
                    self.doc.commit();
                    match result {
                        Ok(()) => self.selected_layers = vec![new],
                        Err(e) => self.engine.log(e),
                    }
                }
            }
            return;
        }
        let (rect, label) = ui.allocate_exact_size(vec2(width, 18.0), Sense::click());
        if selected {
            ui.painter().rect_filled(rect, 2.0, theme::accent().gamma_multiply(0.35));
        } else if label.hovered() {
            ui.painter().rect_filled(rect, 2.0, theme::ribbon_hover());
        }
        // A name too long for the column is cut short with an ellipsis.
        let mut job = egui::text::LayoutJob::simple_singleline(layer.name.clone(), name_font(), theme::text());
        job.wrap = egui::text::TextWrapping::truncate_at_width(width - 8.0);
        let galley = ui.painter().layout_job(job);
        let cut = galley.elided;
        ui.painter().galley(rect.left_center() + vec2(4.0, -galley.size().y / 2.0), galley, theme::text());
        label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &layer.name));
        let tip = if cut { format!("{}\nDouble-click to rename", layer.name) } else { "Double-click to rename".into() };
        let label = label.on_hover_text(tip);
        if label.clicked() {
            self.click_layer(ui, &layer.name, order);
        }
        if label.double_clicked() && layer.name != "0" {
            self.renaming_layer = Some((layer.name.clone(), layer.name.clone()));
        }
    }
}

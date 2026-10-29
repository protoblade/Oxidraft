//! What new objects get, and the dialogs that set it: the Home tab's
//! Properties panel dropdowns (color, linetype, lineweight of the selected
//! objects, or of new ones), COLOR's Select Color, LINETYPE's Linetype
//! Manager and LWEIGHT's Lineweight Settings.

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, Ui, vec2};
use oxidraft_commands::current_props::{color_text, loaded_linetypes, parse_color};
use oxidraft_model::{Color, CurrentProps, EntityId, Linetype, Lineweight, linetype};

use crate::{
    app::OxidraftApp,
    layers::{NAMED_COLORS, color_name, swatch},
    properties::common,
    theme,
};

/// What a color dropdown picked.
pub(crate) enum ColorPick {
    Color(Color),
    /// "Select Colors...": the full palette.
    More,
}

/// A color's name: ByLayer, ByBlock, a named color or its index.
pub(crate) fn color_label(c: Color) -> String {
    match c {
        Color::ByLayer => "ByLayer".into(),
        Color::ByBlock => "ByBlock".into(),
        Color::Index(i) => color_name(i),
    }
}

/// A dropdown of object colors (ByLayer, ByBlock, the named ones, and
/// Select Colors…); `None` shown as varying.
pub(crate) fn object_color_menu(ui: &mut Ui, current: Option<Color>, salt: &str, width: f32) -> Option<ColorPick> {
    let mut chosen = None;
    let text = current.map_or("*VARIES*".into(), color_label);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        match current {
            Some(Color::Index(i)) => {
                swatch(ui, i);
            }
            _ => {
                ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            }
        }
        let shown = egui::ComboBox::from_id_salt(salt).width(width).selected_text(text).show_ui(ui, |ui| {
            for (c, name) in [(Color::ByLayer, "ByLayer"), (Color::ByBlock, "ByBlock")] {
                if ui.selectable_label(current == Some(c), name).clicked() {
                    chosen = Some(ColorPick::Color(c));
                }
            }
            for &(index, name) in NAMED_COLORS {
                ui.horizontal(|ui| {
                    swatch(ui, index);
                    if ui.selectable_label(current == Some(Color::Index(index)), name).clicked() {
                        chosen = Some(ColorPick::Color(Color::Index(index)));
                    }
                });
            }
            ui.separator();
            if ui.selectable_label(false, "Select Colors...").clicked() {
                chosen = Some(ColorPick::More);
            }
        });
        shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Object color"));
    });
    chosen
}

/// A linetype's look: its dashes drawn across `width`.
fn paint_linetype(ui: &mut Ui, l: &Linetype, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), Sense::hover());
    let y = rect.center().y;
    let stroke = Stroke::new(1.2, theme::text());
    if l.pattern.is_empty() {
        ui.painter().line_segment([rect.left_center(), rect.right_center()], stroke);
        return;
    }
    // A pattern's repeat across about a third of the width.
    let scale = (width / 3.0) / l.length().max(1e-9) as f32;
    let mut x = rect.left();
    'draw: loop {
        for &d in &l.pattern {
            let len = d.abs() as f32 * scale;
            if d > 0.0 {
                let end = (x + len).min(rect.right());
                ui.painter().line_segment([egui::pos2(x, y), egui::pos2(end, y)], stroke);
            } else if d == 0.0 {
                ui.painter().circle_filled(egui::pos2(x, y), 0.9, theme::text());
            }
            x += len.max(1.0);
            if x >= rect.right() {
                break 'draw;
            }
        }
    }
}

/// COLOR's Select Color dialog: for new objects, or for objects picked.
pub(crate) struct ColorDialog {
    color: Color,
    text: String,
    objects: Vec<EntityId>,
}

/// LINETYPE's Linetype Manager, and its Load window when open.
pub(crate) struct LinetypeManager {
    selected: Option<String>,
    loading: Option<Vec<(Linetype, bool)>>,
}

/// LWEIGHT's Lineweight Settings.
pub(crate) struct LineweightDialog {
    weight: Lineweight,
    display: bool,
}

impl OxidraftApp {
    /// Sets what new objects get, as one undoable step.
    fn set_current_props(&mut self, f: impl FnOnce(&mut CurrentProps)) {
        let mut props = self.doc.current_props().clone();
        f(&mut props);
        self.set_current(props);
    }

    /// The Home tab's Properties panel: color, linetype and lineweight of
    /// the selected objects, or of new ones when nothing is selected.
    pub(crate) fn property_combos(&mut self, ui: &mut Ui) {
        let selected = if self.engine.is_active() { Vec::new() } else { self.engine.selection().to_vec() };
        let entities: Vec<_> = selected.iter().filter_map(|&id| self.doc.entity(id)).cloned().collect();
        let current = self.doc.current_props().clone();
        let (color, linetype, weight) = if entities.is_empty() {
            (Some(current.color), Some(current.linetype.clone()), Some(current.lineweight))
        } else {
            (
                common(entities.iter().map(|e| e.color)),
                common(entities.iter().map(|e| e.linetype.clone())),
                common(entities.iter().map(|e| e.lineweight)),
            )
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add_space(4.0);
            match object_color_menu(ui, color, "ribbon_color", 130.0) {
                Some(ColorPick::Color(c)) if entities.is_empty() => self.set_current_props(|p| p.color = c),
                Some(ColorPick::Color(c)) => self.edit(selected.clone(), |e| e.color = c),
                Some(ColorPick::More) => self.open_color(selected.clone()),
                None => {}
            }
            let mut names: Vec<String> = [linetype::BYLAYER, linetype::BYBLOCK, linetype::CONTINUOUS]
                .into_iter()
                .map(String::from)
                .chain(loaded_linetypes(&self.doc).into_iter().map(|l| l.name))
                .collect();
            names.push("Other...".into());
            ui.horizontal(|ui| {
                ui.add_space(15.0);
                match crate::layers::linetype_menu(
                    ui,
                    linetype.as_deref(),
                    &names,
                    "ribbon_linetype",
                    130.0,
                    "Object linetype",
                ) {
                    Some(n) if n == "Other..." => self.open_linetype_manager(),
                    Some(n) if entities.is_empty() => self.set_current_props(|p| p.linetype = n),
                    Some(n) => self.edit(selected.clone(), |e| e.linetype = n.clone()),
                    None => {}
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(15.0);
                let special = [Lineweight::ByLayer, Lineweight::ByBlock, Lineweight::Default];
                match crate::layers::lineweight_menu(
                    ui,
                    weight,
                    &special,
                    "ribbon_lineweight",
                    130.0,
                    "Object lineweight",
                ) {
                    Some(w) if entities.is_empty() => self.set_current_props(|p| p.lineweight = w),
                    Some(w) => self.edit(selected.clone(), |e| e.lineweight = w),
                    None => {}
                }
            });
        });
    }

    /// COLOR: the Select Color dialog, for `objects` or (none) new objects.
    pub(crate) fn open_color(&mut self, objects: Vec<EntityId>) {
        let color = match objects.as_slice() {
            [] => self.doc.current_props().color,
            ids => common(ids.iter().filter_map(|id| self.doc.entity(*id)).map(|e| e.color)).unwrap_or(Color::ByLayer),
        };
        self.color_dialog = Some(ColorDialog { color, text: color_text(color), objects });
    }

    pub(crate) fn color_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.color_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Select Color")
            .id(egui::Id::unique("select_color"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 140.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("AutoCAD Color Index (ACI):").strong());
                // 10 to 249 in AutoCAD's order, then the standard and gray rows.
                let mut cell = |ui: &mut Ui, i: u8, size: f32| {
                    let (rect, r) = ui.allocate_exact_size(vec2(size, size), Sense::click());
                    let picked = d.color == Color::Index(i);
                    let edge = if picked || r.hovered() {
                        Stroke::new(2.0, theme::text())
                    } else {
                        Stroke::new(0.5, Color32::BLACK)
                    };
                    ui.painter().rect(rect, 0.0, theme::swatch(i), edge, egui::StrokeKind::Inside);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Color {i}")));
                    if r.on_hover_text(format!("Index color: {i}")).clicked() {
                        d.color = Color::Index(i);
                        d.text = color_text(d.color);
                    }
                };
                egui::Grid::new("aci_grid").spacing([1.0, 1.0]).show(ui, |ui| {
                    for row in 0..10u8 {
                        for column in 0..24u8 {
                            cell(ui, 10 + column * 10 + row, 13.0);
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for i in 1..=9 {
                        cell(ui, i, 18.0);
                    }
                    ui.add_space(12.0);
                    for i in 250..=255 {
                        cell(ui, i, 18.0);
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (c, name) in [(Color::ByLayer, "ByLayer"), (Color::ByBlock, "ByBlock")] {
                        if ui.selectable_label(d.color == c, name).clicked() {
                            d.color = c;
                            d.text = color_text(c);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Color:");
                    let r = ui.add(egui::TextEdit::singleline(&mut d.text).desired_width(120.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Color"));
                    fields.push(r.id);
                    if r.changed()
                        && let Some(c) = parse_color(&d.text)
                    {
                        d.color = c;
                    }
                    if let Color::Index(i) = d.color {
                        swatch(ui, i);
                    }
                });
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            let color = d.color;
            if d.objects.is_empty() {
                self.set_current_props(|p| p.color = color);
            } else {
                self.edit(d.objects.clone(), |e| e.color = color);
            }
        }
        if open && !ok && !cancel {
            self.color_dialog = Some(d);
        }
    }

    pub(crate) fn open_linetype_manager(&mut self) {
        self.linetype_manager = Some(LinetypeManager { selected: None, loading: None });
    }

    /// Linetypes in use anywhere, or current (they can't be deleted).
    fn linetype_in_use(&self, name: &str) -> bool {
        let is = |n: &str| n.eq_ignore_ascii_case(name);
        is(&self.doc.current_props().linetype)
            || self.doc.all_entities().any(|e| is(&e.linetype))
            || self.doc.layers().iter().any(|l| is(&l.linetype))
            || self.doc.blocks().any(|b| b.entities.iter().any(|be| is(&be.linetype)))
    }

    pub(crate) fn linetype_manager_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.linetype_manager.take() else { return };
        let (mut open, mut close, mut load, mut delete, mut make_current) = (true, false, false, false, false);
        let mut fields = Vec::new();
        let current = self.doc.current_props().linetype.clone();
        let rows: Vec<Linetype> = [
            Linetype::new(linetype::BYLAYER, "", &[]),
            Linetype::new(linetype::BYBLOCK, "", &[]),
            Linetype::continuous(),
        ]
        .into_iter()
        .chain(loaded_linetypes(&self.doc))
        .collect();
        let mut ltscale = self.doc.ltscale();
        egui::Window::new("Linetype Manager")
            .id(egui::Id::unique("linetype_manager"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([360.0, 130.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(format!("Current Linetype: {current}"));
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(440.0);
                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        egui::Grid::new("linetypes").num_columns(3).spacing([14.0, 4.0]).show(ui, |ui| {
                            for t in ["Linetype", "Appearance", "Description"] {
                                ui.label(RichText::new(t).strong());
                            }
                            ui.end_row();
                            for l in &rows {
                                let picked = d.selected.as_ref().is_some_and(|s| s.eq_ignore_ascii_case(&l.name));
                                if ui.selectable_label(picked, &l.name).clicked() {
                                    d.selected = Some(l.name.clone());
                                }
                                if l.name == linetype::BYLAYER || l.name == linetype::BYBLOCK {
                                    ui.label("");
                                } else {
                                    paint_linetype(ui, l, 110.0);
                                }
                                ui.label(&l.description);
                                ui.end_row();
                            }
                        });
                    });
                });
                ui.horizontal(|ui| {
                    load = ui.button("Load...").clicked();
                    let deletable = d.selected.as_ref().is_some_and(|s| {
                        self.doc.own_linetypes().iter().any(|o| o.eq_ignore_ascii_case(s)) && !self.linetype_in_use(s)
                    });
                    delete = ui.add_enabled(deletable, egui::Button::new("Delete")).clicked();
                    make_current = ui.add_enabled(d.selected.is_some(), egui::Button::new("Current")).clicked();
                });
                ui.horizontal(|ui| {
                    ui.label("Global scale factor:");
                    let r = ui.add(egui::DragValue::new(&mut ltscale).range(0.0001..=1e6).speed(0.05).max_decimals(4));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "Global scale factor"));
                    fields.push(r.id);
                });
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("OK").clicked();
                });
            });
        self.doc.set_ltscale(ltscale);
        if load {
            let loaded = loaded_linetypes(&self.doc);
            let choices = linetype::standard()
                .into_iter()
                .filter(|l| !loaded.iter().any(|o| o.name.eq_ignore_ascii_case(&l.name)))
                .map(|l| (l, false))
                .collect();
            d.loading = Some(choices);
        }
        if delete && let Some(name) = d.selected.take() {
            self.doc.begin("LINETYPE");
            self.doc.remove_linetype(&name);
            self.doc.commit();
        }
        if make_current && let Some(name) = d.selected.clone() {
            self.set_current_props(|p| p.linetype = name);
        }
        self.load_linetypes_window(ctx, &mut d, &mut fields);
        self.text_fields.extend(fields);
        if open && !close {
            self.linetype_manager = Some(d);
        }
    }

    /// "Load or Reload Linetypes": the standard linetypes not loaded yet.
    fn load_linetypes_window(&mut self, ctx: &egui::Context, d: &mut LinetypeManager, _fields: &mut Vec<egui::Id>) {
        let Some(choices) = &mut d.loading else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Load or Reload Linetypes")
            .id(egui::Id::unique("load_linetypes"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 180.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.label("Available Linetypes");
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(380.0);
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        if choices.is_empty() {
                            ui.colored_label(theme::text_dim(), "Every linetype is loaded.");
                        }
                        for (l, ticked) in choices.iter_mut() {
                            ui.horizontal(|ui| {
                                ui.checkbox(ticked, l.name.as_str());
                                paint_linetype(ui, l, 90.0);
                            });
                        }
                    });
                });
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok {
            self.doc.begin("LINETYPE");
            for (l, _) in choices.iter().filter(|c| c.1) {
                self.doc.load_linetype(l.clone());
            }
            self.doc.commit();
        }
        if !open || ok || cancel {
            d.loading = None;
        }
    }

    pub(crate) fn open_lineweight_settings(&mut self) {
        let weight = self.doc.current_props().lineweight;
        self.lineweight_dialog = Some(LineweightDialog { weight, display: self.show_lineweights });
    }

    pub(crate) fn lineweight_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.lineweight_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Lineweight Settings")
            .id(egui::Id::unique("lineweight_settings"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 150.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Lineweights").strong());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        let all = [Lineweight::ByLayer, Lineweight::ByBlock, Lineweight::Default]
                            .into_iter()
                            .chain(Lineweight::STANDARD.into_iter().map(Lineweight::Hundredths));
                        for w in all {
                            ui.selectable_value(&mut d.weight, w, w.label());
                        }
                    });
                });
                ui.label(format!("Current Lineweight: {}", d.weight.label()));
                ui.checkbox(&mut d.display, "Display Lineweight");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok {
            self.show_lineweights = d.display;
            let weight = d.weight;
            self.set_current_props(|p| p.lineweight = weight);
        }
        if open && !ok && !cancel {
            self.lineweight_dialog = Some(d);
        }
    }
}

//! The Properties palette (PROPERTIES, Ctrl/Cmd+1): general and geometric
//! properties of the selected objects, editable in place.

use eframe::egui::{self, Align, CursorIcon, Frame, Id, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use oxidraft_geom::{Point, curve::ArcSeg};
use oxidraft_model::{
    Color, Entity, EntityId, Geometry, HatchPattern, Lineweight,
    annotation::{angular_measurement, format_angle, format_measurement, measurement, radial_value},
    hatch, linetype,
};

use crate::{
    app::OxidraftApp,
    layers::{NAMED_COLORS, color_name, linetype_menu, lineweight_menu, swatch},
    settings::{Dock, Settings},
    theme,
};

const VARIES: &str = "*VARIES*";
/// The width of the property names; every value takes the rest of the
/// palette's width, at least `MIN_VALUE_WIDTH`.
const LABEL_WIDTH: f32 = 92.0;
const MIN_VALUE_WIDTH: f32 = 110.0;
/// How close to the drawing area's side a dragged palette docks there.
const DOCK_ZONE: f32 = 48.0;

/// A property row's value: read-only text, or a number that can be edited.
enum Value {
    Text(String),
    Number(f64),
    Angle(f64),
    /// Editable text (contents, overrides, pattern names).
    Edit(String),
    /// One of a list, as a dropdown.
    Choice(&'static [&'static str], String),
}

const YES_NO: &[&str] = &["Yes", "No"];

/// Viewport scales as AutoCAD lists them (paper : model), metric and
/// imperial-friendly.
const SCALES: &[&str] = &[
    "1:1", "1:2", "1:4", "1:5", "1:8", "1:10", "1:16", "1:20", "1:25", "1:30", "1:40", "1:50", "1:100", "1:200",
    "1:250", "1:500", "1:1000", "1:2000", "1:5000", "1:10000", "2:1", "4:1", "5:1", "8:1", "10:1", "100:1", "Custom",
];

/// Model units per paper unit for a scale such as "1:50".
fn scale_ratio(scale: &str) -> Option<f64> {
    let (paper, model) = scale.split_once(':')?;
    let (paper, model) = (paper.trim().parse::<f64>().ok()?, model.trim().parse::<f64>().ok()?);
    (paper > 0.0 && model > 0.0).then(|| model / paper)
}

/// The standard scale a viewport's ratio is, or "Custom".
fn scale_name(ratio: f64) -> &'static str {
    SCALES
        .iter()
        .find(|s| scale_ratio(s).is_some_and(|r| (r - ratio).abs() <= ratio * 1e-6))
        .copied()
        .unwrap_or("Custom")
}

fn yes_no(on: bool) -> Value {
    Value::Choice(YES_NO, if on { "Yes" } else { "No" }.into())
}

impl OxidraftApp {
    /// The Properties palette: docked at the left or right of the drawing
    /// area, or floating. Dragging its title moves it; dropped at either
    /// side of the drawing area, it docks there.
    pub(crate) fn properties_palette(&mut self, ui: &mut Ui) {
        if !self.properties_open {
            return;
        }
        let frame = Frame::NONE.fill(theme::ribbon()).inner_margin(Margin::same(8));
        match self.settings.properties_dock {
            Dock::Left => {
                egui::Panel::left("properties")
                    .frame(frame)
                    .resizable(true)
                    .default_size(280.0)
                    .show(ui, |ui| self.properties_body(ui));
            }
            Dock::Right => {
                egui::Panel::right("properties_right")
                    .frame(frame)
                    .resizable(true)
                    .default_size(280.0)
                    .show(ui, |ui| self.properties_body(ui));
            }
            Dock::Float => {
                let window = egui::Window::new("Properties")
                    .id(Id::unique("properties_float"))
                    .title_bar(false)
                    .movable(false)
                    .resizable(true)
                    .collapsible(false)
                    .default_size([290.0, 520.0])
                    .current_pos(self.properties_pos)
                    .constrain_to(self.dialog_area(ui.ctx()))
                    .frame(frame.stroke(Stroke::new(1.0, theme::tooltip_edge())).corner_radius(3));
                window.show(ui.ctx(), |ui| self.properties_body(ui));
            }
        }
        self.drag_properties(ui.ctx());
    }

    /// Moves the palette while its title is held, and docks it when it's
    /// let go at a side of the drawing area.
    fn drag_properties(&mut self, ctx: &egui::Context) {
        let Some(grab) = self.properties_grab else { return };
        let (pointer, down) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down()));
        let Some(pointer) = pointer else { return };
        let area = self.canvas_rect;
        let side = if pointer.x <= area.left() + DOCK_ZONE {
            Some(Dock::Left)
        } else if pointer.x >= area.right() - DOCK_ZONE {
            Some(Dock::Right)
        } else {
            None
        };
        if down {
            self.properties_pos = pointer - grab;
            ctx.set_cursor_icon(CursorIcon::Grabbing);
            // Where it would dock.
            if let Some(side) = side {
                let strip = match side {
                    Dock::Left => egui::Rect::from_min_size(area.left_top(), vec2(280.0, area.height())),
                    _ => egui::Rect::from_min_max(area.right_top() - vec2(280.0, 0.0), area.right_bottom()),
                };
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::unique("dock_hint")));
                painter.rect_filled(strip, 0.0, theme::accent().gamma_multiply(0.25));
            }
            ctx.request_repaint();
            return;
        }
        self.properties_grab = None;
        if let Some(side) = side {
            self.set_properties_dock(ctx, side);
        }
    }

    pub(crate) fn set_properties_dock(&mut self, ctx: &egui::Context, dock: Dock) {
        self.apply_settings(ctx, Settings { properties_dock: dock, ..self.settings.clone() });
    }

    fn properties_body(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new("x").frame(false)).on_hover_text("Close (Cmd+1)").clicked() {
                    self.properties_open = false;
                }
                let dock = self.settings.properties_dock;
                let menu = ui.small_button("Dock").on_hover_text("Dock at the left or right, or float");
                egui::Popup::menu(&menu).show(|ui| {
                    for (label, to) in [("Dock Left", Dock::Left), ("Dock Right", Dock::Right), ("Float", Dock::Float)]
                    {
                        if ui.add_enabled(dock != to, egui::Button::new(label)).clicked() {
                            if to == Dock::Float {
                                self.properties_pos = menu.rect.left_bottom() + vec2(-200.0, 8.0);
                            }
                            self.set_properties_dock(ui.ctx(), to);
                        }
                    }
                });
                if ui
                    .small_button("Quick Select")
                    .on_hover_text("Select objects by type and property (QSELECT)")
                    .clicked()
                {
                    self.open_quick_select();
                }
                // The title is the handle: drag it to move the palette.
                let (rect, title) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::drag());
                ui.painter().text(
                    rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    "PROPERTIES",
                    egui::FontId::proportional(12.5),
                    theme::text_dim(),
                );
                title.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "PROPERTIES"));
                let title =
                    title.on_hover_cursor(CursorIcon::Grab).on_hover_text("Drag to move; drop at a side to dock");
                if title.drag_started()
                    && let Some(at) = title.interact_pointer_pos()
                {
                    // Docked, it floats from where it was.
                    let corner = if self.settings.properties_dock == Dock::Float {
                        self.properties_pos
                    } else {
                        ui.max_rect().left_top() - vec2(8.0, 8.0)
                    };
                    self.properties_grab = Some(at - corner);
                    if self.settings.properties_dock != Dock::Float {
                        self.properties_pos = corner;
                        self.set_properties_dock(ui.ctx(), Dock::Float);
                    }
                }
            });
        });
        let selected: Vec<Entity> =
            self.engine.selection().iter().filter_map(|&id| self.doc.entity(id).cloned()).collect();
        ui.label(RichText::new(selection_title(&selected)).strong());
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Every value as wide as the palette leaves them, so they line up.
            let width = (ui.available_width() - LABEL_WIDTH - 12.0).max(MIN_VALUE_WIDTH);
            section(ui, "General");
            self.general_rows(ui, &selected, width);
            if let [e] = selected.as_slice() {
                section(ui, "Geometry");
                self.geometry_rows(ui, e, width);
            }
        });
    }

    fn general_rows(&mut self, ui: &mut Ui, selected: &[Entity], width: f32) {
        egui::Grid::new("props_general").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            // Color
            label_cell(ui, "Color");
            let common_color = common(selected.iter().map(|e| e.color));
            let text = match (selected.is_empty(), common_color) {
                (true, _) | (false, Some(Color::ByLayer)) => "ByLayer".to_string(),
                (false, Some(Color::ByBlock)) => "ByBlock".to_string(),
                (false, Some(Color::Index(i))) => color_name(i),
                (false, None) => VARIES.to_string(),
            };
            let mut chosen = None;
            ui.add_enabled_ui(!selected.is_empty(), |ui| {
                let shown =
                    egui::ComboBox::from_id_salt("props_color").width(width).selected_text(text).show_ui(ui, |ui| {
                        if ui.selectable_label(common_color == Some(Color::ByLayer), "ByLayer").clicked() {
                            chosen = Some(Color::ByLayer);
                        }
                        if ui.selectable_label(common_color == Some(Color::ByBlock), "ByBlock").clicked() {
                            chosen = Some(Color::ByBlock);
                        }
                        for &(index, name) in NAMED_COLORS {
                            ui.horizontal(|ui| {
                                swatch(ui, index);
                                if ui.selectable_label(common_color == Some(Color::Index(index)), name).clicked() {
                                    chosen = Some(Color::Index(index));
                                }
                            });
                        }
                    });
                shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Color"));
            });
            if let Some(color) = chosen {
                self.edit(selected.iter().map(|e| e.id), |e| e.color = color);
            }
            ui.end_row();

            // Layer: with nothing selected it sets the current layer, as in AutoCAD.
            label_cell(ui, "Layer");
            let common_layer = if selected.is_empty() {
                Some(self.doc.current_layer().to_string())
            } else {
                common(selected.iter().map(|e| e.layer.clone()))
            };
            let names: Vec<String> = self.doc.layers().iter().map(|l| l.name.clone()).collect();
            let mut chosen = None;
            let shown = egui::ComboBox::from_id_salt("props_layer")
                .width(width)
                .truncate()
                .selected_text(common_layer.clone().unwrap_or_else(|| VARIES.into()))
                .show_ui(ui, |ui| {
                    for name in names {
                        if ui.selectable_label(common_layer.as_deref() == Some(name.as_str()), &name).clicked() {
                            chosen = Some(name);
                        }
                    }
                });
            shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Layer"));
            if let Some(layer) = chosen {
                if selected.is_empty() {
                    self.doc.set_current_layer(&layer);
                } else {
                    self.edit(selected.iter().map(|e| e.id), |e| e.layer = layer.clone());
                }
            }
            ui.end_row();

            // Linetype and lineweight; new objects are always ByLayer.
            label_cell(ui, "Linetype");
            let names: Vec<String> = [linetype::BYLAYER, linetype::BYBLOCK]
                .into_iter()
                .map(String::from)
                .chain(self.doc.linetypes().into_iter().map(|l| l.name))
                .collect();
            let common_linetype = if selected.is_empty() {
                Some(linetype::BYLAYER.into())
            } else {
                common(selected.iter().map(|e| e.linetype.clone()))
            };
            let picked = ui.add_enabled_ui(!selected.is_empty(), |ui| {
                linetype_menu(ui, common_linetype.as_deref(), &names, "props_linetype", width)
            });
            if let Some(linetype) = picked.inner {
                self.edit(selected.iter().map(|e| e.id), |e| e.linetype = linetype.clone());
            }
            ui.end_row();

            label_cell(ui, "Lineweight");
            let common_weight = if selected.is_empty() {
                Some(Lineweight::ByLayer)
            } else {
                common(selected.iter().map(|e| e.lineweight))
            };
            let special = [Lineweight::ByLayer, Lineweight::ByBlock, Lineweight::Default];
            let picked = ui.add_enabled_ui(!selected.is_empty(), |ui| {
                lineweight_menu(ui, common_weight, &special, "props_lineweight", width)
            });
            if let Some(lineweight) = picked.inner {
                self.edit(selected.iter().map(|e| e.id), |e| e.lineweight = lineweight);
            }
            ui.end_row();
        });
    }

    fn geometry_rows(&mut self, ui: &mut Ui, e: &Entity, width: f32) {
        let rows: Vec<(&str, Value)> = match &e.geometry {
            Geometry::Point { position } => {
                vec![("Position X", Value::Number(position.x)), ("Position Y", Value::Number(position.y))]
            }
            Geometry::Line { start, end } => {
                let d = *end - *start;
                vec![
                    ("Start X", Value::Number(start.x)),
                    ("Start Y", Value::Number(start.y)),
                    ("End X", Value::Number(end.x)),
                    ("End Y", Value::Number(end.y)),
                    ("Delta X", Value::Text(fmt(d.x))),
                    ("Delta Y", Value::Text(fmt(d.y))),
                    ("Length", Value::Text(fmt(d.length()))),
                    ("Angle", Value::Text(format!("{:.0}°", start.angle_to(*end)))),
                ]
            }
            Geometry::Circle { center, radius } => vec![
                ("Center X", Value::Number(center.x)),
                ("Center Y", Value::Number(center.y)),
                ("Radius", Value::Number(*radius)),
                ("Diameter", Value::Number(radius * 2.0)),
                ("Circumference", Value::Text(fmt(e.geometry.length()))),
                ("Area", Value::Text(fmt(e.geometry.area().unwrap_or(0.0)))),
            ],
            Geometry::Arc { center, radius, start_angle, end_angle } => {
                let arc = ArcSeg::ccw(*center, *radius, *start_angle, *end_angle);
                vec![
                    ("Center X", Value::Number(center.x)),
                    ("Center Y", Value::Number(center.y)),
                    ("Radius", Value::Number(*radius)),
                    ("Start angle", Value::Angle(*start_angle)),
                    ("End angle", Value::Angle(*end_angle)),
                    ("Total angle", Value::Text(format!("{:.0}°", arc.sweep))),
                    ("Arc length", Value::Text(fmt(arc.length()))),
                ]
            }
            Geometry::Polyline { vertices, closed } => {
                let width = match oxidraft_model::constant_width(vertices) {
                    Some(w) => Value::Number(w),
                    None => Value::Text("*VARIES*".into()),
                };
                let mut rows = vec![
                    ("Closed", Value::Text(if *closed { "Yes" } else { "No" }.into())),
                    ("Vertices", Value::Text(vertices.len().to_string())),
                    ("Global width", width),
                    ("Length", Value::Text(fmt(e.geometry.length()))),
                ];
                if let Some(area) = e.geometry.area() {
                    rows.push(("Area", Value::Text(fmt(area))));
                }
                rows
            }
            Geometry::Ellipse(el) => {
                let mut rows = vec![
                    ("Center X", Value::Number(el.center.x)),
                    ("Center Y", Value::Number(el.center.y)),
                    ("Major radius", Value::Number(el.a())),
                    ("Minor radius", Value::Number(el.b())),
                    ("Radius ratio", Value::Text(fmt(el.ratio))),
                ];
                if el.is_full() {
                    rows.push(("Area", Value::Text(fmt(el.area()))));
                } else {
                    rows.push(("Start angle", Value::Text(format!("{:.0}°", el.center.angle_to(el.start_point())))));
                    rows.push(("End angle", Value::Text(format!("{:.0}°", el.center.angle_to(el.end_point())))));
                }
                rows.push(("Length", Value::Text(fmt(e.geometry.length()))));
                rows
            }
            Geometry::Spline(sp) => {
                let mut rows = vec![
                    ("Closed", Value::Text(if sp.is_closed() { "Yes" } else { "No" }.into())),
                    ("Degree", Value::Text(sp.degree().to_string())),
                    ("Control vertices", Value::Text(sp.control().len().to_string())),
                    ("Fit points", Value::Text(sp.fit().len().to_string())),
                    ("Length", Value::Text(fmt(e.geometry.length()))),
                ];
                if let Some(area) = e.geometry.area() {
                    rows.push(("Area", Value::Text(fmt(area))));
                }
                rows
            }
            Geometry::Image { image, origin, u, v, clip } => vec![
                ("Name", Value::Text(image.clone())),
                ("Position X", Value::Number(origin.x)),
                ("Position Y", Value::Number(origin.y)),
                ("Width", Value::Text(fmt(u.length()))),
                ("Height", Value::Text(fmt(v.length()))),
                ("Rotation", Value::Angle(Point::ORIGIN.angle_to(*u))),
                ("Clipped", Value::Text(if clip.len() >= 3 { "Yes" } else { "No" }.into())),
            ],
            Geometry::Viewport { center, width, height, view_center, view_height, locked, on, .. } => {
                // A locked viewport's scale can't change.
                let ratio = view_height / height.max(1e-12);
                let (standard, custom) = if *locked {
                    (Value::Text(scale_name(ratio).into()), Value::Text(fmt(1.0 / ratio)))
                } else {
                    (Value::Choice(SCALES, scale_name(ratio).into()), Value::Number(1.0 / ratio))
                };
                vec![
                    ("Center X", Value::Number(center.x)),
                    ("Center Y", Value::Number(center.y)),
                    ("Width", Value::Number(*width)),
                    ("Height", Value::Number(*height)),
                    ("On", yes_no(*on)),
                    ("Display locked", yes_no(*locked)),
                    ("Standard scale", standard),
                    ("Custom scale", custom),
                    ("View center X", Value::Text(fmt(view_center.x))),
                    ("View center Y", Value::Text(fmt(view_center.y))),
                ]
            }
            Geometry::Xline { base, direction } | Geometry::Ray { base, direction } => vec![
                ("Basepoint X", Value::Number(base.x)),
                ("Basepoint Y", Value::Number(base.y)),
                ("Angle", Value::Angle(Point::ORIGIN.angle_to(*direction))),
            ],
            Geometry::Insert { block, placement, attributes, .. } => {
                let mut rows = vec![
                    ("Name", Value::Text(block.clone())),
                    ("Position X", Value::Number(placement.position.x)),
                    ("Position Y", Value::Number(placement.position.y)),
                    ("Scale", Value::Number(placement.scale)),
                    ("Rotation", Value::Angle(placement.rotation)),
                    ("Mirrored", Value::Text(if placement.mirror { "Yes" } else { "No" }.into())),
                ];
                // Attribute values are editable by tag, as in AutoCAD's palette.
                rows.extend(attributes.iter().map(|a| (a.tag.as_str(), Value::Edit(a.value.clone()))));
                rows
            }
            Geometry::Text { position, height, rotation, width, content, .. } => vec![
                ("Contents", Value::Edit(content.clone())),
                ("Height", Value::Number(*height)),
                ("Rotation", Value::Angle(*rotation)),
                ("Width", Value::Number(*width)),
                ("Position X", Value::Number(position.x)),
                ("Position Y", Value::Number(position.y)),
            ],
            Geometry::Dimension { p1, p2, rotation, aligned, text, scale, .. } => {
                let value = measurement(*p1, *p2, *rotation, *aligned);
                let mut rows = vec![
                    ("Measurement", Value::Text(format_measurement(value))),
                    ("Text override", Value::Edit(text.clone().unwrap_or_default())),
                    ("Dim scale", Value::Number(*scale)),
                ];
                if !aligned {
                    rows.push(("Rotation", Value::Angle(*rotation)));
                }
                rows
            }
            Geometry::RadialDimension { center, point, diameter, text, scale, .. } => vec![
                ("Measurement", Value::Text(radial_value(*center, *point, *diameter))),
                ("Text override", Value::Edit(text.clone().unwrap_or_default())),
                ("Dim scale", Value::Number(*scale)),
            ],
            Geometry::AngularDimension { vertex, p1, p2, text, scale, .. } => vec![
                ("Measurement", Value::Text(format_angle(angular_measurement(*vertex, *p1, *p2)))),
                ("Text override", Value::Edit(text.clone().unwrap_or_default())),
                ("Dim scale", Value::Number(*scale)),
            ],
            Geometry::Leader { content, scale, .. } => {
                vec![("Contents", Value::Edit(content.clone())), ("Scale", Value::Number(*scale))]
            }
            Geometry::Hatch { loops, pattern, scale, angle } => {
                let polys = hatch::polygons(loops);
                let areas: Vec<f64> = polys.iter().map(|p| hatch::signed_area(p).abs()).collect();
                let area = areas.iter().copied().fold(0.0, f64::max) * 2.0 - areas.iter().sum::<f64>();
                vec![
                    ("Pattern", Value::Edit(pattern.name().to_string())),
                    ("Scale", Value::Number(*scale)),
                    ("Angle", Value::Angle(*angle)),
                    ("Area", Value::Text(fmt(area))),
                ]
            }
        };
        egui::Grid::new("props_geometry").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            for (label, value) in rows {
                label_cell(ui, label);
                let changed = match value {
                    Value::Text(t) => {
                        ui.colored_label(theme::text_dim(), t);
                        None
                    }
                    Value::Number(v) => {
                        self.number_field(ui, label, Id::unique(("prop", e.id, label)), v, false, width)
                    }
                    Value::Angle(v) => self.number_field(ui, label, Id::unique(("prop", e.id, label)), v, true, width),
                    Value::Choice(options, current) => {
                        let mut picked = None;
                        let shown = egui::ComboBox::from_id_salt(("prop", e.id, label))
                            .width(width)
                            .selected_text(current.as_str())
                            .show_ui(ui, |ui| {
                                for &o in options {
                                    if ui.selectable_label(o == current, o).clicked() && o != current {
                                        picked = Some(o);
                                    }
                                }
                            });
                        shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
                        if let Some(g) = picked.and_then(|o| with_text_property(&e.geometry, label, o)) {
                            self.edit([e.id], |x| x.geometry = g.clone());
                        }
                        None
                    }
                    Value::Edit(s) => {
                        if let Some(new) = self.string_field(ui, label, Id::unique(("prop", e.id, label)), &s, width) {
                            match with_text_property(&e.geometry, label, &new) {
                                Some(g) => self.edit([e.id], |x| x.geometry = g.clone()),
                                None => self.engine.log(format!("Invalid value for {label}.")),
                            }
                        }
                        None
                    }
                };
                if let Some(v) = changed {
                    let g = with_property(&e.geometry, label, v);
                    match g {
                        Some(g) => self.edit([e.id], |x| x.geometry = g.clone()),
                        None => self.engine.log(format!("Invalid value for {label}.")),
                    }
                }
                ui.end_row();
            }
        });
    }

    /// An editable number that applies on Enter or when it loses focus.
    fn number_field(&mut self, ui: &mut Ui, label: &str, id: Id, value: f64, angle: bool, width: f32) -> Option<f64> {
        let shown = if angle { format!("{value:.0}") } else { fmt(value) };
        let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or(shown);
        let response = ui.add(text_field(&mut text, id, width));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
        self.text_fields.push(id);
        if response.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
            return None;
        }
        ui.data_mut(|d| d.remove::<String>(id));
        // Esc leaves the field as it was.
        if response.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            let v: f64 = text.trim().trim_end_matches('°').parse().ok()?;
            return ((v - value).abs() > 1e-12).then_some(v);
        }
        None
    }

    /// An editable string that applies on Enter or when it loses focus.
    fn string_field(&mut self, ui: &mut Ui, label: &str, id: Id, value: &str, width: f32) -> Option<String> {
        let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string());
        let response = ui.add(text_field(&mut text, id, width));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
        self.text_fields.push(id);
        if response.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
            return None;
        }
        ui.data_mut(|d| d.remove::<String>(id));
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        (response.lost_focus() && !escaped && text != value).then_some(text)
    }

    /// Applies one undoable edit to several objects.
    pub(crate) fn edit(&mut self, ids: impl IntoIterator<Item = EntityId>, f: impl Fn(&mut Entity)) {
        self.doc.begin("PROPERTIES");
        for id in ids {
            self.doc.update(id, &f);
        }
        self.doc.commit();
    }
}

/// The geometry with one property changed, or `None` if the value is invalid.
fn with_property(g: &Geometry, label: &str, v: f64) -> Option<Geometry> {
    let mut g = g.clone();
    match (&mut g, label) {
        (Geometry::Point { position }, "Position X") => position.x = v,
        (Geometry::Point { position }, "Position Y") => position.y = v,
        (Geometry::Line { start, .. }, "Start X") => start.x = v,
        (Geometry::Line { start, .. }, "Start Y") => start.y = v,
        (Geometry::Line { end, .. }, "End X") => end.x = v,
        (Geometry::Line { end, .. }, "End Y") => end.y = v,
        (Geometry::Circle { center, .. } | Geometry::Arc { center, .. }, "Center X") => center.x = v,
        (Geometry::Circle { center, .. } | Geometry::Arc { center, .. }, "Center Y") => center.y = v,
        (Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. }, "Radius") if v > 0.0 => *radius = v,
        (Geometry::Circle { radius, .. }, "Diameter") if v > 0.0 => *radius = v / 2.0,
        (Geometry::Arc { start_angle, .. }, "Start angle") => *start_angle = v.rem_euclid(360.0),
        (Geometry::Arc { end_angle, .. }, "End angle") => *end_angle = v.rem_euclid(360.0),
        (Geometry::Insert { placement, .. }, "Position X") => placement.position = Point::new(v, placement.position.y),
        (Geometry::Insert { placement, .. }, "Position Y") => placement.position = Point::new(placement.position.x, v),
        (Geometry::Insert { placement, .. }, "Scale") if v > 0.0 => placement.scale = v,
        (Geometry::Insert { placement, .. }, "Rotation") => placement.rotation = v.rem_euclid(360.0),
        (Geometry::Text { height, .. }, "Height") if v > 0.0 => *height = v,
        (Geometry::Text { rotation, .. }, "Rotation") => *rotation = v.rem_euclid(360.0),
        (Geometry::Text { width, .. }, "Width") if v >= 0.0 => *width = v,
        (Geometry::Text { position, .. }, "Position X") => position.x = v,
        (Geometry::Text { position, .. }, "Position Y") => position.y = v,
        (
            Geometry::Dimension { scale, .. }
            | Geometry::RadialDimension { scale, .. }
            | Geometry::AngularDimension { scale, .. },
            "Dim scale",
        ) if v > 0.0 => *scale = v,
        (Geometry::Dimension { rotation, .. }, "Rotation") => *rotation = v.rem_euclid(360.0),
        (Geometry::Leader { scale, .. } | Geometry::Hatch { scale, .. }, "Scale") if v > 0.0 => *scale = v,
        (Geometry::Hatch { angle, .. }, "Angle") => *angle = v.rem_euclid(360.0),
        (Geometry::Polyline { vertices, .. }, "Global width") if v >= 0.0 => {
            vertices.iter_mut().for_each(|x| *x = x.with_widths(v, v));
        }
        (Geometry::Ellipse(e), "Center X") => e.center.x = v,
        (Geometry::Ellipse(e), "Center Y") => e.center.y = v,
        (Geometry::Ellipse(e), "Major radius") if v > 0.0 => *e = e.with_axes(v, e.b()),
        (Geometry::Ellipse(e), "Minor radius") if v > 0.0 => *e = e.with_axes(e.a(), v),
        (Geometry::Image { origin, .. }, "Position X" | "Position Y") => {
            let o = *origin;
            let to = if label == "Position X" { Point::new(v, o.y) } else { Point::new(o.x, v) };
            return Some(g.translated(to - o));
        }
        (Geometry::Image { origin, u, .. }, "Rotation") => {
            let (o, turn) = (*origin, v - Point::ORIGIN.angle_to(*u));
            return Some(g.rotated(o, turn));
        }
        (Geometry::Viewport { center, .. }, "Center X" | "Center Y") => {
            let c = *center;
            let to = if label == "Center X" { Point::new(v, c.y) } else { Point::new(c.x, v) };
            return Some(g.translated(to - c));
        }
        (Geometry::Viewport { center, height, .. }, "Width") if v > 0.0 => {
            let (c, h) = (*center, *height);
            return Some(g.reframed(c, v, h));
        }
        (Geometry::Viewport { center, width, .. }, "Height") if v > 0.0 => {
            let (c, w) = (*center, *width);
            return Some(g.reframed(c, w, v));
        }
        (Geometry::Viewport { height, view_height, locked: false, .. }, "Custom scale") if v > 0.0 => {
            *view_height = *height / v;
        }
        (Geometry::Xline { base, .. } | Geometry::Ray { base, .. }, "Basepoint X") => base.x = v,
        (Geometry::Xline { base, .. } | Geometry::Ray { base, .. }, "Basepoint Y") => base.y = v,
        (Geometry::Xline { direction, .. } | Geometry::Ray { direction, .. }, "Angle") => {
            *direction = Point::ORIGIN.polar(direction.length().max(1.0), v);
        }
        _ => return None,
    }
    Some(g)
}

/// The geometry with one text property changed, or `None` if it's invalid.
fn with_text_property(g: &Geometry, label: &str, s: &str) -> Option<Geometry> {
    let mut g = g.clone();
    if let Geometry::Insert { attributes, .. } = &mut g {
        let a = attributes.iter_mut().find(|a| a.tag == label)?;
        a.value = s.to_string();
        return Some(g);
    }
    match (&mut g, label) {
        (Geometry::Text { content, .. } | Geometry::Leader { content, .. }, "Contents") => *content = s.to_string(),
        (
            Geometry::Dimension { text, .. }
            | Geometry::RadialDimension { text, .. }
            | Geometry::AngularDimension { text, .. },
            "Text override",
        ) => {
            *text = Some(s.to_string()).filter(|t| !t.is_empty());
        }
        (Geometry::Hatch { pattern, .. }, "Pattern") => *pattern = HatchPattern::from_name(s)?,
        (Geometry::Viewport { on, .. }, "On") => *on = s == "Yes",
        (Geometry::Viewport { locked, .. }, "Display locked") => *locked = s == "Yes",
        (Geometry::Viewport { height, view_height, locked: false, .. }, "Standard scale") => {
            *view_height = *height * scale_ratio(s)?;
        }
        _ => return None,
    }
    Some(g)
}

/// "No selection", "Line (1)", or "All (3)" for mixed types.
fn selection_title(selected: &[Entity]) -> String {
    match common(selected.iter().map(|e| e.geometry.name())) {
        _ if selected.is_empty() => "No selection".into(),
        Some(name) => format!("{name} ({})", selected.len()),
        None => format!("All ({})", selected.len()),
    }
}

/// The value all items share, or `None` if they differ (or there are none).
pub(crate) fn common<T: PartialEq>(mut items: impl Iterator<Item = T>) -> Option<T> {
    let first = items.next()?;
    items.all(|x| x == first).then_some(first)
}

/// A property's name, as wide as every other (a long one cut short).
fn label_cell(ui: &mut Ui, text: &str) {
    ui.allocate_ui_with_layout(vec2(LABEL_WIDTH, 20.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(LABEL_WIDTH);
        ui.add(egui::Label::new(text).truncate());
    });
}

/// A text field `width` wide overall, like the dropdowns beside it.
fn text_field<'a>(text: &'a mut String, id: Id, width: f32) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).id(id).margin(egui::Margin::symmetric(4, 2)).desired_width(width)
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong().color(theme::accent()));
    ui.add_space(2.0);
}

fn fmt(v: f64) -> String {
    format!("{v:.4}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_line_and_circle_properties() {
        let line = Geometry::Line { start: Point::ORIGIN, end: Point::new(1.0, 1.0) };
        assert_eq!(
            with_property(&line, "End X", 5.0),
            Some(Geometry::Line { start: Point::ORIGIN, end: Point::new(5.0, 1.0) })
        );
        let circle = Geometry::Circle { center: Point::ORIGIN, radius: 1.0 };
        assert_eq!(
            with_property(&circle, "Diameter", 6.0),
            Some(Geometry::Circle { center: Point::ORIGIN, radius: 3.0 })
        );
        assert_eq!(with_property(&circle, "Radius", -1.0), None);
        assert_eq!(with_property(&circle, "End X", 1.0), None);
    }

    #[test]
    fn edits_annotation_properties() {
        let hatch = Geometry::Hatch { loops: Vec::new(), pattern: HatchPattern::Ansi31, scale: 1.0, angle: 0.0 };
        assert!(matches!(
            with_text_property(&hatch, "Pattern", "solid"),
            Some(Geometry::Hatch { pattern: HatchPattern::Solid, .. })
        ));
        assert_eq!(with_text_property(&hatch, "Pattern", "nope"), None);
        let dim = Geometry::Dimension {
            p1: Point::ORIGIN,
            p2: Point::new(1.0, 0.0),
            line: Point::new(0.0, 1.0),
            rotation: 0.0,
            aligned: false,
            text: Some("X".into()),
            scale: 1.0,
        };
        assert!(matches!(with_text_property(&dim, "Text override", ""), Some(Geometry::Dimension { text: None, .. })));
    }

    #[test]
    fn edits_viewport_properties() {
        let vp = Geometry::Viewport {
            center: Point::new(100.0, 50.0),
            width: 200.0,
            height: 100.0,
            view_center: Point::new(1000.0, 500.0),
            view_height: 5000.0,
            twist: 0.0,
            frozen: Vec::new(),
            locked: false,
            on: true,
        };
        let get = |g: Option<Geometry>| match g {
            Some(Geometry::Viewport { center, width, height, view_center, view_height, locked, on, .. }) => {
                (center, width, height, view_center, view_height, locked, on)
            }
            g => panic!("{g:?}"),
        };
        assert_eq!(scale_name(50.0), "1:50");
        assert_eq!(scale_name(33.0), "Custom");
        // A standard scale keeps the view's center.
        let (_, _, _, c, h, ..) = get(with_text_property(&vp, "Standard scale", "1:100"));
        assert_eq!((c, h), (Point::new(1000.0, 500.0), 10_000.0));
        assert_eq!(get(with_property(&vp, "Custom scale", 0.1)).4, 1000.0);
        // Wider: more shows at the same scale.
        let (_, w, _, c, h, ..) = get(with_property(&vp, "Width", 300.0));
        assert_eq!((w, c, h), (300.0, Point::new(1000.0, 500.0), 5000.0));
        let (_, _, ht, _, h, ..) = get(with_property(&vp, "Height", 50.0));
        assert_eq!((ht, h), (50.0, 2500.0));
        // Locked: the scale stays.
        let locked = with_text_property(&vp, "Display locked", "Yes").unwrap();
        assert!(get(Some(locked.clone())).5);
        assert_eq!(with_text_property(&locked, "Standard scale", "1:100"), None);
        assert!(!get(with_text_property(&vp, "On", "No")).6);
    }

    #[test]
    fn titles_and_common_values() {
        assert_eq!(selection_title(&[]), "No selection");
        assert_eq!(common([1, 1, 1].into_iter()), Some(1));
        assert_eq!(common([1, 2].into_iter()), None);
    }
}

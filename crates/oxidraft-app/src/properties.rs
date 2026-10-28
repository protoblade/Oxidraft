//! The Properties palette (PROPERTIES, Ctrl/Cmd+1): general and geometric
//! properties of the selected objects, editable in place.

use eframe::egui::{self, Frame, Id, Margin, RichText, Ui};
use oxidraft_geom::{Point, curve::ArcSeg};
use oxidraft_model::{Color, Entity, EntityId, Geometry};

use crate::{
    app::OxidraftApp,
    layers::{NAMED_COLORS, color_name, swatch},
    theme,
};

const VARIES: &str = "*VARIES*";

/// A property row's value: read-only text, or a number that can be edited.
enum Value {
    Text(String),
    Number(f64),
    Angle(f64),
}

impl OxidraftApp {
    pub(crate) fn properties_palette(&mut self, ui: &mut Ui) {
        if !self.properties_open {
            return;
        }
        let frame = Frame::NONE.fill(theme::RIBBON).inner_margin(Margin::same(8));
        egui::Panel::left("properties").frame(frame).resizable(true).default_size(270.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("PROPERTIES").strong().color(theme::TEXT_DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("x").frame(false)).on_hover_text("Close (Cmd+1)").clicked() {
                        self.properties_open = false;
                    }
                });
            });
            let selected: Vec<Entity> =
                self.engine.selection().iter().filter_map(|&id| self.doc.entity(id).cloned()).collect();
            ui.label(RichText::new(selection_title(&selected)).strong());
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                section(ui, "General");
                self.general_rows(ui, &selected);
                if let [e] = selected.as_slice() {
                    section(ui, "Geometry");
                    self.geometry_rows(ui, e);
                }
            });
        });
    }

    fn general_rows(&mut self, ui: &mut Ui, selected: &[Entity]) {
        egui::Grid::new("props_general").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            // Color
            ui.label("Color");
            let common_color = common(selected.iter().map(|e| e.color));
            let text = match (selected.is_empty(), common_color) {
                (true, _) | (false, Some(Color::ByLayer)) => "ByLayer".to_string(),
                (false, Some(Color::Index(i))) => color_name(i),
                (false, None) => VARIES.to_string(),
            };
            let mut chosen = None;
            ui.add_enabled_ui(!selected.is_empty(), |ui| {
                let shown =
                    egui::ComboBox::from_id_salt("props_color").width(150.0).selected_text(text).show_ui(ui, |ui| {
                        if ui.selectable_label(common_color == Some(Color::ByLayer), "ByLayer").clicked() {
                            chosen = Some(Color::ByLayer);
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
            ui.label("Layer");
            let common_layer = if selected.is_empty() {
                Some(self.doc.current_layer().to_string())
            } else {
                common(selected.iter().map(|e| e.layer.clone()))
            };
            let names: Vec<String> = self.doc.layers().iter().map(|l| l.name.clone()).collect();
            let mut chosen = None;
            let shown = egui::ComboBox::from_id_salt("props_layer")
                .width(150.0)
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
        });
    }

    fn geometry_rows(&mut self, ui: &mut Ui, e: &Entity) {
        let rows: Vec<(&str, Value)> = match &e.geometry {
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
                let mut rows = vec![
                    ("Closed", Value::Text(if *closed { "Yes" } else { "No" }.into())),
                    ("Vertices", Value::Text(vertices.len().to_string())),
                    ("Length", Value::Text(fmt(e.geometry.length()))),
                ];
                if let Some(area) = e.geometry.area() {
                    rows.push(("Area", Value::Text(fmt(area))));
                }
                rows
            }
            Geometry::Insert { block, placement } => vec![
                ("Name", Value::Text(block.clone())),
                ("Position X", Value::Number(placement.position.x)),
                ("Position Y", Value::Number(placement.position.y)),
                ("Scale", Value::Number(placement.scale)),
                ("Rotation", Value::Angle(placement.rotation)),
                ("Mirrored", Value::Text(if placement.mirror { "Yes" } else { "No" }.into())),
            ],
        };
        egui::Grid::new("props_geometry").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            for (label, value) in rows {
                ui.label(label);
                let changed = match value {
                    Value::Text(t) => {
                        ui.colored_label(theme::TEXT_DIM, t);
                        None
                    }
                    Value::Number(v) => self.number_field(ui, Id::unique(("prop", e.id, label)), v, false),
                    Value::Angle(v) => self.number_field(ui, Id::unique(("prop", e.id, label)), v, true),
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
    fn number_field(&mut self, ui: &mut Ui, id: Id, value: f64, angle: bool) -> Option<f64> {
        let shown = if angle { format!("{value:.0}") } else { fmt(value) };
        let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or(shown);
        let response = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(140.0));
        self.text_fields.push(id);
        if response.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
            return None;
        }
        ui.data_mut(|d| d.remove::<String>(id));
        if response.lost_focus() {
            let v: f64 = text.trim().trim_end_matches('°').parse().ok()?;
            return ((v - value).abs() > 1e-12).then_some(v);
        }
        None
    }

    /// Applies one undoable edit to several objects.
    fn edit(&mut self, ids: impl IntoIterator<Item = EntityId>, f: impl Fn(&mut Entity)) {
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
fn common<T: PartialEq>(mut items: impl Iterator<Item = T>) -> Option<T> {
    let first = items.next()?;
    items.all(|x| x == first).then_some(first)
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong().color(theme::ACCENT));
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
    fn titles_and_common_values() {
        assert_eq!(selection_title(&[]), "No selection");
        assert_eq!(common([1, 1, 1].into_iter()), Some(1));
        assert_eq!(common([1, 2].into_iter()), None);
    }
}

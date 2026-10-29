//! DIMSTYLE's Dimension Style dialog, and FIND's Find and Replace.

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::{EntityId, Geometry};

use crate::{app::OxidraftApp, theme};

/// The Find and Replace dialog's fields.
#[derive(Default)]
pub(crate) struct FindReplace {
    find: String,
    replace: String,
    match_case: bool,
    /// What the last Find found.
    found: Option<usize>,
}

/// The texts an object shows that FIND looks in, and a way to change them.
fn texts_of(g: &Geometry) -> Vec<String> {
    match g {
        Geometry::Text { content, .. } | Geometry::Leader { content, .. } => vec![content.clone()],
        Geometry::Insert { attributes, .. } => attributes.iter().map(|a| a.value.clone()).collect(),
        Geometry::Dimension { text: Some(t), .. }
        | Geometry::RadialDimension { text: Some(t), .. }
        | Geometry::AngularDimension { text: Some(t), .. } => vec![t.clone()],
        Geometry::Table { cells, .. } => cells.iter().flatten().cloned().collect(),
        Geometry::AttributeDefinition { default, .. } => vec![default.clone()],
        _ => Vec::new(),
    }
}

fn replace_in(g: &mut Geometry, f: &dyn Fn(&str) -> String) {
    match g {
        Geometry::Text { content, .. } | Geometry::Leader { content, .. } => *content = f(content),
        Geometry::Insert { attributes, .. } => attributes.iter_mut().for_each(|a| a.value = f(&a.value)),
        Geometry::Dimension { text: Some(t), .. }
        | Geometry::RadialDimension { text: Some(t), .. }
        | Geometry::AngularDimension { text: Some(t), .. } => *t = f(t),
        Geometry::Table { cells, .. } => cells.iter_mut().flatten().for_each(|c| *c = f(c)),
        Geometry::AttributeDefinition { default, .. } => *default = f(default),
        _ => {}
    }
}

impl FindReplace {
    fn matches(&self, text: &str) -> bool {
        if self.find.is_empty() {
            return false;
        }
        if self.match_case {
            text.contains(&self.find)
        } else {
            text.to_lowercase().contains(&self.find.to_lowercase())
        }
    }

    fn replaced(&self, text: &str) -> String {
        if self.match_case {
            return text.replace(&self.find, &self.replace);
        }
        // Case-insensitive: find each match in the lower-cased text.
        let (lower, find) = (text.to_lowercase(), self.find.to_lowercase());
        let mut out = String::new();
        let mut rest = 0;
        while let Some(i) = lower[rest..].find(&find) {
            out.push_str(&text[rest..rest + i]);
            out.push_str(&self.replace);
            rest += i + find.len();
        }
        out.push_str(&text[rest..]);
        out
    }
}

impl OxidraftApp {
    pub(crate) fn open_dim_style(&mut self) {
        self.dim_style_dialog = Some((self.doc.dim_style(), self.engine.vars.dim_scale));
    }

    pub(crate) fn dim_style_dialog(&mut self, ctx: &egui::Context) {
        let Some((mut style, mut scale)) = self.dim_style_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Dimension Style - Standard")
            .id(egui::Id::unique("dim_style"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 180.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Lines, symbols and text").strong());
                egui::Grid::new("dim_style_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let mut number = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
                        ui.label(label);
                        let r = ui.add(egui::DragValue::new(v).range(0.0..=f64::MAX).speed(0.05).max_decimals(4));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                        fields.push(r.id);
                        ui.end_row();
                    };
                    number(ui, "Text height:", &mut style.text_height);
                    number(ui, "Arrow size:", &mut style.arrow_size);
                    number(ui, "Offset from origin:", &mut style.ext_offset);
                    number(ui, "Extend beyond dim lines:", &mut style.ext_beyond);
                    number(ui, "Offset from dim line:", &mut style.gap);
                    number(ui, "Overall scale (DIMSCALE):", &mut scale);
                    ui.label("Precision (decimals):");
                    let r = ui.add(egui::DragValue::new(&mut style.decimals).range(0..=8));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Precision"));
                    let precision = r.id;
                    ui.end_row();
                    fields.push(precision);
                });
                ui.colored_label(theme::text_dim(), "Existing dimensions take the new style; new ones the scale too.");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            style.text_height = style.text_height.max(1e-6);
            self.doc.begin("DIMSTYLE");
            self.doc.set_dim_style(style);
            self.doc.commit();
            if scale > 0.0 {
                self.engine.vars.dim_scale = scale;
            }
        } else if open && !cancel {
            self.dim_style_dialog = Some((style, scale));
        }
    }

    pub(crate) fn find_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.find_replace.take() else { return };
        let (mut open, mut find, mut replace_all, mut close) = (true, false, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Find and Replace")
            .id(egui::Id::unique("find_replace"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 180.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                egui::Grid::new("find_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Find what:");
                    let f = ui.add(egui::TextEdit::singleline(&mut d.find).desired_width(220.0));
                    f.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Find what"));
                    ui.end_row();
                    ui.label("Replace with:");
                    let r = ui.add(egui::TextEdit::singleline(&mut d.replace).desired_width(220.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Replace with"));
                    ui.end_row();
                    fields.extend([f.id, r.id]);
                });
                ui.checkbox(&mut d.match_case, "Match case");
                if let Some(n) = d.found {
                    ui.colored_label(theme::text_dim(), format!("{n} object(s) found."));
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                    replace_all = ui.add_enabled(!d.find.is_empty(), egui::Button::new("Replace All")).clicked();
                    find = ui.add_enabled(!d.find.is_empty(), egui::Button::new("Find")).clicked();
                });
            });
        self.text_fields.extend(fields);
        let hits: Vec<EntityId> = if find || replace_all {
            self.doc.entities().filter(|e| texts_of(&e.geometry).iter().any(|t| d.matches(t))).map(|e| e.id).collect()
        } else {
            Vec::new()
        };
        if find {
            // The objects found are selected, and the first one is shown.
            d.found = Some(hits.len());
            if let Some(b) = hits
                .iter()
                .filter_map(|id| self.doc.entity(*id))
                .map(|e| self.doc.entity_bounds(e))
                .reduce(oxidraft_geom::Aabb::union)
            {
                self.pending_views.push(oxidraft_commands::ViewRequest::Window(b));
            }
            self.engine.set_selection(hits.clone());
        }
        if replace_all {
            self.doc.begin("FIND");
            for id in &hits {
                self.doc.update(*id, |e| replace_in(&mut e.geometry, &|t| d.replaced(t)));
            }
            self.doc.commit();
            self.engine.log(format!("{} object(s) changed.", hits.len()));
            d.found = Some(hits.len());
        }
        if open && !close {
            self.find_replace = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_replaces_ignoring_case() {
        let d = FindReplace { find: "door".into(), replace: "Gate".into(), match_case: false, found: None };
        assert!(d.matches("Front DOOR"));
        assert_eq!(d.replaced("Door D1, door D2"), "Gate D1, Gate D2");
        let exact = FindReplace { match_case: true, ..d };
        assert!(!exact.matches("Front DOOR"));
    }
}

//! The Quick Select dialog (QSELECT): select objects by type and property.

use eframe::egui::{self, Ui};
use oxidraft_commands::{QuickOperator, QuickProperty, QuickSelect, object_type};

use crate::{app::OxidraftApp, layers::color_name};

/// A Quick Select value as the dialog shows it: color numbers by name.
fn shown(property: QuickProperty, value: &str) -> String {
    match (property, value.parse::<u8>()) {
        (QuickProperty::Color, Ok(i)) => color_name(i),
        _ => value.to_string(),
    }
}

/// A dropdown of `items` as (value, text); sets `value` to the one picked.
fn dropdown(ui: &mut Ui, label: &str, value: &mut String, items: &[(String, String)]) {
    let text = items.iter().find(|i| i.0 == *value).map_or(value.clone(), |i| i.1.clone());
    let shown = egui::ComboBox::from_id_salt(label).width(200.0).selected_text(text).show_ui(ui, |ui| {
        for (v, t) in items {
            ui.selectable_value(value, v.clone(), t);
        }
    });
    shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
}

impl OxidraftApp {
    /// Opens Quick Select, filtering the current selection if there is one.
    pub(crate) fn open_quick_select(&mut self) {
        self.quick_select = Some(QuickSelect {
            in_selection: !self.engine.selection().is_empty(),
            object_type: None,
            property: QuickProperty::Layer,
            operator: QuickOperator::Equals,
            value: String::new(),
            exclude: false,
            append: false,
        });
    }

    pub(crate) fn quick_select_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut q) = self.quick_select.take() else { return };
        let current = self.engine.selection().to_vec();
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Quick Select")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([340.0, 220.0])
            .show(ctx, |ui| {
                egui::Grid::new("qselect").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Apply to:");
                    let mut scope = if q.in_selection { "selection" } else { "drawing" }.to_string();
                    let mut scopes = vec![("drawing".to_string(), "Entire drawing".to_string())];
                    if !current.is_empty() {
                        scopes.push(("selection".into(), "Current selection".into()));
                    }
                    dropdown(ui, "Apply to", &mut scope, &scopes);
                    q.in_selection = scope == "selection";
                    ui.end_row();

                    // Types and values come from the objects the filter looks at.
                    ui.label("Object type:");
                    let any_type = QuickSelect { object_type: None, ..q.clone() };
                    let mut types: Vec<String> = any_type
                        .candidates(&self.doc, &current)
                        .map(|e| object_type(&e.geometry).to_string())
                        .collect();
                    types.sort();
                    types.dedup();
                    let mut items = vec![(String::new(), "Multiple".to_string())];
                    items.extend(types.into_iter().map(|t| (t.clone(), t)));
                    let mut chosen = q.object_type.clone().unwrap_or_default();
                    dropdown(ui, "Object type", &mut chosen, &items);
                    q.object_type = Some(chosen).filter(|t| !t.is_empty());
                    ui.end_row();

                    ui.label("Properties:");
                    let mut property = q.property.label().to_string();
                    let items: Vec<(String, String)> =
                        QuickProperty::ALL.iter().map(|p| (p.label().to_string(), p.label().to_string())).collect();
                    dropdown(ui, "Properties", &mut property, &items);
                    q.property = QuickProperty::ALL.into_iter().find(|p| p.label() == property).unwrap_or(q.property);
                    ui.end_row();

                    ui.label("Operator:");
                    let mut operator = q.operator.label().to_string();
                    let items: Vec<(String, String)> =
                        QuickOperator::ALL.iter().map(|o| (o.label().to_string(), o.label().to_string())).collect();
                    dropdown(ui, "Operator", &mut operator, &items);
                    q.operator = QuickOperator::ALL.into_iter().find(|o| o.label() == operator).unwrap_or(q.operator);
                    ui.end_row();

                    ui.label("Value:");
                    let mut values: Vec<String> =
                        q.candidates(&self.doc, &current).map(|e| q.property.value(e)).collect();
                    values.sort_by_key(|v| v.to_ascii_lowercase());
                    values.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
                    if !values.iter().any(|v| v.eq_ignore_ascii_case(&q.value)) {
                        q.value = values.first().cloned().unwrap_or_default();
                    }
                    let items: Vec<(String, String)> =
                        values.iter().map(|v| (v.clone(), shown(q.property, v))).collect();
                    ui.add_enabled_ui(q.operator != QuickOperator::SelectAll, |ui| {
                        dropdown(ui, "Value", &mut q.value, &items);
                    });
                    ui.end_row();

                    ui.label("How to apply:");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut q.exclude, false, "Include in new selection set");
                        ui.radio_value(&mut q.exclude, true, "Exclude from new selection set");
                    });
                    ui.end_row();
                });
                ui.checkbox(&mut q.append, "Append to current selection set");
                ui.separator();
                ui.horizontal(|ui| {
                    ok = ui.button("OK").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if ok {
            if self.engine.is_active() {
                self.engine.cancel(&mut self.doc);
            }
            let ids = q.apply(&self.doc, &current);
            let noun = if ids.len() == 1 { "item" } else { "items" };
            self.engine.log(format!("{} {noun} selected.", ids.len()));
            self.engine.set_selection(ids);
        } else if open && !cancel {
            self.quick_select = Some(q);
        }
    }
}

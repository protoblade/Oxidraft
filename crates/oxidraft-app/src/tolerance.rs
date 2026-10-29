//! TOLERANCE: AutoCAD's Geometric Tolerance dialog — two rows of a
//! characteristic symbol, two tolerances (with a diameter sign and a
//! material condition) and three datums — then the frame is placed.

use eframe::egui::{self, Align, ComboBox, Layout, Ui, vec2};
use oxidraft_model::annotation::GDT_SYMBOLS;

use crate::app::OxidraftApp;

/// Material conditions: none, MMC, LMC, RFS.
const CONDITIONS: [(&str, Option<char>); 4] =
    [("", None), ("Ⓜ MMC", Some('m')), ("Ⓛ LMC", Some('l')), ("Ⓢ RFS", Some('s'))];

#[derive(Default, Clone)]
struct Value {
    diameter: bool,
    text: String,
    condition: Option<char>,
}

impl Value {
    /// As AutoCAD writes it, GD&T symbols as `{\Fgdt;x}`.
    fn code(&self) -> String {
        let mut s = String::new();
        if self.diameter {
            s.push_str("{\\Fgdt;n}");
        }
        s.push_str(self.text.trim());
        if let Some(c) = self.condition {
            s.push_str(&format!("{{\\Fgdt;{c}}}"));
        }
        if self.text.trim().is_empty() && !self.diameter { String::new() } else { s }
    }
}

#[derive(Default, Clone)]
struct Row {
    symbol: Option<char>,
    tolerances: [Value; 2],
    datums: [Value; 3],
}

impl Row {
    fn code(&self) -> String {
        let mut cells = vec![self.symbol.map_or(String::new(), |c| format!("{{\\Fgdt;{c}}}"))];
        cells.extend(self.tolerances.iter().map(Value::code));
        cells.extend(self.datums.iter().map(Value::code));
        while cells.last().is_some_and(String::is_empty) {
            cells.pop();
        }
        cells.join("%%v")
    }
}

#[derive(Default)]
pub(crate) struct ToleranceDialog {
    rows: [Row; 2],
}

/// The text field, and for tolerances the Ø toggle, and the condition.
fn value_cell(ui: &mut Ui, v: &mut Value, id: &str, diameter: bool, fields: &mut Vec<egui::Id>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if diameter {
            ui.toggle_value(&mut v.diameter, "Ø").on_hover_text("Diameter");
        }
        let r = ui.add(egui::TextEdit::singleline(&mut v.text).desired_width(52.0));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, id));
        fields.push(r.id);
        let shown = CONDITIONS.iter().find(|c| c.1 == v.condition).map_or("", |c| c.0);
        ComboBox::from_id_salt(format!("{id} condition")).width(56.0).selected_text(shown).show_ui(ui, |ui| {
            for (name, c) in CONDITIONS {
                ui.selectable_value(&mut v.condition, c, if name.is_empty() { "None" } else { name });
            }
        });
    });
}

impl OxidraftApp {
    pub(crate) fn open_tolerance(&mut self) {
        self.tolerance_dialog = Some(ToleranceDialog::default());
    }

    pub(crate) fn tolerance_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.tolerance_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Geometric Tolerance")
            .id(egui::Id::unique("tolerance_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([300.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                egui::Grid::new("tolerance_rows").spacing([10.0, 6.0]).show(ui, |ui| {
                    for title in ["Sym", "Tolerance 1", "Tolerance 2", "Datum 1", "Datum 2", "Datum 3"] {
                        ui.label(title);
                    }
                    ui.end_row();
                    for (r, row) in d.rows.iter_mut().enumerate() {
                        let shown = row.symbol.and_then(|c| GDT_SYMBOLS.iter().find(|s| s.0 == c)).map_or("", |s| s.1);
                        let combo =
                            ComboBox::from_id_salt(("symbol", r)).width(110.0).selected_text(shown).show_ui(ui, |ui| {
                                ui.selectable_value(&mut row.symbol, None, "None");
                                for (c, name) in GDT_SYMBOLS {
                                    ui.selectable_value(&mut row.symbol, Some(*c), *name);
                                }
                            });
                        let label = format!("Symbol {}", r + 1);
                        combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &label));
                        for (i, t) in row.tolerances.iter_mut().enumerate() {
                            value_cell(ui, t, &format!("Tolerance {} row {}", i + 1, r + 1), true, &mut fields);
                        }
                        for (i, t) in row.datums.iter_mut().enumerate() {
                            value_cell(ui, t, &format!("Datum {} row {}", i + 1, r + 1), false, &mut fields);
                        }
                        ui.end_row();
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
            let content: Vec<String> = d.rows.iter().map(Row::code).filter(|c| !c.is_empty()).collect();
            if !content.is_empty() {
                // TOLERANCE asks where the frame goes.
                self.engine.run("TOLERANCE", &mut self.doc);
                let tolerance = self.pick_tolerance();
                self.engine.submit(&content.join("^J"), &mut self.doc, oxidraft_geom::Point::ORIGIN, tolerance);
            }
            return;
        }
        if open && !cancel {
            self.tolerance_dialog = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_written_as_autocad_writes_them() {
        let row = Row {
            symbol: Some('j'),
            tolerances: [Value { diameter: true, text: "0.1".into(), condition: Some('m') }, Value::default()],
            datums: [
                Value { text: "A".into(), ..Value::default() },
                Value { text: "B".into(), ..Value::default() },
                Value::default(),
            ],
        };
        assert_eq!(row.code(), "{\\Fgdt;j}%%v{\\Fgdt;n}0.1{\\Fgdt;m}%%v%%vA%%vB");
    }
}

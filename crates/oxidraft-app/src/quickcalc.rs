//! QUICKCALC (QC, Cmd+8): AutoCAD's QuickCalc palette — an expression
//! line with a number pad and functions, the history of what was worked
//! out, a value to paste to the command line, and units conversion.

use eframe::egui::{self, RichText, vec2};
use oxidraft_commands::calc::{evaluate, show};
use oxidraft_model::units::INSERTION_UNITS;

use crate::{app::OxidraftApp, theme};

/// Angular units: name and degrees in one.
const ANGULAR: &[(&str, f64)] = &[("Degrees", 1.0), ("Radians", 180.0 / std::f64::consts::PI), ("Gradians", 0.9)];

#[derive(Default)]
pub(crate) struct QuickCalc {
    expression: String,
    /// Expressions worked out and their values, latest last.
    history: Vec<(String, String)>,
    error: Option<String>,
    angular: bool,
    from: usize,
    to: usize,
    value: f64,
}

impl QuickCalc {
    fn new() -> Self {
        // Millimeters to inches to start with.
        let at = |code| INSERTION_UNITS.iter().position(|u| u.0 == code).unwrap_or(0);
        Self { from: at(4), to: at(1), value: 1.0, ..Default::default() }
    }

    /// Works out the expression; the value replaces it, as on a calculator.
    fn equals(&mut self) {
        let text = self.expression.trim().to_string();
        if text.is_empty() {
            return;
        }
        match evaluate(&text) {
            Ok(v) => {
                let value = show(v);
                self.history.push((text, value.clone()));
                self.expression = value;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The units conversion's answer.
    fn converted(&self) -> f64 {
        if self.angular {
            let factor = |i: usize| ANGULAR.get(i).map_or(1.0, |a| a.1);
            self.value * factor(self.from) / factor(self.to)
        } else {
            let code = |i: usize| INSERTION_UNITS.get(i).map_or(0, |u| u.0);
            self.value * oxidraft_model::units::conversion(code(self.from), code(self.to))
        }
    }
}

/// The keys: what each shows, and what it types (None: an action).
const PAD: &[&[(&str, &str)]] = &[
    &[("C", ""), ("<-", ""), ("(", "("), (")", ")"), ("^", "^")],
    &[("7", "7"), ("8", "8"), ("9", "9"), ("/", "/"), ("sqrt", "sqrt(")],
    &[("4", "4"), ("5", "5"), ("6", "6"), ("*", "*"), ("sin", "sin(")],
    &[("1", "1"), ("2", "2"), ("3", "3"), ("-", "-"), ("cos", "cos(")],
    &[("0", "0"), (".", "."), ("=", ""), ("+", "+"), ("pi", "pi")],
];

impl OxidraftApp {
    pub(crate) fn toggle_quickcalc(&mut self) {
        self.quickcalc = match self.quickcalc.take() {
            Some(_) => None,
            None => Some(QuickCalc::new()),
        };
    }

    pub(crate) fn open_quickcalc(&mut self) {
        if self.quickcalc.is_none() {
            self.quickcalc = Some(QuickCalc::new());
        }
    }

    pub(crate) fn quickcalc_palette(&mut self, ctx: &egui::Context) {
        let Some(mut q) = self.quickcalc.take() else { return };
        let (mut open, mut paste) = (true, false);
        let mut fields = Vec::new();
        let r = self.dialog_area(ctx);
        egui::Window::new("QuickCalc")
            .id(egui::Id::unique("quickcalc"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([r.right() - 290.0, r.top() + 20.0])
            .constrain_to(r)
            .show(ctx, |ui| {
                ui.set_width(250.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_size(vec2(240.0, 80.0));
                    egui::ScrollArea::vertical().max_height(80.0).stick_to_bottom(true).show(ui, |ui| {
                        if q.history.is_empty() {
                            ui.colored_label(theme::text_dim(), "Worked-out expressions show here.");
                        }
                        let mut again = None;
                        for (e, v) in &q.history {
                            let line = ui.add(egui::Label::new(format!("{e} = {v}")).sense(egui::Sense::click()));
                            if line.on_hover_text("Click to use it again").clicked() {
                                again = Some(e.clone());
                            }
                        }
                        if let Some(e) = again {
                            q.expression = e;
                        }
                    });
                });
                let field = ui.add(
                    egui::TextEdit::singleline(&mut q.expression)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Expression"));
                fields.push(field.id);
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    q.equals();
                    field.request_focus();
                }
                if let Some(e) = &q.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                egui::Grid::new("quickcalc_pad").spacing([4.0, 4.0]).show(ui, |ui| {
                    for row in PAD {
                        for (label, typed) in *row {
                            if ui.add_sized(vec2(44.0, 22.0), egui::Button::new(*label)).clicked() {
                                match *label {
                                    "C" => (q.expression, q.error) = (String::new(), None),
                                    "<-" => {
                                        q.expression.pop();
                                    }
                                    "=" => q.equals(),
                                    _ => q.expression.push_str(typed),
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
                paste = ui.button("Paste value to command line").clicked();
                ui.separator();
                ui.label(RichText::new("Units Conversion").strong());
                ui.horizontal(|ui| {
                    ui.label("Type:");
                    for (angular, name) in [(false, "Length"), (true, "Angular")] {
                        if ui.selectable_label(q.angular == angular, name).clicked() && q.angular != angular {
                            q.angular = angular;
                            let start = QuickCalc::new();
                            (q.from, q.to) = if angular { (0, 1) } else { (start.from, start.to) };
                        }
                    }
                });
                let names: Vec<&str> = if q.angular {
                    ANGULAR.iter().map(|a| a.0).collect()
                } else {
                    INSERTION_UNITS.iter().skip(1).map(|u| u.1).collect()
                };
                // Length lists leave out Unitless.
                let offset = usize::from(!q.angular);
                for (label, index) in [("Convert from", &mut q.from), ("Convert to", &mut q.to)] {
                    ui.horizontal(|ui| {
                        ui.label(format!("{label}:"));
                        let shown = names.get(index.saturating_sub(offset)).copied().unwrap_or("");
                        egui::ComboBox::from_id_salt(label).width(150.0).selected_text(shown).show_ui(ui, |ui| {
                            for (i, n) in names.iter().enumerate() {
                                ui.selectable_value(index, i + offset, *n);
                            }
                        });
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Value to convert:");
                    let r = ui.add(egui::DragValue::new(&mut q.value).speed(0.1).max_decimals(8));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "Value to convert"));
                    fields.push(r.id);
                });
                ui.horizontal(|ui| {
                    ui.label("Converted value:");
                    let converted = show(q.converted());
                    ui.label(RichText::new(&converted).monospace());
                    if ui.small_button("Use").on_hover_text("Puts it on the expression line").clicked() {
                        q.expression.push_str(&converted);
                    }
                });
            });
        self.text_fields.extend(fields);
        if paste {
            if q.error.is_none() && evaluate(&q.expression).is_ok() {
                q.equals();
            }
            self.command_text.push_str(q.expression.trim());
        }
        if open {
            self.quickcalc = Some(q);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works_out_and_converts() {
        let mut q = QuickCalc::new();
        q.expression = "3 * (2 + 4)".into();
        q.equals();
        assert_eq!(q.expression, "18");
        assert_eq!(q.history, [("3 * (2 + 4)".to_string(), "18".to_string())]);
        q.value = 25.4;
        assert!((q.converted() - 1.0).abs() < 1e-12, "25.4 mm is an inch");
        q.angular = true;
        (q.from, q.to, q.value) = (0, 2, 90.0);
        assert!((q.converted() - 100.0).abs() < 1e-12, "90 degrees is 100 grads");
    }
}

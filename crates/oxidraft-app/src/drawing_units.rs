//! UNITS: AutoCAD's Drawing Units dialog — length and angle types and
//! precisions, clockwise angles, the direction of angle 0, and what a unit
//! is — with sample output as it's set.

use eframe::egui::{self, Align, ComboBox, Layout, RichText, vec2};
use oxidraft_model::{
    AngleType, LengthType, Units,
    units::{INSERTION_UNITS, insertion_name},
};

use crate::{app::OxidraftApp, theme};

/// What a length precision looks like for `t`.
fn length_pattern(t: LengthType, p: u8) -> String {
    let decimals = if p == 0 { String::new() } else { format!(".{}", "0".repeat(p.into())) };
    match t {
        LengthType::Decimal => format!("0{decimals}"),
        LengthType::Scientific => format!("0{decimals}E+01"),
        LengthType::Engineering => format!("0'-0{decimals}\""),
        LengthType::Architectural if p == 0 => "0'-0\"".into(),
        LengthType::Architectural => format!("0'-0 1/{}\"", 1u32 << p),
        LengthType::Fractional if p == 0 => "0".into(),
        LengthType::Fractional => format!("0 1/{}", 1u32 << p),
    }
}

/// What an angle precision looks like for `t`.
fn angle_pattern(t: AngleType, p: u8) -> String {
    let decimals = |n: u8| if n == 0 { String::new() } else { format!(".{}", "0".repeat(n.into())) };
    let dms = || match p {
        0 => "0d".to_string(),
        1 | 2 => "0d00'".into(),
        3 | 4 => "0d00'00\"".into(),
        _ => format!("0d00'00{}\"", decimals(p - 4)),
    };
    match t {
        AngleType::DecimalDegrees => format!("0{}", decimals(p)),
        AngleType::Grads => format!("0{}g", decimals(p)),
        AngleType::Radians => format!("0{}r", decimals(p)),
        AngleType::DegMinSec => dms(),
        AngleType::Surveyor => format!("N {} E", dms()),
    }
}

/// The precisions offered: each look once (the finest of those that look
/// alike).
fn choices(pattern: impl Fn(u8) -> String) -> Vec<(u8, String)> {
    let mut out: Vec<(u8, String)> = Vec::new();
    for p in 0..=8 {
        let s = pattern(p);
        match out.last_mut() {
            Some(last) if last.1 == s => last.0 = p,
            _ => out.push((p, s)),
        }
    }
    out
}

/// Names a dropdown for screen readers (and tests).
fn named(r: egui::InnerResponse<Option<()>>, name: &str) {
    r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, name));
}

/// The precision chosen, among those offered, nearest `p`.
fn offered(choices: &[(u8, String)], p: u8) -> u8 {
    choices.iter().map(|c| c.0).find(|&c| c >= p).unwrap_or_else(|| choices.last().map_or(0, |c| c.0))
}

impl OxidraftApp {
    pub(crate) fn open_units(&mut self) {
        self.units_dialog = Some(self.doc.units());
    }

    pub(crate) fn units_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut u) = self.units_dialog else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Drawing Units")
            .id(egui::Id::unique("units_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 140.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Length").strong());
                        ui.label("Type:");
                        named(
                            ComboBox::from_id_salt("length_type").width(150.0).selected_text(u.length.name()).show_ui(
                                ui,
                                |ui| {
                                    for t in LengthType::ALL {
                                        ui.selectable_value(&mut u.length, t, t.name());
                                    }
                                },
                            ),
                            "Length type",
                        );
                        let lengths = choices(|p| length_pattern(u.length, p));
                        u.length_precision = offered(&lengths, u.length_precision);
                        ui.label("Precision:");
                        named(
                            ComboBox::from_id_salt("length_precision")
                                .width(150.0)
                                .selected_text(length_pattern(u.length, u.length_precision))
                                .show_ui(ui, |ui| {
                                    for (p, s) in &lengths {
                                        ui.selectable_value(&mut u.length_precision, *p, s);
                                    }
                                }),
                            "Length precision",
                        );
                    });
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Angle").strong());
                        ui.label("Type:");
                        named(
                            ComboBox::from_id_salt("angle_type").width(150.0).selected_text(u.angle.name()).show_ui(
                                ui,
                                |ui| {
                                    for t in AngleType::ALL {
                                        ui.selectable_value(&mut u.angle, t, t.name());
                                    }
                                },
                            ),
                            "Angle type",
                        );
                        let angles = choices(|p| angle_pattern(u.angle, p));
                        u.angle_precision = offered(&angles, u.angle_precision);
                        ui.label("Precision:");
                        named(
                            ComboBox::from_id_salt("angle_precision")
                                .width(150.0)
                                .selected_text(angle_pattern(u.angle, u.angle_precision))
                                .show_ui(ui, |ui| {
                                    for (p, s) in &angles {
                                        ui.selectable_value(&mut u.angle_precision, *p, s);
                                    }
                                }),
                            "Angle precision",
                        );
                        ui.checkbox(&mut u.clockwise, "Clockwise");
                    });
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Insertion scale").strong());
                ui.label("Units to scale inserted content:");
                named(
                    ComboBox::from_id_salt("insertion_units")
                        .width(200.0)
                        .selected_text(insertion_name(u.insertion))
                        .show_ui(ui, |ui| {
                            for (code, name, _) in INSERTION_UNITS {
                                ui.selectable_value(&mut u.insertion, *code, *name);
                            }
                        }),
                    "Insertion units",
                );
                ui.add_space(6.0);
                ui.label(RichText::new("Direction").strong());
                ui.horizontal(|ui| {
                    ui.label("Base angle:");
                    for (name, a) in [("East", 0.0), ("North", 90.0), ("West", 180.0), ("South", 270.0)] {
                        if ui.selectable_label(u.base_angle == a, name).clicked() {
                            u.base_angle = a;
                        }
                    }
                    let r =
                        ui.add(egui::DragValue::new(&mut u.base_angle).range(0.0..=359.999_999).speed(1.0).suffix("°"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "Base angle"));
                    fields.push(r.id);
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Sample Output").strong());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(330.0);
                    let l = |v: f64| u.length(v);
                    ui.label(format!("{},{},{}", l(1.5), l(2.0039), l(0.0)));
                    ui.label(format!("{}<{},{}", l(3.0), u.direction(45.0), l(0.0)));
                });
                ui.colored_label(theme::text_dim(), "Lengths in feet and inches are inches; a foot is 12 units.");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            self.doc.begin("UNITS");
            self.doc.set_units(u);
            self.doc.commit();
        }
        self.units_dialog = (open && !ok && !cancel).then_some(u);
    }
}

/// An angle as the drawing's units show a direction, with a degree sign
/// for decimal degrees (tooltips, the Properties palette).
pub(crate) fn shown_direction(deg: f64) -> String {
    let u: Units = oxidraft_model::units::current();
    let s = u.direction(deg);
    if u.angle == AngleType::DecimalDegrees { format!("{s}°") } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precisions_offered_look_different() {
        let dms = choices(|p| angle_pattern(AngleType::DegMinSec, p));
        let shown: Vec<&str> = dms.iter().map(|c| c.1.as_str()).collect();
        assert_eq!(shown[..3], ["0d", "0d00'", "0d00'00\""]);
        assert_eq!(dms[1].0, 2, "the finest that looks so");
        assert_eq!(offered(&dms, 1), 2);
        assert_eq!(choices(|p| length_pattern(LengthType::Architectural, p))[4].1, "0'-0 1/16\"");
    }
}

//! STYLE: AutoCAD's Text Style dialog — the styles, each one's font,
//! height, width factor and oblique angle, with a preview; Set Current,
//! New, Delete and Apply. And the Annotate tab's Text Style dropdown.

use eframe::egui::{self, Align, ComboBox, Layout, RichText, Sense, Ui, vec2};
use oxidraft_commands::text_style::FONTS;

/// AutoCAD's big fonts: Japanese, Simplified and Traditional Chinese, Korean.
const BIG_FONTS: &[&str] = &["bigfont.shx", "extfont.shx", "gbcbig.shx", "chineset.shx", "whgtxt.shx"];
use oxidraft_model::{Geometry, TextLook, TextStyle};

use crate::{app::OxidraftApp, properties::common, theme};

pub(crate) struct TextStyleDialog {
    /// The style shown, as edited (Apply keeps it).
    edit: TextStyle,
    /// A new style's name being typed.
    naming: Option<String>,
    error: Option<String>,
}

impl OxidraftApp {
    pub(crate) fn open_text_style(&mut self) {
        let edit = self.doc.current_text_style();
        self.text_style_dialog = Some(TextStyleDialog { edit, naming: None, error: None });
    }

    /// The Annotate tab's Text Style dropdown: the selected texts' style
    /// (picking one restyles them), or the current style.
    pub(crate) fn text_style_combo(&mut self, ui: &mut Ui) {
        let selected: Vec<_> = if self.engine.is_active() { Vec::new() } else { self.engine.selection().to_vec() };
        let texts: Vec<_> = selected
            .iter()
            .filter_map(|id| self.doc.entity(*id))
            .filter(|e| matches!(e.geometry, Geometry::Text { .. }))
            .map(|e| e.id)
            .collect();
        let shown = if texts.is_empty() {
            Some(self.doc.current_text_style().name)
        } else {
            common(texts.iter().filter_map(|id| match &self.doc.entity(*id)?.geometry {
                Geometry::Text { look, .. } => Some(look.style.clone()),
                _ => None,
            }))
        };
        let names: Vec<String> = self.doc.text_styles().iter().map(|s| s.name.clone()).collect();
        let mut chosen = None;
        let r = ComboBox::from_id_salt("text_style_combo")
            .width(130.0)
            .selected_text(shown.clone().unwrap_or_else(|| "*VARIES*".into()))
            .show_ui(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(shown.as_deref() == Some(n), n).clicked() {
                        chosen = Some(n.clone());
                    }
                }
                ui.separator();
                if ui.selectable_label(false, "Manage Text Styles...").clicked() {
                    chosen = Some(String::new());
                }
            });
        r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Text style"));
        match chosen {
            Some(n) if n.is_empty() => self.open_text_style(),
            Some(n) if texts.is_empty() => {
                self.doc.begin("STYLE");
                let _ = self.doc.set_current_text_style(&n);
                self.doc.commit();
            }
            // Restyled text takes the style's look, and its height if it has one.
            Some(n) => {
                let Some(style) = self.doc.text_style(&n).cloned() else { return };
                self.edit(texts, |e| {
                    if let Geometry::Text { look, height, .. } = &mut e.geometry {
                        *look = TextLook::of(&style);
                        if style.height > 0.0 {
                            *height = style.height;
                        }
                    }
                });
            }
            None => {}
        }
    }

    pub(crate) fn text_style_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.text_style_dialog.take() else { return };
        let (mut open, mut close, mut apply, mut set_current, mut delete, mut new, mut create) =
            (true, false, false, false, false, false, false);
        let mut fields = Vec::new();
        let styles = self.doc.text_styles().to_vec();
        let current = self.doc.current_text_style().name;
        egui::Window::new("Text Style")
            .id(egui::Id::unique("text_style_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([360.0, 130.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(format!("Current text style: {current}"));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Styles").strong());
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(vec2(130.0, 210.0));
                            for s in &styles {
                                let picked = s.name.eq_ignore_ascii_case(&d.edit.name);
                                if ui.selectable_label(picked, &s.name).clicked() && !picked {
                                    d.edit = s.clone();
                                    d.error = None;
                                }
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        ui.set_width(250.0);
                        ui.label(RichText::new("Font").strong());
                        ui.horizontal(|ui| {
                            ui.label("Font Name:");
                            let r = ComboBox::from_id_salt("style_font")
                                .width(140.0)
                                .selected_text(&d.edit.font)
                                .show_ui(ui, |ui| {
                                    for f in FONTS {
                                        ui.selectable_value(&mut d.edit.font, (*f).to_string(), *f);
                                    }
                                });
                            r.response
                                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Font Name"));
                        });
                        // Whether this computer has the font (as AutoCAD would).
                        let found = oxidraft_plot::shx::shx_font(&d.edit.font).is_some()
                            || !oxidraft_plot::font::typeface(&d.edit.font).is_default();
                        let note = if found {
                            "Text shows and plots in this font."
                        } else {
                            "Not found on this computer: text shows in the default typeface."
                        };
                        ui.colored_label(theme::text_dim(), note);
                        // An SHX font's big font, for Asian characters.
                        if d.edit.font.to_ascii_lowercase().ends_with(".shx") {
                            let mut use_big = !d.edit.big_font.is_empty();
                            if ui.checkbox(&mut use_big, "Use Big Font").changed() {
                                d.edit.big_font = if use_big { BIG_FONTS[0].to_string() } else { String::new() };
                            }
                            if use_big {
                                ui.horizontal(|ui| {
                                    ui.label("Big Font:");
                                    let r = ComboBox::from_id_salt("style_big_font")
                                        .width(140.0)
                                        .selected_text(&d.edit.big_font)
                                        .show_ui(ui, |ui| {
                                            for f in BIG_FONTS {
                                                ui.selectable_value(&mut d.edit.big_font, (*f).to_string(), *f);
                                            }
                                        });
                                    r.response.widget_info(|| {
                                        egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Big Font")
                                    });
                                });
                            }
                        } else {
                            d.edit.big_font.clear();
                        }
                        ui.add_space(4.0);
                        ui.label(RichText::new("Size").strong());
                        let mut number =
                            |ui: &mut Ui, label: &str, v: &mut f64, range: std::ops::RangeInclusive<f64>| {
                                ui.horizontal(|ui| {
                                    ui.label(label);
                                    let r = ui.add(egui::DragValue::new(v).range(range).speed(0.05).max_decimals(4));
                                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, label));
                                    fields.push(r.id);
                                });
                            };
                        number(ui, "Height:", &mut d.edit.height, 0.0..=1e6);
                        ui.colored_label(theme::text_dim(), "0: asked for each text.");
                        ui.add_space(4.0);
                        ui.label(RichText::new("Effects").strong());
                        number(ui, "Width Factor:", &mut d.edit.width_factor, 0.01..=100.0);
                        number(ui, "Oblique Angle:", &mut d.edit.oblique, -85.0..=85.0);
                    });
                    ui.vertical(|ui| {
                        let size = vec2(96.0, 22.0);
                        set_current = ui.add_sized(size, egui::Button::new("Set Current")).clicked();
                        new = ui.add_sized(size, egui::Button::new("New...")).clicked();
                        delete = ui.add_sized(size, egui::Button::new("Delete")).clicked();
                    });
                });
                // A sample in the style's look.
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
                ui.painter().rect_filled(rect, 2.0, theme::model_bg());
                let painter = ui.painter_at(rect);
                let galley = painter.layout_no_wrap("AaBb123".into(), egui::FontId::proportional(28.0), theme::text());
                let anchor = rect.left_center() + vec2(12.0, 10.0);
                let baseline = galley.rows.first().map_or(22.0, |r| r.pos.y + r.row.size.y * 0.8);
                let shape = egui::epaint::TextShape::new(anchor - vec2(0.0, baseline), galley, theme::text());
                let (wf, ob) = (d.edit.width_factor as f32, d.edit.oblique as f32);
                crate::canvas::paint_text(&painter, shape, anchor, wf, ob, 0.0);
                if let Some(name) = &mut d.naming {
                    ui.horizontal(|ui| {
                        ui.label("Style Name:");
                        let r = ui.add(egui::TextEdit::singleline(name).desired_width(160.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Style Name"));
                        fields.push(r.id);
                        create = ui.button("OK").clicked()
                            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    });
                }
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                    apply = ui.button("Apply").clicked();
                });
            });
        self.text_fields.extend(fields);
        let run = |app: &mut OxidraftApp, f: &dyn Fn(&mut oxidraft_model::Document) -> Result<(), String>| {
            app.doc.begin("STYLE");
            match f(&mut app.doc) {
                Ok(()) => {
                    app.doc.commit();
                    None
                }
                Err(e) => {
                    app.doc.rollback();
                    Some(e)
                }
            }
        };
        if apply || set_current {
            let style = d.edit.clone();
            d.error = run(self, &|doc| doc.set_text_style(style.clone()));
        }
        if set_current && d.error.is_none() {
            let name = d.edit.name.clone();
            d.error = run(self, &|doc| doc.set_current_text_style(&name));
        }
        if delete {
            let name = d.edit.name.clone();
            d.error = run(self, &|doc| doc.delete_text_style(&name));
            if d.error.is_none() {
                d.edit = self.doc.current_text_style();
            }
        }
        if new {
            let n = (1..).map(|i| format!("Style{i}")).find(|n| self.doc.text_style(n).is_none()).unwrap_or_default();
            d.naming = Some(n);
        }
        if create && let Some(name) = d.naming.take() {
            let style = TextStyle { name: name.trim().to_string(), ..d.edit.clone() };
            if self.doc.text_style(&style.name).is_some() {
                d.error = Some(format!("A text style named \"{}\" already exists.", style.name));
                d.naming = Some(name);
            } else {
                let made = style.clone();
                d.error = run(self, &|doc| doc.set_text_style(made.clone()));
                if d.error.is_none() {
                    d.edit = style;
                }
            }
        }
        if open && !close {
            self.text_style_dialog = Some(d);
        }
    }
}

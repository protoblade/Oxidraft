//! PASTESPEC: AutoCAD's Paste Special dialog — the clipboard as drawing
//! objects (where they were, or as a block), or its text as multiline
//! text; and SCALELISTEDIT, the scales offered for annotation, kept with
//! the settings beside AutoCAD's own.

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::annoscale::{SCALES, parse_scale, scale_name};

use crate::{app::OxidraftApp, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasteAs {
    Objects,
    Original,
    Block,
    Text,
}

pub(crate) struct PasteSpecial {
    choice: PasteAs,
    /// The clipboard's text, when it isn't drawing objects.
    text: Option<String>,
    objects: bool,
}

pub(crate) struct ScaleList {
    new_scale: String,
    picked: Option<f64>,
    error: Option<String>,
}

impl OxidraftApp {
    /// The annotation scales offered: AutoCAD's and the user's, smallest
    /// drawing scale first.
    pub(crate) fn scale_list(&self) -> Vec<(String, f64)> {
        let mut out: Vec<(String, f64)> = SCALES.iter().map(|(n, s)| (n.to_string(), *s)).collect();
        for s in &self.settings.scales {
            if !out.iter().any(|o| (o.1 - s).abs() < 1e-9) {
                out.push((scale_name(*s), *s));
            }
        }
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }

    pub(crate) fn open_paste_special(&mut self) {
        let system = arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok();
        let objects = system.as_deref().is_some_and(crate::clipboard::looks_like_dxf) || self.clipboard.is_some();
        let text = system.filter(|t| !crate::clipboard::looks_like_dxf(t) && !t.trim().is_empty());
        if !objects && text.is_none() {
            self.engine.log("Nothing to paste: the clipboard is empty.".to_string());
            return;
        }
        let choice = if objects { PasteAs::Objects } else { PasteAs::Text };
        self.paste_special = Some(PasteSpecial { choice, text, objects });
    }

    pub(crate) fn paste_special_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.paste_special.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Paste Special")
            .id(egui::Id::unique("paste_special"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 200.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("As:").strong());
                ui.add_enabled_ui(d.objects, |ui| {
                    ui.radio_value(&mut d.choice, PasteAs::Objects, "Drawing objects");
                    ui.radio_value(&mut d.choice, PasteAs::Original, "Drawing objects, where they were");
                    ui.radio_value(&mut d.choice, PasteAs::Block, "A block");
                });
                ui.add_enabled_ui(d.text.is_some(), |ui| {
                    ui.radio_value(&mut d.choice, PasteAs::Text, "Text (multiline)");
                });
                if let Some(t) = &d.text {
                    let first = t.lines().next().unwrap_or_default();
                    ui.colored_label(theme::text_dim(), format!("“{}”", first.chars().take(40).collect::<String>()));
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok {
            match d.choice {
                PasteAs::Objects => self.paste_command(false),
                PasteAs::Original => self.paste_command(true),
                PasteAs::Block => self.paste_block_command(),
                PasteAs::Text => {
                    let content = d.text.unwrap_or_default().replace("\r\n", "\n").trim_end().replace('\n', "\\P");
                    let place = oxidraft_commands::field_cmd::FieldText::with_text(content);
                    self.engine.run_with("FIELD", "", Box::new(place), &mut self.doc);
                }
            }
            return;
        }
        if open && !cancel {
            self.paste_special = Some(d);
        }
    }

    pub(crate) fn open_scale_list(&mut self) {
        self.scale_list_dialog = Some(ScaleList { new_scale: String::new(), picked: None, error: None });
    }

    pub(crate) fn scale_list_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.scale_list_dialog.take() else { return };
        let (mut open, mut close, mut add, mut delete, mut reset) = (true, false, false, false, false);
        let mut fields = Vec::new();
        let list = self.scale_list();
        egui::Window::new("Edit Drawing Scales")
            .id(egui::Id::unique("scale_list"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Scale List").strong());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_size(vec2(200.0, 200.0));
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for (name, s) in &list {
                            let own = self.settings.scales.iter().any(|x| (x - s).abs() < 1e-9);
                            let label = if own { format!("{name}  (yours)") } else { name.clone() };
                            if ui.selectable_label(d.picked == Some(*s), label).clicked() {
                                d.picked = Some(*s);
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut d.new_scale).desired_width(90.0).hint_text("1:25"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "New scale"));
                    fields.push(r.id);
                    add = ui.button("Add").clicked();
                    let own = d.picked.is_some_and(|p| self.settings.scales.iter().any(|x| (x - p).abs() < 1e-9));
                    delete = ui.add_enabled(own, egui::Button::new("Delete")).clicked();
                    reset = ui.button("Reset").on_hover_text("Back to AutoCAD's scales alone").clicked();
                });
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        let mut scales = self.settings.scales.clone();
        if add {
            match parse_scale(&d.new_scale) {
                Some(s) if !list.iter().any(|x| (x.1 - s).abs() < 1e-9) => {
                    scales.push(s);
                    d.new_scale.clear();
                    d.error = None;
                }
                Some(_) => d.error = Some("That scale is in the list.".into()),
                None => d.error = Some("Type a scale like 1:25 or 2:1.".into()),
            }
        }
        if delete && let Some(p) = d.picked.take() {
            scales.retain(|x| (x - p).abs() > 1e-9);
        }
        if reset {
            scales.clear();
        }
        if scales != self.settings.scales {
            let settings = crate::settings::Settings { scales, ..self.settings.clone() };
            self.apply_settings(ctx, settings);
        }
        if open && !close {
            self.scale_list_dialog = Some(d);
        }
    }
}

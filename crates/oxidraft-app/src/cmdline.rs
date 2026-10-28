//! The floating command line over the bottom of the drawing area, with
//! AutoComplete and clickable command options, as in AutoCAD LT.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Key, Margin, Order, RichText, Sense, Stroke, TextEdit, vec2,
};
use oxidraft_commands::Engine;

use crate::{app::OxidraftApp, theme};

const BAR_BG: Color32 = Color32::from_rgb(52, 60, 74);
const HISTORY_BG: Color32 = Color32::from_rgba_premultiplied(30, 35, 44, 200);
const KEYWORD_BG: Color32 = Color32::from_rgb(70, 82, 102);

impl OxidraftApp {
    pub(crate) fn command_line(&mut self, ctx: &egui::Context) {
        let canvas = self.canvas_rect;
        if !canvas.is_positive() {
            return;
        }
        let width = (canvas.width() - 40.0).clamp(300.0, 980.0);
        self.handle_command_keys(ctx);

        egui::Area::new(egui::Id::unique("command_line"))
            .order(Order::Foreground)
            .fixed_pos(egui::pos2(canvas.center().x - width / 2.0, canvas.bottom() - 10.0))
            .pivot(Align2::LEFT_BOTTOM)
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                self.suggestions(ui);
                self.history_lines(ui, width);
                self.input_bar(ui, width);
            });
    }

    /// Up/Down/Tab for AutoComplete and input recall. Runs before the text
    /// field so it can consume the keys.
    fn handle_command_keys(&mut self, ctx: &egui::Context) {
        if self.editing_field(ctx) {
            return;
        }
        let idle = !self.engine.is_active();
        let suggestions = if idle { Engine::suggest(&self.command_text).len() } else { 0 };
        let (up, down, tab) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::Tab),
            )
        });
        if suggestions > 0 {
            if up {
                self.suggestion = (self.suggestion + suggestions - 1) % suggestions;
            }
            if down {
                self.suggestion = (self.suggestion + 1) % suggestions;
            }
            if tab && let Some(c) = Engine::suggest(&self.command_text).get(self.suggestion) {
                self.command_text = c.name.to_string();
            }
            return;
        }
        let typed = self.engine.typed();
        if typed.is_empty() {
            return;
        }
        if up {
            let i = self.recall.map_or(typed.len() - 1, |i| i.saturating_sub(1));
            self.recall = Some(i);
            self.command_text = typed[i].clone();
        }
        if down && let Some(i) = self.recall {
            if i + 1 < typed.len() {
                self.recall = Some(i + 1);
                self.command_text = typed[i + 1].clone();
            } else {
                self.recall = None;
                self.command_text.clear();
            }
        }
    }

    fn suggestions(&mut self, ui: &mut egui::Ui) {
        if self.engine.is_active() {
            return;
        }
        let list = Engine::suggest(&self.command_text);
        if list.is_empty() {
            return;
        }
        self.suggestion = self.suggestion.min(list.len() - 1);
        Frame::NONE
            .fill(theme::TOOLTIP_BG)
            .stroke(Stroke::new(1.0, theme::TOOLTIP_EDGE))
            .inner_margin(Margin::same(3))
            .show(ui, |ui| {
                ui.set_width(280.0);
                for (i, c) in list.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(274.0, 22.0), Sense::click());
                    if i == self.suggestion || response.hovered() {
                        ui.painter().rect_filled(rect, 2.0, theme::ACCENT.gamma_multiply(0.35));
                    }
                    let aliases =
                        if c.aliases.is_empty() { String::new() } else { format!(" ({})", c.aliases.join(", ")) };
                    ui.painter().text(
                        rect.left_center() + vec2(8.0, 0.0),
                        Align2::LEFT_CENTER,
                        format!("{}{aliases}", c.name),
                        FontId::proportional(12.5),
                        theme::TEXT,
                    );
                    ui.painter().text(
                        rect.right_center() - vec2(8.0, 0.0),
                        Align2::RIGHT_CENTER,
                        c.description,
                        FontId::proportional(10.5),
                        theme::TEXT_DIM,
                    );
                    if response.clicked() {
                        self.run(c.name);
                    }
                }
            });
        ui.add_space(4.0);
    }

    fn history_lines(&self, ui: &mut egui::Ui, width: f32) {
        let history = self.engine.history();
        let count = if self.history_expanded { 14 } else { 3 };
        let lines = &history[history.len().saturating_sub(count)..];
        if lines.is_empty() {
            return;
        }
        Frame::NONE.fill(HISTORY_BG).inner_margin(Margin::symmetric(10, 4)).show(ui, |ui| {
            ui.set_width(width - 20.0);
            for (i, line) in lines.iter().enumerate() {
                // Older lines fade out, like AutoCAD's floating command line.
                let age = (lines.len() - 1 - i) as f32;
                let alpha = if self.history_expanded { 1.0 } else { 1.0 - age * 0.25 };
                ui.label(RichText::new(line).font(FontId::monospace(12.0)).color(theme::TEXT.gamma_multiply(alpha)));
            }
        });
    }

    fn input_bar(&mut self, ui: &mut egui::Ui, width: f32) {
        let prompt = self.engine.current_prompt();
        let name = self.engine.active_name();
        let mut clicked_keyword = None;
        Frame::NONE
            .fill(BAR_BG)
            .stroke(Stroke::new(1.0, theme::TOOLTIP_EDGE))
            .corner_radius(CornerRadius::same(2))
            .inner_margin(Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.set_width(width - 16.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(">_").font(FontId::monospace(13.0)).color(theme::TEXT_DIM));
                    if let (Some(p), Some(name)) = (&prompt, name) {
                        ui.label(RichText::new(name).font(FontId::proportional(12.5)).strong().color(theme::TEXT));
                        let or = if p.keywords.is_empty() { "" } else { " or" };
                        ui.label(
                            RichText::new(format!("{}{or}", p.text))
                                .font(FontId::proportional(12.5))
                                .color(theme::TEXT),
                        );
                        if !p.keywords.is_empty() {
                            ui.spacing_mut().item_spacing.x = 1.0;
                            ui.label(RichText::new("[").color(theme::TEXT));
                            for (i, k) in p.keywords.iter().enumerate() {
                                if i > 0 {
                                    ui.label(RichText::new("/").color(theme::TEXT));
                                }
                                let button = egui::Button::new(RichText::new(*k).color(theme::TEXT))
                                    .fill(KEYWORD_BG)
                                    .corner_radius(2.0)
                                    .min_size(vec2(0.0, 18.0));
                                if ui.add(button).clicked() {
                                    clicked_keyword = Some(*k);
                                }
                            }
                            ui.label(RichText::new("]").color(theme::TEXT));
                        }
                        ui.spacing_mut().item_spacing.x = 1.0;
                        let default = p.default.as_deref().map_or(String::new(), |d| format!(" <{d}>"));
                        ui.label(RichText::new(format!("{default}:")).color(theme::TEXT));
                        ui.spacing_mut().item_spacing.x = 4.0;
                    }
                    let edit = TextEdit::singleline(&mut self.command_text)
                        .font(FontId::proportional(12.5))
                        .hint_text(if prompt.is_none() { "Type a command" } else { "" })
                        .frame(Frame::NONE)
                        .desired_width(f32::INFINITY);
                    let response = ui.add(edit);
                    if response.changed() {
                        self.suggestion = 0;
                        self.recall = None;
                    }
                    // Keystrokes always land on the command line, as in AutoCAD,
                    // except while a dropdown (e.g. the layer list) is open.
                    if !ui.ctx().any_popup_open() && !self.editing_field(ui.ctx()) {
                        response.request_focus();
                    }
                });
            });

        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        // Space acts as Enter, as in AutoCAD.
        let space = self.command_text.ends_with(' ');
        if space {
            self.command_text.pop();
        }
        if let Some(k) = clicked_keyword {
            self.type_command(k);
        } else if enter || space {
            self.submit();
        }
    }
}

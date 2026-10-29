//! LAYWALK: the drawing's layers in a list; only those selected in it
//! draw, so you can step through them to see what's on each. Closing puts
//! the layers back as they were, unless Restore on exit is off.

use std::collections::HashSet;

use eframe::egui::{self, Align, Layout, RichText, vec2};

use crate::{app::OxidraftApp, layers::swatch, theme};

pub(crate) struct LayerWalk {
    /// The layers selected (upper case), which are the ones drawn.
    shown: HashSet<String>,
    /// Where a Shift-click range starts.
    anchor: Option<usize>,
    restore: bool,
}

impl LayerWalk {
    fn shows(&self, layer: &str) -> bool {
        self.shown.contains(&layer.to_ascii_uppercase())
    }
}

impl OxidraftApp {
    /// True if objects on `layer` draw: it's on and thawed, or picked in
    /// LAYWALK while that's open.
    pub fn shows_layer(&self, layer: &str) -> bool {
        match &self.layer_walk {
            Some(w) if self.doc.layer(layer).is_some() => w.shows(layer),
            _ => self.doc.is_shown(layer),
        }
    }

    /// LAYWALK: starts with the layers that show now.
    pub(crate) fn open_layer_walk(&mut self) {
        let shown = self.doc.layers().iter().filter(|l| l.is_shown()).map(|l| l.name.to_ascii_uppercase()).collect();
        self.layer_walk = Some(LayerWalk { shown, anchor: None, restore: true });
    }

    pub(crate) fn layer_walk_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut w) = self.layer_walk.take() else { return };
        let (mut open, mut close, mut purge) = (true, false, false);
        let layers: Vec<(String, u8, bool)> =
            self.doc.layers().iter().map(|l| (l.name.clone(), l.color, self.doc.layer_usage(&l.name) > 0)).collect();
        let modifiers = ctx.input(|i| i.modifiers);
        egui::Window::new(format!("LayerWalk - Layers: {}", layers.len()))
            .id(egui::Id::unique("layer_walk"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.colored_label(theme::text_dim(), "Only the selected layers show. Ctrl- or Shift-click for more.");
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(260.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (i, (name, color, _)) in layers.iter().enumerate() {
                            let key = name.to_ascii_uppercase();
                            let selected = w.shown.contains(&key);
                            let clicked = ui
                                .horizontal(|ui| {
                                    swatch(ui, *color);
                                    ui.selectable_label(selected, name.as_str()).clicked()
                                })
                                .inner;
                            if !clicked {
                                continue;
                            }
                            if modifiers.shift && w.anchor.is_some() {
                                let a = w.anchor.unwrap_or(i);
                                let range = a.min(i)..=a.max(i);
                                w.shown = layers[range].iter().map(|l| l.0.to_ascii_uppercase()).collect();
                            } else if modifiers.command || modifiers.ctrl {
                                if !w.shown.remove(&key) {
                                    w.shown.insert(key);
                                }
                                w.anchor = Some(i);
                            } else {
                                w.shown = HashSet::from([key]);
                                w.anchor = Some(i);
                            }
                        }
                    });
                });
                ui.checkbox(&mut w.restore, "Restore on exit");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                    let unused = layers.iter().any(|(n, _, used)| !used && w.shows(n) && n != "0");
                    purge = ui
                        .add_enabled(unused, egui::Button::new("Purge"))
                        .on_hover_text("Deletes the selected layers nothing is on")
                        .clicked();
                });
                ui.label(RichText::new(format!("{} of {} layers showing", w.shown.len(), layers.len())).small());
            });
        if purge {
            self.doc.begin("LAYWALK");
            let mut gone = 0;
            for (name, _, used) in &layers {
                if !used && w.shows(name) && self.doc.delete_layer(name).is_ok() {
                    w.shown.remove(&name.to_ascii_uppercase());
                    gone += 1;
                }
            }
            self.doc.commit();
            self.engine.log(format!("{gone} layer(s) purged."));
        }
        if !open || close {
            if !w.restore {
                // What showed stays: those layers on and thawed, the rest off.
                self.doc.begin("LAYWALK");
                for mut l in self.doc.layers().to_vec() {
                    let shown = w.shows(&l.name);
                    l.on = shown;
                    l.frozen &= !shown;
                    if l.name.eq_ignore_ascii_case(self.doc.current_layer()) {
                        l.on = true;
                    }
                    self.doc.update_layer(l);
                }
                self.doc.commit();
            }
            return;
        }
        self.layer_walk = Some(w);
    }
}

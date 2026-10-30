//! Traces, as AutoCAD's: markup over the drawing as if on tracing paper,
//! that leaves the drawing as it is. Here they're kept in the drawing
//! itself (no Autodesk account needed): each trace is a layer of its own,
//! `TRACE$` and its name, drawn over the drawing with tracing paper between
//! them (TRACEFRONT; TRACEBACK puts the drawing in front), the paper as
//! opaque as TRACEPAPERCTL says. The Traces palette (TRACES) makes them,
//! shows or hides each, opens one to draw in, copies one's markup into the
//! drawing and deletes them.

use eframe::egui::{self, RichText};
use oxidraft_model::Layer;

use crate::app::OxidraftApp;
use crate::theme;

/// What makes a layer a trace's.
pub(crate) const PREFIX: &str = "TRACE$";

pub(crate) fn is_trace_layer(layer: &str) -> bool {
    layer.len() > PREFIX.len() && layer[..PREFIX.len()].eq_ignore_ascii_case(PREFIX)
}

/// The Traces palette: the name of a trace to make.
#[derive(Default)]
pub(crate) struct TracesPalette {
    name: String,
}

impl OxidraftApp {
    /// The drawing's traces, by name.
    pub fn traces(&self) -> Vec<String> {
        self.doc
            .layers()
            .iter()
            .filter(|l| is_trace_layer(&l.name))
            .map(|l| l.name[PREFIX.len()..].to_string())
            .collect()
    }

    /// The trace being drawn in, if one is.
    pub fn trace_open(&self) -> Option<&str> {
        self.trace_open.as_ref().map(|(t, _)| t.as_str())
    }

    /// A new trace, opened to draw in.
    pub fn create_trace(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A trace needs a name.".into());
        }
        let layer = format!("{PREFIX}{name}");
        if self.doc.layer(&layer).is_some() {
            return Err(format!("There's a trace named \"{name}\" already."));
        }
        self.doc.begin("TRACES");
        // Markup red, as AutoCAD's traces start.
        self.doc.add_layer(Layer::new(layer, 1));
        self.doc.commit();
        self.open_trace(name)
    }

    /// Draws in trace `name`: what's drawn goes into it, until it's closed.
    pub fn open_trace(&mut self, name: &str) -> Result<(), String> {
        let layer = format!("{PREFIX}{name}");
        if self.doc.layer(&layer).is_none() {
            return Err(format!("No trace named \"{name}\"."));
        }
        self.close_trace();
        let before = self.doc.current_layer().to_string();
        self.set_trace_shown(name, true);
        self.doc.set_current_layer(&layer);
        self.trace_open = Some((name.to_string(), before));
        self.engine.log(format!("Drawing in trace \"{name}\"; close it in the Traces palette."));
        Ok(())
    }

    /// Back to drawing in the drawing.
    pub fn close_trace(&mut self) {
        if let Some((_, before)) = self.trace_open.take()
            && !self.doc.set_current_layer(&before)
        {
            self.doc.set_current_layer("0");
        }
    }

    pub fn set_trace_shown(&mut self, name: &str, on: bool) {
        if let Some(mut l) = self.doc.layer(&format!("{PREFIX}{name}")).cloned()
            && l.on != on
        {
            l.on = on;
            self.doc.begin("TRACES");
            self.doc.update_layer(l);
            self.doc.commit();
        }
    }

    /// The trace's markup copied into the drawing, on layer `layer`; how
    /// many objects.
    pub fn copy_trace_to_drawing(&mut self, name: &str, layer: &str) -> usize {
        let from = format!("{PREFIX}{name}");
        let objects: Vec<oxidraft_model::Entity> =
            self.doc.all_entities().filter(|e| e.layer.eq_ignore_ascii_case(&from)).cloned().collect();
        self.doc.begin("TRACECOPY");
        for e in &objects {
            let id = self.doc.add_like(e.geometry.clone(), e);
            self.doc.update(id, |c| c.layer = layer.to_string());
        }
        self.doc.commit();
        objects.len()
    }

    /// The trace and its markup gone.
    pub fn delete_trace(&mut self, name: &str) {
        if self.trace_open() == Some(name) {
            self.close_trace();
        }
        let layer = format!("{PREFIX}{name}");
        self.doc.begin("TRACES");
        let _ = self.doc.delete_layer_and_objects(&layer);
        self.doc.commit();
    }

    pub(crate) fn traces_palette(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.traces_palette.take() else { return };
        let mut open = true;
        let traces = self.traces();
        let (mut create, mut toggle, mut edit, mut copy, mut delete, mut close) =
            (false, None, None, None, None, false);
        egui::Window::new("TRACES")
            .id(egui::Id::unique("traces_palette"))
            .open(&mut open)
            .collapsible(true)
            .resizable(false)
            .default_pos([80.0, 220.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                if let Some(t) = self.trace_open() {
                    ui.colored_label(theme::accent(), format!("Drawing in \"{t}\""));
                    close = ui.button("Close Trace").clicked();
                    ui.separator();
                }
                if traces.is_empty() {
                    ui.colored_label(theme::text_dim(), "No traces yet.");
                }
                for t in &traces {
                    let shown = self.doc.layer(&format!("{PREFIX}{t}")).is_some_and(|l| l.on);
                    ui.horizontal(|ui| {
                        let mut on = shown;
                        let r = ui.checkbox(&mut on, "");
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::CheckBox, true, format!("Show {t}")));
                        if r.changed() {
                            toggle = Some((t.clone(), on));
                        }
                        ui.label(RichText::new(t).strong());
                        if ui.small_button("Edit").on_hover_text("Draw in this trace").clicked() {
                            edit = Some(t.clone());
                        }
                        if ui.small_button("Copy").on_hover_text("Copy its markup into the drawing").clicked() {
                            copy = Some(t.clone());
                        }
                        if ui.small_button("Delete").clicked() {
                            delete = Some(t.clone());
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let r =
                        ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(130.0).hint_text("Trace name"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Trace name"));
                    self.text_fields.push(r.id);
                    create = ui.add_enabled(!p.name.trim().is_empty(), egui::Button::new("Create Trace")).clicked();
                });
                ui.horizontal(|ui| {
                    ui.label("Tracing paper:");
                    let mut paper = self.trace_paper;
                    let r = ui.add(egui::Slider::new(&mut paper, 0..=100).suffix("%"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Tracing paper"));
                    self.trace_paper = paper;
                });
            });
        if close {
            self.close_trace();
        }
        if let Some((t, on)) = toggle {
            self.set_trace_shown(&t, on);
        }
        if let Some(t) = edit
            && let Err(e) = self.open_trace(&t)
        {
            self.engine.log(e);
        }
        if let Some(t) = copy {
            let layer =
                self.trace_open.as_ref().map_or_else(|| self.doc.current_layer().to_string(), |(_, l)| l.clone());
            let n = self.copy_trace_to_drawing(&t, &layer);
            self.engine.log(format!("{n} object(s) copied from trace \"{t}\" onto layer {layer}."));
        }
        if let Some(t) = delete {
            self.delete_trace(&t);
        }
        if create {
            let name = std::mem::take(&mut p.name);
            if let Err(e) = self.create_trace(&name) {
                self.engine.log(e);
            }
        }
        if open {
            self.traces_palette = Some(p);
        }
    }
}

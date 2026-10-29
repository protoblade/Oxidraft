//! ULAYERS: AutoCAD's Underlay Layers dialog — a PDF underlay's layers,
//! each shown or hidden in the drawing, in plots and in PDFIMPORT.

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::Geometry;

use crate::{app::OxidraftApp, theme, xref::resolve};

pub(crate) struct UnderlayLayers {
    /// The PDF underlays' names, and the one shown.
    underlays: Vec<String>,
    picked: usize,
    /// Its layers and whether each shows.
    layers: Vec<(String, bool)>,
    error: Option<String>,
}

impl OxidraftApp {
    /// The layers of PDF underlay `name`: the file's, with the drawing's
    /// own choices.
    fn underlay_layers(&self, name: &str) -> Result<Vec<(String, bool)>, String> {
        let def = self.doc.image_def(name).ok_or("No such underlay.")?;
        let file = resolve(self.path.as_deref(), &def.path).ok_or_else(|| format!("{} not found", def.path))?;
        let data = std::fs::read(&file).map_err(|e| e.to_string())?;
        let off = self.doc.pdf_layers_off(name);
        Ok(oxidraft_raster::pdf_layers::pdf_layers(&data)
            .into_iter()
            .map(|(layer, shows)| {
                let shows = if off.is_empty() { shows } else { !off.contains(&layer) };
                (layer, shows)
            })
            .collect())
    }

    /// The renderer told which layers each PDF underlay hides.
    pub(crate) fn apply_pdf_layers(&self) {
        for def in self.doc.image_defs().filter(|d| d.is_pdf()) {
            if let Some(file) = resolve(self.path.as_deref(), &def.path) {
                oxidraft_raster::pdf_layers::set_layers_off(&file, self.doc.pdf_layers_off(&def.name).to_vec());
            }
        }
    }

    pub(crate) fn open_underlay_layers(&mut self) {
        let underlays: Vec<String> = self.doc.image_defs().filter(|d| d.is_pdf()).map(|d| d.name.clone()).collect();
        if underlays.is_empty() {
            self.engine.log("There are no PDF underlays in this drawing.".to_string());
            return;
        }
        // The selected underlay's, else the first's.
        let selected = self.engine.selection().iter().find_map(|id| match self.doc.entity(*id).map(|e| &e.geometry) {
            Some(Geometry::Image { image, .. }) => underlays.iter().position(|u| u.eq_ignore_ascii_case(image)),
            _ => None,
        });
        let picked = selected.unwrap_or(0);
        let (layers, error) = match self.underlay_layers(&underlays[picked]) {
            Ok(l) => (l, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        self.underlay_layers_dialog = Some(UnderlayLayers { underlays, picked, layers, error });
    }

    /// Hides `off` in PDF underlay `name`, and shows it again so.
    pub fn set_underlay_layers_off(&mut self, name: &str, off: Vec<String>) {
        self.doc.set_pdf_layers_off(name, off);
        self.apply_pdf_layers();
        self.load_image(name);
    }

    pub(crate) fn underlay_layers_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.underlay_layers_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let before = d.picked;
        egui::Window::new("Underlay Layers")
            .id(egui::Id::unique("ulayers"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Reference name:");
                    egui::ComboBox::from_id_salt("ulayers_ref").selected_text(&d.underlays[d.picked]).show_ui(
                        ui,
                        |ui| {
                            for (i, u) in d.underlays.iter().enumerate() {
                                ui.selectable_value(&mut d.picked, i, u);
                            }
                        },
                    );
                });
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_size(vec2(260.0, 160.0));
                    ui.label(RichText::new("Layers").strong());
                    if d.layers.is_empty() && d.error.is_none() {
                        ui.colored_label(theme::text_dim(), "This PDF has no layers.");
                    }
                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        for (name, shows) in &mut d.layers {
                            ui.checkbox(shows, name.as_str());
                        }
                    });
                });
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if d.picked != before {
            match self.underlay_layers(&d.underlays[d.picked]) {
                Ok(l) => (d.layers, d.error) = (l, None),
                Err(e) => (d.layers, d.error) = (Vec::new(), Some(e)),
            }
        }
        if ok {
            let off: Vec<String> = d.layers.iter().filter(|l| !l.1).map(|l| l.0.clone()).collect();
            let name = d.underlays[d.picked].clone();
            self.set_underlay_layers_off(&name, off);
            return;
        }
        if open && !cancel {
            self.underlay_layers_dialog = Some(d);
        }
    }
}

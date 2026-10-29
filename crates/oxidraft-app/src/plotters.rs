//! The plotters a plot can go to (AutoCAD's plotter configurations for
//! files: DWG To PDF, DWFx ePlot, PublishToWeb PNG and JPG), how each
//! writes sheets, PLOTTERMANAGER (the configurations, paper sizes of your
//! own and the resolution of image plots), and WMFOUT.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::EntityId;
use oxidraft_plot::{MARGIN, Paper, PlotArea, PlotScale, PlotSettings, Plotter, Sheet};

use crate::{app::OxidraftApp, theme};

/// PLOTTERMANAGER's dialog: the paper sizes and resolution being edited.
pub(crate) struct PlotterManager {
    papers: Vec<(String, f64, f64)>,
    dpi: u32,
    /// The size being added.
    name: String,
    width: f64,
    height: f64,
}

impl OxidraftApp {
    /// How fine image plots are, in dots per inch.
    pub(crate) fn raster_dpi(&self) -> u32 {
        if self.settings.raster_dpi == 0 { 150 } else { self.settings.raster_dpi }
    }

    /// `sheets` as `plotter` writes them (an image plotter writes the first).
    pub fn plot_bytes(&self, plotter: Plotter, sheets: &[Sheet]) -> Result<Vec<u8>, String> {
        let title = self.file_name();
        match plotter {
            Plotter::Pdf => Ok(oxidraft_plot::pdf_pages(sheets, &title)),
            Plotter::Dwfx => Ok(oxidraft_plot::dwfx::dwfx(sheets, &title)),
            Plotter::Png | Plotter::Jpg => {
                let sheet = sheets.first().ok_or("There's nothing to plot.")?;
                let pdf = oxidraft_plot::pdf(sheet, &title);
                let px = oxidraft_raster::render_pdf_data(pdf, 0, self.raster_dpi() as f32)?;
                let rgba = image::RgbaImage::from_raw(px.width, px.height, px.rgba).ok_or("The image is empty.")?;
                let mut out = std::io::Cursor::new(Vec::new());
                let written = if plotter == Plotter::Png {
                    rgba.write_to(&mut out, image::ImageFormat::Png)
                } else {
                    image::DynamicImage::ImageRgba8(rgba).to_rgb8().write_to(&mut out, image::ImageFormat::Jpeg)
                };
                written.map_err(|e| e.to_string())?;
                Ok(out.into_inner())
            }
        }
    }

    /// Asks where `plotter`'s file goes, named after the drawing.
    pub(crate) fn plot_file_path(&self, plotter: Plotter) -> Option<PathBuf> {
        let ext = plotter.extension();
        let name = Path::new(&self.file_name()).with_extension(ext);
        let mut dialog =
            rfd::FileDialog::new().add_filter(ext.to_ascii_uppercase(), &[ext]).set_file_name(name.to_string_lossy());
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.save_file()
    }

    /// WMFOUT: asks where the metafile goes, then writes the objects.
    pub(crate) fn wmf_out(&mut self, ids: &[EntityId]) {
        let name = Path::new(&self.file_name()).with_extension("wmf");
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Windows Metafile", &["wmf"])
            .set_file_name(name.to_string_lossy())
            .save_file()
        else {
            return;
        };
        match self.wmf_out_to(&path, ids) {
            Ok(()) => self.engine.log(format!("Windows Metafile written to {}", path.display())),
            Err(e) => self.engine.log(e),
        }
    }

    /// Writes the objects `ids` to a Windows Metafile at `path`: as they
    /// plot, in their colors, filling a sheet their shape.
    pub fn wmf_out_to(&mut self, path: &Path, ids: &[EntityId]) -> Result<(), String> {
        let space = self.shown_space();
        let mut only = self.doc.clone();
        let others: Vec<EntityId> = only.all_entities().map(|e| e.id).filter(|id| !ids.contains(id)).collect();
        for id in others {
            only.remove(id);
        }
        let extents = ids
            .iter()
            .filter_map(|id| self.doc.entity(*id))
            .map(|e| self.doc.entity_bounds(e))
            .reduce(oxidraft_geom::Aabb::union)
            .ok_or("Nothing selected.")?;
        let (w, h) = (extents.width().max(1e-9), extents.height().max(1e-9));
        // 200 mm across its longer side, inside the plotter's margin.
        let k = 200.0 / w.max(h);
        let paper = Paper { name: "WMFOUT", width: w * k + 2.0 * MARGIN, height: h * k + 2.0 * MARGIN };
        let settings = PlotSettings {
            paper,
            landscape: w >= h,
            area: PlotArea::Window(extents),
            scale: PlotScale::Fit,
            center: true,
            ..PlotSettings::default()
        };
        let sheet = oxidraft_plot::render(&only, space, &settings).ok_or("There's nothing to write.")?;
        std::fs::write(path, oxidraft_plot::wmf::wmf(&sheet))
            .map_err(|e| format!("Could not write {}: {e}", path.display()))
    }

    pub(crate) fn open_plotter_manager(&mut self) {
        self.plotter_manager = Some(PlotterManager {
            papers: self.settings.papers.clone(),
            dpi: self.raster_dpi(),
            name: String::new(),
            width: 210.0,
            height: 297.0,
        });
    }

    /// PLOTTERMANAGER: the plotter configurations, paper sizes of your own
    /// (every plotter offers them) and how fine image plots are.
    pub(crate) fn plotter_manager_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.plotter_manager.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Plotter Manager")
            .id(egui::Id::unique("plotter_manager"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 140.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Plotter configurations").strong());
                egui::Grid::new("plotters").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    for p in Plotter::ALL {
                        ui.label(p.name());
                        ui.colored_label(theme::text_dim(), p.description());
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Custom paper sizes").strong());
                let mut remove = None;
                egui::Grid::new("custom_papers").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (i, (name, w, h)) in d.papers.iter().enumerate() {
                        ui.label(name);
                        ui.label(format!("{w:.2} x {h:.2} mm"));
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = remove {
                    d.papers.remove(i);
                }
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(140.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Paper name"));
                    fields.push(r.id);
                    for (label, v) in [("Width:", &mut d.width), ("Height:", &mut d.height)] {
                        ui.label(label);
                        let r = ui.add(egui::DragValue::new(v).range(1.0..=10_000.0).speed(1.0).suffix(" mm"));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                        fields.push(r.id);
                    }
                    let name = d.name.trim().to_string();
                    let new = !name.is_empty() && !oxidraft_plot::papers().iter().any(|p| p.name == name);
                    if ui.add_enabled(new, egui::Button::new("Add")).clicked() {
                        d.papers.push((name, d.width, d.height));
                        d.name.clear();
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("PNG and JPEG plots:").strong());
                    let r = ui.add(egui::DragValue::new(&mut d.dpi).range(36..=1200).suffix(" dpi"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Resolution"));
                    fields.push(r.id);
                });
                let row = vec2(ui.available_width().max(420.0), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            let settings = crate::settings::Settings { papers: d.papers, raster_dpi: d.dpi, ..self.settings.clone() };
            self.apply_settings(ctx, settings);
            return;
        }
        if open && !cancel {
            self.plotter_manager = Some(d);
        }
    }
}

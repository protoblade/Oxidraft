//! PDFIMPORT: a PDF page's drawing brought in as objects — its strokes as
//! lines and polylines (PDF_Geometry), its fills as solid hatches
//! (PDF_Solid Fills), its text as text (PDF_Text), in their colors and
//! lineweights; from a PDF underlay (where it lies) or from a file.

use std::path::PathBuf;

use eframe::egui::{self, Align, Layout, vec2};
use oxidraft_geom::Point;
use oxidraft_model::{Attach, Color, Geometry, HatchPattern, Layer, Lineweight, Vertex, aci_rgb};
use oxidraft_raster::pdf_vectors::{PdfDrawing, pdf_vectors};

use crate::app::OxidraftApp;

/// The AutoCAD Color Index nearest an RGB color.
fn nearest_aci(rgb: [u8; 3]) -> u8 {
    let d = |i: u8| {
        let (r, g, b) = aci_rgb(i);
        let [x, y, z] = rgb.map(i32::from);
        (i32::from(r) - x).pow(2) + (i32::from(g) - y).pow(2) + (i32::from(b) - z).pow(2)
    };
    // Black and white both plot as color 7.
    if rgb.iter().all(|c| *c < 16) || rgb.iter().all(|c| *c > 240) {
        return 7;
    }
    (1..=255).min_by_key(|i| d(*i)).unwrap_or(7)
}

/// The standard lineweight nearest `mm`.
fn lineweight(mm: f64) -> Lineweight {
    let want = (mm * 100.0).round();
    let nearest = Lineweight::STANDARD
        .into_iter()
        .min_by(|a, b| (f64::from(*a) - want).abs().total_cmp(&(f64::from(*b) - want).abs()));
    nearest.map_or(Lineweight::Default, Lineweight::Hundredths)
}

/// Where a page point goes in the drawing.
pub(crate) struct PagePlacement {
    pub origin: Point,
    /// The page's width and height as drawing vectors.
    pub u: Point,
    pub v: Point,
}

/// The objects a page draws, on PDF_Geometry, PDF_Solid Fills and
/// PDF_Text, placed by `place`: (layer, color, lineweight, geometry).
pub(crate) fn page_objects(
    page: &PdfDrawing,
    place: &PagePlacement,
) -> Vec<(&'static str, Color, Lineweight, Geometry)> {
    let to = |p: Point| place.origin + place.u * (p.x / page.width) + place.v * (p.y / page.height);
    // Drawing units per page point.
    let k = place.u.length() / page.width;
    let mut out = Vec::new();
    for path in &page.paths {
        let color = Color::Index(nearest_aci(path.rgb));
        // White fills are the page's paper.
        if path.fill && !path.rgb.iter().all(|c| *c > 245) {
            let loops: Vec<Vec<Vertex>> = path
                .parts
                .iter()
                .filter(|(pts, _)| pts.len() >= 3)
                .map(|(pts, _)| pts.iter().map(|p| Vertex::at(to(*p))).collect())
                .collect();
            if !loops.is_empty() {
                let hatch = Geometry::Hatch {
                    loops,
                    pattern: HatchPattern::Solid,
                    scale: 1.0,
                    angle: 0.0,
                    origin: Point::ORIGIN,
                };
                out.push(("PDF_Solid Fills", color, Lineweight::ByLayer, hatch));
            }
        }
        let Some(width) = path.stroke else { continue };
        let weight = lineweight(width * 25.4 / 72.0);
        for (pts, closed) in &path.parts {
            let pts: Vec<Point> = pts.iter().map(|p| to(*p)).collect();
            let g = if pts.len() == 2 && !closed {
                Geometry::Line { start: pts[0], end: pts[1] }
            } else {
                Geometry::Polyline { vertices: pts.into_iter().map(Vertex::at).collect(), closed: *closed }
            };
            out.push(("PDF_Geometry", color, weight, g));
        }
    }
    let turn = Point::ORIGIN.angle_to(place.u);
    for t in &page.texts {
        let text = Geometry::Text {
            look: Default::default(),
            position: to(t.at),
            height: t.height * k,
            rotation: t.rotation + turn,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: t.text.clone(),
        };
        out.push(("PDF_Text", Color::Index(nearest_aci(t.rgb)), Lineweight::ByLayer, text));
    }
    out
}

pub(crate) struct PdfImportDialog {
    file: PathBuf,
    pages: usize,
    page: usize,
    /// Drawing units per inch on the page.
    per_inch: f64,
}

impl OxidraftApp {
    /// PDFIMPORT: from the selected PDF underlay, or a file picked.
    pub(crate) fn pdf_import(&mut self) {
        let underlay = self.engine.selection().iter().find_map(|id| {
            let e = self.doc.entity(*id)?;
            let Geometry::Image { image, origin, u, v, .. } = &e.geometry else { return None };
            let def = self.doc.image_def(image).filter(|d| d.is_pdf())?;
            Some((def.clone(), PagePlacement { origin: *origin, u: *u, v: *v }))
        });
        if let Some((def, place)) = underlay {
            match crate::xref::resolve(self.path.as_deref(), &def.path) {
                Some(file) => {
                    self.import_pdf_page(&file, def.page.unwrap_or(0), &place);
                }
                None => self.engine.log(format!("PDF \"{}\" not found: {}", def.name, def.path)),
            }
            return;
        }
        if let Some(file) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
            self.open_pdf_import(file);
        }
    }

    pub(crate) fn open_pdf_import(&mut self, file: PathBuf) {
        let pages = oxidraft_raster::probe(&file, 0).map(|i| i.pages).unwrap_or(1).max(1);
        self.pdf_import_dialog = Some(PdfImportDialog { file, pages, page: 0, per_inch: 25.4 });
    }

    /// Imports page `page` of `file`, placed by `place`; how many objects.
    pub(crate) fn import_pdf_page(&mut self, file: &std::path::Path, page: usize, place: &PagePlacement) -> usize {
        // Its layers turned off stay out (ULAYERS).
        let read = oxidraft_raster::pdf_layers::read_pdf(file).map_err(|e| e.to_string());
        let drawing = match read.and_then(|d| pdf_vectors(d, page)) {
            Ok(d) => d,
            Err(e) => {
                self.engine.log(format!("Could not import {}: {e}", file.display()));
                return 0;
            }
        };
        let objects = page_objects(&drawing, place);
        self.engine.cancel(&mut self.doc);
        self.doc.begin("PDFIMPORT");
        for (layer, color) in [("PDF_Geometry", 7), ("PDF_Solid Fills", 7), ("PDF_Text", 7)] {
            if self.doc.layer(layer).is_none() && objects.iter().any(|o| o.0 == layer) {
                self.doc.add_layer(Layer::new(layer, color));
            }
        }
        let n = objects.len();
        for (layer, color, weight, g) in objects {
            self.doc.add_styled(g, layer, color, oxidraft_model::linetype::BYLAYER, weight);
        }
        self.doc.commit();
        self.engine.log(format!("{n} objects imported from {}.", file.display()));
        n
    }

    /// Imports a page of a PDF file at the origin, `per_inch` drawing units
    /// to the page's inch.
    pub fn import_pdf_file(&mut self, file: &std::path::Path, page: usize, per_inch: f64) -> usize {
        let Ok(info) = oxidraft_raster::probe(file, page) else {
            self.engine.log(format!("Could not read {}.", file.display()));
            return 0;
        };
        let k = per_inch / 72.0;
        let place = PagePlacement {
            origin: Point::ORIGIN,
            u: Point::new(info.width * k, 0.0),
            v: Point::new(0.0, info.height * k),
        };
        self.import_pdf_page(file, page, &place)
    }

    pub(crate) fn pdf_import_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.pdf_import_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Import PDF")
            .id(egui::Id::unique("pdf_import"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 180.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(d.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                ui.horizontal(|ui| {
                    ui.label("Page:");
                    let mut shown = d.page + 1;
                    ui.add(egui::DragValue::new(&mut shown).range(1..=d.pages));
                    d.page = shown - 1;
                    ui.label(format!("of {}", d.pages));
                });
                ui.horizontal(|ui| {
                    ui.label("Drawing units per inch:");
                    ui.add(egui::DragValue::new(&mut d.per_inch).range(0.001..=1e6).speed(0.1));
                });
                ui.label("Placed at 0,0; lines, fills and text on PDF_Geometry, PDF_Solid Fills and PDF_Text.");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok {
            self.import_pdf_file(&d.file.clone(), d.page, d.per_inch);
            return;
        }
        if open && !cancel {
            self.pdf_import_dialog = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_weights() {
        assert_eq!(nearest_aci([255, 0, 0]), 1);
        assert_eq!(nearest_aci([0, 0, 0]), 7);
        assert_eq!(lineweight(0.35), Lineweight::Hundredths(35));
    }
}

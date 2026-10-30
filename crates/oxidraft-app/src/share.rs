//! SHARE and MARKUP without Autodesk's cloud. SHARE packs what someone
//! else needs to look at the drawing into one zip file to send: the
//! drawing as DWG, a PDF of its sheets, and the files it uses. MARKUP
//! (MARKUPIMPORT) brings a marked-up plot back: the PDF or image laid over
//! the sheet it was plotted from, on layer MARKUP, to work from, and a
//! PDF's markups (comments as text, drawn markups as lines) with it, as
//! Markup Assist offers them.

use std::path::Path;

use oxidraft_geom::Point;
use oxidraft_model::{Color, Geometry, ImageDef, Layer, MODEL, XrefStatus};
use oxidraft_plot::{PlotArea, Sheet};

use crate::app::OxidraftApp;

impl OxidraftApp {
    /// SHARE: asks where the package goes, then writes it.
    pub(crate) fn share(&mut self) {
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(path) = rfd::FileDialog::new()
            .set_title("Share: the package to send")
            .add_filter("Zip", &["zip"])
            .set_file_name(format!("{stem} shared.zip"))
            .save_file()
        else {
            return;
        };
        match self.share_to(&path) {
            Ok(n) => self.engine.log(format!("Share package of {n} file(s) written to {}", path.display())),
            Err(e) => self.engine.log(e),
        }
    }

    /// Writes the share package to `path`; how many files are in it.
    pub fn share_to(&mut self, path: &Path) -> Result<usize, String> {
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let dwg = oxidraft_io_dwg::write(&self.doc, self.dwg_version.unwrap_or_default()).map_err(|e| e.to_string())?;
        files.push((format!("{stem}.dwg"), dwg));
        // Its sheets: the layouts, or what PLOT plots in model space.
        let spaces: Vec<_> = match self.doc.layouts().iter().map(|l| l.id).collect::<Vec<_>>() {
            l if l.is_empty() => vec![MODEL],
            l => l,
        };
        let sheets: Vec<Sheet> = {
            let pictures = self.plot_pictures(true);
            spaces
                .iter()
                .filter_map(|&s| {
                    let mut settings = self.plot_settings;
                    settings.area = if s == MODEL { PlotArea::Extents } else { PlotArea::Layout };
                    oxidraft_plot::render_with_pictures(&self.doc, s, &settings, &pictures)
                })
                .collect()
        };
        if !sheets.is_empty() {
            files.push((format!("{stem}.pdf"), oxidraft_plot::pdf_pages(&sheets, &self.file_name())));
        }
        // The files it uses, by name, as they're found.
        let host = self.path.clone();
        let saved: Vec<String> =
            self.doc.xrefs().map(|x| x.path.clone()).chain(self.doc.image_defs().map(|d| d.path.clone())).collect();
        for s in saved {
            if let Some(found) = crate::xref::resolve(host.as_deref(), &s)
                && let Ok(data) = std::fs::read(&found)
            {
                let name = found.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if !files.iter().any(|(n, _)| *n == name) {
                    files.push((name, data));
                }
            }
        }
        let n = files.len();
        std::fs::write(path, oxidraft_plot::zip::zip(&files))
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(n)
    }

    /// MARKUP: picks the marked-up PDF or image.
    pub(crate) fn markup_import(&mut self) {
        let Some(file) = rfd::FileDialog::new()
            .set_title("Markup Import: the marked-up plot")
            .add_filter("PDF, DWF or image", &["pdf", "dwf", "dwfx", "png", "jpg", "jpeg", "tif", "tiff", "bmp"])
            .pick_file()
        else {
            return;
        };
        if let Err(e) = self.markup_import_file(&file) {
            self.engine.log(e);
        }
    }

    /// Lays the marked-up plot `file` over the sheet it was plotted from:
    /// the layout shown on its page setup, or model space as the last plot
    /// placed it; on layer MARKUP (red), on top.
    pub fn markup_import_file(&mut self, file: &Path) -> Result<(), String> {
        let space = self.shown_space();
        let mut settings = self.plot_settings;
        settings.area = match settings.area {
            _ if space != MODEL => PlotArea::Layout,
            PlotArea::Display(_) => PlotArea::Display(self.displayed()),
            PlotArea::Layout => PlotArea::Extents,
            area => area,
        };
        let layout = settings.layout(&self.doc, space).ok_or("There's no sheet here to lay it over.")?;
        // A DWFx markup: its lines and text, in red on layer MARKUP.
        if file.extension().is_some_and(|e| ["dwfx", "dwf", "xps"].iter().any(|x| e.eq_ignore_ascii_case(x))) {
            let data = std::fs::read(file).map_err(|e| format!("Could not read {}: {e}", file.display()))?;
            let objects = crate::dwf_in::read_any_dwf(&data)?;
            let origin = layout.window.min - layout.origin * (1.0 / layout.scale);
            let placement =
                oxidraft_model::Placement { position: origin, scale: 1.0 / layout.scale, rotation: 0.0, mirror: false };
            self.engine.cancel(&mut self.doc);
            self.doc.begin("MARKUP");
            if self.doc.layer("MARKUP").is_none() {
                self.doc.add_layer(Layer::new("MARKUP", 1));
            }
            let n = objects.len();
            for (g, _) in objects {
                self.doc.add_on_layer(placement.place(Point::ORIGIN, &g), "MARKUP", Color::ByLayer);
            }
            self.doc.commit();
            self.engine.log(format!("{n} markup object(s) laid over the sheet on layer MARKUP."));
            return Ok(());
        }
        let pdf = oxidraft_raster::is_pdf(file);
        let info = oxidraft_raster::probe(file, 0)?;
        // The page over the whole sheet (an image's height as its shape has it).
        let (w_mm, h_mm) = if pdf {
            (info.width / 72.0 * 25.4, info.height / 72.0 * 25.4)
        } else {
            (layout.width, layout.width * info.height / info.width.max(1.0))
        };
        let to_drawing = |p: Point| layout.window.min + (p - layout.origin) * (1.0 / layout.scale);
        let origin = to_drawing(Point::ORIGIN);
        let u = to_drawing(Point::new(w_mm, 0.0)) - origin;
        let v = to_drawing(Point::new(0.0, h_mm)) - origin;
        let name = {
            let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Markup".into());
            let base = oxidraft_model::sanitize_name(&format!("Markup {stem}"));
            (1..)
                .map(|i| if i == 1 { base.clone() } else { format!("{base} ({i})") })
                .find(|n| self.doc.image_def(n).is_none())
                .unwrap_or(base)
        };
        let def = ImageDef {
            name: name.clone(),
            path: file.to_string_lossy().into_owned(),
            page: pdf.then_some(0),
            width: info.width,
            height: info.height,
            status: XrefStatus::Loaded,
        };
        self.engine.cancel(&mut self.doc);
        self.doc.begin("MARKUP");
        if self.doc.layer("MARKUP").is_none() {
            self.doc.add_layer(Layer::new("MARKUP", 1));
        }
        self.doc.set_image_def(def.clone());
        let image = Geometry::Image {
            adjust: oxidraft_model::PictureAdjust { transparent: true, ..Default::default() },
            image: name.clone(),
            origin,
            u,
            v,
            clip: Vec::new(),
        };
        self.doc.add_on_layer(image, "MARKUP", Color::ByLayer);
        // A PDF's markups — its comments and what reviewers drew — as text
        // and lines on layer MARKUP, as Markup Assist offers them.
        let mut markups = 0;
        if pdf && let Ok(data) = std::fs::read(file) {
            let mm = 25.4 / 72.0;
            let at = |(x, y): (f64, f64)| to_drawing(Point::new(x * mm, y * mm));
            for m in oxidraft_raster::pdf_markups::pdf_markups(&data, 0) {
                let (g, rgb) = match m {
                    oxidraft_raster::pdf_markups::Markup::Text { at: p, height, text, color } => (
                        Geometry::Text {
                            position: at(p),
                            height: height * mm * 0.7 / layout.scale,
                            rotation: 0.0,
                            width: 0.0,
                            attach: oxidraft_model::Attach::TopLeft,
                            content: text.lines().collect::<Vec<_>>().join("\\P"),
                            look: Default::default(),
                        },
                        color,
                    ),
                    oxidraft_raster::pdf_markups::Markup::Shape { points, closed, color } => {
                        let pts: Vec<Point> = points.into_iter().map(at).collect();
                        let g = if pts.len() == 2 && !closed {
                            Geometry::Line { start: pts[0], end: pts[1] }
                        } else {
                            Geometry::Polyline {
                                vertices: pts.into_iter().map(oxidraft_model::Vertex::at).collect(),
                                closed,
                            }
                        };
                        (g, color)
                    }
                };
                self.doc.add_on_layer(g, "MARKUP", Color::Index(crate::pdf_import::nearest_aci(rgb)));
                markups += 1;
            }
        }
        self.doc.commit();
        self.load_image(&name);
        let taken = if markups > 0 { format!(", its {markups} markup(s) as text and lines") } else { String::new() };
        self.engine.log(format!(
            "Markup \"{name}\" laid over the sheet on layer MARKUP{taken}: turn the layer off to compare, PDFIMPORT to take its lines."
        ));
        Ok(())
    }
}

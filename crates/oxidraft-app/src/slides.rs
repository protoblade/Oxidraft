//! MSLIDE and VSLIDE: the view saved as an AutoCAD slide (.sld) of
//! colored vectors, and a slide shown over the drawing area until REDRAW.

use std::path::Path;

use eframe::egui::{Painter, Rect, Stroke, pos2};
use oxidraft_geom::{Point, curve::Seg};
use oxidraft_model::Geometry;
use oxidraft_plot::slide::{Slide, read_sld, write_sld};

use crate::{app::OxidraftApp, theme};

/// A slide's vectors over `rect`, fitted and centered, on the background.
pub(crate) fn paint_slide(painter: &Painter, rect: Rect, slide: &Slide) {
    painter.rect_filled(rect, 0.0, theme::background_now());
    let (w, h) = (f32::from(slide.width.max(1)), f32::from(slide.height.max(1)));
    let k = (rect.width() / w).min(rect.height() / h);
    let origin = pos2(rect.center().x - w * k / 2.0, rect.center().y + h * k / 2.0);
    let at = |(x, y): (i16, i16)| pos2(origin.x + f32::from(x) * k, origin.y - f32::from(y) * k);
    for &(color, from, to) in &slide.vectors {
        painter.line_segment([at(from), at(to)], Stroke::new(1.0, theme::aci(color)));
    }
}

impl OxidraftApp {
    /// The view as a slide: what's drawn in the drawing area, in its dots.
    pub fn view_slide(&self) -> Slide {
        let rect = self.canvas_rect;
        let (w, h) = (rect.width().max(1.0), rect.height().max(1.0));
        let dot = |p: Point| {
            let s = self.view.to_screen(rect, p);
            ((s.x - rect.left()).round() as i32, (rect.bottom() - s.y).round() as i32)
        };
        let inside = |(x, y): (i32, i32)| x >= 0 && y >= 0 && x < w as i32 && y < h as i32;
        let clamp = |(x, y): (i32, i32)| (x.clamp(0, w as i32 - 1) as i16, y.clamp(0, h as i32 - 1) as i16);
        let mut vectors = Vec::new();
        for e in self.doc.space_in_draw_order(self.shown_space()) {
            if !self.doc.object_shown(e) {
                continue;
            }
            for s in self.doc.expanded_styled(e) {
                let mut lines: Vec<(Point, Point)> = Vec::new();
                let font = match &s.geometry {
                    Geometry::Text { look, .. } => self.doc.text_style(&look.style).map(|t| t.font.clone()),
                    _ => None,
                };
                if let Some(shx) = font.as_deref().and_then(oxidraft_plot::shx::shx_font) {
                    for stroke in oxidraft_plot::shx::text_strokes(shx, &s.geometry) {
                        lines.extend(stroke.windows(2).map(|p| (p[0], p[1])));
                    }
                }
                for seg in s.geometry.segments() {
                    match seg {
                        Seg::Line(l) => lines.push((l.start, l.end)),
                        Seg::Arc(a) => {
                            let n = ((a.sweep.abs() / 10.0).ceil() as usize).max(2);
                            let pts: Vec<Point> = (0..=n).map(|i| a.point_at(i as f64 / n as f64)).collect();
                            lines.extend(pts.windows(2).map(|p| (p[0], p[1])));
                        }
                    }
                }
                for (a, b) in lines {
                    let (a, b) = (dot(a), dot(b));
                    if inside(a) || inside(b) {
                        vectors.push((s.color, clamp(a), clamp(b)));
                    }
                }
            }
        }
        Slide { width: w as u16, height: h as u16, vectors }
    }

    /// MSLIDE: the view to a slide file picked.
    pub(crate) fn make_slide(&mut self) {
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(path) =
            rfd::FileDialog::new().add_filter("Slide", &["sld"]).set_file_name(format!("{stem}.sld")).save_file()
        else {
            return;
        };
        let slide = self.view_slide();
        match std::fs::write(&path, write_sld(&slide)) {
            Ok(()) => {
                self.engine.log(format!("Slide of {} vectors written to {}", slide.vectors.len(), path.display()))
            }
            Err(e) => self.engine.log(format!("Could not write {}: {e}", path.display())),
        }
    }

    /// VSLIDE: a slide file picked, shown until REDRAW.
    pub(crate) fn view_slide_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Slide", &["sld"]).pick_file() else { return };
        if let Err(e) = self.show_slide(&path) {
            self.engine.log(e);
        }
    }

    pub fn show_slide(&mut self, path: &Path) -> Result<(), String> {
        let data = std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        self.slide = Some(read_sld(&data)?);
        self.engine.log("REDRAW (or Esc) puts the drawing back.");
        Ok(())
    }

    pub fn showing_slide(&self) -> bool {
        self.slide.is_some()
    }
}

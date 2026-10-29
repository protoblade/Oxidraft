//! A PDF page's drawing as vectors, for PDFIMPORT: its strokes and fills
//! as paths (curves flattened), its text as strings placed and sized, in
//! the page's points (y up, from its lower-left corner).

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterSettings, Paint,
    SoftMask, TransformExt, interpret_page,
};
use hayro::kurbo::{Affine, BezPath, PathEl, Point as KPoint, Rect};
use oxidraft_geom::Point;

/// A stroked or filled path: its parts (each closed or not), its color
/// and, when stroked, its width in points.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfPath {
    pub parts: Vec<(Vec<Point>, bool)>,
    pub rgb: [u8; 3],
    pub fill: bool,
    pub stroke: Option<f64>,
}

/// A run of text: where it starts, how tall, at what angle (degrees).
#[derive(Debug, Clone, PartialEq)]
pub struct PdfText {
    pub text: String,
    pub at: Point,
    pub height: f64,
    pub rotation: f64,
    pub rgb: [u8; 3],
}

/// What a page draws, and its size in points.
#[derive(Debug, Default)]
pub struct PdfDrawing {
    pub width: f64,
    pub height: f64,
    pub paths: Vec<PdfPath>,
    pub texts: Vec<PdfText>,
}

fn rgb(paint: &Paint<'_>) -> [u8; 3] {
    match paint {
        Paint::Color(c) => {
            let [r, g, b, _] = c.to_rgba().components();
            [r, g, b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        }
        Paint::Pattern(_) => [0, 0, 0],
    }
}

fn point(p: KPoint) -> Point {
    Point::new(p.x, p.y)
}

/// The path's parts with curves flattened to points `tolerance` apart.
fn parts(path: &BezPath, transform: Affine) -> Vec<(Vec<Point>, bool)> {
    let mut out: Vec<(Vec<Point>, bool)> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut last = KPoint::ZERO;
    let flush = |current: &mut Vec<Point>, out: &mut Vec<(Vec<Point>, bool)>, closed: bool| {
        if current.len() >= 2 {
            out.push((std::mem::take(current), closed));
        } else {
            current.clear();
        }
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                flush(&mut current, &mut out, false);
                last = p;
                current.push(point(transform * p));
            }
            PathEl::LineTo(p) => {
                last = p;
                current.push(point(transform * p));
            }
            PathEl::QuadTo(c, p) => {
                for i in 1..=8 {
                    let t = f64::from(i) / 8.0;
                    let q = last.lerp(c, t).lerp(c.lerp(p, t), t);
                    current.push(point(transform * q));
                }
                last = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                for i in 1..=12 {
                    let t = f64::from(i) / 12.0;
                    let (a, b, c) = (last.lerp(c1, t), c1.lerp(c2, t), c2.lerp(p, t));
                    let q = a.lerp(b, t).lerp(b.lerp(c, t), t);
                    current.push(point(transform * q));
                }
                last = p;
            }
            PathEl::ClosePath => flush(&mut current, &mut out, true),
        }
    }
    flush(&mut current, &mut out, false);
    out
}

struct Collector {
    drawing: PdfDrawing,
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, draw_mode: &DrawMode) {
        let (fill, stroke) = match draw_mode {
            DrawMode::Fill(_) => (true, None),
            DrawMode::Stroke(s) => (false, Some(s)),
            DrawMode::FillAndStroke(_, s) => (true, Some(s)),
            DrawMode::Invisible => return,
        };
        let parts = parts(path, props.transform);
        if parts.is_empty() {
            return;
        }
        // A stroke's width in points: the transform's scale applied to it.
        let scale = props.transform.determinant().abs().sqrt();
        let stroke = stroke.map(|s| f64::from(s.line_width) * scale);
        self.drawing.paths.push(PdfPath { parts, rgb: rgb(&props.paint), fill, stroke });
    }

    fn push_clip_path(&mut self, _: &ClipPath) {}

    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, draw_mode: &DrawMode) {
        if matches!(draw_mode, DrawMode::Invisible) {
            return;
        }
        let glyphs = run.glyphs();
        let Some(first) = glyphs.first() else { return };
        let text: String = glyphs
            .iter()
            .filter_map(|g| g.as_unicode())
            .map(|s| match s {
                hayro::hayro_interpret::hayro_cmap::BfString::Char(c) => c.to_string(),
                hayro::hayro_interpret::hayro_cmap::BfString::String(s) => s,
            })
            .collect();
        if text.trim().is_empty() {
            return;
        }
        // The first glyph's corner, and its em square's height and slant.
        let t = props.transform * first.transform();
        let origin = t * KPoint::ZERO;
        let up = t * KPoint::new(0.0, 1.0) - origin;
        let along = t * KPoint::new(1.0, 0.0) - origin;
        // A capital stands about 0.7 of the em square.
        let height = up.hypot() * 0.7;
        if height <= 0.0 {
            return;
        }
        self.drawing.texts.push(PdfText {
            text,
            at: point(origin),
            height,
            rotation: along.y.atan2(along.x).to_degrees(),
            rgb: rgb(&props.paint),
        });
    }

    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}

    fn pop_clip(&mut self) {}

    fn pop_transparency_group(&mut self) {}
}

/// Page `page` (from 0) of the PDF in `data`, as vectors.
pub fn pdf_vectors(data: Vec<u8>, page: usize) -> Result<PdfDrawing, String> {
    let pdf = hayro::hayro_syntax::Pdf::new(data).map_err(|e| format!("not a readable PDF ({e:?})"))?;
    let pages = pdf.pages();
    let p = pages.get(page).ok_or_else(|| format!("the PDF has {} pages", pages.len()))?;
    let (w, h) = p.render_dimensions();
    let transform = p.initial_transform(false).to_kurbo();
    let cache = hayro::hayro_interpret::InterpreterCache::new();
    let mut context = Context::new(
        transform,
        Rect::new(0.0, 0.0, f64::from(w), f64::from(h)),
        &cache,
        p.xref(),
        InterpreterSettings::default(),
    );
    let mut collector =
        Collector { drawing: PdfDrawing { width: f64::from(w), height: f64::from(h), ..Default::default() } };
    interpret_page(p, &mut context, &mut collector);
    Ok(collector.drawing)
}

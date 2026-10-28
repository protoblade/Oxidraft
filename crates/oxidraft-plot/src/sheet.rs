//! The plotted sheet: the drawing as strokes, fills and text in
//! millimeters on the paper, ready to write as PDF or show as a preview.

use std::collections::HashMap;
use std::sync::Arc;

use oxidraft_geom::{Aabb, Point, curve::Seg, intersect::clip_segment};
use oxidraft_model::{Document, Geometry, HatchPattern, Styled, Vertex, constant_width, hatch, linetype};

use crate::{
    font::{self, FONT},
    setup::{Layout, PlotSettings},
};

pub type Rgb = [u8; 3];

/// Arcs are drawn as straight pieces this close to them, in millimeters.
const TOLERANCE: f64 = 0.01;

/// A line through points, open or closed.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub points: Vec<Point>,
    pub closed: bool,
}

/// One thing drawn on the sheet, in millimeters from its lower-left corner.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Lines `width` millimeters wide (0: as thin as the device draws), with
    /// round ends; `wide` ones are polylines with a width, with square ends
    /// and mitered corners.
    Stroke { paths: Vec<Path>, width: f64, color: Rgb, wide: bool },
    /// Areas filled even-odd.
    Fill { polygons: Vec<Vec<Point>>, color: Rgb },
    /// A line of text from the start of its baseline; `size` is the font
    /// size in millimeters.
    Text { at: Point, rotation: f64, size: f64, text: String, color: Rgb },
    /// Picture `name` over the parallelogram from `origin` along `u` and
    /// `v`, inside `clip` when that has points; in grays for monochrome
    /// and grayscale pen tables.
    Image { name: String, origin: Point, u: Point, v: Point, clip: Vec<Point>, gray: bool },
}

/// An image's pixels, RGBA, rows top to bottom.
#[derive(Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A plotted sheet, with the pictures its images show.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub layout: Layout,
    pub items: Vec<Item>,
    pub pictures: HashMap<String, Arc<Picture>>,
}

struct Plotter<'a> {
    doc: &'a Document,
    settings: &'a PlotSettings,
    layout: Layout,
    items: Vec<Item>,
    lookup: &'a dyn Fn(&str) -> Option<Arc<Picture>>,
    pictures: HashMap<String, Arc<Picture>>,
}

/// How many straight pieces draw an arc of radius `r` and `sweep` radians
/// (both on paper) within [`TOLERANCE`].
fn arc_steps(r: f64, sweep: f64) -> usize {
    if r <= TOLERANCE {
        return 1;
    }
    let step = 2.0 * (1.0 - TOLERANCE / r).acos();
    ((sweep.abs() / step).ceil() as usize).clamp(2, 4096)
}

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    a.max.x >= b.min.x && a.min.x <= b.max.x && a.max.y >= b.min.y && a.min.y <= b.max.y
}

impl Plotter<'_> {
    fn paper(&self, p: Point) -> Point {
        self.layout.to_paper(p)
    }

    /// A curve on paper, arcs split finely.
    fn path(&self, g: &Geometry) -> Vec<Point> {
        let mut points = Vec::new();
        for seg in g.segments() {
            let steps = match seg {
                Seg::Line(_) => 1,
                Seg::Arc(a) => arc_steps(a.radius * self.layout.scale, a.sweep.to_radians()),
            };
            let first = usize::from(!points.is_empty());
            for i in first..=steps {
                points.push(self.paper(seg.point_at(i as f64 / steps as f64)));
            }
        }
        points
    }

    fn push_stroke(&mut self, paths: Vec<Path>, width: f64, color: Rgb, wide: bool) {
        if paths.is_empty() {
            return;
        }
        // Runs of strokes alike share one item.
        if let Some(Item::Stroke { paths: last, width: w, color: c, wide: v }) = self.items.last_mut()
            && (*w, *c, *v) == (width, color, wide)
        {
            last.extend(paths);
            return;
        }
        self.items.push(Item::Stroke { paths, width, color, wide });
    }

    /// A path in the object's linetype: dashes scaled by LTSCALE and the
    /// plot scale; too fine to see, solid.
    fn dashed(&mut self, points: Vec<Point>, closed: bool, s: &Styled, width: f64, color: Rgb) {
        let pattern = self.doc.linetype(&s.linetype).filter(|l| !l.is_continuous()).map(|l| {
            let k = self.doc.ltscale() * self.layout.scale;
            (l.length() * k, l.pattern.iter().map(|d| d * k).collect::<Vec<f64>>())
        });
        let paths = match pattern {
            Some((length, pattern)) if length >= 0.5 => {
                let mut points = points;
                if closed && let Some(&first) = points.first() {
                    points.push(first);
                }
                linetype::dashes(&points, &pattern, 1.0)
                    .into_iter()
                    .map(|run| Path { points: if run.len() == 1 { vec![run[0], run[0]] } else { run }, closed: false })
                    .collect()
            }
            _ => vec![Path { points, closed }],
        };
        self.push_stroke(paths, width, color, false);
    }

    fn styled(&mut self, s: &Styled) {
        let color = self.settings.style.rgb(s.color);
        let width = if self.settings.lineweights { s.lineweight.mm().unwrap_or(0.25) } else { 0.0 };
        let g = &s.geometry;
        match g {
            // Unloaded images don't plot; frames don't either (IMAGEFRAME 2).
            Geometry::Image { image, origin, u, v, clip } => {
                let Some(picture) = (self.lookup)(image) else { return };
                self.pictures.insert(image.clone(), picture);
                let paper = |q: Point| self.paper(q);
                let (o, pu, pv) =
                    (paper(*origin), paper(*origin + *u) - paper(*origin), paper(*origin + *v) - paper(*origin));
                let clip = clip.iter().map(|q| paper(*q)).collect();
                let gray = self.settings.style != crate::PlotStyle::None;
                self.items.push(Item::Image { name: image.clone(), origin: o, u: pu, v: pv, clip, gray });
            }
            Geometry::Text { .. } => self.text(g, color),
            Geometry::Hatch { .. } => self.hatch(g, color, width),
            Geometry::Point { position } => self.point(*position, color, width),
            Geometry::Polyline { vertices, closed }
                if vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0) =>
            {
                self.wide(vertices, *closed, color);
            }
            _ if g.is_infinite() => {
                let Some(Seg::Line(line)) = g.segments().first().copied() else { return };
                if let Some(c) = clip_segment(line, &self.layout.window) {
                    self.dashed(vec![self.paper(c.start), self.paper(c.end)], false, s, width, color);
                }
            }
            _ => {
                let mut points = self.path(g);
                let closed = g.is_closed();
                if closed && points.len() > 2 && points[0].distance(points[points.len() - 1]) < 1e-9 {
                    points.pop();
                }
                if points.len() >= 2 {
                    self.dashed(points, closed, s, width, color);
                }
            }
        }
    }

    /// Text placed as the drawing area places it: lines set left, the block
    /// anchored by its attachment between the first line's capitals and
    /// the last line's baseline.
    fn text(&mut self, g: &Geometry, color: Rgb) {
        let Geometry::Text { position, height, rotation, width, attach, content } = g else { return };
        let size = height / 0.7;
        let lines = font::lines(content, if *width > 0.0 { width / size } else { 0.0 });
        let widest = lines.iter().map(|l| FONT.width(l)).fold(0.0, f64::max);
        let row = FONT.row_height();
        let span = 0.7 + (lines.len() as f64 - 1.0) * row;
        let (fx, fy) = attach.fractions();
        for (i, line) in lines.into_iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let local = Point::new(-fx * widest, -0.7 + fy * span - i as f64 * row) * size;
            let at = *position + local.rotated_about(Point::ORIGIN, *rotation);
            let item = Item::Text {
                at: self.paper(at),
                rotation: *rotation,
                size: size * self.layout.scale,
                text: line,
                color,
            };
            self.items.push(item);
        }
    }

    fn hatch(&mut self, g: &Geometry, color: Rgb, width: f64) {
        let Geometry::Hatch { loops, pattern, scale, angle } = g else { return };
        let polys = hatch::polygons(loops);
        if *pattern == HatchPattern::Solid {
            let polygons = polys.iter().map(|p| p.iter().map(|q| self.paper(*q)).collect()).collect();
            self.items.push(Item::Fill { polygons, color });
        } else if let Some(lines) = hatch::pattern_lines(&polys, *pattern, *scale, *angle, 200_000) {
            let paths =
                lines.into_iter().map(|(a, b)| Path { points: vec![self.paper(a), self.paper(b)], closed: false });
            self.push_stroke(paths.collect(), width, color, false);
        }
    }

    /// A point in the PDMODE style; PDSIZE is relative to the plotted area.
    fn point(&mut self, at: Point, color: Rgb, width: f64) {
        let (mode, size) = (self.doc.pdmode(), self.doc.pdsize());
        let area = self.layout.window.height() * self.layout.scale;
        let full = match size {
            s if s > 0.0 => s * self.layout.scale,
            s if s < 0.0 => area * -s / 100.0,
            _ => area * 0.05,
        };
        let (c, h) = (self.paper(at), full / 2.0);
        let p = |x: f64, y: f64| c + Point::new(x, y);
        let open = |points: Vec<Point>| Path { points, closed: false };
        let mut paths = match mode & 31 {
            0 => vec![open(vec![c, c])],
            2 => vec![open(vec![p(-h, 0.0), p(h, 0.0)]), open(vec![p(0.0, -h), p(0.0, h)])],
            3 => vec![open(vec![p(-h, -h), p(h, h)]), open(vec![p(-h, h), p(h, -h)])],
            4 => vec![open(vec![c, p(0.0, h)])],
            _ => Vec::new(),
        };
        if mode & 32 != 0 {
            let n = arc_steps(h, std::f64::consts::TAU);
            let ring = (0..n).map(|i| c.polar(h, 360.0 * i as f64 / n as f64)).collect();
            paths.push(Path { points: ring, closed: true });
        }
        if mode & 64 != 0 {
            paths.push(Path { points: vec![p(-h, -h), p(h, -h), p(h, h), p(-h, h)], closed: true });
        }
        // A dot needs some width to show.
        self.push_stroke(paths, if mode & 31 == 0 { width.max(0.25) } else { width }, color, false);
    }

    /// Wide polylines plot filled: one width throughout as a wide line,
    /// tapers as a band per segment.
    fn wide(&mut self, vertices: &[Vertex], closed: bool, color: Rgb) {
        let g = Geometry::Polyline { vertices: vertices.to_vec(), closed };
        if let Some(w) = constant_width(vertices) {
            let mut points = self.path(&g);
            if closed && points.len() > 2 && points[0].distance(points[points.len() - 1]) < 1e-9 {
                points.pop();
            }
            self.push_stroke(vec![Path { points, closed }], w * self.layout.scale, color, true);
            return;
        }
        let mut polygons = Vec::new();
        for (seg, v) in g.segments().iter().zip(vertices) {
            let steps = match seg {
                Seg::Line(_) => 1,
                Seg::Arc(a) => arc_steps(a.radius * self.layout.scale, a.sweep.to_radians()),
            };
            let (mut left, mut right) = (Vec::new(), Vec::new());
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                let half = (v.start_width + (v.end_width - v.start_width) * t) / 2.0;
                let (p, d) = (seg.point_at(t), seg.tangent_at(t));
                let n = Point::new(-d.y, d.x) * half;
                left.push(self.paper(p + n));
                right.push(self.paper(p - n));
            }
            right.reverse();
            left.extend(right);
            polygons.push(left);
        }
        self.items.push(Item::Fill { polygons, color });
    }
}

/// The sheet `settings` plot `doc` on, or `None` when there's nothing to
/// plot; images draw as nothing. Hatches go first, behind everything
/// else, as on screen.
pub fn render(doc: &Document, settings: &PlotSettings) -> Option<Sheet> {
    render_with_pictures(doc, settings, &|_| None)
}

/// Like [`render`], with images showing the pictures `lookup` has for
/// them (loaded ones); images go behind even hatches.
pub fn render_with_pictures(
    doc: &Document,
    settings: &PlotSettings,
    lookup: &dyn Fn(&str) -> Option<Arc<Picture>>,
) -> Option<Sheet> {
    let layout = settings.layout(doc)?;
    let mut plotter = Plotter { doc, settings, layout, items: Vec::new(), lookup, pictures: HashMap::new() };
    let rank = |g: &Geometry| match g {
        Geometry::Image { .. } => 0,
        Geometry::Hatch { .. } => 1,
        _ => 2,
    };
    let mut entities: Vec<_> = doc.entities().collect();
    entities.sort_by_key(|e| rank(&e.geometry));
    for e in entities {
        if !doc.is_shown(&e.layer) || (!e.geometry.is_infinite() && !overlaps(&doc.entity_bounds(e), &layout.window)) {
            continue;
        }
        for s in doc.expanded_styled(e) {
            plotter.styled(&s);
        }
    }
    Some(Sheet { layout, items: plotter.items, pictures: plotter.pictures })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{PlotArea, PlotScale, PlotStyle};
    use oxidraft_model::{Attach, Color, Layer, Lineweight};

    fn settings(window: Aabb) -> PlotSettings {
        PlotSettings {
            area: PlotArea::Window(window),
            scale: PlotScale::Custom { mm: 1.0, units: 1.0 },
            center: false,
            ..PlotSettings::default()
        }
    }

    #[test]
    fn lines_land_on_the_paper_in_their_color_and_weight() {
        let mut doc = Document::new();
        doc.add_layer(Layer::new("Red", 1));
        let id = doc.add_on_layer(
            Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) },
            "Red",
            Color::ByLayer,
        );
        doc.update(id, |e| e.lineweight = Lineweight::Hundredths(50));
        doc.add(Geometry::Circle { center: Point::new(50.0, 50.0), radius: 5.0 });
        let window = Aabb::from_points(Point::ORIGIN, Point::new(100.0, 100.0));
        let sheet = render(&doc, &settings(window)).unwrap();
        let Item::Stroke { paths, width, color, .. } = &sheet.items[0] else { panic!() };
        assert_eq!((*width, *color), (0.5, [255, 0, 0]));
        assert_eq!(paths[0].points, vec![Point::new(5.0, 5.0), Point::new(15.0, 5.0)]);
        // Color 7 plots black; the circle is closed and smooth to 0.01 mm.
        let Item::Stroke { paths, color, .. } = &sheet.items[1] else { panic!() };
        assert_eq!(*color, [0, 0, 0]);
        assert!(paths[0].closed && paths[0].points.len() >= 40);
        // Monochrome, without lineweights.
        let mono = PlotSettings { style: PlotStyle::Monochrome, lineweights: false, ..settings(window) };
        let Item::Stroke { width, color, .. } = &render(&doc, &mono).unwrap().items[0] else { panic!() };
        assert_eq!((*width, *color), (0.0, [0, 0, 0]));
    }

    #[test]
    fn dashes_text_and_wide_polylines() {
        let mut doc = Document::new();
        let id = doc.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(50.0, 0.0) });
        doc.update(id, |e| e.linetype = "DASHED".into());
        doc.add(Geometry::Text {
            position: Point::new(10.0, 20.0),
            height: 3.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "A\\PB".into(),
        });
        doc.add(Geometry::Polyline {
            vertices: vec![
                Vertex::at(Point::new(0.0, 40.0)).with_widths(2.0, 2.0),
                Vertex::at(Point::new(20.0, 40.0)).with_widths(2.0, 2.0),
            ],
            closed: false,
        });
        let sheet = render(&doc, &settings(Aabb::from_points(Point::ORIGIN, Point::new(60.0, 60.0)))).unwrap();
        // DASHED is 12.7 on, 6.35 off: three dashes in 50.
        let Item::Stroke { paths, .. } = &sheet.items[0] else { panic!() };
        assert_eq!(paths.len(), 3);
        assert!(paths[1].points[0].approx_eq(Point::new(5.0 + 19.05, 5.0)));
        // Two lines; the last one's baseline sits on the insertion point.
        let texts: Vec<&Item> = sheet.items.iter().filter(|i| matches!(i, Item::Text { .. })).collect();
        assert_eq!(texts.len(), 2);
        let Item::Text { at, size, .. } = texts[1] else { panic!() };
        assert!(at.approx_eq(Point::new(15.0, 25.0)) && (size - 5.0).abs() < 1e-9);
        assert!(matches!(sheet.items.last(), Some(Item::Stroke { width: 2.0, wide: true, .. })));
    }
}

//! The plotted sheet: the drawing as strokes, fills and text in
//! millimeters on the paper, ready to write as PDF or show as a preview.

use std::collections::HashMap;
use std::sync::Arc;

use oxidraft_geom::{Aabb, Point, curve::Seg, intersect::clip_segment};
use oxidraft_model::{
    Document, Entity, Geometry, HatchPattern, MODEL, Space, Styled, Vertex, constant_width, hatch, linetype,
};

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
    /// A line of text from its baseline's start: `width_factor` stretches
    /// it along the line, `oblique` slants it (degrees, leaning right).
    Text { at: Point, rotation: f64, size: f64, text: String, color: Rgb, width_factor: f64, oblique: f64 },
    /// Picture `name` over the parallelogram from `origin` along `u` and
    /// `v`, inside `clip` when that has points; in grays for monochrome
    /// and grayscale pen tables.
    Image { name: String, origin: Point, u: Point, v: Point, clip: Vec<Point>, gray: bool },
    /// What a layout viewport shows: `items` cut to its frame, and to
    /// `shape` too when it isn't rectangular.
    Clipped { clip: Aabb, shape: Vec<Point>, items: Vec<Item> },
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

/// Model space as a layout viewport shows it.
#[derive(Clone, Copy)]
struct Through {
    /// The viewport's center on the sheet, and the model point shown there.
    center: Point,
    view_center: Point,
    /// Paper units per model unit.
    k: f64,
    /// Degrees model space shows turned in it.
    twist: f64,
    /// The part of model space it shows.
    window: Aabb,
}

struct Plotter<'a> {
    doc: &'a Document,
    settings: &'a PlotSettings,
    layout: Layout,
    items: Vec<Item>,
    lookup: &'a dyn Fn(&str) -> Option<Arc<Picture>>,
    pictures: HashMap<String, Arc<Picture>>,
    /// Plotting model space through a viewport.
    through: Option<Through>,
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

impl<'a> Plotter<'a> {
    fn paper(&self, p: Point) -> Point {
        match self.through {
            Some(t) => {
                self.layout.to_paper(t.center + ((p - t.view_center) * t.k).rotated_about(Point::ORIGIN, t.twist))
            }
            None => self.layout.to_paper(p),
        }
    }

    /// Degrees what's plotted turns: a viewport's twist.
    fn twist(&self) -> f64 {
        self.through.map_or(0.0, |t| t.twist)
    }

    /// Millimeters on paper per drawing unit.
    fn unit(&self) -> f64 {
        self.layout.scale * self.through.map_or(1.0, |t| t.k)
    }

    /// Plots `entities` that show and fall in `window`, but those on the
    /// `frozen` layers. A layout's viewports show model space through them.
    fn objects(&mut self, entities: Vec<&'a Entity>, window: Aabb, frozen: &[String]) {
        let doc = self.doc;
        for e in entities {
            if !doc.is_shown(&e.layer) || frozen.iter().any(|f| f.eq_ignore_ascii_case(&e.layer)) {
                continue;
            }
            if !e.geometry.is_infinite() && !overlaps(&doc.entity_bounds(e), &window) {
                continue;
            }
            if self.through.is_none() {
                self.viewport(&e.geometry);
            }
            for s in doc.expanded_styled(e) {
                self.styled(&s);
            }
        }
    }

    /// A viewport that's on: model space at its scale, cut to its frame
    /// (which plots as its own lines).
    fn viewport(&mut self, g: &'a Geometry) {
        let Geometry::Viewport {
            center,
            width,
            height,
            view_center,
            view_height,
            twist,
            frozen,
            on: true,
            clip: boundary,
            ..
        } = g
        else {
            return;
        };
        if *view_height <= 0.0 || *height <= 0.0 {
            return;
        }
        let k = height / view_height;
        let half = Point::new(width / 2.0, height / 2.0);
        // What it shows of model space: its frame, turned back by its twist.
        let frame = Aabb::from_points(*center - half, *center + half);
        let back =
            frame.corners().map(|c| *view_center + ((c - *center) * (1.0 / k)).rotated_about(Point::ORIGIN, -twist));
        let window = back.iter().fold(Aabb::from_points(back[0], back[0]), |b, p| b.union(Aabb::from_points(*p, *p)));
        let clip = Aabb::from_points(self.layout.to_paper(*center - half), self.layout.to_paper(*center + half));
        let sheet = std::mem::take(&mut self.items);
        self.through = Some(Through { center: *center, view_center: *view_center, k, twist: *twist, window });
        self.objects(self.doc.space_in_draw_order(MODEL), window, frozen);
        self.through = None;
        let items = std::mem::replace(&mut self.items, sheet);
        if !items.is_empty() {
            let shape = boundary.iter().map(|p| self.layout.to_paper(*p)).collect();
            self.items.push(Item::Clipped { clip, shape, items });
        }
    }

    /// A curve on paper, arcs split finely.
    fn path(&self, g: &Geometry) -> Vec<Point> {
        let mut points = Vec::new();
        for seg in g.segments() {
            let steps = match seg {
                Seg::Line(_) => 1,
                Seg::Arc(a) => arc_steps(a.radius * self.unit(), a.sweep.to_radians()),
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
    /// plot scale; too fine to see, solid. Through a viewport they're as
    /// long as on the sheet (PSLTSCALE 1), whatever its scale.
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
        // A plot style table's lineweight for the color, else the object's.
        let width = match self.settings.style.lineweight(s.color) {
            Some(w) => w,
            None if self.settings.lineweights => s.lineweight.mm().unwrap_or(0.25),
            None => 0.0,
        };
        let g = &s.geometry;
        match g {
            // Unloaded images don't plot; frames do with IMAGEFRAME (PDFFRAME) 1.
            Geometry::Image { image, origin, u, v, clip, adjust } => {
                let paper = |q: Point| self.paper(q);
                let ring: Vec<Point> = if clip.len() >= 3 {
                    clip.clone()
                } else {
                    oxidraft_model::image_corners(*origin, *u, *v).to_vec()
                };
                let frame: Vec<Point> = ring.iter().map(|q| paper(*q)).collect();
                let (o, pu, pv) =
                    (paper(*origin), paper(*origin + *u) - paper(*origin), paper(*origin + *v) - paper(*origin));
                let paper_clip: Vec<Point> = clip.iter().map(|q| paper(*q)).collect();
                if let Some(picture) = (self.lookup)(image) {
                    let pdf = self.doc.image_def(image).is_some_and(|d| d.is_pdf());
                    // Adjusted (ADJUST): its own pixels, under a name of their own.
                    let shown = adjust.shown(pdf);
                    let name = if shown.is_plain() {
                        image.clone()
                    } else {
                        format!("{image}|{}-{}-{}-{}", shown.brightness, shown.contrast, shown.fade, shown.monochrome)
                    };
                    if !self.pictures.contains_key(&name) {
                        let picture = if shown.is_plain() {
                            picture
                        } else {
                            let mut rgba = picture.rgba.clone();
                            shown.apply(&mut rgba);
                            Arc::new(Picture { width: picture.width, height: picture.height, rgba })
                        };
                        self.pictures.insert(name.clone(), picture);
                    }
                    let gray = self.settings.style != crate::PlotStyle::None;
                    self.items.push(Item::Image { name, origin: o, u: pu, v: pv, clip: paper_clip, gray });
                }
                if self.doc.frame_of(image) == 1 {
                    self.push_stroke(vec![Path { points: frame, closed: true }], width, color, false);
                }
            }
            Geometry::Text { .. } => self.text(g, color),
            // White paper over what's under it, and its frame (WIPEOUTFRAME 1).
            Geometry::Wipeout { points } => {
                let polygon: Vec<Point> = points.iter().map(|p| self.paper(*p)).collect();
                self.items.push(Item::Fill { polygons: vec![polygon.clone()], color: [255, 255, 255] });
                if self.doc.wipeout_frame() == 1 {
                    self.push_stroke(vec![Path { points: polygon, closed: true }], width, color, false);
                }
            }
            Geometry::Hatch { .. } => self.hatch(g, color, width),
            Geometry::Point { position } => self.point(*position, color, width),
            Geometry::Polyline { vertices, closed }
                if vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0) =>
            {
                self.wide(vertices, *closed, color);
            }
            _ if g.is_infinite() => {
                let Some(Seg::Line(line)) = g.segments().first().copied() else { return };
                let window = self.through.map_or(self.layout.window, |t| t.window);
                if let Some(c) = clip_segment(line, &window) {
                    self.dashed(vec![self.paper(c.start), self.paper(c.end)], false, s, width, color);
                }
            }
            _ => {
                let mut points = self.path(g);
                // A viewport's frame closes like a rectangle's.
                let closed = g.is_closed() || matches!(g, Geometry::Viewport { .. });
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
        let Geometry::Text { position, height, rotation, width, attach, content, look } = g else { return };
        let size = height / 0.7;
        let wf = if look.width_factor > 0.0 { look.width_factor } else { 1.0 };
        let lines = font::lines(content, if *width > 0.0 { width / (size * wf) } else { 0.0 });
        let widest = lines.iter().map(|l| FONT.width(l)).fold(0.0, f64::max) * wf;
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
                rotation: rotation + self.twist(),
                size: size * self.unit(),
                text: line,
                color,
                width_factor: wf,
                oblique: look.oblique,
            };
            self.items.push(item);
        }
    }

    fn hatch(&mut self, g: &Geometry, color: Rgb, width: f64) {
        let Geometry::Hatch { loops, pattern, scale, angle } = g else { return };
        let polys = hatch::polygons(loops);
        if let HatchPattern::Gradient { kind, from, to } = *pattern {
            // Small triangles, each in the color halfway through it.
            let (from, to) = (self.settings.style.rgb(from), self.settings.style.rgb(to));
            for t in hatch::gradient_triangles(&polys, kind, *angle) {
                let mid = (t[0].1 + t[1].1 + t[2].1) / 3.0;
                let polygon = t.iter().map(|(p, _)| self.paper(*p)).collect();
                self.items.push(Item::Fill { polygons: vec![polygon], color: hatch::blend_rgb(from, to, mid) });
            }
        } else if *pattern == HatchPattern::Solid {
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
            s if s > 0.0 => s * self.unit(),
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
            self.push_stroke(vec![Path { points, closed }], w * self.unit(), color, true);
            return;
        }
        let mut polygons = Vec::new();
        for (seg, v) in g.segments().iter().zip(vertices) {
            let steps = match seg {
                Seg::Line(_) => 1,
                Seg::Arc(a) => arc_steps(a.radius * self.unit(), a.sweep.to_radians()),
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

/// The sheet `settings` plot `space` of `doc` on (model space, or a
/// layout with model space through its viewports), or `None` when there's
/// nothing to plot; images draw as nothing. Objects plot in draw order, as
/// on screen.
pub fn render(doc: &Document, space: Space, settings: &PlotSettings) -> Option<Sheet> {
    render_with_pictures(doc, space, settings, &|_| None)
}

/// Like [`render`], with images showing the pictures `lookup` has for
/// them (loaded ones).
pub fn render_with_pictures(
    doc: &Document,
    space: Space,
    settings: &PlotSettings,
    lookup: &dyn Fn(&str) -> Option<Arc<Picture>>,
) -> Option<Sheet> {
    let layout = settings.layout(doc, space)?;
    let mut plotter =
        Plotter { doc, settings, layout, items: Vec::new(), lookup, pictures: HashMap::new(), through: None };
    plotter.objects(doc.space_in_draw_order(space), layout.window, &[]);
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
        let sheet = render(&doc, MODEL, &settings(window)).unwrap();
        let Item::Stroke { paths, width, color, .. } = &sheet.items[0] else { panic!() };
        assert_eq!((*width, *color), (0.5, [255, 0, 0]));
        assert_eq!(paths[0].points, vec![Point::new(5.0, 5.0), Point::new(15.0, 5.0)]);
        // Color 7 plots black; the circle is closed and smooth to 0.01 mm.
        let Item::Stroke { paths, color, .. } = &sheet.items[1] else { panic!() };
        assert_eq!(*color, [0, 0, 0]);
        assert!(paths[0].closed && paths[0].points.len() >= 40);
        // Monochrome, without lineweights.
        let mono = PlotSettings { style: PlotStyle::Monochrome, lineweights: false, ..settings(window) };
        let Item::Stroke { width, color, .. } = &render(&doc, MODEL, &mono).unwrap().items[0] else { panic!() };
        assert_eq!((*width, *color), (0.0, [0, 0, 0]));
    }

    #[test]
    fn a_layout_plots_its_sheet_and_model_space_through_its_viewports() {
        let mut doc = Document::new();
        doc.add_layer(Layer::new("Trees", 3));
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(1000.0, 0.0) });
        doc.add_on_layer(Geometry::Circle { center: Point::new(500.0, 0.0), radius: 50.0 }, "Trees", Color::ByLayer);
        let paper = oxidraft_model::Paper {
            width: 297.0,
            height: 210.0,
            margins: [10.0, 10.0, 10.0, 10.0],
            millimeters: true,
            ..Default::default()
        };
        let space = doc.add_layout(oxidraft_model::layout::Layout { id: 0, name: "L".into(), tab_order: 1, paper });
        doc.set_space(space);
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(277.0, 0.0) });
        // 1:10, showing model 0..1000 x -250..250 in 100 x 50 at 150, 100.
        let viewport = doc.add(Geometry::Viewport {
            center: Point::new(150.0, 100.0),
            width: 100.0,
            height: 50.0,
            view_center: Point::new(500.0, 0.0),
            view_height: 500.0,
            twist: 0.0,
            frozen: vec!["Trees".into()],
            locked: false,
            on: true,
            clip: Vec::new(),
        });
        doc.set_space(MODEL);
        let settings = PlotSettings { area: PlotArea::Layout, ..PlotSettings::default() };
        let sheet = render(&doc, space, &settings).unwrap();
        assert_eq!((sheet.layout.width, sheet.layout.height), (297.0, 210.0));
        // The sheet's line, at its place on paper (origin 10 mm in).
        let Item::Stroke { paths, .. } = &sheet.items[0] else { panic!("{:?}", sheet.items[0]) };
        assert_eq!(paths[0].points, vec![Point::new(10.0, 10.0), Point::new(287.0, 10.0)]);
        // Model space through the viewport, cut to its frame; Trees frozen there.
        let Item::Clipped { clip, items, .. } = &sheet.items[1] else { panic!("{:?}", sheet.items[1]) };
        assert_eq!(*clip, Aabb::from_points(Point::new(110.0, 85.0), Point::new(210.0, 135.0)));
        assert_eq!(items.len(), 1, "the line, not the frozen circle");
        let Item::Stroke { paths, .. } = &items[0] else { panic!() };
        assert!(paths[0].points[0].approx_eq(Point::new(110.0, 110.0)));
        assert!(paths[0].points[1].approx_eq(Point::new(210.0, 110.0)));
        // Then the viewport's own frame.
        assert!(matches!(&sheet.items[2], Item::Stroke { paths, .. } if paths[0].closed), "{:?}", sheet.items.get(2));
        // Twisted a quarter turn, the model's line stands up in the frame.
        doc.update(viewport, |e| {
            if let Geometry::Viewport { twist, .. } = &mut e.geometry {
                *twist = 90.0;
            }
        });
        let sheet = render(&doc, space, &settings).unwrap();
        let Item::Clipped { items, .. } = &sheet.items[1] else { panic!("{:?}", sheet.items[1]) };
        let Item::Stroke { paths, .. } = &items[0] else { panic!() };
        let (a, b) = (paths[0].points[0], paths[0].points[1]);
        assert!((a.x - 160.0).abs() < 1e-9 && (b.x - 160.0).abs() < 1e-9, "{a:?} {b:?}");
        assert!(((a.y - b.y).abs() - 100.0).abs() < 1e-9);
        // A viewport turned off shows nothing through it.
        doc.update(viewport, |e| {
            if let Geometry::Viewport { on, .. } = &mut e.geometry {
                *on = false;
            }
        });
        let sheet = render(&doc, space, &settings).unwrap();
        assert!(!sheet.items.iter().any(|i| matches!(i, Item::Clipped { .. })));
    }

    #[test]
    fn dashes_text_and_wide_polylines() {
        let mut doc = Document::new();
        let id = doc.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(50.0, 0.0) });
        doc.update(id, |e| e.linetype = "DASHED".into());
        doc.add(Geometry::Text {
            look: Default::default(),
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
        let sheet = render(&doc, MODEL, &settings(Aabb::from_points(Point::ORIGIN, Point::new(60.0, 60.0)))).unwrap();
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

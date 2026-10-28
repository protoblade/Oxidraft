use oxidraft_geom::{
    Aabb, EPSILON, Point, Segment,
    curve::{ArcSeg, Seg},
};

use crate::{
    annotation::{self, Attach},
    hatch::{self, HatchPattern},
};

/// Stable identifier of an entity within one document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u64);

/// Entity color, following DXF conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    /// Use the color of the entity's layer.
    #[default]
    ByLayer,
    /// AutoCAD Color Index, 1..=255 (1 red, 2 yellow, 3 green, 4 cyan, 5 blue, 6 magenta, 7 white/black).
    Index(u8),
}

/// A polyline vertex. `bulge` describes the segment that starts here:
/// 0 is straight, otherwise the tangent of a quarter of the arc's sweep
/// (negative = clockwise), as in DXF.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub point: Point,
    pub bulge: f64,
}

impl Vertex {
    pub const fn new(point: Point, bulge: f64) -> Self {
        Self { point, bulge }
    }

    pub const fn at(point: Point) -> Self {
        Self { point, bulge: 0.0 }
    }
}

/// The shape of an entity.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    Line {
        start: Point,
        end: Point,
    },
    Circle {
        center: Point,
        radius: f64,
    },
    /// Counterclockwise from `start_angle` to `end_angle`, in degrees (as in DXF).
    Arc {
        center: Point,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    /// A 2D polyline of straight and arc segments (DXF LWPOLYLINE).
    Polyline {
        vertices: Vec<Vertex>,
        closed: bool,
    },
    /// A block reference (DXF INSERT): the named block's contents placed in
    /// the drawing. Its curves come from [`crate::Document::expanded`].
    Insert {
        block: String,
        placement: Placement,
    },
    /// Text (MTEXT, or single-line TEXT with a bottom-left attachment).
    /// `content` may hold several lines; `width` > 0 wraps long lines.
    Text {
        position: Point,
        height: f64,
        rotation: f64,
        width: f64,
        attach: Attach,
        content: String,
    },
    /// A linear dimension measuring `p1` to `p2`, with its dimension line
    /// through `line`. Rotated (DIMLINEAR) measures along `rotation`;
    /// aligned (DIMALIGNED) along `p1 -> p2`. `text` overrides the value
    /// (`<>` stands for the measurement). `scale` is DIMSCALE.
    Dimension {
        p1: Point,
        p2: Point,
        line: Point,
        rotation: f64,
        aligned: bool,
        text: Option<String>,
        scale: f64,
    },
    /// A multileader: arrowhead at `points[0]`, through the other points,
    /// then a landing and the text.
    Leader {
        points: Vec<Point>,
        content: String,
        scale: f64,
    },
    /// A hatch: boundary loops filled even-odd with a pattern.
    Hatch {
        loops: Vec<Vec<Vertex>>,
        pattern: HatchPattern,
        scale: f64,
        angle: f64,
    },
}

/// Where and how a block reference places its block: the block's base point
/// lands on `position`, scaled uniformly, rotated, and optionally mirrored
/// (a negative X scale in DXF).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub position: Point,
    pub scale: f64,
    /// Degrees, counterclockwise.
    pub rotation: f64,
    pub mirror: bool,
}

impl Placement {
    pub const fn at(position: Point) -> Self {
        Self { position, scale: 1.0, rotation: 0.0, mirror: false }
    }

    /// World position of block-local point `p`, for a block with base point `base`.
    pub fn apply(&self, base: Point, p: Point) -> Point {
        let mut d = p - base;
        if self.mirror {
            d.x = -d.x;
        }
        (self.position + d * self.scale).rotated_about(self.position, self.rotation)
    }

    /// Block-local geometry placed in the world.
    pub fn place(&self, base: Point, g: &Geometry) -> Geometry {
        g.map_points(|p| self.apply(base, p), self.scale, self.mirror)
    }
}

impl Geometry {
    pub fn arc(a: &ArcSeg) -> Geometry {
        let (start_angle, end_angle) = a.ccw_angles();
        Geometry::Arc { center: a.center, radius: a.radius, start_angle, end_angle }
    }

    pub fn from_seg(seg: &Seg) -> Geometry {
        match seg {
            Seg::Line(s) => Geometry::Line { start: s.start, end: s.end },
            Seg::Arc(a) => Geometry::arc(a),
        }
    }

    /// A polyline made of consecutive segments.
    pub fn polyline_from_segs(segs: &[Seg], closed: bool) -> Geometry {
        let mut vertices: Vec<Vertex> = segs.iter().map(|s| Vertex::new(s.start(), s.bulge())).collect();
        if !closed && let Some(last) = segs.last() {
            vertices.push(Vertex::at(last.end()));
        }
        Geometry::Polyline { vertices, closed }
    }

    /// The object type as AutoCAD names it in prompts and messages.
    pub fn name(&self) -> &'static str {
        match self {
            Geometry::Line { .. } => "Line",
            Geometry::Circle { .. } => "Circle",
            Geometry::Arc { .. } => "Arc",
            Geometry::Polyline { .. } => "Polyline",
            Geometry::Insert { .. } => "Block Reference",
            Geometry::Text { .. } => "MText",
            Geometry::Dimension { aligned: true, .. } => "Aligned Dimension",
            Geometry::Dimension { .. } => "Rotated Dimension",
            Geometry::Leader { .. } => "MultiLeader",
            Geometry::Hatch { .. } => "Hatch",
        }
    }

    /// Dimensions and leaders as the lines, arrowheads and text they draw.
    /// `None` for everything else.
    pub fn primitives(&self) -> Option<Vec<Geometry>> {
        match self {
            Geometry::Dimension { p1, p2, line, rotation, aligned, text, scale } => {
                Some(annotation::dimension_primitives(*p1, *p2, *line, *rotation, *aligned, text.as_deref(), *scale))
            }
            Geometry::Leader { points, content, scale } => Some(annotation::leader_primitives(points, content, *scale)),
            _ => None,
        }
    }

    /// Edges used for bounds, picking and crossing windows. Unlike
    /// [`Geometry::segments`] this includes text boxes and hatch boundaries,
    /// which aren't curves to snap to, trim or intersect.
    fn outline(&self) -> Vec<Seg> {
        match self {
            Geometry::Text { position, height, rotation, width, attach, content } => {
                let b = annotation::text_box(*position, *height, *rotation, *width, *attach, content);
                (0..4).map(|i| Seg::Line(Segment::new(b[i], b[(i + 1) % 4]))).collect()
            }
            Geometry::Hatch { loops, .. } => loops.iter().flat_map(|l| hatch::loop_segments(l)).collect(),
            _ => self.segments(),
        }
    }

    /// True if `p` is inside a text box or a hatched area.
    pub fn contains(&self, p: Point) -> bool {
        match self {
            Geometry::Text { position, height, rotation, width, attach, content } => {
                let b = annotation::text_box(*position, *height, *rotation, *width, *attach, content);
                hatch::contains(&[b.to_vec()], p)
            }
            Geometry::Hatch { loops, .. } => hatch::contains(&hatch::polygons(loops), p),
            _ => false,
        }
    }

    /// The curve as line and arc segments. A circle is one 360° arc.
    pub fn segments(&self) -> Vec<Seg> {
        match self {
            Geometry::Line { start, end } => vec![Seg::Line(Segment::new(*start, *end))],
            Geometry::Circle { center, radius } => vec![Seg::Arc(ArcSeg::circle(*center, *radius))],
            Geometry::Arc { center, radius, start_angle, end_angle } => {
                vec![Seg::Arc(ArcSeg::ccw(*center, *radius, *start_angle, *end_angle))]
            }
            Geometry::Polyline { vertices, closed } => {
                let n = vertices.len();
                let count = if *closed && n >= 2 { n } else { n.saturating_sub(1) };
                (0..count)
                    .map(|i| {
                        let (a, b) = (vertices[i], vertices[(i + 1) % n]);
                        Seg::from_bulge(a.point, b.point, a.bulge)
                    })
                    .collect()
            }
            // Expanded by the document (blocks) or by `primitives` (dimensions,
            // leaders); text and hatches aren't curves.
            Geometry::Insert { .. }
            | Geometry::Text { .. }
            | Geometry::Dimension { .. }
            | Geometry::Leader { .. }
            | Geometry::Hatch { .. } => Vec::new(),
        }
    }

    pub fn is_closed(&self) -> bool {
        match self {
            Geometry::Circle { .. } => true,
            Geometry::Polyline { closed, .. } => *closed,
            _ => false,
        }
    }

    pub fn bounds(&self) -> Aabb {
        let segs = self.outline();
        match (segs.first(), self) {
            (Some(first), _) => segs[1..].iter().fold(first.bounds(), |b, s| b.union(s.bounds())),
            (None, Geometry::Polyline { vertices, .. }) if !vertices.is_empty() => {
                Aabb::from_points(vertices[0].point, vertices[0].point)
            }
            (None, Geometry::Insert { placement, .. }) => Aabb::from_points(placement.position, placement.position),
            (None, Geometry::Leader { points, .. }) if !points.is_empty() => {
                points.iter().fold(Aabb::from_points(points[0], points[0]), |b, p| b.union(Aabb::from_points(*p, *p)))
            }
            (None, Geometry::Dimension { p1, p2, line, .. }) => {
                Aabb::from_points(*p1, *p2).union(Aabb::from_points(*line, *line))
            }
            _ => Aabb::from_points(Point::ORIGIN, Point::ORIGIN),
        }
    }

    /// Total length of the curve (circumference for a circle).
    pub fn length(&self) -> f64 {
        self.segments().iter().map(Seg::length).sum()
    }

    /// Enclosed area of a closed curve, `None` for open ones.
    pub fn area(&self) -> Option<f64> {
        if !self.is_closed() {
            return None;
        }
        // Shoelace over the chords, plus the circular segment each arc adds or removes.
        let signed: f64 = self
            .segments()
            .iter()
            .map(|seg| {
                let chord = seg.start().cross(seg.end()) / 2.0;
                match seg {
                    Seg::Line(_) => chord,
                    Seg::Arc(a) => {
                        let theta = a.sweep.to_radians();
                        chord + a.radius * a.radius / 2.0 * (theta - theta.sin())
                    }
                }
            })
            .sum();
        Some(signed.abs())
    }

    /// Shortest distance from `p` to the drawn curve (not a hatch's filled
    /// area; anywhere on a text counts as on it).
    pub fn distance_to(&self, p: Point) -> f64 {
        if matches!(self, Geometry::Text { .. }) && self.contains(p) {
            return 0.0;
        }
        self.outline().iter().map(|s| s.distance_to(p)).fold(f64::INFINITY, f64::min)
    }

    /// True if any part of the object lies inside the box (crossing selection).
    pub fn touches(&self, b: &Aabb) -> bool {
        self.outline().iter().any(|s| s.touches(b))
    }

    // ---- parameters (TRIM, EXTEND, FILLET) ---------------------------------
    //
    // A curve's parameter runs from 0 to its segment count: segment i covers
    // [i, i + 1]. A circle is one segment starting at angle 0.

    pub fn param_count(&self) -> f64 {
        self.segments().len() as f64
    }

    pub fn point_at(&self, s: f64) -> Point {
        let segs = self.segments();
        let i = (s.floor() as usize).min(segs.len().saturating_sub(1));
        segs[i].point_at(s - i as f64)
    }

    /// Parameter of the point on the curve closest to `p`.
    pub fn param_of(&self, p: Point) -> f64 {
        self.segments()
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let t = seg.param_of(p);
                (seg.point_at(t).distance(p), i as f64 + t)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(0.0, |(_, s)| s)
    }

    /// Parameters on `self` where it crosses `other`.
    pub fn intersection_params(&self, other: &Geometry) -> Vec<f64> {
        let theirs = other.segments();
        let mut out = Vec::new();
        for (i, seg) in self.segments().iter().enumerate() {
            for o in &theirs {
                out.extend(seg.intersections(o).into_iter().map(|(t, _)| i as f64 + t));
            }
        }
        out
    }

    /// The part of the curve from parameter `s0` to `s1`. On closed curves
    /// `s1 < s0` wraps through the start. `None` if the piece is empty.
    pub fn sub(&self, s0: f64, s1: f64) -> Option<Geometry> {
        let segs = self.segments();
        let n = segs.len() as f64;
        let mut pieces = Vec::new();
        let mut collect = |a: f64, b: f64| {
            for (i, seg) in segs.iter().enumerate() {
                let (lo, hi) = ((a - i as f64).max(0.0), (b - i as f64).min(1.0));
                if hi - lo > 1e-9 {
                    pieces.push(seg.sub(lo, hi));
                }
            }
        };
        if s1 > s0 {
            collect(s0, s1);
        } else if self.is_closed() {
            collect(s0, n);
            collect(0.0, s1);
        }
        if pieces.iter().map(Seg::length).sum::<f64>() <= EPSILON {
            return None;
        }
        Some(match self {
            Geometry::Line { .. } => Geometry::from_seg(&pieces[0]),
            Geometry::Arc { .. } => Geometry::from_seg(&pieces[0]),
            // The two pieces of a wrapped circle are one arc.
            Geometry::Circle { center, radius } => {
                let start = 360.0 * s0;
                let sweep = 360.0 * (s1 - s0).rem_euclid(1.0);
                Geometry::arc(&ArcSeg { center: *center, radius: *radius, start, sweep })
            }
            Geometry::Polyline { .. } => Geometry::polyline_from_segs(&pieces, false),
            _ => return None,
        })
    }

    // ---- transforms --------------------------------------------------------

    pub fn translated(&self, d: Point) -> Geometry {
        self.map_points(|p| p + d, 1.0, false)
    }

    pub fn rotated(&self, base: Point, angle_deg: f64) -> Geometry {
        self.map_points(|p| p.rotated_about(base, angle_deg), 1.0, false)
    }

    /// Scaled about `base`; `factor` must be positive.
    pub fn scaled(&self, base: Point, factor: f64) -> Geometry {
        self.map_points(|p| p.scaled_about(base, factor), factor, false)
    }

    /// Reflected across the line through `a` and `b`.
    pub fn mirrored(&self, a: Point, b: Point) -> Geometry {
        self.map_points(|p| p.mirrored(a, b), 1.0, true)
    }

    /// Applies a similarity transform given by where it sends points, how it
    /// scales lengths, and whether it flips orientation (mirroring).
    pub(crate) fn map_points(&self, f: impl Fn(Point) -> Point, length_factor: f64, flips: bool) -> Geometry {
        match self {
            Geometry::Line { start, end } => Geometry::Line { start: f(*start), end: f(*end) },
            Geometry::Circle { center, radius } => {
                Geometry::Circle { center: f(*center), radius: radius * length_factor }
            }
            Geometry::Arc { .. } => {
                let Some(Seg::Arc(a)) = self.segments().first().copied() else { return self.clone() };
                // Mapping three points keeps the arc exact and handles mirroring's flip.
                ArcSeg::through(f(a.start_point()), f(a.point_at(0.5)), f(a.end_point()))
                    .map_or_else(|| self.clone(), |m| Geometry::arc(&m))
            }
            Geometry::Polyline { vertices, closed } => Geometry::Polyline {
                vertices: vertices
                    .iter()
                    .map(|v| Vertex::new(f(v.point), if flips { -v.bulge } else { v.bulge }))
                    .collect(),
                closed: *closed,
            },
            Geometry::Text { position, height, rotation, width, attach, content } => Geometry::Text {
                position: f(*position),
                height: height * length_factor,
                // Mirrored text stays readable (AutoCAD's MIRRTEXT=0).
                rotation: if flips { *rotation } else { mapped_angle(&f, *position, *rotation) },
                width: width * length_factor,
                attach: *attach,
                content: content.clone(),
            },
            Geometry::Dimension { p1, p2, line, rotation, aligned, text, scale } => Geometry::Dimension {
                p1: f(*p1),
                p2: f(*p2),
                line: f(*line),
                rotation: mapped_angle(&f, *p1, *rotation),
                aligned: *aligned,
                text: text.clone(),
                scale: scale * length_factor,
            },
            Geometry::Leader { points, content, scale } => Geometry::Leader {
                points: points.iter().map(|p| f(*p)).collect(),
                content: content.clone(),
                scale: scale * length_factor,
            },
            Geometry::Hatch { loops, pattern, scale, angle } => {
                let origin = loops.first().and_then(|l| l.first()).map_or(Point::ORIGIN, |v| v.point);
                Geometry::Hatch {
                    loops: loops
                        .iter()
                        .map(|l| {
                            l.iter().map(|v| Vertex::new(f(v.point), if flips { -v.bulge } else { v.bulge })).collect()
                        })
                        .collect(),
                    pattern: *pattern,
                    scale: scale * length_factor,
                    // Mirrored patterns keep their direction (AutoCAD's MIRRHATCH=0).
                    angle: if flips { *angle } else { mapped_angle(&f, origin, *angle) },
                }
            }
            Geometry::Insert { block, placement } => {
                let position = f(placement.position);
                let axis = f(placement.position.polar(1.0, placement.rotation)) - position;
                let mut rotation = Point::ORIGIN.angle_to(axis);
                // A reflection turns the block over: flip its mirror state and
                // compensate with a half turn (checked in the tests).
                if flips {
                    rotation = (rotation + 180.0).rem_euclid(360.0);
                }
                Geometry::Insert {
                    block: block.clone(),
                    placement: Placement {
                        position,
                        scale: placement.scale * length_factor,
                        rotation,
                        mirror: placement.mirror != flips,
                    },
                }
            }
        }
    }

    /// STRETCH: moves the defining points that lie inside any of `windows` by `d`.
    /// A circle can't deform, so it moves only when its center is inside.
    pub fn stretched(&self, windows: &[Aabb], d: Point) -> Geometry {
        let inside = |p: Point| windows.iter().any(|w| w.contains(p));
        let shift = |p: Point| if inside(p) { p + d } else { p };
        match self {
            Geometry::Line { start, end } => Geometry::Line { start: shift(*start), end: shift(*end) },
            Geometry::Circle { center, radius } => Geometry::Circle { center: shift(*center), radius: *radius },
            Geometry::Arc { .. } => {
                let Some(Seg::Arc(a)) = self.segments().first().copied() else { return self.clone() };
                let (s, e) = (a.start_point(), a.end_point());
                let moved = usize::from(inside(s)) + usize::from(inside(e));
                let mid = a.point_at(0.5) + d * (moved as f64 / 2.0);
                ArcSeg::through(shift(s), mid, shift(e)).map_or_else(|| self.clone(), |m| Geometry::arc(&m))
            }
            Geometry::Polyline { vertices, closed } => Geometry::Polyline {
                vertices: vertices.iter().map(|v| Vertex::new(shift(v.point), v.bulge)).collect(),
                closed: *closed,
            },
            Geometry::Insert { placement, .. } => self.translated(shift(placement.position) - placement.position),
            Geometry::Text { position, .. } => self.translated(shift(*position) - *position),
            Geometry::Dimension { p1, p2, line, rotation, aligned, text, scale } => Geometry::Dimension {
                p1: shift(*p1),
                p2: shift(*p2),
                line: shift(*line),
                rotation: *rotation,
                aligned: *aligned,
                text: text.clone(),
                scale: *scale,
            },
            Geometry::Leader { points, content, scale } => Geometry::Leader {
                points: points.iter().map(|p| shift(*p)).collect(),
                content: content.clone(),
                scale: *scale,
            },
            Geometry::Hatch { loops, pattern, scale, angle } => Geometry::Hatch {
                loops: loops.iter().map(|l| l.iter().map(|v| Vertex::new(shift(v.point), v.bulge)).collect()).collect(),
                pattern: *pattern,
                scale: *scale,
                angle: *angle,
            },
        }
    }

    // ---- grips -------------------------------------------------------------

    /// Grip locations, as AutoCAD shows them on selected objects:
    /// line = start, midpoint, end; circle = center then the four quadrants;
    /// arc = start, midpoint, end, center; polyline = its vertices.
    pub fn grips(&self) -> Vec<Point> {
        match self {
            Geometry::Line { start, end } => vec![*start, start.midpoint(*end), *end],
            Geometry::Circle { center, radius } => {
                let mut g = vec![*center];
                g.extend(quadrants(*center, *radius));
                g
            }
            Geometry::Arc { center, radius, start_angle, end_angle } => {
                let a = ArcSeg::ccw(*center, *radius, *start_angle, *end_angle);
                vec![a.start_point(), a.point_at(0.5), a.end_point(), *center]
            }
            Geometry::Polyline { vertices, .. } => vertices.iter().map(|v| v.point).collect(),
            Geometry::Insert { placement, .. } => vec![placement.position],
            Geometry::Text { position, .. } => vec![*position],
            Geometry::Dimension { p1, p2, line, .. } => vec![*p1, *p2, *line],
            Geometry::Leader { points, .. } => points.clone(),
            Geometry::Hatch { .. } => vec![self.bounds().center()],
        }
    }

    /// The shape after dragging grip `index` to `to` (grip STRETCH).
    pub fn with_grip_moved(&self, index: usize, to: Point) -> Geometry {
        match (self, index) {
            (Geometry::Line { end, .. }, 0) => Geometry::Line { start: to, end: *end },
            (Geometry::Line { start, end }, 1) => self.translated(to - start.midpoint(*end)),
            (Geometry::Line { start, .. }, _) => Geometry::Line { start: *start, end: to },
            (Geometry::Circle { center, .. }, 0) => self.translated(to - *center),
            (Geometry::Circle { center, .. }, _) => Geometry::Circle { center: *center, radius: center.distance(to) },
            (Geometry::Arc { center, .. }, 3) => self.translated(to - *center),
            (Geometry::Arc { .. }, i) => {
                let mut pts: Vec<Point> = self.grips()[..3].to_vec();
                pts[i.min(2)] = to;
                ArcSeg::through(pts[0], pts[1], pts[2]).map_or_else(|| self.clone(), |a| Geometry::arc(&a))
            }
            (Geometry::Polyline { vertices, closed }, i) => {
                let mut vertices = vertices.clone();
                if let Some(v) = vertices.get_mut(i) {
                    v.point = to;
                }
                Geometry::Polyline { vertices, closed: *closed }
            }
            (Geometry::Insert { placement, .. }, _) => self.translated(to - placement.position),
            (Geometry::Text { position, .. }, _) => self.translated(to - *position),
            (Geometry::Dimension { p1, p2, line, rotation, aligned, text, scale }, i) => {
                let mut pts = [*p1, *p2, *line];
                pts[i.min(2)] = to;
                Geometry::Dimension {
                    p1: pts[0],
                    p2: pts[1],
                    line: pts[2],
                    rotation: *rotation,
                    aligned: *aligned,
                    text: text.clone(),
                    scale: *scale,
                }
            }
            (Geometry::Leader { points, content, scale }, i) => {
                let mut points = points.clone();
                if let Some(p) = points.get_mut(i) {
                    *p = to;
                }
                Geometry::Leader { points, content: content.clone(), scale: *scale }
            }
            (Geometry::Hatch { .. }, _) => self.translated(to - self.bounds().center()),
        }
    }
}

/// Where a transform sends a direction: the angle of `f(at + dir(angle)) - f(at)`.
fn mapped_angle(f: &impl Fn(Point) -> Point, at: Point, angle: f64) -> f64 {
    Point::ORIGIN.angle_to(f(at.polar(1.0, angle)) - f(at))
}

/// Quadrant points of a circle: 0°, 90°, 180°, 270°.
pub fn quadrants(center: Point, radius: f64) -> [Point; 4] {
    [
        Point::new(center.x + radius, center.y),
        Point::new(center.x, center.y + radius),
        Point::new(center.x - radius, center.y),
        Point::new(center.x, center.y - radius),
    ]
}

/// A drawable object in the document.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub layer: String,
    pub color: Color,
    pub geometry: Geometry,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Geometry {
        let p = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        Geometry::Polyline { vertices: p.iter().map(|&(x, y)| Vertex::at(Point::new(x, y))).collect(), closed: true }
    }

    #[test]
    fn transforms_keep_circles_round() {
        let c = Geometry::Circle { center: Point::new(2.0, 0.0), radius: 1.0 };
        assert_eq!(c.scaled(Point::ORIGIN, 3.0), Geometry::Circle { center: Point::new(6.0, 0.0), radius: 3.0 });
        let Geometry::Circle { center, radius } = c.rotated(Point::ORIGIN, 90.0) else { panic!() };
        assert!(center.approx_eq(Point::new(0.0, 2.0)) && radius == 1.0);
    }

    #[test]
    fn mirroring_an_arc_keeps_it_counterclockwise() {
        let arc = Geometry::Arc { center: Point::new(5.0, 5.0), radius: 2.0, start_angle: 0.0, end_angle: 90.0 };
        let Geometry::Arc { center, start_angle, end_angle, .. } = arc.mirrored(Point::ORIGIN, Point::new(1.0, 0.0))
        else {
            panic!()
        };
        assert!(center.approx_eq(Point::new(5.0, -5.0)));
        assert!((start_angle - 270.0).abs() < 1e-9 && end_angle.abs() < 1e-9);
    }

    #[test]
    fn placing_and_mirroring_block_references() {
        let base = Point::new(1.0, 0.0);
        let p = Placement { position: Point::new(10.0, 10.0), scale: 2.0, rotation: 90.0, mirror: false };
        // (2,0) is 1 unit right of the base: scaled to 2, rotated to point up.
        assert!(p.apply(base, Point::new(2.0, 0.0)).approx_eq(Point::new(10.0, 12.0)));

        let insert = Geometry::Insert { block: "B".into(), placement: Placement::at(Point::new(5.0, 0.0)) };
        let local = Geometry::Line { start: Point::ORIGIN, end: Point::new(3.0, 1.0) };
        // Mirroring the reference must give the same curves as mirroring its contents.
        for axis in [Point::new(0.0, 1.0), Point::new(1.0, 0.0), Point::new(1.0, 2.0)] {
            let Geometry::Insert { placement: m, .. } = insert.mirrored(Point::ORIGIN, axis) else { panic!() };
            assert!(m.mirror);
            let direct = Placement::at(Point::new(5.0, 0.0)).place(Point::ORIGIN, &local).mirrored(Point::ORIGIN, axis);
            let (Geometry::Line { start: a, end: b }, Geometry::Line { start: c, end: d }) =
                (m.place(Point::ORIGIN, &local), direct)
            else {
                panic!()
            };
            assert!(a.approx_eq(c) && b.approx_eq(d), "axis {axis:?}");
        }
    }

    #[test]
    fn stretch_moves_only_points_inside_the_window() {
        let w = [Aabb::from_points(Point::new(5.0, -1.0), Point::new(15.0, 1.0))];
        let line = Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) };
        assert_eq!(
            line.stretched(&w, Point::new(0.0, 5.0)),
            Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 5.0) }
        );
        let circle = Geometry::Circle { center: Point::new(4.0, 0.0), radius: 2.0 };
        assert_eq!(circle.stretched(&w, Point::new(0.0, 5.0)), circle, "center outside: stays put");
    }

    #[test]
    fn polyline_segments_params_and_bounds() {
        let sq = square();
        assert_eq!(sq.segments().len(), 4);
        assert_eq!(sq.param_count(), 4.0);
        assert!(sq.point_at(1.5).approx_eq(Point::new(10.0, 5.0)));
        assert!((sq.param_of(Point::new(5.0, 11.0)) - 2.5).abs() < 1e-9);
        assert!((sq.distance_to(Point::new(5.0, 5.0)) - 5.0).abs() < 1e-9);
        assert_eq!(sq.bounds(), Aabb::from_points(Point::ORIGIN, Point::new(10.0, 10.0)));
    }

    #[test]
    fn sub_wraps_on_closed_curves() {
        let circle = Geometry::Circle { center: Point::ORIGIN, radius: 1.0 };
        let Some(Geometry::Arc { start_angle, end_angle, .. }) = circle.sub(0.75, 0.25) else { panic!() };
        assert!((start_angle - 270.0).abs() < 1e-9 && (end_angle - 90.0).abs() < 1e-9);

        let Some(Geometry::Polyline { vertices, closed }) = square().sub(3.5, 1.5) else { panic!() };
        assert!(!closed);
        let pts: Vec<Point> = vertices.iter().map(|v| v.point).collect();
        assert_eq!(pts, vec![Point::new(0.0, 5.0), Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 5.0)]);
    }

    #[test]
    fn lengths_and_areas() {
        assert!((square().length() - 40.0).abs() < 1e-9);
        assert!((square().area().unwrap() - 100.0).abs() < 1e-9);
        let c = Geometry::Circle { center: Point::new(3.0, 3.0), radius: 2.0 };
        assert!((c.area().unwrap() - std::f64::consts::PI * 4.0).abs() < 1e-9);
        // A 2x2 square with its top edge bulged into a half circle.
        let d = Geometry::Polyline {
            vertices: vec![
                Vertex::at(Point::new(0.0, 0.0)),
                Vertex::new(Point::new(2.0, 0.0), 0.0),
                Vertex::new(Point::new(2.0, 2.0), 1.0),
                Vertex::at(Point::new(0.0, 2.0)),
            ],
            closed: true,
        };
        assert!((d.area().unwrap() - (4.0 + std::f64::consts::PI / 2.0)).abs() < 1e-9);
        assert!(Geometry::Line { start: Point::ORIGIN, end: Point::new(3.0, 4.0) }.area().is_none());
    }

    #[test]
    fn polyline_grips_are_vertices() {
        let moved = square().with_grip_moved(2, Point::new(12.0, 12.0));
        assert_eq!(moved.grips()[2], Point::new(12.0, 12.0));
    }
}

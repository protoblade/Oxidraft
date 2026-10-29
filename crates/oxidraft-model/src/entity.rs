use oxidraft_geom::{
    Aabb, EPSILON, Point, Segment,
    curve::{ArcSeg, Seg},
    ellipse::Ellipse,
    spline::Spline,
};

use crate::{
    annotation::{self, Attach},
    hatch::{self, HatchPattern},
    linetype::Lineweight,
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
    /// Inside a block: use the color of the block reference.
    ByBlock,
    /// AutoCAD Color Index, 1..=255 (1 red, 2 yellow, 3 green, 4 cyan, 5 blue, 6 magenta, 7 white/black).
    Index(u8),
}

/// A polyline vertex. `bulge` describes the segment that starts here:
/// 0 is straight, otherwise the tangent of a quarter of the arc's sweep
/// (negative = clockwise), as in DXF. So do the widths: the segment is
/// `start_width` wide here and tapers to `end_width` at the next vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub point: Point,
    pub bulge: f64,
    pub start_width: f64,
    pub end_width: f64,
}

impl Vertex {
    pub const fn new(point: Point, bulge: f64) -> Self {
        Self { point, bulge, start_width: 0.0, end_width: 0.0 }
    }

    pub const fn at(point: Point) -> Self {
        Self::new(point, 0.0)
    }

    pub const fn with_widths(self, start_width: f64, end_width: f64) -> Self {
        Self { start_width, end_width, ..self }
    }
}

/// How far XLINE and RAY curves reach in [`Geometry::segments`]: far
/// enough to cross anything in a drawing.
pub const FAR: f64 = 1.0e8;

/// The widths of a polyline whose every segment is `w` wide.
pub fn constant_width(vertices: &[Vertex]) -> Option<f64> {
    let w = vertices.first()?.start_width;
    vertices.iter().all(|v| v.start_width == w && v.end_width == w).then_some(w)
}

/// A viewport's frame: lower left, lower right, upper right, upper left.
pub fn viewport_corners(center: Point, width: f64, height: f64) -> [Point; 4] {
    let (w, h) = (width / 2.0, height / 2.0);
    [
        Point::new(center.x - w, center.y - h),
        Point::new(center.x + w, center.y - h),
        Point::new(center.x + w, center.y + h),
        Point::new(center.x - w, center.y + h),
    ]
}

/// `direction` scaled to length 1 (or +X if it has none).
fn unit(direction: Point) -> Point {
    let len = direction.length();
    if len > EPSILON { direction * (1.0 / len) } else { Point::new(1.0, 0.0) }
}

/// The shape of an entity.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A point (DXF POINT), drawn in the style PDMODE and PDSIZE set.
    Point {
        position: Point,
    },
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
    /// A 2D polyline of straight and arc segments (DXF LWPOLYLINE), wide
    /// or tapered where its vertices give widths.
    Polyline {
        vertices: Vec<Vertex>,
        closed: bool,
    },
    /// A block reference (DXF INSERT): the named block's contents placed in
    /// the drawing. Its curves come from [`crate::Document::expanded`].
    Insert {
        block: String,
        placement: Placement,
        /// Attribute values (DXF ATTRIB), in world coordinates.
        attributes: Vec<Attribute>,
        /// XCLIP's boundary, if it's clipped.
        clip: Option<Box<XClip>>,
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
        /// Its text style, width factor and oblique angle.
        look: crate::annotation::TextLook,
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
    /// A radius (DIMRADIUS) or diameter (DIMDIAMETER) dimension of the
    /// circle around `center` through `point`, with its text at `text_at`.
    RadialDimension {
        center: Point,
        point: Point,
        text_at: Point,
        diameter: bool,
        text: Option<String>,
        scale: f64,
    },
    /// An angular dimension (DIMANGULAR): the angle at `vertex`,
    /// counterclockwise from the ray through `p1` to the ray through `p2`,
    /// drawn as an arc through `arc`.
    AngularDimension {
        vertex: Point,
        p1: Point,
        p2: Point,
        arc: Point,
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
    /// An ellipse or elliptical arc (DXF ELLIPSE).
    Ellipse(Ellipse),
    /// A NURBS curve (DXF SPLINE).
    Spline(Spline),
    /// A construction line (DXF XLINE): infinite both ways through `base`.
    Xline {
        base: Point,
        direction: Point,
    },
    /// A ray (DXF RAY): from `base` to infinity along `direction`.
    Ray {
        base: Point,
        direction: Point,
    },
    /// An image or PDF page underlay (DXF IMAGE, PDFUNDERLAY): the picture
    /// [`crate::ImageDef`] `image` names, over the parallelogram from
    /// `origin` (its lower-left corner) along `u` (its bottom edge) and `v`
    /// (its left edge); shown only inside `clip` when that has points.
    Image {
        image: String,
        origin: Point,
        u: Point,
        v: Point,
        clip: Vec<Point>,
    },
    /// A layout viewport (DXF VIEWPORT): a `width` x `height` window in
    /// paper space centered on `center`, showing model space around
    /// `view_center`, `view_height` model units tall, turned by `twist`
    /// degrees. `frozen` names the layers it doesn't show.
    Viewport {
        center: Point,
        width: f64,
        height: f64,
        view_center: Point,
        view_height: f64,
        twist: f64,
        frozen: Vec<String>,
        locked: bool,
        on: bool,
        /// A non-rectangular viewport's boundary on the sheet (MVIEW
        /// Polygonal or Object, VPCLIP), inside its rectangle; empty for a
        /// rectangular one.
        clip: Vec<Point>,
    },
    /// An ordinate dimension (DIMORDINATE): `feature`'s X (`x_datum`) or Y
    /// by a leader ending at `leader_end`.
    OrdinateDimension {
        feature: Point,
        leader_end: Point,
        x_datum: bool,
        text: Option<String>,
        scale: f64,
    },
    /// An arc length dimension (DIMARC) of the arc of `radius` about
    /// `center` from `start_angle` to `end_angle`, its dimension arc at
    /// `dim_radius`.
    ArcLengthDimension {
        center: Point,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        dim_radius: f64,
        text: Option<String>,
        scale: f64,
    },
    /// A jogged radius dimension (DIMJOGGED): the radius from `center` to
    /// `point`, drawn from `center_override` with a zigzag at `jog`.
    JoggedDimension {
        center: Point,
        point: Point,
        center_override: Point,
        jog: Point,
        text: Option<String>,
        scale: f64,
    },
    /// An attribute definition (ATTDEF). In a block, what its references
    /// ask for (`prompt`, offering `default`) and show as attribute `tag`;
    /// a constant one shows `default` in every reference. Outside a block,
    /// it shows its tag.
    AttributeDefinition {
        tag: String,
        prompt: String,
        default: String,
        position: Point,
        height: f64,
        rotation: f64,
        attach: Attach,
        invisible: bool,
        constant: bool,
    },
    /// A wipeout (WIPEOUT): the closed polygon through `points`, filled
    /// with the background, hiding what's drawn before it.
    Wipeout {
        points: Vec<Point>,
    },
    /// A table (TABLE): rows of cells hanging from its upper-left corner
    /// `position`, turned `rotation` degrees. `widths` are its columns',
    /// `heights` its rows', `cells` each row's texts (one per column) at
    /// `text_height`. The first row is its title, across every column.
    Table {
        position: Point,
        rotation: f64,
        widths: Vec<f64>,
        heights: Vec<f64>,
        cells: Vec<Vec<String>>,
        text_height: f64,
    },
}

/// An attribute of a block reference: a tagged text value (parcel number,
/// elevation, …). Its text is in world coordinates and moves with the
/// reference.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub tag: String,
    pub value: String,
    /// Attributes have their own layer and color; ByBlock means the
    /// reference's color.
    pub layer: String,
    pub color: Color,
    pub position: Point,
    pub height: f64,
    pub rotation: f64,
    pub attach: Attach,
    pub invisible: bool,
}

impl Attribute {
    pub fn text(&self) -> Geometry {
        Geometry::Text {
            look: Default::default(),
            position: self.position,
            height: self.height,
            rotation: self.rotation,
            width: 0.0,
            attach: self.attach,
            content: self.value.clone(),
        }
    }

    /// The attribute after the same transform as its reference.
    fn mapped(&self, f: &dyn Fn(Point) -> Point, length_factor: f64, flips: bool) -> Attribute {
        match self.text().map_points(f, length_factor, flips) {
            Geometry::Text { position, height, rotation, .. } => {
                Attribute { position, height, rotation, ..self.clone() }
            }
            _ => self.clone(),
        }
    }
}

/// A block reference's clipping boundary (XCLIP), in the block's own
/// coordinates so it moves with the reference. Only what's inside shows,
/// or only what's outside when `inverted`; `on` false shows everything.
#[derive(Debug, Clone, PartialEq)]
pub struct XClip {
    pub boundary: Vec<Point>,
    pub inverted: bool,
    pub on: bool,
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

    /// Where world point `p` lies in the block's own coordinates.
    pub fn local(&self, base: Point, p: Point) -> Point {
        let mut d = (p.rotated_about(self.position, -self.rotation) - self.position) * (1.0 / self.scale);
        if self.mirror {
            d.x = -d.x;
        }
        base + d
    }

    /// Block-local geometry placed in the world.
    pub fn place(&self, base: Point, g: &Geometry) -> Geometry {
        g.map_points(&|p| self.apply(base, p), self.scale, self.mirror)
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
            Geometry::Point { .. } => "Point",
            Geometry::Line { .. } => "Line",
            Geometry::Circle { .. } => "Circle",
            Geometry::Arc { .. } => "Arc",
            Geometry::Polyline { .. } => "Polyline",
            Geometry::Insert { .. } => "Block Reference",
            Geometry::Text { .. } => "MText",
            Geometry::Dimension { aligned: true, .. } => "Aligned Dimension",
            Geometry::Dimension { .. } => "Rotated Dimension",
            Geometry::RadialDimension { diameter: true, .. } => "Diametric Dimension",
            Geometry::RadialDimension { .. } => "Radial Dimension",
            Geometry::AngularDimension { .. } => "Angular Dimension",
            Geometry::Leader { .. } => "MultiLeader",
            Geometry::Hatch { .. } => "Hatch",
            Geometry::Ellipse(_) => "Ellipse",
            Geometry::Spline(_) => "Spline",
            Geometry::Xline { .. } => "Xline",
            Geometry::Ray { .. } => "Ray",
            Geometry::Image { .. } => "Raster Image",
            Geometry::Viewport { .. } => "Viewport",
            Geometry::Table { .. } => "Table",
            Geometry::Wipeout { .. } => "Wipeout",
            Geometry::OrdinateDimension { .. } => "Ordinate Dimension",
            Geometry::ArcLengthDimension { .. } => "Arc Dimension",
            Geometry::JoggedDimension { .. } => "Radial Dimension Large",
            Geometry::AttributeDefinition { .. } => "Attribute Definition",
        }
    }

    /// Dimensions and leaders as the lines, arrowheads and text they draw.
    /// `None` for everything else.
    pub fn primitives(&self) -> Option<Vec<Geometry>> {
        match self {
            Geometry::Dimension { p1, p2, line, rotation, aligned, text, scale } => {
                Some(annotation::dimension_primitives(*p1, *p2, *line, *rotation, *aligned, text.as_deref(), *scale))
            }
            Geometry::RadialDimension { center, point, text_at, diameter, text, scale } => {
                Some(annotation::radial_primitives(*center, *point, *text_at, *diameter, text.as_deref(), *scale))
            }
            Geometry::AngularDimension { vertex, p1, p2, arc, text, scale } => {
                Some(annotation::angular_primitives(*vertex, *p1, *p2, *arc, text.as_deref(), *scale))
            }
            Geometry::Leader { points, content, scale } => Some(annotation::leader_primitives(points, content, *scale)),
            Geometry::Table { .. } => Some(crate::table::primitives(self)),
            Geometry::OrdinateDimension { feature, leader_end, x_datum, text, scale } => {
                Some(annotation::ordinate_primitives(*feature, *leader_end, *x_datum, text.as_deref(), *scale))
            }
            Geometry::ArcLengthDimension { center, radius, start_angle, end_angle, dim_radius, text, scale } => {
                Some(annotation::arc_length_primitives(
                    *center,
                    *radius,
                    *start_angle,
                    *end_angle,
                    *dim_radius,
                    text.as_deref(),
                    *scale,
                ))
            }
            Geometry::JoggedDimension { center, point, center_override, jog, text, scale } => {
                Some(annotation::jogged_primitives(*center, *point, *center_override, *jog, text.as_deref(), *scale))
            }
            Geometry::AttributeDefinition { tag, position, height, rotation, attach, .. } => {
                Some(vec![Geometry::Text {
                    look: Default::default(),
                    position: *position,
                    height: *height,
                    rotation: *rotation,
                    width: 0.0,
                    attach: *attach,
                    content: tag.clone(),
                }])
            }
            _ => None,
        }
    }

    /// Edges used for bounds, picking and crossing windows. Unlike
    /// [`Geometry::segments`] this includes text boxes and hatch boundaries,
    /// which aren't curves to snap to, trim or intersect.
    fn outline(&self) -> Vec<Seg> {
        match self {
            Geometry::Text { position, height, rotation, width, attach, content, look } => {
                let b =
                    annotation::text_box(*position, *height, *rotation, *width, *attach, content, look.width_factor);
                (0..4).map(|i| Seg::Line(Segment::new(b[i], b[(i + 1) % 4]))).collect()
            }
            Geometry::Hatch { loops, .. } => loops.iter().flat_map(|l| hatch::loop_segments(l)).collect(),
            Geometry::Point { position } => vec![Seg::Line(Segment::new(*position, *position))],
            // Its frame, or its clipping boundary.
            Geometry::Image { origin, u, v, clip, .. } => {
                let corners = crate::underlay::image_corners(*origin, *u, *v).to_vec();
                let ring = if clip.len() >= 3 { clip } else { &corners };
                (0..ring.len()).map(|i| Seg::Line(Segment::new(ring[i], ring[(i + 1) % ring.len()]))).collect()
            }
            _ => self.segments(),
        }
    }

    /// The outline as runs of points, arcs cut into chords of 10° at
    /// most: what a lasso is tested against.
    pub fn outline_runs(&self) -> Vec<Vec<Point>> {
        self.outline()
            .iter()
            .map(|s| match s {
                Seg::Line(l) => vec![l.start, l.end],
                Seg::Arc(a) => {
                    let n = ((a.sweep.abs() / 10.0).ceil() as usize).max(2);
                    (0..=n).map(|i| s.point_at(i as f64 / n as f64)).collect()
                }
            })
            .collect()
    }

    /// True if `p` is inside a text box or a hatched area.
    pub fn contains(&self, p: Point) -> bool {
        match self {
            Geometry::Text { position, height, rotation, width, attach, content, look } => {
                let b =
                    annotation::text_box(*position, *height, *rotation, *width, *attach, content, look.width_factor);
                hatch::contains(&[b.to_vec()], p)
            }
            Geometry::Hatch { loops, .. } => hatch::contains(&hatch::polygons(loops), p),
            Geometry::Wipeout { points } => hatch::contains(std::slice::from_ref(points), p),
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
            Geometry::Ellipse(e) => e.segs(),
            Geometry::Spline(s) => s.segs().to_vec(),
            Geometry::Xline { base, direction } => {
                let d = unit(*direction) * FAR;
                vec![Seg::Line(Segment::new(*base - d, *base + d))]
            }
            Geometry::Ray { base, direction } => vec![Seg::Line(Segment::new(*base, *base + unit(*direction) * FAR))],
            // Expanded by the document (blocks) or by `primitives` (dimensions,
            // leaders); points, text and hatches aren't curves.
            Geometry::Point { .. }
            | Geometry::Image { .. }
            | Geometry::AttributeDefinition { .. }
            | Geometry::OrdinateDimension { .. }
            | Geometry::ArcLengthDimension { .. }
            | Geometry::JoggedDimension { .. }
            | Geometry::Insert { .. }
            | Geometry::Text { .. }
            | Geometry::Dimension { .. }
            | Geometry::RadialDimension { .. }
            | Geometry::AngularDimension { .. }
            | Geometry::Leader { .. }
            | Geometry::Hatch { .. } => Vec::new(),
            // Its border, which snaps and picks like a rectangle.
            Geometry::Table { position, rotation, widths, heights, .. } => {
                let grid = crate::table::Grid { position: *position, rotation: *rotation, widths, heights };
                grid.lines().into_iter().map(Seg::Line).collect()
            }
            Geometry::Wipeout { points } if points.len() >= 2 => {
                (0..points.len()).map(|i| Seg::Line(Segment::new(points[i], points[(i + 1) % points.len()]))).collect()
            }
            Geometry::Wipeout { .. } => Vec::new(),
            // Its boundary, if it isn't rectangular.
            Geometry::Viewport { clip, .. } if clip.len() >= 3 => {
                (0..clip.len()).map(|i| Seg::Line(Segment::new(clip[i], clip[(i + 1) % clip.len()]))).collect()
            }
            Geometry::Viewport { center, width, height, .. } => {
                let c = viewport_corners(*center, *width, *height);
                (0..4).map(|i| Seg::Line(Segment::new(c[i], c[(i + 1) % 4]))).collect()
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        match self {
            Geometry::Circle { .. } | Geometry::Wipeout { .. } => true,
            Geometry::Polyline { closed, .. } => *closed,
            Geometry::Ellipse(e) => e.is_full(),
            Geometry::Spline(s) => s.is_closed(),
            _ => false,
        }
    }

    /// True for construction lines and rays, which go on forever.
    pub fn is_infinite(&self) -> bool {
        matches!(self, Geometry::Xline { .. } | Geometry::Ray { .. })
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
            (None, Geometry::RadialDimension { point, text_at, .. }) => Aabb::from_points(*point, *text_at),
            (None, Geometry::AngularDimension { p1, p2, arc, .. }) => {
                Aabb::from_points(*p1, *p2).union(Aabb::from_points(*arc, *arc))
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
        if let Geometry::Ellipse(e) = self {
            return Some(e.area());
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
        // Anywhere on a wide polyline's band counts as on it.
        if let Geometry::Polyline { vertices, .. } = self
            && vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0)
        {
            return self
                .segments()
                .iter()
                .zip(vertices)
                .map(|(s, v)| (s.distance_to(p) - v.start_width.max(v.end_width) / 2.0).max(0.0))
                .fold(f64::INFINITY, f64::min);
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
        match self {
            Geometry::Ellipse(e) => return ellipse_sub(e, s0, s1),
            Geometry::Spline(s) => return spline_sub(s, s0, s1),
            Geometry::Xline { base, direction } | Geometry::Ray { base, direction } => {
                return infinite_sub(self, *base, *direction, s0, s1);
            }
            _ => {}
        }
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
            // A constant width stays; tapers are lost.
            Geometry::Polyline { vertices, .. } => {
                let piece = Geometry::polyline_from_segs(&pieces, false);
                match constant_width(vertices) {
                    Some(w) if w > 0.0 => piece.with_width(w),
                    _ => piece,
                }
            }
            _ => return None,
        })
    }

    /// A polyline with every segment `w` wide; anything else unchanged.
    pub fn with_width(self, w: f64) -> Geometry {
        match self {
            Geometry::Polyline { vertices, closed } => {
                Geometry::Polyline { vertices: vertices.into_iter().map(|v| v.with_widths(w, w)).collect(), closed }
            }
            g => g,
        }
    }

    // ---- transforms --------------------------------------------------------

    pub fn translated(&self, d: Point) -> Geometry {
        self.map_points(&|p| p + d, 1.0, false)
    }

    pub fn rotated(&self, base: Point, angle_deg: f64) -> Geometry {
        self.map_points(&|p| p.rotated_about(base, angle_deg), 1.0, false)
    }

    /// Scaled about `base`; `factor` must be positive.
    pub fn scaled(&self, base: Point, factor: f64) -> Geometry {
        self.map_points(&|p| p.scaled_about(base, factor), factor, false)
    }

    /// Reflected across the line through `a` and `b`.
    pub fn mirrored(&self, a: Point, b: Point) -> Geometry {
        self.map_points(&|p| p.mirrored(a, b), 1.0, true)
    }

    /// Applies a similarity transform given by where it sends points, how it
    /// scales lengths, and whether it flips orientation (mirroring).
    pub(crate) fn map_points(&self, f: &dyn Fn(Point) -> Point, length_factor: f64, flips: bool) -> Geometry {
        match self {
            Geometry::Point { position } => Geometry::Point { position: f(*position) },
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
                    .map(|v| Vertex {
                        point: f(v.point),
                        bulge: if flips { -v.bulge } else { v.bulge },
                        start_width: v.start_width * length_factor,
                        end_width: v.end_width * length_factor,
                    })
                    .collect(),
                closed: *closed,
            },
            Geometry::Ellipse(e) => Geometry::Ellipse(e.mapped(f, flips)),
            Geometry::Spline(s) => Geometry::Spline(s.mapped(f)),
            Geometry::Xline { base, direction } => {
                Geometry::Xline { base: f(*base), direction: f(*base + *direction) - f(*base) }
            }
            Geometry::Image { image, origin, u, v, clip } => Geometry::Image {
                image: image.clone(),
                origin: f(*origin),
                u: f(*origin + *u) - f(*origin),
                v: f(*origin + *v) - f(*origin),
                clip: clip.iter().map(|p| f(*p)).collect(),
            },
            Geometry::Wipeout { points } => Geometry::Wipeout { points: points.iter().map(|p| f(*p)).collect() },
            Geometry::OrdinateDimension { feature, leader_end, x_datum, text, scale } => Geometry::OrdinateDimension {
                feature: f(*feature),
                leader_end: f(*leader_end),
                x_datum: *x_datum,
                text: text.clone(),
                scale: *scale,
            },
            Geometry::ArcLengthDimension { center, radius, start_angle, end_angle, dim_radius, text, scale } => {
                // The arc's ends where they're sent: its angles follow.
                let (a, b) = (f(center.polar(*radius, *start_angle)), f(center.polar(*radius, *end_angle)));
                let c = f(*center);
                let (a, b) = if flips { (b, a) } else { (a, b) };
                Geometry::ArcLengthDimension {
                    center: c,
                    radius: radius * length_factor,
                    start_angle: c.angle_to(a),
                    end_angle: c.angle_to(b),
                    dim_radius: dim_radius * length_factor,
                    text: text.clone(),
                    scale: *scale,
                }
            }
            Geometry::JoggedDimension { center, point, center_override, jog, text, scale } => {
                Geometry::JoggedDimension {
                    center: f(*center),
                    point: f(*point),
                    center_override: f(*center_override),
                    jog: f(*jog),
                    text: text.clone(),
                    scale: *scale,
                }
            }
            Geometry::AttributeDefinition {
                tag,
                prompt,
                default,
                position,
                height,
                rotation,
                attach,
                invisible,
                constant,
            } => {
                let turned = f(*position + Point::ORIGIN.polar(1.0, *rotation)) - f(*position);
                Geometry::AttributeDefinition {
                    tag: tag.clone(),
                    prompt: prompt.clone(),
                    default: default.clone(),
                    position: f(*position),
                    height: height * length_factor,
                    rotation: Point::ORIGIN.angle_to(turned),
                    attach: *attach,
                    invisible: *invisible,
                    constant: *constant,
                }
            }
            // It turns and scales, but stays upright text in rows: mirrored,
            // what was its upper-right corner becomes its upper-left.
            Geometry::Table { position, rotation, widths, heights, cells, text_height } => {
                let grid = crate::table::Grid { position: *position, rotation: *rotation, widths, heights };
                let [upper_left, upper_right, ..] = grid.corners();
                let (from, to) = if flips { (f(upper_right), f(upper_left)) } else { (f(upper_left), f(upper_right)) };
                Geometry::Table {
                    position: from,
                    rotation: from.angle_to(to),
                    widths: widths.iter().map(|w| w * length_factor).collect(),
                    heights: heights.iter().map(|h| h * length_factor).collect(),
                    cells: cells.clone(),
                    text_height: text_height * length_factor,
                }
            }
            // Its frame moves and scales; the view through it stays.
            Geometry::Viewport { center, width, height, view_center, view_height, twist, frozen, locked, on, clip } => {
                Geometry::Viewport {
                    center: f(*center),
                    width: width * length_factor,
                    height: height * length_factor,
                    view_center: *view_center,
                    view_height: *view_height,
                    twist: *twist,
                    frozen: frozen.clone(),
                    locked: *locked,
                    on: *on,
                    clip: clip.iter().map(|p| f(*p)).collect(),
                }
            }
            Geometry::Ray { base, direction } => {
                Geometry::Ray { base: f(*base), direction: f(*base + *direction) - f(*base) }
            }
            Geometry::Text { position, height, rotation, width, attach, content, look } => Geometry::Text {
                look: look.clone(),
                position: f(*position),
                height: height * length_factor,
                // Mirrored text stays readable (AutoCAD's MIRRTEXT=0).
                rotation: if flips { *rotation } else { mapped_angle(f, *position, *rotation) },
                width: width * length_factor,
                attach: *attach,
                content: content.clone(),
            },
            Geometry::Dimension { p1, p2, line, rotation, aligned, text, scale } => Geometry::Dimension {
                p1: f(*p1),
                p2: f(*p2),
                line: f(*line),
                rotation: mapped_angle(f, *p1, *rotation),
                aligned: *aligned,
                text: text.clone(),
                scale: scale * length_factor,
            },
            Geometry::RadialDimension { center, point, text_at, diameter, text, scale } => Geometry::RadialDimension {
                center: f(*center),
                point: f(*point),
                text_at: f(*text_at),
                diameter: *diameter,
                text: text.clone(),
                scale: scale * length_factor,
            },
            // A reflection turns counterclockwise into clockwise: swap the rays.
            Geometry::AngularDimension { vertex, p1, p2, arc, text, scale } => {
                let (p1, p2) = if flips { (p2, p1) } else { (p1, p2) };
                Geometry::AngularDimension {
                    vertex: f(*vertex),
                    p1: f(*p1),
                    p2: f(*p2),
                    arc: f(*arc),
                    text: text.clone(),
                    scale: scale * length_factor,
                }
            }
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
                    angle: if flips { *angle } else { mapped_angle(f, origin, *angle) },
                }
            }
            Geometry::Insert { block, placement, attributes, clip } => {
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
                    attributes: attributes.iter().map(|a| a.mapped(f, length_factor, flips)).collect(),
                    // The boundary is in block coordinates: it moves with the reference.
                    clip: clip.clone(),
                }
            }
        }
    }

    /// STRETCH: moves the defining points that lie inside any of `windows` by `d`.
    /// A circle can't deform, so it moves only when its center is inside.
    pub fn stretched(&self, windows: &[Aabb], d: Point) -> Geometry {
        self.stretched_where(&|p| windows.iter().any(|w| w.contains(p)), d)
    }

    /// Like [`Geometry::stretched`], moving the points `inside` says are
    /// in the selection's windows or lassos.
    pub fn stretched_where(&self, inside: &dyn Fn(Point) -> bool, d: Point) -> Geometry {
        let shift = |p: Point| if inside(p) { p + d } else { p };
        match self {
            Geometry::Point { position } => Geometry::Point { position: shift(*position) },
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
                vertices: vertices.iter().map(|v| Vertex { point: shift(v.point), ..*v }).collect(),
                closed: *closed,
            },
            // Ellipses keep their shape, like circles; splines move the
            // points their grips edit.
            Geometry::Ellipse(e) => self.translated(shift(e.center) - e.center),
            Geometry::Spline(s) => {
                let moved: Vec<Point> = s.grip_points().into_iter().map(shift).collect();
                s.with_grip_points(&moved).map_or_else(|| self.clone(), Geometry::Spline)
            }
            Geometry::Xline { base, .. } | Geometry::Ray { base, .. } => self.translated(shift(*base) - *base),
            Geometry::Image { origin, .. } => self.translated(shift(*origin) - *origin),
            Geometry::Table { position, .. } => self.translated(shift(*position) - *position),
            Geometry::Wipeout { points } => Geometry::Wipeout { points: points.iter().map(|p| shift(*p)).collect() },
            Geometry::AttributeDefinition { position, .. } => self.translated(shift(*position) - *position),
            Geometry::OrdinateDimension { feature, leader_end, x_datum, text, scale } => Geometry::OrdinateDimension {
                feature: shift(*feature),
                leader_end: shift(*leader_end),
                x_datum: *x_datum,
                text: text.clone(),
                scale: *scale,
            },
            Geometry::ArcLengthDimension { center, .. } => self.translated(shift(*center) - *center),
            Geometry::JoggedDimension { center, .. } => self.translated(shift(*center) - *center),
            Geometry::Viewport { center, .. } => self.translated(shift(*center) - *center),
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
            Geometry::RadialDimension { center, point, text_at, diameter, text, scale } => Geometry::RadialDimension {
                center: shift(*center),
                point: shift(*point),
                text_at: shift(*text_at),
                diameter: *diameter,
                text: text.clone(),
                scale: *scale,
            },
            Geometry::AngularDimension { vertex, p1, p2, arc, text, scale } => Geometry::AngularDimension {
                vertex: shift(*vertex),
                p1: shift(*p1),
                p2: shift(*p2),
                arc: shift(*arc),
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
            Geometry::Point { position } => vec![*position],
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
            Geometry::RadialDimension { center, point, text_at, .. } => vec![*center, *point, *text_at],
            Geometry::AngularDimension { vertex, p1, p2, arc, .. } => vec![*vertex, *p1, *p2, *arc],
            Geometry::Leader { points, .. } => points.clone(),
            Geometry::Hatch { .. } => vec![self.bounds().center()],
            // Center, the ends of both axes, and an arc's ends.
            Geometry::Ellipse(e) => {
                let (m, n) = (e.major, e.minor());
                let mut g = vec![e.center, e.center + m, e.center + n, e.center - m, e.center - n];
                if !e.is_full() {
                    g.extend([e.start_point(), e.end_point()]);
                }
                g
            }
            Geometry::Spline(s) => s.grip_points(),
            Geometry::Image { origin, u, v, .. } => crate::underlay::image_corners(*origin, *u, *v).to_vec(),
            Geometry::Xline { base, direction } | Geometry::Ray { base, direction } => vec![*base, *base + *direction],
            Geometry::Table { position, .. } => vec![*position],
            Geometry::Wipeout { points } => points.clone(),
            Geometry::AttributeDefinition { position, .. } => vec![*position],
            Geometry::OrdinateDimension { feature, leader_end, .. } => vec![*feature, *leader_end],
            Geometry::ArcLengthDimension { center, dim_radius, start_angle, end_angle, .. } => {
                let mid = start_angle + (end_angle - start_angle).rem_euclid(360.0) / 2.0;
                vec![*center, center.polar(*dim_radius, mid)]
            }
            Geometry::JoggedDimension { point, center_override, jog, .. } => vec![*center_override, *jog, *point],
            Geometry::Viewport { center, clip, .. } if clip.len() >= 3 => {
                std::iter::once(*center).chain(clip.iter().copied()).collect()
            }
            Geometry::Viewport { center, width, height, .. } => {
                let mut g = vec![*center];
                g.extend(viewport_corners(*center, *width, *height));
                g
            }
        }
    }

    /// A viewport with a new frame, showing model space at the same scale
    /// and in the same place on the sheet: more or less of it shows.
    pub fn reframed(&self, center: Point, width: f64, height: f64) -> Geometry {
        let mut g = self.clone();
        if let Geometry::Viewport { center: c, width: w, height: h, view_center, view_height, .. } = &mut g {
            // Model units per paper unit.
            let n = if *h > 0.0 { *view_height / *h } else { 1.0 };
            *view_center = *view_center + (center - *c) * n;
            *view_height = height * n;
            (*c, *w, *h) = (center, width, height);
        }
        g
    }

    /// A viewport with `clip` as its boundary (none: rectangular again),
    /// its rectangle round it, at the same scale.
    pub fn with_clip(&self, clip: Vec<Point>) -> Geometry {
        let fitted = match clip.first() {
            Some(&first) if clip.len() >= 3 => {
                let b = clip.iter().fold(Aabb::from_points(first, first), |b, p| b.union(Aabb::from_points(*p, *p)));
                self.reframed(b.center(), b.width(), b.height())
            }
            _ => self.clone(),
        };
        let mut g = fitted;
        if let Geometry::Viewport { clip: c, .. } = &mut g {
            *c = if clip.len() >= 3 { clip } else { Vec::new() };
        }
        g
    }

    /// The shape after dragging grip `index` to `to` (grip STRETCH).
    pub fn with_grip_moved(&self, index: usize, to: Point) -> Geometry {
        match (self, index) {
            (Geometry::Point { .. }, _) => Geometry::Point { position: to },
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
            (Geometry::RadialDimension { center, point, text_at, diameter, text, scale }, i) => {
                let mut pts = [*center, *point, *text_at];
                pts[i.min(2)] = to;
                Geometry::RadialDimension {
                    center: pts[0],
                    point: pts[1],
                    text_at: pts[2],
                    diameter: *diameter,
                    text: text.clone(),
                    scale: *scale,
                }
            }
            (Geometry::AngularDimension { vertex, p1, p2, arc, text, scale }, i) => {
                let mut pts = [*vertex, *p1, *p2, *arc];
                pts[i.min(3)] = to;
                Geometry::AngularDimension {
                    vertex: pts[0],
                    p1: pts[1],
                    p2: pts[2],
                    arc: pts[3],
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
            (Geometry::Table { position, .. }, _) => self.translated(to - *position),
            (Geometry::AttributeDefinition { position, .. }, _) => self.translated(to - *position),
            (Geometry::OrdinateDimension { feature, .. }, 0) => self.translated(to - *feature),
            (Geometry::OrdinateDimension { feature, x_datum, text, scale, .. }, _) => Geometry::OrdinateDimension {
                feature: *feature,
                leader_end: to,
                x_datum: *x_datum,
                text: text.clone(),
                scale: *scale,
            },
            (Geometry::ArcLengthDimension { center, .. }, 0) => self.translated(to - *center),
            (Geometry::ArcLengthDimension { center, radius, start_angle, end_angle, text, scale, .. }, _) => {
                Geometry::ArcLengthDimension {
                    center: *center,
                    radius: *radius,
                    start_angle: *start_angle,
                    end_angle: *end_angle,
                    dim_radius: center.distance(to),
                    text: text.clone(),
                    scale: *scale,
                }
            }
            (Geometry::JoggedDimension { center, point, center_override, jog, text, scale }, i) => {
                let (mut o, mut j) = (*center_override, *jog);
                match i {
                    0 => o = to,
                    1 => j = to,
                    _ => return self.clone(),
                }
                Geometry::JoggedDimension {
                    center: *center,
                    point: *point,
                    center_override: o,
                    jog: j,
                    text: text.clone(),
                    scale: *scale,
                }
            }
            (Geometry::Wipeout { points }, i) => {
                let mut points = points.clone();
                if let Some(p) = points.get_mut(i) {
                    *p = to;
                }
                Geometry::Wipeout { points }
            }
            (Geometry::Viewport { center, .. }, 0) => self.translated(to - *center),
            // A boundary's vertex: the viewport fits round the new shape.
            (Geometry::Viewport { clip, .. }, i) if clip.len() >= 3 => {
                let mut clip = clip.clone();
                if let Some(p) = clip.get_mut(i - 1) {
                    *p = to;
                }
                self.with_clip(clip)
            }
            // A corner: the opposite one stays, and so does the scale.
            (Geometry::Viewport { center, width, height, .. }, i) => {
                let corners = viewport_corners(*center, *width, *height);
                let fixed = corners[(i + 1) % 4];
                self.reframed(fixed.midpoint(to), (to.x - fixed.x).abs(), (to.y - fixed.y).abs())
            }
            (Geometry::Ellipse(e), 0) => self.translated(to - e.center),
            (Geometry::Ellipse(e), 1 | 3) => Geometry::Ellipse(e.with_axes(e.center.distance(to), e.b())),
            (Geometry::Ellipse(e), 2 | 4) => Geometry::Ellipse(e.with_axes(e.a(), e.center.distance(to))),
            (Geometry::Ellipse(e), i) => {
                let t = e.param_at_angle(e.center.angle_to(to));
                Geometry::Ellipse(if i == 5 { e.arc(t, e.end) } else { e.arc(e.start, t) })
            }
            (Geometry::Spline(s), i) => s.with_grip_moved(i, to).map_or_else(|| self.clone(), Geometry::Spline),
            (Geometry::Xline { base, .. } | Geometry::Ray { base, .. }, 0) => self.translated(to - *base),
            (Geometry::Xline { base, .. }, _) if !to.approx_eq(*base) => {
                Geometry::Xline { base: *base, direction: to - *base }
            }
            (Geometry::Ray { base, .. }, _) if !to.approx_eq(*base) => {
                Geometry::Ray { base: *base, direction: to - *base }
            }
            (Geometry::Xline { .. } | Geometry::Ray { .. }, _) => self.clone(),
            // The first corner moves the picture; the others scale it from there.
            (Geometry::Image { origin, .. }, 0) => self.translated(to - *origin),
            (Geometry::Image { origin, u, v, .. }, i) => {
                let corner = crate::underlay::image_corners(*origin, *u, *v)[i.min(3)];
                let (from, now) = (origin.distance(corner), origin.distance(to));
                if from > EPSILON && now > EPSILON { self.scaled(*origin, now / from) } else { self.clone() }
            }
        }
    }
}

/// The part of an ellipse between two places along its approximation.
fn ellipse_sub(e: &Ellipse, s0: f64, s1: f64) -> Option<Geometry> {
    if s1 <= s0 && !e.is_full() {
        return None;
    }
    let (t0, t1) = (e.param_at(s0), e.param_at(s1));
    let sweep = if s1 > s0 && (s1 - s0) >= e.segs().len() as f64 { 360.0 } else { (t1 - t0).rem_euclid(360.0) };
    (sweep > 1e-9).then(|| Geometry::Ellipse(if sweep >= 360.0 { *e } else { e.arc(t0, t1) }))
}

/// The part of a spline between two places along its approximation; on a
/// closed one, `s1 < s0` goes round through the start.
fn spline_sub(s: &Spline, s0: f64, s1: f64) -> Option<Geometry> {
    let n = s.segs().len() as f64;
    let (start, end) = s.domain();
    let at = |x: f64| {
        if x <= 0.0 {
            start
        } else if x >= n {
            end
        } else {
            s.param_at(x)
        }
    };
    let (u0, u1) = (at(s0), at(s1));
    let piece = if s1 > s0 {
        s.sub(u0, u1)
    } else if s.is_closed() {
        match (s.sub(u0, end), s.sub(start, u1)) {
            (Some(a), Some(b)) => a.joined(&b),
            (a, b) => a.or(b),
        }
    } else {
        None
    };
    piece.map(Geometry::Spline)
}

/// The part of a construction line or ray between two parameters of its
/// long stand-in segment: what's left reaching the far end stays infinite
/// (a ray), the rest becomes a line.
fn infinite_sub(g: &Geometry, base: Point, direction: Point, s0: f64, s1: f64) -> Option<Geometry> {
    const END: f64 = 1e-12;
    let seg = g.segments()[0];
    let (a, b) = (seg.point_at(s0.clamp(0.0, 1.0)), seg.point_at(s1.clamp(0.0, 1.0)));
    if s1 <= s0 || a.approx_eq(b) {
        return None;
    }
    let from_start = s0 <= END && matches!(g, Geometry::Xline { .. });
    let to_end = s1 >= 1.0 - END;
    Some(match (from_start, to_end) {
        (true, true) => Geometry::Xline { base, direction },
        (true, false) => Geometry::Ray { base: b, direction: -direction },
        (false, true) => Geometry::Ray { base: a, direction },
        (false, false) => Geometry::Line { start: a, end: b },
    })
}

/// Where a transform sends a direction: the angle of `f(at + dir(angle)) - f(at)`.
fn mapped_angle(f: &dyn Fn(Point) -> Point, at: Point, angle: f64) -> f64 {
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
    /// Model space, or the layout it's drawn on.
    pub space: crate::layout::Space,
    pub layer: String,
    pub color: Color,
    /// A linetype name, or ByLayer / ByBlock (see [`crate::linetype`]).
    pub linetype: String,
    pub lineweight: Lineweight,
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
    fn a_viewport_resized_by_a_corner_keeps_its_scale_and_the_model_in_place() {
        let vp = Geometry::Viewport {
            center: Point::new(50.0, 50.0),
            width: 100.0,
            height: 100.0,
            view_center: Point::new(500.0, 500.0),
            view_height: 1000.0,
            twist: 0.0,
            frozen: Vec::new(),
            locked: false,
            on: true,
            clip: Vec::new(),
        };
        // The upper-right corner (grip 3) out to 200, 150; the lower-left stays.
        let Geometry::Viewport { center, width, height, view_center, view_height, .. } =
            vp.with_grip_moved(3, Point::new(200.0, 150.0))
        else {
            panic!()
        };
        assert_eq!((center, width, height), (Point::new(100.0, 75.0), 200.0, 150.0));
        // Still 1:10, and model 0,0 is still at the sheet's 0,0.
        assert_eq!(view_height, 1500.0);
        assert_eq!(view_center, Point::new(1000.0, 750.0));
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

        let insert = Geometry::Insert {
            block: "B".into(),
            placement: Placement::at(Point::new(5.0, 0.0)),
            attributes: Vec::new(),
            clip: None,
        };
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

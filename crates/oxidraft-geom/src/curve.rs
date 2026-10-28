//! Curve segments (straight or circular) and the operations TRIM, EXTEND,
//! OFFSET and FILLET need: parameters, sub-segments and intersections.
//!
//! A segment's parameter runs from 0 at its start to 1 at its end. For arcs it
//! is the fraction of the sweep, not of the arc length.

use crate::{Aabb, EPSILON, Point, Segment, intersect::segment_touches_aabb};

/// Parameter slack when deciding whether an intersection lies on a segment.
const PARAM_TOL: f64 = 1e-9;
/// The same slack as a distance along the segment, so a crossing exactly
/// where two short pieces of a curve meet isn't lost to rounding.
const LENGTH_TOL: f64 = 1e-7;

/// How far past its ends a parameter on `s` still counts as on it.
fn param_slack(s: &Seg) -> f64 {
    (LENGTH_TOL / s.length().max(EPSILON)).clamp(PARAM_TOL, 1e-3)
}

/// A circular arc sweeping `sweep` degrees from angle `start`
/// (positive = counterclockwise, negative = clockwise).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcSeg {
    pub center: Point,
    pub radius: f64,
    pub start: f64,
    pub sweep: f64,
}

impl ArcSeg {
    /// A full circle as a 360° arc starting at angle 0.
    pub fn circle(center: Point, radius: f64) -> Self {
        Self { center, radius, start: 0.0, sweep: 360.0 }
    }

    /// A counterclockwise arc from `start_angle` to `end_angle` (DXF convention).
    pub fn ccw(center: Point, radius: f64, start_angle: f64, end_angle: f64) -> Self {
        let mut sweep = (end_angle - start_angle).rem_euclid(360.0);
        if sweep <= EPSILON {
            sweep = 360.0;
        }
        Self { center, radius, start: start_angle, sweep }
    }

    /// Start and end angles walking counterclockwise, each in `[0, 360)`.
    pub fn ccw_angles(&self) -> (f64, f64) {
        let (a, b) = if self.sweep >= 0.0 {
            (self.start, self.start + self.sweep)
        } else {
            (self.start + self.sweep, self.start)
        };
        (a.rem_euclid(360.0), b.rem_euclid(360.0))
    }

    pub fn is_full(&self) -> bool {
        self.sweep.abs() >= 360.0 - 1e-9
    }

    pub fn point_at(&self, t: f64) -> Point {
        self.center.polar(self.radius, self.start + self.sweep * t)
    }

    pub fn start_point(&self) -> Point {
        self.point_at(0.0)
    }

    pub fn end_point(&self) -> Point {
        self.point_at(1.0)
    }

    /// The arc from `p0` to `p1` described by a polyline bulge: the tangent of a
    /// quarter of the sweep, negative for clockwise. `None` for a straight segment.
    pub fn from_bulge(p0: Point, p1: Point, bulge: f64) -> Option<Self> {
        if bulge.abs() < 1e-12 || p0.approx_eq(p1) {
            return None;
        }
        let sweep = 4.0 * bulge.atan();
        let chord = p0.distance(p1);
        let radius = chord / (2.0 * (sweep / 2.0).sin().abs());
        let d = (p1 - p0) * (1.0 / chord);
        let left = Point::new(-d.y, d.x);
        // The center sits left of the chord for counterclockwise arcs under 180°.
        let h = radius * (sweep / 2.0).cos();
        let center = p0.midpoint(p1) + left * (h * sweep.signum());
        Some(Self { center, radius, start: center.angle_to(p0), sweep: sweep.to_degrees() })
    }

    pub fn bulge(&self) -> f64 {
        (self.sweep.to_radians() / 4.0).tan()
    }

    /// The arc that starts at `p0`, passes through `p1` and ends at `p2`.
    pub fn through(p0: Point, p1: Point, p2: Point) -> Option<Self> {
        let d = 2.0 * (p0.x * (p1.y - p2.y) + p1.x * (p2.y - p0.y) + p2.x * (p0.y - p1.y));
        let scale = p0.distance(p1).max(p1.distance(p2)).max(1.0);
        if d.abs() <= EPSILON * scale * scale {
            return None;
        }
        let (s0, s1, s2) = (p0.dot(p0), p1.dot(p1), p2.dot(p2));
        let center = Point::new(
            (s0 * (p1.y - p2.y) + s1 * (p2.y - p0.y) + s2 * (p0.y - p1.y)) / d,
            (s0 * (p2.x - p1.x) + s1 * (p0.x - p2.x) + s2 * (p1.x - p0.x)) / d,
        );
        let (a0, a1, a2) = (center.angle_to(p0), center.angle_to(p1), center.angle_to(p2));
        let ccw = (a2 - a0).rem_euclid(360.0);
        let sweep = if (a1 - a0).rem_euclid(360.0) <= ccw { ccw } else { ccw - 360.0 };
        Some(Self { center, radius: center.distance(p0), start: a0, sweep })
    }

    /// The arc that leaves `p0` heading in direction `dir_deg` and ends at `p1`
    /// (PLINE's arc mode). `None` if `p1` is straight ahead.
    pub fn tangent(p0: Point, dir_deg: f64, p1: Point) -> Option<Self> {
        let alpha = (p0.angle_to(p1) - dir_deg + 180.0).rem_euclid(360.0) - 180.0;
        if alpha.abs() < 1e-9 {
            return None;
        }
        Self::from_bulge(p0, p1, (alpha.to_radians() / 2.0).tan())
    }

    /// Parameter of an angle on the arc's circle, in `[0, 360/|sweep|)`; values
    /// above 1 lie past the end.
    pub fn param_of_angle(&self, angle: f64) -> f64 {
        let delta = if self.sweep >= 0.0 {
            (angle - self.start).rem_euclid(360.0)
        } else {
            (self.start - angle).rem_euclid(360.0)
        };
        delta / self.sweep.abs()
    }

    /// Parameter of the closest point on the arc to `p`.
    pub fn param_of(&self, p: Point) -> f64 {
        let t = self.param_of_angle(self.center.angle_to(p));
        if t <= 1.0 {
            t
        } else if p.distance(self.start_point()) <= p.distance(self.end_point()) {
            0.0
        } else {
            1.0
        }
    }

    pub fn length(&self) -> f64 {
        self.radius * self.sweep.abs().to_radians()
    }

    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::from_points(self.start_point(), self.end_point());
        for k in 0..4 {
            let a = 90.0 * f64::from(k);
            if self.param_of_angle(a) <= 1.0 {
                let p = self.center.polar(self.radius, a);
                b = b.union(Aabb::from_points(p, p));
            }
        }
        b
    }

    /// Unit tangent in the direction of travel at parameter `t`.
    pub fn tangent_at(&self, t: f64) -> Point {
        let a = (self.start + self.sweep * t).to_radians();
        Point::new(-a.sin(), a.cos()) * self.sweep.signum()
    }
}

/// One piece of a curve: a straight segment or a circular arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line(Segment),
    Arc(ArcSeg),
}

impl Seg {
    /// A polyline segment from `p0` to `p1` with the given bulge.
    pub fn from_bulge(p0: Point, p1: Point, bulge: f64) -> Seg {
        ArcSeg::from_bulge(p0, p1, bulge).map_or(Seg::Line(Segment::new(p0, p1)), Seg::Arc)
    }

    pub fn bulge(&self) -> f64 {
        match self {
            Seg::Line(_) => 0.0,
            Seg::Arc(a) => a.bulge(),
        }
    }

    pub fn start(&self) -> Point {
        self.point_at(0.0)
    }

    pub fn end(&self) -> Point {
        self.point_at(1.0)
    }

    pub fn point_at(&self, t: f64) -> Point {
        match self {
            Seg::Line(s) => s.start.lerp(s.end, t),
            Seg::Arc(a) => a.point_at(t),
        }
    }

    /// Parameter of the closest point, in `[0, 1]`.
    pub fn param_of(&self, p: Point) -> f64 {
        match self {
            Seg::Line(s) => {
                let d = s.end - s.start;
                let len2 = d.dot(d);
                if len2 <= EPSILON * EPSILON { 0.0 } else { ((p - s.start).dot(d) / len2).clamp(0.0, 1.0) }
            }
            Seg::Arc(a) => a.param_of(p),
        }
    }

    pub fn distance_to(&self, p: Point) -> f64 {
        self.point_at(self.param_of(p)).distance(p)
    }

    pub fn length(&self) -> f64 {
        match self {
            Seg::Line(s) => s.length(),
            Seg::Arc(a) => a.length(),
        }
    }

    pub fn bounds(&self) -> Aabb {
        match self {
            Seg::Line(s) => Aabb::from_points(s.start, s.end),
            Seg::Arc(a) => a.bounds(),
        }
    }

    /// Unit direction of travel at parameter `t`.
    pub fn tangent_at(&self, t: f64) -> Point {
        match self {
            Seg::Line(s) => {
                let d = s.end - s.start;
                d * (1.0 / d.length().max(EPSILON))
            }
            Seg::Arc(a) => a.tangent_at(t),
        }
    }

    /// The part between parameters `t0` and `t1` (may run backwards or past the ends).
    pub fn sub(&self, t0: f64, t1: f64) -> Seg {
        match self {
            Seg::Line(_) => Seg::Line(Segment::new(self.point_at(t0), self.point_at(t1))),
            Seg::Arc(a) => Seg::Arc(ArcSeg { start: a.start + a.sweep * t0, sweep: a.sweep * (t1 - t0), ..*a }),
        }
    }

    pub fn reversed(&self) -> Seg {
        self.sub(1.0, 0.0)
    }

    /// True if any part of the segment lies inside or on the box.
    pub fn touches(&self, b: &Aabb) -> bool {
        match self {
            Seg::Line(s) => segment_touches_aabb(*s, b),
            Seg::Arc(_) => {
                b.contains(self.start())
                    || b.contains(self.end())
                    || b.edges().iter().any(|e| !self.intersections(&Seg::Line(*e)).is_empty())
            }
        }
    }

    /// Parameter of `p` on the segment's carrier (infinite line or full circle),
    /// not clamped: lines give any real value, arcs give `[0, 360/|sweep|)`.
    fn carrier_param(&self, p: Point) -> f64 {
        match self {
            Seg::Line(s) => {
                let d = s.end - s.start;
                (p - s.start).dot(d) / d.dot(d).max(EPSILON * EPSILON)
            }
            Seg::Arc(a) => a.param_of_angle(a.center.angle_to(p)),
        }
    }

    /// Where both segments meet, as `(param on self, param on other)`.
    pub fn intersections(&self, other: &Seg) -> Vec<(f64, f64)> {
        let (mine, theirs) = (param_slack(self), param_slack(other));
        carrier_points(self, other)
            .into_iter()
            .map(|p| (self.carrier_param(p), other.carrier_param(p)))
            .filter(|&(t, u)| (-mine..=1.0 + mine).contains(&t) && (-theirs..=1.0 + theirs).contains(&u))
            .map(|(t, u)| (t.clamp(0.0, 1.0), u.clamp(0.0, 1.0)))
            .collect()
    }

    /// Where `other` crosses this segment's carrier (EXTEND's boundary hits),
    /// as parameters on self's carrier; see [`Seg::carrier_param`].
    pub fn extended_intersections(&self, other: &Seg) -> Vec<f64> {
        let slack = param_slack(other);
        carrier_points(self, other)
            .into_iter()
            .filter(|&p| (-slack..=1.0 + slack).contains(&other.carrier_param(p)))
            .map(|p| self.carrier_param(p))
            .collect()
    }
}

/// Circular arcs following the curve `f` from parameter `t0` to `t1`, each
/// with the parameter where it starts. Each arc goes through three points of
/// the curve; a piece whose arc strays more than `tol` from the curve
/// halfway between them is split in two, at most `depth` times.
pub fn fit_arcs(f: &dyn Fn(f64) -> Point, t0: f64, t1: f64, tol: f64, depth: u32, out: &mut Vec<(f64, Seg)>) {
    let at = |k: f64| f(t0 + (t1 - t0) * k);
    let (a, m, b) = (at(0.0), at(0.5), at(1.0));
    let seg = ArcSeg::through(a, m, b).map_or(Seg::Line(Segment::new(a, b)), Seg::Arc);
    if depth == 0 || (seg.distance_to(at(0.25)) <= tol && seg.distance_to(at(0.75)) <= tol) {
        out.push((t0, seg));
    } else {
        let mid = (t0 + t1) / 2.0;
        fit_arcs(f, t0, mid, tol, depth - 1, out);
        fit_arcs(f, mid, t1, tol, depth - 1, out);
    }
}

/// Intersection points of the two segments' carriers (infinite lines / full circles).
pub fn carrier_points(a: &Seg, b: &Seg) -> Vec<Point> {
    match (a, b) {
        (Seg::Line(s), Seg::Line(t)) => {
            let r = s.end - s.start;
            let q = t.end - t.start;
            let denom = r.cross(q);
            if denom.abs() <= EPSILON * r.length().max(1.0) * q.length().max(1.0) {
                return Vec::new();
            }
            vec![s.start + r * ((t.start - s.start).cross(q) / denom)]
        }
        (Seg::Line(s), Seg::Arc(c)) | (Seg::Arc(c), Seg::Line(s)) => line_circle(*s, c.center, c.radius),
        (Seg::Arc(c1), Seg::Arc(c2)) => crate::intersect::circle_circle(c1.center, c1.radius, c2.center, c2.radius),
    }
}

/// Where the line through `s` meets the circle: from the foot of the
/// perpendicular from the center, which stays accurate however long `s` is
/// (construction lines stand in as very long segments).
fn line_circle(s: Segment, center: Point, radius: f64) -> Vec<Point> {
    let d = s.end - s.start;
    let len = d.length();
    if len <= EPSILON {
        return Vec::new();
    }
    let u = d * (1.0 / len);
    let foot = s.start + u * (center - s.start).dot(u);
    let off = center - foot;
    let h2 = radius * radius - off.dot(off);
    if h2 < -EPSILON * radius.max(1.0) {
        return Vec::new();
    }
    let h = h2.max(0.0).sqrt();
    if h <= EPSILON {
        return vec![foot];
    }
    vec![foot - u * h, foot + u * h]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn bulge_roundtrip_both_directions() {
        // Quarter circle CCW from (1,0) to (0,1) around the origin.
        let a = ArcSeg::from_bulge(Point::new(1.0, 0.0), Point::new(0.0, 1.0), (22.5f64).to_radians().tan()).unwrap();
        assert!(a.center.approx_eq(Point::ORIGIN));
        assert!(close(a.radius, 1.0) && close(a.sweep, 90.0));
        assert!(close(a.bulge(), (22.5f64).to_radians().tan()));
        // Clockwise half circle.
        let a = ArcSeg::from_bulge(Point::new(0.0, 0.0), Point::new(2.0, 0.0), -1.0).unwrap();
        assert!(a.center.approx_eq(Point::new(1.0, 0.0)) && close(a.sweep, -180.0));
        assert!(a.point_at(0.5).approx_eq(Point::new(1.0, 1.0)));
    }

    #[test]
    fn three_point_arc_direction() {
        let a = ArcSeg::through(Point::new(1.0, 0.0), Point::new(0.0, 1.0), Point::new(-1.0, 0.0)).unwrap();
        assert!(a.center.approx_eq(Point::ORIGIN) && close(a.sweep, 180.0));
        let a = ArcSeg::through(Point::new(1.0, 0.0), Point::new(0.0, -1.0), Point::new(-1.0, 0.0)).unwrap();
        assert!(close(a.sweep, -180.0));
        assert!(ArcSeg::through(Point::ORIGIN, Point::new(1.0, 1.0), Point::new(2.0, 2.0)).is_none());
    }

    #[test]
    fn tangent_arc_continues_direction() {
        // Heading east from the origin, ending at (1,1): a CCW quarter circle centered at (0,1).
        let a = ArcSeg::tangent(Point::ORIGIN, 0.0, Point::new(1.0, 1.0)).unwrap();
        assert!(a.center.approx_eq(Point::new(0.0, 1.0)) && close(a.sweep, 90.0));
        assert!(a.tangent_at(0.0).approx_eq(Point::new(1.0, 0.0)));
    }

    #[test]
    fn arc_bounds_include_quadrants_inside_sweep() {
        let a = ArcSeg::ccw(Point::ORIGIN, 1.0, 45.0, 135.0);
        let b = a.bounds();
        assert!(close(b.max.y, 1.0) && close(b.min.y, (45f64).to_radians().sin()));
    }

    #[test]
    fn intersections_report_params_on_both() {
        let line = Seg::Line(Segment::new(Point::new(-2.0, 0.5), Point::new(2.0, 0.5)));
        let arc = Seg::Arc(ArcSeg::ccw(Point::ORIGIN, 1.0, 0.0, 90.0));
        let hits = line.intersections(&arc);
        assert_eq!(hits.len(), 1, "only the hit inside the arc's sweep counts");
        let (t, u) = hits[0];
        assert!(line.point_at(t).approx_eq(arc.point_at(u)));
    }

    #[test]
    fn extended_intersections_go_past_the_end() {
        let line = Seg::Line(Segment::new(Point::new(0.0, 0.0), Point::new(1.0, 0.0)));
        let wall = Seg::Line(Segment::new(Point::new(5.0, -1.0), Point::new(5.0, 1.0)));
        assert_eq!(line.extended_intersections(&wall), vec![5.0]);
        assert!(line.intersections(&wall).is_empty());
    }

    #[test]
    fn sub_segments() {
        let arc = Seg::Arc(ArcSeg::ccw(Point::ORIGIN, 2.0, 0.0, 180.0));
        let half = arc.sub(0.0, 0.5);
        assert!(half.end().approx_eq(Point::new(0.0, 2.0)));
        assert!(arc.reversed().start().approx_eq(Point::new(-2.0, 0.0)));
    }
}

//! Ellipses and elliptical arcs (DXF ELLIPSE). Everything that works on
//! line and arc segments (snaps, TRIM, OFFSET, hatching) sees an ellipse as
//! a chain of short circular arcs, close enough to the true curve to draw,
//! snap to and cut.

use crate::{
    Point,
    curve::{Seg, fit_arcs},
};

/// Pieces per full turn the approximation starts from; each is split
/// further until its arc is within 1e-7 of the ellipse's size.
const PIECES: f64 = 8.0;

/// An ellipse around `center`. `major` runs from the center to one end of
/// the major axis; the minor axis is `ratio` times as long, a quarter turn
/// counterclockwise from it. An elliptical arc runs counterclockwise from
/// parameter `start` to `end` (degrees: 0 at the end of `major`, 90 at the
/// end of the minor axis); a full ellipse runs from 0 to 360.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    pub center: Point,
    pub major: Point,
    pub ratio: f64,
    pub start: f64,
    pub end: f64,
}

impl Ellipse {
    pub fn full(center: Point, major: Point, ratio: f64) -> Self {
        Self { center, major, ratio, start: 0.0, end: 360.0 }
    }

    /// From the center to the end of the minor axis.
    pub fn minor(&self) -> Point {
        Point::new(-self.major.y, self.major.x) * self.ratio
    }

    /// Half the major axis.
    pub fn a(&self) -> f64 {
        self.major.length()
    }

    /// Half the minor axis.
    pub fn b(&self) -> f64 {
        self.major.length() * self.ratio
    }

    /// Degrees of parameter the arc covers, in `(0, 360]`.
    pub fn sweep(&self) -> f64 {
        let s = (self.end - self.start).rem_euclid(360.0);
        if s <= 1e-9 { 360.0 } else { s }
    }

    pub fn is_full(&self) -> bool {
        self.sweep() >= 360.0 - 1e-9
    }

    pub fn point_at(&self, t: f64) -> Point {
        let (sin, cos) = t.to_radians().sin_cos();
        self.center + self.major * cos + self.minor() * sin
    }

    pub fn start_point(&self) -> Point {
        self.point_at(self.start)
    }

    pub fn end_point(&self) -> Point {
        self.point_at(self.end)
    }

    /// The parameter of the point of the ellipse in the direction of `p`
    /// as seen from the center, in `[0, 360)`.
    pub fn param_of(&self, p: Point) -> f64 {
        let a = self.a().max(f64::MIN_POSITIVE);
        let u = self.major * (1.0 / a);
        let v = Point::new(-u.y, u.x);
        let d = p - self.center;
        let (x, y) = (d.dot(u) / a, d.dot(v) / (a * self.ratio).max(f64::MIN_POSITIVE));
        let t = y.atan2(x).to_degrees().rem_euclid(360.0);
        if t >= 360.0 - 1e-9 { 0.0 } else { t }
    }

    /// The parameter where the ray from the center at `angle` (degrees)
    /// meets the ellipse: how ELLIPSE's Arc option turns angles into
    /// parameters.
    pub fn param_at_angle(&self, angle: f64) -> f64 {
        self.param_of(self.center.polar(1.0, angle))
    }

    /// True if parameter `t` lies on the arc.
    pub fn covers(&self, t: f64) -> bool {
        self.is_full() || (t - self.start).rem_euclid(360.0) <= self.sweep() + 1e-9
    }

    /// The approximation: arcs through three points of the ellipse, with
    /// the parameter where each starts.
    fn pieces(&self) -> Vec<(f64, Seg)> {
        let n = (PIECES * self.sweep() / 360.0).ceil().max(2.0) as usize;
        let step = self.sweep() / n as f64;
        let tol = self.a() * 1e-7;
        let mut out = Vec::new();
        for i in 0..n {
            let t0 = self.start + step * i as f64;
            fit_arcs(&|t| self.point_at(t), t0, t0 + step, tol, 10, &mut out);
        }
        out
    }

    /// The curve as circular arcs, each within 1e-7 of the ellipse's size
    /// of it.
    pub fn segs(&self) -> Vec<Seg> {
        self.pieces().into_iter().map(|(_, s)| s).collect()
    }

    /// The parameter at `s` along [`Ellipse::segs`] (piece `i` covers
    /// `[i, i + 1]`).
    pub fn param_at(&self, s: f64) -> f64 {
        let segs = self.segs();
        if s <= 0.0 {
            return self.start;
        }
        if s >= segs.len() as f64 {
            return self.start + self.sweep();
        }
        let i = s.floor() as usize;
        self.param_of(segs[i].point_at(s - i as f64))
    }

    /// The part from parameter `t0` counterclockwise to `t1`.
    pub fn arc(&self, t0: f64, t1: f64) -> Ellipse {
        Ellipse { start: t0.rem_euclid(360.0), end: t1.rem_euclid(360.0), ..*self }
    }

    /// The ellipse after a similarity transform: where it sends points, and
    /// whether it flips orientation (mirroring).
    pub fn mapped(&self, f: &dyn Fn(Point) -> Point, flips: bool) -> Ellipse {
        let center = f(self.center);
        let major = f(self.center + self.major) - center;
        let (start, end) = match (flips, self.is_full()) {
            (_, true) => (0.0, 360.0),
            // A reflection runs the parameter the other way round.
            (true, false) => ((-self.end).rem_euclid(360.0), (-self.start).rem_euclid(360.0)),
            (false, false) => (self.start, self.end),
        };
        Ellipse { center, major, ratio: self.ratio, start, end }
    }

    /// The same curve with its axes resized: `along` the major axis and
    /// `across` it. When the minor axis becomes the longer one the axes
    /// swap, so `ratio` stays at most 1.
    pub fn with_axes(&self, along: f64, across: f64) -> Ellipse {
        let u = self.major * (1.0 / self.a().max(f64::MIN_POSITIVE));
        if across <= along {
            return Ellipse { major: u * along, ratio: across / along.max(f64::MIN_POSITIVE), ..*self };
        }
        // The old minor axis becomes the major one: parameters shift by a quarter turn.
        let shift = |t: f64| (t - 90.0).rem_euclid(360.0);
        let full = self.is_full();
        Ellipse {
            major: Point::new(-u.y, u.x) * across,
            ratio: along / across,
            start: if full { 0.0 } else { shift(self.start) },
            end: if full { 360.0 } else { shift(self.end) },
            ..*self
        }
    }

    /// Enclosed area of the full ellipse.
    pub fn area(&self) -> f64 {
        std::f64::consts::PI * self.a() * self.b()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arcs_follow_the_ellipse_closely() {
        let e = Ellipse::full(Point::new(3.0, 1.0), Point::new(10.0, 0.0), 0.2);
        let pieces = e.pieces();
        assert!(pieces.len() < 400, "{} pieces", pieces.len());
        // Between the three points each arc goes through, the gap stays tiny.
        for (i, (t0, seg)) in pieces.iter().enumerate() {
            let t1 = pieces.get(i + 1).map_or(360.0, |p| p.0);
            for k in [0.125, 0.375, 0.625, 0.875] {
                let on = e.point_at(t0 + (t1 - t0) * k);
                assert!(seg.distance_to(on) < 5e-6, "piece {i}: {}", seg.distance_to(on));
            }
        }
        let segs = e.segs();
        assert!(segs[0].start().approx_eq(Point::new(13.0, 1.0)));
        assert!(segs[segs.len() - 1].end().approx_eq(Point::new(13.0, 1.0)));
        // Even a very flat ellipse takes a reasonable number of arcs.
        assert!(Ellipse::full(Point::ORIGIN, Point::new(10.0, 0.0), 0.01).segs().len() < 600);
    }

    #[test]
    fn parameters_angles_and_arcs() {
        let e = Ellipse::full(Point::ORIGIN, Point::new(0.0, 4.0), 0.5);
        assert!(e.minor().approx_eq(Point::new(-2.0, 0.0)));
        assert!((e.param_at_angle(90.0)).abs() < 1e-9);
        assert!((e.param_at_angle(180.0) - 90.0).abs() < 1e-9);
        let arc = e.arc(0.0, 90.0);
        assert!(arc.end_point().approx_eq(Point::new(-2.0, 0.0)) && !arc.is_full());
        assert!(arc.covers(45.0) && !arc.covers(200.0));
        assert!((e.area() - std::f64::consts::PI * 8.0).abs() < 1e-9);
    }

    #[test]
    fn mirroring_keeps_the_arc_in_place() {
        let arc =
            Ellipse { center: Point::new(5.0, 0.0), major: Point::new(4.0, 0.0), ratio: 0.5, start: 0.0, end: 90.0 };
        let m = arc.mapped(&|p| Point::new(p.x, -p.y), true);
        // The mirrored arc covers the mirror images of the original's ends.
        let ends = [m.start_point(), m.end_point()];
        for p in [arc.start_point(), arc.end_point()] {
            let image = Point::new(p.x, -p.y);
            assert!(ends.iter().any(|e| e.approx_eq(image)), "{image:?} in {ends:?}");
        }
        assert!(m.point_at(m.start + m.sweep() / 2.0).y < 0.0);
    }

    #[test]
    fn resizing_past_the_major_axis_swaps_them() {
        let arc = Ellipse { center: Point::ORIGIN, major: Point::new(4.0, 0.0), ratio: 0.5, start: 10.0, end: 80.0 };
        let r = arc.with_axes(1.0, 2.0);
        assert!(r.major.approx_eq(Point::new(0.0, 2.0)) && (r.ratio - 0.5).abs() < 1e-12);
        // Same shape as resizing without swapping would give.
        let p = Point::new(10f64.to_radians().cos(), 2.0 * 10f64.to_radians().sin());
        assert!(r.start_point().approx_eq(p));
    }
}

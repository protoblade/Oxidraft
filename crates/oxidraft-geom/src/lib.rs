//! 2D geometry primitives used by every other Oxidraft crate.
//!
//! All coordinates are `f64` drawing units (world space). Screen-space
//! conversion lives in the app, not here.

pub mod curve;
pub mod ellipse;
pub mod intersect;
pub mod offset;
pub mod spline;

use std::ops::{Add, Mul, Neg, Sub};

/// Tolerance for treating two lengths or coordinates as equal.
pub const EPSILON: f64 = 1e-9;

/// A point (or vector) in drawing space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Point at `distance` from `self` in direction `angle_deg` (0° = +X, CCW positive).
    pub fn polar(self, distance: f64, angle_deg: f64) -> Point {
        let a = angle_deg.to_radians();
        Point::new(self.x + distance * a.cos(), self.y + distance * a.sin())
    }

    pub fn distance(self, other: Point) -> f64 {
        (other - self).length()
    }

    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    /// Angle of the vector `self -> other` in degrees, in `[0, 360)`.
    pub fn angle_to(self, other: Point) -> f64 {
        let d = other - self;
        d.y.atan2(d.x).to_degrees().rem_euclid(360.0)
    }

    pub fn dot(self, other: Point) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// Z component of the 3D cross product; positive when `other` is CCW of `self`.
    pub fn cross(self, other: Point) -> f64 {
        self.x * other.y - self.y * other.x
    }

    pub fn lerp(self, other: Point, t: f64) -> Point {
        self + (other - self) * t
    }

    pub fn midpoint(self, other: Point) -> Point {
        self.lerp(other, 0.5)
    }

    pub fn approx_eq(self, other: Point) -> bool {
        self.distance(other) <= EPSILON
    }

    /// This point rotated about `base` by `angle_deg` (counterclockwise).
    pub fn rotated_about(self, base: Point, angle_deg: f64) -> Point {
        let (sin, cos) = angle_deg.to_radians().sin_cos();
        let d = self - base;
        base + Point::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    }

    /// This point scaled away from `base` by `factor`.
    pub fn scaled_about(self, base: Point, factor: f64) -> Point {
        base + (self - base) * factor
    }

    /// Reflection of this point across the infinite line through `a` and `b`.
    pub fn mirrored(self, a: Point, b: Point) -> Point {
        let d = b - a;
        let len2 = d.dot(d);
        if len2 <= EPSILON * EPSILON {
            return self;
        }
        let foot = a + d * ((self - a).dot(d) / len2);
        foot * 2.0 - self
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for Point {
    type Output = Point;
    fn mul(self, s: f64) -> Point {
        Point::new(self.x * s, self.y * s)
    }
}

impl Neg for Point {
    type Output = Point;
    fn neg(self) -> Point {
        Point::new(-self.x, -self.y)
    }
}

/// A straight segment between two points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub start: Point,
    pub end: Point,
}

impl Segment {
    pub const fn new(start: Point, end: Point) -> Self {
        Self { start, end }
    }

    pub fn length(&self) -> f64 {
        self.start.distance(self.end)
    }

    pub fn midpoint(&self) -> Point {
        self.start.midpoint(self.end)
    }

    /// Closest point on the segment to `p`.
    pub fn closest_point(&self, p: Point) -> Point {
        let d = self.end - self.start;
        let len2 = d.dot(d);
        if len2 <= EPSILON * EPSILON {
            return self.start;
        }
        let t = ((p - self.start).dot(d) / len2).clamp(0.0, 1.0);
        self.start.lerp(self.end, t)
    }

    pub fn distance_to(&self, p: Point) -> f64 {
        self.closest_point(p).distance(p)
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Point,
    pub max: Point,
}

impl Aabb {
    pub fn from_points(a: Point, b: Point) -> Self {
        Self { min: Point::new(a.x.min(b.x), a.y.min(b.y)), max: Point::new(a.x.max(b.x), a.y.max(b.y)) }
    }

    pub fn union(self, other: Aabb) -> Aabb {
        Aabb {
            min: Point::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Point::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    pub fn center(&self) -> Point {
        self.min.midpoint(self.max)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn contains_aabb(&self, other: &Aabb) -> bool {
        self.contains(other.min) && self.contains(other.max)
    }

    pub fn corners(&self) -> [Point; 4] {
        [self.min, Point::new(self.max.x, self.min.y), self.max, Point::new(self.min.x, self.max.y)]
    }

    pub fn edges(&self) -> [Segment; 4] {
        let c = self.corners();
        [Segment::new(c[0], c[1]), Segment::new(c[1], c[2]), Segment::new(c[2], c[3]), Segment::new(c[3], c[0])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polar_and_angle_roundtrip() {
        let p = Point::new(1.0, 1.0);
        let q = p.polar(10.0, 30.0);
        assert!((p.distance(q) - 10.0).abs() < 1e-12);
        assert!((p.angle_to(q) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn rotate_scale_mirror() {
        let base = Point::new(1.0, 1.0);
        assert!(Point::new(3.0, 1.0).rotated_about(base, 90.0).approx_eq(Point::new(1.0, 3.0)));
        assert!(Point::new(3.0, 2.0).scaled_about(base, 2.0).approx_eq(Point::new(5.0, 3.0)));
        let p = Point::new(2.0, 5.0).mirrored(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert!(p.approx_eq(Point::new(2.0, -5.0)));
        let p = Point::new(3.0, 0.0).mirrored(Point::new(0.0, 0.0), Point::new(1.0, 1.0));
        assert!(p.approx_eq(Point::new(0.0, 3.0)));
    }

    #[test]
    fn angle_is_normalized() {
        assert!((Point::ORIGIN.angle_to(Point::new(0.0, -1.0)) - 270.0).abs() < 1e-9);
    }

    #[test]
    fn segment_closest_point_clamps() {
        let s = Segment::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert_eq!(s.closest_point(Point::new(5.0, 3.0)), Point::new(5.0, 0.0));
        assert_eq!(s.closest_point(Point::new(-4.0, 3.0)), Point::new(0.0, 0.0));
        assert!((s.distance_to(Point::new(13.0, 4.0)) - 5.0).abs() < 1e-12);
    }
}

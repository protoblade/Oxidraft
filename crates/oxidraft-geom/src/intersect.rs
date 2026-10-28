//! Intersections between segments and circles.
//!
//! These back the Intersection object snap and crossing selection, and will
//! back TRIM/EXTEND later.

use crate::{Aabb, EPSILON, Point, Segment};

/// Intersection point of two segments, if they cross or touch.
/// Parallel overlapping segments report no point.
pub fn segment_segment(a: Segment, b: Segment) -> Option<Point> {
    let r = a.end - a.start;
    let s = b.end - b.start;
    let denom = r.cross(s);
    if denom.abs() <= EPSILON {
        return None;
    }
    let qp = b.start - a.start;
    let t = qp.cross(s) / denom;
    let u = qp.cross(r) / denom;
    let tol = 1e-12;
    ((-tol..=1.0 + tol).contains(&t) && (-tol..=1.0 + tol).contains(&u)).then(|| a.start.lerp(a.end, t))
}

/// The part of `s` inside the box `b`, if any (Liang–Barsky clipping).
pub fn clip_segment(s: Segment, b: &Aabb) -> Option<Segment> {
    let d = s.end - s.start;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    let sides = [
        (-d.x, s.start.x - b.min.x),
        (d.x, b.max.x - s.start.x),
        (-d.y, s.start.y - b.min.y),
        (d.y, b.max.y - s.start.y),
    ];
    for (p, q) in sides {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            t0 = t0.max(r);
        } else {
            t1 = t1.min(r);
        }
        if t0 > t1 {
            return None;
        }
    }
    Some(Segment::new(s.start + d * t0, s.start + d * t1))
}

/// Points where a segment crosses a circle's outline (0, 1 or 2).
pub fn segment_circle(seg: Segment, center: Point, radius: f64) -> Vec<Point> {
    let d = seg.end - seg.start;
    let f = seg.start - center;
    let a = d.dot(d);
    if a <= EPSILON * EPSILON {
        return Vec::new();
    }
    let b = 2.0 * f.dot(d);
    let c = f.dot(f) - radius * radius;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return Vec::new();
    }
    let sq = disc.sqrt();
    let mut ts = vec![(-b - sq) / (2.0 * a)];
    if sq > EPSILON {
        ts.push((-b + sq) / (2.0 * a));
    }
    ts.into_iter().filter(|t| (-1e-12..=1.0 + 1e-12).contains(t)).map(|t| seg.start.lerp(seg.end, t)).collect()
}

/// Points where two circle outlines cross (0, 1 or 2).
pub fn circle_circle(c1: Point, r1: f64, c2: Point, r2: f64) -> Vec<Point> {
    let d = c1.distance(c2);
    if d <= EPSILON || d > r1 + r2 + EPSILON || d < (r1 - r2).abs() - EPSILON {
        return Vec::new();
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let dir = (c2 - c1) * (1.0 / d);
    let mid = c1 + dir * a;
    let perp = Point::new(-dir.y, dir.x);
    if h <= EPSILON { vec![mid] } else { vec![mid + perp * h, mid - perp * h] }
}

/// True if any part of the segment lies inside or on the box.
pub fn segment_touches_aabb(seg: Segment, b: &Aabb) -> bool {
    b.contains(seg.start) || b.contains(seg.end) || b.edges().iter().any(|e| segment_segment(seg, *e).is_some())
}

/// True if any part of the circle's outline lies inside or on the box.
pub fn circle_touches_aabb(center: Point, radius: f64, b: &Aabb) -> bool {
    let closest = Point::new(center.x.clamp(b.min.x, b.max.x), center.y.clamp(b.min.y, b.max.y));
    let farthest = b.corners().into_iter().map(|c| c.distance(center)).fold(0.0, f64::max);
    closest.distance(center) <= radius && farthest >= radius
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(ax: f64, ay: f64, bx: f64, by: f64) -> Segment {
        Segment::new(Point::new(ax, ay), Point::new(bx, by))
    }

    #[test]
    fn segments_cross_or_not() {
        let p = segment_segment(seg(0.0, 0.0, 10.0, 10.0), seg(0.0, 10.0, 10.0, 0.0)).unwrap();
        assert!(p.approx_eq(Point::new(5.0, 5.0)));
        assert!(segment_segment(seg(0.0, 0.0, 1.0, 1.0), seg(0.0, 10.0, 10.0, 0.0)).is_none());
        assert!(segment_segment(seg(0.0, 0.0, 10.0, 0.0), seg(0.0, 1.0, 10.0, 1.0)).is_none());
        // Touching at an endpoint counts.
        assert!(segment_segment(seg(0.0, 0.0, 5.0, 0.0), seg(5.0, 0.0, 5.0, 5.0)).is_some());
    }

    #[test]
    fn segment_circle_hits() {
        let hits = segment_circle(seg(-10.0, 0.0, 10.0, 0.0), Point::ORIGIN, 5.0);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|p| p.approx_eq(Point::new(-5.0, 0.0))));
        assert_eq!(segment_circle(seg(0.0, 0.0, 1.0, 0.0), Point::ORIGIN, 5.0).len(), 0, "inside, no crossing");
        assert_eq!(segment_circle(seg(-10.0, 5.0, 10.0, 5.0), Point::ORIGIN, 5.0).len(), 1, "tangent");
    }

    #[test]
    fn circle_circle_hits() {
        let hits = circle_circle(Point::ORIGIN, 5.0, Point::new(8.0, 0.0), 5.0);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|p| (p.x - 4.0).abs() < 1e-9 && (p.y.abs() - 3.0).abs() < 1e-9));
        assert!(circle_circle(Point::ORIGIN, 1.0, Point::new(8.0, 0.0), 1.0).is_empty());
        assert!(circle_circle(Point::ORIGIN, 5.0, Point::new(1.0, 0.0), 1.0).is_empty(), "nested");
    }

    #[test]
    fn crossing_tests() {
        let b = Aabb::from_points(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert!(segment_touches_aabb(seg(-5.0, 5.0, 15.0, 5.0), &b));
        assert!(!segment_touches_aabb(seg(-5.0, 15.0, 15.0, 15.0), &b));
        assert!(circle_touches_aabb(Point::new(5.0, 5.0), 2.0, &b));
        assert!(!circle_touches_aabb(Point::new(5.0, 5.0), 100.0, &b), "box entirely inside circle");
        assert!(circle_touches_aabb(Point::new(-3.0, 5.0), 4.0, &b));
    }
}

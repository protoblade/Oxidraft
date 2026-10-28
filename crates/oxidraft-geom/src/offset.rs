//! Parallel offsets (the OFFSET command).
//!
//! Positive distances offset to the left of the direction of travel.

use cavalier_contours::polyline::{PlineSource, PlineSourceMut, Polyline};

use crate::{
    EPSILON, Point, Segment,
    curve::{ArcSeg, Seg, carrier_points},
};

/// A single line or arc offset by `distance`, or `None` if an arc would
/// collapse through its center.
pub fn offset_seg(seg: &Seg, distance: f64) -> Option<Seg> {
    match seg {
        Seg::Line(s) => {
            let d = s.end - s.start;
            let len = d.length();
            if len <= EPSILON {
                return None;
            }
            let left = Point::new(-d.y, d.x) * (distance / len);
            Some(Seg::Line(Segment::new(s.start + left, s.end + left)))
        }
        Seg::Arc(a) => {
            // Left of a counterclockwise arc is toward its center.
            let radius = a.radius - distance * a.sweep.signum();
            (radius > EPSILON).then_some(Seg::Arc(ArcSeg { radius, ..*a }))
        }
    }
}

/// Offsets a polyline given as `(point, bulge)` vertices. Self-intersections
/// are removed, so the result may be several polylines (or none, when an
/// inward offset is too large). Gaps at outside corners between straight
/// segments are closed by extending them to a sharp corner, like AutoCAD's
/// default `OFFSETGAPTYPE=0`.
pub fn offset_polyline(vertices: &[(Point, f64)], closed: bool, distance: f64) -> Vec<(Vec<(Point, f64)>, bool)> {
    let mut pline = Polyline::new();
    for &(p, bulge) in vertices {
        pline.add(p.x, p.y, bulge);
    }
    pline.set_is_closed(closed);
    let corners: Vec<Point> = vertices.iter().map(|v| v.0).collect();
    pline
        .parallel_offset(distance)
        .into_iter()
        .map(|o| {
            let verts: Vec<(Point, f64)> = o.iter_vertexes().map(|v| (Point::new(v.x, v.y), v.bulge)).collect();
            (sharpen_joins(&verts, o.is_closed(), &corners, distance.abs()), o.is_closed())
        })
        .collect()
}

/// Replaces the round joins the offset inserts around original corners with
/// the intersection of the neighbouring straight segments.
fn sharpen_joins(verts: &[(Point, f64)], closed: bool, corners: &[Point], distance: f64) -> Vec<(Point, f64)> {
    let n = verts.len();
    let seg_count = if closed { n } else { n.saturating_sub(1) };
    let mut segs: Vec<Seg> =
        (0..seg_count).map(|i| Seg::from_bulge(verts[i].0, verts[(i + 1) % n].0, verts[i].1)).collect();
    let tol = 1e-6 * distance.max(1.0);
    let mut i = 0;
    while i < segs.len() && segs.len() > 2 {
        let is_join = matches!(segs[i], Seg::Arc(a)
            if (a.radius - distance).abs() <= tol && corners.iter().any(|c| c.distance(a.center) <= tol));
        let (prev, next) = match (closed, i) {
            (_, i) if i > 0 && i + 1 < segs.len() => (i - 1, i + 1),
            (true, 0) => (segs.len() - 1, 1),
            (true, i) if i + 1 == segs.len() => (i - 1, 0),
            _ => {
                i += 1;
                continue;
            }
        };
        if let (true, Seg::Line(a), Seg::Line(b)) = (is_join, segs[prev], segs[next])
            && let Some(&x) = carrier_points(&segs[prev], &segs[next]).first()
        {
            segs[prev] = Seg::Line(Segment::new(a.start, x));
            segs[next] = Seg::Line(Segment::new(x, b.end));
            segs.remove(i);
            continue;
        }
        i += 1;
    }
    let mut out: Vec<(Point, f64)> = segs.iter().map(|s| (s.start(), s.bulge())).collect();
    if !closed && let Some(last) = segs.last() {
        out.push((last.end(), 0.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_arc_offsets() {
        let line = Seg::Line(Segment::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0)));
        let up = offset_seg(&line, 2.0).unwrap();
        assert!(up.start().approx_eq(Point::new(0.0, 2.0)));

        let ccw = Seg::Arc(ArcSeg::ccw(Point::ORIGIN, 5.0, 0.0, 90.0));
        let Some(Seg::Arc(inner)) = offset_seg(&ccw, 2.0) else { panic!() };
        assert!((inner.radius - 3.0).abs() < 1e-12, "left of a CCW arc is inside");
        assert!(offset_seg(&ccw, 6.0).is_none());
    }

    #[test]
    fn closed_square_offsets_inward_and_outward() {
        // Counterclockwise unit square: left is inside.
        let sq = [
            (Point::new(0.0, 0.0), 0.0),
            (Point::new(10.0, 0.0), 0.0),
            (Point::new(10.0, 10.0), 0.0),
            (Point::new(0.0, 10.0), 0.0),
        ];
        let inner = offset_polyline(&sq, true, 1.0);
        assert_eq!(inner.len(), 1);
        assert!(inner[0].1);
        assert!(inner[0].0.iter().any(|(p, _)| p.approx_eq(Point::new(1.0, 1.0))));

        let outer = offset_polyline(&sq, true, -1.0);
        assert_eq!(outer.len(), 1);
        // Outside corners stay sharp, as with AutoCAD's OFFSETGAPTYPE=0.
        assert_eq!(outer[0].0.len(), 4);
        assert!(outer[0].0.iter().all(|&(_, b)| b.abs() < 1e-12));
        assert!(outer[0].0.iter().any(|(p, _)| p.approx_eq(Point::new(-1.0, -1.0))));

        assert!(offset_polyline(&sq, true, 6.0).is_empty(), "too far inward leaves nothing");
    }

    #[test]
    fn open_polyline_keeps_its_ends() {
        let l = [(Point::new(0.0, 0.0), 0.0), (Point::new(10.0, 0.0), 0.0), (Point::new(10.0, 10.0), 0.0)];
        // Right of travel is the outside of this corner.
        let out = offset_polyline(&l, false, -1.0);
        assert_eq!(out.len(), 1);
        let pts: Vec<Point> = out[0].0.iter().map(|v| v.0).collect();
        assert!(pts[0].approx_eq(Point::new(0.0, -1.0)));
        assert!(pts[1].approx_eq(Point::new(11.0, -1.0)));
        assert!(pts.last().unwrap().approx_eq(Point::new(11.0, 10.0)));
    }
}

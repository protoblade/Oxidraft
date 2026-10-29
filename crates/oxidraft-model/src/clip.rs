//! XCLIP: what a clipped block reference shows. Curves are cut where they
//! cross the boundary; fills and pictures are cut to it (fills through the
//! triangles of a boundary that isn't convex); text and points show when
//! their insertion point does.

use oxidraft_geom::{Point, Segment, curve::Seg};

use crate::{Geometry, Styled, Vertex, constant_width, hatch, image_corners};

fn inside(boundary: &[Point], p: Point) -> bool {
    hatch::contains(std::slice::from_ref(&boundary.to_vec()), p)
}

/// Twice the signed area: positive when the ring runs counterclockwise.
fn signed_area(ring: &[Point]) -> f64 {
    (0..ring.len()).map(|i| ring[i].cross(ring[(i + 1) % ring.len()])).sum()
}

fn is_convex(ring: &[Point]) -> bool {
    let n = ring.len();
    let turns: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b, c) = (ring[i], ring[(i + 1) % n], ring[(i + 2) % n]);
            (b - a).cross(c - b)
        })
        .filter(|t| t.abs() > 1e-12)
        .collect();
    turns.iter().all(|t| *t > 0.0) || turns.iter().all(|t| *t < 0.0)
}

/// Sutherland–Hodgman: the part of `ring` inside the convex, counterclockwise `clip`.
fn clip_ring(ring: &[Point], clip: &[Point]) -> Vec<Point> {
    let mut out = ring.to_vec();
    for i in 0..clip.len() {
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let side = |p: Point| (b - a).cross(p - a);
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                out.push(p + (q - p) * (sp / (sp - sq)));
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// The part of `ring` inside the convex polygon `convex` (either way round).
pub fn clip_to_convex(ring: &[Point], convex: &[Point]) -> Vec<Point> {
    clip_ring(ring, &ccw(convex.to_vec()))
}

/// The pieces of a curve on the kept side, or `None` if all of it is kept
/// (or it has no curves).
fn cut_curve(g: &Geometry, edges: &[Seg], keep: &dyn Fn(Point) -> bool) -> Option<Vec<Geometry>> {
    let segs = g.segments();
    let (mut runs, mut run, mut cut) = (Vec::new(), Vec::new(), false);
    for seg in &segs {
        let mut ts: Vec<f64> = edges.iter().flat_map(|e| seg.intersections(e).into_iter().map(|(t, _)| t)).collect();
        ts.extend([0.0, 1.0]);
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        for w in ts.windows(2) {
            if keep(seg.point_at((w[0] + w[1]) / 2.0)) {
                run.push(seg.sub(w[0], w[1]));
            } else {
                cut = true;
                if !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                }
            }
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    if !cut {
        return None;
    }
    let width = match g {
        Geometry::Polyline { vertices, .. } => constant_width(vertices).unwrap_or(0.0),
        _ => 0.0,
    };
    Some(
        runs.into_iter()
            .map(|r| {
                if r.len() == 1 && width == 0.0 {
                    Geometry::from_seg(&r[0])
                } else {
                    Geometry::polyline_from_segs(&r, false).with_width(width)
                }
            })
            .collect(),
    )
}

/// Counterclockwise.
fn ccw(mut ring: Vec<Point>) -> Vec<Point> {
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    ring
}

/// A picture's clipping polygon cut to the boundary, if either is convex.
fn cut_image(g: &Geometry, boundary: &[Point]) -> Option<Geometry> {
    let Geometry::Image { image, origin, u, v, clip } = g else { return None };
    let ring = if clip.len() >= 3 { clip.clone() } else { image_corners(*origin, *u, *v).to_vec() };
    let cut = if is_convex(&ring) {
        clip_ring(&ccw(boundary.to_vec()), &ccw(ring))
    } else if is_convex(boundary) {
        clip_ring(&ring, &ccw(boundary.to_vec()))
    } else {
        return keep_whole(g, boundary);
    };
    (cut.len() >= 3).then(|| Geometry::Image { image: image.clone(), origin: *origin, u: *u, v: *v, clip: cut })
}

fn keep_whole(g: &Geometry, boundary: &[Point]) -> Option<Geometry> {
    inside(boundary, g.bounds().center()).then(|| g.clone())
}

/// A fill cut to the boundary: one hatch per convex piece of it.
fn cut_fill(g: &Geometry, boundary: &[Point]) -> Vec<Geometry> {
    let Geometry::Hatch { loops, pattern, scale, angle } = g else { return Vec::new() };
    let polys = hatch::polygons(loops);
    let pieces: Vec<Vec<Point>> = if is_convex(boundary) {
        vec![boundary.to_vec()]
    } else {
        hatch::triangles(std::slice::from_ref(&boundary.to_vec())).into_iter().map(|t| t.to_vec()).collect()
    };
    pieces
        .into_iter()
        .filter_map(|piece| {
            let piece = ccw(piece);
            let rings: Vec<Vec<Vertex>> = polys
                .iter()
                .map(|r| clip_ring(r, &piece))
                .filter(|r| r.len() >= 3 && signed_area(r).abs() > 1e-12)
                .map(|r| r.into_iter().map(Vertex::at).collect())
                .collect();
            (!rings.is_empty()).then_some(Geometry::Hatch {
                loops: rings,
                pattern: *pattern,
                scale: *scale,
                angle: *angle,
            })
        })
        .collect()
}

/// What of `items` (a reference's contents, in world coordinates) shows
/// with the clipping `boundary`: inside it, or outside when `inverted`.
pub(crate) fn clip_styled(items: Vec<Styled>, boundary: &[Point], inverted: bool) -> Vec<Styled> {
    let keep = |p: Point| inside(boundary, p) != inverted;
    let edges: Vec<Seg> =
        (0..boundary.len()).map(|i| Seg::Line(Segment::new(boundary[i], boundary[(i + 1) % boundary.len()]))).collect();
    let mut out = Vec::with_capacity(items.len());
    for s in items {
        let g = &s.geometry;
        let shown: Vec<Geometry> = match g {
            Geometry::Text { position, .. } | Geometry::Point { position } => {
                if keep(*position) {
                    vec![g.clone()]
                } else {
                    Vec::new()
                }
            }
            // Fills and pictures are cut exactly inside; outside, they show
            // whole or not at all.
            Geometry::Image { .. } if !inverted => cut_image(g, boundary).into_iter().collect(),
            Geometry::Hatch { .. } if !inverted => cut_fill(g, boundary),
            Geometry::Image { .. } | Geometry::Hatch { .. } => {
                if keep(g.bounds().center()) {
                    vec![g.clone()]
                } else {
                    Vec::new()
                }
            }
            _ => cut_curve(g, &edges, &keep).unwrap_or_else(|| vec![g.clone()]),
        };
        out.extend(shown.into_iter().map(|geometry| Styled { geometry, ..s.clone() }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linetype::Lineweight;

    fn styled(g: Geometry) -> Styled {
        Styled {
            geometry: g,
            color: 7,
            layer: "0".into(),
            linetype: "Continuous".into(),
            lineweight: Lineweight::Default,
        }
    }

    #[test]
    fn curves_are_cut_and_fills_trimmed() {
        let square = vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)];
        let line = Geometry::Line { start: Point::new(-5.0, 5.0), end: Point::new(15.0, 5.0) };
        let shown = clip_styled(vec![styled(line.clone())], &square, false);
        assert_eq!(shown.len(), 1);
        let Geometry::Line { start, end } = shown[0].geometry else { panic!("{:?}", shown[0].geometry) };
        assert!(start.approx_eq(Point::new(0.0, 5.0)) && end.approx_eq(Point::new(10.0, 5.0)));
        // Inverted: the two ends outside.
        assert_eq!(clip_styled(vec![styled(line)], &square, true).len(), 2);
        // A circle crossing the edge keeps its inside arc only.
        let circle = Geometry::Circle { center: Point::new(10.0, 5.0), radius: 2.0 };
        let shown = clip_styled(vec![styled(circle)], &square, false);
        assert!(shown.len() == 1 && (shown[0].geometry.length() - std::f64::consts::PI * 2.0).abs() < 1e-9);
        // A fill under an L-shaped boundary keeps only the L.
        let ell = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 4.0),
            Point::new(4.0, 4.0),
            Point::new(4.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let big = [(-1.0, -1.0), (11.0, -1.0), (11.0, 11.0), (-1.0, 11.0)].map(|(x, y)| Vertex::at(Point::new(x, y)));
        let fill =
            Geometry::Hatch { loops: vec![big.to_vec()], pattern: crate::HatchPattern::Solid, scale: 1.0, angle: 0.0 };
        let area: f64 = clip_styled(vec![styled(fill)], &ell, false)
            .iter()
            .map(|s| match &s.geometry {
                Geometry::Hatch { loops, .. } => {
                    hatch::polygons(loops).iter().map(|r| signed_area(r).abs() / 2.0).sum()
                }
                _ => 0.0,
            })
            .sum();
        assert!((area - 64.0).abs() < 1e-9, "{area}");
        // Text shows by its insertion point.
        let text = |x: f64| {
            styled(Geometry::Text {
                look: Default::default(),
                position: Point::new(x, 5.0),
                height: 1.0,
                rotation: 0.0,
                width: 0.0,
                attach: crate::Attach::BottomLeft,
                content: "A".into(),
            })
        };
        assert_eq!(clip_styled(vec![text(5.0), text(50.0)], &square, false).len(), 1);
    }
}

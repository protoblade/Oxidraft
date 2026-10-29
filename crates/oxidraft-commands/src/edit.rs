//! The geometry edits behind TRIM, EXTEND, OFFSET, FILLET and CHAMFER, as plain
//! functions on geometry so they can be tested without the command flow.

use oxidraft_geom::{
    EPSILON, Point,
    curve::{ArcSeg, Seg, carrier_points},
    offset::{offset_polyline, offset_seg},
};
use oxidraft_model::{Document, EntityId, Geometry, Vertex, snap};

/// Parameter tolerance when comparing cut points.
const TOL: f64 = 1e-7;

/// What TRIM does to the picked object.
#[derive(Debug, PartialEq)]
pub(crate) enum Trimmed {
    /// Nothing bounds the picked part, so the whole object goes (Quick mode).
    Erase,
    Replace(Vec<Geometry>),
}

/// Trims the part of `g` around parameter `pick`, bounded by the nearest cut
/// parameters on either side.
pub(crate) fn trim(g: &Geometry, cuts: &[f64], pick: f64) -> Result<Trimmed, String> {
    let n = g.param_count();
    let mut cuts: Vec<f64> = if g.is_closed() {
        cuts.iter().map(|c| c.rem_euclid(n)).collect()
    } else {
        cuts.iter().copied().filter(|&c| c > TOL && c < n - TOL).collect()
    };
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() <= TOL);
    if g.is_closed() && cuts.len() > 1 && cuts[cuts.len() - 1] - cuts[0] >= n - TOL {
        cuts.pop();
    }
    let below = cuts.iter().rev().find(|&&c| c < pick).copied();
    let above = cuts.iter().find(|&&c| c > pick).copied();
    if g.is_closed() {
        return match cuts.len() {
            0 => Ok(Trimmed::Erase),
            1 => Err("A closed object needs two cutting edges to be trimmed.".into()),
            _ => {
                let lo = below.unwrap_or(cuts[cuts.len() - 1]);
                let hi = above.unwrap_or(cuts[0]);
                Ok(Trimmed::Replace(g.sub(hi, lo).into_iter().collect()))
            }
        };
    }
    if below.is_none() && above.is_none() {
        return Ok(Trimmed::Erase);
    }
    let mut pieces = Vec::new();
    if let Some(lo) = below {
        pieces.extend(g.sub(0.0, lo));
    }
    if let Some(hi) = above {
        pieces.extend(g.sub(hi, n));
    }
    Ok(Trimmed::Replace(pieces))
}

/// Extends the end of `g` nearest parameter `pick` to the first boundary it meets.
pub(crate) fn extend(g: &Geometry, boundaries: &[Seg], pick: f64) -> Result<Geometry, String> {
    if g.is_closed() {
        return Err("Closed objects cannot be extended.".into());
    }
    if matches!(g, Geometry::Ellipse(_) | Geometry::Spline(_)) || g.is_infinite() {
        return Err(format!("{} objects can't be extended.", g.name()));
    }
    let mut segs = g.segments();
    let n = segs.len();
    let at_end = pick >= n as f64 / 2.0;
    let idx = if at_end { n - 1 } else { 0 };
    // Work on the end segment oriented outward, so "past the end" is t > 1.
    let seg = if at_end { segs[idx] } else { segs[idx].reversed() };
    let best = boundaries
        .iter()
        .flat_map(|b| seg.extended_intersections(b))
        .filter(|&t| t > 1.0 + TOL)
        .min_by(f64::total_cmp)
        .ok_or("Object does not intersect an edge.")?;
    let extended = seg.sub(0.0, best);
    segs[idx] = if at_end { extended } else { extended.reversed() };
    Ok(match g {
        Geometry::Polyline { .. } => Geometry::polyline_from_segs(&segs, false),
        _ => Geometry::from_seg(&segs[0]),
    })
}

/// The curves of every other shown object (block contents included), which
/// act as cutting or boundary edges.
fn visible_others(doc: &Document, id: EntityId) -> Vec<Geometry> {
    doc.entities().filter(|e| e.id != id && doc.is_shown(&e.layer)).flat_map(|e| doc.expanded(e)).collect()
}

/// Only curves can be trimmed or extended.
fn check_trimmable(g: &Geometry) -> Result<(), String> {
    match g {
        Geometry::Insert { .. } => Err("Block references can't be trimmed or extended; explode them first.".into()),
        _ if g.segments().is_empty() => Err(format!("{} objects can't be trimmed or extended.", g.name())),
        _ => Ok(()),
    }
}

/// TRIM in Quick mode: every other object is a cutting edge. Returns the
/// number of document changes (for the command's Undo option).
pub(crate) fn trim_at(
    doc: &mut Document,
    at: Point,
    tolerance: f64,
    chosen: Option<EntityId>,
) -> Result<usize, String> {
    let Some(id) = chosen.or_else(|| snap::pick(doc, at, tolerance)) else { return Ok(0) };
    let Some(e) = doc.entity(id).cloned() else { return Ok(0) };
    check_trimmable(&e.geometry)?;
    let cuts: Vec<f64> = visible_others(doc, id).iter().flat_map(|o| e.geometry.intersection_params(o)).collect();
    match trim(&e.geometry, &cuts, e.geometry.param_of(at))? {
        Trimmed::Erase => {
            doc.remove(id);
            Ok(1)
        }
        Trimmed::Replace(pieces) => {
            let mut pieces = pieces.into_iter();
            let Some(first) = pieces.next() else {
                doc.remove(id);
                return Ok(1);
            };
            doc.modify(id, first);
            let mut changes = 1;
            for piece in pieces {
                doc.add_like(piece, &e);
                changes += 1;
            }
            Ok(changes)
        }
    }
}

/// EXTEND in Quick mode: every other object is a boundary edge.
pub(crate) fn extend_at(
    doc: &mut Document,
    at: Point,
    tolerance: f64,
    chosen: Option<EntityId>,
) -> Result<usize, String> {
    let Some(id) = chosen.or_else(|| snap::pick(doc, at, tolerance)) else { return Ok(0) };
    let Some(g) = doc.entity(id).map(|e| e.geometry.clone()) else { return Ok(0) };
    check_trimmable(&g)?;
    let boundaries: Vec<Seg> = visible_others(doc, id).iter().flat_map(Geometry::segments).collect();
    let extended = extend(&g, &boundaries, g.param_of(at))?;
    doc.modify(id, extended);
    Ok(1)
}

/// The offsets of `g` by `distance` on its left and on its right.
pub(crate) fn offsets(g: &Geometry, distance: f64) -> [Vec<Geometry>; 2] {
    [distance, -distance].map(|d| match g {
        Geometry::Line { .. } | Geometry::Arc { .. } => {
            g.segments().iter().filter_map(|s| offset_seg(s, d)).map(|s| Geometry::from_seg(&s)).collect()
        }
        // Left of a counterclockwise circle is its inside.
        Geometry::Circle { center, radius } => {
            let r = radius - d;
            if r > EPSILON { vec![Geometry::Circle { center: *center, radius: r }] } else { Vec::new() }
        }
        // A constant width carries over to the copies.
        Geometry::Polyline { vertices, closed } => {
            let verts: Vec<(Point, f64)> = vertices.iter().map(|v| (v.point, v.bulge)).collect();
            let width = oxidraft_model::constant_width(vertices).unwrap_or(0.0);
            offset_polyline(&verts, *closed, d)
                .into_iter()
                .map(|(vs, closed)| {
                    let vertices = vs.into_iter().map(|(p, b)| Vertex::new(p, b).with_widths(width, width)).collect();
                    Geometry::Polyline { vertices, closed }
                })
                .collect()
        }
        // Left of a counterclockwise ellipse is its inside; the copies are
        // polylines of arcs, as close to the true offset curve as the
        // ellipse's own arcs are to it.
        Geometry::Ellipse(_) | Geometry::Spline(_) => {
            let segs = g.segments();
            let verts: Vec<(Point, f64)> = segs
                .iter()
                .map(|s| (s.start(), s.bulge()))
                .chain((!g.is_closed()).then(|| segs.last().map(|s| (s.end(), 0.0))).flatten())
                .collect();
            offset_polyline(&verts, g.is_closed(), d)
                .into_iter()
                .map(|(vs, closed)| Geometry::Polyline {
                    vertices: vs.into_iter().map(|(p, b)| Vertex::new(p, b)).collect(),
                    closed,
                })
                .collect()
        }
        Geometry::Xline { direction, .. } | Geometry::Ray { direction, .. } => {
            let len = direction.length().max(f64::MIN_POSITIVE);
            vec![g.translated(Point::new(-direction.y, direction.x) * (d / len))]
        }
        _ => Vec::new(),
    })
}

/// The offset of `g` on the same side as `side`.
pub(crate) fn offset_toward(g: &Geometry, distance: f64, side: Point) -> Vec<Geometry> {
    let near = |set: &Vec<Geometry>| set.iter().map(|o| o.distance_to(side)).fold(f64::INFINITY, f64::min);
    let [left, right] = offsets(g, distance);
    if near(&left) <= near(&right) { left } else { right }
}

/// The result of filleting two lines.
#[derive(Debug, PartialEq)]
pub(crate) struct Filleted {
    pub first: Geometry,
    pub second: Geometry,
    pub arc: Option<Geometry>,
}

/// The corner where two lines meet (extended if needed), with each line's
/// direction away from the corner toward the part to keep and its far end.
struct Corner {
    x: Point,
    u1: Point,
    far1: Point,
    u2: Point,
    far2: Point,
    /// The angle between the kept parts, in radians.
    theta: f64,
}

fn corner(a: (Point, Point), pick_a: Point, b: (Point, Point), pick_b: Point) -> Result<Corner, String> {
    let (sa, sb) = (Seg::Line(oxidraft_geom::Segment::new(a.0, a.1)), Seg::Line(oxidraft_geom::Segment::new(b.0, b.1)));
    let x = *carrier_points(&sa, &sb).first().ok_or("Lines are parallel.")?;
    // Unit direction from the corner toward the part of each line to keep.
    let keep_dir = |line: (Point, Point), seg: &Seg, pick: Point| -> Option<(Point, Point)> {
        let on = seg.point_at(seg.param_of(pick));
        let toward = if on.distance(x) > EPSILON {
            on
        } else if line.0.distance(x) >= line.1.distance(x) {
            line.0
        } else {
            line.1
        };
        let d = toward - x;
        let u = d * (1.0 / d.length().max(EPSILON));
        let far = if (line.0 - x).dot(u) >= (line.1 - x).dot(u) { line.0 } else { line.1 };
        (d.length() > EPSILON).then_some((u, far))
    };
    let (u1, far1) = keep_dir(a, &sa, pick_a).ok_or("Pick each line away from the corner.")?;
    let (u2, far2) = keep_dir(b, &sb, pick_b).ok_or("Pick each line away from the corner.")?;
    let theta = u1.dot(u2).clamp(-1.0, 1.0).acos();
    if theta < 1e-9 || (std::f64::consts::PI - theta) < 1e-9 {
        return Err("Lines are parallel.".into());
    }
    Ok(Corner { x, u1, far1, u2, far2, theta })
}

/// The line from `far` to `end`, keeping the original line's direction.
fn rebuild(line: (Point, Point), far: Point, end: Point) -> Geometry {
    if line.0.approx_eq(far) { Geometry::Line { start: far, end } } else { Geometry::Line { start: end, end: far } }
}

/// Fillets two lines with radius `r` (0 = sharp corner), keeping the parts
/// on the side of each pick point.
pub(crate) fn fillet_lines(
    a: (Point, Point),
    pick_a: Point,
    b: (Point, Point),
    pick_b: Point,
    r: f64,
) -> Result<Filleted, String> {
    let Corner { x, u1, far1, u2, far2, theta } = corner(a, pick_a, b, pick_b)?;
    if r <= EPSILON {
        return Ok(Filleted { first: rebuild(a, far1, x), second: rebuild(b, far2, x), arc: None });
    }
    let d = r / (theta / 2.0).tan();
    if d > (far1 - x).dot(u1) + EPSILON || d > (far2 - x).dot(u2) + EPSILON {
        return Err("Radius is too large.".into());
    }
    let (t1, t2) = (x + u1 * d, x + u2 * d);
    let bisector = (u1 + u2) * (1.0 / (u1 + u2).length());
    let center = x + bisector * (r / (theta / 2.0).sin());
    let mid = center + (x - center) * (r / center.distance(x));
    let arc = ArcSeg::through(t1, mid, t2).ok_or("Radius is too large.")?;
    Ok(Filleted { first: rebuild(a, far1, t1), second: rebuild(b, far2, t2), arc: Some(Geometry::arc(&arc)) })
}

/// Chamfers two lines: cuts `d1` back from the corner along the first and
/// `d2` along the second, joined by a new line (none when both are 0).
pub(crate) fn chamfer_lines(
    a: (Point, Point),
    pick_a: Point,
    b: (Point, Point),
    pick_b: Point,
    d1: f64,
    d2: f64,
) -> Result<Filleted, String> {
    let Corner { x, u1, far1, u2, far2, .. } = corner(a, pick_a, b, pick_b)?;
    if d1 > (far1 - x).dot(u1) + EPSILON || d2 > (far2 - x).dot(u2) + EPSILON {
        return Err("Distance is too large.".into());
    }
    let (t1, t2) = (x + u1 * d1, x + u2 * d2);
    let cut = (!t1.approx_eq(t2)).then_some(Geometry::Line { start: t1, end: t2 });
    Ok(Filleted { first: rebuild(a, far1, t1), second: rebuild(b, far2, t2), arc: cut })
}

/// Chamfers every corner between two straight segments of a polyline,
/// cutting `d1` back along the incoming segment and `d2` along the outgoing
/// one. Returns the new vertices, the corners chamfered and those skipped
/// because their segments are too short.
pub(crate) fn chamfer_polyline(vertices: &[Vertex], closed: bool, d1: f64, d2: f64) -> (Vec<Vertex>, usize, usize) {
    let n = vertices.len();
    let mut out = Vec::with_capacity(n * 2);
    let (mut done, mut short) = (0, 0);
    for i in 0..n {
        let has_corner = closed || (i > 0 && i + 1 < n);
        let (prev, next, v) = (vertices[(i + n - 1) % n], vertices[(i + 1) % n], vertices[i]);
        if !has_corner || (d1 <= EPSILON && d2 <= EPSILON) || prev.bulge.abs() > 1e-12 || v.bulge.abs() > 1e-12 {
            out.push(v);
            continue;
        }
        let x = v.point;
        let (l1, l2) = (prev.point.distance(x), next.point.distance(x));
        // Each segment may be shared with another chamfered corner.
        if l1 <= EPSILON || l2 <= EPSILON || d1 > l1 / 2.0 + EPSILON || d2 > l2 / 2.0 + EPSILON {
            short += usize::from(l1 > EPSILON && l2 > EPSILON);
            out.push(v);
            continue;
        }
        out.push(Vertex::at(x + (prev.point - x) * (d1 / l1)));
        out.push(Vertex::at(x + (next.point - x) * (d2 / l2)));
        done += 1;
    }
    (out, done, short)
}

/// Fillets every corner between two straight segments of a polyline.
/// Returns the new vertices, the number of corners filleted and the number
/// skipped because their segments are too short.
pub(crate) fn fillet_polyline(vertices: &[Vertex], closed: bool, r: f64) -> (Vec<Vertex>, usize, usize) {
    let n = vertices.len();
    let mut out = Vec::with_capacity(n * 2);
    let (mut done, mut short) = (0, 0);
    for i in 0..n {
        let has_corner = closed || (i > 0 && i + 1 < n);
        let (prev, next) = (vertices[(i + n - 1) % n], vertices[(i + 1) % n]);
        let v = vertices[i];
        if !has_corner || r <= EPSILON || prev.bulge.abs() > 1e-12 || v.bulge.abs() > 1e-12 {
            out.push(v);
            continue;
        }
        let x = v.point;
        let (l1, l2) = (prev.point.distance(x), next.point.distance(x));
        if l1 <= EPSILON || l2 <= EPSILON {
            out.push(v);
            continue;
        }
        let (u1, u2) = ((prev.point - x) * (1.0 / l1), (next.point - x) * (1.0 / l2));
        let theta = u1.dot(u2).clamp(-1.0, 1.0).acos();
        if theta < 1e-9 || (std::f64::consts::PI - theta) < 1e-9 {
            out.push(v);
            continue;
        }
        let d = r / (theta / 2.0).tan();
        // Each segment may be shared with another filleted corner.
        if d > l1 / 2.0 + EPSILON || d > l2 / 2.0 + EPSILON {
            short += 1;
            out.push(v);
            continue;
        }
        let turn = (-u1).cross(u2).signum();
        let sweep = std::f64::consts::PI - theta;
        out.push(Vertex::new(x + u1 * d, turn * (sweep / 4.0).tan()));
        out.push(Vertex::at(x + u2 * d));
        done += 1;
    }
    (out, done, short)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Geometry {
        Geometry::Line { start: Point::new(ax, ay), end: Point::new(bx, by) }
    }

    #[test]
    fn trim_open_curve_between_cuts() {
        let g = line(0.0, 0.0, 10.0, 0.0);
        assert_eq!(
            trim(&g, &[0.3, 0.6], 0.5),
            Ok(Trimmed::Replace(vec![line(0.0, 0.0, 3.0, 0.0), line(6.0, 0.0, 10.0, 0.0)]))
        );
        assert_eq!(trim(&g, &[0.3], 0.5), Ok(Trimmed::Replace(vec![line(0.0, 0.0, 3.0, 0.0)])));
        assert_eq!(trim(&g, &[0.0, 1.0], 0.5), Ok(Trimmed::Erase), "cuts at the ends don't bound anything");
    }

    #[test]
    fn trim_circle_keeps_the_far_arc() {
        let c = Geometry::Circle { center: Point::ORIGIN, radius: 1.0 };
        // Cuts at 90° and 180°; picking at 135° leaves the arc from 180° round to 90°.
        let Ok(Trimmed::Replace(pieces)) = trim(&c, &[0.25, 0.5], 0.375) else { panic!() };
        let Geometry::Arc { start_angle, end_angle, .. } = pieces[0] else { panic!() };
        assert!((start_angle - 180.0).abs() < 1e-9 && (end_angle - 90.0).abs() < 1e-9);
        assert!(trim(&c, &[0.25], 0.5).is_err());
    }

    #[test]
    fn extend_line_to_boundary_either_end() {
        let g = line(2.0, 0.0, 5.0, 0.0);
        let walls = [
            Seg::Line(oxidraft_geom::Segment::new(Point::new(10.0, -5.0), Point::new(10.0, 5.0))),
            Seg::Line(oxidraft_geom::Segment::new(Point::new(-4.0, -5.0), Point::new(-4.0, 5.0))),
        ];
        assert_eq!(extend(&g, &walls, 0.9), Ok(line(2.0, 0.0, 10.0, 0.0)));
        assert_eq!(extend(&g, &walls, 0.1), Ok(line(-4.0, 0.0, 5.0, 0.0)));
        assert!(extend(&g, &walls[..0], 0.9).is_err());
    }

    #[test]
    fn extend_arc_along_its_circle() {
        let arc = Geometry::Arc { center: Point::ORIGIN, radius: 5.0, start_angle: 0.0, end_angle: 45.0 };
        let wall = [Seg::Line(oxidraft_geom::Segment::new(Point::new(-10.0, 0.0), Point::new(10.0, 0.0)))];
        // The end at 45° runs on counterclockwise until it meets the line again at 180°.
        let Ok(Geometry::Arc { end_angle, .. }) = extend(&arc, &wall, 0.9) else { panic!() };
        assert!((end_angle - 180.0).abs() < 1e-9);
    }

    #[test]
    fn offset_picks_the_side_of_the_point() {
        let g = line(0.0, 0.0, 10.0, 0.0);
        assert_eq!(offset_toward(&g, 2.0, Point::new(5.0, -1.0)), vec![line(0.0, -2.0, 10.0, -2.0)]);
        let c = Geometry::Circle { center: Point::ORIGIN, radius: 5.0 };
        assert_eq!(
            offset_toward(&c, 2.0, Point::new(9.0, 0.0)),
            vec![Geometry::Circle { center: Point::ORIGIN, radius: 7.0 }]
        );
    }

    #[test]
    fn fillet_corner_with_radius_and_zero() {
        let (a, b) = ((Point::new(0.0, 0.0), Point::new(10.0, 0.0)), (Point::new(12.0, 2.0), Point::new(12.0, 10.0)));
        // Radius 0 extends both lines to meet at (12, 0).
        let f = fillet_lines(a, Point::new(5.0, 0.0), b, Point::new(12.0, 6.0), 0.0).unwrap();
        assert_eq!(f.first, line(0.0, 0.0, 12.0, 0.0));
        assert_eq!(f.second, line(12.0, 0.0, 12.0, 10.0));
        assert!(f.arc.is_none());

        let f = fillet_lines(a, Point::new(5.0, 0.0), b, Point::new(12.0, 6.0), 2.0).unwrap();
        let ends = |g: &Geometry| match g {
            Geometry::Line { start, end } => (*start, *end),
            _ => panic!("expected a line"),
        };
        let (s1, e1) = ends(&f.first);
        assert!(s1.approx_eq(Point::new(0.0, 0.0)) && e1.approx_eq(Point::new(10.0, 0.0)));
        let (s2, e2) = ends(&f.second);
        assert!(s2.approx_eq(Point::new(12.0, 2.0)) && e2.approx_eq(Point::new(12.0, 10.0)));
        let Some(Geometry::Arc { center, radius, .. }) = f.arc else { panic!() };
        assert!(center.approx_eq(Point::new(10.0, 2.0)) && (radius - 2.0).abs() < 1e-9);

        assert!(fillet_lines(a, Point::new(5.0, 0.0), b, Point::new(12.0, 6.0), 50.0).is_err());
    }

    #[test]
    fn chamfer_corner_and_rectangle() {
        let (a, b) = ((Point::new(0.0, 0.0), Point::new(10.0, 0.0)), (Point::new(12.0, 2.0), Point::new(12.0, 10.0)));
        let f = chamfer_lines(a, Point::new(5.0, 0.0), b, Point::new(12.0, 6.0), 2.0, 3.0).unwrap();
        assert_eq!(f.first, line(0.0, 0.0, 10.0, 0.0));
        assert_eq!(f.second, line(12.0, 3.0, 12.0, 10.0));
        assert_eq!(f.arc, Some(line(10.0, 0.0, 12.0, 3.0)));
        assert!(chamfer_lines(a, Point::new(5.0, 0.0), b, Point::new(12.0, 6.0), 20.0, 1.0).is_err());

        let sq: Vec<Vertex> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .iter()
            .map(|&(x, y)| Vertex::at(Point::new(x, y)))
            .collect();
        let (out, done, _) = chamfer_polyline(&sq, true, 1.0, 1.0);
        assert_eq!((done, out.len()), (4, 8));
        assert_eq!(out[0].point, Point::new(0.0, 1.0));
        assert_eq!(out[1].point, Point::new(1.0, 0.0));
        let g = Geometry::Polyline { vertices: out, closed: true };
        assert!((g.area().unwrap() - 98.0).abs() < 1e-9);
    }

    #[test]
    fn fillet_rectangle_corners() {
        let sq: Vec<Vertex> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .iter()
            .map(|&(x, y)| Vertex::at(Point::new(x, y)))
            .collect();
        let (out, done, short) = fillet_polyline(&sq, true, 2.0);
        assert_eq!((done, short), (4, 0));
        assert_eq!(out.len(), 8);
        let g = Geometry::Polyline { vertices: out, closed: true };
        // A counterclockwise rectangle turns left: positive bulges.
        assert!(g.segments().iter().filter(|s| matches!(s, Seg::Arc(a) if a.sweep > 0.0)).count() == 4);
        // Segment 2 is the arc at the (10, 0) corner, centered at (8, 2).
        assert!(g.point_at(2.5).approx_eq(Point::new(
            10.0 - 2.0 + 2.0 * (45f64).to_radians().cos(),
            2.0 - 2.0 * (45f64).to_radians().sin()
        )));
        assert_eq!(fillet_polyline(&sq, true, 6.0).2, 4, "radius 6 doesn't fit 10-unit sides twice");
    }
}

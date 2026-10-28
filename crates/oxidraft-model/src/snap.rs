//! Object snaps (OSNAP), entity picking and window/crossing selection.

use oxidraft_geom::{Aabb, Point, curve::Seg};

use crate::{Document, EntityId, Geometry, entity::quadrants};

/// Snap modes, in the priority order used to break ties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    Endpoint,
    Midpoint,
    Center,
    Quadrant,
    Intersection,
    /// A block reference's insertion point.
    Insertion,
}

impl SnapKind {
    /// Tooltip text shown next to the AutoSnap marker.
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Endpoint => "Endpoint",
            SnapKind::Midpoint => "Midpoint",
            SnapKind::Center => "Center",
            SnapKind::Quadrant => "Quadrant",
            SnapKind::Intersection => "Intersection",
            SnapKind::Insertion => "Insertion",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub point: Point,
    pub kind: SnapKind,
}

/// Snap candidates an entity offers on its own.
fn candidates(g: &Geometry, out: &mut Vec<Snap>) {
    if let Geometry::Circle { center, radius } = g {
        out.push(Snap { point: *center, kind: SnapKind::Center });
        out.extend(quadrants(*center, *radius).map(|point| Snap { point, kind: SnapKind::Quadrant }));
        return;
    }
    for seg in g.segments() {
        out.push(Snap { point: seg.start(), kind: SnapKind::Endpoint });
        out.push(Snap { point: seg.end(), kind: SnapKind::Endpoint });
        out.push(Snap { point: seg.point_at(0.5), kind: SnapKind::Midpoint });
        if let Seg::Arc(a) = seg {
            out.push(Snap { point: a.center, kind: SnapKind::Center });
            for (k, point) in quadrants(a.center, a.radius).into_iter().enumerate() {
                if a.param_of_angle(90.0 * k as f64) <= 1.0 {
                    out.push(Snap { point, kind: SnapKind::Quadrant });
                }
            }
        }
    }
}

fn intersections(a: &Geometry, b: &Geometry) -> Vec<Point> {
    let theirs = b.segments();
    a.segments()
        .iter()
        .flat_map(|s| theirs.iter().flat_map(move |o| s.intersections(o).into_iter().map(move |(t, _)| s.point_at(t))))
        .collect()
}

/// Nearest snap point within `tolerance` of `cursor` (both in drawing units).
///
/// Ties go to the earlier [`SnapKind`], so endpoints beat midpoints, etc.
pub fn find_snap(doc: &Document, cursor: Point, tolerance: f64) -> Option<Snap> {
    let mut all = Vec::new();
    // Only curves passing near the cursor can produce an intersection there.
    let mut near: Vec<Geometry> = Vec::new();
    for e in doc.entities().filter(|e| doc.is_shown(&e.layer)) {
        if let Geometry::Insert { placement, .. } = &e.geometry {
            all.push(Snap { point: placement.position, kind: SnapKind::Insertion });
        }
        for g in doc.expanded(e) {
            candidates(&g, &mut all);
            if g.distance_to(cursor) <= tolerance {
                near.push(g);
            }
        }
    }
    for (i, a) in near.iter().enumerate() {
        for b in &near[i + 1..] {
            all.extend(intersections(a, b).into_iter().map(|point| Snap { point, kind: SnapKind::Intersection }));
        }
    }
    all.into_iter()
        .map(|s| (s.point.distance(cursor), s))
        .filter(|(d, _)| *d <= tolerance)
        .min_by(|(da, a), (db, b)| da.total_cmp(db).then_with(|| (a.kind as u8).cmp(&(b.kind as u8))))
        .map(|(_, s)| s)
}

/// Entities selected by a selection window from `first` to `second` corner.
///
/// Dragging left-to-right makes a *window* (objects entirely inside);
/// right-to-left makes a *crossing* window (objects inside or touching).
pub fn window_select(doc: &Document, first: Point, second: Point) -> Vec<EntityId> {
    let b = Aabb::from_points(first, second);
    let crossing = second.x < first.x;
    doc.entities()
        .filter(|e| doc.is_editable(&e.layer))
        .filter(|e| if crossing { doc.entity_touches(e, &b) } else { b.contains_aabb(&doc.entity_bounds(e)) })
        .map(|e| e.id)
        .collect()
}

/// The entity whose curve is closest to `p`, if within `tolerance`.
/// Objects on locked, off or frozen layers can't be picked.
pub fn pick(doc: &Document, p: Point, tolerance: f64) -> Option<EntityId> {
    doc.entities()
        .filter(|e| doc.is_editable(&e.layer))
        .map(|e| (doc.entity_distance(e, p), e.id))
        .filter(|(d, _)| *d <= tolerance)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_nearest_endpoint_or_midpoint() {
        let mut d = Document::new();
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let s = find_snap(&d, Point::new(9.8, 0.3), 1.0).unwrap();
        assert_eq!(s, Snap { point: Point::new(10.0, 0.0), kind: SnapKind::Endpoint });
        let s = find_snap(&d, Point::new(5.2, 0.1), 1.0).unwrap();
        assert_eq!(s.kind, SnapKind::Midpoint);
        assert!(find_snap(&d, Point::new(3.0, 3.0), 1.0).is_none());
    }

    #[test]
    fn snaps_to_intersection_and_quadrant() {
        let mut d = Document::new();
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 10.0) });
        d.add(Geometry::Line { start: Point::new(0.0, 4.0), end: Point::new(10.0, 4.0) });
        let s = find_snap(&d, Point::new(4.2, 4.1), 1.0).unwrap();
        assert_eq!(s.kind, SnapKind::Intersection);
        assert!(s.point.approx_eq(Point::new(4.0, 4.0)));

        let mut d = Document::new();
        d.add(Geometry::Circle { center: Point::ORIGIN, radius: 5.0 });
        assert_eq!(find_snap(&d, Point::new(0.2, 4.9), 1.0).unwrap().kind, SnapKind::Quadrant);
    }

    #[test]
    fn window_vs_crossing() {
        let mut d = Document::new();
        let inside = d.add(Geometry::Line { start: Point::new(1.0, 1.0), end: Point::new(2.0, 2.0) });
        let partly = d.add(Geometry::Line { start: Point::new(5.0, 5.0), end: Point::new(50.0, 5.0) });
        d.add(Geometry::Line { start: Point::new(80.0, 80.0), end: Point::new(90.0, 90.0) });
        let (a, b) = (Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert_eq!(window_select(&d, a, b), vec![inside]);
        assert_eq!(window_select(&d, b, a), vec![inside, partly]);
    }

    #[test]
    fn picks_circle_by_its_outline() {
        let mut d = Document::new();
        let c = d.add(Geometry::Circle { center: Point::ORIGIN, radius: 5.0 });
        assert_eq!(pick(&d, Point::new(5.1, 0.0), 0.5), Some(c));
        assert_eq!(pick(&d, Point::ORIGIN, 0.5), None);
    }
}

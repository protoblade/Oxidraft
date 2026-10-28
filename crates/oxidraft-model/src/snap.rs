//! Object snaps (OSNAP), entity picking and window/crossing selection.

use oxidraft_geom::{Aabb, Point, curve::Seg};

use crate::{Document, Entity, EntityId, Geometry, entity::quadrants};

/// Snap modes, in the priority order used to break ties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    Endpoint,
    Midpoint,
    Center,
    /// A point object.
    Node,
    Quadrant,
    Intersection,
    /// A block reference's insertion point.
    Insertion,
    /// The foot of the perpendicular from the point you're drawing from.
    Perpendicular,
    /// Where a line from the point you're drawing from touches a circle or arc.
    Tangent,
    /// The closest point on the object; only when nothing else snaps.
    Nearest,
}

impl SnapKind {
    pub const ALL: [SnapKind; 10] = [
        SnapKind::Endpoint,
        SnapKind::Midpoint,
        SnapKind::Center,
        SnapKind::Node,
        SnapKind::Quadrant,
        SnapKind::Intersection,
        SnapKind::Insertion,
        SnapKind::Perpendicular,
        SnapKind::Tangent,
        SnapKind::Nearest,
    ];

    /// The running snaps that are on in a new session.
    pub const DEFAULT_RUNNING: [SnapKind; 7] = [
        SnapKind::Endpoint,
        SnapKind::Midpoint,
        SnapKind::Center,
        SnapKind::Node,
        SnapKind::Quadrant,
        SnapKind::Intersection,
        SnapKind::Insertion,
    ];

    /// Tooltip text shown next to the AutoSnap marker.
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Endpoint => "Endpoint",
            SnapKind::Midpoint => "Midpoint",
            SnapKind::Center => "Center",
            SnapKind::Node => "Node",
            SnapKind::Quadrant => "Quadrant",
            SnapKind::Intersection => "Intersection",
            SnapKind::Insertion => "Insertion",
            SnapKind::Perpendicular => "Perpendicular",
            SnapKind::Tangent => "Tangent",
            SnapKind::Nearest => "Nearest",
        }
    }

    /// Snaps found on the object under the cursor, however far the point is
    /// from it: they depend on where you're drawing from, or on the cursor.
    fn on_touched_object(self) -> bool {
        matches!(self, SnapKind::Perpendicular | SnapKind::Tangent | SnapKind::Nearest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub point: Point,
    pub kind: SnapKind,
}

/// The point halfway along an open curve.
fn midway(g: &Geometry) -> Option<Point> {
    let segs = g.segments();
    let mut left = segs.iter().map(Seg::length).sum::<f64>() / 2.0;
    for seg in &segs {
        let len = seg.length();
        if left <= len {
            return Some(seg.point_at(if len > 0.0 { left / len } else { 0.0 }));
        }
        left -= len;
    }
    None
}

/// Snap candidates an entity offers on its own.
fn candidates(g: &Geometry, out: &mut Vec<Snap>) {
    let mut add = |point: Point, kind: SnapKind| out.push(Snap { point, kind });
    match g {
        Geometry::Point { position } => return add(*position, SnapKind::Node),
        // Curves made of many short arcs snap like the curve, not its pieces.
        Geometry::Ellipse(e) => {
            add(e.center, SnapKind::Center);
            for (k, v) in [e.major, e.minor(), -e.major, -e.minor()].into_iter().enumerate() {
                if e.covers(90.0 * k as f64) {
                    add(e.center + v, SnapKind::Quadrant);
                }
            }
            if !e.is_full() {
                add(e.start_point(), SnapKind::Endpoint);
                add(e.end_point(), SnapKind::Endpoint);
                midway(g).into_iter().for_each(|p| add(p, SnapKind::Midpoint));
            }
            return;
        }
        Geometry::Spline(s) => {
            if !s.is_closed() {
                add(s.start_point(), SnapKind::Endpoint);
                add(s.end_point(), SnapKind::Endpoint);
                midway(g).into_iter().for_each(|p| add(p, SnapKind::Midpoint));
            }
            return;
        }
        // A construction line's midpoint is the point it was drawn through.
        Geometry::Xline { base, .. } => return add(*base, SnapKind::Midpoint),
        Geometry::Ray { base, .. } => return add(*base, SnapKind::Endpoint),
        _ => {}
    }
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

/// Perpendicular, tangent and nearest points on a curve: the feet of
/// perpendiculars and the tangent points from `from`, and the point closest
/// to `cursor`.
fn touched_candidates(g: &Geometry, cursor: Point, from: Option<Point>, out: &mut Vec<Snap>) {
    for seg in g.segments() {
        out.push(Snap { point: seg.point_at(seg.param_of(cursor)), kind: SnapKind::Nearest });
        let Some(from) = from else { continue };
        let mut perpendicular = |point: Point| {
            // From a point on the curve itself, the "foot" is that point.
            if !point.approx_eq(from) {
                out.push(Snap { point, kind: SnapKind::Perpendicular });
            }
        };
        match seg {
            Seg::Line(l) => {
                let d = l.end - l.start;
                let t = (from - l.start).dot(d) / d.dot(d).max(f64::MIN_POSITIVE);
                if (0.0..=1.0).contains(&t) {
                    perpendicular(l.start.lerp(l.end, t));
                }
            }
            Seg::Arc(a) => {
                let dist = a.center.distance(from);
                if dist <= 1e-9 {
                    continue;
                }
                let toward = a.center.angle_to(from);
                for angle in [toward, toward + 180.0] {
                    if a.param_of_angle(angle) <= 1.0 {
                        perpendicular(a.center.polar(a.radius, angle));
                    }
                }
                if dist > a.radius {
                    let spread = (a.radius / dist).acos().to_degrees();
                    for angle in [toward - spread, toward + spread] {
                        if a.param_of_angle(angle) <= 1.0 {
                            out.push(Snap { point: a.center.polar(a.radius, angle), kind: SnapKind::Tangent });
                        }
                    }
                }
            }
        }
    }
}

/// Snap candidates within `tolerance` of `cursor`, among `kinds`.
fn collect(doc: &Document, cursor: Point, tolerance: f64, kinds: &[SnapKind], from: Option<Point>) -> Vec<Snap> {
    let mut all = Vec::new();
    // Only curves passing near the cursor can produce an intersection there.
    let mut near: Vec<Geometry> = Vec::new();
    for e in doc.entities().filter(|e| doc.is_shown(&e.layer)) {
        match &e.geometry {
            Geometry::Insert { placement, .. } => {
                all.push(Snap { point: placement.position, kind: SnapKind::Insertion })
            }
            Geometry::Text { position, .. } => all.push(Snap { point: *position, kind: SnapKind::Insertion }),
            _ => {}
        }
        for g in doc.expanded(e) {
            candidates(&g, &mut all);
            if g.distance_to(cursor) <= tolerance {
                near.push(g);
            }
        }
    }
    all.retain(|s| s.point.distance(cursor) <= tolerance);
    for (i, a) in near.iter().enumerate() {
        for b in &near[i + 1..] {
            all.extend(
                intersections(a, b)
                    .into_iter()
                    .filter(|p| p.distance(cursor) <= tolerance)
                    .map(|point| Snap { point, kind: SnapKind::Intersection }),
            );
        }
    }
    if kinds.iter().any(|k| k.on_touched_object()) {
        for g in &near {
            touched_candidates(g, cursor, from, &mut all);
        }
    }
    all.retain(|s| kinds.contains(&s.kind));
    all
}

/// The running object snap at `cursor` (both in drawing units), among the
/// `kinds` turned on. `from` is the point the command is drawing from, for
/// Perpendicular and Tangent.
///
/// Snaps on the object under the cursor (Perpendicular, Tangent) count from
/// anywhere along it; the rest must lie within `tolerance`. The closest
/// wins, ties going to the earlier [`SnapKind`]; Nearest only when nothing
/// else snaps.
pub fn find_snap(
    doc: &Document,
    cursor: Point,
    tolerance: f64,
    kinds: &[SnapKind],
    from: Option<Point>,
) -> Option<Snap> {
    let (nearest, others): (Vec<Snap>, Vec<Snap>) =
        collect(doc, cursor, tolerance, kinds, from).into_iter().partition(|s| s.kind == SnapKind::Nearest);
    let closest = |snaps: Vec<Snap>| {
        snaps
            .into_iter()
            .map(|s| (s.point.distance(cursor), s))
            .min_by(|(da, a), (db, b)| da.total_cmp(db).then_with(|| (a.kind as u8).cmp(&(b.kind as u8))))
            .map(|(_, s)| s)
    };
    closest(others).or_else(|| closest(nearest))
}

/// A one-shot snap override (Shift+right-click menu in AutoCAD): the `kind`
/// point of whichever object the aperture touches, however far that point
/// is from the cursor — the midpoint of the line under the cursor, say.
pub fn find_override_snap(
    doc: &Document,
    cursor: Point,
    tolerance: f64,
    kind: SnapKind,
    from: Option<Point>,
) -> Option<Snap> {
    let mut all = Vec::new();
    let mut near: Vec<Geometry> = Vec::new();
    for e in doc.entities().filter(|e| doc.is_shown(&e.layer)) {
        let touched = doc.entity_distance(e, cursor) <= tolerance;
        if touched {
            match &e.geometry {
                Geometry::Insert { placement, .. } => {
                    all.push(Snap { point: placement.position, kind: SnapKind::Insertion })
                }
                Geometry::Text { position, .. } => all.push(Snap { point: *position, kind: SnapKind::Insertion }),
                _ => {}
            }
        }
        for g in doc.expanded(e).into_iter().filter(|g| g.distance_to(cursor) <= tolerance) {
            candidates(&g, &mut all);
            touched_candidates(&g, cursor, from, &mut all);
            near.push(g);
        }
    }
    for (i, a) in near.iter().enumerate() {
        for b in &near[i + 1..] {
            all.extend(intersections(a, b).into_iter().map(|point| Snap { point, kind: SnapKind::Intersection }));
        }
    }
    all.into_iter()
        .filter(|s| s.kind == kind)
        .min_by(|a, b| a.point.distance(cursor).total_cmp(&b.point.distance(cursor)))
}

/// Entities selected by a selection window from `first` to `second` corner.
///
/// Dragging left-to-right makes a *window* (objects entirely inside);
/// right-to-left makes a *crossing* window (objects inside or touching).
pub fn window_select(doc: &Document, first: Point, second: Point) -> Vec<EntityId> {
    let b = Aabb::from_points(first, second);
    let crossing = second.x < first.x;
    doc.entities()
        .filter(|e| doc.is_selectable(e))
        .filter(|e| if crossing { doc.entity_touches(e, &b) } else { b.contains_aabb(&doc.entity_bounds(e)) })
        .map(|e| e.id)
        .collect()
}

/// The entity whose curve is closest to `p`, if within `tolerance`.
/// Objects on locked, off or frozen layers can't be picked, nor, while a
/// reference is edited in place, objects outside its working set.
pub fn pick(doc: &Document, p: Point, tolerance: f64) -> Option<EntityId> {
    pick_where(doc, p, tolerance, |e| doc.is_selectable(e))
}

/// Like [`pick`], but objects on locked layers count too (LAYULK).
pub fn pick_locked(doc: &Document, p: Point, tolerance: f64) -> Option<EntityId> {
    let editing = |e: &Entity| doc.refedit().is_none_or(|r| r.in_working_set(e.id));
    pick_where(doc, p, tolerance, |e| doc.is_shown(&e.layer) && editing(e))
}

/// Like [`pick`], among the objects `ok` allows.
pub fn pick_where(doc: &Document, p: Point, tolerance: f64, ok: impl Fn(&Entity) -> bool) -> Option<EntityId> {
    let editable = || doc.entities().filter(|e| ok(e));
    editable()
        .map(|e| (doc.entity_distance(e, p), e.id))
        .filter(|(d, _)| *d <= tolerance)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
        .or_else(|| {
            // Clicking inside a hatched area picks it; the one drawn on top.
            doc.entities_in_draw_order()
                .into_iter()
                .filter(|e| ok(e))
                .rfind(|e| matches!(e.geometry, Geometry::Hatch { .. }) && e.geometry.contains(p))
                .map(|e| e.id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: [SnapKind; 7] = SnapKind::DEFAULT_RUNNING;

    #[test]
    fn snaps_to_nearest_endpoint_or_midpoint() {
        let mut d = Document::new();
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let s = find_snap(&d, Point::new(9.8, 0.3), 1.0, &RUNNING, None).unwrap();
        assert_eq!(s, Snap { point: Point::new(10.0, 0.0), kind: SnapKind::Endpoint });
        let s = find_snap(&d, Point::new(5.2, 0.1), 1.0, &RUNNING, None).unwrap();
        assert_eq!(s.kind, SnapKind::Midpoint);
        assert!(find_snap(&d, Point::new(3.0, 3.0), 1.0, &RUNNING, None).is_none());
        // An override takes the touched line's midpoint, even from near its end.
        let s = find_override_snap(&d, Point::new(9.8, 0.2), 0.5, SnapKind::Midpoint, None).unwrap();
        assert_eq!(s.point, Point::new(5.0, 0.0));
        assert!(
            find_override_snap(&d, Point::new(9.8, 3.0), 0.5, SnapKind::Midpoint, None).is_none(),
            "nothing touched"
        );
    }

    #[test]
    fn snaps_to_intersection_and_quadrant() {
        let mut d = Document::new();
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 10.0) });
        d.add(Geometry::Line { start: Point::new(0.0, 4.0), end: Point::new(10.0, 4.0) });
        let s = find_snap(&d, Point::new(4.2, 4.1), 1.0, &RUNNING, None).unwrap();
        assert_eq!(s.kind, SnapKind::Intersection);
        assert!(s.point.approx_eq(Point::new(4.0, 4.0)));

        let mut d = Document::new();
        d.add(Geometry::Circle { center: Point::ORIGIN, radius: 5.0 });
        assert_eq!(find_snap(&d, Point::new(0.2, 4.9), 1.0, &RUNNING, None).unwrap().kind, SnapKind::Quadrant);
    }

    #[test]
    fn perpendicular_tangent_and_nearest() {
        let mut d = Document::new();
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 0.0) });
        d.add(Geometry::Circle { center: Point::new(50.0, 50.0), radius: 10.0 });
        let all = SnapKind::ALL;
        // Drawing from (30,20): over the line far from its ends, the foot is (30,0).
        let s = find_snap(&d, Point::new(37.0, 0.2), 0.5, &all, Some(Point::new(30.0, 20.0))).unwrap();
        assert_eq!((s.kind, s.point), (SnapKind::Perpendicular, Point::new(30.0, 0.0)));
        // Nothing but Nearest there without a point to draw from.
        let s = find_snap(&d, Point::new(37.0, 0.2), 0.5, &all, None).unwrap();
        assert_eq!((s.kind, s.point), (SnapKind::Nearest, Point::new(37.0, 0.0)));
        assert!(find_snap(&d, Point::new(37.0, 0.2), 0.5, &RUNNING, None).is_none(), "not running by default");

        // From (50,30), 20 below the center of the r=10 circle, the tangents touch at -30° and -150°.
        let from = Point::new(50.0, 30.0);
        let s = find_override_snap(&d, Point::new(58.0, 44.0), 0.5, SnapKind::Tangent, Some(from)).unwrap();
        assert!((s.point - from).dot(s.point - Point::new(50.0, 50.0)).abs() < 1e-9, "radius ⟂ tangent line");
        assert!(s.point.x > 50.0);
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

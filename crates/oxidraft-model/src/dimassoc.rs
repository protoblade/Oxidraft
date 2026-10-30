//! Associative dimensions: each of a dimension's definition points can
//! follow a point on an object (an endpoint, midpoint, center, quadrant,
//! node, insertion point, or a point on a circle), so the dimension
//! changes with the objects it measures (DIMASSOC 2, DIMREASSOCIATE,
//! DIMDISASSOCIATE, DIMREGEN).

use std::collections::HashSet;

use oxidraft_geom::Point;

use crate::document::Change;
use crate::{Document, Entity, EntityId, Geometry};

/// What point on its object a definition point follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssocKind {
    Endpoint,
    Midpoint,
    Center,
    Quadrant,
    Node,
    Insertion,
    /// A point on a circle or arc, at the angle the reference keeps.
    OnCircle,
}

/// One definition point's object and point on it: `index` picks the
/// vertex, segment or quadrant; `angle` is the angle on a circle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AssocRef {
    pub target: EntityId,
    pub kind: AssocKind,
    pub index: usize,
    pub angle: f64,
}

/// A dimension's associations, one for each definition point (or none).
pub type DimAssoc = Vec<Option<AssocRef>>;

impl Geometry {
    /// A dimension's definition points: a linear one's two extension line
    /// origins, a radial one's center and point on the curve, an angular
    /// one's vertex and two points, an ordinate one's feature.
    pub fn dim_def_points(&self) -> Vec<Point> {
        match self {
            Geometry::Dimension { p1, p2, .. } => vec![*p1, *p2],
            Geometry::RadialDimension { center, point, .. } => vec![*center, *point],
            Geometry::AngularDimension { vertex, p1, p2, .. } => vec![*vertex, *p1, *p2],
            Geometry::OrdinateDimension { feature, .. } => vec![*feature],
            _ => Vec::new(),
        }
    }

    /// The dimension with new definition points; what goes with them
    /// (its line, its text) moves along when they all moved alike.
    pub fn with_dim_def_points(&self, points: &[Point]) -> Geometry {
        let old = self.dim_def_points();
        if old.len() != points.len() {
            return self.clone();
        }
        let moves: Vec<Point> = old.iter().zip(points).map(|(a, b)| *b - *a).collect();
        let rigid = moves.windows(2).all(|w| w[0].approx_eq(w[1]));
        let shift = if rigid { moves[0] } else { Point::ORIGIN };
        let mut g = self.clone();
        match &mut g {
            Geometry::Dimension { p1, p2, line, extra, .. } => {
                (*p1, *p2) = (points[0], points[1]);
                *line = *line + shift;
                extra.text_at = extra.text_at.map(|t| t + shift);
            }
            Geometry::RadialDimension { center, point, text_at, .. } => {
                // The text keeps its place relative to the point on the curve.
                *text_at = *text_at + (points[1] - *point);
                (*center, *point) = (points[0], points[1]);
            }
            Geometry::AngularDimension { vertex, p1, p2, arc, .. } => {
                (*vertex, *p1, *p2) = (points[0], points[1], points[2]);
                *arc = *arc + moves[0];
            }
            Geometry::OrdinateDimension { feature, leader_end, .. } => {
                *feature = points[0];
                *leader_end = *leader_end + moves[0];
            }
            _ => {}
        }
        g
    }

    pub(crate) fn is_dimension(&self) -> bool {
        !self.dim_def_points().is_empty()
    }

    /// A curve's center and radius, if it's a circle or an arc.
    fn circle(&self) -> Option<(Point, f64)> {
        match self {
            Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => Some((*center, *radius)),
            _ => None,
        }
    }
}

/// Where `r`'s point is on `g` now.
pub fn assoc_point(g: &Geometry, r: &AssocRef) -> Option<Point> {
    match r.kind {
        AssocKind::Endpoint => {
            let segs = g.segments();
            match segs.get(r.index) {
                Some(s) => Some(s.start()),
                None if r.index == segs.len() => segs.last().map(|s| s.end()),
                None => None,
            }
        }
        AssocKind::Midpoint => g.segments().get(r.index).map(|s| s.point_at(0.5)),
        AssocKind::Center => match g {
            Geometry::Ellipse(e) => Some(e.center),
            _ => g.circle().map(|c| c.0),
        },
        AssocKind::Quadrant => g.circle().map(|(c, radius)| c.polar(radius, 90.0 * r.index as f64)),
        AssocKind::OnCircle => g.circle().map(|(c, radius)| c.polar(radius, r.angle)),
        AssocKind::Node => match g {
            Geometry::Point { position } => Some(*position),
            _ => None,
        },
        AssocKind::Insertion => match g {
            Geometry::Insert { placement, .. } => Some(placement.position),
            Geometry::Text { position, .. } => Some(*position),
            _ => None,
        },
    }
}

/// The ways `g` offers a point to follow: kind and index, with where.
fn candidates(g: &Geometry) -> Vec<(AssocKind, usize, Point)> {
    let mut out = Vec::new();
    match g {
        Geometry::Point { position } => out.push((AssocKind::Node, 0, *position)),
        Geometry::Insert { placement, .. } => out.push((AssocKind::Insertion, 0, placement.position)),
        Geometry::Text { position, .. } => out.push((AssocKind::Insertion, 0, *position)),
        _ => {}
    }
    if let Some((c, radius)) = g.circle() {
        out.push((AssocKind::Center, 0, c));
        if matches!(g, Geometry::Circle { .. }) {
            out.extend((0..4).map(|q| (AssocKind::Quadrant, q, c.polar(radius, 90.0 * q as f64))));
        }
    }
    if let Geometry::Ellipse(e) = g {
        out.push((AssocKind::Center, 0, e.center));
    }
    if matches!(
        g,
        Geometry::Line { .. }
            | Geometry::Arc { .. }
            | Geometry::Polyline { .. }
            | Geometry::Spline(_)
            | Geometry::Ellipse(_)
    ) {
        let segs = g.segments();
        for (i, s) in segs.iter().enumerate() {
            out.push((AssocKind::Endpoint, i, s.start()));
            if !matches!(g, Geometry::Spline(_)) {
                out.push((AssocKind::Midpoint, i, s.point_at(0.5)));
            }
        }
        if let Some(last) = segs.last() {
            out.push((AssocKind::Endpoint, segs.len(), last.end()));
        }
    }
    out
}

/// The reference that keeps `p` on object `e`, if `p` is one of its
/// points (or, with `on_curve`, anywhere on its circle).
pub fn reference_at(e: &Entity, p: Point, on_curve: bool) -> Option<AssocRef> {
    let tolerance = 1e-6 * (1.0 + p.x.abs().max(p.y.abs()));
    if let Some((kind, index, _)) = candidates(&e.geometry).into_iter().find(|c| c.2.distance(p) <= tolerance) {
        return Some(AssocRef { target: e.id, kind, index, angle: 0.0 });
    }
    let (c, radius) = e.geometry.circle().filter(|_| on_curve)?;
    ((c.distance(p) - radius).abs() <= tolerance).then(|| AssocRef {
        target: e.id,
        kind: AssocKind::OnCircle,
        index: 0,
        angle: c.angle_to(p),
    })
}

/// The reference of `kind` on object `e` nearest `p` (as a file gives a
/// kind of snap and its point).
pub fn reference_of_kind(e: &Entity, kind: AssocKind, p: Point) -> Option<AssocRef> {
    if kind == AssocKind::OnCircle {
        let (c, _) = e.geometry.circle()?;
        return Some(AssocRef { target: e.id, kind, index: 0, angle: c.angle_to(p) });
    }
    candidates(&e.geometry)
        .into_iter()
        .filter(|c| c.0 == kind)
        .min_by(|a, b| a.2.distance(p).total_cmp(&b.2.distance(p)))
        .map(|(kind, index, _)| AssocRef { target: e.id, kind, index, angle: 0.0 })
}

impl Document {
    /// A dimension's associations, if it has any.
    pub fn dim_assoc(&self, id: EntityId) -> Option<&DimAssoc> {
        self.dim_assoc.get(&id)
    }

    /// Every associative dimension and its associations.
    pub fn all_dim_assoc(&self) -> impl Iterator<Item = (&EntityId, &DimAssoc)> {
        self.dim_assoc.iter()
    }

    /// Sets (or with `None`, clears) a dimension's associations, as an
    /// undoable change.
    pub fn set_dim_assoc(&mut self, id: EntityId, assoc: Option<DimAssoc>) {
        let assoc = assoc.filter(|a| a.iter().any(Option::is_some));
        let before = self.dim_assoc.get(&id).cloned();
        if before == assoc {
            return;
        }
        match &assoc {
            Some(a) => self.dim_assoc.insert(id, a.clone()),
            None => self.dim_assoc.remove(&id),
        };
        self.record(Change::DimAssoc { id, before, after: assoc });
    }

    /// Associates dimension `id`'s definition points with the objects
    /// whose points they lie on; how many of them.
    pub fn associate_dimension(&mut self, id: EntityId) -> usize {
        let Some(dim) = self.entity(id).cloned() else { return 0 };
        let radial = matches!(dim.geometry, Geometry::RadialDimension { .. });
        let points = dim.geometry.dim_def_points();
        let mut refs: DimAssoc = vec![None; points.len()];
        for (slot, p) in points.iter().enumerate() {
            refs[slot] = self
                .space_in_draw_order(dim.space)
                .into_iter()
                .filter(|e| e.id != id && !e.geometry.is_dimension())
                .find_map(|e| reference_at(e, *p, radial && slot == 1));
        }
        let n = refs.iter().flatten().count();
        self.set_dim_assoc(id, Some(refs));
        n
    }

    /// Brings the dimensions that follow `changed` objects up to date (and
    /// forgets objects that are gone), within the open transaction.
    pub(crate) fn follow_associations(&mut self, changed: &HashSet<EntityId>) {
        let dims: Vec<EntityId> = self
            .dim_assoc
            .iter()
            .filter(|(dim, refs)| changed.contains(dim) || refs.iter().flatten().any(|r| changed.contains(&r.target)))
            .map(|(dim, _)| *dim)
            .collect();
        for dim in dims {
            if self.entity(dim).is_none() {
                self.set_dim_assoc(dim, None);
                continue;
            }
            self.regen_dimension(dim);
        }
    }

    /// Puts dimension `id`'s associated points where their objects are
    /// now; true if it changed.
    pub fn regen_dimension(&mut self, id: EntityId) -> bool {
        let (Some(refs), Some(dim)) = (self.dim_assoc.get(&id).cloned(), self.entity(id).cloned()) else {
            return false;
        };
        let mut points = dim.geometry.dim_def_points();
        let mut kept = refs.clone();
        for (slot, r) in refs.iter().enumerate() {
            let Some(r) = r else { continue };
            match self.entity(r.target).and_then(|t| assoc_point(&t.geometry, r)) {
                Some(p) if slot < points.len() => points[slot] = p,
                _ => kept[slot] = None,
            }
        }
        if kept != refs {
            self.set_dim_assoc(id, Some(kept));
        }
        let g = dim.geometry.with_dim_def_points(&points);
        if g == dim.geometry {
            return false;
        }
        self.update(id, |e| e.geometry = g);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(doc: &mut Document, a: Point, b: Point) -> EntityId {
        doc.add(Geometry::Dimension {
            p1: a,
            p2: b,
            line: Point::new(a.x, a.y + 10.0),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Default::default(),
        })
    }

    #[test]
    fn dimensions_follow_what_they_measure() {
        let mut doc = Document::new();
        let line = doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(40.0, 0.0) });
        let dim = linear(&mut doc, Point::new(0.0, 0.0), Point::new(40.0, 0.0));
        assert_eq!(doc.associate_dimension(dim), 2);
        // Stretch the line: the dimension measures it still.
        doc.begin("STRETCH");
        doc.update(line, |e| e.geometry = Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(55.0, 0.0) });
        doc.commit();
        assert!(
            matches!(doc.entity(dim).unwrap().geometry, Geometry::Dimension { p2, .. } if p2 == Point::new(55.0, 0.0))
        );
        // One undo takes both back.
        doc.undo();
        assert!(
            matches!(doc.entity(dim).unwrap().geometry, Geometry::Dimension { p2, .. } if p2 == Point::new(40.0, 0.0))
        );
        // Moved whole, the dimension's line comes along.
        doc.begin("MOVE");
        doc.update(line, |e| e.geometry = e.geometry.translated(Point::new(0.0, 5.0)));
        doc.commit();
        assert!(
            matches!(doc.entity(dim).unwrap().geometry, Geometry::Dimension { line, .. } if line.approx_eq(Point::new(0.0, 15.0)))
        );
        // Erased: the dimension stays, no longer associated.
        doc.begin("ERASE");
        doc.remove(line);
        doc.commit();
        assert!(doc.dim_assoc(dim).is_none());
    }

    #[test]
    fn radius_dimensions_follow_their_circle() {
        let mut doc = Document::new();
        let circle = doc.add(Geometry::Circle { center: Point::new(0.0, 0.0), radius: 5.0 });
        let dim = doc.add(Geometry::RadialDimension {
            center: Point::new(0.0, 0.0),
            point: Point::new(0.0, 5.0),
            text_at: Point::new(0.0, 8.0),
            diameter: false,
            text: None,
            scale: 1.0,
        });
        assert_eq!(doc.associate_dimension(dim), 2);
        doc.begin("SCALE");
        doc.update(circle, |e| e.geometry = Geometry::Circle { center: Point::new(1.0, 0.0), radius: 10.0 });
        doc.commit();
        let g = doc.entity(dim).unwrap().geometry.clone();
        assert!(
            matches!(g, Geometry::RadialDimension { center, point, .. }
            if center == Point::new(1.0, 0.0) && point.approx_eq(Point::new(1.0, 10.0))),
            "{g:?}"
        );
    }
}

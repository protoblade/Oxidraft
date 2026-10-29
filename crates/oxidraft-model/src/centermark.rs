//! Center marks that follow their circles: a center mark is a group of
//! center lines described "Center mark" (CENTERMARK). When a circle or arc
//! moves or changes size, the marks crossing at its center go with it, as
//! associative center marks do in AutoCAD. CENTERDISASSOCIATE stops one
//! (its description says so), CENTERREASSOCIATE puts one on a circle,
//! CENTERRESET gives one the standard extension again.

use oxidraft_geom::Point;

use crate::{Document, EntityId, Geometry, Group};

/// How a following center mark's group is described.
pub const CENTER_MARK: &str = "Center mark";
/// One that no longer follows.
pub const LOOSE_CENTER_MARK: &str = "Center mark (not associative)";
/// How far center lines reach past what they mark (CENTEREXE).
pub const CENTER_EXTENSION: f64 = 3.5;

/// A circle's or arc's center and radius.
fn round(g: &Geometry) -> Option<(Point, f64)> {
    match g {
        Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => Some((*center, *radius)),
        _ => None,
    }
}

impl Document {
    /// The center mark groups and their lines: (group, [(id, start, end)]).
    fn center_marks(&self, following: bool) -> Vec<(Group, Vec<(EntityId, Point, Point)>)> {
        let want = if following { CENTER_MARK } else { LOOSE_CENTER_MARK };
        self.groups()
            .into_iter()
            .filter(|g| g.description == want)
            .map(|g| {
                let lines = g
                    .members
                    .iter()
                    .filter_map(|id| match self.entity(*id).map(|e| &e.geometry) {
                        Some(Geometry::Line { start, end }) => Some((*id, *start, *end)),
                        _ => None,
                    })
                    .collect();
                (g, lines)
            })
            .collect()
    }

    /// Lays a center mark's lines over the circle at `center`, `radius`,
    /// reaching `extension` past it.
    fn set_mark_lines(&mut self, lines: &[(EntityId, Point, Point)], center: Point, radius: f64, extension: f64) {
        let reach = radius + extension;
        for (id, start, end) in lines {
            let d = *end - *start;
            let u = if d.length() > 1e-12 { d * (1.0 / d.length()) } else { Point::new(1.0, 0.0) };
            let g = Geometry::Line { start: center - u * reach, end: center + u * reach };
            self.modify(*id, g);
        }
    }

    /// Center marks follow circles and arcs changed in the open
    /// transaction (from `before` to now).
    pub(crate) fn follow_center_marks(&mut self, changes: &[(EntityId, Geometry)]) {
        for (id, before) in changes {
            let (Some((old_c, old_r)), Some((new_c, new_r))) =
                (round(before), self.entity(*id).and_then(|e| round(&e.geometry)))
            else {
                continue;
            };
            if old_c.approx_eq(new_c) && (old_r - new_r).abs() < 1e-9 {
                continue;
            }
            for (_, lines) in self.center_marks(true) {
                let crosses = lines.iter().all(|(_, s, e)| s.midpoint(*e).approx_eq(old_c));
                if lines.is_empty() || !crosses {
                    continue;
                }
                let extension = lines[0].1.distance(lines[0].2) / 2.0 - old_r;
                self.set_mark_lines(&lines, new_c, new_r, extension);
            }
        }
    }

    /// CENTERDISASSOCIATE (`on` false) or back to following: the center
    /// mark `member` is part of.
    pub fn set_center_mark_follows(&mut self, member: EntityId, on: bool) -> bool {
        let mut groups = self.groups();
        let Some(g) = groups.iter_mut().find(|g| {
            g.members.contains(&member) && (g.description == CENTER_MARK || g.description == LOOSE_CENTER_MARK)
        }) else {
            return false;
        };
        g.description = if on { CENTER_MARK } else { LOOSE_CENTER_MARK }.into();
        self.set_groups(groups);
        true
    }

    /// CENTERREASSOCIATE: the center mark `member` is part of, onto the
    /// circle or arc `circle`, following it.
    pub fn reassociate_center_mark(&mut self, member: EntityId, circle: EntityId) -> bool {
        let Some((center, radius)) = self.entity(circle).and_then(|e| round(&e.geometry)) else { return false };
        if !self.set_center_mark_follows(member, true) {
            return false;
        }
        let lines = self
            .center_marks(true)
            .into_iter()
            .find(|(g, _)| g.members.contains(&member))
            .map(|(_, l)| l)
            .unwrap_or_default();
        let extension = lines.first().map_or(CENTER_EXTENSION, |(_, s, e)| s.distance(*e) / 2.0 - radius).max(0.0);
        self.set_mark_lines(&lines, center, radius, if extension > 0.0 { extension } else { CENTER_EXTENSION });
        true
    }

    /// CENTERRESET: the center mark `member` is part of, the standard
    /// extension past the circle it's on again.
    pub fn reset_center_mark(&mut self, member: EntityId) -> bool {
        let Some((_, lines)) = self.center_marks(true).into_iter().find(|(g, _)| g.members.contains(&member)) else {
            return false;
        };
        let Some(&(_, s, e)) = lines.first() else { return false };
        let c = s.midpoint(e);
        let circle = self.entities().filter_map(|x| round(&x.geometry)).find(|(center, _)| center.approx_eq(c));
        let Some((center, radius)) = circle else { return false };
        self.set_mark_lines(&lines, center, radius, CENTER_EXTENSION);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_marks_follow_their_circles() {
        let mut d = Document::new();
        d.begin("draw");
        let c = d.add(Geometry::Circle { center: Point::new(10.0, 10.0), radius: 5.0 });
        let h = d.add(Geometry::Line { start: Point::new(1.5, 10.0), end: Point::new(18.5, 10.0) });
        let v = d.add(Geometry::Line { start: Point::new(10.0, 1.5), end: Point::new(10.0, 18.5) });
        d.add_group("", CENTER_MARK, vec![h, v]).unwrap();
        d.commit();
        let line = |d: &Document, id| match d.entity(id).unwrap().geometry {
            Geometry::Line { start, end } => (start, end),
            _ => unreachable!(),
        };
        // Moved and made bigger: the mark goes with it, still 3.5 past it.
        d.begin("move");
        d.modify(c, Geometry::Circle { center: Point::new(30.0, 0.0), radius: 8.0 });
        d.commit();
        let (s, e) = line(&d, h);
        assert!(s.approx_eq(Point::new(18.5, 0.0)) && e.approx_eq(Point::new(41.5, 0.0)), "{s:?} {e:?}");
        // One undo puts both back.
        d.undo();
        assert!(line(&d, v).0.approx_eq(Point::new(10.0, 1.5)));
        // Not associative: it stays.
        d.begin("loose");
        assert!(d.set_center_mark_follows(h, false));
        d.commit();
        d.begin("move");
        d.modify(c, Geometry::Circle { center: Point::new(0.0, 0.0), radius: 5.0 });
        d.commit();
        assert!(line(&d, h).0.approx_eq(Point::new(1.5, 10.0)));
        // Reassociated: onto the circle where it is.
        d.begin("again");
        assert!(d.reassociate_center_mark(h, c));
        d.commit();
        assert!(line(&d, h).0.approx_eq(Point::new(-8.5, 0.0)), "{:?}", line(&d, h));
    }
}

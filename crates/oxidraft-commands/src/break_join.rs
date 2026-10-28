//! BREAK, BREAKATPOINT and JOIN: cut objects apart and put them back together.

use oxidraft_geom::{
    EPSILON, Point, Segment,
    curve::{ArcSeg, Seg},
};
use oxidraft_model::{EntityId, Geometry, snap};

use crate::{Command, Ctx, Input, Prompt, Step};

/// Distance under which two endpoints count as the same point for JOIN.
pub(crate) const JOIN_TOL: f64 = 1e-6;

/// The pieces left after removing the part of `g` between `p1` and `p2`
/// (both projected onto it). Equal points split the object there. On a
/// circle the part removed runs counterclockwise from `p1` to `p2`; on a
/// closed polyline, in the direction of its vertices.
pub(crate) fn break_geometry(g: &Geometry, p1: Point, p2: Point) -> Result<Vec<Geometry>, String> {
    let (s1, s2) = (g.param_of(p1), g.param_of(p2));
    match g {
        Geometry::Circle { .. } if (s1 - s2).abs() < 1e-9 => Err("Arc cannot be full 360 degrees.".into()),
        Geometry::Circle { .. } | Geometry::Polyline { closed: true, .. } => Ok(g.sub(s2, s1).into_iter().collect()),
        Geometry::Line { .. } | Geometry::Arc { .. } | Geometry::Polyline { .. } => {
            let (lo, hi) = (s1.min(s2), s1.max(s2));
            Ok([g.sub(0.0, lo), g.sub(hi, g.param_count())].into_iter().flatten().collect())
        }
        _ => Err(format!("A {} can't be broken.", g.name())),
    }
}

#[derive(Clone, Copy)]
enum Stage {
    Select,
    /// The pick was the first break point.
    Second {
        id: EntityId,
        first: Point,
    },
    /// First point option: the pick only chose the object.
    First {
        id: EntityId,
    },
    SecondAfterFirst {
        id: EntityId,
        first: Point,
    },
}

/// BREAK: removes the part of an object between two points. BREAKATPOINT
/// splits it in two at one point.
pub struct Break {
    stage: Stage,
    at_point: bool,
}

impl Break {
    pub fn two_points() -> Self {
        Self { stage: Stage::Select, at_point: false }
    }

    pub fn at_point() -> Self {
        Self { stage: Stage::Select, at_point: true }
    }

    fn apply(id: EntityId, p1: Point, p2: Point, ctx: &mut Ctx) -> Result<Step, String> {
        let e = ctx.doc.entity(id).cloned().ok_or("Object not found.")?;
        let mut pieces = break_geometry(&e.geometry, p1, p2)?.into_iter();
        match pieces.next() {
            Some(first) => {
                ctx.doc.modify(id, first);
                for rest in pieces {
                    ctx.doc.add_like(rest, &e);
                }
            }
            None => {
                ctx.doc.remove(id);
            }
        }
        Ok(Step::Done)
    }
}

impl Command for Break {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select object"),
            Stage::First { .. } if self.at_point => Prompt::new("Specify break point"),
            Stage::Second { .. } => Prompt::new("Specify second break point").with_keywords(&["First point"]),
            Stage::First { .. } => Prompt::new("Specify first break point"),
            Stage::SecondAfterFirst { .. } => Prompt::new("Specify second break point"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::Select, Input::Pick { at, .. }) => {
                let id = snap::pick(ctx.doc, at, ctx.pick_tolerance).ok_or("No object found.")?;
                let g = &ctx.doc.entity(id).ok_or("No object found.")?.geometry;
                if g.segments().is_empty() {
                    return Err(format!("A {} can't be broken.", g.name()));
                }
                let first = g.point_at(g.param_of(at));
                self.stage = if self.at_point { Stage::First { id } } else { Stage::Second { id, first } };
                Ok(Step::Continue)
            }
            (Stage::Second { id, .. }, Input::Keyword("First point")) => {
                self.stage = Stage::First { id };
                Ok(Step::Continue)
            }
            (Stage::First { id }, Input::Point(p)) if self.at_point => Self::apply(id, p, p, ctx),
            (Stage::First { id }, Input::Point(first)) => {
                self.stage = Stage::SecondAfterFirst { id, first };
                Ok(Step::Continue)
            }
            (Stage::Second { id, first } | Stage::SecondAfterFirst { id, first }, Input::Point(p)) => {
                Self::apply(id, first, p, ctx)
            }
            (_, Input::Enter) => Ok(Step::Done),
            (Stage::Select, _) => Err("Select an object.".into()),
            _ => Err("Point or option keyword required.".into()),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Second { first, .. } | Stage::SecondAfterFirst { first, .. } => Some(first),
            _ => None,
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            Stage::Select => Vec::new(),
            Stage::Second { id, .. } | Stage::First { id } | Stage::SecondAfterFirst { id, .. } => vec![id],
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::Select)
    }
}

/// One curve of a chain being joined: segments in chain order.
pub(crate) fn ends(segs: &[Seg]) -> (Point, Point) {
    (segs[0].start(), segs[segs.len() - 1].end())
}

pub(crate) fn reversed(segs: &[Seg]) -> Vec<Seg> {
    segs.iter().rev().map(Seg::reversed).collect()
}

/// An object that can join a chain, as its segments.
pub(crate) type Piece = (EntityId, Vec<Seg>);

/// Grows `chain` with the unused `pieces` whose ends meet its ends within
/// `fuzz`, bridging any gap with a straight segment (AutoCAD's "Add" join
/// type). Stops once the chain closes. Returns the pieces added.
pub(crate) fn grow_chain(chain: &mut Vec<Seg>, pieces: &[Piece], used: &mut [bool], fuzz: f64) -> Vec<EntityId> {
    let fuzz = fuzz.max(JOIN_TOL);
    let bridge = |a: Point, b: Point| (a.distance(b) > JOIN_TOL).then(|| Seg::Line(Segment::new(a, b)));
    let mut added = Vec::new();
    'grow: loop {
        let (cs, ce) = ends(chain);
        if !added.is_empty() && cs.distance(ce) <= fuzz {
            break;
        }
        for (j, (id, segs)) in pieces.iter().enumerate() {
            if used[j] {
                continue;
            }
            let (ps, pe) = ends(segs);
            if ce.distance(ps) <= fuzz {
                chain.extend(bridge(ce, ps));
                chain.extend(segs.iter().copied());
            } else if ce.distance(pe) <= fuzz {
                chain.extend(bridge(ce, pe));
                chain.extend(reversed(segs));
            } else if cs.distance(pe) <= fuzz {
                let front: Vec<Seg> = segs.iter().copied().chain(bridge(pe, cs)).collect();
                chain.splice(0..0, front);
            } else if cs.distance(ps) <= fuzz {
                let front: Vec<Seg> = reversed(segs).into_iter().chain(bridge(ps, cs)).collect();
                chain.splice(0..0, front);
            } else {
                continue;
            }
            used[j] = true;
            added.push(*id);
            continue 'grow;
        }
        break;
    }
    added
}

/// The single object a chain of end-to-end segments becomes: one line if
/// they're collinear lines, one arc (or circle) if they're arcs of the same
/// circle, otherwise a polyline.
pub(crate) fn joined(segs: &[Seg]) -> Geometry {
    let (start, end) = ends(segs);
    let closed = segs.len() > 1 && start.distance(end) <= JOIN_TOL;
    let unit = |p: Point| p * (1.0 / p.length().max(EPSILON));
    let lines: Option<Vec<Point>> =
        segs.iter().map(|s| if let Seg::Line(l) = s { Some(unit(l.end - l.start)) } else { None }).collect();
    if let Some(dirs) = lines
        && !closed
        && dirs.iter().all(|d| d.cross(dirs[0]).abs() < 1e-9 && d.dot(dirs[0]) > 0.0)
    {
        return Geometry::Line { start, end };
    }
    let arcs: Option<Vec<ArcSeg>> = segs.iter().map(|s| if let Seg::Arc(a) = s { Some(*a) } else { None }).collect();
    if let Some(arcs) = arcs {
        let a = arcs[0];
        let same = arcs.iter().all(|b| {
            b.center.distance(a.center) <= JOIN_TOL
                && (b.radius - a.radius).abs() <= JOIN_TOL
                && b.sweep * a.sweep > 0.0
        });
        if same {
            let sweep: f64 = arcs.iter().map(|b| b.sweep).sum();
            if closed || sweep.abs() >= 360.0 - 1e-9 {
                return Geometry::Circle { center: a.center, radius: a.radius };
            }
            return Geometry::arc(&ArcSeg { sweep, ..a });
        }
    }
    Geometry::polyline_from_segs(segs, closed)
}

/// JOIN: joins lines, arcs and open polylines that meet end to end.
#[derive(Default)]
pub struct Join;

impl Command for Join {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects to join")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(selection) = input else {
            return Err("Select objects, then press Enter.".into());
        };
        let pieces: Vec<Piece> = selection
            .ids
            .iter()
            .filter_map(|&id| {
                let g = &ctx.doc.entity(id)?.geometry;
                let joinable = matches!(g, Geometry::Line { .. } | Geometry::Arc { .. })
                    || matches!(g, Geometry::Polyline { closed: false, .. });
                let segs = g.segments();
                (joinable && !segs.is_empty()).then_some((id, segs))
            })
            .collect();
        let mut used = vec![false; pieces.len()];
        let mut joined_count = 0;
        for i in 0..pieces.len() {
            if used[i] {
                continue;
            }
            used[i] = true;
            let mut chain = pieces[i].1.clone();
            let mut members = vec![pieces[i].0];
            members.extend(grow_chain(&mut chain, &pieces, &mut used, JOIN_TOL));
            if members.len() < 2 {
                continue;
            }
            let result = joined(&chain);
            let message = match &result {
                Geometry::Line { .. } => format!("{} lines joined into 1 line", members.len()),
                Geometry::Arc { .. } => format!("{} arcs joined into 1 arc", members.len()),
                Geometry::Circle { .. } => format!("{} arcs converted to 1 circle", members.len()),
                _ => format!("{} objects converted to 1 polyline", members.len()),
            };
            ctx.messages.push(message);
            ctx.doc.modify(members[0], result);
            for id in &members[1..] {
                ctx.doc.remove(*id);
            }
            joined_count += members.len();
        }
        let discarded = selection.ids.len() - joined_count;
        if joined_count == 0 {
            ctx.messages.push("No objects meet end to end; nothing was joined.".into());
        } else if discarded > 0 {
            let noun = if discarded == 1 { "object" } else { "objects" };
            ctx.messages.push(format!("{discarded} {noun} discarded from the operation"));
        }
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Geometry {
        Geometry::Line { start: Point::new(ax, ay), end: Point::new(bx, by) }
    }

    #[test]
    fn break_lines_circles_and_closed_polylines() {
        let pieces = break_geometry(&line(0.0, 0.0, 10.0, 0.0), Point::new(7.0, 0.3), Point::new(3.0, 0.0)).unwrap();
        assert_eq!(pieces, vec![line(0.0, 0.0, 3.0, 0.0), line(7.0, 0.0, 10.0, 0.0)]);
        // At one point: two pieces meeting there.
        let pieces = break_geometry(&line(0.0, 0.0, 10.0, 0.0), Point::new(4.0, 0.0), Point::new(4.0, 0.0)).unwrap();
        assert_eq!(pieces, vec![line(0.0, 0.0, 4.0, 0.0), line(4.0, 0.0, 10.0, 0.0)]);
        // From an end: one piece.
        assert_eq!(
            break_geometry(&line(0.0, 0.0, 10.0, 0.0), Point::new(-1.0, 0.0), Point::new(6.0, 0.0)).unwrap().len(),
            1
        );

        // A circle loses the part counterclockwise from the first point to the second.
        let c = Geometry::Circle { center: Point::ORIGIN, radius: 1.0 };
        let pieces = break_geometry(&c, Point::new(1.0, 0.0), Point::new(0.0, 1.0)).unwrap();
        let [Geometry::Arc { start_angle, end_angle, .. }] = pieces[..] else { panic!("{pieces:?}") };
        assert!((start_angle - 90.0).abs() < 1e-9 && end_angle.abs() < 1e-9);
        assert!(break_geometry(&c, Point::new(1.0, 0.0), Point::new(1.0, 0.0)).is_err());
    }

    #[test]
    fn join_lines_arcs_and_mixed_chains() {
        let segs = |gs: &[Geometry]| gs.iter().flat_map(Geometry::segments).collect::<Vec<_>>();
        assert_eq!(joined(&segs(&[line(0.0, 0.0, 5.0, 0.0), line(5.0, 0.0, 9.0, 0.0)])), line(0.0, 0.0, 9.0, 0.0));
        let a = Geometry::Arc { center: Point::ORIGIN, radius: 2.0, start_angle: 0.0, end_angle: 90.0 };
        let b = Geometry::Arc { center: Point::ORIGIN, radius: 2.0, start_angle: 90.0, end_angle: 200.0 };
        let Geometry::Arc { start_angle, end_angle, .. } = joined(&segs(&[a, b])) else { panic!() };
        assert!(start_angle.abs() < 1e-9 && (end_angle - 200.0).abs() < 1e-9);
        let Geometry::Polyline { vertices, closed } =
            joined(&segs(&[line(0.0, 0.0, 5.0, 0.0), line(5.0, 0.0, 5.0, 5.0), line(5.0, 5.0, 0.0, 0.0)]))
        else {
            panic!()
        };
        assert!(closed && vertices.len() == 3);
    }
}

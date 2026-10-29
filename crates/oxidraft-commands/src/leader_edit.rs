//! MLEADEREDIT and MLEADERALIGN: arrowheads added to a multileader or
//! taken off, and multileaders' text lined up.

use oxidraft_geom::{Point, Segment};
use oxidraft_model::{EntityId, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step};

fn leader_at(ctx: &Ctx, at: Point) -> Option<EntityId> {
    ctx.pick(at).filter(|id| matches!(ctx.doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Leader { .. })))
}

#[derive(Clone, Copy, PartialEq)]
enum EditStage {
    Select,
    Add(EntityId),
    Remove(EntityId),
}

/// MLEADEREDIT (MLE): more arrowheads for a multileader, each with a
/// leader to its text; or Remove leaders added.
pub struct MLeaderEdit {
    stage: EditStage,
}

impl Default for MLeaderEdit {
    fn default() -> Self {
        Self { stage: EditStage::Select }
    }
}

impl Command for MLeaderEdit {
    fn prompt(&self) -> Prompt {
        match self.stage {
            EditStage::Select => Prompt::new("Select a multileader"),
            EditStage::Add(_) => Prompt::new("Specify leader arrowhead location").with_keywords(&["Remove leaders"]),
            EditStage::Remove(_) => Prompt::new("Specify leaders to remove"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (EditStage::Select, Input::Pick { at, .. }) => {
                self.stage = EditStage::Add(leader_at(ctx, at).ok_or("Select a multileader.")?);
            }
            (EditStage::Add(id), Input::Keyword(_)) => self.stage = EditStage::Remove(id),
            (EditStage::Add(id), Input::Point(p)) => {
                ctx.doc.update(id, |e| {
                    if let Geometry::Leader { more, .. } = &mut e.geometry {
                        more.push(p);
                    }
                });
            }
            (EditStage::Remove(id), Input::Pick { at, .. } | Input::Point(at)) => {
                let Some(Geometry::Leader { points, more, .. }) = ctx.doc.entity(id).map(|e| e.geometry.clone()) else {
                    return Ok(Step::Done);
                };
                let end = *points.last().ok_or("No leader.")?;
                // The added leader passing nearest the point.
                let nearest = (0..more.len()).min_by(|&a, &b| {
                    let d = |i: usize| Segment::new(more[i], end).distance_to(at);
                    d(a).total_cmp(&d(b))
                });
                let Some(i) = nearest else { return Err("Only added leaders can be removed.".into()) };
                ctx.doc.update(id, |e| {
                    if let Geometry::Leader { more, .. } = &mut e.geometry {
                        more.remove(i);
                    }
                });
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn base_point(&self) -> Option<Point> {
        None
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, EditStage::Select | EditStage::Remove(_))
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            EditStage::Add(id) | EditStage::Remove(id) => vec![id],
            EditStage::Select => Vec::new(),
        }
    }
}

#[derive(Clone, PartialEq)]
enum AlignStage {
    Select,
    Reference(Vec<EntityId>),
    Direction(Vec<EntityId>, EntityId),
}

/// MLEADERALIGN (MLA): the selected multileaders' text lined up with one
/// of them, along a direction picked; their arrowheads stay.
pub struct MLeaderAlign {
    stage: AlignStage,
}

impl Default for MLeaderAlign {
    fn default() -> Self {
        Self { stage: AlignStage::Select }
    }
}

/// Where a leader's lines end, before its landing and text.
fn landing(ctx: &Ctx, id: EntityId) -> Option<Point> {
    match ctx.doc.entity(id).map(|e| &e.geometry) {
        Some(Geometry::Leader { points, .. }) => points.last().copied(),
        _ => None,
    }
}

impl Command for MLeaderAlign {
    fn prompt(&self) -> Prompt {
        match self.stage {
            AlignStage::Select => Prompt::new("Select multileaders"),
            AlignStage::Reference(_) => Prompt::new("Select multileader to align to"),
            AlignStage::Direction(..) => Prompt::new("Specify direction"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (AlignStage::Select, Input::Selection(s)) => {
                let ids: Vec<EntityId> = s
                    .ids
                    .into_iter()
                    .filter(|id| matches!(ctx.doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Leader { .. })))
                    .collect();
                if ids.is_empty() {
                    return Ok(Step::Done);
                }
                self.stage = AlignStage::Reference(ids);
            }
            (AlignStage::Reference(ids), Input::Pick { at, .. }) => {
                let reference = leader_at(ctx, at).ok_or("Select a multileader.")?;
                self.stage = AlignStage::Direction(ids, reference);
            }
            (AlignStage::Direction(ids, reference), Input::Point(p)) => {
                let base = landing(ctx, reference).ok_or("The multileader is gone.")?;
                let d = p - base;
                if d.length() < 1e-9 {
                    return Err("Pick a direction away from the multileader's text.".into());
                }
                let u = d * (1.0 / d.length());
                let mut moved = 0;
                for id in ids.iter().filter(|id| **id != reference) {
                    let Some(end) = landing(ctx, *id) else { continue };
                    let target = base + u * (end - base).dot(u);
                    ctx.doc.update(*id, |e| {
                        if let Geometry::Leader { points, .. } = &mut e.geometry
                            && let Some(last) = points.last_mut()
                        {
                            *last = target;
                        }
                    });
                    moved += 1;
                }
                ctx.messages.push(format!("{moved} multileader(s) aligned."));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn base_point(&self) -> Option<Point> {
        None
    }

    fn wants_selection(&self) -> bool {
        self.stage == AlignStage::Select
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, AlignStage::Reference(_))
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    fn click(e: &mut Engine, d: &mut Document, x: f64, y: f64) {
        let p = Point::new(x, y);
        e.click(p, p, false, d, 0.5);
    }

    fn leader(tip: (f64, f64), end: (f64, f64)) -> Geometry {
        Geometry::Leader {
            points: vec![Point::new(tip.0, tip.1), Point::new(end.0, end.1)],
            content: "NOTE".into(),
            scale: 1.0,
            more: Vec::new(),
        }
    }

    #[test]
    fn add_remove_and_align() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let a = d.add(leader((0.0, 0.0), (10.0, 10.0)));
        let b = d.add(leader((0.0, -20.0), (14.0, -5.0)));
        run(&mut e, &mut d, &["mleaderedit"]);
        click(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &["20,0", "25,2", ""]);
        let more = |d: &Document, id| match &d.entity(id).unwrap().geometry {
            Geometry::Leader { more, .. } => more.clone(),
            _ => Vec::new(),
        };
        assert_eq!(more(&d, a).len(), 2);
        run(&mut e, &mut d, &["mleaderedit"]);
        click(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &["r"]);
        click(&mut e, &mut d, 15.0, 5.0);
        run(&mut e, &mut d, &[""]);
        assert_eq!(more(&d, a), vec![Point::new(25.0, 2.0)], "the one nearest went");
        // Align b's text under a's, straight down.
        run(&mut e, &mut d, &["mleaderalign", "all", ""]);
        click(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &["10,0"]);
        let end = match &d.entity(b).unwrap().geometry {
            Geometry::Leader { points, .. } => points[1],
            _ => Point::ORIGIN,
        };
        assert_eq!(end, Point::new(10.0, -5.0));
    }
}

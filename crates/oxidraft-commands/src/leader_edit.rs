//! MLEADEREDIT and MLEADERALIGN: arrowheads added to a multileader or
//! taken off, and multileaders' text lined up; MLEADERCOLLECT: the blocks
//! of several multileaders gathered on one.

use oxidraft_geom::{Point, Segment};
use oxidraft_model::annotation::LeaderBlocks;
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

#[derive(Clone, PartialEq)]
enum CollectStage {
    Select,
    Location,
    Wrap,
    Number,
}

/// MLEADERCOLLECT: multileaders with block content made one, its blocks
/// in a row (Horizontal), a column (Vertical) or rows of a width (Wrap)
/// where the location says; the first one's leader stays.
pub struct MLeaderCollect {
    stage: CollectStage,
    ids: Vec<EntityId>,
    /// Blocks to a row; none: all in one.
    per_row: Option<usize>,
}

impl Default for MLeaderCollect {
    fn default() -> Self {
        Self { stage: CollectStage::Select, ids: Vec::new(), per_row: None }
    }
}

impl MLeaderCollect {
    fn collect(&self, ctx: &mut Ctx, at: Point) -> Step {
        let mut items = Vec::new();
        for id in &self.ids {
            if let Some(Geometry::Leader { blocks: Some(b), .. }) = ctx.doc.entity(*id).map(|e| &e.geometry) {
                items.extend(b.items.iter().cloned());
            }
        }
        let per_row = self.per_row.unwrap_or(items.len()).clamp(1, items.len().max(1));
        let (first, rest) = self.ids.split_first().expect("leaders were selected");
        ctx.doc.update(*first, |e| {
            if let Geometry::Leader { points, blocks, .. } = &mut e.geometry {
                if let Some(last) = points.last_mut() {
                    *last = at;
                }
                *blocks = Some(Box::new(LeaderBlocks { items, per_row }));
            }
        });
        for id in rest {
            ctx.doc.remove(*id);
        }
        Step::Done
    }
}

impl Command for MLeaderCollect {
    fn prompt(&self) -> Prompt {
        match self.stage {
            CollectStage::Select => Prompt::new("Select multileaders"),
            CollectStage::Location => Prompt::new("Specify collected multileader location")
                .with_keywords(&["Vertical", "Horizontal", "Wrap"])
                .with_default("Horizontal"),
            CollectStage::Wrap => Prompt::new("Specify wrap width").with_keywords(&["Number"]),
            CollectStage::Number => Prompt::new("Enter the maximum number of blocks per row"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (CollectStage::Select, Input::Selection(s)) => {
                self.ids = s
                    .ids
                    .into_iter()
                    .filter(|id| {
                        matches!(
                            ctx.doc.entity(*id).map(|e| &e.geometry),
                            Some(Geometry::Leader { blocks: Some(_), .. })
                        )
                    })
                    .collect();
                if self.ids.is_empty() {
                    ctx.messages.push("No multileaders with block content selected.".into());
                    return Ok(Step::Done);
                }
                self.stage = CollectStage::Location;
            }
            (CollectStage::Select, _) => return Err("Select multileaders, then press Enter.".into()),
            (CollectStage::Location, Input::Point(p)) => return Ok(self.collect(ctx, p)),
            (CollectStage::Location, Input::Keyword("Vertical")) => self.per_row = Some(1),
            (CollectStage::Location, Input::Keyword("Wrap")) => self.stage = CollectStage::Wrap,
            (CollectStage::Location, Input::Keyword(_)) => self.per_row = None,
            (CollectStage::Wrap, Input::Keyword(_)) => self.stage = CollectStage::Number,
            (CollectStage::Wrap, Input::Value(w)) if w > 0.0 => {
                // As many blocks as fit across.
                let first = self.ids[0];
                let width = match ctx.doc.entity(first).map(|e| &e.geometry) {
                    Some(Geometry::Leader { blocks: Some(b), scale, .. }) => b
                        .items
                        .iter()
                        .filter_map(|i| ctx.doc.block_extents(&i.name))
                        .map(|x| (x.max.x - x.min.x) * scale)
                        .fold(0.0, f64::max),
                    _ => 0.0,
                };
                self.per_row = Some(if width > 0.0 { ((w / width).floor() as usize).max(1) } else { 1 });
                self.stage = CollectStage::Location;
            }
            (CollectStage::Number, Input::Value(n)) if n >= 1.0 && n.fract() == 0.0 => {
                self.per_row = Some(n as usize);
                self.stage = CollectStage::Location;
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (CollectStage::Number, _) => return Err("Requires a whole number of 1 or more.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.ids.clone()
    }

    fn wants_selection(&self) -> bool {
        self.stage == CollectStage::Select
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
            blocks: None,
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

    #[test]
    fn tag_leaders_and_collect() {
        use oxidraft_model::annotation::TagShape;
        let (mut e, mut d) = (Engine::new(), Document::new());
        // The style's content a circled tag; three of them, numbered.
        let mut style = d.leader_style();
        style.block = Some(TagShape::Circle);
        d.set_annotation_styles(style, d.table_style());
        for (i, x) in [0.0, 30.0, 60.0].iter().enumerate() {
            run(&mut e, &mut d, &["mleader", &format!("{x},0"), &format!("{},10", x + 5.0), &(i + 1).to_string()]);
        }
        assert_eq!(d.len(), 3);
        assert!(d.block("_TagCircle").is_some(), "the tag block was defined");
        let values = |d: &Document| -> Vec<String> {
            d.entities()
                .flat_map(|x| match &x.geometry {
                    Geometry::Leader { blocks: Some(b), .. } => b.items.iter().map(|i| i.value.clone()).collect(),
                    _ => Vec::new(),
                })
                .collect()
        };
        assert_eq!(values(&d), ["1", "2", "3"]);
        // Collected two to a row, at (20,30).
        run(&mut e, &mut d, &["mleadercollect", "all", "", "w", "n", "2", "20,30"]);
        assert!(!e.is_active());
        assert_eq!(d.len(), 1);
        assert_eq!(values(&d), ["1", "2", "3"]);
        let drawn = d.expanded(d.entities().next().unwrap());
        assert_eq!(drawn.iter().filter(|g| matches!(g, Geometry::Circle { .. })).count(), 3);
        // Options, Content type Mtext: a text one even so.
        run(&mut e, &mut d, &["mleader", "o", "m", "0,-40", "10,-30", "NOTE"]);
        assert!(
            d.entities()
                .any(|x| matches!(&x.geometry, Geometry::Leader { content, blocks: None, .. } if content == "NOTE"))
        );
    }
}

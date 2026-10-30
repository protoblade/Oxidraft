use oxidraft_geom::Point;
use oxidraft_model::{EntityId, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step};

/// Grip editing: dragging a hot grip stretches every selected object that has
/// a grip at that spot (AutoCAD's `** STRETCH **` grip mode).
pub struct GripStretch {
    /// Entity, grip index and its geometry before the edit.
    targets: Vec<(EntityId, usize, Geometry)>,
    base: Point,
}

impl GripStretch {
    pub fn new(targets: Vec<(EntityId, usize, Geometry)>, base: Point) -> Self {
        Self { targets, base }
    }

    fn stretched(&self, to: Point) -> impl Iterator<Item = (EntityId, Geometry)> + '_ {
        self.targets.iter().map(move |(id, grip, g)| (*id, g.with_grip_moved(*grip, to)))
    }
}

impl Command for GripStretch {
    fn prompt(&self) -> Prompt {
        Prompt::new("** STRETCH ** Specify stretch point").with_keywords(&["eXit"])
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let to = match input {
            Input::Point(p) => p,
            Input::Value(v) if !self.base.approx_eq(ctx.cursor) => self.base.polar(v, self.base.angle_to(ctx.cursor)),
            Input::Enter | Input::Keyword(_) => return Ok(Step::Done),
            _ => return Err("Point required.".into()),
        };
        for (id, g) in self.stretched(to).collect::<Vec<_>>() {
            ctx.doc.modify(id, g);
        }
        // An annotative object's representation grip (a text's point, a
        // dimension's line, a multileader's text end): only this scale's
        // representation moves, as in AutoCAD.
        for (id, grip, g) in &self.targets {
            let anchor = g.representation_grip() == Some(*grip);
            if anchor && ctx.doc.entity(*id).is_some_and(|e| e.annotative.is_some()) {
                ctx.doc.move_scale_representation(*id, to - self.base);
            }
        }
        Ok(Step::Done)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.stretched(cursor).map(|(_, g)| g).collect()
    }

    fn base_point(&self) -> Option<Point> {
        Some(self.base)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.targets.iter().map(|t| t.0).collect()
    }
}

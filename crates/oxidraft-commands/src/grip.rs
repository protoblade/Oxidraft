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

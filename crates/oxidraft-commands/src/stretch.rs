use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Entity, Geometry, snap};

use crate::{
    Command, Ctx, Input, Prompt, Step, Vars,
    common::{SELECT, apply, selected_entities},
};

enum Stage {
    Select,
    Base,
    Second(Point),
}

/// STRETCH: moves the endpoints that fall inside the selection window(s),
/// stretching the lines attached to them.
pub struct Stretch {
    stage: Stage,
    selected: Vec<Entity>,
    windows: Vec<Aabb>,
    lassos: Vec<Vec<Point>>,
}

impl Default for Stretch {
    fn default() -> Self {
        Self { stage: Stage::Select, selected: Vec::new(), windows: Vec::new(), lassos: Vec::new() }
    }
}

impl Stretch {
    /// Whether `p` is in one of the selection's windows or lassos.
    fn inside(&self, p: Point) -> bool {
        self.windows.iter().any(|w| w.contains(p)) || self.lassos.iter().any(|l| snap::in_polygon(l, p))
    }

    fn finish(&self, d: Point, ctx: &mut Ctx) -> Step {
        apply(&self.selected, false, ctx, |g| g.stretched_where(&|p| self.inside(p), d));
        Step::Done
    }
}

impl Command for Stretch {
    fn start(&mut self, _vars: &Vars) -> Option<String> {
        Some("Select objects to stretch by crossing-window or crossing-polygon...".into())
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select objects"),
            Stage::Base => Prompt::new("Specify base point"),
            Stage::Second(_) => Prompt::new("Specify second point").with_default("use first point as displacement"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (Stage::Select, Input::Selection(s)) => {
                self.selected = selected_entities(&s, ctx);
                (self.windows, self.lassos) = (s.windows, s.lassos);
                if self.windows.is_empty() && self.lassos.is_empty() && !self.selected.is_empty() {
                    ctx.messages.push("No crossing window was used, so there is nothing to stretch.".into());
                    return Ok(Step::Done);
                }
                self.stage = Stage::Base;
                Ok(if self.selected.is_empty() { Step::Done } else { Step::Continue })
            }
            (Stage::Select, _) => Err(SELECT.into()),
            (Stage::Base, Input::Point(p)) => {
                self.stage = Stage::Second(p);
                Ok(Step::Continue)
            }
            (Stage::Base, Input::Enter) => Ok(Step::Done),
            (Stage::Second(base), Input::Point(p)) => Ok(self.finish(p - *base, ctx)),
            (Stage::Second(base), Input::Value(v)) => {
                if base.approx_eq(ctx.cursor) {
                    return Err("Move the cursor to show a direction.".into());
                }
                let to = base.polar(v, base.angle_to(ctx.cursor));
                Ok(self.finish(to - *base, ctx))
            }
            // Enter uses the base point itself as the displacement, as in AutoCAD.
            (Stage::Second(base), Input::Enter) => Ok(self.finish(*base, ctx)),
            _ => Err("Point required.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Stage::Second(base) = self.stage else { return Vec::new() };
        let d = cursor - base;
        let mut out: Vec<Geometry> =
            self.selected.iter().map(|e| e.geometry.stretched_where(&|p| self.inside(p), d)).collect();
        out.push(Geometry::Line { start: base, end: cursor });
        out
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Second(base) => Some(base),
            _ => None,
        }
    }

    fn highlighted(&self) -> Vec<oxidraft_model::EntityId> {
        self.selected.iter().map(|e| e.id).collect()
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, Stage::Select)
    }
}

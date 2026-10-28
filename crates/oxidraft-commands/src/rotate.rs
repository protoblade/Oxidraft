use oxidraft_geom::Point;
use oxidraft_model::{Entity, Geometry};

use crate::{
    Command, Ctx, Input, Prompt, Step, Vars,
    common::{SELECT, apply, selected_entities},
};

enum Stage {
    Select,
    Base,
    Angle,
    /// Reference option: waiting for the reference angle (a value or two points).
    ReferenceAngle,
    ReferenceSecond(Point),
    NewAngle {
        reference: f64,
    },
}

/// ROTATE: select, base point, rotation angle (or Copy / Reference).
pub struct Rotate {
    stage: Stage,
    selected: Vec<Entity>,
    base: Point,
    copy: bool,
}

impl Default for Rotate {
    fn default() -> Self {
        Self { stage: Stage::Select, selected: Vec::new(), base: Point::ORIGIN, copy: false }
    }
}

impl Rotate {
    fn finish(&self, angle: f64, ctx: &mut Ctx) -> Step {
        let base = self.base;
        apply(&self.selected, self.copy, ctx, |g| g.rotated(base, angle));
        Step::Done
    }

    /// Angle from the base point to `p`, for picking an angle with the mouse.
    fn angle_to(&self, p: Point) -> f64 {
        self.base.angle_to(p)
    }
}

impl Command for Rotate {
    fn start(&mut self, _vars: &Vars) -> Option<String> {
        Some("Current positive angle in UCS:  ANGDIR=counterclockwise  ANGBASE=0".into())
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select objects"),
            Stage::Base => Prompt::new("Specify base point"),
            Stage::Angle => {
                Prompt::new("Specify rotation angle").with_keywords(&["Copy", "Reference"]).with_default("0")
            }
            Stage::ReferenceAngle => Prompt::new("Specify the reference angle").with_default("0"),
            Stage::ReferenceSecond(_) => Prompt::new("Specify second point"),
            Stage::NewAngle { .. } => Prompt::new("Specify the new angle").with_default("0"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (Stage::Select, Input::Selection(s)) => {
                self.selected = selected_entities(&s, ctx);
                self.stage = Stage::Base;
                Ok(if self.selected.is_empty() { Step::Done } else { Step::Continue })
            }
            (Stage::Select, _) => Err(SELECT.into()),
            (Stage::Base, Input::Point(p)) => {
                self.base = p;
                self.stage = Stage::Angle;
                Ok(Step::Continue)
            }
            (Stage::Base, Input::Enter) => Ok(Step::Done),
            (Stage::Base, _) => Err("Point required.".into()),
            (Stage::Angle, Input::Value(a)) => Ok(self.finish(a, ctx)),
            (Stage::Angle, Input::Point(p)) => Ok(self.finish(self.angle_to(p), ctx)),
            (Stage::Angle, Input::Enter) => Ok(Step::Done),
            (Stage::Angle, Input::Keyword("Copy")) => {
                self.copy = true;
                ctx.messages.push("Rotating a copy of the selected objects.".into());
                Ok(Step::Continue)
            }
            (Stage::Angle, Input::Keyword("Reference")) => {
                self.stage = Stage::ReferenceAngle;
                Ok(Step::Continue)
            }
            (Stage::ReferenceAngle, Input::Value(r)) => {
                self.stage = Stage::NewAngle { reference: r };
                Ok(Step::Continue)
            }
            (Stage::ReferenceAngle, Input::Point(p)) => {
                self.stage = Stage::ReferenceSecond(p);
                Ok(Step::Continue)
            }
            (Stage::ReferenceAngle, Input::Enter) => {
                self.stage = Stage::NewAngle { reference: 0.0 };
                Ok(Step::Continue)
            }
            (Stage::ReferenceSecond(first), Input::Point(p)) => {
                self.stage = Stage::NewAngle { reference: first.angle_to(p) };
                Ok(Step::Continue)
            }
            (Stage::NewAngle { reference }, Input::Value(a)) => Ok(self.finish(a - reference, ctx)),
            (Stage::NewAngle { reference }, Input::Point(p)) => Ok(self.finish(self.angle_to(p) - reference, ctx)),
            (Stage::NewAngle { reference }, Input::Enter) => Ok(self.finish(-reference, ctx)),
            _ => Err("Requires a valid angle or two points.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let angle = match self.stage {
            Stage::Angle => self.angle_to(cursor),
            Stage::NewAngle { reference } => self.angle_to(cursor) - reference,
            Stage::ReferenceSecond(first) => return vec![Geometry::Line { start: first, end: cursor }],
            _ => return Vec::new(),
        };
        let mut out: Vec<Geometry> = self.selected.iter().map(|e| e.geometry.rotated(self.base, angle)).collect();
        out.push(Geometry::Line { start: self.base, end: cursor });
        out
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Angle | Stage::NewAngle { .. } | Stage::ReferenceAngle => Some(self.base),
            Stage::ReferenceSecond(first) => Some(first),
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

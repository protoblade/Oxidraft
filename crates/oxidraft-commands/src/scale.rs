use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Entity, Geometry};

use crate::{
    Command, Ctx, Input, Prompt, Step,
    common::{SELECT, apply, selected_entities},
};

enum Stage {
    Select,
    Base,
    Factor,
    /// Reference option: waiting for the reference length (a value or two points).
    ReferenceLength,
    ReferenceSecond(Point),
    NewLength {
        reference: f64,
    },
}

/// SCALE: select, base point, scale factor (or Copy / Reference).
pub struct Scale {
    stage: Stage,
    selected: Vec<Entity>,
    base: Point,
    copy: bool,
}

impl Default for Scale {
    fn default() -> Self {
        Self { stage: Stage::Select, selected: Vec::new(), base: Point::ORIGIN, copy: false }
    }
}

const POSITIVE: &str = "Requires a positive nonzero value.";

impl Scale {
    fn finish(&self, factor: f64, ctx: &mut Ctx) -> Result<Step, String> {
        if factor <= EPSILON || !factor.is_finite() {
            return Err(POSITIVE.into());
        }
        let base = self.base;
        apply(&self.selected, self.copy, ctx, |g| g.scaled(base, factor));
        Ok(Step::Done)
    }

    fn preview_factor(&self, cursor: Point) -> Option<f64> {
        let d = self.base.distance(cursor);
        let f = match self.stage {
            Stage::Factor => d,
            Stage::NewLength { reference } => d / reference,
            _ => return None,
        };
        (f > EPSILON && f.is_finite()).then_some(f)
    }
}

impl Command for Scale {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select objects"),
            Stage::Base => Prompt::new("Specify base point"),
            Stage::Factor => Prompt::new("Specify scale factor").with_keywords(&["Copy", "Reference"]),
            Stage::ReferenceLength => Prompt::new("Specify reference length").with_default("1.0000"),
            Stage::ReferenceSecond(_) => Prompt::new("Specify second point"),
            Stage::NewLength { .. } => Prompt::new("Specify new length").with_default("1.0000"),
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
                self.stage = Stage::Factor;
                Ok(Step::Continue)
            }
            (Stage::Base, Input::Enter) => Ok(Step::Done),
            (Stage::Base, _) => Err("Point required.".into()),
            (Stage::Factor, Input::Value(f)) => self.finish(f, ctx),
            // A picked point's distance from the base point is the factor.
            (Stage::Factor, Input::Point(p)) => self.finish(self.base.distance(p), ctx),
            (Stage::Factor, Input::Keyword("Copy")) => {
                self.copy = true;
                ctx.messages.push("Scaling a copy of the selected objects.".into());
                Ok(Step::Continue)
            }
            (Stage::Factor, Input::Keyword("Reference")) => {
                self.stage = Stage::ReferenceLength;
                Ok(Step::Continue)
            }
            (Stage::ReferenceLength, Input::Value(r)) if r > EPSILON => {
                self.stage = Stage::NewLength { reference: r };
                Ok(Step::Continue)
            }
            (Stage::ReferenceLength, Input::Point(p)) => {
                self.stage = Stage::ReferenceSecond(p);
                Ok(Step::Continue)
            }
            (Stage::ReferenceLength, Input::Enter) => {
                self.stage = Stage::NewLength { reference: 1.0 };
                Ok(Step::Continue)
            }
            (Stage::ReferenceSecond(first), Input::Point(p)) => {
                let r = first.distance(p);
                if r <= EPSILON {
                    return Err(POSITIVE.into());
                }
                self.stage = Stage::NewLength { reference: r };
                Ok(Step::Continue)
            }
            (Stage::NewLength { reference }, Input::Value(n)) => self.finish(n / reference, ctx),
            (Stage::NewLength { reference }, Input::Point(p)) => self.finish(self.base.distance(p) / reference, ctx),
            (Stage::NewLength { reference }, Input::Enter) => self.finish(1.0 / reference, ctx),
            _ => Err(POSITIVE.into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        if let Stage::ReferenceSecond(first) = self.stage {
            return vec![Geometry::Line { start: first, end: cursor }];
        }
        let Some(f) = self.preview_factor(cursor) else { return Vec::new() };
        let mut out: Vec<Geometry> = self.selected.iter().map(|e| e.geometry.scaled(self.base, f)).collect();
        out.push(Geometry::Line { start: self.base, end: cursor });
        out
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Factor | Stage::NewLength { .. } | Stage::ReferenceLength => Some(self.base),
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

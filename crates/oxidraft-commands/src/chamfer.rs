use oxidraft_geom::Point;
use oxidraft_model::{EntityId, Geometry};

use crate::{
    Command, Ctx, Input, Prompt, Step, Vars,
    edit::{chamfer_lines, chamfer_polyline},
};

#[derive(Clone, Copy)]
enum Stage {
    First,
    /// Distance option; returns to the stage it was entered from.
    FirstDistance {
        back_to_second: Option<(EntityId, Point)>,
    },
    SecondDistance {
        back_to_second: Option<(EntityId, Point)>,
    },
    Second {
        first: EntityId,
        at: Point,
    },
    Polyline,
}

/// CHAMFER: bevels the corner between two lines, or every corner of a
/// polyline. The distances are remembered in CHAMFERA and CHAMFERB.
pub struct Chamfer {
    stage: Stage,
    d1: f64,
    d2: f64,
}

impl Default for Chamfer {
    fn default() -> Self {
        Self { stage: Stage::First, d1: 0.0, d2: 0.0 }
    }
}

const ONLY_LINES: &str = "Only lines can be chamfered so far (use the Polyline option for polyline corners).";

fn line_at(ctx: &Ctx, at: Point) -> Result<(EntityId, (Point, Point)), String> {
    let id = ctx.pick(at).ok_or("No object found.")?;
    match ctx.doc.entity(id).map(|e| &e.geometry) {
        Some(Geometry::Line { start, end }) => Ok((id, (*start, *end))),
        _ => Err(ONLY_LINES.into()),
    }
}

impl Command for Chamfer {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        (self.d1, self.d2) = (vars.chamfer_a, vars.chamfer_b);
        Some(format!("(TRIM mode) Current chamfer Dist1 = {:.4}, Dist2 = {:.4}", self.d1, self.d2))
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::First => Prompt::new("Select first line").with_keywords(&["Polyline", "Distance"]),
            Stage::FirstDistance { .. } => {
                Prompt::new("Specify first chamfer distance").with_default(format!("{:.4}", self.d1))
            }
            Stage::SecondDistance { .. } => {
                Prompt::new("Specify second chamfer distance").with_default(format!("{:.4}", self.d2))
            }
            Stage::Second { .. } => {
                Prompt::new("Select second line or shift-select to apply corner").with_keywords(&["Distance"])
            }
            Stage::Polyline => Prompt::new("Select 2D polyline"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let nonnegative = |input: &Input| match input {
            Input::Value(v) if *v < 0.0 => Err("Requires a nonnegative value.".to_string()),
            _ => Ok(()),
        };
        match (self.stage, input) {
            (Stage::First, Input::Pick { at, .. }) => {
                let (first, _) = line_at(ctx, at)?;
                self.stage = Stage::Second { first, at };
            }
            (Stage::First, Input::Keyword("Distance")) => {
                self.stage = Stage::FirstDistance { back_to_second: None };
            }
            (Stage::Second { first, at }, Input::Keyword("Distance")) => {
                self.stage = Stage::FirstDistance { back_to_second: Some((first, at)) };
            }
            (Stage::First, Input::Keyword("Polyline")) => self.stage = Stage::Polyline,
            (Stage::FirstDistance { back_to_second }, input @ (Input::Value(_) | Input::Enter)) => {
                nonnegative(&input)?;
                if let Input::Value(d) = input {
                    self.d1 = d;
                    // Like AutoCAD, the second distance defaults to the first.
                    self.d2 = d;
                }
                self.stage = Stage::SecondDistance { back_to_second };
            }
            (Stage::SecondDistance { back_to_second }, input @ (Input::Value(_) | Input::Enter)) => {
                nonnegative(&input)?;
                if let Input::Value(d) = input {
                    self.d2 = d;
                }
                (ctx.vars.chamfer_a, ctx.vars.chamfer_b) = (self.d1, self.d2);
                self.stage = match back_to_second {
                    Some((first, at)) => Stage::Second { first, at },
                    None => Stage::First,
                };
            }
            (Stage::Second { first, at: first_at }, Input::Pick { at, shift }) => {
                let (second, line_b) = line_at(ctx, at)?;
                if second == first {
                    return Err("Select a different line.".into());
                }
                let Some(Geometry::Line { start, end }) = ctx.doc.entity(first).map(|e| e.geometry.clone()) else {
                    return Err(ONLY_LINES.into());
                };
                let (d1, d2) = if shift { (0.0, 0.0) } else { (self.d1, self.d2) };
                let f = chamfer_lines((start, end), first_at, line_b, at, d1, d2)?;
                let source = ctx.doc.entity(first).cloned().ok_or("Object not found.")?;
                ctx.doc.modify(first, f.first);
                ctx.doc.modify(second, f.second);
                if let Some(cut) = f.arc {
                    ctx.doc.add_like(cut, &source);
                }
                return Ok(Step::Done);
            }
            (Stage::Polyline, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                let Some(Geometry::Polyline { vertices, closed }) = ctx.doc.entity(id).map(|e| e.geometry.clone())
                else {
                    return Err("Object selected is not a 2D polyline.".into());
                };
                let (vertices, done, short) = chamfer_polyline(&vertices, closed, self.d1, self.d2);
                if done > 0 {
                    ctx.doc.modify(id, Geometry::Polyline { vertices, closed });
                }
                let noun = if done == 1 { "line was" } else { "lines were" };
                ctx.messages.push(format!("{done} {noun} chamfered"));
                if short > 0 {
                    ctx.messages.push(format!("{short} were too short"));
                }
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::FirstDistance { .. } | Stage::SecondDistance { .. }, _) => {
                return Err("Requires a nonnegative distance.".into());
            }
            _ => return Err("Select a line or an option keyword.".into()),
        }
        Ok(Step::Continue)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            Stage::Second { first, .. }
            | Stage::FirstDistance { back_to_second: Some((first, _)) }
            | Stage::SecondDistance { back_to_second: Some((first, _)) } => vec![first],
            _ => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::First | Stage::Second { .. } | Stage::Polyline)
    }
}

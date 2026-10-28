use oxidraft_geom::Point;
use oxidraft_model::{EntityId, Geometry, snap};

use crate::{
    Command, Ctx, Input, Prompt, Step, Vars,
    edit::{fillet_lines, fillet_polyline},
};

#[derive(Clone, Copy)]
enum Stage {
    First,
    /// Radius option; returns to the stage it was entered from.
    Radius {
        back_to_second: Option<(EntityId, Point)>,
    },
    Second {
        first: EntityId,
        at: Point,
    },
    Polyline,
}

/// FILLET: rounds (or, with radius 0, squares) the corner between two lines,
/// or every corner of a polyline. The radius is remembered in FILLETRAD.
pub struct Fillet {
    stage: Stage,
    radius: f64,
}

impl Default for Fillet {
    fn default() -> Self {
        Self { stage: Stage::First, radius: 0.0 }
    }
}

const ONLY_LINES: &str = "Only lines can be filleted so far (use the Polyline option for polyline corners).";

fn line_at(ctx: &Ctx, at: Point) -> Result<(EntityId, (Point, Point)), String> {
    let id = snap::pick(ctx.doc, at, ctx.pick_tolerance).ok_or("No object found.")?;
    match ctx.doc.entity(id).map(|e| &e.geometry) {
        Some(Geometry::Line { start, end }) => Ok((id, (*start, *end))),
        _ => Err(ONLY_LINES.into()),
    }
}

impl Command for Fillet {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.radius = vars.fillet_radius;
        Some(format!("Current settings: Mode = TRIM, Radius = {:.4}", self.radius))
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::First => Prompt::new("Select first object").with_keywords(&["Polyline", "Radius"]),
            Stage::Radius { .. } => Prompt::new("Specify fillet radius").with_default(format!("{:.4}", self.radius)),
            Stage::Second { .. } => {
                Prompt::new("Select second object or shift-select to apply corner").with_keywords(&["Radius"])
            }
            Stage::Polyline => Prompt::new("Select 2D polyline"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::First, Input::Pick { at, .. }) => {
                let (first, _) = line_at(ctx, at)?;
                self.stage = Stage::Second { first, at };
            }
            (Stage::First, Input::Keyword("Radius")) => self.stage = Stage::Radius { back_to_second: None },
            (Stage::Second { first, at }, Input::Keyword("Radius")) => {
                self.stage = Stage::Radius { back_to_second: Some((first, at)) };
            }
            (Stage::First, Input::Keyword("Polyline")) => self.stage = Stage::Polyline,
            (Stage::Radius { back_to_second }, input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(r) = input {
                    if r < 0.0 {
                        return Err("Requires a nonnegative value.".into());
                    }
                    self.radius = r;
                    ctx.vars.fillet_radius = r;
                }
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
                let radius = if shift { 0.0 } else { self.radius };
                let f = fillet_lines((start, end), first_at, line_b, at, radius)?;
                let (layer, color) =
                    ctx.doc.entity(first).map(|e| (e.layer.clone(), e.color)).ok_or("Object not found.")?;
                ctx.doc.modify(first, f.first);
                ctx.doc.modify(second, f.second);
                if let Some(arc) = f.arc {
                    ctx.doc.add_on_layer(arc, &layer, color);
                }
                return Ok(Step::Done);
            }
            (Stage::Polyline, Input::Pick { at, .. }) => {
                let id = snap::pick(ctx.doc, at, ctx.pick_tolerance).ok_or("No object found.")?;
                let Some(Geometry::Polyline { vertices, closed }) = ctx.doc.entity(id).map(|e| e.geometry.clone())
                else {
                    return Err("Object selected is not a 2D polyline.".into());
                };
                let (vertices, done, short) = fillet_polyline(&vertices, closed, self.radius);
                if done > 0 {
                    ctx.doc.modify(id, Geometry::Polyline { vertices, closed });
                }
                let noun = if done == 1 { "line was" } else { "lines were" };
                ctx.messages.push(format!("{done} {noun} filleted"));
                if short > 0 {
                    ctx.messages.push(format!("{short} were too short"));
                }
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Select an object or an option keyword.".into()),
        }
        Ok(Step::Continue)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            Stage::Second { first, .. } | Stage::Radius { back_to_second: Some((first, _)) } => vec![first],
            _ => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::First | Stage::Second { .. } | Stage::Polyline)
    }
}

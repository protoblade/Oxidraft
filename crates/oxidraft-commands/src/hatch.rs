use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Geometry, HatchPattern, Vertex, boundary::boundary_at};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

enum Stage {
    PickPoint,
    SelectObjects,
    Pattern,
    Scale,
    Angle,
}

/// HATCH: fills areas found around picked points (or inside picked closed
/// objects) with the current pattern. Islands inside are left out.
pub struct Hatch {
    stage: Stage,
    pattern: HatchPattern,
    scale: f64,
    angle: f64,
    created: usize,
}

impl Default for Hatch {
    fn default() -> Self {
        Self { stage: Stage::PickPoint, pattern: HatchPattern::Ansi31, scale: 1.0, angle: 0.0, created: 0 }
    }
}

impl Hatch {
    fn settings(&self) -> String {
        format!("Current pattern: {}  Scale: {:.4}  Angle: {:.0}", self.pattern.name(), self.scale, self.angle)
    }

    fn add(&mut self, loops: Vec<Vec<Vertex>>, ctx: &mut Ctx) {
        let id = ctx.doc.add(Geometry::Hatch { loops, pattern: self.pattern, scale: self.scale, angle: self.angle });
        // Behind what it fills (AutoCAD's HPDRAWORDER "Behind boundary").
        ctx.doc.send_behind_drawing(&[id]);
        self.created += 1;
    }
}

/// A closed object's outline as a hatch loop.
fn closed_loop(g: &Geometry) -> Option<Vec<Vertex>> {
    match g {
        // Two half circles (bulge 1 each).
        Geometry::Circle { center, radius } => Some(vec![
            Vertex::new(Point::new(center.x + radius, center.y), 1.0),
            Vertex::new(Point::new(center.x - radius, center.y), 1.0),
        ]),
        Geometry::Polyline { vertices, closed: true } if vertices.len() >= 2 => Some(vertices.clone()),
        _ => None,
    }
}

impl Command for Hatch {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        (self.pattern, self.scale, self.angle) = (vars.hatch_pattern, vars.hatch_scale, vars.hatch_angle);
        Some(self.settings())
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::PickPoint => Prompt::new("Pick internal point").with_keywords(&[
                "Select objects",
                "Pattern",
                "sCale",
                "Angle",
                "Undo",
            ]),
            Stage::SelectObjects => Prompt::new("Select a closed object").with_keywords(&["Undo"]),
            Stage::Pattern => Prompt::new("Enter a pattern name or [?]").with_default(self.pattern.name()),
            Stage::Scale => Prompt::new("Specify a scale for the pattern").with_default(format!("{:.4}", self.scale)),
            Stage::Angle => Prompt::new("Specify an angle for the pattern").with_default(format!("{:.0}", self.angle)),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (Stage::PickPoint, Input::Pick { at, .. }) => match boundary_at(ctx.doc, at) {
                Some(loops) => self.add(loops, ctx),
                None => ctx.messages.push("Valid hatch boundary not found.".into()),
            },
            (Stage::SelectObjects, Input::Pick { at, .. }) => {
                let picked = ctx.pick(at).and_then(|id| ctx.doc.entity(id));
                match picked.and_then(|e| closed_loop(&e.geometry)) {
                    Some(l) => self.add(vec![l], ctx),
                    None => return Err("Select a circle or a closed polyline.".into()),
                }
            }
            (Stage::PickPoint, Input::Keyword("Select objects")) => self.stage = Stage::SelectObjects,
            (Stage::PickPoint, Input::Keyword("Pattern")) => self.stage = Stage::Pattern,
            (Stage::PickPoint, Input::Keyword("sCale")) => self.stage = Stage::Scale,
            (Stage::PickPoint, Input::Keyword("Angle")) => self.stage = Stage::Angle,
            (Stage::PickPoint | Stage::SelectObjects, Input::Keyword("Undo")) => {
                if self.created > 0 {
                    ctx.doc.undo_last_in_transaction();
                    self.created -= 1;
                }
            }
            (Stage::Pattern, Input::Text(t)) if t == "?" => {
                let names: Vec<&str> = HatchPattern::ALL.iter().map(|p| p.name()).collect();
                ctx.messages.push(format!("Patterns: {}", names.join(", ")));
            }
            (Stage::Pattern, Input::Text(t)) => {
                self.pattern =
                    HatchPattern::from_name(&t).ok_or(format!("Unknown pattern \"{t}\". Enter ? to list them."))?;
                ctx.vars.hatch_pattern = self.pattern;
                ctx.messages.push(self.settings());
                self.stage = Stage::PickPoint;
            }
            (Stage::Scale, Input::Value(v)) if v > EPSILON => {
                self.scale = v;
                ctx.vars.hatch_scale = v;
                self.stage = Stage::PickPoint;
            }
            (Stage::Scale, Input::Value(_)) => return Err("Requires a positive nonzero value.".into()),
            (Stage::Angle, Input::Value(a)) => {
                self.angle = a;
                ctx.vars.hatch_angle = a;
                self.stage = Stage::PickPoint;
            }
            (Stage::Pattern | Stage::Scale | Stage::Angle, Input::Enter) => self.stage = Stage::PickPoint,
            (Stage::PickPoint | Stage::SelectObjects, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Pick a point or an option keyword.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::PickPoint | Stage::SelectObjects)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, Stage::Pattern)
    }
}

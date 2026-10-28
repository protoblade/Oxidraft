//! DONUT: filled rings and dots, as wide closed polylines of two arcs.

use oxidraft_geom::Point;
use oxidraft_model::{Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Inside,
    Outside,
    Center,
}

/// DONUT, remembering its diameters (DONUTID, DONUTOD). Places donuts
/// until Enter.
pub struct Donut {
    stage: Stage,
    inside: f64,
    outside: f64,
}

impl Default for Donut {
    fn default() -> Self {
        Self { stage: Stage::Inside, inside: 0.5, outside: 1.0 }
    }
}

/// A donut: two half circles through the middle of the ring, as wide as
/// the ring. An inside diameter of 0 makes a filled dot.
pub(crate) fn donut(center: Point, inside: f64, outside: f64) -> Geometry {
    let (r, w) = ((inside + outside) / 4.0, (outside - inside) / 2.0);
    let v = |x: f64| Vertex::new(center + Point::new(x, 0.0), 1.0).with_widths(w, w);
    Geometry::Polyline { vertices: vec![v(-r), v(r)], closed: true }
}

impl Command for Donut {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        (self.inside, self.outside) = (vars.donut_inside, vars.donut_outside);
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Inside => {
                Prompt::new("Specify inside diameter of donut").with_default(format!("{:.4}", self.inside))
            }
            Stage::Outside => {
                Prompt::new("Specify outside diameter of donut").with_default(format!("{:.4}", self.outside))
            }
            Stage::Center => Prompt::new("Specify center of donut").with_default("exit"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::Inside, Input::Value(d)) if d >= 0.0 => {
                self.inside = d;
                self.stage = Stage::Outside;
            }
            (Stage::Inside, Input::Enter) => self.stage = Stage::Outside,
            (Stage::Outside, Input::Value(d)) if d > self.inside => {
                self.outside = d;
                self.stage = Stage::Center;
            }
            (Stage::Outside, Input::Enter) if self.outside > self.inside => self.stage = Stage::Center,
            (Stage::Outside, _) => return Err("The outside diameter must be larger than the inside one.".into()),
            (Stage::Center, Input::Point(p)) => {
                ctx.doc.add(donut(p, self.inside, self.outside));
            }
            (Stage::Center, Input::Enter) => return Ok(Step::Done),
            (Stage::Inside, _) => return Err("Requires a diameter of 0 or more.".into()),
            _ => return Err("Point required.".into()),
        }
        (ctx.vars.donut_inside, ctx.vars.donut_outside) = (self.inside, self.outside);
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        if self.stage == Stage::Center { vec![donut(cursor, self.inside, self.outside)] } else { Vec::new() }
    }
}

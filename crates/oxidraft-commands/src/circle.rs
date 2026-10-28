use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::Geometry;

use crate::{Command, Ctx, Input, Prompt, Step};

/// CIRCLE: center, then radius or diameter.
#[derive(Default)]
pub struct Circle {
    center: Option<Point>,
    diameter: bool,
}

impl Circle {
    fn finish(&self, center: Point, size: f64, ctx: &mut Ctx) -> Result<Step, String> {
        let radius = if self.diameter { size / 2.0 } else { size };
        if radius <= EPSILON {
            return Err("Value must be positive and nonzero.".into());
        }
        ctx.doc.add(Geometry::Circle { center, radius });
        Ok(Step::Done)
    }
}

impl Command for Circle {
    fn prompt(&self) -> Prompt {
        match (self.center, self.diameter) {
            (None, _) => Prompt::new("Specify center point for circle"),
            (Some(_), false) => Prompt::new("Specify radius of circle").with_keywords(&["Diameter"]),
            (Some(_), true) => Prompt::new("Specify diameter of circle"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some(center) = self.center else {
            return match input {
                Input::Point(p) => {
                    self.center = Some(p);
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err("Point required.".into()),
            };
        };
        match input {
            Input::Point(p) => self.finish(center, center.distance(p), ctx),
            Input::Value(v) => self.finish(center, v, ctx),
            Input::Keyword("Diameter") => {
                self.diameter = true;
                Ok(Step::Continue)
            }
            Input::Keyword(_) | Input::Selection(_) | Input::Pick { .. } | Input::Text(_) => {
                Err("Point or option keyword required.".into())
            }
            Input::Enter => Ok(Step::Done),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Some(center) = self.center else { return Vec::new() };
        let d = center.distance(cursor);
        let radius = if self.diameter { d / 2.0 } else { d };
        vec![Geometry::Line { start: center, end: cursor }, Geometry::Circle { center, radius }]
    }

    fn base_point(&self) -> Option<Point> {
        self.center
    }
}

//! PLOT's Window option: the corners of the part of the drawing to plot,
//! picked while the Plot dialog steps aside.

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest, Vars};

/// Asks for two corners, then hands the window back to the Plot dialog
/// (`None` if Enter skipped it).
#[derive(Default)]
pub struct PlotWindow {
    first: Option<Point>,
}

impl Command for PlotWindow {
    fn start(&mut self, _vars: &Vars) -> Option<String> {
        Some("Specify window for printing".into())
    }

    fn prompt(&self) -> Prompt {
        Prompt::new(if self.first.is_none() { "Specify first corner" } else { "Specify opposite corner" })
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.first, input) {
            (None, Input::Point(p)) => {
                self.first = Some(p);
                Ok(Step::Continue)
            }
            (Some(a), Input::Point(b)) => {
                if (a.x - b.x).abs() < 1e-9 || (a.y - b.y).abs() < 1e-9 {
                    return Err("The window needs a width and a height.".into());
                }
                ctx.ui = Some(UiRequest::PlotWindow(Some(Aabb::from_points(a, b))));
                Ok(Step::Done)
            }
            (_, Input::Enter) => {
                ctx.ui = Some(UiRequest::PlotWindow(None));
                Ok(Step::Done)
            }
            _ => Err("Specify a point.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Some(a) = self.first else { return Vec::new() };
        let corners = [a, Point::new(cursor.x, a.y), cursor, Point::new(a.x, cursor.y)];
        vec![Geometry::Polyline { vertices: corners.into_iter().map(Vertex::at).collect(), closed: true }]
    }

    fn base_point(&self) -> Option<Point> {
        self.first
    }
}

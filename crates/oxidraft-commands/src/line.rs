use oxidraft_geom::Point;
use oxidraft_model::Geometry;

use crate::{Command, Ctx, Input, Prompt, Step};

/// LINE: a chain of independent line segments.
#[derive(Default)]
pub struct Line {
    points: Vec<Point>,
}

impl Line {
    fn add_point(&mut self, p: Point, ctx: &mut Ctx) -> Result<Step, String> {
        if let Some(&last) = self.points.last() {
            if last.approx_eq(p) {
                return Err("Zero-length line ignored.".into());
            }
            ctx.doc.add(Geometry::Line { start: last, end: p });
        }
        self.points.push(p);
        Ok(Step::Continue)
    }
}

impl Command for Line {
    fn prompt(&self) -> Prompt {
        match self.points.len() {
            0 => Prompt::new("Specify first point"),
            1 | 2 => Prompt::new("Specify next point").with_keywords(&["Undo"]),
            _ => Prompt::new("Specify next point").with_keywords(&["Close", "Undo"]),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => self.add_point(p, ctx),
            // Direct distance entry: go `d` units toward the cursor.
            Input::Value(d) => {
                let last = *self.points.last().ok_or("Point or option keyword required.")?;
                if last.approx_eq(ctx.cursor) {
                    return Err("Move the cursor to show a direction.".into());
                }
                let p = last.polar(d, last.angle_to(ctx.cursor));
                self.add_point(p, ctx)
            }
            Input::Keyword("Close") => {
                let (first, last) = (self.points[0], self.points[self.points.len() - 1]);
                ctx.doc.add(Geometry::Line { start: last, end: first });
                Ok(Step::Done)
            }
            Input::Keyword("Undo") => {
                if self.points.len() > 1 {
                    ctx.doc.undo_last_in_transaction();
                }
                self.points.pop();
                Ok(Step::Continue)
            }
            Input::Keyword(_) | Input::Selection(_) | Input::Pick { .. } | Input::Text(_) => {
                Err("Point or option keyword required.".into())
            }
            Input::Enter => Ok(Step::Done),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.points.last().map(|&start| Geometry::Line { start, end: cursor }).into_iter().collect()
    }

    fn base_point(&self) -> Option<Point> {
        self.points.last().copied()
    }
}

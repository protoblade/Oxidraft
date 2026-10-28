//! SPLINE: a smooth curve through fit points, or shaped by control
//! vertices (the CV method).

use oxidraft_geom::Point;
use oxidraft_model::{Geometry, Spline};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

/// SPLINE, by fit points or control vertices (SPLMETHOD).
#[derive(Default)]
pub struct SplineCmd {
    points: Vec<Point>,
    cv: bool,
    /// Asking for the method.
    method: bool,
}

impl SplineCmd {
    fn curve(&self, points: &[Point], closed: bool) -> Option<Spline> {
        if self.cv { Spline::from_control(points, closed) } else { Spline::through(points, closed) }
    }

    fn finish(&self, closed: bool, ctx: &mut Ctx) -> Result<Step, String> {
        if let Some(s) = self.curve(&self.points, closed) {
            ctx.doc.add(Geometry::Spline(s));
        }
        Ok(Step::Done)
    }
}

impl Command for SplineCmd {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.cv = vars.spline_cv;
        Some(format!("Current settings: Method={}   Knots=Chord", if self.cv { "CV" } else { "Fit" }))
    }

    fn prompt(&self) -> Prompt {
        if self.method {
            return Prompt::new("Enter spline creation method")
                .with_keywords(&["Fit", "CV"])
                .with_default(if self.cv { "CV" } else { "Fit" });
        }
        let what = if self.cv { "vertex" } else { "point" };
        match self.points.len() {
            0 => Prompt::new(format!("Specify first {what}")).with_keywords(&["Method"]),
            1 => Prompt::new(format!("Enter next {what}")).with_keywords(&["Undo"]),
            _ => Prompt::new(format!("Enter next {what}")).with_keywords(&["Close", "Undo"]),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if self.method {
            match input {
                Input::Keyword("CV") => self.cv = true,
                Input::Keyword("Fit") => self.cv = false,
                Input::Enter => {}
                _ => return Err("Choose Fit or CV.".into()),
            }
            ctx.vars.spline_cv = self.cv;
            self.method = false;
            return Ok(Step::Continue);
        }
        match input {
            Input::Point(p) => {
                if self.points.last().is_some_and(|q| q.approx_eq(p)) {
                    return Err("Point ignored: it repeats the last one.".into());
                }
                self.points.push(p);
                Ok(Step::Continue)
            }
            Input::Keyword("Method") if self.points.is_empty() => {
                self.method = true;
                Ok(Step::Continue)
            }
            Input::Keyword("Undo") => {
                self.points.pop();
                Ok(Step::Continue)
            }
            Input::Keyword("Close") if self.points.len() >= 2 => self.finish(true, ctx),
            Input::Enter => self.finish(false, ctx),
            _ => Err("Point or option keyword required.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        if self.points.is_empty() || self.method {
            return Vec::new();
        }
        let mut points = self.points.clone();
        points.push(cursor);
        let mut out = Vec::new();
        // The control polygon shows while drawing by control vertices.
        if self.cv {
            out.extend(points.windows(2).map(|w| Geometry::Line { start: w[0], end: w[1] }));
        }
        out.extend(self.curve(&points, false).map(Geometry::Spline));
        out
    }

    fn base_point(&self) -> Option<Point> {
        self.points.last().copied()
    }
}

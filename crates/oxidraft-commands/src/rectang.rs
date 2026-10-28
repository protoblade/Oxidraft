use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step};

enum Stage {
    First,
    Other,
    Length,
    Width {
        length: f64,
    },
    /// Dimensions given; the other corner only picks the quadrant.
    Quadrant {
        length: f64,
        width: f64,
    },
}

/// RECTANG: a closed four-vertex polyline from two corners or from dimensions.
pub struct Rectang {
    stage: Stage,
    first: Point,
}

impl Default for Rectang {
    fn default() -> Self {
        Self { stage: Stage::First, first: Point::ORIGIN }
    }
}

impl Rectang {
    fn rectangle(a: Point, c: Point) -> Geometry {
        let corners = [a, Point::new(c.x, a.y), c, Point::new(a.x, c.y)];
        Geometry::Polyline { vertices: corners.into_iter().map(Vertex::at).collect(), closed: true }
    }

    /// The corner opposite the first one, given dimensions and a point in the wanted quadrant.
    fn sized_corner(&self, length: f64, width: f64, toward: Point) -> Point {
        let sx = if toward.x < self.first.x { -1.0 } else { 1.0 };
        let sy = if toward.y < self.first.y { -1.0 } else { 1.0 };
        self.first + Point::new(sx * length, sy * width)
    }

    fn finish(&self, other: Point, ctx: &mut Ctx) -> Result<Step, String> {
        let d = other - self.first;
        if d.x.abs() <= EPSILON || d.y.abs() <= EPSILON {
            return Err("The rectangle needs a nonzero width and height.".into());
        }
        ctx.doc.add(Self::rectangle(self.first, other));
        Ok(Step::Done)
    }
}

impl Command for Rectang {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::First => Prompt::new("Specify first corner point"),
            Stage::Other => Prompt::new("Specify other corner point").with_keywords(&["Dimensions"]),
            Stage::Length => Prompt::new("Specify length for rectangles").with_default("10.0000"),
            Stage::Width { .. } => Prompt::new("Specify width for rectangles").with_default("10.0000"),
            Stage::Quadrant { .. } => Prompt::new("Specify other corner point"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let positive = |v: f64| if v > EPSILON { Ok(v) } else { Err("Requires a positive nonzero value.".to_string()) };
        match (&self.stage, input) {
            (Stage::First, Input::Point(p)) => {
                self.first = p;
                self.stage = Stage::Other;
            }
            (Stage::Other, Input::Point(p)) => return self.finish(p, ctx),
            (Stage::Other, Input::Keyword("Dimensions")) => self.stage = Stage::Length,
            (Stage::Length, Input::Value(v)) => self.stage = Stage::Width { length: positive(v)? },
            (Stage::Length, Input::Enter) => self.stage = Stage::Width { length: 10.0 },
            (Stage::Width { length }, Input::Value(v)) => {
                self.stage = Stage::Quadrant { length: *length, width: positive(v)? }
            }
            (Stage::Width { length }, Input::Enter) => self.stage = Stage::Quadrant { length: *length, width: 10.0 },
            (Stage::Quadrant { length, width }, Input::Point(p)) => {
                let corner = self.sized_corner(*length, *width, p);
                return self.finish(corner, ctx);
            }
            (Stage::First | Stage::Other, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            Stage::Other => vec![Self::rectangle(self.first, cursor)],
            Stage::Quadrant { length, width } => {
                vec![Self::rectangle(self.first, self.sized_corner(length, width, cursor))]
            }
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Other | Stage::Quadrant { .. } => Some(self.first),
            _ => None,
        }
    }
}

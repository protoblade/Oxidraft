//! POLYGON: regular polygons as closed polylines, inscribed in or
//! circumscribed about a circle, or built on an edge.

use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

const INSCRIBED: &str = "Inscribed in circle";
const CIRCUMSCRIBED: &str = "Circumscribed about circle";

#[derive(Clone, Copy)]
enum Stage {
    Sides,
    Center,
    Kind { center: Point },
    Radius { center: Point },
    EdgeFirst,
    EdgeSecond { first: Point },
}

/// POLYGON, remembering the number of sides (POLYSIDES) and whether it was
/// inscribed.
pub struct Polygon {
    stage: Stage,
    sides: usize,
    inscribed: bool,
}

impl Default for Polygon {
    fn default() -> Self {
        Self { stage: Stage::Sides, sides: 4, inscribed: true }
    }
}

fn closed(points: Vec<Point>) -> Geometry {
    Geometry::Polyline { vertices: points.into_iter().map(Vertex::at).collect(), closed: true }
}

impl Polygon {
    /// The polygon around `center` with a vertex (inscribed) or the middle
    /// of an edge (circumscribed) toward `angle`, that far from the center.
    fn around(&self, center: Point, radius: f64, angle: f64) -> Geometry {
        let step = 360.0 / self.sides as f64;
        let (r, first) = if self.inscribed {
            (radius, angle)
        } else {
            (radius / (step / 2.0).to_radians().cos(), angle + step / 2.0)
        };
        closed((0..self.sides).map(|k| center.polar(r, first + step * k as f64)).collect())
    }

    /// The polygon whose first edge runs from `a` to `b`, the rest on its left.
    fn on_edge(&self, a: Point, b: Point) -> Geometry {
        let (side, mut heading) = (a.distance(b), a.angle_to(b));
        let mut points = vec![a];
        for _ in 1..self.sides {
            let last = points[points.len() - 1];
            points.push(last.polar(side, heading));
            heading += 360.0 / self.sides as f64;
        }
        closed(points)
    }

    /// Typed radii put the bottom edge level, as in AutoCAD.
    fn level(&self, center: Point, radius: f64) -> Geometry {
        let half = 180.0 / self.sides as f64;
        self.around(center, radius, if self.inscribed { 270.0 - half } else { 270.0 })
    }
}

impl Command for Polygon {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        (self.sides, self.inscribed) = (vars.polygon_sides, vars.polygon_inscribed);
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Sides => Prompt::new("Enter number of sides").with_default(self.sides.to_string()),
            Stage::Center => Prompt::new("Specify center of polygon").with_keywords(&["Edge"]),
            Stage::Kind { .. } => Prompt::new("Enter an option")
                .with_keywords(&[INSCRIBED, CIRCUMSCRIBED])
                .with_default(if self.inscribed { "I" } else { "C" }),
            Stage::Radius { .. } => Prompt::new("Specify radius of circle"),
            Stage::EdgeFirst => Prompt::new("Specify first endpoint of edge"),
            Stage::EdgeSecond { .. } => Prompt::new("Specify second endpoint of edge"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::Sides, Input::Value(v)) => {
                if v.fract() != 0.0 || !(3.0..=1024.0).contains(&v) {
                    return Err("Enter an integer between 3 and 1024.".into());
                }
                self.sides = v as usize;
                ctx.vars.polygon_sides = self.sides;
                self.stage = Stage::Center;
            }
            (Stage::Sides, Input::Enter) => self.stage = Stage::Center,
            (Stage::Center, Input::Point(center)) => self.stage = Stage::Kind { center },
            (Stage::Center, Input::Keyword("Edge")) => self.stage = Stage::EdgeFirst,
            (Stage::Kind { center }, input @ (Input::Keyword(_) | Input::Enter)) => {
                if let Input::Keyword(k) = input {
                    self.inscribed = k == INSCRIBED;
                }
                ctx.vars.polygon_inscribed = self.inscribed;
                self.stage = Stage::Radius { center };
            }
            (Stage::Radius { center }, Input::Point(p)) => {
                if center.distance(p) <= EPSILON {
                    return Err("Value must be positive and nonzero.".into());
                }
                ctx.doc.add(self.around(center, center.distance(p), center.angle_to(p)));
                return Ok(Step::Done);
            }
            (Stage::Radius { center }, Input::Value(r)) if r > EPSILON => {
                ctx.doc.add(self.level(center, r));
                return Ok(Step::Done);
            }
            (Stage::EdgeFirst, Input::Point(first)) => self.stage = Stage::EdgeSecond { first },
            (Stage::EdgeSecond { first }, Input::Point(p)) if !p.approx_eq(first) => {
                ctx.doc.add(self.on_edge(first, p));
                return Ok(Step::Done);
            }
            (Stage::Sides, _) => return Err("Requires an integer between 3 and 1024.".into()),
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Radius { .. }, _) => return Err("Value must be positive and nonzero.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            Stage::Radius { center } if center.distance(cursor) > EPSILON => vec![
                Geometry::Line { start: center, end: cursor },
                self.around(center, center.distance(cursor), center.angle_to(cursor)),
            ],
            Stage::EdgeSecond { first } if !first.approx_eq(cursor) => vec![self.on_edge(first, cursor)],
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Radius { center } => Some(center),
            Stage::EdgeSecond { first } => Some(first),
            _ => None,
        }
    }
}

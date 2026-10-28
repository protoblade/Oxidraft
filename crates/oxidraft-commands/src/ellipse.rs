//! ELLIPSE: ellipses by an axis and the other half-axis (or a rotation),
//! from an axis endpoint or the center, and elliptical arcs.

use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Ellipse, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Clone, Copy)]
enum Stage {
    First,
    Center,
    /// The second axis point: the other endpoint (`from_center` false) or,
    /// after Center, the endpoint.
    AxisEnd {
        p1: Point,
        from_center: bool,
    },
    /// `axis` runs from the center to the end of the first axis.
    Distance {
        center: Point,
        axis: Point,
    },
    Rotation {
        center: Point,
        axis: Point,
    },
    /// Elliptical arcs: `first` is the direction of the first axis, which
    /// typed angles count from.
    Start {
        e: Ellipse,
        first: f64,
        parameter: bool,
    },
    End {
        e: Ellipse,
        first: f64,
        start: f64,
        mode: EndMode,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum EndMode {
    Angle,
    Parameter,
    Included,
}

/// The ellipse with one half-axis from `center` to `axis` and the other
/// `other` long; the longer one is the major axis.
fn ellipse_from(center: Point, axis: Point, other: f64) -> Option<Ellipse> {
    let h = axis.length();
    if h <= EPSILON || other <= EPSILON {
        return None;
    }
    Some(if other <= h {
        Ellipse::full(center, axis, other / h)
    } else {
        Ellipse::full(center, Point::new(-axis.y, axis.x) * (other / h), h / other)
    })
}

/// ELLIPSE, with its Arc and Center options.
pub struct EllipseCmd {
    stage: Stage,
    arc: bool,
}

impl EllipseCmd {
    pub fn full() -> Self {
        Self { stage: Stage::First, arc: false }
    }

    /// The ellipse once its shape is known: added, or on to its arc angles.
    fn shaped(&mut self, center: Point, axis: Point, other: f64, ctx: &mut Ctx) -> Result<Step, String> {
        let e = ellipse_from(center, axis, other).ok_or("Value must be positive and nonzero.")?;
        if !self.arc {
            ctx.doc.add(Geometry::Ellipse(e));
            return Ok(Step::Done);
        }
        self.stage = Stage::Start { e, first: Point::ORIGIN.angle_to(axis), parameter: false };
        Ok(Step::Continue)
    }

    /// A start or end given as an angle (from the first axis) or a point.
    fn param(e: &Ellipse, first: f64, parameter: bool, input: &Input) -> Option<f64> {
        match (input, parameter) {
            (Input::Value(v), false) => Some(e.param_at_angle(first + v)),
            (Input::Value(v), true) => Some(v.rem_euclid(360.0)),
            (Input::Point(p), false) => Some(e.param_at_angle(e.center.angle_to(*p))),
            (Input::Point(p), true) => Some(e.param_of(*p)),
            _ => None,
        }
    }

    fn end_param(e: &Ellipse, first: f64, start: f64, mode: EndMode, input: &Input) -> Option<f64> {
        match mode {
            EndMode::Included => {
                let from = e.center.angle_to(e.point_at(start));
                match input {
                    Input::Value(v) => Some(e.param_at_angle(from + v)),
                    Input::Enter => Some(e.param_at_angle(from + 180.0)),
                    Input::Point(p) => Some(e.param_at_angle(e.center.angle_to(*p))),
                    _ => None,
                }
            }
            _ => Self::param(e, first, mode == EndMode::Parameter, input),
        }
    }
}

impl Command for EllipseCmd {
    fn prompt(&self) -> Prompt {
        let what = if self.arc { "elliptical arc" } else { "ellipse" };
        match self.stage {
            Stage::First if self.arc => {
                Prompt::new(format!("Specify axis endpoint of {what}")).with_keywords(&["Center"])
            }
            Stage::First => Prompt::new(format!("Specify axis endpoint of {what}")).with_keywords(&["Arc", "Center"]),
            Stage::Center => Prompt::new(format!("Specify center of {what}")),
            Stage::AxisEnd { from_center: false, .. } => Prompt::new("Specify other endpoint of axis"),
            Stage::AxisEnd { from_center: true, .. } => Prompt::new("Specify endpoint of axis"),
            Stage::Distance { .. } => Prompt::new("Specify distance to other axis").with_keywords(&["Rotation"]),
            Stage::Rotation { .. } => Prompt::new("Specify rotation around major axis"),
            Stage::Start { parameter: false, .. } => Prompt::new("Specify start angle").with_keywords(&["Parameter"]),
            Stage::Start { parameter: true, .. } => Prompt::new("Specify start parameter").with_keywords(&["Angle"]),
            Stage::End { mode: EndMode::Angle, .. } => {
                Prompt::new("Specify end angle").with_keywords(&["Parameter", "Included angle"])
            }
            Stage::End { mode: EndMode::Parameter, .. } => {
                Prompt::new("Specify end parameter").with_keywords(&["Angle", "Included angle"])
            }
            Stage::End { mode: EndMode::Included, .. } => {
                Prompt::new("Specify included angle for arc").with_default("180")
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::First, Input::Keyword("Arc")) => self.arc = true,
            (Stage::First, Input::Keyword("Center")) => self.stage = Stage::Center,
            (Stage::First, Input::Point(p1)) => self.stage = Stage::AxisEnd { p1, from_center: false },
            (Stage::Center, Input::Point(center)) => self.stage = Stage::AxisEnd { p1: center, from_center: true },
            (Stage::AxisEnd { p1, from_center }, Input::Point(p2)) => {
                if p1.approx_eq(p2) {
                    return Err("Invalid axis.".into());
                }
                let center = if from_center { p1 } else { p1.midpoint(p2) };
                self.stage = Stage::Distance { center, axis: p2 - center };
            }
            (Stage::Distance { center, axis }, Input::Point(p)) => {
                return self.shaped(center, axis, center.distance(p), ctx);
            }
            (Stage::Distance { center, axis }, Input::Value(d)) => return self.shaped(center, axis, d, ctx),
            (Stage::Distance { center, axis }, Input::Keyword("Rotation")) => {
                self.stage = Stage::Rotation { center, axis };
            }
            // The ellipse is a circle tilted this far around the first axis.
            (Stage::Rotation { center, axis }, input @ (Input::Value(_) | Input::Point(_))) => {
                let angle = match input {
                    Input::Point(p) => center.angle_to(p),
                    Input::Value(v) => v,
                    _ => unreachable!(),
                };
                let ratio = angle.to_radians().cos().abs();
                if ratio < 0.01 {
                    return Err("Invalid rotation: the ellipse would be flat.".into());
                }
                return self.shaped(center, axis, axis.length() * ratio, ctx);
            }
            (Stage::Start { e, first, parameter }, Input::Keyword("Parameter" | "Angle")) => {
                self.stage = Stage::Start { e, first, parameter: !parameter };
            }
            (Stage::Start { e, first, parameter }, input) if Self::param(&e, first, parameter, &input).is_some() => {
                let start = Self::param(&e, first, parameter, &input).unwrap_or(0.0);
                let mode = if parameter { EndMode::Parameter } else { EndMode::Angle };
                self.stage = Stage::End { e, first, start, mode };
            }
            (Stage::End { e, first, start, .. }, Input::Keyword(k @ ("Parameter" | "Angle" | "Included angle"))) => {
                let mode = match k {
                    "Parameter" => EndMode::Parameter,
                    "Angle" => EndMode::Angle,
                    _ => EndMode::Included,
                };
                self.stage = Stage::End { e, first, start, mode };
            }
            (Stage::End { e, first, start, mode }, input) => {
                if mode != EndMode::Included && input == Input::Enter {
                    return Ok(Step::Done);
                }
                let end = Self::end_param(&e, first, start, mode, &input).ok_or("Requires an angle or a point.")?;
                if (end - start).rem_euclid(360.0) < 1e-9 {
                    return Err("The end must differ from the start.".into());
                }
                ctx.doc.add(Geometry::Ellipse(e.arc(start, end)));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Distance { .. } | Stage::Rotation { .. }, _) => {
                return Err("Requires a distance or a point.".into());
            }
            (Stage::Start { .. }, _) => return Err("Requires an angle or a point.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            Stage::AxisEnd { p1, .. } => vec![Geometry::Line { start: p1, end: cursor }],
            Stage::Distance { center, axis } => {
                let mut out = vec![Geometry::Line { start: center, end: cursor }];
                out.extend(ellipse_from(center, axis, center.distance(cursor)).map(Geometry::Ellipse));
                out
            }
            Stage::Start { e, .. } => {
                vec![Geometry::Ellipse(e), Geometry::Line { start: e.center, end: cursor }]
            }
            Stage::End { e, first, start, mode } => {
                let end = Self::end_param(&e, first, start, mode, &Input::Point(cursor)).unwrap_or(start);
                let mut out = vec![Geometry::Line { start: e.center, end: cursor }];
                if (end - start).rem_euclid(360.0) > 1e-9 {
                    out.push(Geometry::Ellipse(e.arc(start, end)));
                }
                out
            }
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::AxisEnd { p1, .. } => Some(p1),
            Stage::Distance { center, .. } | Stage::Rotation { center, .. } => Some(center),
            Stage::Start { e, .. } | Stage::End { e, .. } => Some(e.center),
            _ => None,
        }
    }
}

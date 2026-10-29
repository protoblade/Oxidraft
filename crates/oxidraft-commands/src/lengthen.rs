use oxidraft_geom::{
    EPSILON, Point,
    curve::{ArcSeg, Seg},
};
use oxidraft_model::{EntityId, Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step};

/// Lines, arcs and open polylines have a length LENGTHEN can change.
fn changeable(g: &Geometry) -> bool {
    matches!(g, Geometry::Line { .. } | Geometry::Arc { .. } | Geometry::Polyline { closed: false, .. })
}

/// True if `pick` is nearer the curve's end than its start.
fn near_end(g: &Geometry, pick: Point) -> bool {
    let segs = g.segments();
    let (start, end) = (segs[0].start(), segs[segs.len() - 1].end());
    pick.distance(end) <= pick.distance(start)
}

/// The curve with its end nearest `pick` moved so its total length is
/// `length`. Arcs keep their radius; a polyline changes only its end segment.
pub(crate) fn lengthen_to(g: &Geometry, pick: Point, length: f64) -> Result<Geometry, String> {
    if length <= EPSILON {
        return Err("Value must be positive and nonzero.".into());
    }
    let at_end = near_end(g, pick);
    match g {
        Geometry::Line { start, end } => {
            let (fixed, moving) = if at_end { (*start, *end) } else { (*end, *start) };
            let d = moving - fixed;
            let moved = fixed + d * (length / d.length().max(EPSILON));
            Ok(if at_end {
                Geometry::Line { start: fixed, end: moved }
            } else {
                Geometry::Line { start: moved, end: fixed }
            })
        }
        Geometry::Arc { center, radius, start_angle, end_angle } => {
            let a = ArcSeg::ccw(*center, *radius, *start_angle, *end_angle);
            let sweep = (length / radius).to_degrees();
            if sweep >= 360.0 - 1e-9 {
                return Err("The arc can't reach 360 degrees.".into());
            }
            let start = if at_end { a.start } else { a.start + a.sweep - sweep };
            Ok(Geometry::arc(&ArcSeg { start, sweep, ..a }))
        }
        Geometry::Polyline { vertices, closed: false } => {
            let segs = g.segments();
            let (i, seg) = if at_end { (segs.len() - 1, segs[segs.len() - 1]) } else { (0, segs[0]) };
            let Seg::Line(l) = seg else {
                return Err("Only a polyline whose end segment is straight can be lengthened.".into());
            };
            let seg_length = l.length() + length - g.length();
            if seg_length <= EPSILON {
                return Err("The end segment would vanish.".into());
            }
            let mut vertices: Vec<Vertex> = vertices.clone();
            if at_end {
                let moved = l.start + (l.end - l.start) * (seg_length / l.length());
                vertices[i + 1].point = moved;
            } else {
                vertices[0].point = l.end + (l.start - l.end) * (seg_length / l.length());
            }
            Ok(Geometry::Polyline { vertices, closed: false })
        }
        _ => Err(format!("A {} can't be lengthened.", g.name())),
    }
}

/// The length the curve gets if its end nearest `pick` is dragged to `p`.
fn dynamic_length(g: &Geometry, pick: Point, p: Point) -> Option<f64> {
    let at_end = near_end(g, pick);
    let segs = g.segments();
    match (g, if at_end { segs[segs.len() - 1] } else { segs[0] }) {
        (Geometry::Arc { .. }, Seg::Arc(a)) => {
            let angle = a.center.angle_to(p);
            let sweep = if at_end {
                (angle - a.start).rem_euclid(360.0)
            } else {
                (a.start + a.sweep - angle).rem_euclid(360.0)
            };
            Some(a.radius * sweep.to_radians())
        }
        (_, Seg::Line(l)) => {
            let (fixed, moving) = if at_end { (l.start, l.end) } else { (l.end, l.start) };
            let u = (moving - fixed) * (1.0 / l.length().max(EPSILON));
            Some(g.length() - l.length() + (p - fixed).dot(u))
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Delta,
    Percent,
    Total,
    Dynamic,
}

#[derive(Clone)]
enum Stage {
    Measure,
    /// Waiting for the delta, percentage or total.
    Amount(Mode),
    Change(Mode),
    DynamicEnd {
        id: EntityId,
        at: Point,
        geometry: Geometry,
    },
}

/// LENGTHEN: shows an object's length, or changes the length of lines,
/// arcs and open polylines by a delta, a percentage, to a total, or by
/// dragging the end.
pub struct Lengthen {
    stage: Stage,
    delta: f64,
    percent: f64,
    total: f64,
}

impl Default for Lengthen {
    fn default() -> Self {
        Self { stage: Stage::Measure, delta: 0.0, percent: 100.0, total: 1.0 }
    }
}

impl Lengthen {
    fn pick(ctx: &Ctx, at: Point) -> Result<(EntityId, Geometry), String> {
        let id = ctx.pick(at).ok_or("No object found.")?;
        let g = ctx.doc.entity(id).map(|e| e.geometry.clone()).ok_or("No object found.")?;
        Ok((id, g))
    }

    fn new_length(&self, mode: Mode, length: f64) -> f64 {
        match mode {
            Mode::Delta => length + self.delta,
            Mode::Percent => length * self.percent / 100.0,
            Mode::Total | Mode::Dynamic => self.total,
        }
    }
}

impl Command for Lengthen {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            Stage::Measure => Prompt::new("Select an object to measure")
                .with_keywords(&["DElta", "Percent", "Total", "DYnamic"])
                .with_default("Total"),
            Stage::Amount(Mode::Delta) => Prompt::new("Enter delta length").with_default(format!("{:.4}", self.delta)),
            Stage::Amount(Mode::Percent) => {
                Prompt::new("Enter percentage length").with_default(format!("{:.4}", self.percent))
            }
            Stage::Amount(_) => Prompt::new("Specify total length").with_default(format!("{:.4}", self.total)),
            Stage::Change(_) => Prompt::new("Select an object to change").with_keywords(&["Undo"]),
            Stage::DynamicEnd { .. } => Prompt::new("Specify new end point"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (Stage::Measure, Input::Pick { at, .. }) => {
                let (_, g) = Self::pick(ctx, at)?;
                let message = match &g {
                    Geometry::Arc { start_angle, end_angle, .. } => format!(
                        "Current length: {:.4}, included angle: {:.4}",
                        g.length(),
                        (end_angle - start_angle).rem_euclid(360.0)
                    ),
                    _ if !g.segments().is_empty() => format!("Current length: {:.4}", g.length()),
                    _ => return Err(format!("A {} has no length.", g.name())),
                };
                ctx.messages.push(message);
            }
            (Stage::Measure, Input::Keyword(k)) => {
                self.stage = match k {
                    "DElta" => Stage::Amount(Mode::Delta),
                    "Percent" => Stage::Amount(Mode::Percent),
                    "DYnamic" => Stage::Change(Mode::Dynamic),
                    _ => Stage::Amount(Mode::Total),
                };
            }
            (Stage::Measure, Input::Enter) => self.stage = Stage::Amount(Mode::Total),
            (Stage::Amount(mode), input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(v) = input {
                    match mode {
                        Mode::Delta => self.delta = v,
                        Mode::Percent | Mode::Total if v <= 0.0 => {
                            return Err("Value must be positive and nonzero.".into());
                        }
                        Mode::Percent => self.percent = v,
                        _ => self.total = v,
                    }
                }
                self.stage = Stage::Change(mode);
            }
            (Stage::Change(mode), Input::Pick { at, .. }) => {
                let (id, g) = Self::pick(ctx, at)?;
                if !changeable(&g) {
                    return Err(format!("A {} can't be lengthened.", g.name()));
                }
                if mode == Mode::Dynamic {
                    self.stage = Stage::DynamicEnd { id, at, geometry: g };
                } else {
                    let changed = lengthen_to(&g, at, self.new_length(mode, g.length()))?;
                    ctx.doc.modify(id, changed);
                }
            }
            (Stage::Change(_), Input::Keyword("Undo")) => {
                ctx.doc.undo_last_in_transaction();
            }
            (Stage::DynamicEnd { id, at, geometry }, Input::Point(p)) => {
                let length = dynamic_length(&geometry, at, p).ok_or("Can't drag that end.")?;
                ctx.doc.modify(id, lengthen_to(&geometry, at, length)?);
                self.stage = Stage::Change(Mode::Dynamic);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Amount(_), _) => return Err("Requires a number.".into()),
            _ => return Err("Select an object or an option keyword.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Stage::DynamicEnd { at, geometry, .. } = &self.stage else { return Vec::new() };
        dynamic_length(geometry, *at, cursor).and_then(|l| lengthen_to(geometry, *at, l).ok()).into_iter().collect()
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match &self.stage {
            Stage::DynamicEnd { id, .. } => vec![*id],
            _ => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::Measure | Stage::Change(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengthen_lines_arcs_and_polylines_at_the_picked_end() {
        let line = Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) };
        assert_eq!(
            lengthen_to(&line, Point::new(9.0, 0.0), 15.0).unwrap(),
            Geometry::Line { start: Point::ORIGIN, end: Point::new(15.0, 0.0) }
        );
        assert_eq!(
            lengthen_to(&line, Point::new(1.0, 0.0), 4.0).unwrap(),
            Geometry::Line { start: Point::new(6.0, 0.0), end: Point::new(10.0, 0.0) }
        );
        assert!(lengthen_to(&line, Point::new(1.0, 0.0), 0.0).is_err());

        let r = 10.0;
        let arc = Geometry::Arc { center: Point::ORIGIN, radius: r, start_angle: 0.0, end_angle: 90.0 };
        let quarter = std::f64::consts::FRAC_PI_2 * r;
        let Geometry::Arc { start_angle, end_angle, .. } =
            lengthen_to(&arc, Point::new(0.0, 10.0), quarter * 2.0).unwrap()
        else {
            panic!()
        };
        assert!(start_angle.abs() < 1e-9 && (end_angle - 180.0).abs() < 1e-9);

        let pl = Geometry::Polyline {
            vertices: vec![
                Vertex::at(Point::ORIGIN),
                Vertex::at(Point::new(10.0, 0.0)),
                Vertex::at(Point::new(10.0, 5.0)),
            ],
            closed: false,
        };
        let Geometry::Polyline { vertices, .. } = lengthen_to(&pl, Point::new(10.0, 4.0), 20.0).unwrap() else {
            panic!()
        };
        assert!(vertices[2].point.approx_eq(Point::new(10.0, 10.0)));
        // Dragging the end of a line to x = 12 makes it 12 long.
        assert!((dynamic_length(&line, Point::new(9.0, 0.0), Point::new(12.0, 3.0)).unwrap() - 12.0).abs() < 1e-9);
    }
}

//! XLINE and RAY: construction lines, infinite both ways or one way.

use oxidraft_geom::{Point, curve::Seg};
use oxidraft_model::{Geometry, snap};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

#[derive(Clone, Copy)]
enum Stage {
    First,
    /// Through points for lines from `base`, until Enter.
    Through {
        base: Point,
    },
    /// Hor, Ver and Ang: through points for lines at a fixed angle.
    Fixed {
        angle: f64,
    },
    Angle,
    BisectVertex,
    BisectStart {
        vertex: Point,
    },
    BisectEnd {
        vertex: Point,
        start: Point,
    },
    OffsetDistance,
    OffsetPick,
    /// The picked line, as a point on it and its direction.
    OffsetSide {
        base: Point,
        direction: Point,
    },
}

/// XLINE (`ray` false) and RAY.
pub struct ConstructionLine {
    stage: Stage,
    ray: bool,
    /// Offset: a distance, or `None` for Through.
    offset: Option<f64>,
}

impl ConstructionLine {
    pub fn xline() -> Self {
        Self { stage: Stage::First, ray: false, offset: None }
    }

    pub fn ray() -> Self {
        Self { stage: Stage::First, ray: true, offset: None }
    }

    fn line(&self, base: Point, direction: Point) -> Geometry {
        if self.ray { Geometry::Ray { base, direction } } else { Geometry::Xline { base, direction } }
    }

    /// The line a through point makes at this stage.
    fn through(&self, p: Point) -> Option<Geometry> {
        match self.stage {
            Stage::Through { base } if !p.approx_eq(base) => Some(self.line(base, p - base)),
            Stage::Fixed { angle } => Some(Geometry::Xline { base: p, direction: Point::ORIGIN.polar(1.0, angle) }),
            Stage::BisectEnd { vertex, start } if !p.approx_eq(vertex) => {
                let (a, b) = (vertex.angle_to(start), vertex.angle_to(p));
                let half = (b - a).rem_euclid(360.0) / 2.0;
                Some(Geometry::Xline { base: vertex, direction: Point::ORIGIN.polar(1.0, a + half) })
            }
            Stage::OffsetSide { base, direction } => {
                let len = direction.length();
                let left = Point::new(-direction.y, direction.x) * (1.0 / len);
                let side = (p - base).dot(left);
                let shift = match self.offset {
                    Some(d) => left * (d * side.signum()),
                    None => left * side,
                };
                Some(Geometry::Xline { base: base + shift, direction })
            }
            _ => None,
        }
    }

    /// The straight line picked for Offset: a point on it and its direction.
    fn picked_line(ctx: &Ctx, at: Point) -> Option<(Point, Point)> {
        let g = &ctx.doc.entity(snap::pick(ctx.doc, at, ctx.pick_tolerance)?)?.geometry;
        match g {
            Geometry::Line { start, end } => Some((*start, *end - *start)),
            Geometry::Xline { base, direction } | Geometry::Ray { base, direction } => Some((*base, *direction)),
            Geometry::Polyline { .. } => {
                let segs = g.segments();
                let nearest = segs.iter().min_by(|a, b| a.distance_to(at).total_cmp(&b.distance_to(at)))?;
                match nearest {
                    Seg::Line(s) => Some((s.start, s.end - s.start)),
                    Seg::Arc(_) => None,
                }
            }
            _ => None,
        }
    }
}

impl Command for ConstructionLine {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.offset = vars.offset_distance;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::First if self.ray => Prompt::new("Specify start point"),
            Stage::First => Prompt::new("Specify a point").with_keywords(&["Hor", "Ver", "Ang", "Bisect", "Offset"]),
            Stage::Through { .. } | Stage::Fixed { .. } => Prompt::new("Specify through point"),
            Stage::Angle => Prompt::new("Enter angle of xline").with_default("0"),
            Stage::BisectVertex => Prompt::new("Specify angle vertex point"),
            Stage::BisectStart { .. } => Prompt::new("Specify angle start point"),
            Stage::BisectEnd { .. } => Prompt::new("Specify angle end point"),
            Stage::OffsetDistance => Prompt::new("Specify offset distance")
                .with_keywords(&["Through"])
                .with_default(self.offset.map_or("Through".into(), |d| format!("{d:.4}"))),
            Stage::OffsetPick => Prompt::new("Select a line object"),
            Stage::OffsetSide { .. } if self.offset.is_none() => Prompt::new("Specify through point"),
            Stage::OffsetSide { .. } => Prompt::new("Specify side to offset"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::First, Input::Point(base)) => self.stage = Stage::Through { base },
            (Stage::First, Input::Keyword("Hor")) => self.stage = Stage::Fixed { angle: 0.0 },
            (Stage::First, Input::Keyword("Ver")) => self.stage = Stage::Fixed { angle: 90.0 },
            (Stage::First, Input::Keyword("Ang")) => self.stage = Stage::Angle,
            (Stage::First, Input::Keyword("Bisect")) => self.stage = Stage::BisectVertex,
            (Stage::First, Input::Keyword("Offset")) => self.stage = Stage::OffsetDistance,
            (Stage::Angle, Input::Value(angle)) => self.stage = Stage::Fixed { angle },
            (Stage::Angle, Input::Enter) => self.stage = Stage::Fixed { angle: 0.0 },
            (Stage::BisectVertex, Input::Point(vertex)) => self.stage = Stage::BisectStart { vertex },
            (Stage::BisectStart { vertex }, Input::Point(start)) if !start.approx_eq(vertex) => {
                self.stage = Stage::BisectEnd { vertex, start };
            }
            (Stage::OffsetDistance, Input::Value(d)) if d > 0.0 => {
                self.offset = Some(d);
                ctx.vars.offset_distance = Some(d);
                self.stage = Stage::OffsetPick;
            }
            (Stage::OffsetDistance, Input::Keyword("Through")) => {
                self.offset = None;
                ctx.vars.offset_distance = None;
                self.stage = Stage::OffsetPick;
            }
            (Stage::OffsetDistance, Input::Enter) => self.stage = Stage::OffsetPick,
            (Stage::OffsetPick, Input::Pick { at, .. }) => {
                let (base, direction) =
                    Self::picked_line(ctx, at).ok_or("Select a line, xline, ray or straight polyline segment.")?;
                self.stage = Stage::OffsetSide { base, direction };
            }
            (Stage::OffsetSide { .. }, Input::Point(p)) => {
                if let Some(g) = self.through(p) {
                    ctx.doc.add(g);
                }
                self.stage = Stage::OffsetPick;
            }
            (Stage::Through { .. } | Stage::Fixed { .. } | Stage::BisectEnd { .. }, Input::Point(p)) => {
                let g = self.through(p).ok_or("Specify a point away from the first one.")?;
                ctx.doc.add(g);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Angle, _) => return Err("Requires an angle.".into()),
            (Stage::OffsetDistance, _) => return Err("Requires a positive distance or Through.".into()),
            (Stage::OffsetPick, _) => return Err("Select a line object, or press Enter to exit.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            Stage::BisectStart { vertex } => vec![Geometry::Line { start: vertex, end: cursor }],
            _ => self.through(cursor).into_iter().collect(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Through { base } => Some(base),
            Stage::BisectStart { vertex } | Stage::BisectEnd { vertex, .. } => Some(vertex),
            _ => None,
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::OffsetPick)
    }
}

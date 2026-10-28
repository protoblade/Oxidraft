use oxidraft_geom::{Point, curve::ArcSeg};
use oxidraft_model::Geometry;

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Default)]
enum Stage {
    #[default]
    Start,
    /// Three-point arc: start given, waiting for the second point.
    Second {
        start: Point,
    },
    ThreePointEnd {
        start: Point,
        second: Point,
    },
    /// Center option, from the first prompt.
    CenterFirst,
    /// Center option after the start point.
    CenterAfterStart {
        start: Point,
    },
    StartAfterCenter {
        center: Point,
    },
    End {
        center: Point,
        start: Point,
    },
    Angle {
        center: Point,
        start: Point,
    },
}

/// ARC: three points (the default) or Center, Start, End / included Angle.
#[derive(Default)]
pub struct Arc {
    stage: Stage,
}

impl Arc {
    fn center_arc(center: Point, start: Point, end: Point) -> Option<ArcSeg> {
        let radius = center.distance(start);
        (radius > 0.0).then(|| ArcSeg::ccw(center, radius, center.angle_to(start), center.angle_to(end)))
    }

    fn add(arc: Option<ArcSeg>, ctx: &mut Ctx) -> Result<Step, String> {
        let arc = arc.ok_or("Invalid arc: the points are collinear or coincident.")?;
        ctx.doc.add(Geometry::arc(&arc));
        Ok(Step::Done)
    }
}

impl Command for Arc {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Start => Prompt::new("Specify start point of arc").with_keywords(&["Center"]),
            Stage::Second { .. } => Prompt::new("Specify second point of arc").with_keywords(&["Center"]),
            Stage::ThreePointEnd { .. } => Prompt::new("Specify end point of arc"),
            Stage::CenterFirst | Stage::CenterAfterStart { .. } => Prompt::new("Specify center point of arc"),
            Stage::StartAfterCenter { .. } => Prompt::new("Specify start point of arc"),
            Stage::End { .. } => Prompt::new("Specify end point of arc").with_keywords(&["Angle"]),
            Stage::Angle { .. } => Prompt::new("Specify included angle"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let next = match (&self.stage, input) {
            (Stage::Start, Input::Point(p)) => Stage::Second { start: p },
            (Stage::Start, Input::Keyword("Center")) => Stage::CenterFirst,
            (Stage::Second { start }, Input::Point(p)) => Stage::ThreePointEnd { start: *start, second: p },
            (Stage::Second { start }, Input::Keyword("Center")) => Stage::CenterAfterStart { start: *start },
            (Stage::ThreePointEnd { start, second }, Input::Point(p)) => {
                return Self::add(ArcSeg::through(*start, *second, p), ctx);
            }
            (Stage::CenterFirst, Input::Point(p)) => Stage::StartAfterCenter { center: p },
            (Stage::CenterAfterStart { start }, Input::Point(p)) => Stage::End { center: p, start: *start },
            (Stage::StartAfterCenter { center }, Input::Point(p)) => Stage::End { center: *center, start: p },
            (Stage::End { center, start }, Input::Point(p)) => {
                return Self::add(Self::center_arc(*center, *start, p), ctx);
            }
            (Stage::End { center, start }, Input::Keyword("Angle")) => Stage::Angle { center: *center, start: *start },
            (Stage::Angle { center, start }, Input::Value(a)) if a.abs() > 1e-9 && a.abs() < 360.0 => {
                let radius = center.distance(*start);
                let arc = ArcSeg { center: *center, radius, start: center.angle_to(*start), sweep: a };
                return Self::add(Some(arc), ctx);
            }
            (Stage::Angle { .. }, _) => return Err("Requires an angle between -360 and 360, not 0.".into()),
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Point or option keyword required.".into()),
        };
        self.stage = next;
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let arc = match self.stage {
            Stage::Second { start } => return vec![Geometry::Line { start, end: cursor }],
            Stage::ThreePointEnd { start, second } => ArcSeg::through(start, second, cursor),
            Stage::StartAfterCenter { center } | Stage::CenterAfterStart { start: center } => {
                return vec![Geometry::Line { start: center, end: cursor }];
            }
            Stage::End { center, start } => Self::center_arc(center, start, cursor),
            _ => None,
        };
        arc.map(|a| Geometry::arc(&a)).into_iter().collect()
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Second { start } | Stage::CenterAfterStart { start } => Some(start),
            Stage::ThreePointEnd { second, .. } => Some(second),
            Stage::StartAfterCenter { center } | Stage::End { center, .. } => Some(center),
            _ => None,
        }
    }
}

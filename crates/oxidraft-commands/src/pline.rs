use oxidraft_geom::{Point, curve::ArcSeg};
use oxidraft_model::{EntityId, Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step};

/// PLINE: one polyline of straight and arc segments.
///
/// The polyline is added after its first segment and updated with each new
/// one, so Esc keeps what was drawn (as in AutoCAD) and each segment can be
/// undone on its own.
#[derive(Default)]
pub struct Pline {
    id: Option<EntityId>,
    vertices: Vec<Vertex>,
    arc_mode: bool,
    /// Arc mode "Second pt": `Some(None)` waits for the second point,
    /// `Some(Some(p))` for the end point.
    three_point: Option<Option<Point>>,
}

impl Pline {
    fn last(&self) -> Point {
        self.vertices.last().map_or(Point::ORIGIN, |v| v.point)
    }

    fn segment_count(&self) -> usize {
        self.vertices.len().saturating_sub(1)
    }

    /// Direction of travel at the end of the polyline, for tangent arcs.
    fn end_direction(&self) -> f64 {
        let g = Geometry::Polyline { vertices: self.vertices.clone(), closed: false };
        match g.segments().last() {
            Some(seg) => Point::ORIGIN.angle_to(seg.tangent_at(1.0)),
            None => 0.0,
        }
    }

    /// The bulge of an arc from the last point to `to` in the current arc mode.
    fn arc_bulge(&self, to: Point) -> f64 {
        let from = self.last();
        let arc = match self.three_point {
            Some(Some(second)) => ArcSeg::through(from, second, to),
            _ => ArcSeg::tangent(from, self.end_direction(), to),
        };
        arc.map_or(0.0, |a| a.bulge())
    }

    fn geometry(&self, closed: bool) -> Geometry {
        Geometry::Polyline { vertices: self.vertices.clone(), closed }
    }

    fn add_segment(&mut self, bulge: f64, to: Point, ctx: &mut Ctx) -> Result<Step, String> {
        if self.last().approx_eq(to) {
            return Err("Zero-length segment ignored.".into());
        }
        if let Some(v) = self.vertices.last_mut() {
            v.bulge = bulge;
        }
        self.vertices.push(Vertex::at(to));
        let g = self.geometry(false);
        match self.id {
            Some(id) => {
                ctx.doc.modify(id, g);
            }
            None => self.id = Some(ctx.doc.add(g)),
        }
        Ok(Step::Continue)
    }

    fn close(&mut self, ctx: &mut Ctx) -> Result<Step, String> {
        if self.segment_count() < 2 {
            return Err("Close needs at least two segments.".into());
        }
        let first = self.vertices[0].point;
        let bulge = if self.arc_mode { self.arc_bulge(first) } else { 0.0 };
        if let Some(v) = self.vertices.last_mut() {
            v.bulge = bulge;
        }
        if let Some(id) = self.id {
            ctx.doc.modify(id, self.geometry(true));
        }
        Ok(Step::Done)
    }

    fn undo(&mut self, ctx: &mut Ctx) {
        if self.three_point.take().is_some() {
            return;
        }
        if self.vertices.len() >= 2 {
            ctx.doc.undo_last_in_transaction();
        }
        self.vertices.pop();
        if let Some(v) = self.vertices.last_mut() {
            v.bulge = 0.0;
        }
        if self.vertices.len() < 2 {
            self.id = None;
        }
    }
}

impl Command for Pline {
    fn prompt(&self) -> Prompt {
        if self.vertices.is_empty() {
            return Prompt::new("Specify start point");
        }
        let closable = self.segment_count() >= 2;
        match (self.arc_mode, self.three_point) {
            (true, Some(None)) => Prompt::new("Specify second point on arc"),
            (true, Some(Some(_))) => Prompt::new("Specify end point of arc"),
            (true, None) => Prompt::new("Specify endpoint of arc").with_keywords(if closable {
                &["CLose", "Line", "Second pt", "Undo"]
            } else {
                &["Line", "Second pt", "Undo"]
            }),
            (false, _) => Prompt::new("Specify next point").with_keywords(if closable {
                &["Arc", "Close", "Undo"]
            } else {
                &["Arc", "Undo"]
            }),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if self.vertices.is_empty() {
            return match input {
                Input::Point(p) => {
                    self.vertices.push(Vertex::at(p));
                    ctx.messages.push("Current line-width is 0.0000".into());
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err("Point required.".into()),
            };
        }
        match input {
            Input::Point(p) if self.arc_mode => {
                if self.three_point == Some(None) {
                    self.three_point = Some(Some(p));
                    return Ok(Step::Continue);
                }
                let bulge = self.arc_bulge(p);
                self.three_point = None;
                self.add_segment(bulge, p, ctx)
            }
            Input::Point(p) => self.add_segment(0.0, p, ctx),
            // Direct distance entry along the rubber band.
            Input::Value(d) if !self.arc_mode => {
                let from = self.last();
                if from.approx_eq(ctx.cursor) {
                    return Err("Move the cursor to show a direction.".into());
                }
                self.add_segment(0.0, from.polar(d, from.angle_to(ctx.cursor)), ctx)
            }
            Input::Keyword("Arc") => {
                self.arc_mode = true;
                Ok(Step::Continue)
            }
            Input::Keyword("Line") => {
                self.arc_mode = false;
                self.three_point = None;
                Ok(Step::Continue)
            }
            Input::Keyword("Second pt") => {
                self.three_point = Some(None);
                Ok(Step::Continue)
            }
            Input::Keyword("Close" | "CLose") => self.close(ctx),
            Input::Keyword("Undo") => {
                self.undo(ctx);
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Point or option keyword required.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        if self.vertices.is_empty() {
            return Vec::new();
        }
        let from = self.last();
        match (self.arc_mode, self.three_point) {
            (true, Some(None)) | (false, _) => vec![Geometry::Line { start: from, end: cursor }],
            (true, _) => {
                let bulge = self.arc_bulge(cursor);
                vec![Geometry::Polyline { vertices: vec![Vertex::new(from, bulge), Vertex::at(cursor)], closed: false }]
            }
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.three_point {
            Some(Some(second)) => Some(second),
            _ => self.vertices.last().map(|v| v.point),
        }
    }
}

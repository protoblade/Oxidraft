use oxidraft_geom::{Point, curve::ArcSeg};
use oxidraft_model::{EntityId, Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

/// Asking for a width: `half` for Halfwidth; `start` once the starting
/// width is in, waiting for the ending one.
#[derive(Clone, Copy)]
struct WidthPrompt {
    half: bool,
    start: Option<f64>,
}

/// PLINE: one polyline of straight and arc segments, optionally wide or
/// tapered.
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
    /// Widths of the next segment; after a taper, the ending width
    /// carries on (PLINEWID).
    start_width: f64,
    end_width: f64,
    width: Option<WidthPrompt>,
    /// Length option: the length of a line continuing the last segment.
    length: bool,
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
            *v = Vertex::new(v.point, bulge).with_widths(self.start_width, self.end_width);
        }
        self.start_width = self.end_width;
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
            *v = Vertex::new(v.point, bulge).with_widths(self.start_width, self.end_width);
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

    fn width_input(&mut self, w: WidthPrompt, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let factor = if w.half { 2.0 } else { 1.0 };
        let default = w.start.unwrap_or(self.end_width);
        let value = match input {
            Input::Value(v) if v >= 0.0 => v * factor,
            Input::Point(p) => self.last().distance(p) * factor,
            Input::Enter => default,
            _ => return Err("Requires a width of 0 or more.".into()),
        };
        match w.start {
            None => self.width = Some(WidthPrompt { start: Some(value), ..w }),
            Some(start) => {
                (self.start_width, self.end_width) = (start, value);
                ctx.vars.pline_width = value;
                self.width = None;
            }
        }
        Ok(Step::Continue)
    }
}

impl Command for Pline {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        (self.start_width, self.end_width) = (vars.pline_width, vars.pline_width);
        None
    }

    fn prompt(&self) -> Prompt {
        if self.vertices.is_empty() {
            return Prompt::new("Specify start point");
        }
        if let Some(w) = self.width {
            let (shown, noun) = if w.half { (0.5, "half-width") } else { (1.0, "width") };
            let (which, default) = match w.start {
                None => ("starting", self.end_width * shown),
                Some(start) => ("ending", start * shown),
            };
            return Prompt::new(format!("Specify {which} {noun}")).with_default(format!("{default:.4}"));
        }
        if self.length {
            return Prompt::new("Specify length of line");
        }
        let closable = self.segment_count() >= 2;
        match (self.arc_mode, self.three_point) {
            (true, Some(None)) => Prompt::new("Specify second point on arc"),
            (true, Some(Some(_))) => Prompt::new("Specify end point of arc"),
            (true, None) => Prompt::new("Specify endpoint of arc").with_keywords(if closable {
                &["CLose", "Halfwidth", "Line", "Second pt", "Undo", "Width"]
            } else {
                &["Halfwidth", "Line", "Second pt", "Undo", "Width"]
            }),
            (false, _) => Prompt::new("Specify next point").with_keywords(if closable {
                &["Arc", "Close", "Halfwidth", "Length", "Undo", "Width"]
            } else {
                &["Arc", "Halfwidth", "Length", "Undo", "Width"]
            }),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if self.vertices.is_empty() {
            return match input {
                Input::Point(p) => {
                    self.vertices.push(Vertex::at(p));
                    ctx.messages.push(format!("Current line-width is {:.4}", self.end_width));
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err("Point required.".into()),
            };
        }
        if let Some(w) = self.width {
            return self.width_input(w, input, ctx);
        }
        if self.length {
            return match input {
                Input::Value(d) if d > 0.0 => {
                    self.length = false;
                    let from = self.last();
                    self.add_segment(0.0, from.polar(d, self.end_direction()), ctx)
                }
                Input::Point(p) => {
                    self.length = false;
                    let from = self.last();
                    self.add_segment(0.0, from.polar(from.distance(p), self.end_direction()), ctx)
                }
                Input::Enter => {
                    self.length = false;
                    Ok(Step::Continue)
                }
                _ => Err("Requires a positive length.".into()),
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
            Input::Keyword(k @ ("Width" | "Halfwidth")) => {
                self.width = Some(WidthPrompt { half: k == "Halfwidth", start: None });
                Ok(Step::Continue)
            }
            Input::Keyword("Length") => {
                self.length = true;
                Ok(Step::Continue)
            }
            Input::Keyword("Undo") => {
                self.undo(ctx);
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Point or option keyword required.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        if self.vertices.is_empty() || self.width.is_some() {
            return Vec::new();
        }
        let from = self.last();
        let segment = |bulge: f64, to: Point| Geometry::Polyline {
            vertices: vec![Vertex::new(from, bulge).with_widths(self.start_width, self.end_width), Vertex::at(to)],
            closed: false,
        };
        match (self.arc_mode, self.three_point) {
            _ if self.length => vec![segment(0.0, from.polar(from.distance(cursor), self.end_direction()))],
            (true, Some(None)) => vec![Geometry::Line { start: from, end: cursor }],
            (false, _) => vec![segment(0.0, cursor)],
            (true, _) => vec![segment(self.arc_bulge(cursor), cursor)],
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.three_point {
            Some(Some(second)) => Some(second),
            _ => self.vertices.last().map(|v| v.point),
        }
    }
}

//! POINT, DIVIDE and MEASURE, and the point style (PDMODE, PDSIZE).

use oxidraft_geom::{EPSILON, Point, curve::Seg};
use oxidraft_model::{Document, Geometry, Placement, snap};

use crate::{Command, Ctx, Input, Prompt, Step};

fn point_modes(doc: &Document) -> String {
    format!("Current point modes:  PDMODE={}  PDSIZE={:.4}", doc.pdmode(), doc.pdsize())
}

/// POINT: places a point object.
#[derive(Default)]
pub struct PointCmd;

impl Command for PointCmd {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        Some(point_modes(doc))
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Specify a point")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => {
                ctx.doc.add(Geometry::Point { position: p });
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Specify a point.".into()),
        }
    }
}

/// PDMODE values AutoCAD accepts: a figure (0-4), plus 32 for a circle and
/// 64 for a square.
fn valid_pdmode(v: f64) -> Option<i32> {
    let n = v as i32;
    (f64::from(n) == v && (0..=4).contains(&(n & 31)) && n & !(31 | 32 | 64) == 0).then_some(n)
}

/// PDMODE and PDSIZE, typed as commands.
pub struct PointStyle {
    size: bool,
    current: String,
}

impl PointStyle {
    pub fn mode() -> Self {
        Self { size: false, current: String::new() }
    }

    pub fn size() -> Self {
        Self { size: true, current: String::new() }
    }
}

impl Command for PointStyle {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.current = if self.size { format!("{:.4}", doc.pdsize()) } else { doc.pdmode().to_string() };
        None
    }

    fn prompt(&self) -> Prompt {
        let name = if self.size { "PDSIZE" } else { "PDMODE" };
        Prompt::new(format!("Enter new value for {name}")).with_default(self.current.clone())
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let (mode, size) = (ctx.doc.pdmode(), ctx.doc.pdsize());
        match input {
            Input::Value(v) if self.size => ctx.doc.set_point_style(mode, v),
            Input::Value(v) => {
                let n = valid_pdmode(v).ok_or("Requires 0-4, plus 32 for a circle or 64 for a square.")?;
                ctx.doc.set_point_style(n, size);
            }
            Input::Enter => {}
            _ => return Err("Requires a number.".into()),
        }
        Ok(Step::Done)
    }
}

/// The point at distance `d` along consecutive segments, and the direction
/// there (degrees).
fn at_length(segs: &[Seg], mut d: f64) -> Option<(Point, f64)> {
    for s in segs {
        let len = s.length();
        if d <= len + EPSILON {
            let t = if len > EPSILON { (d / len).clamp(0.0, 1.0) } else { 0.0 };
            return Some((s.point_at(t), Point::ORIGIN.angle_to(s.tangent_at(t))));
        }
        d -= len;
    }
    None
}

/// Most marks DIVIDE and MEASURE place, as in AutoCAD.
const MAX_MARKS: usize = 32767;

/// What DIVIDE and MEASURE place: points, or a block aligned or not.
#[derive(Clone)]
enum Marker {
    Point,
    Block { name: String, align: bool },
}

impl Marker {
    fn place(&self, ctx: &mut Ctx, (at, angle): (Point, f64)) {
        let geometry = match self {
            Marker::Point => Geometry::Point { position: at },
            Marker::Block { name, align } => Geometry::Insert {
                block: name.clone(),
                placement: Placement { rotation: if *align { angle } else { 0.0 }, ..Placement::at(at) },
                attributes: Vec::new(),
            },
        };
        ctx.doc.add(geometry);
    }
}

#[derive(Clone)]
enum Stage {
    Object,
    /// The object's segments, in the direction to work along.
    Amount(Vec<Seg>, bool),
    BlockName(Vec<Seg>, bool),
    Align(Vec<Seg>, bool, String),
    BlockAmount(Vec<Seg>, bool, Marker),
}

/// DIVIDE (`measure` false) places marks that split an object into equal
/// parts; MEASURE places them a given length apart, from the end nearest
/// where you picked it.
pub struct Marks {
    measure: bool,
    stage: Stage,
}

impl Marks {
    pub fn divide() -> Self {
        Self { measure: false, stage: Stage::Object }
    }

    pub fn measure() -> Self {
        Self { measure: true, stage: Stage::Object }
    }

    fn verb(&self) -> &'static str {
        if self.measure { "measure" } else { "divide" }
    }

    /// Places the marks; `amount` is the number of segments or their length.
    fn mark(&self, ctx: &mut Ctx, segs: &[Seg], closed: bool, amount: f64, marker: &Marker) -> Result<Step, String> {
        let total: f64 = segs.iter().map(Seg::length).sum();
        let distances: Vec<f64> = if self.measure {
            if amount <= EPSILON {
                return Err("Requires a positive length.".into());
            }
            if total / amount > MAX_MARKS as f64 {
                return Err("That would place too many marks.".into());
            }
            (1..).map(|k| k as f64 * amount).take_while(|d| *d < total - EPSILON).collect()
        } else {
            let n = amount as usize;
            if amount.fract() != 0.0 || !(2..=MAX_MARKS).contains(&n) {
                return Err(format!("Requires an integer between 2 and {MAX_MARKS}."));
            }
            // A closed object gets a mark at its start too.
            let first = if closed { 0 } else { 1 };
            (first..n).map(|k| total * k as f64 / n as f64).collect()
        };
        for d in distances {
            if let Some(at) = at_length(segs, d) {
                marker.place(ctx, at);
            }
        }
        Ok(Step::Done)
    }
}

impl Command for Marks {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        Some(point_modes(doc))
    }

    fn prompt(&self) -> Prompt {
        match &self.stage {
            Stage::Object => Prompt::new(format!("Select object to {}", self.verb())),
            Stage::Amount(..) if self.measure => Prompt::new("Specify length of segment").with_keywords(&["Block"]),
            Stage::Amount(..) => Prompt::new("Enter the number of segments").with_keywords(&["Block"]),
            Stage::BlockName(..) => Prompt::new("Enter name of block to insert"),
            Stage::Align(..) => Prompt::new("Align block with object?").with_keywords(&["Yes", "No"]).with_default("Y"),
            Stage::BlockAmount(..) if self.measure => Prompt::new("Specify length of segment"),
            Stage::BlockAmount(..) => Prompt::new("Enter the number of segments"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (Stage::Object, Input::Pick { at, .. }) => {
                let Some(e) = snap::pick(ctx.doc, at, ctx.pick_tolerance).and_then(|id| ctx.doc.entity(id)) else {
                    return Err("No object found.".into());
                };
                let g = &e.geometry;
                let mut segs = g.segments();
                if segs.is_empty()
                    || !matches!(
                        g,
                        Geometry::Line { .. }
                            | Geometry::Arc { .. }
                            | Geometry::Circle { .. }
                            | Geometry::Polyline { .. }
                            | Geometry::Ellipse(_)
                            | Geometry::Spline(_)
                    )
                {
                    return Err(format!("Cannot {} that object.", self.verb()));
                }
                // MEASURE starts from the end nearest the pick.
                let closed = g.is_closed();
                if self.measure && !closed {
                    let (start, end) = (segs[0].start(), segs[segs.len() - 1].end());
                    if end.distance(at) < start.distance(at) {
                        segs = segs.iter().rev().map(Seg::reversed).collect();
                    }
                }
                self.stage = Stage::Amount(segs, closed);
                Ok(Step::Continue)
            }
            (Stage::Amount(segs, closed), Input::Value(v)) => self.mark(ctx, &segs, closed, v, &Marker::Point),
            (Stage::Amount(segs, closed), Input::Keyword("Block")) => {
                self.stage = Stage::BlockName(segs, closed);
                Ok(Step::Continue)
            }
            (Stage::BlockName(segs, closed), Input::Text(name)) => {
                let Some(block) = ctx.doc.block(&name) else {
                    return Err(format!("Block \"{name}\" cannot be found."));
                };
                self.stage = Stage::Align(segs, closed, block.name.clone());
                Ok(Step::Continue)
            }
            (Stage::Align(segs, closed, name), input @ (Input::Keyword(_) | Input::Enter)) => {
                let align = input != Input::Keyword("No");
                self.stage = Stage::BlockAmount(segs, closed, Marker::Block { name, align });
                Ok(Step::Continue)
            }
            (Stage::BlockAmount(segs, closed, marker), Input::Value(v)) => self.mark(ctx, &segs, closed, v, &marker),
            (_, Input::Enter) => Ok(Step::Done),
            (Stage::Object, _) => Err(format!("Select an object to {}.", self.verb())),
            _ => Err("Requires a number.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::Object)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, Stage::BlockName(..))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_modes_and_lengths_along_a_curve() {
        assert_eq!(valid_pdmode(35.0), Some(35));
        assert_eq!(valid_pdmode(5.0), None);
        assert_eq!(valid_pdmode(128.0), None);
        let l = Geometry::Polyline {
            vertices: vec![
                oxidraft_model::Vertex::at(Point::new(0.0, 0.0)),
                oxidraft_model::Vertex::at(Point::new(10.0, 0.0)),
                oxidraft_model::Vertex::at(Point::new(10.0, 10.0)),
            ],
            closed: false,
        };
        let (p, angle) = at_length(&l.segments(), 15.0).unwrap();
        assert!(p.approx_eq(Point::new(10.0, 5.0)) && (angle - 90.0).abs() < 1e-9);
        assert!(at_length(&l.segments(), 21.0).is_none());
    }
}

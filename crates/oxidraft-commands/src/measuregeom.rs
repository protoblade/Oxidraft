//! MEASUREGEOM (MEA): distance, radius, angle, area and volume, one after
//! another; and BASE and OOPS.

use oxidraft_geom::{Point, curve::Seg};
use oxidraft_model::{Geometry, units};

use crate::{
    Command, Ctx, Input, Prompt, Step,
    inquiry::{Area, Dist},
};

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Option,
    /// DIST or AREA doing the measuring.
    Inner,
    Radius,
    /// The first object for an angle, or Enter for a vertex.
    Angle,
    SecondLine(Point, Point, Point),
    Vertex,
    FirstEnd(Point),
    SecondEnd(Point, Point),
    Height,
}

#[derive(Clone, Copy, PartialEq)]
enum Measure {
    Distance,
    Area,
    Volume,
}

pub struct MeasureGeom {
    stage: Stage,
    measure: Measure,
    inner: Option<Box<dyn Command>>,
    /// Something was measured: Enter now ends the command.
    measured: bool,
}

impl Default for MeasureGeom {
    fn default() -> Self {
        Self { stage: Stage::Option, measure: Measure::Distance, inner: None, measured: false }
    }
}

/// The angle at `vertex` between the directions to `a` and `b`.
fn angle_at(vertex: Point, a: Point, b: Point) -> f64 {
    let (u, v) = (a - vertex, b - vertex);
    v.cross(u).atan2(u.dot(v)).to_degrees().abs()
}

impl MeasureGeom {
    fn start(&mut self, measure: Measure) {
        self.measure = measure;
        self.inner = Some(match measure {
            Measure::Distance => Box::new(Dist::default()),
            _ => Box::new(Area::default()),
        });
        self.stage = Stage::Inner;
    }

    fn done(&mut self) -> Step {
        self.measured = true;
        self.stage = Stage::Option;
        Step::Continue
    }

    fn report_angle(&mut self, ctx: &mut Ctx, deg: f64) -> Step {
        ctx.messages.push(format!("Angle = {}°", units::angle(deg)));
        self.done()
    }
}

impl Command for MeasureGeom {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Option => Prompt::new("Enter an option")
                .with_keywords(&["Distance", "Radius", "Angle", "ARea", "Volume", "eXit"])
                .with_default(if self.measured { "eXit" } else { "Distance" }),
            Stage::Inner => self.inner.as_ref().map_or_else(|| Prompt::new(""), |c| c.prompt()),
            Stage::Radius => Prompt::new("Select arc or circle"),
            Stage::Angle => Prompt::new("Select arc, circle, line, or <specify vertex>"),
            Stage::SecondLine(..) => Prompt::new("Select second line"),
            Stage::Vertex => Prompt::new("Specify angle vertex"),
            Stage::FirstEnd(_) => Prompt::new("Specify first angle endpoint"),
            Stage::SecondEnd(..) => Prompt::new("Specify second angle endpoint"),
            Stage::Height => Prompt::new("Specify height"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let picked = |ctx: &Ctx, at: Point| ctx.pick(at).and_then(|id| ctx.doc.entity(id)).map(|e| e.geometry.clone());
        match (self.stage, input) {
            (Stage::Option, Input::Keyword(k)) => match k {
                "Distance" => self.start(Measure::Distance),
                "ARea" => self.start(Measure::Area),
                "Volume" => self.start(Measure::Volume),
                "Radius" => self.stage = Stage::Radius,
                "Angle" => self.stage = Stage::Angle,
                _ => return Ok(Step::Done),
            },
            (Stage::Option, Input::Enter) if self.measured => return Ok(Step::Done),
            (Stage::Option, Input::Enter) => self.start(Measure::Distance),
            // A point straight away measures a distance from it.
            (Stage::Option, Input::Point(p)) => {
                self.start(Measure::Distance);
                return self.input(Input::Point(p), ctx);
            }
            (Stage::Inner, input) => {
                let inner = self.inner.as_mut().ok_or("Nothing to measure.")?;
                if inner.input(input, ctx)? == Step::Done {
                    self.inner = None;
                    if self.measure == Measure::Volume {
                        self.stage = Stage::Height;
                        return Ok(Step::Continue);
                    }
                    return Ok(self.done());
                }
            }
            (Stage::Height, Input::Value(h)) => {
                ctx.messages
                    .push(format!("Volume = {}", units::area(ctx.vars.last_area * h).replace("square", "cubic")));
                return Ok(self.done());
            }
            (Stage::Radius, Input::Pick { at, .. }) => {
                let r = match picked(ctx, at) {
                    Some(Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. }) => radius,
                    Some(g) => match g.segments().as_slice() {
                        [Seg::Arc(a), ..] => a.radius,
                        _ => return Err("Select an arc or circle.".into()),
                    },
                    None => return Err("No object found.".into()),
                };
                ctx.messages.push(format!("Radius = {}", units::length(r)));
                ctx.messages.push(format!("Diameter = {}", units::length(2.0 * r)));
                return Ok(self.done());
            }
            (Stage::Angle, Input::Pick { at, .. }) => match picked(ctx, at) {
                Some(Geometry::Arc { start_angle, end_angle, .. }) => {
                    return Ok(self.report_angle(ctx, (end_angle - start_angle).rem_euclid(360.0)));
                }
                Some(Geometry::Circle { center, .. }) => self.stage = Stage::SecondEnd(center, at),
                Some(Geometry::Line { start, end }) => self.stage = Stage::SecondLine(start, end, at),
                Some(_) => return Err("Select an arc, circle or line.".into()),
                None => return Err("No object found.".into()),
            },
            (Stage::SecondLine(a, b, first), Input::Pick { at, .. }) => {
                let Some(Geometry::Line { start, end }) = picked(ctx, at) else {
                    return Err("Select a line.".into());
                };
                let (d1, d2) = (b - a, end - start);
                let cross = d1.cross(d2);
                if cross.abs() < 1e-12 * d1.length() * d2.length() {
                    ctx.messages.push("The lines are parallel: Angle = 0°".into());
                    return Ok(self.done());
                }
                // Where they cross, toward the points picked on each.
                let t = (start - a).cross(d2) / cross;
                let vertex = a + d1 * t;
                return Ok(self.report_angle(ctx, angle_at(vertex, first, at)));
            }
            (Stage::Angle, Input::Enter) => self.stage = Stage::Vertex,
            (Stage::Vertex, Input::Point(p)) => self.stage = Stage::FirstEnd(p),
            (Stage::FirstEnd(v), Input::Point(p)) => self.stage = Stage::SecondEnd(v, p),
            (Stage::SecondEnd(v, a), Input::Point(b) | Input::Pick { at: b, .. }) => {
                return Ok(self.report_angle(ctx, angle_at(v, a, b)));
            }
            (_, Input::Enter) => return Ok(self.done()),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.inner.as_ref().map_or_else(Vec::new, |c| c.preview(cursor))
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Inner => self.inner.as_ref().and_then(|c| c.base_point()),
            Stage::FirstEnd(v) | Stage::SecondEnd(v, _) => Some(v),
            _ => None,
        }
    }

    fn wants_pick(&self) -> bool {
        match self.stage {
            Stage::Inner => self.inner.as_ref().is_some_and(|c| c.wants_pick()),
            Stage::Radius | Stage::Angle | Stage::SecondLine(..) => true,
            _ => false,
        }
    }
}

/// BASE: the drawing's insertion base point (INSBASE), where it lands
/// when inserted into another.
#[derive(Default)]
pub struct Base {
    current: Point,
}

impl Command for Base {
    fn prepare(&mut self, doc: &oxidraft_model::Document) -> Option<String> {
        self.current = doc.insbase();
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Enter base point").with_default(units::current().point(self.current.x, self.current.y))
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => {
                ctx.doc.set_insbase(p);
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Point required.".into()),
        }
    }
}

/// OOPS, run at once: the erased objects back in their spaces.
pub(crate) fn oops(ctx_doc: &mut oxidraft_model::Document, vars: &mut crate::Vars) -> String {
    let erased = std::mem::take(&mut vars.erased);
    if erased.is_empty() {
        return "Nothing to restore.".into();
    }
    let here = ctx_doc.space();
    for e in &erased {
        ctx_doc.set_space(e.space);
        ctx_doc.add_styled(e.geometry.clone(), &e.layer, e.color, &e.linetype, e.lineweight);
    }
    ctx_doc.set_space(here);
    format!("{} object(s) restored.", erased.len())
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    fn pick(e: &mut Engine, d: &mut Document, x: f64, y: f64) {
        let p = Point::new(x, y);
        e.click(p, p, false, d, 0.5);
    }

    fn said(e: &Engine, start: &str) -> String {
        e.history().iter().rev().find(|l| l.starts_with(start)).cloned().unwrap_or_default()
    }

    #[test]
    fn measures_one_thing_after_another() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        d.add(Geometry::Circle { center: Point::new(50.0, 0.0), radius: 5.0 });
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(20.0, 0.0) });
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(20.0, 20.0) });
        run(&mut e, &mut d, &["mea", "d", "0,0", "3,4"]);
        assert!(said(&e, "Distance").starts_with("Distance = 5.0000"));
        run(&mut e, &mut d, &["r"]);
        pick(&mut e, &mut d, 55.0, 0.0);
        assert_eq!(said(&e, "Radius"), "Radius = 5.0000");
        assert_eq!(said(&e, "Diameter"), "Diameter = 10.0000");
        run(&mut e, &mut d, &["a"]);
        pick(&mut e, &mut d, 15.0, 0.0);
        pick(&mut e, &mut d, 10.0, 10.0);
        assert_eq!(said(&e, "Angle"), "Angle = 45°");
        run(&mut e, &mut d, &["v", "0,0", "10,0", "10,10", "0,10", "", "3"]);
        assert_eq!(said(&e, "Volume"), "Volume = 300.0000");
        run(&mut e, &mut d, &[""]);
        assert!(!e.is_active());
    }

    #[test]
    fn base_and_oops() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["base", "10,20"]);
        assert_eq!(d.insbase(), Point::new(10.0, 20.0));
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(20.0, 0.0) });
        e.select_all(&d);
        run(&mut e, &mut d, &["erase"]);
        assert_eq!(d.len(), 0);
        d.add(Geometry::Circle { center: Point::ORIGIN, radius: 1.0 });
        run(&mut e, &mut d, &["oops"]);
        assert_eq!(d.len(), 2, "the line is back, the circle stays");
    }
}

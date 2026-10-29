//! Inquiry commands: ID, DIST and AREA, which only print what they
//! measure; and LTSCALE, which sets a drawing setting.

use oxidraft_geom::{Point, curve::Seg};
use oxidraft_model::{Document, Geometry, Vertex, hatch, snap};

use crate::{Command, Ctx, Input, Prompt, Step};

/// An angle in degrees as AutoCAD prints it: no trailing zeros.
fn angle(deg: f64) -> String {
    let s = format!("{:.4}", deg.rem_euclid(360.0));
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// ID: prints the coordinates of a point.
#[derive(Default)]
pub struct Id;

impl Command for Id {
    fn prompt(&self) -> Prompt {
        Prompt::new("Specify point")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => {
                ctx.messages.push(format!("X = {:.4}     Y = {:.4}     Z = 0.0000", p.x, p.y));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Point required.".into()),
        }
    }
}

/// DIST: distance and angle between two points.
#[derive(Default)]
pub struct Dist {
    first: Option<Point>,
}

impl Command for Dist {
    fn prompt(&self) -> Prompt {
        match self.first {
            None => Prompt::new("Specify first point"),
            Some(_) => Prompt::new("Specify second point"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.first, input) {
            (None, Input::Point(p)) => {
                self.first = Some(p);
                Ok(Step::Continue)
            }
            (Some(a), Input::Point(b)) => {
                let d = b - a;
                ctx.messages.push(format!(
                    "Distance = {:.4},  Angle in XY Plane = {},  Angle from XY Plane = 0",
                    a.distance(b),
                    angle(a.angle_to(b))
                ));
                ctx.messages.push(format!("Delta X = {:.4},  Delta Y = {:.4},  Delta Z = 0.0000", d.x, d.y));
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err("Point required.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.first.map(|start| Geometry::Line { start, end: cursor }).into_iter().collect()
    }

    fn base_point(&self) -> Option<Point> {
        self.first
    }
}

/// AREA: the area and perimeter of a polygon through picked points, or of
/// an object.
#[derive(Default)]
pub struct Area {
    points: Vec<Point>,
    object: bool,
}

/// Signed area of a closed loop of polyline vertices (shoelace plus arc segments).
fn loop_area(vertices: &[Vertex]) -> f64 {
    Geometry::Polyline { vertices: vertices.to_vec(), closed: true }.area().unwrap_or(0.0)
}

/// What AREA reports for an object: its area and its perimeter (with the
/// word AutoCAD uses for it). Open curves are measured as if closed.
pub(crate) fn object_area(g: &Geometry) -> Option<(f64, f64, &'static str)> {
    match g {
        Geometry::Circle { .. } => Some((g.area()?, g.length(), "Circumference")),
        Geometry::Polyline { vertices, closed: true } if vertices.len() >= 3 => {
            Some((g.area()?, g.length(), "Perimeter"))
        }
        Geometry::Polyline { vertices, closed: false } if vertices.len() >= 3 => {
            Some((loop_area(vertices), g.length(), "Length"))
        }
        Geometry::Arc { .. } => {
            let Some(Seg::Arc(a)) = g.segments().first().copied() else { return None };
            let chorded = [Vertex::new(a.start_point(), a.bulge()), Vertex::at(a.end_point())];
            Some((loop_area(&chorded), g.length(), "Length"))
        }
        Geometry::Ellipse(_) | Geometry::Spline(_) => {
            let chorded: Vec<Vertex> = g.segments().iter().map(|s| Vertex::new(s.start(), s.bulge())).collect();
            let word = if g.is_closed() { "Perimeter" } else { "Length" };
            Some((g.area().unwrap_or_else(|| loop_area(&chorded)), g.length(), word))
        }
        // Islands inside a hatch don't count: alternate adding and removing
        // loops by how deeply they're nested.
        Geometry::Hatch { loops, .. } => {
            let polys = hatch::polygons(loops);
            let mut total = 0.0;
            for (i, l) in loops.iter().enumerate() {
                let Some(inside) = polys[i].first() else { continue };
                let depth = (0..polys.len())
                    .filter(|&j| j != i && hatch::contains(std::slice::from_ref(&polys[j]), *inside))
                    .count();
                total += if depth % 2 == 0 { loop_area(l) } else { -loop_area(l) };
            }
            let perimeter = loops.iter().flat_map(|l| hatch::loop_segments(l)).map(|s| s.length()).sum();
            Some((total, perimeter, "Perimeter"))
        }
        _ => None,
    }
}

impl Area {
    fn report(ctx: &mut Ctx, area: f64, perimeter: f64, word: &str) {
        ctx.messages.push(format!("Area = {area:.4}, {word} = {perimeter:.4}"));
    }

    fn polygon(&self) -> Geometry {
        Geometry::Polyline { vertices: self.points.iter().map(|&p| Vertex::at(p)).collect(), closed: true }
    }
}

impl Command for Area {
    fn prompt(&self) -> Prompt {
        match (self.object, self.points.len()) {
            (true, _) => Prompt::new("Select object"),
            (false, 0) => Prompt::new("Specify first corner point").with_keywords(&["Object"]).with_default("Object"),
            (false, 1 | 2) => Prompt::new("Specify next point").with_keywords(&["Undo"]),
            (false, _) => Prompt::new("Specify next point").with_keywords(&["Undo", "Total"]).with_default("Total"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } if self.object => {
                let id = snap::pick(ctx.doc, at, ctx.pick_tolerance).ok_or("No object found.")?;
                let g = ctx.doc.entity(id).map(|e| e.geometry.clone()).ok_or("No object found.")?;
                let (area, perimeter, word) = object_area(&g).ok_or("Selected object does not have an area.")?;
                Self::report(ctx, area, perimeter, word);
                Ok(Step::Done)
            }
            Input::Keyword("Object") => {
                self.object = true;
                Ok(Step::Continue)
            }
            Input::Enter if self.points.is_empty() && !self.object => {
                self.object = true;
                Ok(Step::Continue)
            }
            Input::Point(p) if !self.object => {
                self.points.push(p);
                Ok(Step::Continue)
            }
            Input::Keyword("Undo") => {
                self.points.pop();
                Ok(Step::Continue)
            }
            Input::Keyword("Total") | Input::Enter if self.points.len() >= 3 => {
                let g = self.polygon();
                Self::report(ctx, g.area().unwrap_or(0.0), g.length(), "Perimeter");
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ if self.object => Err("Select an object.".into()),
            _ => Err("Point or option keyword required.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        if self.object || self.points.is_empty() {
            return Vec::new();
        }
        let mut points = self.points.clone();
        points.push(cursor);
        let vertices = points.into_iter().map(Vertex::at).collect();
        vec![Geometry::Polyline { vertices, closed: true }]
    }

    fn base_point(&self) -> Option<Point> {
        self.points.last().copied()
    }

    fn wants_pick(&self) -> bool {
        self.object
    }
}

/// LTSCALE: the global scale of linetype patterns.
#[derive(Default)]
pub struct Ltscale {
    current: f64,
}

impl Command for Ltscale {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.current = doc.ltscale();
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Enter new linetype scale factor").with_default(format!("{:.4}", self.current))
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Value(v) if v > 0.0 => {
                ctx.doc.set_ltscale(v);
                ctx.messages.push("Regenerating model.".into());
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Requires a positive number.".into()),
        }
    }
}

/// SNAP: the snap spacing, which turns snap on, or ON and OFF.
#[derive(Default)]
pub struct Snap {
    spacing: f64,
}

impl Command for Snap {
    fn start(&mut self, vars: &crate::Vars) -> Option<String> {
        self.spacing = vars.snap_spacing;
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Specify snap spacing").with_keywords(&["ON", "OFF"]).with_default(format!("{:.4}", self.spacing))
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Value(v) if v > 0.0 => (ctx.vars.snap_spacing, ctx.vars.snap) = (v, true),
            Input::Keyword(k) => ctx.vars.snap = k == "ON",
            Input::Enter => ctx.vars.snap = true,
            _ => return Err("Requires a positive number, ON or OFF.".into()),
        }
        Ok(Step::Done)
    }
}

/// COLORTHEME: the ribbon, palettes and command line dark (0) or light (1).
#[derive(Default)]
pub struct ColorTheme {
    current: u8,
}

impl Command for ColorTheme {
    fn start(&mut self, vars: &crate::Vars) -> Option<String> {
        self.current = vars.color_theme;
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Enter new value for COLORTHEME").with_default(self.current.to_string())
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Value(v) if v == 0.0 || v == 1.0 => {
                ctx.vars.color_theme = v as u8;
                ctx.ui = Some(crate::UiRequest::ColorTheme(v as u8));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Requires an integer between 0 and 1.".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_of_objects() {
        let r = 2.0;
        let arc = Geometry::Arc { center: Point::ORIGIN, radius: r, start_angle: 0.0, end_angle: 180.0 };
        let (a, len, word) = object_area(&arc).unwrap();
        assert!((a - std::f64::consts::PI * 2.0).abs() < 1e-9, "half disc: {a}");
        assert!((len - std::f64::consts::PI * 2.0).abs() < 1e-9 && word == "Length");

        // A 10x10 hatch with a 2x2 hole.
        let sq = |x0: f64, s: f64| {
            [(x0, x0), (x0 + s, x0), (x0 + s, x0 + s), (x0, x0 + s)]
                .iter()
                .map(|&(x, y)| Vertex::at(Point::new(x, y)))
                .collect::<Vec<_>>()
        };
        let h = Geometry::Hatch {
            loops: vec![sq(0.0, 10.0), sq(4.0, 2.0)],
            pattern: oxidraft_model::HatchPattern::Solid,
            scale: 1.0,
            angle: 0.0,
        };
        let (a, p, _) = object_area(&h).unwrap();
        assert!((a - 96.0).abs() < 1e-9 && (p - 48.0).abs() < 1e-9);
        assert!(object_area(&Geometry::Line { start: Point::ORIGIN, end: Point::new(1.0, 0.0) }).is_none());
    }
}

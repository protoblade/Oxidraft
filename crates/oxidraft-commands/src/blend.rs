//! BOUNDARY and BLEND: closed polylines around an area picked inside, and
//! a spline joining two open objects' ends smoothly.

use oxidraft_geom::Point;
use oxidraft_model::{Geometry, Spline, boundary::boundary_at};

use crate::{Command, Ctx, Input, Prompt, Step};

/// BOUNDARY (BO): a closed polyline around each area picked inside (and
/// around the islands in it), as HATCH finds them.
#[derive(Default)]
pub struct Boundary {
    made: usize,
}

impl Command for Boundary {
    fn prompt(&self) -> Prompt {
        Prompt::new("Pick internal point")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } | Input::Point(at) => {
                let loops = boundary_at(ctx.doc, at).ok_or("Valid hatch boundary not found.")?;
                for l in loops {
                    ctx.doc.add(Geometry::Polyline { vertices: l, closed: true });
                    self.made += 1;
                }
                Ok(Step::Continue)
            }
            _ => {
                ctx.messages.push(format!("BOUNDARY created {} polyline(s)", self.made));
                Ok(Step::Done)
            }
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

/// An object's end nearest `at`, and the way it leaves the object there.
fn end_near(g: &Geometry, at: Point) -> Option<(Point, Point)> {
    let segs = g.segments();
    let (first, last) = (segs.first()?, segs.last()?);
    if g.is_closed() {
        return None;
    }
    let (start, end) = (first.start(), last.end());
    let unit = |p: Point| p * (1.0 / p.length().max(1e-12));
    Some(if at.distance(start) <= at.distance(end) {
        (start, unit(-first.tangent_at(0.0)))
    } else {
        (end, unit(last.tangent_at(1.0)))
    })
}

#[derive(Clone, Copy, PartialEq)]
enum BlendStage {
    First,
    Second(Point, Point),
}

/// BLEND: a spline from one open object's end to another's, leaving each
/// along it — Tangent (cubic) or Smooth (degree five, straighter at the ends).
pub struct Blend {
    stage: BlendStage,
    smooth: bool,
}

impl Default for Blend {
    fn default() -> Self {
        Self { stage: BlendStage::First, smooth: false }
    }
}

/// The blend from `a` leaving along `ta` to `b` arriving against `tb`.
pub(crate) fn blend(a: Point, ta: Point, b: Point, tb: Point, smooth: bool) -> Option<Spline> {
    let reach = a.distance(b);
    let control = if smooth {
        let k = reach / 5.0;
        vec![a, a + ta * k, a + ta * (2.0 * k), b + tb * (2.0 * k), b + tb * k, b]
    } else {
        let k = reach / 3.0;
        vec![a, a + ta * k, b + tb * k, b]
    };
    // One Bezier piece: as many control points, one degree fewer.
    let n = control.len();
    let knots = std::iter::repeat_n(0.0, n).chain(std::iter::repeat_n(1.0, n)).collect();
    Spline::new(n - 1, control, knots, Vec::new(), false)
}

impl Command for Blend {
    fn start(&mut self, _vars: &crate::Vars) -> Option<String> {
        Some("Continuity = Tangent".into())
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            BlendStage::First => Prompt::new("Select first object").with_keywords(&["CONtinuity"]),
            BlendStage::Second(..) => Prompt::new("Select second object"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let end = |ctx: &Ctx, at: Point| {
            let g =
                ctx.pick(at).and_then(|id| ctx.doc.entity(id)).map(|e| e.geometry.clone()).ok_or("No object found.")?;
            end_near(&g, at).ok_or("Select a line, arc, polyline or spline with ends.")
        };
        match (self.stage, input) {
            (BlendStage::First, Input::Keyword(_)) => {
                self.smooth = !self.smooth;
                ctx.messages.push(format!("Continuity = {}", if self.smooth { "Smooth" } else { "Tangent" }));
            }
            (BlendStage::First, Input::Pick { at, .. }) => {
                let (p, t) = end(ctx, at)?;
                self.stage = BlendStage::Second(p, t);
            }
            (BlendStage::Second(a, ta), Input::Pick { at, .. }) => {
                let (b, tb) = end(ctx, at)?;
                if a.approx_eq(b) {
                    return Err("The ends already meet.".into());
                }
                let s = blend(a, ta, b, tb, self.smooth).ok_or("Can't blend those.")?;
                ctx.doc.add(Geometry::Spline(s));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Select an object.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        true
    }
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

    fn click(e: &mut Engine, d: &mut Document, x: f64, y: f64) {
        let p = Point::new(x, y);
        e.click(p, p, false, d, 0.5);
    }

    #[test]
    fn boundary_and_blend() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        for (a, b) in [
            ((0.0, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 10.0)),
            ((10.0, 10.0), (0.0, 10.0)),
            ((0.0, 10.0), (0.0, 0.0)),
        ] {
            d.add(Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) });
        }
        run(&mut e, &mut d, &["bo"]);
        click(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &[""]);
        assert!(
            d.entities()
                .any(|x| matches!(&x.geometry, Geometry::Polyline { closed: true, vertices } if vertices.len() == 4))
        );
        // Two lines' ends blended, leaving each straight on.
        let mut d = Document::new();
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        d.add(Geometry::Line { start: Point::new(30.0, 10.0), end: Point::new(40.0, 10.0) });
        run(&mut e, &mut d, &["blend"]);
        click(&mut e, &mut d, 9.0, 0.0);
        click(&mut e, &mut d, 31.0, 10.0);
        let s = d.entities().find_map(|x| match &x.geometry {
            Geometry::Spline(s) => Some(s.clone()),
            _ => None,
        });
        let s = s.expect("a blend");
        assert!(s.start_point().approx_eq(Point::new(10.0, 0.0)) && s.end_point().approx_eq(Point::new(30.0, 10.0)));
        // Tangent: it leaves along +x and arrives along +x.
        let c = s.control();
        assert!((c[1].y - 0.0).abs() < 1e-9 && (c[c.len() - 2].y - 10.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod gradient_tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry, HatchPattern, hatch::GradientKind};

    #[test]
    fn gradient_fills_an_area() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        for (a, b) in [
            ((0.0, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 10.0)),
            ((10.0, 10.0), (0.0, 10.0)),
            ((0.0, 10.0), (0.0, 0.0)),
        ] {
            d.add(Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) });
        }
        e.dynamic_input = false;
        e.submit("gradient", &mut d, Point::ORIGIN, 0.5);
        e.submit("p", &mut d, Point::ORIGIN, 0.5);
        e.submit("spherical", &mut d, Point::ORIGIN, 0.5);
        let p = Point::new(5.0, 5.0);
        e.click(p, p, false, &mut d, 0.5);
        e.submit("", &mut d, Point::ORIGIN, 0.5);
        let pattern = d.entities().find_map(|x| match x.geometry {
            Geometry::Hatch { pattern, .. } => Some(pattern),
            _ => None,
        });
        assert!(matches!(pattern, Some(HatchPattern::Gradient { kind: GradientKind::Spherical, .. })));
        // Shading: the middle is the first color, the corners the second.
        let polys =
            vec![vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)]];
        let tris = oxidraft_model::hatch::gradient_triangles(&polys, GradientKind::Spherical, 0.0);
        assert!(tris.len() > 50, "fine enough to shade");
        let at = |q: Point| GradientKind::Spherical.at(q, (Point::new(0.0, 0.0), Point::new(10.0, 10.0)), 0.0);
        assert!(at(Point::new(5.0, 5.0)) < 0.01 && at(Point::new(10.0, 10.0)) > 0.99);
    }
}

//! UCS: the user coordinate system moved, turned, aligned to an object or
//! the view, saved and restored by name; PLAN: the view turned so a UCS's
//! X axis runs across the screen.

use oxidraft_geom::{Point, curve::Seg};
use oxidraft_model::ucs::{Ucs, WORLD};
use oxidraft_model::{Document, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step, ViewRequest};

#[derive(Clone, PartialEq)]
enum UcsStage {
    Origin,
    XAxis(Point),
    Named,
    Restore,
    Save,
    Delete,
    Object,
    Z,
}

/// UCS: a new origin (and a point on the X axis), NAmed (Restore, Save,
/// Delete, ?), OBject, Previous, View, World, or a turn about Z.
pub struct UcsCmd {
    stage: UcsStage,
    now: Ucs,
    names: Vec<String>,
}

impl Default for UcsCmd {
    fn default() -> Self {
        Self { stage: UcsStage::Origin, now: WORLD, names: Vec::new() }
    }
}

fn list(doc: &Document) -> Vec<String> {
    let mut out = vec!["Current ucs name:  ".to_string() + &ucs_name(doc)];
    for n in doc.named_ucs() {
        let o = n.ucs.origin;
        out.push(format!("  {}  origin = {},{}  X axis at {:.4}", n.name, o.x, o.y, n.ucs.angle));
    }
    out
}

/// The current UCS's name, as AutoCAD shows it.
pub fn ucs_name(doc: &Document) -> String {
    let ucs = doc.ucs();
    if ucs.is_world() {
        return "*WORLD*".into();
    }
    doc.named_ucs().iter().find(|n| n.ucs == ucs).map_or("*NO NAME*".into(), |n| n.name.clone())
}

/// A UCS along an object at `at`: a line or segment from its nearer end,
/// a circle or arc from its center, a text or block along its rotation.
fn along_object(g: &Geometry, at: Point) -> Option<Ucs> {
    match g {
        Geometry::Circle { center, .. } => Ucs::through(*center, at),
        Geometry::Text { position, rotation, .. } => Some(Ucs { origin: *position, angle: *rotation }),
        Geometry::Insert { placement, .. } => Some(Ucs { origin: placement.position, angle: placement.rotation }),
        _ => {
            let seg = g.segments().into_iter().min_by(|a, b| a.distance_to(at).total_cmp(&b.distance_to(at)))?;
            match seg {
                Seg::Arc(a) => {
                    let (s, e) = (seg.start(), seg.end());
                    Ucs::through(a.center, if s.distance(at) <= e.distance(at) { s } else { e })
                }
                Seg::Line(_) => {
                    let (s, e) = (seg.start(), seg.end());
                    if s.distance(at) <= e.distance(at) { Ucs::through(s, e) } else { Ucs::through(e, s) }
                }
            }
        }
    }
}

impl Command for UcsCmd {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.now = doc.ucs();
        self.names = doc.named_ucs().iter().map(|n| n.name.clone()).collect();
        Some(format!("Current ucs name:  {}", ucs_name(doc)))
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            UcsStage::Origin => Prompt::new("Specify origin of UCS or")
                .with_keywords(&["NAmed", "OBject", "Previous", "View", "World", "X", "Y", "Z"])
                .with_default("World"),
            UcsStage::XAxis(_) => Prompt::new("Specify point on X-axis or <Accept>"),
            UcsStage::Named => Prompt::new("Enter an option").with_keywords(&["Restore", "Save", "Delete", "?"]),
            UcsStage::Restore => Prompt::new("Enter name of UCS to restore or [?]"),
            UcsStage::Save => Prompt::new("Enter name to save current UCS or [?]"),
            UcsStage::Delete => Prompt::new("Enter UCS name(s) to delete <none>"),
            UcsStage::Object => Prompt::new("Select object to align UCS"),
            UcsStage::Z => Prompt::new("Specify rotation angle about Z axis").with_default("90"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (UcsStage::Origin, Input::Point(p)) => self.stage = UcsStage::XAxis(p),
            (UcsStage::Origin, Input::Keyword("NAmed")) => self.stage = UcsStage::Named,
            (UcsStage::Origin, Input::Keyword("OBject")) => self.stage = UcsStage::Object,
            (UcsStage::Origin, Input::Keyword("Previous")) => {
                if !ctx.doc.previous_ucs() {
                    ctx.messages.push("*No previous UCS*".into());
                }
                return Ok(Step::Done);
            }
            (UcsStage::Origin, Input::Keyword("View")) => {
                ctx.doc.set_ucs(Ucs { origin: self.now.origin, angle: -ctx.vars.view_rotation });
                return Ok(Step::Done);
            }
            (UcsStage::Origin, Input::Keyword("X" | "Y")) => {
                return Err("Oxidraft draws in 2D: turns about X or Y aren't available.".into());
            }
            (UcsStage::Origin, Input::Keyword("Z")) => self.stage = UcsStage::Z,
            (UcsStage::Origin, Input::Keyword(_) | Input::Enter) => {
                ctx.doc.set_ucs(WORLD);
                return Ok(Step::Done);
            }
            (UcsStage::XAxis(origin), input @ (Input::Point(_) | Input::Enter)) => {
                let ucs = match input {
                    Input::Point(p) => Ucs::through(origin, p).ok_or("The point is at the origin.")?,
                    _ => Ucs { origin, angle: self.now.angle },
                };
                ctx.doc.set_ucs(ucs);
                return Ok(Step::Done);
            }
            (UcsStage::Named, Input::Keyword("Restore")) => self.stage = UcsStage::Restore,
            (UcsStage::Named, Input::Keyword("Save")) => self.stage = UcsStage::Save,
            (UcsStage::Named, Input::Keyword("Delete")) => self.stage = UcsStage::Delete,
            (UcsStage::Named, Input::Keyword(_)) => {
                ctx.messages.extend(list(ctx.doc));
            }
            (UcsStage::Restore, Input::Text(t)) if t.trim() == "?" => ctx.messages.extend(list(ctx.doc)),
            (UcsStage::Restore, Input::Text(t)) => {
                let named = ctx.doc.named_ucs().iter().find(|n| n.name.eq_ignore_ascii_case(t.trim()));
                let ucs = named.map(|n| n.ucs).ok_or_else(|| format!("Cannot find UCS {}.", t.trim()))?;
                ctx.doc.set_ucs(ucs);
                return Ok(Step::Done);
            }
            (UcsStage::Save, Input::Text(t)) if t.trim() == "?" => ctx.messages.extend(list(ctx.doc)),
            (UcsStage::Save, Input::Text(t)) => {
                ctx.doc.save_ucs(t.trim(), self.now)?;
                return Ok(Step::Done);
            }
            (UcsStage::Delete, Input::Text(t)) => {
                let mut n = 0;
                for name in t.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let names: Vec<String> =
                        self.names.iter().filter(|u| crate::block_tools::matches_spec(name, u)).cloned().collect();
                    for u in names {
                        n += usize::from(ctx.doc.delete_ucs(&u));
                    }
                }
                ctx.messages.push(format!("Deleted {n} UCS(s)."));
                return Ok(Step::Done);
            }
            (UcsStage::Object, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("Nothing there.")?;
                let g = ctx.doc.entity(id).map(|e| e.geometry.clone()).ok_or("Nothing there.")?;
                let ucs = along_object(&g, at).ok_or("A UCS can't be aligned to that object.")?;
                ctx.doc.set_ucs(ucs);
                return Ok(Step::Done);
            }
            (UcsStage::Z, input @ (Input::Value(_) | Input::Enter)) => {
                let turn = if let Input::Value(v) = input { v } else { 90.0 };
                ctx.doc.set_ucs(Ucs { origin: self.now.origin, angle: self.now.angle + turn });
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        self.stage == UcsStage::Object
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, UcsStage::Restore | UcsStage::Save | UcsStage::Delete)
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            UcsStage::XAxis(p) => Some(p),
            _ => None,
        }
    }
}

/// PLAN: the view turned so the current UCS's X axis (or a named one's,
/// or the world's) runs across the screen, the drawing's extents shown.
#[derive(Default)]
pub struct Plan {
    named: bool,
}

impl Command for Plan {
    fn prompt(&self) -> Prompt {
        if self.named {
            Prompt::new("Enter name of UCS")
        } else {
            Prompt::new("Enter an option").with_keywords(&["Current ucs", "Ucs", "World"]).with_default("Current ucs")
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let angle = match (self.named, input) {
            (false, Input::Keyword("Ucs")) => {
                self.named = true;
                return Ok(Step::Continue);
            }
            (false, Input::Keyword("World")) => 0.0,
            (false, Input::Keyword(_) | Input::Enter) => ctx.doc.ucs().angle,
            (true, Input::Text(t)) => {
                let named = ctx.doc.named_ucs().iter().find(|n| n.name.eq_ignore_ascii_case(t.trim()));
                named.map(|n| n.ucs.angle).ok_or_else(|| format!("Cannot find UCS {}.", t.trim()))?
            }
            (true, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        };
        ctx.view = Some(ViewRequest::Plan(angle));
        Ok(Step::Done)
    }

    fn wants_text(&self) -> bool {
        self.named
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for (i, s) in inputs.iter().enumerate() {
            if i == 0 {
                e.run(s, d);
            } else {
                e.submit(s, d, Point::ORIGIN, 0.5);
            }
        }
    }

    #[test]
    fn ucs_moves_turns_and_takes_typed_points() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["ucs", "100,50", "100,60"]);
        let u = d.ucs();
        assert!(u.origin.approx_eq(Point::new(100.0, 50.0)) && (u.angle - 90.0).abs() < 1e-9);
        // A line typed in the UCS lands in the world turned and moved.
        run(&mut e, &mut d, &["line", "0,0", "10,0", ""]);
        let line = d.entities().find_map(|x| match x.geometry {
            Geometry::Line { start, end } => Some((start, end)),
            _ => None,
        });
        let (s, end) = line.unwrap();
        assert!(s.approx_eq(Point::new(100.0, 50.0)) && end.approx_eq(Point::new(100.0, 60.0)), "{s:?} {end:?}");
        // `*`: world coordinates.
        run(&mut e, &mut d, &["line", "*0,0", "*5,0", ""]);
        assert!(d.entities().any(|x| matches!(x.geometry, Geometry::Line { start, end } if start == Point::ORIGIN && end.approx_eq(Point::new(5.0, 0.0)))));
        run(&mut e, &mut d, &["ucs", "na", "s", "North"]);
        run(&mut e, &mut d, &["ucs", "z", "-90"]);
        assert!(d.ucs().angle.abs() < 1e-9);
        run(&mut e, &mut d, &["ucs", "p"]);
        assert!((d.ucs().angle - 90.0).abs() < 1e-9);
        run(&mut e, &mut d, &["ucs", ""]);
        assert!(d.ucs().is_world());
        run(&mut e, &mut d, &["ucs", "na", "r", "north"]);
        assert!((d.ucs().angle - 90.0).abs() < 1e-9);
        run(&mut e, &mut d, &["plan", ""]);
        assert_eq!(e.take_view_requests(), vec![ViewRequest::Plan(90.0)]);
    }

    #[test]
    fn ucs_object_aligns_to_a_line() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        d.add(Geometry::Line { start: Point::new(10.0, 10.0), end: Point::new(20.0, 20.0) });
        run(&mut e, &mut d, &["ucs", "ob"]);
        let p = Point::new(11.0, 11.0);
        e.click(p, p, false, &mut d, 0.5);
        let u = d.ucs();
        assert!(u.origin.approx_eq(Point::new(10.0, 10.0)) && (u.angle - 45.0).abs() < 1e-9, "{u:?}");
    }
}

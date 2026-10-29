//! HATCHEDIT (-HATCHEDIT, HE): a hatch's pattern, scale and angle, its
//! draw order, and its boundaries: loops added or removed, polylines made
//! of them, or the hatch split into one per separate area.

use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Document, EntityId, Geometry, HatchPattern, Vertex, boundary::boundary_at, snap::in_polygon};

use crate::{Command, Ctx, Input, Prompt, Step, hatch::closed_loop};

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Select,
    Option,
    Pattern,
    Scale,
    Angle,
    Order,
    Add,
    AddObjects,
    Remove,
}

pub struct HatchEdit {
    stage: Stage,
    id: Option<EntityId>,
    pattern: HatchPattern,
    scale: f64,
    angle: f64,
}

impl Default for HatchEdit {
    fn default() -> Self {
        Self { stage: Stage::Select, id: None, pattern: HatchPattern::Ansi31, scale: 1.0, angle: 0.0 }
    }
}

/// A loop's corners (its arcs' bulges left out), enough to tell what's
/// inside what.
fn ring(l: &[Vertex]) -> Vec<Point> {
    l.iter().map(|v| v.point).collect()
}

fn outline(l: &[Vertex]) -> Geometry {
    Geometry::Polyline { vertices: l.to_vec(), closed: true }
}

/// The loops grouped into separate areas: each outer loop with the
/// islands just inside it.
pub(crate) fn areas(loops: &[Vec<Vertex>]) -> Vec<Vec<Vec<Vertex>>> {
    let inside = |i: usize, j: usize| i != j && in_polygon(&ring(&loops[j]), loops[i][0].point);
    let depth: Vec<usize> = (0..loops.len()).map(|i| (0..loops.len()).filter(|&j| inside(i, j)).count()).collect();
    let mut areas: Vec<(usize, Vec<Vec<Vertex>>)> =
        (0..loops.len()).filter(|&i| depth[i].is_multiple_of(2)).map(|i| (i, vec![loops[i].clone()])).collect();
    for i in (0..loops.len()).filter(|&i| depth[i] % 2 == 1) {
        if let Some(area) = areas.iter_mut().find(|(o, _)| depth[*o] + 1 == depth[i] && inside(i, *o)) {
            area.1.push(loops[i].clone());
        }
    }
    areas.into_iter().map(|(_, a)| a).collect()
}

/// The objects a hatch's loops run along (what its boundary was made
/// from).
fn boundary_objects(doc: &Document, id: EntityId, loops: &[Vec<Vertex>]) -> Vec<EntityId> {
    let edges: Vec<_> = loops.iter().flat_map(|l| outline(l).segments()).collect();
    let on = |p: Point| edges.iter().any(|s| s.distance_to(p) < 1e-6);
    doc.space_in_draw_order(doc.space())
        .into_iter()
        .filter(|e| e.id != id && !matches!(e.geometry, Geometry::Hatch { .. }))
        .filter(|e| {
            let segs = e.geometry.segments();
            !segs.is_empty() && segs.iter().all(|s| on(s.start()) && on(s.end()) && on(s.point_at(0.5)))
        })
        .map(|e| e.id)
        .collect()
}

impl HatchEdit {
    fn loops(&self, ctx: &Ctx) -> Option<Vec<Vec<Vertex>>> {
        match &ctx.doc.entity(self.id?)?.geometry {
            Geometry::Hatch { loops, .. } => Some(loops.clone()),
            _ => None,
        }
    }

    fn set_loops(&self, ctx: &mut Ctx, new: Vec<Vec<Vertex>>) {
        let Some(id) = self.id else { return };
        ctx.doc.update(id, |e| {
            if let Geometry::Hatch { loops, .. } = &mut e.geometry {
                *loops = new;
            }
        });
    }

    fn set_properties(&mut self, ctx: &mut Ctx) -> Result<Step, String> {
        let (pattern, scale, angle) = (self.pattern, self.scale, self.angle);
        let id = self.id.ok_or("No hatch.")?;
        ctx.doc.update(id, |e| {
            if let Geometry::Hatch { pattern: p, scale: s, angle: a, .. } = &mut e.geometry {
                (*p, *s, *a) = (pattern, scale, angle);
            }
        });
        Ok(Step::Done)
    }

    fn option(&mut self, k: &str, ctx: &mut Ctx) -> Result<Step, String> {
        let id = self.id.ok_or("No hatch.")?;
        let loops = self.loops(ctx).ok_or("The hatch is gone.")?;
        match k {
            "DRaw order" => self.stage = Stage::Order,
            "ADd boundaries" => self.stage = Stage::Add,
            "Remove boundaries" => self.stage = Stage::Remove,
            "recreate Boundary" => {
                for l in &loops {
                    ctx.doc.add(outline(l));
                }
                ctx.messages.push(format!("{} boundary polyline(s) created.", loops.len()));
                return Ok(Step::Done);
            }
            "separate Hatches" => {
                let areas = areas(&loops);
                if areas.len() < 2 {
                    ctx.messages.push("The hatch has a single area; there's nothing to separate.".into());
                    return Ok(Step::Done);
                }
                let entity = ctx.doc.entity(id).cloned().ok_or("The hatch is gone.")?;
                let mut areas = areas.into_iter();
                self.set_loops(ctx, areas.next().unwrap_or_default());
                let mut made = Vec::new();
                for area in areas {
                    let mut g = entity.geometry.clone();
                    if let Geometry::Hatch { loops, .. } = &mut g {
                        *loops = area;
                    }
                    made.push(ctx.doc.add_like(g, &entity));
                }
                ctx.doc.place_under(&made, &[id]);
                ctx.messages.push(format!("The hatch was separated into {}.", made.len() + 1));
                return Ok(Step::Done);
            }
            _ => self.stage = Stage::Pattern,
        }
        Ok(Step::Continue)
    }
}

impl Command for HatchEdit {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select hatch object"),
            Stage::Option => Prompt::new("Enter hatch option")
                .with_keywords(&[
                    "Properties",
                    "DRaw order",
                    "ADd boundaries",
                    "Remove boundaries",
                    "recreate Boundary",
                    "separate Hatches",
                ])
                .with_default("Properties"),
            Stage::Pattern => Prompt::new("Enter a pattern name or [?/Solid]").with_default(self.pattern.name()),
            Stage::Scale => Prompt::new("Specify a scale for the pattern").with_default(format!("{:.4}", self.scale)),
            Stage::Angle => Prompt::new("Specify an angle for the pattern").with_default(format!("{:.0}", self.angle)),
            Stage::Order => Prompt::new("Enter draw order")
                .with_keywords(&[
                    "do not change",
                    "send to Back",
                    "bring to Front",
                    "send beHind boundary",
                    "bring in front of bounDary",
                ])
                .with_default("send beHind boundary"),
            Stage::Add => Prompt::new("Pick internal point").with_keywords(&["Select objects"]),
            Stage::AddObjects => Prompt::new("Select a closed object"),
            Stage::Remove => Prompt::new("Select boundary to remove"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::Select, Input::Pick { at, .. }) => {
                let e = ctx.pick(at).and_then(|id| ctx.doc.entity(id)).ok_or("No object found.")?;
                let Geometry::Hatch { pattern, scale, angle, .. } = &e.geometry else {
                    return Err("Select a hatch.".into());
                };
                (self.pattern, self.scale, self.angle) = (*pattern, *scale, *angle);
                self.id = Some(e.id);
                self.stage = Stage::Option;
            }
            (Stage::Option, Input::Keyword(k)) => return self.option(k, ctx),
            (Stage::Option, Input::Enter) => self.stage = Stage::Pattern,
            (Stage::Pattern, Input::Text(t)) if t.trim() == "?" => {
                let gradients = oxidraft_model::hatch::GradientKind::ALL.map(|k| k.name());
                let names: Vec<&str> = HatchPattern::ALL.iter().map(|p| p.name()).chain(gradients).collect();
                ctx.messages.push(format!("Patterns: {}", names.join(", ")));
            }
            (Stage::Pattern, Input::Text(t)) => {
                self.pattern = HatchPattern::from_name(t.trim())
                    .ok_or(format!("Unknown pattern \"{t}\". Enter ? to list them."))?;
                if self.pattern == HatchPattern::Solid {
                    return self.set_properties(ctx);
                }
                self.stage = Stage::Scale;
            }
            (Stage::Pattern, Input::Enter) if self.pattern == HatchPattern::Solid => return self.set_properties(ctx),
            (Stage::Pattern, Input::Enter) => self.stage = Stage::Scale,
            (Stage::Scale, Input::Value(v)) if v > EPSILON => {
                self.scale = v;
                self.stage = Stage::Angle;
            }
            (Stage::Scale, Input::Value(_)) => return Err("Requires a positive nonzero value.".into()),
            (Stage::Scale, Input::Enter) => self.stage = Stage::Angle,
            (Stage::Angle, Input::Value(a)) => {
                self.angle = a;
                return self.set_properties(ctx);
            }
            (Stage::Angle, Input::Enter) => return self.set_properties(ctx),
            (Stage::Order, input @ (Input::Keyword(_) | Input::Enter)) => {
                let id = self.id.ok_or("No hatch.")?;
                let loops = self.loops(ctx).unwrap_or_default();
                match input {
                    Input::Keyword("send to Back") => ctx.doc.send_to_back(&[id]),
                    Input::Keyword("bring to Front") => ctx.doc.bring_to_front(&[id]),
                    Input::Keyword("do not change") => {}
                    Input::Keyword("bring in front of bounDary") => {
                        let edges = boundary_objects(ctx.doc, id, &loops);
                        ctx.doc.place_above(&[id], &edges);
                    }
                    _ => {
                        let edges = boundary_objects(ctx.doc, id, &loops);
                        ctx.doc.place_under(&[id], &edges);
                    }
                }
                return Ok(Step::Done);
            }
            (Stage::Add, Input::Keyword(_)) => self.stage = Stage::AddObjects,
            (Stage::Add, Input::Pick { at, .. }) => {
                let found = boundary_at(ctx.doc, at).ok_or("Valid hatch boundary not found.")?;
                let mut loops = self.loops(ctx).unwrap_or_default();
                loops.extend(found);
                self.set_loops(ctx, loops);
            }
            (Stage::AddObjects, Input::Pick { at, .. }) => {
                let picked = ctx.pick(at).and_then(|id| ctx.doc.entity(id));
                let l = picked.and_then(|e| closed_loop(&e.geometry)).ok_or("Select a circle or a closed polyline.")?;
                let mut loops = self.loops(ctx).unwrap_or_default();
                loops.push(l);
                self.set_loops(ctx, loops);
            }
            (Stage::Remove, Input::Pick { at, .. } | Input::Point(at)) => {
                let mut loops = self.loops(ctx).unwrap_or_default();
                if loops.len() < 2 {
                    return Err("The hatch's last boundary can't be removed.".into());
                }
                let distance = |l: &[Vertex]| {
                    outline(l).segments().iter().map(|s| s.distance_to(at)).fold(f64::INFINITY, f64::min)
                };
                let nearest = (0..loops.len()).min_by(|&i, &j| distance(&loops[i]).total_cmp(&distance(&loops[j])));
                if let Some(i) = nearest {
                    loops.remove(i);
                    self.set_loops(ctx, loops);
                }
            }
            (Stage::Add | Stage::AddObjects | Stage::Remove | Stage::Select, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Pick a point or an option keyword.".into()),
        }
        Ok(Step::Continue)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.id.into_iter().collect()
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::Select | Stage::Add | Stage::AddObjects | Stage::Remove)
    }

    fn wants_text(&self) -> bool {
        self.stage == Stage::Pattern
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry, HatchPattern, Vertex};

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

    fn square(x: f64, y: f64, size: f64) -> Vec<Vertex> {
        [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .iter()
            .map(|(a, b)| Vertex::new(Point::new(x + a * size, y + b * size), 0.0))
            .collect()
    }

    fn hatches(d: &Document) -> Vec<(Vec<Vec<Vertex>>, HatchPattern, f64, f64)> {
        d.entities()
            .filter_map(|e| match &e.geometry {
                Geometry::Hatch { loops, pattern, scale, angle } => Some((loops.clone(), *pattern, *scale, *angle)),
                _ => None,
            })
            .collect()
    }

    fn doc() -> Document {
        let mut d = Document::new();
        // Two areas, the first with an island.
        let loops = vec![square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0), square(20.0, 0.0, 10.0)];
        d.add(Geometry::Hatch { loops, pattern: HatchPattern::Ansi31, scale: 1.0, angle: 0.0 });
        d
    }

    #[test]
    fn properties_boundaries_and_separating() {
        let (mut e, mut d) = (Engine::new(), doc());
        run(&mut e, &mut d, &["hatchedit"]);
        pick(&mut e, &mut d, 10.0, 5.0);
        run(&mut e, &mut d, &["", "net", "2", "45"]);
        assert!(!e.is_active());
        let h = &hatches(&d)[0];
        assert_eq!((h.1, h.2, h.3), (HatchPattern::Net, 2.0, 45.0));
        // Remove the island, then put it back by picking inside a polyline.
        run(&mut e, &mut d, &["he"]);
        pick(&mut e, &mut d, 10.0, 5.0);
        run(&mut e, &mut d, &["r"]);
        pick(&mut e, &mut d, 6.0, 5.0);
        run(&mut e, &mut d, &[""]);
        assert_eq!(hatches(&d)[0].0.len(), 2);
        d.add(Geometry::Polyline { vertices: square(4.0, 4.0, 2.0), closed: true });
        run(&mut e, &mut d, &["he"]);
        pick(&mut e, &mut d, 10.0, 5.0);
        run(&mut e, &mut d, &["ad", "s"]);
        pick(&mut e, &mut d, 6.0, 5.0);
        run(&mut e, &mut d, &[""]);
        assert_eq!(hatches(&d)[0].0.len(), 3);
        // Separate: the square with its island, and the other square.
        run(&mut e, &mut d, &["he"]);
        pick(&mut e, &mut d, 10.0, 5.0);
        run(&mut e, &mut d, &["h"]);
        let mut sizes: Vec<usize> = hatches(&d).iter().map(|h| h.0.len()).collect();
        sizes.sort();
        assert_eq!(sizes, [1, 2]);
        assert!(hatches(&d).iter().all(|h| h.1 == HatchPattern::Net));
        // Boundary polylines for one of them.
        let before = d.len();
        run(&mut e, &mut d, &["he"]);
        pick(&mut e, &mut d, 30.0, 5.0);
        run(&mut e, &mut d, &["b"]);
        assert_eq!(d.len(), before + 1);
    }

    #[test]
    fn draw_order_behind_or_in_front_of_its_boundary() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let other = d.add(Geometry::Circle { center: Point::new(50.0, 50.0), radius: 1.0 });
        let edge = d.add(Geometry::Polyline { vertices: square(0.0, 0.0, 10.0), closed: true });
        let hatch = d.add(Geometry::Hatch {
            loops: vec![square(0.0, 0.0, 10.0)],
            pattern: HatchPattern::Solid,
            scale: 1.0,
            angle: 0.0,
        });
        let order = |d: &Document| d.space_in_draw_order(d.space()).iter().map(|e| e.id).collect::<Vec<_>>();
        // Inside, where only the hatch is.
        run(&mut e, &mut d, &["he"]);
        pick(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &["dr", ""]);
        assert_eq!(order(&d), [other, hatch, edge], "just under its boundary");
        run(&mut e, &mut d, &["he"]);
        pick(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &["dr", "d"]);
        assert_eq!(order(&d), [other, edge, hatch]);
        run(&mut e, &mut d, &["he"]);
        pick(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &["dr", "b"]);
        assert_eq!(order(&d)[0], hatch);
    }
}

//! XCLIP: shows only part of a block reference or xref, inside (or
//! outside) a rectangle, a polygon or a polyline's outline. And CLIP,
//! which clips whatever is picked: a reference, an image or a PDF.

use oxidraft_geom::{Aabb, Point, curve::Seg};
use oxidraft_model::{Document, EntityId, Geometry, Vertex, XClip};

use crate::{Command, Ctx, Input, Prompt, Step, underlay::ImageClip};

const OPTIONS: &[&str] = &["ON", "OFF", "Delete", "generate Polyline", "New boundary"];
const KINDS: &[&str] = &["Select polyline", "Polygonal", "Rectangular", "Invert clip"];

#[derive(Clone, PartialEq)]
enum Stage {
    Select,
    Option(Vec<EntityId>),
    DeleteOld(Vec<EntityId>),
    Kind { ids: Vec<EntityId>, inverted: bool },
    Polyline { ids: Vec<EntityId>, inverted: bool },
    Corner { ids: Vec<EntityId>, inverted: bool },
    Opposite { ids: Vec<EntityId>, inverted: bool, first: Point },
    Polygon { ids: Vec<EntityId>, inverted: bool, points: Vec<Point> },
}

/// XCLIP (XC, CLIP): sets, turns on or off, removes or traces the
/// clipping boundary of block references and xrefs.
pub struct XClipCommand {
    stage: Stage,
}

impl Default for XClipCommand {
    fn default() -> Self {
        Self { stage: Stage::Select }
    }
}

fn clip_of(doc: &Document, id: EntityId) -> Option<&XClip> {
    match &doc.entity(id)?.geometry {
        Geometry::Insert { clip, .. } => clip.as_deref(),
        _ => None,
    }
}

fn update_clip(doc: &mut Document, ids: &[EntityId], f: impl Fn(&mut Option<Box<XClip>>)) {
    for &id in ids {
        doc.update(id, |e| {
            if let Geometry::Insert { clip, .. } = &mut e.geometry {
                f(clip);
            }
        });
    }
}

/// A polyline's outline as a polygon, arcs split into short chords.
fn outline(vertices: &[Vertex]) -> Vec<Point> {
    let n = vertices.len();
    let mut pts = Vec::new();
    for i in 0..n {
        let seg = Seg::from_bulge(vertices[i].point, vertices[(i + 1) % n].point, vertices[i].bulge);
        let steps = match seg {
            Seg::Line(_) => 1,
            Seg::Arc(a) => (a.sweep.abs() / 5.0).ceil().max(1.0) as usize,
        };
        pts.extend((0..steps).map(|k| seg.point_at(k as f64 / steps as f64)));
    }
    pts.dedup_by(|a, b| a.approx_eq(*b));
    pts
}

impl XClipCommand {
    /// Clips every reference in `ids` to the world `boundary`.
    fn clip(&self, ctx: &mut Ctx, ids: &[EntityId], boundary: &[Point], inverted: bool) -> Step {
        for &id in ids {
            let Some(Geometry::Insert { block, placement, .. }) = ctx.doc.entity(id).map(|e| &e.geometry) else {
                continue;
            };
            let Some(base) = ctx.doc.block(block).map(|b| b.base) else { continue };
            let placement = *placement;
            let local: Vec<Point> = boundary.iter().map(|p| placement.local(base, *p)).collect();
            ctx.doc.update(id, |e| {
                if let Geometry::Insert { clip, .. } = &mut e.geometry {
                    *clip = Some(Box::new(XClip { boundary: local.clone(), inverted, on: true }));
                }
            });
        }
        Step::Done
    }
}

impl Command for XClipCommand {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            Stage::Select => Prompt::new("Select objects"),
            Stage::Option(_) => {
                Prompt::new("Enter clipping option").with_keywords(OPTIONS).with_default("New boundary")
            }
            Stage::DeleteOld(_) => {
                Prompt::new("Delete old boundary(s)?").with_keywords(&["Yes", "No"]).with_default("Yes")
            }
            Stage::Kind { inverted, .. } => Prompt::new(if *inverted {
                "Inside mode - Objects inside boundary will be hidden.\nSpecify clipping boundary or select invert option"
            } else {
                "Outside mode - Objects outside boundary will be hidden.\nSpecify clipping boundary or select invert option"
            })
            .with_keywords(KINDS)
            .with_default("Rectangular"),
            Stage::Polyline { .. } => Prompt::new("Select polyline"),
            Stage::Corner { .. } => Prompt::new("Specify first corner"),
            Stage::Opposite { .. } => Prompt::new("Specify opposite corner"),
            Stage::Polygon { points, .. } if points.is_empty() => Prompt::new("Specify first point"),
            Stage::Polygon { .. } => Prompt::new("Specify next point").with_keywords(&["Undo"]),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (Stage::Select, Input::Selection(s)) => {
                let ids: Vec<EntityId> = s
                    .ids
                    .into_iter()
                    .filter(|id| matches!(ctx.doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Insert { .. })))
                    .collect();
                if ids.is_empty() {
                    ctx.messages.push("No block references or xrefs selected.".into());
                    return Ok(Step::Done);
                }
                self.stage = Stage::Option(ids);
            }
            (Stage::Option(ids), Input::Keyword(on @ ("ON" | "OFF"))) => {
                update_clip(ctx.doc, &ids, |c| {
                    if let Some(c) = c {
                        c.on = on == "ON";
                    }
                });
                return Ok(Step::Done);
            }
            (Stage::Option(ids), Input::Keyword("Delete")) => {
                update_clip(ctx.doc, &ids, |c| *c = None);
                return Ok(Step::Done);
            }
            (Stage::Option(ids), Input::Keyword("generate Polyline")) => {
                let mut made = 0;
                for id in ids {
                    let Some(e) = ctx.doc.entity(id) else { continue };
                    if let Some(boundary) = ctx.doc.clip_boundary(&e.geometry) {
                        let vertices = boundary.into_iter().map(Vertex::at).collect();
                        ctx.doc.add(Geometry::Polyline { vertices, closed: true });
                        made += 1;
                    }
                }
                if made == 0 {
                    ctx.messages.push("No clipping boundary to trace.".into());
                }
                return Ok(Step::Done);
            }
            (Stage::Option(ids), Input::Keyword(_) | Input::Enter) => {
                self.stage = if ids.iter().any(|id| clip_of(ctx.doc, *id).is_some()) {
                    Stage::DeleteOld(ids)
                } else {
                    Stage::Kind { ids, inverted: false }
                };
            }
            (Stage::DeleteOld(_), Input::Keyword("No")) => return Ok(Step::Done),
            (Stage::DeleteOld(ids), Input::Keyword(_) | Input::Enter) => {
                self.stage = Stage::Kind { ids, inverted: false }
            }
            (Stage::Kind { ids, inverted }, Input::Keyword("Invert clip")) => {
                self.stage = Stage::Kind { ids, inverted: !inverted }
            }
            (Stage::Kind { ids, inverted }, Input::Keyword("Select polyline")) => {
                self.stage = Stage::Polyline { ids, inverted }
            }
            (Stage::Kind { ids, inverted }, Input::Keyword("Polygonal")) => {
                self.stage = Stage::Polygon { ids, inverted, points: Vec::new() }
            }
            (Stage::Kind { ids, inverted }, Input::Keyword(_) | Input::Enter) => {
                self.stage = Stage::Corner { ids, inverted }
            }
            (Stage::Kind { ids, inverted }, Input::Point(first)) => {
                self.stage = Stage::Opposite { ids, inverted, first }
            }
            (Stage::Polyline { ids, inverted }, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                let Some(Geometry::Polyline { vertices, .. }) = ctx.doc.entity(id).map(|e| &e.geometry) else {
                    return Err("Select a polyline.".into());
                };
                let boundary = outline(vertices);
                if boundary.len() < 3 {
                    return Err("The polyline needs at least three vertices.".into());
                }
                return Ok(self.clip(ctx, &ids, &boundary, inverted));
            }
            (Stage::Corner { ids, inverted }, Input::Point(first)) => {
                self.stage = Stage::Opposite { ids, inverted, first }
            }
            (Stage::Opposite { ids, inverted, first }, Input::Point(p)) => {
                let b = Aabb::from_points(first, p);
                if b.width() <= 0.0 || b.height() <= 0.0 {
                    return Err("The boundary needs a width and a height.".into());
                }
                return Ok(self.clip(ctx, &ids, &b.corners(), inverted));
            }
            (Stage::Polygon { ids, inverted, mut points }, Input::Point(p)) => {
                points.push(p);
                self.stage = Stage::Polygon { ids, inverted, points };
            }
            (Stage::Polygon { ids, inverted, mut points }, Input::Keyword("Undo")) => {
                points.pop();
                self.stage = Stage::Polygon { ids, inverted, points };
            }
            (Stage::Polygon { ids, inverted, points }, Input::Enter) => {
                if points.len() >= 3 {
                    return Ok(self.clip(ctx, &ids, &points, inverted));
                }
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Select, _) => return Err("Select block references or xrefs.".into()),
            (Stage::Polyline { .. }, _) => return Err("Select a polyline.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match &self.stage {
            Stage::Opposite { first, .. } => {
                let b = Aabb::from_points(*first, cursor);
                vec![Geometry::Polyline { vertices: b.corners().into_iter().map(Vertex::at).collect(), closed: true }]
            }
            Stage::Polygon { points, .. } if !points.is_empty() => {
                let mut all = points.clone();
                all.push(cursor);
                vec![Geometry::Polyline { vertices: all.into_iter().map(Vertex::at).collect(), closed: true }]
            }
            _ => Vec::new(),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match &self.stage {
            Stage::Select => Vec::new(),
            Stage::Option(ids)
            | Stage::DeleteOld(ids)
            | Stage::Kind { ids, .. }
            | Stage::Polyline { ids, .. }
            | Stage::Corner { ids, .. }
            | Stage::Opposite { ids, .. }
            | Stage::Polygon { ids, .. } => ids.clone(),
        }
    }

    fn wants_selection(&self) -> bool {
        self.stage == Stage::Select
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::Polyline { .. })
    }
}

/// CLIP: clips the picked xref or block reference (as XCLIP does), or
/// image or PDF underlay (as IMAGECLIP does).
#[derive(Default)]
pub struct Clip {
    inner: Option<Box<dyn Command>>,
}

impl Command for Clip {
    fn prompt(&self) -> Prompt {
        self.inner.as_ref().map_or_else(|| Prompt::new("Select Object to clip"), |c| c.prompt())
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if let Some(inner) = &mut self.inner {
            return inner.input(input, ctx);
        }
        match input {
            Input::Pick { at, .. } => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                self.inner = Some(match ctx.doc.entity(id).map(|e| &e.geometry) {
                    Some(Geometry::Insert { .. }) => Box::new(XClipCommand { stage: Stage::Option(vec![id]) }),
                    Some(Geometry::Image { .. }) => Box::new(ImageClip::picked(id)),
                    _ => return Err("Select an xref, block reference, image or PDF underlay.".into()),
                });
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select an xref, block reference, image or PDF underlay.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.inner.as_ref().map_or_else(Vec::new, |c| c.preview(cursor))
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.inner.as_ref().map_or_else(Vec::new, |c| c.highlighted())
    }

    fn wants_selection(&self) -> bool {
        self.inner.as_ref().is_some_and(|c| c.wants_selection())
    }

    fn wants_pick(&self) -> bool {
        self.inner.as_ref().is_none_or(|c| c.wants_pick())
    }
}

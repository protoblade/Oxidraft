//! IMAGEATTACH and PDFATTACH's placement (the picture's frame follows the
//! cursor until its corner is picked), and IMAGECLIP.

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Color, Document, EntityId, Geometry, ImageDef, Vertex, image_corners, sanitize_name};

use crate::{Command, Ctx, Input, Prompt, Step};

fn frame(origin: Point, u: Point, v: Point) -> Geometry {
    let corners = image_corners(origin, u, v);
    Geometry::Polyline { vertices: corners.into_iter().map(Vertex::at).collect(), closed: true }
}

/// Places an image or PDF page: defines it the first time, then adds a
/// picture of it, `u` across and `v` up from its corner, on a layer of its
/// own name.
pub struct AttachImage {
    def: ImageDef,
    u: Point,
    v: Point,
}

impl AttachImage {
    pub fn new(def: ImageDef, u: Point, v: Point) -> Self {
        Self { def, u, v }
    }

    pub fn attach(&self, doc: &mut Document, at: Point) {
        if doc.image_def(&self.def.name).is_none() {
            doc.set_image_def(self.def.clone());
        }
        let image =
            Geometry::Image { image: self.def.name.clone(), origin: at, u: self.u, v: self.v, clip: Vec::new() };
        // Pictures are underlays: behind the drawing.
        let id = doc.add_on_layer(image, &sanitize_name(&self.def.name), Color::ByLayer);
        doc.send_to_back(&[id]);
    }
}

impl Command for AttachImage {
    fn prompt(&self) -> Prompt {
        Prompt::new("Specify insertion point")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => {
                self.attach(ctx.doc, p);
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Specify an insertion point.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        vec![frame(cursor, self.u, self.v)]
    }
}

#[derive(Clone, PartialEq)]
enum Stage {
    Pick,
    Option { id: EntityId },
    Kind { id: EntityId },
    Corner { id: EntityId },
    Opposite { id: EntityId, first: Point },
    Polygon { id: EntityId, points: Vec<Point> },
}

/// IMAGECLIP (and PDFCLIP): clips an image or PDF underlay to a rectangle
/// or polygon, or removes its clipping.
pub struct ImageClip {
    stage: Stage,
}

impl Default for ImageClip {
    fn default() -> Self {
        Self { stage: Stage::Pick }
    }
}

impl ImageClip {
    /// Past "Select image to clip", with `id` picked (CLIP picks it).
    pub(crate) fn picked(id: EntityId) -> Self {
        Self { stage: Stage::Option { id } }
    }
}

fn set_clip(ctx: &mut Ctx, id: EntityId, boundary: Vec<Point>) {
    ctx.doc.update(id, |e| {
        if let Geometry::Image { clip, .. } = &mut e.geometry {
            *clip = boundary;
        }
    });
}

impl Command for ImageClip {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            Stage::Pick => Prompt::new("Select image to clip"),
            Stage::Option { .. } => Prompt::new("Enter image clipping option")
                .with_keywords(&["Delete", "New boundary"])
                .with_default("New boundary"),
            Stage::Kind { .. } => Prompt::new("Enter clipping type")
                .with_keywords(&["Polygonal", "Rectangular"])
                .with_default("Rectangular"),
            Stage::Corner { .. } => Prompt::new("Specify first corner point"),
            Stage::Opposite { .. } => Prompt::new("Specify opposite corner point"),
            Stage::Polygon { points, .. } if points.is_empty() => Prompt::new("Specify first point"),
            Stage::Polygon { .. } => Prompt::new("Specify next point").with_keywords(&["Undo"]),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (Stage::Pick, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                if !matches!(ctx.doc.entity(id).map(|e| &e.geometry), Some(Geometry::Image { .. })) {
                    return Err("Select an image or PDF underlay.".into());
                }
                self.stage = Stage::Option { id };
            }
            (Stage::Option { id }, Input::Keyword("Delete")) => {
                set_clip(ctx, id, Vec::new());
                return Ok(Step::Done);
            }
            (Stage::Option { id }, Input::Keyword(_) | Input::Enter) => self.stage = Stage::Kind { id },
            (Stage::Kind { id }, Input::Keyword("Polygonal")) => self.stage = Stage::Polygon { id, points: Vec::new() },
            (Stage::Kind { id }, Input::Keyword(_) | Input::Enter) => self.stage = Stage::Corner { id },
            (Stage::Corner { id }, Input::Point(first)) => self.stage = Stage::Opposite { id, first },
            (Stage::Opposite { id, first }, Input::Point(p)) => {
                let b = Aabb::from_points(first, p);
                if b.width() <= 0.0 || b.height() <= 0.0 {
                    return Err("The boundary needs a width and a height.".into());
                }
                set_clip(ctx, id, b.corners().to_vec());
                return Ok(Step::Done);
            }
            (Stage::Polygon { id, mut points }, Input::Point(p)) => {
                points.push(p);
                self.stage = Stage::Polygon { id, points };
            }
            (Stage::Polygon { id, mut points }, Input::Keyword("Undo")) => {
                points.pop();
                self.stage = Stage::Polygon { id, points };
            }
            (Stage::Polygon { id, points }, Input::Enter) => {
                if points.len() >= 3 {
                    set_clip(ctx, id, points);
                }
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Pick, _) => return Err("Select an image or PDF underlay.".into()),
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
            Stage::Pick => Vec::new(),
            Stage::Option { id }
            | Stage::Kind { id }
            | Stage::Corner { id }
            | Stage::Opposite { id, .. }
            | Stage::Polygon { id, .. } => vec![*id],
        }
    }

    fn wants_pick(&self) -> bool {
        self.stage == Stage::Pick
    }
}

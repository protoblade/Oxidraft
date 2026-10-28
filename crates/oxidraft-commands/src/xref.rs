//! XATTACH's placement: the attached drawing follows the cursor until its
//! insertion point is picked.

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Block, Document, Geometry, Placement, Vertex, Xref};

use crate::{Command, Ctx, Input, Prompt, Step};

/// Previews of drawings bigger than this show their extents instead.
const PREVIEW_LIMIT: usize = 3000;

/// Places an attached drawing: defines it the first time, then adds a
/// reference.
pub struct AttachXref {
    xref: Xref,
    content: Document,
    scale: f64,
    rotation: f64,
    /// What the drawing looks like, around its base point.
    outline: Vec<Geometry>,
}

impl AttachXref {
    pub fn new(xref: Xref, content: Document, scale: f64, rotation: f64) -> Self {
        let mut outline: Vec<Geometry> = content.entities().flat_map(|e| content.expanded(e)).collect();
        if outline.len() > PREVIEW_LIMIT
            && let Some(b) = content.extents()
        {
            outline = vec![frame(&b)];
        }
        Self { xref, content, scale, rotation, outline }
    }

    fn placement(&self, at: Point) -> Placement {
        Placement { position: at, scale: self.scale, rotation: self.rotation, mirror: false }
    }

    /// Attaches the drawing (if it isn't yet) and references it at `at`.
    pub fn attach(&self, doc: &mut Document, at: Point) {
        if doc.xref(&self.xref.name).is_none() {
            doc.define_block(Block {
                name: self.xref.name.clone(),
                base: self.content.insbase(),
                entities: Vec::new(),
            });
            doc.set_xref(self.xref.clone());
            doc.load_xref(&self.xref.name, &self.content);
        }
        // Each attached drawing goes on a layer of its own name, so a layer
        // shows or hides all of it.
        let name = doc.xref(&self.xref.name).map_or(self.xref.name.clone(), |x| x.name.clone());
        let layer = oxidraft_model::sanitize_name(&name);
        let insert =
            Geometry::Insert { block: name, placement: self.placement(at), attributes: Vec::new(), clip: None };
        doc.add_on_layer(insert, &layer, oxidraft_model::Color::ByLayer);
    }
}

fn frame(b: &Aabb) -> Geometry {
    Geometry::Polyline { vertices: b.corners().into_iter().map(Vertex::at).collect(), closed: true }
}

impl Command for AttachXref {
    fn prompt(&self) -> Prompt {
        Prompt::new("Specify insertion point")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => {
                self.attach(ctx.doc, p);
                ctx.messages.push(format!("Attach Xref \"{}\": {}", self.xref.name, self.xref.path));
                ctx.messages.push(format!("\"{}\" loaded.", self.xref.name));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Specify an insertion point.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let placement = self.placement(cursor);
        self.outline.iter().map(|g| placement.place(self.content.insbase(), g)).collect()
    }
}

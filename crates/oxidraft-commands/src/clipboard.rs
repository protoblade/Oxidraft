//! COPYCLIP, CUTCLIP, COPYBASE and PASTECLIP: copy and paste objects, with
//! the layers, linetypes and blocks they use. The clipboard holds a small
//! drawing whose INSBASE is the base point; the app puts it on the system
//! clipboard as DXF.

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{BlockEntity, Document, EntityId, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step};

/// Block names `g` uses, nested ones included.
fn blocks_used(doc: &Document, g: &Geometry, out: &mut Vec<String>) {
    let Geometry::Insert { block, .. } = g else { return };
    let Some(b) = doc.block(block) else { return };
    if out.iter().any(|n| n.eq_ignore_ascii_case(&b.name)) {
        return;
    }
    out.push(b.name.clone());
    for be in &b.entities {
        blocks_used(doc, &be.geometry, out);
    }
}

/// A drawing holding copies of `ids` and what they need, with `base` as its
/// INSBASE.
pub fn clip_of(doc: &Document, ids: &[EntityId], base: Point) -> Document {
    let entities: Vec<BlockEntity> = ids
        .iter()
        .filter_map(|&id| doc.entity(id))
        .map(|e| BlockEntity {
            layer: e.layer.clone(),
            color: e.color,
            linetype: e.linetype.clone(),
            lineweight: e.lineweight,
            geometry: e.geometry.clone(),
        })
        .collect();
    excerpt(doc, &entities, base)
}

/// WBLOCK's Block source: a drawing of the block's contents and what they
/// need, its base point as INSBASE. `None` if there's no such block.
pub fn block_drawing(doc: &Document, name: &str) -> Option<Document> {
    let block = doc.block(name)?;
    Some(excerpt(doc, &block.entities, block.base))
}

/// A drawing of `entities` with the layers, linetypes and blocks they use,
/// in `doc`'s units.
fn excerpt(doc: &Document, entities: &[BlockEntity], base: Point) -> Document {
    let mut blocks = Vec::new();
    for e in entities {
        blocks_used(doc, &e.geometry, &mut blocks);
    }
    let mut layers: Vec<&str> = Vec::new();
    for e in entities {
        layers.push(&e.layer);
        if let Geometry::Insert { attributes, .. } = &e.geometry {
            layers.extend(attributes.iter().map(|a| a.layer.as_str()));
        }
    }
    for b in blocks.iter().filter_map(|n| doc.block(n)) {
        layers.extend(b.entities.iter().map(|be| be.layer.as_str()));
    }

    let mut clip = Document::new();
    for layer in layers.into_iter().filter_map(|n| doc.layer(n)) {
        clip.add_layer(layer.clone());
        clip.update_layer(layer.clone());
    }
    for lt in doc.linetypes() {
        clip.define_linetype(lt);
    }
    for b in blocks.iter().filter_map(|n| doc.block(n)) {
        clip.define_block(b.clone());
    }
    for e in entities {
        clip.add_styled(e.geometry.clone(), &e.layer, e.color, &e.linetype, e.lineweight);
    }
    clip.set_insbase(base);
    clip.set_units(doc.units());
    clip
}

/// Adds the clipboard drawing's objects to `doc`, moved by `offset`, with
/// the layers, linetypes and blocks they use that `doc` doesn't have yet
/// (existing ones are kept, as in AutoCAD). Returns the new objects.
pub fn paste(doc: &mut Document, clip: &Document, offset: Point) -> Vec<EntityId> {
    for layer in clip.layers() {
        if doc.layer(&layer.name).is_none() {
            doc.add_layer(layer.clone());
            doc.update_layer(layer.clone());
        }
    }
    for lt in clip.linetypes() {
        if doc.linetype(&lt.name).is_none() {
            doc.define_linetype(lt);
        }
    }
    for b in clip.blocks() {
        if doc.block(&b.name).is_none() {
            doc.define_block(b.clone());
        }
    }
    clip.entities()
        .map(|e| doc.add_styled(e.geometry.translated(offset), &e.layer, e.color, &e.linetype, e.lineweight))
        .collect()
}

/// The lower-left corner of the objects: COPYCLIP's base point.
fn lower_left(doc: &Document, ids: &[EntityId]) -> Point {
    ids.iter()
        .filter_map(|&id| doc.entity(id))
        .map(|e| doc.entity_bounds(e))
        .reduce(Aabb::union)
        .map_or(Point::ORIGIN, |b| b.min)
}

/// COPYCLIP and CUTCLIP (base point: the objects' lower-left corner), and
/// COPYBASE (asks for the base point first).
pub struct CopyClip {
    cut: bool,
    /// COPYBASE: the base point, once picked.
    base: Option<Point>,
    ask_base: bool,
    preselected: Vec<EntityId>,
}

impl CopyClip {
    pub fn copy() -> Self {
        Self { cut: false, base: None, ask_base: false, preselected: Vec::new() }
    }

    pub fn cut() -> Self {
        Self { cut: true, ..Self::copy() }
    }

    pub fn with_base() -> Self {
        Self { ask_base: true, ..Self::copy() }
    }

    fn copy_objects(&self, ctx: &mut Ctx, ids: &[EntityId]) -> Step {
        if ids.is_empty() {
            return Step::Done;
        }
        let base = self.base.unwrap_or_else(|| lower_left(ctx.doc, ids));
        ctx.copied = Some(clip_of(ctx.doc, ids, base));
        if self.cut {
            for &id in ids {
                ctx.doc.remove(id);
            }
        }
        Step::Done
    }
}

impl Command for CopyClip {
    fn prompt(&self) -> Prompt {
        if self.ask_base && self.base.is_none() {
            Prompt::new("Specify base point")
        } else {
            Prompt::new("Select objects")
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) if self.ask_base && self.base.is_none() => {
                self.base = Some(p);
                // Objects selected before COPYBASE are used right away.
                let ids = std::mem::take(&mut self.preselected);
                Ok(if ids.is_empty() { Step::Continue } else { self.copy_objects(ctx, &ids) })
            }
            Input::Selection(s) => Ok(self.copy_objects(ctx, &s.ids)),
            Input::Enter => Ok(Step::Done),
            _ if self.ask_base && self.base.is_none() => Err("Specify a point.".into()),
            _ => Err("Select objects, then press Enter.".into()),
        }
    }

    fn preselected(&mut self, ids: &[EntityId]) {
        self.preselected = ids.to_vec();
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.preselected.clone()
    }

    fn wants_selection(&self) -> bool {
        !self.ask_base || self.base.is_some()
    }
}

/// PASTECLIP: places the clipboard's objects at a point, previewing them
/// on the cursor.
pub struct Paste {
    clip: Document,
    /// What the objects look like (blocks expanded), at their copied place.
    outline: Vec<Geometry>,
}

impl Paste {
    pub fn new(clip: Document) -> Self {
        let outline = clip.entities().flat_map(|e| clip.expanded(e)).collect();
        Self { clip, outline }
    }
}

impl Command for Paste {
    fn prompt(&self) -> Prompt {
        Prompt::new("Specify insertion point")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => {
                paste(ctx.doc, &self.clip, p - self.clip.insbase());
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Specify an insertion point.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let d = cursor - self.clip.insbase();
        self.outline.iter().map(|g| g.translated(d)).collect()
    }
}

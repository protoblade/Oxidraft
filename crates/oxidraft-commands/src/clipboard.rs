//! COPYCLIP, CUTCLIP, COPYBASE and PASTECLIP: copy and paste objects, with
//! the layers, linetypes, blocks and pictures they use, their hyperlinks,
//! and the files kept in the drawing that those are (INSERTOBJ's, OLE
//! objects' documents and pictures). The clipboard holds a small drawing
//! whose INSBASE is the base point; the app puts it on the system clipboard
//! as DXF.

use std::collections::HashMap;

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{BlockEntity, Document, EMBEDDED_PREFIX, EntityId, Geometry, Hyperlink, ImageDef};

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

/// The picture `g` shows (an image or PDF underlay's definition), if it's
/// one not in `out` yet.
fn image_used(g: &Geometry, out: &mut Vec<String>) {
    if let Geometry::Image { image, .. } = g
        && !out.iter().any(|n| n.eq_ignore_ascii_case(image))
    {
        out.push(image.clone());
    }
}

/// The file at `address`, if it's one kept in `doc`, kept in `clip` too.
fn carry_file(doc: &Document, clip: &mut Document, address: &str) {
    if let Some(name) = address.strip_prefix(EMBEDDED_PREFIX)
        && let Some(bytes) = doc.embedded_file(name)
    {
        clip.embed_file(name, bytes.to_vec());
    }
}

/// A drawing holding copies of `ids` and what they need, with `base` as its
/// INSBASE.
pub fn clip_of(doc: &Document, ids: &[EntityId], base: Point) -> Document {
    let (entities, links): (Vec<BlockEntity>, Vec<Option<Hyperlink>>) = ids
        .iter()
        .filter_map(|&id| doc.entity(id))
        .map(|e| {
            let copy = BlockEntity {
                layer: e.layer.clone(),
                color: e.color,
                linetype: e.linetype.clone(),
                lineweight: e.lineweight,
                geometry: e.geometry.clone(),
            };
            (copy, e.hyperlink.as_deref().cloned())
        })
        .unzip();
    excerpt(doc, &entities, &links, base)
}

/// WBLOCK's Block source: a drawing of the block's contents and what they
/// need, its base point as INSBASE. `None` if there's no such block.
pub fn block_drawing(doc: &Document, name: &str) -> Option<Document> {
    let block = doc.block(name)?;
    Some(excerpt(doc, &block.entities, &[], block.base))
}

/// A drawing of `entities` (each with its hyperlink in `links`, if it has
/// one) with the layers, linetypes, blocks and pictures they use, and the
/// files kept in `doc` that those and the links are, in `doc`'s units.
fn excerpt(doc: &Document, entities: &[BlockEntity], links: &[Option<Hyperlink>], base: Point) -> Document {
    let mut blocks = Vec::new();
    for e in entities {
        blocks_used(doc, &e.geometry, &mut blocks);
    }
    let mut images = Vec::new();
    for e in entities {
        image_used(&e.geometry, &mut images);
    }
    for b in blocks.iter().filter_map(|n| doc.block(n)) {
        for be in &b.entities {
            image_used(&be.geometry, &mut images);
        }
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
    for def in images.iter().filter_map(|n| doc.image_def(n)) {
        clip.set_image_def(def.clone());
        carry_file(doc, &mut clip, &def.path);
    }
    for (i, e) in entities.iter().enumerate() {
        let id = clip.add_styled(e.geometry.clone(), &e.layer, e.color, &e.linetype, e.lineweight);
        if let Some(link) = links.get(i).cloned().flatten() {
            carry_file(doc, &mut clip, &link.url);
            clip.update(id, |c| c.hyperlink = Some(Box::new(link)));
        }
    }
    clip.set_insbase(base);
    clip.set_units(doc.units());
    clip
}

/// Adds the clipboard drawing's objects to `doc`, moved by `offset`, with
/// their hyperlinks, and the layers, linetypes, blocks, pictures and kept
/// files they use that `doc` doesn't have yet (existing ones are kept, as
/// in AutoCAD). Returns the new objects.
pub fn paste(doc: &mut Document, clip: &Document, offset: Point) -> Vec<EntityId> {
    let renamed = paste_definitions(doc, clip);
    clip.entities()
        .map(|e| {
            let mut geometry = e.geometry.translated(offset);
            renamed.image_of(&mut geometry);
            let id = doc.add_styled(geometry, &e.layer, e.color, &e.linetype, e.lineweight);
            if let Some(link) = &e.hyperlink {
                let link = Hyperlink { url: renamed.address(&link.url), description: link.description.clone() };
                doc.update(id, |n| n.hyperlink = Some(Box::new(link)));
            }
            id
        })
        .collect()
}

/// What pasting named apart from what `doc` had under the clipboard's
/// names: files kept in the drawing, and pictures' definitions (by the
/// clipboard's name, in capitals).
#[derive(Default)]
pub(crate) struct Renamed {
    files: HashMap<String, String>,
    images: HashMap<String, String>,
}

impl Renamed {
    /// `g`'s picture, by its name in the drawing.
    pub(crate) fn image_of(&self, g: &mut Geometry) {
        if let Geometry::Image { image, .. } = g
            && let Some(name) = self.images.get(&image.to_ascii_uppercase())
        {
            *image = name.clone();
        }
    }

    /// `address`, a kept file's by its name in the drawing.
    fn address(&self, address: &str) -> String {
        match address.strip_prefix(EMBEDDED_PREFIX).and_then(|n| self.files.get(n)) {
            Some(name) => format!("{EMBEDDED_PREFIX}{name}"),
            None => address.to_string(),
        }
    }
}

/// The clipboard drawing's layers, linetypes, blocks, kept files and
/// pictures that `doc` hasn't got yet; a file or picture of the same name
/// but not the same is named apart.
pub(crate) fn paste_definitions(doc: &mut Document, clip: &Document) -> Renamed {
    let mut renamed = Renamed::default();
    for (name, bytes) in clip.embedded_files() {
        let kept = doc.embed_file(name, bytes.to_vec());
        if kept != *name {
            renamed.files.insert(name.clone(), kept);
        }
    }
    for def in clip.image_defs() {
        let def = ImageDef { path: renamed.address(&def.path), ..def.clone() };
        let same = |d: &ImageDef| d.path == def.path && d.page == def.page;
        let name = (1..)
            .map(|i| if i == 1 { def.name.clone() } else { format!("{}({i})", def.name) })
            .find(|n| doc.image_def(n).is_none_or(same))
            .unwrap_or_default();
        if doc.image_def(&name).is_none() {
            doc.set_image_def(ImageDef { name: name.clone(), ..def.clone() });
        }
        if name != def.name {
            renamed.images.insert(def.name.to_ascii_uppercase(), name);
        }
    }
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
    renamed
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

#[cfg(test)]
mod tests {
    use oxidraft_model::XrefStatus;

    use super::*;

    /// A drawing with picture "OLE1" of file OLE1.png (`picture`), linked to
    /// OLE1.xlsx (`document`).
    fn with_picture(picture: &[u8], document: &[u8]) -> (Document, EntityId) {
        let mut doc = Document::new();
        doc.embed_file("OLE1.png", picture.to_vec());
        doc.embed_file("OLE1.xlsx", document.to_vec());
        doc.set_image_def(ImageDef {
            name: "OLE1".into(),
            path: format!("{EMBEDDED_PREFIX}OLE1.png"),
            page: None,
            width: 0.0,
            height: 0.0,
            status: XrefStatus::Loaded,
        });
        let id = doc.add(Geometry::Image {
            image: "OLE1".into(),
            origin: Point::new(5.0, 5.0),
            u: Point::new(4.0, 0.0),
            v: Point::new(0.0, 2.0),
            clip: Vec::new(),
            adjust: Default::default(),
        });
        let link = Hyperlink { url: format!("{EMBEDDED_PREFIX}OLE1.xlsx"), description: "Document".into() };
        doc.update(id, |e| e.hyperlink = Some(Box::new(link)));
        (doc, id)
    }

    fn pasted(doc: &Document, ids: &[EntityId]) -> (String, String) {
        let e = doc.entity(ids[0]).unwrap();
        let Geometry::Image { image, .. } = &e.geometry else { panic!("{:?}", e.geometry) };
        (image.clone(), e.hyperlink.as_ref().unwrap().url.clone())
    }

    #[test]
    fn pictures_and_their_documents_copy_and_paste() {
        let (source, id) = with_picture(b"red", b"pumps");
        let clip = clip_of(&source, &[id], Point::new(5.0, 5.0));
        assert_eq!(clip.image_def("OLE1").map(|d| d.path.as_str()), Some("embedded:OLE1.png"));
        assert_eq!(clip.embedded_files().count(), 2, "the picture and the document");

        // Back into the same drawing: the same picture and document.
        let mut same = source.clone();
        let ids = paste(&mut same, &clip, Point::new(10.0, 0.0));
        assert_eq!(pasted(&same, &ids), ("OLE1".into(), "embedded:OLE1.xlsx".into()));
        assert_eq!((same.image_defs().count(), same.embedded_files().count()), (1, 2));

        // Into one with other files and a picture of the same names: named apart.
        let (mut other, _) = with_picture(b"blue", b"valves");
        let ids = paste(&mut other, &clip, Point::ORIGIN);
        assert_eq!(pasted(&other, &ids), ("OLE1(2)".into(), "embedded:OLE1 (2).xlsx".into()));
        let def = other.image_def("OLE1(2)").unwrap();
        assert_eq!(def.path, "embedded:OLE1 (2).png");
        assert_eq!(other.embedded_file("OLE1 (2).png"), Some(&b"red"[..]));
        assert_eq!(other.embedded_file("OLE1.png"), Some(&b"blue"[..]), "its own kept");

        // PASTEBLOCK's block shows the pasted picture too.
        let (mut third, _) = with_picture(b"green", b"tanks");
        let one = crate::block_tools::as_block(&mut third, &clip);
        let Some(Geometry::Insert { block, .. }) = one.entities().next().map(|e| &e.geometry) else { panic!() };
        let inside = &third.block(block).unwrap().entities[0].geometry;
        assert!(matches!(inside, Geometry::Image { image, .. } if image == "OLE1(2)"), "{inside:?}");
    }
}

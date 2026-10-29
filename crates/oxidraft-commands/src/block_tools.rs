//! SETBYLAYER, NCOPY and -ATTEDIT; ATTEDIT (the app's attribute editor);
//! and PASTEBLOCK's block.

use oxidraft_geom::Point;
use oxidraft_model::{
    Block, BlockEntity, Color, Document, Entity, EntityId, Geometry, Lineweight, Placement, linetype,
};

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest};

#[derive(Clone, Copy, PartialEq)]
enum ByLayerStage {
    Select,
    ByBlock,
    Blocks,
}

/// SETBYLAYER: the selected objects' color, linetype and lineweight
/// ByLayer; ByBlock ones too if asked, and inside blocks if asked.
pub struct SetByLayer {
    stage: ByLayerStage,
    ids: Vec<EntityId>,
    by_block: bool,
}

impl Default for SetByLayer {
    fn default() -> Self {
        Self { stage: ByLayerStage::Select, ids: Vec::new(), by_block: true }
    }
}

/// `color`, `linetype` and `lineweight` made ByLayer (ByBlock ones only
/// with `by_block`). True if any changed.
fn to_by_layer(color: &mut Color, lt: &mut String, lw: &mut Lineweight, by_block: bool) -> bool {
    let before = (*color, lt.clone(), *lw);
    if *color != Color::ByBlock || by_block {
        *color = Color::ByLayer;
    }
    if !linetype::is_by_block(lt) || by_block {
        *lt = linetype::BYLAYER.into();
    }
    if *lw != Lineweight::ByBlock || by_block {
        *lw = Lineweight::ByLayer;
    }
    before != (*color, lt.clone(), *lw)
}

impl Command for SetByLayer {
    fn start(&mut self, _vars: &crate::Vars) -> Option<String> {
        Some("Current active settings: Color Linetype Lineweight".into())
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            ByLayerStage::Select => Prompt::new("Select objects"),
            ByLayerStage::ByBlock => {
                Prompt::new("Change ByBlock to ByLayer?").with_keywords(&["Yes", "No"]).with_default("Yes")
            }
            ByLayerStage::Blocks => Prompt::new("Include blocks?").with_keywords(&["Yes", "No"]).with_default("Yes"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (ByLayerStage::Select, Input::Selection(s)) => {
                if s.ids.is_empty() {
                    return Ok(Step::Done);
                }
                self.ids = s.ids;
                self.stage = ByLayerStage::ByBlock;
            }
            (ByLayerStage::ByBlock, input @ (Input::Keyword(_) | Input::Enter)) => {
                self.by_block = input != Input::Keyword("No");
                self.stage = ByLayerStage::Blocks;
            }
            (ByLayerStage::Blocks, input @ (Input::Keyword(_) | Input::Enter)) => {
                let blocks = input != Input::Keyword("No");
                let by_block = self.by_block;
                let mut changed = 0;
                let mut names: Vec<String> = Vec::new();
                for id in &self.ids {
                    let mut any = false;
                    ctx.doc.update(*id, |e| {
                        any = to_by_layer(&mut e.color, &mut e.linetype, &mut e.lineweight, by_block);
                        if let Geometry::Insert { block, .. } = &e.geometry {
                            names.push(block.clone());
                        }
                    });
                    changed += usize::from(any);
                }
                if blocks {
                    names.sort();
                    names.dedup();
                    for name in names {
                        let Some(mut b) = ctx.doc.block(&name).cloned() else { continue };
                        for be in &mut b.entities {
                            to_by_layer(&mut be.color, &mut be.linetype, &mut be.lineweight, by_block);
                        }
                        ctx.doc.define_block(b);
                    }
                }
                ctx.messages.push(format!("{changed} object(s) changed to ByLayer."));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        self.stage == ByLayerStage::Select
    }
}

#[derive(Clone, Copy, PartialEq)]
enum NCopyStage {
    Pick,
    Base,
    Second(Point),
}

/// NCOPY: objects inside block references (or external references)
/// copied out into the drawing, in place (Insert) or moved by two points.
pub struct NCopy {
    stage: NCopyStage,
    picked: Vec<Entity>,
}

impl Default for NCopy {
    fn default() -> Self {
        Self { stage: NCopyStage::Pick, picked: Vec::new() }
    }
}

impl NCopy {
    /// The part of a block reference nearest `at`, in the drawing's
    /// coordinates, with its look.
    fn nested(doc: &Document, at: Point, tolerance: f64) -> Option<Entity> {
        let id = oxidraft_model::snap::pick(doc, at, tolerance)?;
        let e = doc.entity(id)?;
        if !matches!(e.geometry, Geometry::Insert { .. }) {
            return None;
        }
        let parts = doc.expanded_styled(e);
        let part = parts.into_iter().min_by(|a, b| {
            let d = |g: &Geometry| g.segments().iter().map(|s| s.distance_to(at)).fold(f64::INFINITY, f64::min);
            d(&a.geometry).total_cmp(&d(&b.geometry))
        })?;
        Some(Entity {
            id,
            space: e.space,
            layer: part.layer,
            color: Color::Index(part.color),
            linetype: part.linetype,
            lineweight: part.lineweight,
            transparency: e.transparency,
            geometry: part.geometry,
        })
    }

    fn add(&self, ctx: &mut Ctx, offset: Point) -> usize {
        for e in &self.picked {
            ctx.doc.add_styled(e.geometry.translated(offset), &e.layer, e.color, &e.linetype, e.lineweight);
        }
        self.picked.len()
    }
}

impl Command for NCopy {
    fn prompt(&self) -> Prompt {
        match self.stage {
            NCopyStage::Pick => Prompt::new("Select nested objects to copy"),
            NCopyStage::Base => Prompt::new("Specify base point").with_keywords(&["Insert"]).with_default("Insert"),
            NCopyStage::Second(_) => Prompt::new("Specify second point"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (NCopyStage::Pick, Input::Pick { at, .. }) => {
                let e =
                    Self::nested(ctx.doc, at, ctx.pick_tolerance).ok_or("Pick an object inside a block reference.")?;
                ctx.messages.push(format!("1 found ({} picked)", self.picked.len() + 1));
                self.picked.push(e);
            }
            (NCopyStage::Pick, Input::Enter) if !self.picked.is_empty() => self.stage = NCopyStage::Base,
            (NCopyStage::Base, Input::Keyword(_) | Input::Enter) => {
                let n = self.add(ctx, Point::ORIGIN);
                ctx.messages.push(format!("{n} object(s) copied."));
                return Ok(Step::Done);
            }
            (NCopyStage::Base, Input::Point(p)) => self.stage = NCopyStage::Second(p),
            (NCopyStage::Second(base), Input::Point(p)) => {
                self.add(ctx, p - base);
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            NCopyStage::Second(base) => self.picked.iter().map(|e| e.geometry.translated(cursor - base)).collect(),
            _ => self.picked.iter().map(|e| e.geometry.clone()).collect(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            NCopyStage::Second(p) => Some(p),
            _ => None,
        }
    }

    fn wants_pick(&self) -> bool {
        self.stage == NCopyStage::Pick
    }
}

/// ATTEDIT: a block reference's attribute values, in the app's editor.
#[derive(Default)]
pub struct AttEdit;

impl Command for AttEdit {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select block reference")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                match ctx.doc.entity(id).map(|e| &e.geometry) {
                    Some(Geometry::Insert { attributes, .. }) if !attributes.is_empty() => {
                        ctx.ui = Some(UiRequest::EditAttributes(id));
                        Ok(Step::Done)
                    }
                    _ => Err("That block reference has no attributes.".into()),
                }
            }
            _ => Ok(Step::Done),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

#[derive(Clone, PartialEq)]
enum GlobalStage {
    OneAtATime,
    BlockSpec,
    TagSpec(String),
    ValueSpec(String, String),
    Select(String, String, String),
    From(Vec<(EntityId, String)>),
    To(Vec<(EntityId, String)>, String),
}

/// -ATTEDIT: attribute values changed across references: by block, tag
/// and value (wildcards * and ?), replacing one string with another.
pub struct AttEditGlobal {
    stage: GlobalStage,
}

impl Default for AttEditGlobal {
    fn default() -> Self {
        Self { stage: GlobalStage::OneAtATime }
    }
}

/// `*` and `?` wildcards, ignoring case.
fn matches_spec(spec: &str, s: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], s) || (!s.is_empty() && go(p, &s[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => go(&p[1..], &s[1..]),
            _ => false,
        }
    }
    go(&spec.chars().collect::<Vec<_>>(), &s.chars().collect::<Vec<_>>())
}

impl Command for AttEditGlobal {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            GlobalStage::OneAtATime => {
                Prompt::new("Edit attributes one at a time?").with_keywords(&["Yes", "No"]).with_default("Y")
            }
            GlobalStage::BlockSpec => Prompt::new("Enter block name specification").with_default("*"),
            GlobalStage::TagSpec(_) => Prompt::new("Enter attribute tag specification").with_default("*"),
            GlobalStage::ValueSpec(..) => Prompt::new("Enter attribute value specification").with_default("*"),
            GlobalStage::Select(..) => Prompt::new("Select Attributes"),
            GlobalStage::From(found) => {
                Prompt::new(format!("{} attribute(s) selected. Enter string to change", found.len()))
            }
            GlobalStage::To(..) => Prompt::new("Enter new string"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let text = |input: &Input| match input {
            Input::Text(t) => Some(t.clone()),
            Input::Enter => Some("*".to_string()),
            _ => None,
        };
        match (self.stage.clone(), input) {
            (GlobalStage::OneAtATime, Input::Keyword(_) | Input::Enter) => self.stage = GlobalStage::BlockSpec,
            (GlobalStage::BlockSpec, input) => self.stage = GlobalStage::TagSpec(text(&input).ok_or("Enter a name.")?),
            (GlobalStage::TagSpec(b), input) => {
                self.stage = GlobalStage::ValueSpec(b, text(&input).ok_or("Enter a tag.")?)
            }
            (GlobalStage::ValueSpec(b, t), input) => {
                self.stage = GlobalStage::Select(b, t, text(&input).ok_or("Enter a value.")?);
            }
            (GlobalStage::Select(b, t, v), Input::Selection(s)) => {
                let mut found = Vec::new();
                for id in s.ids {
                    let Some(Geometry::Insert { block, attributes, .. }) = ctx.doc.entity(id).map(|e| &e.geometry)
                    else {
                        continue;
                    };
                    if !matches_spec(&b, block) {
                        continue;
                    }
                    for a in attributes.iter().filter(|a| matches_spec(&t, &a.tag) && matches_spec(&v, &a.value)) {
                        found.push((id, a.tag.clone()));
                    }
                }
                if found.is_empty() {
                    ctx.messages.push("0 attributes selected.".into());
                    return Ok(Step::Done);
                }
                self.stage = GlobalStage::From(found);
            }
            (GlobalStage::From(found), Input::Text(from)) => self.stage = GlobalStage::To(found, from),
            (GlobalStage::To(found, from), input @ (Input::Text(_) | Input::Enter)) => {
                let to = if let Input::Text(t) = input { t } else { String::new() };
                for (id, tag) in &found {
                    ctx.doc.update(*id, |e| {
                        if let Geometry::Insert { attributes, .. } = &mut e.geometry {
                            for a in attributes.iter_mut().filter(|a| &a.tag == tag) {
                                a.value = a.value.replace(&from, &to);
                            }
                        }
                    });
                }
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, GlobalStage::Select(..))
    }

    fn wants_text(&self) -> bool {
        matches!(
            self.stage,
            GlobalStage::BlockSpec
                | GlobalStage::TagSpec(_)
                | GlobalStage::ValueSpec(..)
                | GlobalStage::From(_)
                | GlobalStage::To(..)
        )
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// PASTEBLOCK: the clipboard's objects made an anonymous-looking block
/// (A$C followed by hex digits, as AutoCAD names them) in `doc`, and a
/// clipboard of one reference to it to place.
pub fn as_block(doc: &mut Document, clip: &Document) -> Document {
    crate::clipboard::paste_definitions(doc, clip);
    let name = (0u32..)
        .map(|i| format!("A$C{:08X}", (clip.len() as u32).wrapping_mul(2_654_435_761).wrapping_add(i)))
        .find(|n| doc.block(n).is_none())
        .unwrap_or_default();
    let entities = clip
        .entities()
        .map(|e| BlockEntity {
            layer: e.layer.clone(),
            color: e.color,
            linetype: e.linetype.clone(),
            lineweight: e.lineweight,
            geometry: e.geometry.clone(),
        })
        .collect();
    let base = clip.insbase();
    doc.define_block(Block { name: name.clone(), base, entities });
    let mut one = Document::new();
    one.add(Geometry::Insert { block: name, placement: Placement::at(base), attributes: Vec::new(), clip: None });
    one.set_insbase(base);
    one
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Attach, Attribute, Block, BlockEntity, Color, Document, Geometry, Lineweight, Placement};

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

    fn block_doc() -> Document {
        let mut d = Document::new();
        let mut red =
            BlockEntity::new("0", Color::Index(1), Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) });
        red.lineweight = Lineweight::Hundredths(50);
        let circle =
            BlockEntity::new("0", Color::ByBlock, Geometry::Circle { center: Point::new(5.0, 5.0), radius: 2.0 });
        d.define_block(Block { name: "Tag".into(), base: Point::ORIGIN, entities: vec![red, circle] });
        let attribute = Attribute {
            tag: "ROOM".into(),
            value: "Kitchen 1".into(),
            layer: "0".into(),
            color: Color::ByLayer,
            position: Point::new(100.0, 0.0),
            height: 2.5,
            rotation: 0.0,
            attach: Attach::BottomLeft,
            invisible: false,
        };
        d.add(Geometry::Insert {
            block: "Tag".into(),
            placement: Placement::at(Point::new(100.0, 0.0)),
            attributes: vec![attribute],
            clip: None,
        });
        d
    }

    #[test]
    fn pasteblock_places_the_clipboard_as_a_block() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let mut clip = Document::new();
        clip.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) });
        clip.add(Geometry::Circle { center: Point::new(5.0, 5.0), radius: 1.0 });
        e.paste_block(clip, &mut d);
        click(&mut e, &mut d, 50.0, 50.0);
        let inserts: Vec<_> = d
            .entities()
            .filter_map(|x| match &x.geometry {
                Geometry::Insert { block, placement, .. } => Some((block.clone(), placement.position)),
                _ => None,
            })
            .collect();
        assert_eq!(inserts.len(), 1, "one reference, nothing else");
        assert_eq!(d.len(), 1);
        let (name, at) = &inserts[0];
        assert!(name.starts_with("A$C") && d.block(name).is_some_and(|b| b.entities.len() == 2));
        assert_eq!(*at, Point::new(50.0, 50.0));
    }

    #[test]
    fn setbylayer_ncopy_and_attedit() {
        let (mut e, mut d) = (Engine::new(), block_doc());
        // NCOPY the circle out of the reference, in place.
        run(&mut e, &mut d, &["ncopy"]);
        click(&mut e, &mut d, 107.0, 5.0);
        run(&mut e, &mut d, &["", ""]);
        assert!(
            d.entities()
                .any(|x| matches!(x.geometry, Geometry::Circle { center, .. } if center == Point::new(105.0, 5.0)))
        );
        // -ATTEDIT: Kitchen to Bathroom.
        run(&mut e, &mut d, &["-attedit", "n", "", "ROOM", "", "all", "", "Kitchen", "Bathroom"]);
        let value = d.entities().find_map(|x| match &x.geometry {
            Geometry::Insert { attributes, .. } => attributes.first().map(|a| a.value.clone()),
            _ => None,
        });
        assert_eq!(value.as_deref(), Some("Bathroom 1"));
        // SETBYLAYER, blocks too.
        e.select_all(&d);
        run(&mut e, &mut d, &["setbylayer", "", ""]);
        let b = d.block("Tag").unwrap();
        assert!(b.entities.iter().all(|be| be.color == Color::ByLayer && be.lineweight == Lineweight::ByLayer));
    }
}

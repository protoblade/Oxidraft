//! REFEDIT: editing a block reference or xref in place. The block's
//! contents are copied into the drawing where the reference shows them,
//! as the working set, and the reference is hidden; the rest of the
//! drawing fades and can't be selected. REFSET moves objects in and out of
//! the working set, and REFCLOSE saves it back to the block (or the xref's
//! file) or discards it.

use std::collections::BTreeSet;

use oxidraft_geom::Point;

use crate::document::{Block, BlockEntity, Document};
use crate::entity::{EntityId, Geometry, Placement};
use crate::linetype::Linetype;
use crate::xref::renamed_geometry;

/// A reference being edited in place.
#[derive(Debug, Clone, PartialEq)]
pub struct RefEdit {
    /// The reference picked, hidden while its contents are edited.
    pub reference: EntityId,
    pub block: String,
    pub placement: Placement,
    /// The drawing's own objects when editing began. They're outside the
    /// working set (unless REFSET adds them), and Discard leaves them be.
    pub host: BTreeSet<EntityId>,
    /// The objects outside the working set now: the drawing's own, less
    /// those REFSET added, plus those it removed.
    pub outside: BTreeSet<EntityId>,
    /// The block editor (BEDIT): the block is edited on its own, the rest
    /// of the drawing hidden, and `reference` is a temporary one at the
    /// origin, gone when the editor closes.
    pub editor: bool,
}

impl RefEdit {
    /// True for objects being edited. Objects drawn meanwhile join the
    /// working set, as in AutoCAD.
    pub fn in_working_set(&self, id: EntityId) -> bool {
        id != self.reference && !self.outside.contains(&id)
    }
}

/// True for an object that can go into `block`: anything but a reference
/// to the block itself, which would contain itself.
fn fits(block: &str, g: &Geometry) -> bool {
    !matches!(g, Geometry::Insert { block: b, .. } if b.eq_ignore_ascii_case(block))
}

/// The placement that takes the world back into the block's coordinates
/// (mapping from the reference's insertion point).
fn inverse(p: &Placement, base: Point) -> Placement {
    // Undoing a mirror turns the rotation the other way round.
    Placement {
        position: base,
        scale: 1.0 / p.scale,
        rotation: if p.mirror { p.rotation } else { -p.rotation },
        mirror: p.mirror,
    }
}

/// What `name` from xref `xref` is called in the xref's own drawing:
/// "SITE|Walls" is "Walls" there.
fn independent_name(xref: &str, name: &str) -> String {
    match name.split_once('|') {
        Some((x, rest)) if x.eq_ignore_ascii_case(xref) => rest.to_string(),
        _ => name.to_string(),
    }
}

impl Document {
    /// Starts editing the block reference or xref `reference` in place.
    pub fn begin_refedit(&mut self, reference: EntityId) -> Result<usize, String> {
        if self.refedit().is_some() {
            return Err("** A reference is already being edited; end it with REFCLOSE **".into());
        }
        let Some(Geometry::Insert { block, placement, .. }) = self.entity(reference).map(|e| e.geometry.clone()) else {
            return Err("Select a block reference or xref.".into());
        };
        if self.xref(&block).is_some_and(|x| x.status != crate::XrefStatus::Loaded) {
            return Err(format!("The xref \"{block}\" isn't loaded."));
        }
        let Some(def) = self.block(&block).cloned() else {
            return Err(format!("Block \"{block}\" isn't defined."));
        };
        let host: BTreeSet<EntityId> = self.entities().map(|e| e.id).filter(|id| *id != reference).collect();
        for be in &def.entities {
            self.add_styled(placement.place(def.base, &be.geometry), &be.layer, be.color, &be.linetype, be.lineweight);
        }
        let outside = host.clone();
        self.set_refedit(Some(RefEdit { reference, block: def.name, placement, host, outside, editor: false }));
        Ok(def.entities.len())
    }

    /// BEDIT: opens block `name` in the block editor, its base point at the
    /// origin.
    pub fn begin_block_edit(&mut self, name: &str) -> Result<usize, String> {
        if self.refedit().is_some() {
            return Err("** A reference is already being edited; end it with REFCLOSE **".into());
        }
        let Some(def) = self.block(name).cloned() else { return Err(format!("Block \"{name}\" isn't defined.")) };
        if self.xref(&def.name).is_some() || crate::is_dependent(&def.name) {
            return Err(format!("\"{}\" is an external reference; edit it with REFEDIT.", def.name));
        }
        let reference = self.add(Geometry::Insert {
            block: def.name,
            placement: Placement::at(Point::ORIGIN),
            attributes: Vec::new(),
            clip: None,
        });
        let n = self.begin_refedit(reference)?;
        if let Some(r) = self.refedit().cloned() {
            self.set_refedit(Some(RefEdit { editor: true, ..r }));
        }
        Ok(n)
    }

    /// BSAVE: redefines the block from the working set, and goes on editing.
    pub fn save_block_edit(&mut self) {
        let Some(r) = self.refedit().cloned() else { return };
        if let Some(def) = self.block(&r.block) {
            let block = Block { name: def.name.clone(), base: def.base, entities: self.refedit_contents() };
            self.define_block(block);
        }
    }

    /// REFSET: moves objects into the working set (`add`) or out of it.
    /// References to the block being edited can't go in.
    pub fn refset(&mut self, ids: &[EntityId], add: bool) -> usize {
        let Some(mut r) = self.refedit().cloned() else { return 0 };
        let mut moved = 0;
        let movable =
            |id: EntityId| id != r.reference && self.entity(id).is_some_and(|e| !add || fits(&r.block, &e.geometry));
        for &id in ids.iter().filter(|id| movable(**id)) {
            let changed = if add { r.outside.remove(&id) } else { r.outside.insert(id) };
            moved += usize::from(changed);
        }
        if moved > 0 {
            self.set_refedit(Some(r));
        }
        moved
    }

    /// The working set in the block's own coordinates, as it would be saved.
    pub fn refedit_contents(&self) -> Vec<BlockEntity> {
        let Some(r) = self.refedit() else { return Vec::new() };
        let Some(base) = self.block(&r.block).map(|b| b.base) else { return Vec::new() };
        let back = inverse(&r.placement, base);
        self.entities_in_draw_order()
            .into_iter()
            .filter(|e| r.in_working_set(e.id) && fits(&r.block, &e.geometry))
            .map(|e| BlockEntity {
                layer: e.layer.clone(),
                color: e.color,
                linetype: e.linetype.clone(),
                lineweight: e.lineweight,
                geometry: back.place(r.placement.position, &e.geometry),
            })
            .collect()
    }

    /// REFCLOSE: ends editing in place. Saving redefines the block from the
    /// working set (an xref's file is written by whoever calls this) and
    /// takes the working set out of the drawing; discarding removes only
    /// what was copied or drawn for it. Returns the session that ended.
    pub fn close_refedit(&mut self, save: bool) -> Option<RefEdit> {
        let r = self.refedit()?.clone();
        if save && let Some(def) = self.block(&r.block) {
            let block = Block { name: def.name.clone(), base: def.base, entities: self.refedit_contents() };
            self.define_block(block);
        }
        // Saving leaves references to the block itself in the drawing.
        let working: Vec<EntityId> = self
            .entities()
            .filter(|e| r.in_working_set(e.id))
            .filter(|e| if save { fits(&r.block, &e.geometry) } else { !r.host.contains(&e.id) })
            .map(|e| e.id)
            .collect();
        for id in working {
            self.remove(id);
        }
        self.set_refedit(None);
        if r.editor {
            self.remove(r.reference);
        }
        Some(r)
    }

    /// Puts the working set of an xref being edited into `site`, the xref's
    /// own drawing as read from its file: its objects become the working
    /// set, under their own names there ("SITE|Walls" is "Walls"). Layers,
    /// linetypes and blocks they use that `site` lacks come from here.
    pub fn store_refedit(&self, site: &mut Document) {
        let Some(r) = self.refedit() else { return };
        let own = |name: &str| independent_name(&r.block, name);
        let old: Vec<EntityId> = site.entities().map(|e| e.id).collect();
        for id in old {
            site.remove(id);
        }
        for be in self.refedit_contents() {
            self.copy_names(site, &be, &own);
            let geometry = renamed_geometry(&be.geometry, &own);
            site.add_styled(geometry, &own(&be.layer), be.color, &own(&be.linetype), be.lineweight);
        }
    }

    /// Defines in `site` the layer, linetype and blocks `be` uses.
    fn copy_names(&self, site: &mut Document, be: &BlockEntity, own: &dyn Fn(&str) -> String) {
        if site.layer(&own(&be.layer)).is_none()
            && let Some(l) = self.layer(&be.layer)
        {
            self.copy_linetype(site, &l.linetype, own);
            site.add_layer(crate::Layer { name: own(&l.name), linetype: own(&l.linetype), ..l.clone() });
        }
        self.copy_linetype(site, &be.linetype, own);
        if let Geometry::Insert { block, .. } = &be.geometry
            && site.block(&own(block)).is_none()
            && let Some(def) = self.block(block).filter(|_| self.xref(block).is_none())
        {
            // Defined first, so a block that contains itself stops here.
            site.define_block(Block { name: own(&def.name), base: def.base, entities: Vec::new() });
            let entities = def
                .entities
                .iter()
                .map(|e| {
                    self.copy_names(site, e, own);
                    BlockEntity {
                        layer: own(&e.layer),
                        linetype: own(&e.linetype),
                        geometry: renamed_geometry(&e.geometry, own),
                        ..e.clone()
                    }
                })
                .collect();
            site.define_block(Block { name: own(&def.name), base: def.base, entities });
        }
    }

    fn copy_linetype(&self, site: &mut Document, name: &str, own: &dyn Fn(&str) -> String) {
        if site.linetype(&own(name)).is_none()
            && let Some(lt) = self.linetype(name).filter(|l| !l.pattern.is_empty())
        {
            site.define_linetype(Linetype { name: own(&lt.name), ..lt.clone() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attach, Color, Layer, Xref, XrefStatus};

    fn approx(a: &[BlockEntity], b: &[BlockEntity]) -> bool {
        let points = |g: &Geometry| g.bounds();
        a.len() == b.len()
            && a.iter().zip(b).all(|(x, y)| {
                let (p, q) = (points(&x.geometry), points(&y.geometry));
                p.min.approx_eq(q.min) && p.max.approx_eq(q.max) && x.layer == y.layer
            })
    }

    fn block(d: &mut Document) -> Vec<BlockEntity> {
        let entities = vec![
            BlockEntity::new("0", Color::ByBlock, Geometry::Line { start: Point::ORIGIN, end: Point::new(4.0, 0.0) }),
            BlockEntity::new("0", Color::ByLayer, Geometry::Circle { center: Point::new(2.0, 1.0), radius: 1.0 }),
            BlockEntity::new(
                "0",
                Color::ByLayer,
                Geometry::Text {
                    position: Point::new(0.0, 3.0),
                    height: 0.5,
                    rotation: 10.0,
                    width: 0.0,
                    attach: Attach::BottomLeft,
                    content: "A".into(),
                },
            ),
        ];
        d.define_block(Block { name: "B".into(), base: Point::new(1.0, 1.0), entities: entities.clone() });
        entities
    }

    #[test]
    fn edits_a_block_in_place_and_saves_or_discards() {
        let mut d = Document::new();
        let original = block(&mut d);
        let other = d.add(Geometry::Line { start: Point::new(50.0, 0.0), end: Point::new(60.0, 0.0) });
        let placement = Placement { position: Point::new(10.0, 5.0), scale: 2.0, rotation: 30.0, mirror: true };
        let reference = d.add(Geometry::Insert { block: "B".into(), placement, attributes: Vec::new(), clip: None });
        let second = d.add(Geometry::Insert {
            block: "B".into(),
            placement: Placement::at(Point::new(-20.0, 0.0)),
            attributes: Vec::new(),
            clip: None,
        });

        d.begin("REFEDIT");
        assert_eq!(d.begin_refedit(reference), Ok(3));
        d.commit();
        let r = d.refedit().unwrap().clone();
        let working: Vec<EntityId> = d.entities().map(|e| e.id).filter(|id| r.in_working_set(*id)).collect();
        assert_eq!(working.len(), 3);
        // The copies sit where the reference shows its contents.
        let shown: Vec<Geometry> = d.expanded(d.entity(reference).unwrap());
        for (id, g) in working.iter().zip(&shown) {
            let b = d.entity(*id).unwrap().geometry.bounds();
            assert!(b.min.approx_eq(g.bounds().min) && b.max.approx_eq(g.bounds().max));
        }
        assert!(!d.is_selectable(d.entity(other).unwrap()) && !d.is_selectable(d.entity(reference).unwrap()));
        assert!(d.is_selectable(d.entity(working[0]).unwrap()));
        // Unchanged, the contents map back to what the block holds.
        assert!(approx(&d.refedit_contents(), &original));

        // Erase the line, draw a new one, bring in a drawing object.
        d.remove(working[0]);
        let drawn = d.add(Geometry::Line { start: Point::new(10.0, 5.0), end: Point::new(12.0, 5.0) });
        assert!(d.is_selectable(d.entity(drawn).unwrap()), "new objects join the working set");
        assert_eq!(d.refset(&[other], true), 1);
        assert_eq!(d.refset(&[second], true), 0, "a reference to the block can't go into it");
        let drawn_reference = d.add(Geometry::Insert {
            block: "B".into(),
            placement: Placement::at(Point::new(30.0, 0.0)),
            attributes: Vec::new(),
            clip: None,
        });
        d.begin("REFCLOSE");
        d.close_refedit(true);
        d.commit();
        let saved = &d.block("B").unwrap().entities;
        assert_eq!(saved.len(), 4);
        // The new line, from the insertion point along the (mirrored) x axis.
        let line = saved.iter().find_map(|e| match e.geometry {
            Geometry::Line { start, end } if start.approx_eq(Point::new(1.0, 1.0)) => Some(end),
            _ => None,
        });
        let end = Point::new(1.0 - 30f64.to_radians().cos(), 0.5);
        assert!(line.is_some_and(|p| p.approx_eq(end)), "{line:?}");
        assert!(d.refedit().is_none() && d.entity(other).is_none() && d.entity(drawn).is_none());
        assert!(d.entity(reference).is_some() && d.entity(second).is_some());
        assert!(d.entity(drawn_reference).is_some(), "left in the drawing, not the block");

        // Undo brings back the block and the session's end state.
        d.undo();
        assert_eq!(d.block("B").unwrap().entities.len(), 3);
        assert!(d.refedit().is_some());
        // Discard keeps the block and the drawing's own objects.
        d.begin("REFCLOSE");
        d.close_refedit(false);
        d.commit();
        assert!(approx(&d.block("B").unwrap().entities, &original));
        assert!(d.entity(other).is_some() && d.refedit().is_none());
        assert_eq!(d.entities().count(), 3);
    }

    #[test]
    fn block_editor_edits_a_block_at_the_origin_and_hides_the_rest() {
        let mut d = Document::new();
        block(&mut d);
        let other = d.add(Geometry::Line { start: Point::new(50.0, 0.0), end: Point::new(60.0, 0.0) });
        let reference = d.add(Geometry::Insert {
            block: "B".into(),
            placement: Placement { position: Point::new(10.0, 5.0), scale: 2.0, rotation: 30.0, mirror: false },
            attributes: Vec::new(),
            clip: None,
        });
        d.begin("BEDIT");
        assert_eq!(d.begin_block_edit("B"), Ok(3));
        d.commit();
        let r = d.refedit().unwrap().clone();
        assert!(r.editor && r.reference != reference);
        // The base point (1, 1) is at the origin.
        let line =
            d.entities().find(|e| r.in_working_set(e.id) && matches!(e.geometry, Geometry::Line { .. })).unwrap();
        assert!(matches!(line.geometry, Geometry::Line { start, .. } if start.approx_eq(Point::new(-1.0, -1.0))));
        assert!(!d.is_selectable(d.entity(other).unwrap()));
        // A line drawn from the origin goes into the block from its base point.
        d.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(3.0, 0.0) });
        d.begin("BSAVE");
        d.save_block_edit();
        d.commit();
        assert_eq!(d.block("B").unwrap().entities.len(), 4);
        assert!(d.refedit().is_some(), "still editing");
        d.begin("BCLOSE");
        d.close_refedit(false);
        d.commit();
        assert_eq!(d.block("B").unwrap().entities.len(), 4, "saved before closing");
        assert!(d.refedit().is_none() && d.entity(r.reference).is_none(), "the temporary reference is gone");
        assert_eq!(d.entities().count(), 2);
        let saved = d.block("B").unwrap().entities.iter().any(|e| {
            matches!(e.geometry, Geometry::Line { start, end } if start.approx_eq(Point::new(1.0, 1.0)) && end.approx_eq(Point::new(4.0, 1.0)))
        });
        assert!(saved);
    }

    #[test]
    fn stores_an_xref_edit_under_its_own_names() {
        let mut site = Document::new();
        site.add_layer(Layer::new("Walls", 1));
        site.add_on_layer(Geometry::Line { start: Point::ORIGIN, end: Point::new(5.0, 0.0) }, "Walls", Color::ByLayer);
        let mut d = Document::new();
        d.set_xref(Xref { name: "SITE".into(), path: "site.dxf".into(), overlay: false, status: XrefStatus::NotFound });
        d.load_xref("SITE", &site);
        let reference = d.add(Geometry::Insert {
            block: "SITE".into(),
            placement: Placement::at(Point::new(100.0, 0.0)),
            attributes: Vec::new(),
            clip: None,
        });
        d.begin_refedit(reference).unwrap();
        let working = d.entities().find(|e| e.layer == "SITE|Walls").unwrap().id;
        d.update(working, |e| {
            e.geometry = Geometry::Line { start: Point::new(100.0, 0.0), end: Point::new(108.0, 0.0) }
        });
        d.add_layer(Layer::new("New", 3));
        d.add_on_layer(Geometry::Circle { center: Point::new(101.0, 1.0), radius: 0.5 }, "New", Color::ByLayer);
        d.store_refedit(&mut site);
        let lines: Vec<(String, f64)> = site.entities().map(|e| (e.layer.clone(), e.geometry.length())).collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.contains(&("Walls".into(), 8.0)));
        assert!(site.layer("New").is_some_and(|l| l.color == 3));
        let circle = site.entities().find(|e| e.layer == "New").unwrap();
        assert!(matches!(circle.geometry, Geometry::Circle { center, .. } if center.approx_eq(Point::new(1.0, 1.0))));
    }
}

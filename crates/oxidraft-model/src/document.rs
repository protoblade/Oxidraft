use std::collections::BTreeMap;

use oxidraft_geom::{Aabb, Point};

use crate::entity::{Color, Entity, EntityId, Geometry};

/// A named layer. Layer "0" always exists, as in DXF.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    /// AutoCAD Color Index of entities drawn `ByLayer`.
    pub color: u8,
    /// Off layers aren't drawn or selectable.
    pub on: bool,
    /// Frozen layers behave like off layers (AutoCAD also skips them in regens).
    pub frozen: bool,
    /// Locked layers are drawn faded and snap, but can't be selected or edited.
    pub locked: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, color: u8) -> Self {
        Self { name: name.into(), color, on: true, frozen: false, locked: false }
    }

    pub fn is_shown(&self) -> bool {
        self.on && !self.frozen
    }
}

/// Characters AutoCAD doesn't allow in layer and block names.
const BAD_NAME_CHARS: &[char] = &['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`'];

/// Checks a layer or block name the way AutoCAD does.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err("The name can't be empty.".into())
    } else if name.len() > 255 || name.contains(BAD_NAME_CHARS) {
        Err(format!("\"{name}\" isn't a valid name (it can't contain {}).", BAD_NAME_CHARS.iter().collect::<String>()))
    } else {
        Ok(())
    }
}

/// An object inside a block definition.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntity {
    pub layer: String,
    pub color: Color,
    pub geometry: Geometry,
}

/// A named block definition; block references place copies of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub name: String,
    /// The point that lands on a reference's insertion point.
    pub base: Point,
    pub entities: Vec<BlockEntity>,
}

/// The layer table and the current layer, recorded whole for undo.
#[derive(Debug, Clone, PartialEq)]
struct LayerTable {
    layers: Vec<Layer>,
    current: String,
}

/// One edit, recorded with what it replaced so it can be undone.
#[derive(Debug, Clone)]
enum Change {
    /// `before` is `None` for an add, `after` is `None` for a removal.
    Entity {
        id: EntityId,
        before: Option<Entity>,
        after: Option<Entity>,
    },
    Layers {
        before: LayerTable,
        after: LayerTable,
    },
    Block {
        name: String,
        before: Option<Block>,
        after: Option<Block>,
    },
}

#[derive(Debug, Clone)]
struct Transaction {
    name: String,
    changes: Vec<Change>,
}

/// Nesting limit when expanding block references (guards against cycles).
const MAX_BLOCK_DEPTH: usize = 16;

/// A drawing: entities, layers, block definitions and undo/redo history.
///
/// Edits made between [`Document::begin`] and [`Document::commit`] undo as a
/// single step. Edits made outside a transaction each get their own step.
#[derive(Debug, Clone)]
pub struct Document {
    entities: BTreeMap<EntityId, Entity>,
    table: LayerTable,
    blocks: BTreeMap<String, Block>,
    next_id: u64,
    open: Option<Transaction>,
    undo_stack: Vec<Transaction>,
    redo_stack: Vec<Transaction>,
    revision: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            table: LayerTable { layers: vec![Layer::new("0", 7)], current: "0".into() },
            blocks: BTreeMap::new(),
            next_id: 1,
            open: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            revision: 0,
        }
    }

    /// Increases on every change, including undo and redo.
    /// Compare against a saved value to show the unsaved-changes marker.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    // ---- queries -------------------------------------------------------

    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn layers(&self) -> &[Layer] {
        &self.table.layers
    }

    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.table.layers.iter().find(|l| l.name.eq_ignore_ascii_case(name))
    }

    pub fn current_layer(&self) -> &str {
        &self.table.current
    }

    /// True if objects on `layer` are drawn (layer on and thawed).
    pub fn is_shown(&self, layer: &str) -> bool {
        self.layer(layer).is_none_or(Layer::is_shown)
    }

    /// True if objects on `layer` can be selected and edited.
    pub fn is_editable(&self, layer: &str) -> bool {
        self.layer(layer).is_none_or(|l| l.is_shown() && !l.locked)
    }

    /// Number of objects on `layer`.
    pub fn layer_usage(&self, layer: &str) -> usize {
        self.entities.values().filter(|e| e.layer.eq_ignore_ascii_case(layer)).count()
    }

    fn color_on(&self, layer: &str, color: Color) -> u8 {
        match color {
            Color::Index(i) => i,
            Color::ByLayer => self.layer(layer).map_or(7, |l| l.color),
        }
    }

    /// Resolved ACI color of an entity (layer color when `ByLayer`).
    pub fn display_color(&self, e: &Entity) -> u8 {
        self.color_on(&e.layer, e.color)
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.blocks.values()
    }

    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks.get(name).or_else(|| self.blocks.values().find(|b| b.name.eq_ignore_ascii_case(name)))
    }

    /// The entity's curves in world coordinates, with each curve's display
    /// color and layer. Block references are expanded recursively; objects on
    /// layer "0" inside a block take the reference's layer, as in AutoCAD.
    pub fn expanded_styled(&self, e: &Entity) -> Vec<(Geometry, u8, String)> {
        let mut out = Vec::new();
        self.expand_into(&e.geometry, &e.layer, e.color, 0, &mut out);
        out
    }

    /// The entity's curves in world coordinates (block references expanded).
    pub fn expanded(&self, e: &Entity) -> Vec<Geometry> {
        if !matches!(e.geometry, Geometry::Insert { .. } | Geometry::Dimension { .. } | Geometry::Leader { .. }) {
            return vec![e.geometry.clone()];
        }
        self.expanded_styled(e).into_iter().map(|(g, _, _)| g).collect()
    }

    fn expand_into(
        &self,
        g: &Geometry,
        layer: &str,
        color: Color,
        depth: usize,
        out: &mut Vec<(Geometry, u8, String)>,
    ) {
        if let Some(parts) = g.primitives() {
            for part in &parts {
                self.expand_into(part, layer, color, depth + 1, out);
            }
            return;
        }
        let Geometry::Insert { block, placement } = g else {
            out.push((g.clone(), self.color_on(layer, color), layer.to_string()));
            return;
        };
        let Some(def) = self.block(block).filter(|_| depth < MAX_BLOCK_DEPTH) else { return };
        for be in &def.entities {
            let inner_layer = if be.layer == "0" { layer } else { &be.layer };
            if !self.is_shown(inner_layer) {
                continue;
            }
            self.expand_into(&placement.place(def.base, &be.geometry), inner_layer, be.color, depth + 1, out);
        }
    }

    /// Bounding box of an entity, block contents included.
    pub fn entity_bounds(&self, e: &Entity) -> Aabb {
        self.expanded(e).iter().map(Geometry::bounds).reduce(Aabb::union).unwrap_or_else(|| e.geometry.bounds())
    }

    /// Distance from `p` to the entity's curves (a block reference also
    /// counts its insertion point, so an empty block can still be picked).
    pub fn entity_distance(&self, e: &Entity, p: Point) -> f64 {
        let curves = self.expanded(e).iter().map(|g| g.distance_to(p)).fold(f64::INFINITY, f64::min);
        match &e.geometry {
            Geometry::Insert { placement, .. } => curves.min(placement.position.distance(p)),
            _ => curves,
        }
    }

    /// True if any of the entity's curves lies in the box (crossing selection).
    pub fn entity_touches(&self, e: &Entity, b: &Aabb) -> bool {
        self.expanded(e).iter().any(|g| g.touches(b))
    }

    /// Bounding box of all entities, or `None` for an empty drawing.
    pub fn extents(&self) -> Option<Aabb> {
        self.entities.values().map(|e| self.entity_bounds(e)).reduce(Aabb::union)
    }

    // ---- layers ----------------------------------------------------------

    /// Replaces the layer table (one undoable change).
    fn set_table(&mut self, after: LayerTable) {
        if after == self.table {
            return;
        }
        let before = std::mem::replace(&mut self.table, after.clone());
        self.record(Change::Layers { before, after });
    }

    /// Adds a layer, or updates the color of an existing one with the same name.
    pub fn add_layer(&mut self, layer: Layer) {
        let mut table = self.table.clone();
        match table.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&layer.name)) {
            Some(existing) => existing.color = layer.color,
            None => table.layers.push(layer),
        }
        self.set_table(table);
    }

    /// Updates a layer's color and states (not its name; see [`Document::rename_layer`]).
    pub fn update_layer(&mut self, layer: Layer) -> bool {
        let mut table = self.table.clone();
        let Some(existing) = table.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&layer.name)) else {
            return false;
        };
        *existing = Layer { name: existing.name.clone(), ..layer };
        self.set_table(table);
        true
    }

    /// Makes `name` the layer new entities go on. Returns false if it doesn't exist.
    pub fn set_current_layer(&mut self, name: &str) -> bool {
        let Some(l) = self.layer(name) else { return false };
        let table = LayerTable { current: l.name.clone(), ..self.table.clone() };
        self.set_table(table);
        true
    }

    /// Renames a layer and moves its objects with it. Layer "0" can't be renamed.
    pub fn rename_layer(&mut self, old: &str, new: &str) -> Result<(), String> {
        check_name(new)?;
        let Some(layer) = self.layer(old).cloned() else { return Err(format!("Layer \"{old}\" not found.")) };
        if layer.name == "0" {
            return Err("Layer 0 can't be renamed.".into());
        }
        if !new.eq_ignore_ascii_case(old) && self.layer(new).is_some() {
            return Err(format!("A layer named \"{new}\" already exists."));
        }
        let mut table = self.table.clone();
        for l in &mut table.layers {
            if l.name == layer.name {
                l.name = new.to_string();
            }
        }
        if table.current == layer.name {
            table.current = new.to_string();
        }
        self.set_table(table);
        let ids: Vec<EntityId> = self.entities.values().filter(|e| e.layer == layer.name).map(|e| e.id).collect();
        for id in ids {
            self.update(id, |e| e.layer = new.to_string());
        }
        Ok(())
    }

    /// Deletes an empty layer. Layer "0" and the current layer can't be deleted.
    pub fn delete_layer(&mut self, name: &str) -> Result<(), String> {
        let Some(layer) = self.layer(name).cloned() else { return Err(format!("Layer \"{name}\" not found.")) };
        if layer.name == "0" {
            return Err("Layer 0 can't be deleted.".into());
        }
        if layer.name == self.table.current {
            return Err("The current layer can't be deleted.".into());
        }
        let used = self.layer_usage(&layer.name)
            + self.blocks.values().flat_map(|b| &b.entities).filter(|be| be.layer == layer.name).count();
        if used > 0 {
            return Err(format!("Layer \"{}\" has {used} objects on it.", layer.name));
        }
        let mut table = self.table.clone();
        table.layers.retain(|l| l.name != layer.name);
        self.set_table(table);
        Ok(())
    }

    /// A layer name like "Layer1" that isn't taken yet.
    pub fn unused_layer_name(&self) -> String {
        (1..).map(|i| format!("Layer{i}")).find(|n| self.layer(n).is_none()).unwrap_or_default()
    }

    // ---- blocks ----------------------------------------------------------

    /// Adds or replaces a block definition.
    pub fn define_block(&mut self, block: Block) {
        let name = block.name.clone();
        let before = self.blocks.insert(name.clone(), block.clone());
        self.record(Change::Block { name, before, after: Some(block) });
    }

    // ---- edits ---------------------------------------------------------

    /// Adds an entity on the current layer.
    pub fn add(&mut self, geometry: Geometry) -> EntityId {
        let layer = self.table.current.clone();
        self.add_on_layer(geometry, &layer, Color::ByLayer)
    }

    /// Adds an entity on a specific layer (created with color 7 if missing).
    pub fn add_on_layer(&mut self, geometry: Geometry, layer: &str, color: Color) -> EntityId {
        let layer = match self.layer(layer) {
            Some(l) => l.name.clone(),
            None => {
                self.add_layer(Layer::new(layer, 7));
                layer.to_string()
            }
        };
        let id = EntityId(self.next_id);
        self.next_id += 1;
        let entity = Entity { id, layer, color, geometry };
        self.entities.insert(id, entity.clone());
        self.record(Change::Entity { id, before: None, after: Some(entity) });
        id
    }

    /// Removes an entity. Returns false if it doesn't exist.
    pub fn remove(&mut self, id: EntityId) -> bool {
        match self.entities.remove(&id) {
            Some(e) => {
                self.record(Change::Entity { id, before: Some(e), after: None });
                true
            }
            None => false,
        }
    }

    /// Changes an entity in place (geometry, layer or color). Returns false if it doesn't exist.
    pub fn update(&mut self, id: EntityId, f: impl FnOnce(&mut Entity)) -> bool {
        let Some(e) = self.entities.get_mut(&id) else { return false };
        let before = e.clone();
        f(e);
        e.id = id;
        if *e == before {
            return true;
        }
        let after = e.clone();
        self.record(Change::Entity { id, before: Some(before), after: Some(after) });
        true
    }

    /// Replaces an entity's geometry. Returns false if it doesn't exist.
    pub fn modify(&mut self, id: EntityId, geometry: Geometry) -> bool {
        self.update(id, |e| e.geometry = geometry)
    }

    // ---- transactions & undo ------------------------------------------

    /// Starts grouping edits into one undo step. Commits any transaction already open.
    pub fn begin(&mut self, name: impl Into<String>) {
        self.commit();
        self.open = Some(Transaction { name: name.into(), changes: Vec::new() });
    }

    /// Ends the open transaction. Empty transactions leave no undo step.
    pub fn commit(&mut self) {
        if let Some(t) = self.open.take()
            && !t.changes.is_empty()
        {
            self.undo_stack.push(t);
            self.redo_stack.clear();
        }
    }

    /// Reverts every edit made since [`Document::begin`].
    pub fn rollback(&mut self) {
        if let Some(t) = self.open.take() {
            self.revert(&t);
        }
    }

    /// Reverts the last edit in the open transaction but keeps it open, so
    /// later edits still group with it.
    pub fn undo_last_in_transaction(&mut self) -> bool {
        let Some(change) = self.open.as_mut().and_then(|t| t.changes.pop()) else {
            return false;
        };
        self.apply(&change, false);
        true
    }

    /// Undoes the last committed step and returns its name.
    pub fn undo(&mut self) -> Option<String> {
        self.commit();
        let t = self.undo_stack.pop()?;
        self.revert(&t);
        let name = t.name.clone();
        self.redo_stack.push(t);
        Some(name)
    }

    /// Redoes the last undone step and returns its name.
    pub fn redo(&mut self) -> Option<String> {
        self.commit();
        let t = self.redo_stack.pop()?;
        for c in &t.changes {
            self.apply(c, true);
        }
        let name = t.name.clone();
        self.undo_stack.push(t);
        Some(name)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty() || self.open.as_ref().is_some_and(|t| !t.changes.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Drops all undo/redo history, e.g. right after opening a file.
    pub fn forget_history(&mut self) {
        self.open = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    fn record(&mut self, change: Change) {
        self.revision += 1;
        match &mut self.open {
            Some(t) => t.changes.push(change),
            None => {
                let name = match &change {
                    Change::Entity { before: None, .. } => "Add",
                    Change::Entity { after: None, .. } => "Erase",
                    Change::Entity { .. } => "Modify",
                    Change::Layers { .. } => "Layer",
                    Change::Block { .. } => "Block",
                };
                self.undo_stack.push(Transaction { name: name.into(), changes: vec![change] });
                self.redo_stack.clear();
            }
        }
    }

    fn revert(&mut self, t: &Transaction) {
        for c in t.changes.iter().rev() {
            self.apply(c, false);
        }
    }

    /// Applies a change forwards (redo) or backwards (undo).
    fn apply(&mut self, c: &Change, forward: bool) {
        self.revision += 1;
        match c {
            Change::Entity { id, before, after } => {
                match if forward { after } else { before } {
                    Some(e) => self.entities.insert(*id, e.clone()),
                    None => self.entities.remove(id),
                };
            }
            Change::Layers { before, after } => self.table = if forward { after } else { before }.clone(),
            Change::Block { name, before, after } => {
                match if forward { after } else { before } {
                    Some(b) => self.blocks.insert(name.clone(), b.clone()),
                    None => self.blocks.remove(name),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Placement;

    fn line(x: f64) -> Geometry {
        Geometry::Line { start: Point::new(x, 0.0), end: Point::new(x, 1.0) }
    }

    #[test]
    fn transaction_undoes_as_one_step() {
        let mut d = Document::new();
        d.begin("LINE");
        d.add(line(0.0));
        d.add(line(1.0));
        d.commit();
        assert_eq!(d.len(), 2);
        assert_eq!(d.undo().as_deref(), Some("LINE"));
        assert!(d.is_empty());
        assert_eq!(d.redo().as_deref(), Some("LINE"));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn rollback_discards_open_edits() {
        let mut d = Document::new();
        d.add(line(0.0));
        d.begin("LINE");
        d.add(line(1.0));
        d.rollback();
        assert_eq!(d.len(), 1);
        assert!(!d.can_redo());
    }

    #[test]
    fn erase_is_undoable_and_new_edit_clears_redo() {
        let mut d = Document::new();
        let id = d.add(line(0.0));
        assert!(d.remove(id));
        assert!(d.is_empty());
        d.undo();
        assert_eq!(d.len(), 1);
        d.add(line(2.0));
        assert!(!d.can_redo());
    }

    #[test]
    fn modify_is_undoable() {
        let mut d = Document::new();
        let id = d.add(line(0.0));
        assert!(d.modify(id, line(5.0)));
        assert_eq!(d.entity(id).unwrap().geometry, line(5.0));
        d.undo();
        assert_eq!(d.entity(id).unwrap().geometry, line(0.0));
        d.redo();
        assert_eq!(d.entity(id).unwrap().geometry, line(5.0));
    }

    #[test]
    fn bylayer_color_resolves_from_layer() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Walls", 1));
        assert!(d.set_current_layer("walls"));
        let id = d.add(line(0.0));
        let e = d.entity(id).unwrap();
        assert_eq!(e.layer, "Walls");
        assert_eq!(d.display_color(e), 1);
    }

    #[test]
    fn layer_changes_are_undoable() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Walls", 1));
        d.update_layer(Layer { locked: true, ..Layer::new("walls", 3) });
        assert!(d.layer("Walls").unwrap().locked);
        assert_eq!(d.layer("Walls").unwrap().color, 3);
        d.undo();
        assert!(!d.layer("Walls").unwrap().locked);
        d.undo();
        assert!(d.layer("Walls").is_none());
    }

    #[test]
    fn rename_moves_objects_and_undoes_in_one_step() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Walls", 1));
        d.set_current_layer("Walls");
        let id = d.add(line(0.0));
        d.begin("LAYER");
        d.rename_layer("Walls", "A-Wall").unwrap();
        d.commit();
        assert_eq!(d.entity(id).unwrap().layer, "A-Wall");
        assert_eq!(d.current_layer(), "A-Wall");
        d.undo();
        assert_eq!(d.entity(id).unwrap().layer, "Walls");
        assert!(d.rename_layer("0", "Zero").is_err());
        assert!(d.rename_layer("Walls", "a/b").is_err());
    }

    #[test]
    fn delete_layer_rules() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Empty", 1));
        d.add_layer(Layer::new("Used", 2));
        d.add_on_layer(line(0.0), "Used", Color::ByLayer);
        assert!(d.delete_layer("0").is_err());
        assert!(d.delete_layer("Used").is_err());
        assert!(d.delete_layer("Empty").is_ok());
        assert!(d.layer("Empty").is_none());
        assert_eq!(d.unused_layer_name(), "Layer1");
    }

    #[test]
    fn block_references_expand_with_layer_zero_inheriting() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Doors", 2));
        d.add_layer(Layer::new("Red", 1));
        d.define_block(Block {
            name: "DOOR".into(),
            base: Point::ORIGIN,
            entities: vec![
                BlockEntity { layer: "0".into(), color: Color::ByLayer, geometry: line(1.0) },
                BlockEntity { layer: "Red".into(), color: Color::ByLayer, geometry: line(2.0) },
            ],
        });
        let insert = Geometry::Insert { block: "DOOR".into(), placement: Placement::at(Point::new(10.0, 0.0)) };
        let id = d.add_on_layer(insert, "Doors", Color::ByLayer);
        let e = d.entity(id).unwrap();
        let styled = d.expanded_styled(e);
        assert_eq!(styled.len(), 2);
        assert_eq!((styled[0].1, styled[0].2.as_str()), (2, "Doors"), "layer 0 takes the reference's layer");
        assert_eq!((styled[1].1, styled[1].2.as_str()), (1, "Red"));
        assert!((d.entity_distance(e, Point::new(11.0, 0.5)) - 0.0).abs() < 1e-9);
        assert_eq!(d.extents().unwrap().min, Point::new(11.0, 0.0));

        // Undo removes the reference, then the definition.
        assert!(d.undo().is_some() && d.undo().is_some());
        assert!(d.block("DOOR").is_none());
    }

    #[test]
    fn self_referencing_blocks_stop_expanding() {
        let mut d = Document::new();
        let me = Geometry::Insert { block: "LOOP".into(), placement: Placement::at(Point::ORIGIN) };
        d.define_block(Block {
            name: "LOOP".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity { layer: "0".into(), color: Color::ByLayer, geometry: me.clone() }],
        });
        let id = d.add(me);
        assert!(d.expanded(d.entity(id).unwrap()).is_empty());
    }
}

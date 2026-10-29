//! Block libraries: bringing a block, or a whole drawing as a block, from
//! another drawing into this one, with the blocks, layers and linetypes it
//! uses. What this drawing already has keeps its own definition, as in
//! AutoCAD.

use crate::document::{Block, BlockEntity, Document};
use crate::entity::Geometry;

impl Document {
    /// Brings block `name` from `from` into this drawing. Returns its name
    /// here, or `None` if `from` has no such block.
    pub fn import_block(&mut self, from: &Document, name: &str) -> Option<String> {
        let def = from.block(name)?.clone();
        if self.block(&def.name).is_none() {
            self.import_names(from, &def.entities);
            self.define_block(def.clone());
        }
        Some(def.name)
    }

    /// Makes `from`'s whole drawing a block named `name` here (its base
    /// point is `from`'s INSBASE), unless this drawing has one by that name.
    pub fn import_drawing(&mut self, from: &Document, name: &str) -> String {
        if let Some(existing) = self.block(name) {
            return existing.name.clone();
        }
        let entities: Vec<BlockEntity> = from
            .entities()
            .map(|e| BlockEntity {
                layer: e.layer.clone(),
                color: e.color,
                linetype: e.linetype.clone(),
                lineweight: e.lineweight,
                geometry: e.geometry.clone(),
            })
            .collect();
        self.import_names(from, &entities);
        self.define_block(Block { name: name.to_string(), base: from.insbase(), entities });
        name.to_string()
    }

    /// Brings layer `name` from `from` (with its linetype), unless this
    /// drawing has one by that name. True if it came.
    pub fn import_layer(&mut self, from: &Document, name: &str) -> bool {
        let Some(layer) = from.layer(name).cloned() else { return false };
        if self.layer(&layer.name).is_some() {
            return false;
        }
        self.import_linetype(from, &layer.linetype);
        self.add_layer(layer.clone());
        self.update_layer(layer);
        true
    }

    /// Brings linetype `name`'s definition from `from`, unless this
    /// drawing has it. True if it came.
    pub fn import_linetype(&mut self, from: &Document, name: &str) -> bool {
        match from.linetype(name).filter(|lt| !lt.pattern.is_empty() && self.linetype(&lt.name).is_none()) {
            Some(lt) => {
                self.define_linetype(lt);
                true
            }
            None => false,
        }
    }

    /// Brings text style `name` from `from`, unless this drawing has one
    /// by that name. True if it came.
    pub fn import_text_style(&mut self, from: &Document, name: &str) -> bool {
        let Some(style) = from.text_styles().iter().find(|s| s.name.eq_ignore_ascii_case(name)).cloned() else {
            return false;
        };
        if self.text_styles().iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
            return false;
        }
        self.set_text_style(style).is_ok()
    }

    /// The blocks, layers and linetypes `entities` use, from `from`.
    fn import_names(&mut self, from: &Document, entities: &[BlockEntity]) {
        for be in entities {
            let mut layers = vec![be.layer.as_str()];
            if let Geometry::Insert { block, attributes, .. } = &be.geometry {
                layers.extend(attributes.iter().map(|a| a.layer.as_str()));
                if from.xref(block).is_none() {
                    self.import_block(from, block);
                }
            }
            for layer in layers.into_iter().filter_map(|n| from.layer(n)) {
                if self.layer(&layer.name).is_none() {
                    if let Some(lt) =
                        from.linetype(&layer.linetype).filter(|_| self.linetype(&layer.linetype).is_none())
                    {
                        self.define_linetype(lt);
                    }
                    self.add_layer(layer.clone());
                    self.update_layer(layer.clone());
                }
            }
            if let Some(lt) = from.linetype(&be.linetype).filter(|_| self.linetype(&be.linetype).is_none()) {
                self.define_linetype(lt);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use oxidraft_geom::Point;

    use crate::{Block, BlockEntity, Color, Document, Geometry, Layer, Placement};

    #[test]
    fn blocks_and_drawings_come_with_what_they_use() {
        let mut lib = Document::new();
        lib.add_layer(Layer::new("Trees", 3));
        let circle = Geometry::Circle { center: Point::ORIGIN, radius: 1.0 };
        lib.define_block(Block {
            name: "LEAF".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("Trees", Color::ByLayer, circle)],
        });
        let leaf = Geometry::Insert {
            block: "LEAF".into(),
            placement: Placement::at(Point::new(2.0, 0.0)),
            attributes: Vec::new(),
            clip: None,
        };
        lib.define_block(Block {
            name: "TREE".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByLayer, leaf)],
        });
        lib.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(5.0, 0.0) });

        let mut doc = Document::new();
        assert_eq!(doc.import_block(&lib, "tree").as_deref(), Some("TREE"));
        assert!(doc.block("LEAF").is_some(), "the block it nests");
        assert!(doc.layer("Trees").is_some_and(|l| l.color == 3), "the layer inside it");
        assert_eq!(doc.import_block(&lib, "missing"), None);
        assert_eq!(doc.import_drawing(&lib, "site"), "site");
        assert_eq!(doc.block("site").map(|b| b.entities.len()), Some(1));
    }
}

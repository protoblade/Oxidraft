//! External references (XREF): other drawings attached to this one. Each
//! shows through a block of its name. Its layers, linetypes and blocks come
//! along named "NAME|layer", as in AutoCAD, so they can't clash with this
//! drawing's own, and can be turned off, frozen and recolored here. Only
//! the path is saved; the contents are read from the file each time.

use std::collections::BTreeSet;

use crate::{
    Block, BlockEntity, Document, Geometry,
    linetype::{self, CONTINUOUS, Linetype},
};

/// An attached drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Xref {
    /// The block name its references use.
    pub name: String,
    /// Where the file is, as saved: a full path, one relative to this
    /// drawing's folder, or a bare file name.
    pub path: String,
    /// Overlays don't come along when this drawing is attached elsewhere.
    pub overlay: bool,
    pub status: XrefStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrefStatus {
    Loaded,
    /// Kept attached, but not drawn (the palette's Unload).
    Unloaded,
    /// The file couldn't be found or read.
    NotFound,
}

impl XrefStatus {
    pub fn label(self) -> &'static str {
        match self {
            XrefStatus::Loaded => "Loaded",
            XrefStatus::Unloaded => "Unloaded",
            XrefStatus::NotFound => "Not Found",
        }
    }
}

/// True for a layer, linetype or block that comes from an attached drawing.
pub fn is_dependent(name: &str) -> bool {
    name.contains('|')
}

/// Names every drawing shares, which an xref's objects keep.
fn shared(name: &str) -> bool {
    name == "0"
        || name.eq_ignore_ascii_case("Defpoints")
        || name.eq_ignore_ascii_case(CONTINUOUS)
        || linetype::is_by_layer(name)
        || linetype::is_by_block(name)
}

/// What `name` from xref `xref` is called here: "XREF|name", unless every
/// drawing has it or it already comes from a nested xref.
pub fn dependent_name(xref: &str, name: &str) -> String {
    if shared(name) || is_dependent(name) { name.to_string() } else { format!("{xref}|{name}") }
}

/// The geometry with the blocks it references and its attributes' layers renamed.
pub(crate) fn renamed_geometry(g: &Geometry, rename: &dyn Fn(&str) -> String) -> Geometry {
    match g {
        Geometry::Insert { block, placement, attributes, clip } => Geometry::Insert {
            block: rename(block),
            placement: *placement,
            attributes: attributes.iter().map(|a| crate::Attribute { layer: rename(&a.layer), ..a.clone() }).collect(),
            clip: clip.clone(),
        },
        g => g.clone(),
    }
}

fn renamed_entity(e: &BlockEntity, rename: &dyn Fn(&str) -> String) -> BlockEntity {
    BlockEntity {
        layer: rename(&e.layer),
        color: e.color,
        linetype: rename(&e.linetype),
        lineweight: e.lineweight,
        geometry: renamed_geometry(&e.geometry, rename),
    }
}

impl Document {
    /// Attaches (or re-reads) xref `name`: fills its block with the model
    /// space of `content` and brings its layers, linetypes and blocks
    /// along. Layers already here keep their settings, as with AutoCAD's
    /// VISRETAIN, so turning an xref's layer off survives a reload.
    pub fn load_xref(&mut self, name: &str, content: &Document) {
        let Some(mut xref) = self.xref(name).cloned() else { return };
        let dep = |n: &str| dependent_name(&xref.name, n);
        let mut layers = Vec::new();
        let mut linetypes: Vec<(String, String)> = Vec::new();
        for l in content.layers().iter().filter(|l| !shared(&l.name)) {
            let n = dep(&l.name);
            match self.layer(&n) {
                // Kept settings, but the linetype comes from the xref.
                Some(existing) if existing.linetype != dep(&l.linetype) => linetypes.push((n, dep(&l.linetype))),
                Some(_) => {}
                None if !layers.iter().any(|x: &crate::Layer| x.name.eq_ignore_ascii_case(&n)) => {
                    layers.push(crate::Layer { name: n, linetype: dep(&l.linetype), ..l.clone() });
                }
                None => {}
            }
        }
        if !layers.is_empty() || !linetypes.is_empty() {
            self.edit_layers(|table| {
                for (name, lt) in linetypes {
                    if let Some(l) = table.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&name)) {
                        l.linetype = lt;
                    }
                }
                table.extend(layers);
            });
        }
        for lt in content.linetypes().into_iter().filter(|l| !shared(&l.name)) {
            self.define_linetype(Linetype { name: dep(&lt.name), ..lt });
        }
        for b in content.blocks() {
            let entities = b.entities.iter().map(|e| renamed_entity(e, &dep)).collect();
            self.define_block(Block { name: dep(&b.name), base: b.base, entities });
        }
        let entities = content
            .entities()
            .map(|e| BlockEntity {
                layer: dep(&e.layer),
                color: e.color,
                linetype: dep(&e.linetype),
                lineweight: e.lineweight,
                geometry: renamed_geometry(&e.geometry, &dep),
            })
            .collect();
        self.define_block(Block { name: xref.name.clone(), base: content.insbase(), entities });
        xref.status = crate::XrefStatus::Loaded;
        self.set_xref(xref);
    }

    /// Marks an xref as not found (or unloaded): it stays attached, its
    /// references draw nothing, and its layers keep their settings.
    pub fn set_xref_status(&mut self, name: &str, status: crate::XrefStatus) {
        let Some(mut xref) = self.xref(name).cloned() else { return };
        if status != crate::XrefStatus::Loaded
            && let Some(b) = self.block(&xref.name).cloned()
            && !b.entities.is_empty()
        {
            self.define_block(Block { entities: Vec::new(), ..b });
        }
        xref.status = status;
        self.set_xref(xref);
    }

    /// How many references in the drawing show xref `name`.
    pub fn xref_references(&self, name: &str) -> usize {
        self.entities()
            .filter(|e| matches!(&e.geometry, Geometry::Insert { block, .. } if block.eq_ignore_ascii_case(name)))
            .count()
    }

    /// DETACH: removes an xref, its references, and what it brought along.
    pub fn detach_xref(&mut self, name: &str) {
        let ids: Vec<_> = self
            .entities()
            .filter(|e| matches!(&e.geometry, Geometry::Insert { block, .. } if block.eq_ignore_ascii_case(name)))
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.remove(id);
        }
        self.remove_block(name);
        self.remove_xref(name);
        self.purge_dependents();
    }

    /// BIND: makes xref `name` part of this drawing. Bind keeps what it
    /// brought apart, renaming "NAME|layer" to "NAME$0$layer"; Insert merges
    /// it into this drawing's own layers, linetypes and blocks.
    pub fn bind_xref(&mut self, name: &str, insert: bool) {
        let Some(xref) = self.xref(name).cloned() else { return };
        // The blocks the xref draws with, nested ones included.
        let mut reach: BTreeSet<String> = BTreeSet::new();
        let mut todo = vec![xref.name.clone()];
        while let Some(b) = todo.pop() {
            let Some(block) = self.block(&b) else { continue };
            if !reach.insert(block.name.clone()) {
                continue;
            }
            for e in &block.entities {
                if let Geometry::Insert { block, .. } = &e.geometry {
                    todo.push(block.clone());
                }
            }
        }
        let rename = |n: &str| -> String {
            if !is_dependent(n) {
                return n.to_string();
            }
            if insert { n.rsplit('|').next().unwrap_or(n).to_string() } else { n.replace('|', "$0$") }
        };
        // Layers and linetypes the bound blocks use get their new names.
        let mut layers = Vec::new();
        let mut linetypes = Vec::new();
        for b in reach.iter().filter_map(|b| self.block(b)) {
            for e in &b.entities {
                layers.push(e.layer.clone());
                linetypes.push(e.linetype.clone());
                if let Geometry::Insert { attributes, .. } = &e.geometry {
                    layers.extend(attributes.iter().map(|a| a.layer.clone()));
                }
            }
        }
        let mut new_layers = Vec::new();
        for l in layers.iter().filter(|l| is_dependent(l)).filter_map(|l| self.layer(l)) {
            let n = rename(&l.name);
            if self.layer(&n).is_none() && !new_layers.iter().any(|x: &crate::Layer| x.name.eq_ignore_ascii_case(&n)) {
                new_layers.push(crate::Layer { name: n, linetype: rename(&l.linetype), ..l.clone() });
            }
        }
        if !new_layers.is_empty() {
            self.edit_layers(|table| table.extend(new_layers));
        }
        let dependent_linetypes: Vec<Linetype> =
            linetypes.iter().filter(|l| is_dependent(l)).filter_map(|l| self.linetype(l)).collect();
        for lt in dependent_linetypes {
            let n = rename(&lt.name);
            if self.linetype(&n).is_none() {
                self.define_linetype(Linetype { name: n, ..lt });
            }
        }
        for b in &reach {
            let Some(block) = self.block(b).cloned() else { continue };
            let entities = block.entities.iter().map(|e| renamed_entity(e, &rename)).collect();
            let new_name = rename(&block.name);
            if new_name != block.name {
                self.remove_block(&block.name);
                // Insert keeps a block this drawing already has.
                if self.block(&new_name).is_some() {
                    continue;
                }
            }
            self.define_block(Block { name: new_name, base: block.base, entities });
        }
        self.remove_xref(&xref.name);
        self.purge_dependents();
    }

    /// Drops the layers, linetypes and blocks that came from xrefs and
    /// nothing uses any more.
    pub fn purge_dependents(&mut self) {
        loop {
            let used_blocks: BTreeSet<String> = self
                .entities()
                .map(|e| &e.geometry)
                .chain(self.blocks().flat_map(|b| b.entities.iter().map(|e| &e.geometry)))
                .filter_map(|g| match g {
                    Geometry::Insert { block, .. } => Some(block.to_ascii_uppercase()),
                    _ => None,
                })
                .collect();
            let unused: Vec<String> = self
                .blocks()
                .filter(|b| is_dependent(&b.name) && !used_blocks.contains(&b.name.to_ascii_uppercase()))
                .map(|b| b.name.clone())
                .collect();
            if unused.is_empty() {
                break;
            }
            for b in unused {
                self.remove_block(&b);
            }
        }
        let mut used: BTreeSet<String> = BTreeSet::new();
        for (layer, linetype) in self
            .entities()
            .map(|e| (&e.layer, &e.linetype))
            .chain(self.blocks().flat_map(|b| b.entities.iter().map(|e| (&e.layer, &e.linetype))))
        {
            used.insert(layer.to_ascii_uppercase());
            used.insert(linetype.to_ascii_uppercase());
        }
        let current = self.current_layer().to_ascii_uppercase();
        let stale =
            |n: &str| is_dependent(n) && !used.contains(&n.to_ascii_uppercase()) && n.to_ascii_uppercase() != current;
        if self.layers().iter().any(|l| stale(&l.name)) {
            self.edit_layers(|table| table.retain(|l| !stale(&l.name)));
        }
        let by_layers: BTreeSet<String> = self.layers().iter().map(|l| l.linetype.to_ascii_uppercase()).collect();
        let unused: Vec<String> = self
            .linetypes()
            .into_iter()
            .map(|l| l.name)
            .filter(|n| {
                is_dependent(n)
                    && !used.contains(&n.to_ascii_uppercase())
                    && !by_layers.contains(&n.to_ascii_uppercase())
            })
            .collect();
        for n in unused {
            self.remove_linetype(&n);
        }
    }
}

#[cfg(test)]
mod tests {
    use oxidraft_geom::Point;

    use super::*;
    use crate::{Color, Layer, Placement, XrefStatus};

    /// A site plan: a boundary on its own layer and a tree block.
    fn site() -> Document {
        let mut d = Document::new();
        d.add_layer(Layer::new("Boundary", 1));
        d.add_on_layer(Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) }, "Boundary", Color::ByLayer);
        d.define_block(Block {
            name: "Tree".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new(
                "Boundary",
                Color::ByLayer,
                Geometry::Circle { center: Point::ORIGIN, radius: 1.0 },
            )],
        });
        d.add(Geometry::Insert {
            block: "Tree".into(),
            placement: Placement::at(Point::new(5.0, 5.0)),
            attributes: Vec::new(),
            clip: None,
        });
        d.set_insbase(Point::new(1.0, 1.0));
        d
    }

    fn attach(host: &mut Document, name: &str) {
        host.set_xref(Xref {
            name: name.into(),
            path: format!("{name}.dwg"),
            overlay: false,
            status: XrefStatus::NotFound,
        });
        host.load_xref(name, &site());
        host.add(Geometry::Insert {
            block: name.into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: Vec::new(),
            clip: None,
        });
    }

    #[test]
    fn attaching_brings_prefixed_layers_and_blocks() {
        let mut host = Document::new();
        host.add_layer(Layer::new("Boundary", 3));
        attach(&mut host, "SITE");
        assert_eq!(host.xref("site").map(|x| x.status), Some(XrefStatus::Loaded));
        assert!(host.layer("SITE|Boundary").is_some_and(|l| l.color == 1));
        assert!(host.layer("Boundary").is_some_and(|l| l.color == 3), "this drawing's own layer is untouched");
        let block = host.block("SITE").unwrap();
        assert_eq!(block.base, Point::new(1.0, 1.0));
        assert_eq!(block.entities.len(), 2);
        assert!(
            block
                .entities
                .iter()
                .any(|e| matches!(&e.geometry, Geometry::Insert { block, .. } if block == "SITE|Tree"))
        );
        // Its objects draw through the reference, on the prefixed layers.
        let reference = host.entities().last().unwrap().clone();
        let styled = host.expanded_styled(&reference);
        assert_eq!(styled.len(), 2);
        assert!(styled.iter().all(|s| s.layer == "SITE|Boundary" && s.color == 1));
        // Turning its layer off hides it, and survives a reload.
        let mut off = host.layer("SITE|Boundary").cloned().unwrap();
        off.on = false;
        host.update_layer(off);
        host.load_xref("SITE", &site());
        assert!(host.expanded_styled(&reference).is_empty());
        assert_eq!(dependent_name("SITE", "0"), "0");
        assert_eq!(dependent_name("SITE", "B|Wall"), "B|Wall");
    }

    #[test]
    fn unload_detach_and_undo() {
        let mut host = Document::new();
        attach(&mut host, "SITE");
        host.set_xref_status("SITE", XrefStatus::Unloaded);
        assert!(host.block("SITE").unwrap().entities.is_empty());
        host.load_xref("SITE", &site());
        host.begin("DETACH");
        host.detach_xref("SITE");
        host.commit();
        assert!(host.xref("SITE").is_none() && host.block("SITE").is_none() && host.is_empty());
        assert!(host.layers().iter().all(|l| !is_dependent(&l.name)));
        assert!(host.blocks().all(|b| !is_dependent(&b.name)));
        host.undo();
        assert!(host.xref("SITE").is_some() && host.layer("SITE|Boundary").is_some() && host.len() == 1);
    }

    #[test]
    fn bind_keeps_names_apart_and_insert_merges_them() {
        let mut host = Document::new();
        attach(&mut host, "SITE");
        host.bind_xref("SITE", false);
        assert!(host.xref("SITE").is_none());
        assert!(host.layer("SITE$0$Boundary").is_some() && host.block("SITE$0$Tree").is_some());
        assert!(host.layers().iter().all(|l| !is_dependent(&l.name)), "{:?}", host.layers());
        assert_eq!(host.block("SITE").unwrap().entities.len(), 2, "now an ordinary block");

        let mut host = Document::new();
        host.add_layer(Layer::new("Boundary", 3));
        attach(&mut host, "SITE");
        host.bind_xref("SITE", true);
        assert!(host.layer("Boundary").is_some_and(|l| l.color == 3), "merged into the existing layer");
        assert!(host.block("Tree").is_some() && host.blocks().all(|b| !is_dependent(&b.name)));
        let entities = &host.block("SITE").unwrap().entities;
        assert!(entities.iter().any(|e| e.layer == "Boundary") && entities.iter().all(|e| !is_dependent(&e.layer)));
    }
}

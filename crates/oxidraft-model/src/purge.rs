//! PURGE and AUDIT: what a drawing defines but doesn't use, and objects
//! that are broken.

use crate::{Document, EntityId, Geometry, entity::Entity, xref::is_dependent};

/// What PURGE can take out: layers, blocks and linetypes nothing uses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Unused {
    pub layers: Vec<String>,
    pub blocks: Vec<String>,
    pub linetypes: Vec<String>,
}

impl Unused {
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty() && self.blocks.is_empty() && self.linetypes.is_empty()
    }

    pub fn count(&self) -> usize {
        self.layers.len() + self.blocks.len() + self.linetypes.len()
    }
}

/// The blocks a shape refers to.
fn blocks_in(g: &Geometry, out: &mut Vec<String>) {
    if let Geometry::Insert { block, .. } = g {
        out.push(block.to_ascii_uppercase());
    }
}

impl Document {
    /// The layers, blocks and linetypes nothing uses: what PURGE offers.
    /// What a block definition uses counts as used until the block goes
    /// (purging nested items takes it next). Layer 0, the current layer,
    /// Defpoints and what external references bring are kept.
    pub fn unused(&self) -> Unused {
        let entities: Vec<&Entity> = self.all_entities().collect();
        let mut used_blocks = Vec::new();
        for e in &entities {
            blocks_in(&e.geometry, &mut used_blocks);
        }
        for b in self.blocks() {
            for be in &b.entities {
                blocks_in(&be.geometry, &mut used_blocks);
            }
        }
        let is_used_block = |name: &str| used_blocks.iter().any(|u| u.eq_ignore_ascii_case(name));
        let kept_blocks: Vec<_> = self.blocks().collect();
        let mut layers_used: Vec<&str> = entities.iter().map(|e| e.layer.as_str()).collect();
        let mut linetypes_used: Vec<&str> = entities.iter().map(|e| e.linetype.as_str()).collect();
        for e in &entities {
            if let Geometry::Insert { attributes, .. } = &e.geometry {
                layers_used.extend(attributes.iter().map(|a| a.layer.as_str()));
            }
        }
        for b in &kept_blocks {
            layers_used.extend(b.entities.iter().map(|be| be.layer.as_str()));
            linetypes_used.extend(b.entities.iter().map(|be| be.linetype.as_str()));
        }
        linetypes_used.extend(self.layers().iter().map(|l| l.linetype.as_str()));
        let has = |list: &[&str], name: &str| list.iter().any(|u| u.eq_ignore_ascii_case(name));
        let current = self.current_layer().to_string();
        Unused {
            layers: self
                .layers()
                .iter()
                .map(|l| l.name.clone())
                .filter(|n| n != "0" && *n != current && !n.eq_ignore_ascii_case("Defpoints") && !is_dependent(n))
                .filter(|n| !has(&layers_used, n))
                .collect(),
            blocks: self
                .blocks()
                .map(|b| b.name.clone())
                .filter(|n| !n.starts_with('*') && self.xref(n).is_none() && !is_dependent(n) && !is_used_block(n))
                .collect(),
            linetypes: self.own_linetypes().into_iter().filter(|n| !has(&linetypes_used, n)).collect(),
        }
    }

    /// Takes out what `what` names; with `nested`, again and again while
    /// that leaves more unused (a block's layers once the block is gone).
    /// Returns how many went.
    pub fn purge(&mut self, what: &Unused, nested: bool) -> usize {
        let mut count = 0;
        let mut what = what.clone();
        loop {
            for b in &what.blocks {
                count += usize::from(self.remove_block(b).is_some());
            }
            for l in &what.layers {
                count += usize::from(self.delete_layer(l).is_ok());
            }
            for l in &what.linetypes {
                count += usize::from(self.remove_linetype(l));
            }
            if !nested {
                return count;
            }
            // Only more of the kinds asked for.
            let next = self.unused();
            what = Unused {
                layers: if what.layers.is_empty() { Vec::new() } else { next.layers },
                blocks: if what.blocks.is_empty() { Vec::new() } else { next.blocks },
                linetypes: if what.linetypes.is_empty() { Vec::new() } else { next.linetypes },
            };
            if what.is_empty() {
                return count;
            }
        }
    }

    /// Lines and arcs of no length and text with nothing in it: PURGE's
    /// zero-length geometry and empty text.
    pub fn empty_objects(&self) -> Vec<EntityId> {
        self.all_entities()
            .filter(|e| match &e.geometry {
                Geometry::Line { start, end } => start.distance(*end) < 1e-12,
                Geometry::Arc { radius, start_angle, end_angle, .. } => {
                    *radius < 1e-12 || (end_angle - start_angle).rem_euclid(360.0) < 1e-12
                }
                Geometry::Polyline { vertices, .. } => {
                    vertices.windows(2).all(|w| w[0].point.distance(w[1].point) < 1e-12)
                }
                Geometry::Text { content, .. } => content.trim().is_empty(),
                _ => false,
            })
            .map(|e| e.id)
            .collect()
    }

    /// AUDIT: what's broken, as lines to print; with `fix`, mended:
    /// objects on layers that don't exist get them back, and references
    /// to missing blocks, shapes with impossible numbers (not finite,
    /// a radius or height of nothing, a polyline without two vertices) and
    /// objects of layouts that are gone are erased.
    pub fn audit(&mut self, fix: bool) -> Vec<String> {
        let mut found = Vec::new();
        let missing_layers: Vec<String> = {
            let mut names: Vec<String> =
                self.all_entities().filter(|e| self.layer(&e.layer).is_none()).map(|e| e.layer.clone()).collect();
            names.sort();
            names.dedup();
            names
        };
        for name in &missing_layers {
            found.push(format!("Layer \"{name}\" is used but not defined{}", if fix { ": added" } else { "" }));
            if fix {
                self.add_layer(crate::Layer::new(name.clone(), 7));
            }
        }
        let mut erase = Vec::new();
        for e in self.all_entities() {
            let problem = match &e.geometry {
                Geometry::Insert { block, .. } if self.block(block).is_none() && self.xref(block).is_none() => {
                    Some(format!("Block \"{block}\" not found"))
                }
                Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. }
                    if radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) =>
                {
                    Some("a radius of nothing".into())
                }
                Geometry::Polyline { vertices, .. } if vertices.len() < 2 => Some("fewer than two vertices".into()),
                Geometry::Text { height, .. } if height.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) => {
                    Some("text with no height".into())
                }
                _ if e.space != crate::layout::MODEL && self.layout(e.space).is_none() => {
                    Some("on a layout that doesn't exist".into())
                }
                g => {
                    let finite = g.outline_runs().iter().flatten().all(|p| p.x.is_finite() && p.y.is_finite());
                    (!finite).then(|| "coordinates that aren't numbers".into())
                }
            };
            if let Some(p) = problem {
                found.push(format!("{} {}: {p}{}", e.geometry.name(), e.id.0, if fix { ": erased" } else { "" }));
                erase.push(e.id);
            }
        }
        if fix {
            for id in erase {
                self.remove(id);
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, BlockEntity, Color, Layer, Linetype, Placement};
    use oxidraft_geom::Point;

    #[test]
    fn purge_takes_out_what_nothing_uses_nested_too() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Used", 1));
        d.add_layer(Layer::new("Idle", 2));
        d.add_layer(Layer::new("InBlock", 3));
        d.define_linetype(Linetype { name: "DOTTY".into(), description: String::new(), pattern: vec![0.0, -1.0] });
        let line = Geometry::Line { start: Point::ORIGIN, end: Point::new(1.0, 0.0) };
        d.add_on_layer(line.clone(), "Used", Color::ByLayer);
        d.define_block(Block {
            name: "OLD".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("InBlock", Color::ByLayer, line.clone())],
        });
        d.define_block(Block { name: "KEPT".into(), base: Point::ORIGIN, entities: vec![] });
        d.add(Geometry::Insert {
            block: "KEPT".into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: vec![],
            clip: None,
        });
        let unused = d.unused();
        assert_eq!(unused.layers, ["Idle"], "InBlock is used, by OLD");
        assert_eq!(unused.blocks, ["OLD"]);
        assert_eq!(unused.linetypes, ["DOTTY"]);
        d.begin("PURGE");
        assert_eq!(d.purge(&unused, true), 4, "OLD, then Idle and InBlock, and DOTTY");
        d.commit();
        assert!(d.unused().is_empty());
        d.undo();
        assert_eq!(d.unused().count(), 3);
    }

    #[test]
    fn audit_finds_and_mends() {
        let mut d = Document::new();
        d.add(Geometry::Circle { center: Point::ORIGIN, radius: 0.0 });
        d.add(Geometry::Insert {
            block: "GONE".into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: vec![],
            clip: None,
        });
        let ok = d.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(f64::NAN, 1.0) });
        d.update(ok, |e| e.layer = "Ghost".into());
        let report = d.audit(false);
        assert_eq!(report.len(), 4, "{report:?}");
        assert_eq!(d.len(), 3, "nothing changed");
        d.audit(true);
        assert_eq!(d.len(), 0);
        assert!(d.layer("Ghost").is_some());
        assert!(d.audit(false).is_empty());
    }

    #[test]
    fn empty_text_and_zero_length_lines() {
        let mut d = Document::new();
        let empty = d.add(Geometry::Line { start: Point::ORIGIN, end: Point::ORIGIN });
        d.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(1.0, 0.0) });
        assert_eq!(d.empty_objects(), vec![empty]);
    }
}

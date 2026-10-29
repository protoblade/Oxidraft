//! -RENAME: renames a block, layer, linetype or named view at the command
//! line (RENAME's dialog is the app's).

use oxidraft_model::Document;

use crate::{Command, Ctx, Input, Prompt, Step};

/// The kinds of named object that can be renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    Block,
    Layer,
    Linetype,
    View,
}

impl Named {
    pub const ALL: [Named; 4] = [Named::Block, Named::Layer, Named::Linetype, Named::View];

    pub fn plural(self) -> &'static str {
        match self {
            Named::Block => "Blocks",
            Named::Layer => "Layers",
            Named::Linetype => "Linetypes",
            Named::View => "Views",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Named::Block => "block",
            Named::Layer => "layer",
            Named::Linetype => "linetype",
            Named::View => "view",
        }
    }

    /// The names of this kind that can be renamed.
    pub fn names(self, doc: &Document) -> Vec<String> {
        let mut names: Vec<String> = match self {
            Named::Block => doc
                .blocks()
                .filter(|b| !b.name.starts_with('*') && doc.xref(&b.name).is_none())
                .map(|b| b.name.clone())
                .collect(),
            Named::Layer => doc.layers().iter().filter(|l| l.name != "0").map(|l| l.name.clone()).collect(),
            Named::Linetype => {
                // The drawing's own, and the standard ones it uses.
                let mut names = doc.own_linetypes();
                let used = doc
                    .all_entities()
                    .map(|e| e.linetype.as_str())
                    .chain(doc.layers().iter().map(|l| l.linetype.as_str()))
                    .chain(doc.blocks().flat_map(|b| b.entities.iter().map(|be| be.linetype.as_str())));
                for n in used {
                    let special = ["Continuous", "ByLayer", "ByBlock"].iter().any(|s| s.eq_ignore_ascii_case(n));
                    if let Some(l) = doc.linetype(n).filter(|_| !special)
                        && !names.iter().any(|x| x.eq_ignore_ascii_case(&l.name))
                    {
                        names.push(l.name);
                    }
                }
                names
            }
            Named::View => doc.views().iter().map(|v| v.name.clone()).collect(),
        };
        names.retain(|n| !oxidraft_model::is_dependent(n));
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// Renames `old` to `new`.
    pub fn rename(self, doc: &mut Document, old: &str, new: &str) -> Result<(), String> {
        match self {
            Named::Block => doc.rename_block(old, new),
            Named::Layer => doc.rename_layer(old, new.trim()),
            Named::Linetype => doc.rename_linetype(old, new),
            Named::View => doc.rename_view(old, new),
        }
    }
}

#[derive(Default)]
pub struct Rename {
    kind: Option<Named>,
    old: Option<String>,
}

impl Command for Rename {
    fn prompt(&self) -> Prompt {
        match (self.kind, &self.old) {
            (None, _) => Prompt::new("Enter object type to rename").with_keywords(&["Block", "LAyer", "LType", "VIew"]),
            (Some(k), None) => Prompt::new(format!("Enter old {} name", k.noun())),
            (Some(k), Some(old)) => Prompt::new(format!("Rename {} {old} to", k.noun())),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.kind, &self.old, input) {
            (None, _, Input::Keyword(k)) => {
                self.kind = Some(match k {
                    "Block" => Named::Block,
                    "LAyer" => Named::Layer,
                    "LType" => Named::Linetype,
                    _ => Named::View,
                });
            }
            (Some(k), None, Input::Text(old)) => {
                let name = k.names(ctx.doc).into_iter().find(|n| n.eq_ignore_ascii_case(old.trim()));
                self.old = Some(name.ok_or_else(|| format!("Cannot find {} \"{}\".", k.noun(), old.trim()))?);
            }
            (Some(k), Some(old), Input::Text(new)) => {
                k.rename(ctx.doc, old, &new)?;
                return Ok(Step::Done);
            }
            (_, _, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Enter an object type, then names.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        self.kind.is_some()
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Block, BlockEntity, Color, Document, Geometry, Layer, NamedView, Placement};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn renames_blocks_layers_linetypes_and_views() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let circle = Geometry::Circle { center: Point::ORIGIN, radius: 1.0 };
        d.define_block(Block {
            name: "Bolt".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByLayer, circle)],
        });
        let insert = |name: &str| Geometry::Insert {
            block: name.into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: Vec::new(),
            clip: None,
        };
        let mut nut = BlockEntity::new("0", Color::ByLayer, insert("Bolt"));
        nut.linetype = "DASHED".into();
        d.define_block(Block { name: "Fixing".into(), base: Point::ORIGIN, entities: vec![nut] });
        let bolt = d.add(insert("Bolt"));
        d.add_layer(Layer { linetype: "DASHED".into(), ..Layer::new("Walls", 1) });
        d.save_view(NamedView { name: "Front".into(), center: Point::ORIGIN, height: 10.0, width: 10.0, space: 0 })
            .unwrap();
        let line = d.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(1.0, 0.0) });
        d.update(line, |e| e.linetype = "DASHED".into());

        run(&mut e, &mut d, &["-rename", "b", "bolt", "Bolt M8"]);
        assert!(d.block("Bolt").is_none() && d.block("Bolt M8").is_some());
        assert!(matches!(&d.entity(bolt).unwrap().geometry, Geometry::Insert { block, .. } if block == "Bolt M8"));
        let nested = &d.block("Fixing").unwrap().entities[0].geometry;
        assert!(matches!(nested, Geometry::Insert { block, .. } if block == "Bolt M8"), "nested references too");
        run(&mut e, &mut d, &["-rename", "la", "walls", "Outer walls"]);
        assert!(d.layer("Outer walls").is_some());
        run(&mut e, &mut d, &["-rename", "lt", "dashed", "Hidden2"]);
        assert_eq!(d.entity(line).unwrap().linetype, "Hidden2");
        assert_eq!(d.layer("Outer walls").unwrap().linetype, "Hidden2");
        assert_eq!(d.block("Fixing").unwrap().entities[0].linetype, "Hidden2");
        assert!(d.linetype("Hidden2").is_some_and(|l| !l.pattern.is_empty()));
        run(&mut e, &mut d, &["-rename", "vi", "front", "North"]);
        assert_eq!(d.views()[0].name, "North");
        // Taken names are refused.
        run(&mut e, &mut d, &["-rename", "b", "Fixing", "Bolt M8"]);
        assert!(d.block("Fixing").is_some());
        run(&mut e, &mut d, &[""]);
    }
}

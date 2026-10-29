//! ATTDEF (ATT): an attribute definition, for a block's references to ask
//! for; ATTSYNC brings references' attributes in line with their block's
//! definitions.

use oxidraft_geom::Point;
use oxidraft_model::{Attach, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

#[derive(Clone, PartialEq)]
enum Stage {
    Modes,
    Tag,
    Prompt,
    Default,
    Point,
    Height(Point),
    Rotation(Point, f64),
}

/// ATTDEF on the command line: modes (Invisible, Constant), tag, prompt,
/// default value, start point, height and rotation.
pub struct AttDef {
    stage: Stage,
    invisible: bool,
    constant: bool,
    tag: String,
    prompt: String,
    default: String,
    height: f64,
}

impl Default for AttDef {
    fn default() -> Self {
        Self {
            stage: Stage::Modes,
            invisible: false,
            constant: false,
            tag: String::new(),
            prompt: String::new(),
            default: String::new(),
            height: 2.5,
        }
    }
}

impl Command for AttDef {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.height = vars.text_height;
        None
    }

    fn prompt(&self) -> Prompt {
        let yn = |b: bool| if b { "Y" } else { "N" };
        match &self.stage {
            Stage::Modes => Prompt::new(format!(
                "Current attribute modes: Invisible={} Constant={}\nEnter an option to change",
                yn(self.invisible),
                yn(self.constant)
            ))
            .with_keywords(&["Invisible", "Constant"])
            .with_default("done"),
            Stage::Tag => Prompt::new("Enter attribute tag name"),
            Stage::Prompt => Prompt::new("Enter attribute prompt"),
            Stage::Default if self.constant => Prompt::new("Enter attribute value"),
            Stage::Default => Prompt::new("Enter default attribute value"),
            Stage::Point => Prompt::new("Specify start point of text"),
            Stage::Height(_) => Prompt::new("Specify text height").with_default(format!("{:.4}", self.height)),
            Stage::Rotation(..) => Prompt::new("Specify rotation angle of text").with_default("0"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (Stage::Modes, Input::Keyword("Invisible")) => self.invisible = !self.invisible,
            (Stage::Modes, Input::Keyword(_)) => self.constant = !self.constant,
            (Stage::Modes, Input::Enter) => self.stage = Stage::Tag,
            (Stage::Tag, Input::Text(t)) => {
                if t.trim().is_empty() || t.contains(' ') {
                    return Err("A tag is one word.".into());
                }
                self.tag = t.trim().to_uppercase();
                self.stage = if self.constant { Stage::Default } else { Stage::Prompt };
            }
            (Stage::Tag, Input::Enter) => return Err("Enter a tag.".into()),
            (Stage::Prompt, Input::Text(t)) => {
                self.prompt = t;
                self.stage = Stage::Default;
            }
            (Stage::Prompt, Input::Enter) => {
                self.prompt = self.tag.clone();
                self.stage = Stage::Default;
            }
            (Stage::Default, Input::Text(t)) => {
                self.default = t;
                self.stage = Stage::Point;
            }
            (Stage::Default, Input::Enter) => self.stage = Stage::Point,
            (Stage::Point, Input::Point(p)) => self.stage = Stage::Height(p),
            (Stage::Height(p), Input::Value(h)) if h > 0.0 => {
                self.height = h;
                self.stage = Stage::Rotation(p, h);
            }
            (Stage::Height(p), Input::Enter) => self.stage = Stage::Rotation(p, self.height),
            (Stage::Rotation(p, height), input @ (Input::Value(_) | Input::Enter)) => {
                let rotation = if let Input::Value(a) = input { a } else { 0.0 };
                ctx.vars.text_height = height;
                ctx.doc.add(Geometry::AttributeDefinition {
                    tag: self.tag.clone(),
                    prompt: self.prompt.clone(),
                    default: self.default.clone(),
                    position: p,
                    height,
                    rotation,
                    attach: Attach::BottomLeft,
                    invisible: self.invisible,
                    constant: self.constant,
                });
                return Ok(Step::Done);
            }
            (Stage::Point, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, Stage::Tag | Stage::Prompt | Stage::Default)
    }

    fn text_with_spaces(&self) -> bool {
        matches!(self.stage, Stage::Prompt | Stage::Default)
    }
}

/// ATTSYNC: every reference of the block picked gets the attributes its
/// definitions now say, keeping the values of the tags it had.
#[derive(Default)]
pub struct AttSync;

/// ATTSYNC's work: every reference to block `name` given the attributes
/// its definition now has, keeping the values of tags it still has.
/// Returns how many references.
pub fn sync_attributes(doc: &mut oxidraft_model::Document, name: &str) -> usize {
    let Some(def) = doc.block(name).cloned() else { return 0 };
    let references: Vec<_> = doc
        .all_entities()
        .filter(|e| matches!(&e.geometry, Geometry::Insert { block, .. } if block.eq_ignore_ascii_case(name)))
        .map(|e| e.id)
        .collect();
    for id in &references {
        doc.update(*id, |e| {
            if let Geometry::Insert { placement, attributes, .. } = &mut e.geometry {
                let old = attributes.clone();
                let value_of = |tag: &str| old.iter().find(|a| a.tag == tag).map(|a| a.value.clone());
                *attributes = def.attributes_for(placement, value_of);
            }
        });
    }
    references.len()
}

impl Command for AttSync {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select a block")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } => {
                let picked = ctx.pick(at).and_then(|id| ctx.doc.entity(id));
                let Some(Geometry::Insert { block, .. }) = picked.map(|e| &e.geometry) else {
                    return Err("That isn't a block reference.".into());
                };
                let name = block.clone();
                ctx.doc.block(&name).ok_or("The block isn't defined.")?;
                let n = sync_attributes(ctx.doc, &name);
                ctx.messages.push(format!("ATTSYNC complete: {n} reference(s) of \"{name}\"."));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Pick a block reference.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn attdef_block_insert_asks_and_attsync_follows() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["c", "0,0", "5"]);
        run(&mut e, &mut d, &["att", "", "room", "Room number", "101", "-2,-1", "2", "0"]);
        assert!(
            matches!(&d.entities().last().unwrap().geometry, Geometry::AttributeDefinition { tag, .. } if tag == "ROOM")
        );
        // The circle and the definition become a block.
        run(&mut e, &mut d, &["b", "ROOMTAG", "0,0", "all", ""]);
        // Inserting asks for the value.
        run(&mut e, &mut d, &["i", "ROOMTAG", "100,100", "Kitchen 12"]);
        let insert = d
            .entities()
            .find(|x| matches!(&x.geometry, Geometry::Insert { placement, .. } if placement.position.x == 100.0))
            .unwrap()
            .clone();
        let Geometry::Insert { attributes, .. } = &insert.geometry else { panic!() };
        assert_eq!(attributes.len(), 1);
        assert_eq!((attributes[0].tag.as_str(), attributes[0].value.as_str()), ("ROOM", "Kitchen 12"));
        assert!(attributes[0].position.approx_eq(Point::new(98.0, 99.0)));
        // A second definition in the block: ATTSYNC adds it with its default.
        let mut block = d.block("ROOMTAG").unwrap().clone();
        let mut area =
            block.entities.iter().find(|b| matches!(b.geometry, Geometry::AttributeDefinition { .. })).unwrap().clone();
        if let Geometry::AttributeDefinition { tag, default, .. } = &mut area.geometry {
            (*tag, *default) = ("AREA".into(), "20 m2".into());
        }
        block.entities.push(area);
        d.define_block(block);
        run(&mut e, &mut d, &["attsync"]);
        let at = Point::new(105.0, 100.0);
        e.click(at, at, false, &mut d, 0.5);
        let Some(Geometry::Insert { attributes, .. }) = d.entity(insert.id).map(|x| x.geometry.clone()) else {
            panic!()
        };
        let values: Vec<(&str, &str)> = attributes.iter().map(|a| (a.tag.as_str(), a.value.as_str())).collect();
        assert_eq!(values, [("ROOM", "Kitchen 12"), ("AREA", "20 m2")]);
    }
}

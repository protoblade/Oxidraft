use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Block, BlockEntity, Geometry, Placement, check_name};

use crate::{
    Command, Ctx, Input, Prompt, Step, Vars,
    common::{SELECT, selected_entities},
};

enum DefineStage {
    Name,
    Base { name: String },
    Select { name: String, base: Point },
}

/// BLOCK: defines a named block from selected objects and replaces them
/// with a reference to it (like the dialog's "Convert to block").
pub struct DefineBlock {
    stage: DefineStage,
}

impl Default for DefineBlock {
    fn default() -> Self {
        Self { stage: DefineStage::Name }
    }
}

impl Command for DefineBlock {
    fn prompt(&self) -> Prompt {
        match self.stage {
            DefineStage::Name => Prompt::new("Enter block name"),
            DefineStage::Base { .. } => Prompt::new("Specify insertion base point"),
            DefineStage::Select { .. } => Prompt::new("Select objects"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (DefineStage::Name, Input::Text(name)) => {
                check_name(&name)?;
                if ctx.doc.block(&name).is_some() {
                    return Err(format!("A block named \"{name}\" already exists."));
                }
                self.stage = DefineStage::Base { name };
            }
            (DefineStage::Base { name }, Input::Point(base)) => {
                self.stage = DefineStage::Select { name: name.clone(), base };
            }
            (DefineStage::Select { name, base }, Input::Selection(selection)) => {
                let selected = selected_entities(&selection, ctx);
                if selected.is_empty() {
                    ctx.messages.push("No objects selected; no block was created.".into());
                    return Ok(Step::Done);
                }
                let entities = selected
                    .iter()
                    .map(|e| BlockEntity {
                        layer: e.layer.clone(),
                        color: e.color,
                        linetype: e.linetype.clone(),
                        lineweight: e.lineweight,
                        geometry: e.geometry.clone(),
                    })
                    .collect();
                let block = Block { name: name.clone(), base: *base, entities };
                // The objects become a reference, its attributes at their defaults.
                let placement = Placement::at(*base);
                let attributes = block.attributes_for(&placement, |_| None);
                ctx.doc.define_block(block);
                for e in &selected {
                    ctx.doc.remove(e.id);
                }
                ctx.doc.add(Geometry::Insert { block: name.clone(), placement, attributes, clip: None });
                // INSERT will offer the new block first.
                ctx.vars.insert_name = Some(name.clone());
                let n = selected.len();
                ctx.messages
                    .push(format!("Block \"{name}\" created from {n} object{}.", if n == 1 { "" } else { "s" }));
                return Ok(Step::Done);
            }
            (DefineStage::Select { .. }, _) => return Err(SELECT.into()),
            (_, Input::Enter) => return Ok(Step::Done),
            (DefineStage::Name, _) => return Err("Enter a name for the block.".into()),
            _ => return Err("Point required.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, DefineStage::Name)
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, DefineStage::Select { .. })
    }
}

enum InsertStage {
    Name,
    Point,
    Scale,
    Rotate,
    /// Asking for the block's attribute values, one by one: where it goes,
    /// and the values given so far.
    Attributes(Point, Vec<String>),
}

/// INSERT: places a reference to a block, with optional scale and rotation.
pub struct InsertBlock {
    stage: InsertStage,
    /// The last block inserted (INSNAME), offered as the default.
    default_name: Option<String>,
    block: Option<Block>,
    scale: f64,
    rotation: f64,
}

impl Default for InsertBlock {
    fn default() -> Self {
        Self { stage: InsertStage::Name, default_name: None, block: None, scale: 1.0, rotation: 0.0 }
    }
}

impl InsertBlock {
    fn choose(&mut self, name: &str, ctx: &mut Ctx) -> Result<(), String> {
        let block =
            ctx.doc.block(name).cloned().ok_or(format!("Block \"{name}\" not found. Enter ? to list blocks."))?;
        self.block = Some(block);
        self.stage = InsertStage::Point;
        Ok(())
    }

    fn placement(&self, at: Point) -> Placement {
        Placement { position: at, scale: self.scale, rotation: self.rotation, mirror: false }
    }

    /// The reference, its attributes the `values` given in turn.
    fn place(&self, at: Point, values: Vec<String>, ctx: &mut Ctx) -> Step {
        let Some(block) = &self.block else { return Step::Done };
        let placement = self.placement(at);
        let tags: Vec<String> = block.attribute_prompts().into_iter().map(|p| p.0).collect();
        let value_of = |tag: &str| tags.iter().position(|t| t == tag).and_then(|i| values.get(i).cloned());
        let attributes = block.attributes_for(&placement, value_of);
        ctx.doc.add(Geometry::Insert { block: block.name.clone(), placement, attributes, clip: None });
        ctx.vars.insert_name = Some(block.name.clone());
        Step::Done
    }
}

impl Command for InsertBlock {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.default_name = vars.insert_name.clone();
        None
    }

    fn prompt(&self) -> Prompt {
        match &self.stage {
            InsertStage::Name => {
                let p = Prompt::new("Enter block name or [?]");
                match &self.default_name {
                    Some(n) => p.with_default(n.clone()),
                    None => p,
                }
            }
            InsertStage::Point => Prompt::new("Specify insertion point").with_keywords(&["Scale", "Rotate"]),
            InsertStage::Scale => Prompt::new("Specify scale factor").with_default(format!("{:.4}", self.scale)),
            InsertStage::Rotate => Prompt::new("Specify rotation angle").with_default(format!("{:.0}", self.rotation)),
            InsertStage::Attributes(_, values) => {
                let prompts = self.block.as_ref().map(|b| b.attribute_prompts()).unwrap_or_default();
                match prompts.get(values.len()) {
                    Some((tag, prompt, default)) => {
                        let text = if prompt.trim().is_empty() { tag.clone() } else { prompt.clone() };
                        Prompt::new(text).with_default(default.clone())
                    }
                    None => Prompt::new("Enter attribute value"),
                }
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (InsertStage::Name, Input::Text(t)) if t == "?" => {
                let names: Vec<String> = ctx.doc.blocks().map(|b| b.name.clone()).collect();
                if names.is_empty() {
                    ctx.messages.push("No blocks are defined in this drawing.".into());
                } else {
                    ctx.messages.push(format!("Defined blocks: {}", names.join(", ")));
                }
            }
            (InsertStage::Name, Input::Text(name)) => self.choose(&name, ctx)?,
            (InsertStage::Name, Input::Enter) => match self.default_name.clone() {
                Some(name) => self.choose(&name, ctx)?,
                None => return Ok(Step::Done),
            },
            (InsertStage::Point, Input::Point(at)) => {
                let block = self.block.as_ref().ok_or("No block chosen.")?;
                // A block with attributes asks for their values first.
                if !block.attribute_prompts().is_empty() {
                    ctx.messages.push("Enter attribute values".into());
                    self.stage = InsertStage::Attributes(at, Vec::new());
                    return Ok(Step::Continue);
                }
                return Ok(self.place(at, Vec::new(), ctx));
            }
            (InsertStage::Attributes(at, values), input @ (Input::Text(_) | Input::Enter)) => {
                let prompts = self.block.as_ref().map(|b| b.attribute_prompts()).unwrap_or_default();
                let default = prompts.get(values.len()).map(|p| p.2.clone()).unwrap_or_default();
                let mut values = values.clone();
                values.push(match input {
                    Input::Text(t) => t,
                    _ => default,
                });
                if values.len() < prompts.len() {
                    self.stage = InsertStage::Attributes(*at, values);
                    return Ok(Step::Continue);
                }
                let at = *at;
                return Ok(self.place(at, values, ctx));
            }
            (InsertStage::Point, Input::Keyword("Scale")) => self.stage = InsertStage::Scale,
            (InsertStage::Point, Input::Keyword("Rotate")) => self.stage = InsertStage::Rotate,
            (InsertStage::Scale, Input::Value(s)) if s > EPSILON => {
                self.scale = s;
                self.stage = InsertStage::Point;
            }
            (InsertStage::Scale, Input::Value(_)) => return Err("Requires a positive nonzero value.".into()),
            (InsertStage::Rotate, Input::Value(a)) => {
                self.rotation = a;
                self.stage = InsertStage::Point;
            }
            (InsertStage::Scale | InsertStage::Rotate, Input::Enter) => self.stage = InsertStage::Point,
            (InsertStage::Point, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let (InsertStage::Point, Some(block)) = (&self.stage, &self.block) else { return Vec::new() };
        let placement = self.placement(cursor);
        block.entities.iter().map(|be| placement.place(block.base, &be.geometry)).collect()
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, InsertStage::Name | InsertStage::Attributes(..))
    }

    fn text_with_spaces(&self) -> bool {
        matches!(self.stage, InsertStage::Attributes(..))
    }
}

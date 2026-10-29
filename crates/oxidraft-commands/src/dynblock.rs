//! Dynamic block authoring in the block editor: BPARAMETER (Linear, Flip,
//! Rotation), BACTION (a linear parameter's stretch frame), BVSTATE (New,
//! Set, Delete), BVHIDE and BVSHOW; and RESETBLOCK on references.

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::dynamic::{DynamicBlock, DynamicValues, Param, VisibilityState};
use oxidraft_model::{Document, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step};

/// The block open in the block editor, and its base point (block
/// coordinates are drawing coordinates plus it).
fn edited(doc: &Document) -> Option<(String, Point)> {
    let r = doc.refedit().filter(|r| r.editor)?;
    let base = doc.block(&r.block)?.base;
    Some((r.block.clone(), base))
}

const NOT_IN_EDITOR: &str = "** Only in the block editor (BEDIT) **";

fn definition(doc: &Document, block: &str) -> DynamicBlock {
    doc.dynamic_block(block).cloned().unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq)]
enum ParamStage {
    Kind,
    First,
    Second(Point),
    Name(Point, Point),
}

/// BPARAMETER: a parameter added to the block being edited.
pub struct BParameter {
    stage: ParamStage,
    kind: &'static str,
    outside: bool,
}

impl Default for BParameter {
    fn default() -> Self {
        Self { stage: ParamStage::Kind, kind: "Linear", outside: false }
    }
}

impl Command for BParameter {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.outside = edited(doc).is_none();
        self.outside.then(|| NOT_IN_EDITOR.into())
    }

    fn is_finished(&self) -> bool {
        self.outside
    }

    fn prompt(&self) -> Prompt {
        match (self.stage, self.kind) {
            (ParamStage::Kind, _) => Prompt::new("Enter parameter type")
                .with_keywords(&["Linear", "Flip", "Rotation"])
                .with_default("Linear"),
            (ParamStage::First, "Flip") => Prompt::new("Specify base point of reflection line"),
            (ParamStage::First, "Rotation") => Prompt::new("Specify base point"),
            (ParamStage::First, _) => Prompt::new("Specify start point"),
            (ParamStage::Second(_), "Flip") => Prompt::new("Specify endpoint of reflection line"),
            (ParamStage::Second(_), "Rotation") => Prompt::new("Specify radius of parameter"),
            (ParamStage::Second(_), _) => Prompt::new("Specify endpoint"),
            (ParamStage::Name(..), _) => Prompt::new("Enter parameter name"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some((block, base)) = edited(ctx.doc) else { return Err(NOT_IN_EDITOR.into()) };
        match (self.stage, input) {
            (ParamStage::Kind, Input::Keyword(k)) => {
                self.kind = match k {
                    "Flip" => "Flip",
                    "Rotation" => "Rotation",
                    _ => "Linear",
                };
                self.stage = ParamStage::First;
            }
            (ParamStage::Kind, Input::Enter) => self.stage = ParamStage::First,
            (ParamStage::First, Input::Point(p)) => self.stage = ParamStage::Second(p),
            (ParamStage::Second(a), Input::Point(b)) => {
                if a.approx_eq(b) {
                    return Err("The points must differ.".into());
                }
                self.stage = ParamStage::Name(a, b);
            }
            (ParamStage::Name(a, b), input @ (Input::Text(_) | Input::Enter)) => {
                let mut def = definition(ctx.doc, &block);
                let fallback =
                    format!("{}{}", if self.kind == "Linear" { "Distance" } else { self.kind }, def.params.len() + 1);
                let name = match input {
                    Input::Text(t) if !t.trim().is_empty() => t.trim().to_string(),
                    _ => fallback,
                };
                if def.params.iter().any(|p| p.name().eq_ignore_ascii_case(&name)) {
                    return Err(format!("A parameter named \"{name}\" already exists."));
                }
                let (a, b) = (a + base, b + base);
                def.params.push(match self.kind {
                    "Flip" => Param::Flip { name: name.clone(), base: a, end: b },
                    "Rotation" => Param::Rotation { name: name.clone(), base: a, radius: a.distance(b) },
                    _ => Param::Linear { name: name.clone(), base: a, end: b, stretch: None },
                });
                ctx.doc.set_dynamic_block(&block, Some(def));
                if self.kind == "Linear" {
                    ctx.messages.push(format!("BACTION gives \"{name}\" a stretch frame."));
                }
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Point required.".into()),
        }
        Ok(Step::Continue)
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            ParamStage::Second(a) => Some(a),
            _ => None,
        }
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, ParamStage::Name(..))
    }
}

#[derive(Clone, PartialEq)]
enum ActionStage {
    Param,
    First(String),
    Second(String, Point),
}

/// BACTION: a linear parameter's stretch action — what lies inside a
/// frame moves with the parameter's end. Flip and rotation parameters act
/// on the whole block.
pub struct BAction {
    stage: ActionStage,
    outside: bool,
}

impl Default for BAction {
    fn default() -> Self {
        Self { stage: ActionStage::Param, outside: false }
    }
}

impl Command for BAction {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.outside = edited(doc).is_none();
        self.outside.then(|| NOT_IN_EDITOR.into())
    }

    fn is_finished(&self) -> bool {
        self.outside
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            ActionStage::Param => Prompt::new("Enter the name of the parameter to stretch"),
            ActionStage::First(_) => Prompt::new("Specify first corner of stretch frame"),
            ActionStage::Second(..) => Prompt::new("Specify opposite corner"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some((block, base)) = edited(ctx.doc) else { return Err(NOT_IN_EDITOR.into()) };
        match (self.stage.clone(), input) {
            (ActionStage::Param, Input::Text(t)) => {
                let def = definition(ctx.doc, &block);
                match def.params.iter().find(|p| p.name().eq_ignore_ascii_case(t.trim())) {
                    Some(Param::Linear { name, .. }) => self.stage = ActionStage::First(name.clone()),
                    Some(_) => {
                        ctx.messages.push("Flip and rotation parameters act on the whole block.".into());
                        return Ok(Step::Done);
                    }
                    None => return Err(format!("No parameter named \"{}\".", t.trim())),
                }
            }
            (ActionStage::First(name), Input::Point(p)) => self.stage = ActionStage::Second(name, p),
            (ActionStage::Second(name, a), Input::Point(b)) => {
                let mut def = definition(ctx.doc, &block);
                for p in &mut def.params {
                    if let Param::Linear { name: n, stretch, .. } = p
                        && *n == name
                    {
                        *stretch = Some(Aabb::from_points(a + base, b + base));
                    }
                }
                ctx.doc.set_dynamic_block(&block, Some(def));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        self.stage == ActionStage::Param
    }
}

/// BVSTATE: visibility states — New (and current), Set current, Delete.
#[derive(Default)]
pub struct BvState {
    option: Option<&'static str>,
    outside: bool,
}

impl Command for BvState {
    fn is_finished(&self) -> bool {
        self.outside
    }

    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.outside = edited(doc).is_none();
        if self.outside {
            return Some(NOT_IN_EDITOR.into());
        }
        let (block, _) = edited(doc)?;
        let def = definition(doc, &block);
        let names: Vec<&str> = def.states.iter().map(|s| s.name.as_str()).collect();
        let current = def.states.get(def.current_state).map_or("none", |s| s.name.as_str());
        Some(format!(
            "Visibility states: {} (current: {current})",
            if names.is_empty() { "none".into() } else { names.join(", ") }
        ))
    }

    fn prompt(&self) -> Prompt {
        match self.option {
            None => Prompt::new("Enter an option").with_keywords(&["New", "Set", "Delete"]).with_default("New"),
            Some("New") => Prompt::new("Enter name for new visibility state"),
            Some(_) => Prompt::new("Enter visibility state name"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some((block, _)) = edited(ctx.doc) else { return Err(NOT_IN_EDITOR.into()) };
        let mut def = definition(ctx.doc, &block);
        match (self.option, input) {
            (None, Input::Keyword(k)) => {
                self.option = Some(match k {
                    "Set" => "Set",
                    "Delete" => "Delete",
                    _ => "New",
                })
            }
            (None, Input::Enter) => self.option = Some("New"),
            (Some(option), Input::Text(t)) if !t.trim().is_empty() => {
                let name = t.trim().to_string();
                let found = def.states.iter().position(|s| s.name.eq_ignore_ascii_case(&name));
                match (option, found) {
                    ("New", Some(_)) => return Err(format!("\"{name}\" exists already.")),
                    ("New", None) => {
                        // A first state shows everything; a new one starts as the current one.
                        let hidden = def.states.get(def.current_state).map(|s| s.hidden.clone()).unwrap_or_default();
                        def.states.push(VisibilityState { name, hidden });
                        def.current_state = def.states.len() - 1;
                    }
                    ("Set", Some(i)) => def.current_state = i,
                    ("Delete", Some(i)) if def.states.len() > 1 => {
                        def.states.remove(i);
                        def.current_state = def.current_state.min(def.states.len() - 1);
                    }
                    ("Delete", Some(_)) => return Err("The last visibility state can't be deleted.".into()),
                    (_, None) => return Err(format!("No visibility state named \"{name}\".")),
                    _ => {}
                }
                ctx.doc.set_dynamic_block(&block, Some(def));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Enter a name.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        self.option.is_some()
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// BVHIDE and BVSHOW: objects of the block being edited hidden in, or
/// shown again in, the current visibility state.
pub struct BvShow {
    show: bool,
    outside: bool,
}

impl BvShow {
    pub fn new(show: bool) -> Self {
        Self { show, outside: false }
    }
}

impl Command for BvShow {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.outside = edited(doc).is_none();
        self.outside.then(|| NOT_IN_EDITOR.into())
    }

    fn is_finished(&self) -> bool {
        self.outside
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some((block, _)) = edited(ctx.doc) else { return Err(NOT_IN_EDITOR.into()) };
        let Input::Selection(s) = input else { return Err("Select objects, then press Enter.".into()) };
        let order = ctx.doc.block_editor_order();
        let places: Vec<usize> = s.ids.iter().filter_map(|id| order.iter().position(|o| o == id)).collect();
        let mut def = definition(ctx.doc, &block);
        if def.states.is_empty() {
            def.states.push(VisibilityState { name: "VisibilityState0".into(), hidden: Vec::new() });
            def.current_state = 0;
        }
        let current = def.current_state.min(def.states.len() - 1);
        let state = &mut def.states[current];
        for i in places {
            if self.show {
                state.hidden.retain(|h| *h != i);
            } else if !state.hidden.contains(&i) {
                state.hidden.push(i);
            }
        }
        let name = state.name.clone();
        ctx.doc.set_dynamic_block(&block, Some(def));
        ctx.messages.push(format!("In visibility state \"{name}\"."));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

/// RESETBLOCK: references back to their dynamic block's defaults.
#[derive(Default)]
pub struct ResetBlock;

impl Command for ResetBlock {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select block references to reset")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Err("Select objects, then press Enter.".into()) };
        let mut n = 0;
        for id in s.ids {
            let Some(v) = ctx.doc.dynamic_values(id) else { continue };
            if ctx.doc.set_dynamic_values(id, DynamicValues { block: v.block, ..Default::default() }) {
                n += 1;
            }
        }
        ctx.messages.push(format!("{n} block reference(s) reset."));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

/// -DYNPROP (and the Properties palette): a dynamic reference's parameter
/// or visibility state set by name and value.
#[derive(Default)]
pub struct DynProp {
    id: Option<oxidraft_model::EntityId>,
    name: Option<String>,
}

impl Command for DynProp {
    fn prompt(&self) -> Prompt {
        match (&self.id, &self.name) {
            (None, _) => Prompt::new("Select a dynamic block reference"),
            (Some(_), None) => Prompt::new("Enter parameter name, or Visibility"),
            (Some(_), Some(n)) => Prompt::new(format!("Enter value for {n}")),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.id, self.name.clone(), input) {
            (None, _, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                let v = ctx.doc.dynamic_values(id).ok_or("Not a dynamic block reference.")?;
                if let Some(def) = ctx.doc.dynamic_block(&v.block) {
                    let names: Vec<String> = def.params.iter().map(|p| p.name().to_string()).collect();
                    ctx.messages.push(format!("Parameters: {}", names.join(", ")));
                }
                self.id = Some(id);
            }
            (Some(_), None, Input::Text(t)) => self.name = Some(t.trim().to_string()),
            (Some(id), Some(name), input @ (Input::Text(_) | Input::Value(_))) => {
                let mut v = ctx.doc.dynamic_values(id).ok_or("Not a dynamic block reference.")?;
                if name.eq_ignore_ascii_case("Visibility") {
                    let Input::Text(t) = input else { return Err("Enter a state's name.".into()) };
                    v.visibility = Some(t.trim().to_string());
                } else {
                    let value = match input {
                        Input::Value(x) => x,
                        Input::Text(t) => t.trim().parse().map_err(|_| "Requires a number.")?,
                        _ => unreachable!(),
                    };
                    v.values.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
                    v.values.push((name, value));
                }
                ctx.doc.set_dynamic_values(id, v);
                return Ok(Step::Done);
            }
            (_, _, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        self.id.is_none()
    }

    fn wants_text(&self) -> bool {
        self.id.is_some()
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// Whether `g` is a reference to a dynamic block (for the palette).
pub fn is_dynamic_reference(doc: &Document, g: &Geometry) -> bool {
    matches!(g, Geometry::Insert { block, .. } if doc.dynamic_block(block).is_some())
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Block, BlockEntity, Color, Document, Geometry, Placement};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn a_door_authored_then_placed_wider_and_closed() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let line = |a: (f64, f64), b: (f64, f64)| {
            BlockEntity::new(
                "0",
                Color::ByBlock,
                Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) },
            )
        };
        d.define_block(Block {
            name: "DOOR".into(),
            base: Point::ORIGIN,
            entities: vec![line((0.0, 0.0), (900.0, 0.0)), line((0.0, 0.0), (0.0, 900.0))],
        });
        // Outside the block editor it isn't allowed.
        run(&mut e, &mut d, &["bparameter"]);
        assert!(!e.is_active());
        d.begin_block_edit("DOOR").unwrap();
        run(&mut e, &mut d, &["bparameter", "l", "0,0", "900,0", "Width"]);
        run(&mut e, &mut d, &["baction", "Width", "800,-10", "1000,10"]);
        run(&mut e, &mut d, &["bvstate", "n", "Open"]);
        run(&mut e, &mut d, &["bvstate", "n", "Closed"]);
        let swing = d.block_editor_order()[1];
        e.set_selection(vec![swing]);
        run(&mut e, &mut d, &["bvhide"]);
        d.close_refedit(true);
        let def = d.dynamic_block("DOOR").expect("dynamic");
        assert_eq!(def.params.len(), 1);
        assert_eq!(def.states[1].hidden, [1]);
        // A reference, 1000 wide and closed.
        let id = d.add(Geometry::Insert {
            block: "DOOR".into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: vec![],
            clip: None,
        });
        e.click(Point::new(450.0, 0.0), Point::new(450.0, 0.0), false, &mut d, 0.5);
        e.set_selection(Vec::new());
        run(&mut e, &mut d, &["-dynprop"]);
        e.click(Point::new(450.0, 0.0), Point::new(450.0, 0.0), false, &mut d, 0.5);
        run(&mut e, &mut d, &["Width", "1000"]);
        run(&mut e, &mut d, &["-dynprop"]);
        e.click(Point::new(450.0, 0.0), Point::new(450.0, 0.0), false, &mut d, 0.5);
        run(&mut e, &mut d, &["Visibility", "Closed"]);
        let drawn = d.expanded(d.entity(id).unwrap());
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        assert!(matches!(drawn[0], Geometry::Line { end, .. } if end.approx_eq(Point::new(1000.0, 0.0))));
        // RESETBLOCK: back to 900, open.
        e.set_selection(vec![id]);
        run(&mut e, &mut d, &["resetblock"]);
        assert_eq!(d.expanded(d.entity(id).unwrap()).len(), 2);
    }
}

/// BSAVEAS: the block being edited saved under another name (its dynamic
/// definition too); the editor goes on with the original.
#[derive(Default)]
pub struct BSaveAs {
    outside: bool,
}

impl Command for BSaveAs {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.outside = edited(doc).is_none();
        self.outside.then(|| NOT_IN_EDITOR.into())
    }

    fn is_finished(&self) -> bool {
        self.outside
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Enter new block name")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some((block, base)) = edited(ctx.doc) else { return Err(NOT_IN_EDITOR.into()) };
        let Input::Text(t) = input else { return Ok(Step::Done) };
        let name = t.trim().to_string();
        oxidraft_model::check_name(&name)?;
        if ctx.doc.block(&name).is_some() {
            return Err(format!("A block named \"{name}\" already exists."));
        }
        let entities = ctx.doc.refedit_contents();
        ctx.doc.define_block(oxidraft_model::Block { name: name.clone(), base, entities });
        if let Some(def) = ctx.doc.dynamic_block(&block).cloned() {
            ctx.doc.set_dynamic_block(&name, Some(def));
        }
        ctx.messages.push(format!("Saved as block \"{name}\"."));
        Ok(Step::Done)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

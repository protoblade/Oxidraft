//! Dynamic block authoring in the block editor: BPARAMETER (Linear, Flip,
//! Rotation, Point, Polar, XY, Alignment, Lookup), BACTION (Stretch, Move,
//! Array), BLOOKUPTABLE, BVSTATE (New, Set, Delete), BVHIDE and BVSHOW;
//! and RESETBLOCK on references.

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
                .with_keywords(&["Linear", "Flip", "Rotation", "Point", "Polar", "XY", "Alignment", "lOokup"])
                .with_default("Linear"),
            (ParamStage::First, "Flip") => Prompt::new("Specify base point of reflection line"),
            (ParamStage::First, "Rotation" | "XY" | "Alignment") => Prompt::new("Specify base point"),
            (ParamStage::First, "Point" | "Lookup") => Prompt::new("Specify parameter location"),
            (ParamStage::First, _) => Prompt::new("Specify start point"),
            (ParamStage::Second(_), "Flip") => Prompt::new("Specify endpoint of reflection line"),
            (ParamStage::Second(_), "Rotation") => Prompt::new("Specify radius of parameter"),
            (ParamStage::Second(_), "XY") => Prompt::new("Specify opposite corner"),
            (ParamStage::Second(_), "Alignment") => Prompt::new("Specify alignment direction"),
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
                    "Point" => "Point",
                    "Polar" => "Polar",
                    "XY" => "XY",
                    "Alignment" => "Alignment",
                    "lOokup" => "Lookup",
                    _ => "Linear",
                };
                self.stage = ParamStage::First;
            }
            (ParamStage::Kind, Input::Enter) => self.stage = ParamStage::First,
            // One point is enough for these.
            (ParamStage::First, Input::Point(p)) if matches!(self.kind, "Point" | "Lookup") => {
                self.stage = ParamStage::Name(p, p);
            }
            (ParamStage::First, Input::Point(p)) => self.stage = ParamStage::Second(p),
            (ParamStage::Second(a), Input::Point(b)) => {
                if a.approx_eq(b) {
                    return Err("The points must differ.".into());
                }
                self.stage = ParamStage::Name(a, b);
            }
            (ParamStage::Name(a, b), input @ (Input::Text(_) | Input::Enter)) => {
                let mut def = definition(ctx.doc, &block);
                let word = match self.kind {
                    "Linear" | "Polar" => "Distance",
                    "Point" => "Position",
                    "Rotation" => "Angle",
                    k => k,
                };
                let fallback = format!("{word}{}", def.params.len() + 1);
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
                    "Point" => Param::Point { name: name.clone(), base: a, objects: Vec::new() },
                    "Polar" => Param::Polar { name: name.clone(), base: a, end: b, stretch: None },
                    "XY" => Param::Xy { name: name.clone(), base: a, end: b, stretch: None },
                    "Alignment" => Param::Alignment { name: name.clone(), base: a, end: b },
                    "Lookup" => Param::Lookup { name: name.clone(), base: a, rows: Vec::new() },
                    _ => Param::Linear { name: name.clone(), base: a, end: b, stretch: None },
                });
                ctx.doc.set_dynamic_block(&block, Some(def));
                let hint = match self.kind {
                    "Linear" | "Polar" | "XY" => format!("BACTION gives \"{name}\" a stretch frame or an array."),
                    "Point" => format!("BACTION gives \"{name}\" the objects it moves."),
                    "Lookup" => format!("BLOOKUPTABLE gives \"{name}\" its rows."),
                    _ => String::new(),
                };
                if !hint.is_empty() {
                    ctx.messages.push(hint);
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
    /// Stretch or Array, for a linear, polar or XY parameter.
    Kind(String),
    First(String),
    Second(String, Point),
    /// The objects a point's move or an array takes.
    Objects(String, bool),
    Spacing(String, Vec<usize>),
}

/// BACTION: what a parameter does. A linear, polar or XY parameter's
/// stretch (what lies inside a frame moves with its end) or array (the
/// objects repeat along its distance), a point's move (the objects go with
/// it). Flip, rotation and alignment parameters act on the whole block.
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
        match &self.stage {
            ActionStage::Param => Prompt::new("Enter the name of the parameter"),
            ActionStage::Kind(_) => {
                Prompt::new("Enter action type").with_keywords(&["Stretch", "Array"]).with_default("Stretch")
            }
            ActionStage::First(_) => Prompt::new("Specify first corner of stretch frame"),
            ActionStage::Second(..) => Prompt::new("Specify opposite corner"),
            ActionStage::Objects(..) => Prompt::new("Select objects"),
            ActionStage::Spacing(..) => Prompt::new("Enter the distance between items"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some((block, base)) = edited(ctx.doc) else { return Err(NOT_IN_EDITOR.into()) };
        match (self.stage.clone(), input) {
            (ActionStage::Param, Input::Text(t)) => {
                let def = definition(ctx.doc, &block);
                match def.params.iter().find(|p| p.name().eq_ignore_ascii_case(t.trim())) {
                    Some(Param::Linear { name, .. } | Param::Polar { name, .. } | Param::Xy { name, .. }) => {
                        self.stage = ActionStage::Kind(name.clone())
                    }
                    Some(Param::Point { name, .. }) => self.stage = ActionStage::Objects(name.clone(), false),
                    Some(Param::Lookup { .. }) => {
                        ctx.messages.push("A lookup's table is BLOOKUPTABLE's.".into());
                        return Ok(Step::Done);
                    }
                    Some(_) => {
                        ctx.messages.push("Flip, rotation and alignment parameters act on the whole block.".into());
                        return Ok(Step::Done);
                    }
                    None => return Err(format!("No parameter named \"{}\".", t.trim())),
                }
            }
            (ActionStage::Kind(name), Input::Keyword("Array")) => self.stage = ActionStage::Objects(name, true),
            // A point: Stretch, the default, its frame's first corner.
            (ActionStage::Kind(name), Input::Point(p)) => self.stage = ActionStage::Second(name, p),
            (ActionStage::Kind(name), Input::Keyword(_) | Input::Enter) => self.stage = ActionStage::First(name),
            (ActionStage::First(name), Input::Point(p)) => self.stage = ActionStage::Second(name, p),
            (ActionStage::Second(name, a), Input::Point(b)) => {
                let mut def = definition(ctx.doc, &block);
                let frame = Aabb::from_points(a + base, b + base);
                for p in &mut def.params {
                    match p {
                        Param::Linear { name: n, stretch, .. }
                        | Param::Polar { name: n, stretch, .. }
                        | Param::Xy { name: n, stretch, .. }
                            if *n == name =>
                        {
                            *stretch = Some(frame);
                        }
                        _ => {}
                    }
                }
                ctx.doc.set_dynamic_block(&block, Some(def));
                return Ok(Step::Done);
            }
            (ActionStage::Objects(name, array), Input::Selection(s)) => {
                let order = ctx.doc.block_editor_order();
                let places: Vec<usize> = s.ids.iter().filter_map(|id| order.iter().position(|o| o == id)).collect();
                if places.is_empty() {
                    return Ok(Step::Done);
                }
                if array {
                    self.stage = ActionStage::Spacing(name, places);
                    return Ok(Step::Continue);
                }
                let mut def = definition(ctx.doc, &block);
                for p in &mut def.params {
                    if let Param::Point { name: n, objects, .. } = p
                        && *n == name
                    {
                        *objects = places.clone();
                    }
                }
                ctx.doc.set_dynamic_block(&block, Some(def));
                return Ok(Step::Done);
            }
            (ActionStage::Spacing(name, objects), Input::Value(d)) if d > 0.0 => {
                let mut def = definition(ctx.doc, &block);
                let action = format!("Array{}", def.params.len() + 1);
                def.params.push(Param::Array { name: action, follows: name, objects, spacing: d });
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

    fn wants_selection(&self) -> bool {
        matches!(self.stage, ActionStage::Objects(..))
    }
}

/// BLOOKUPTABLE: a lookup parameter's rows — each a label and the values
/// it gives other parameters' properties (`Length=60, Width=30`).
#[derive(Default)]
pub struct BLookupTable {
    lookup: Option<String>,
    label: Option<String>,
    outside: bool,
}

impl Command for BLookupTable {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.outside = edited(doc).is_none();
        self.outside.then(|| NOT_IN_EDITOR.into())
    }

    fn is_finished(&self) -> bool {
        self.outside
    }

    fn prompt(&self) -> Prompt {
        match (&self.lookup, &self.label) {
            (None, _) => Prompt::new("Enter the name of the lookup parameter"),
            (Some(_), None) => Prompt::new("Enter a row's label (Enter when done)"),
            (Some(_), Some(l)) => Prompt::new(format!("Enter {l}'s values as property=value, ...")),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some((block, _)) = edited(ctx.doc) else { return Err(NOT_IN_EDITOR.into()) };
        let text = match input {
            Input::Text(t) if !t.trim().is_empty() => t.trim().to_string(),
            _ => return Ok(Step::Done),
        };
        let mut def = definition(ctx.doc, &block);
        match (self.lookup.clone(), self.label.take()) {
            (None, _) => {
                let found = def
                    .params
                    .iter()
                    .find(|p| matches!(p, Param::Lookup { .. }) && p.name().eq_ignore_ascii_case(&text));
                self.lookup = Some(found.ok_or(format!("No lookup parameter named \"{text}\"."))?.name().to_string());
            }
            (Some(_), None) => self.label = Some(text),
            (Some(lookup), Some(label)) => {
                let mut values = Vec::new();
                for pair in text.split(',') {
                    let (k, v) = pair.split_once('=').ok_or("Enter property=value pairs.")?;
                    let (k, v) =
                        (k.trim(), v.trim().parse::<f64>().map_err(|_| format!("\"{}\" isn't a number.", v.trim()))?);
                    let known = def.properties().iter().any(|(p, _)| p.eq_ignore_ascii_case(k));
                    if !known {
                        return Err(format!("No property named \"{k}\"."));
                    }
                    values.push((k.to_string(), v));
                }
                for p in &mut def.params {
                    if let Param::Lookup { name, rows, .. } = p
                        && *name == lookup
                    {
                        rows.retain(|(l, _)| !l.eq_ignore_ascii_case(&label));
                        rows.push((label.clone(), values.clone()));
                    }
                }
                ctx.doc.set_dynamic_block(&block, Some(def));
            }
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_with_spaces(&self) -> bool {
        true
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
                    let names: Vec<String> = def.properties().into_iter().map(|(k, _)| k).collect();
                    ctx.messages.push(format!("Properties: {}", names.join(", ")));
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
                    // A lookup takes its row's label.
                    let row = match (&input, ctx.doc.dynamic_block(&v.block).and_then(|d| d.param_of(&name))) {
                        (Input::Text(t), Some(Param::Lookup { rows, .. })) => {
                            rows.iter().position(|(l, _)| l.eq_ignore_ascii_case(t.trim())).map(|i| i as f64)
                        }
                        _ => None,
                    };
                    let value = match (row, input) {
                        (Some(r), _) => r,
                        (None, Input::Value(x)) => x,
                        (None, Input::Text(t)) => t.trim().parse().map_err(|_| "Requires a number.")?,
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

/// Dragging a dynamic reference's parameter grip: its properties set from
/// where it's dropped. An alignment grip dropped on an object lines the
/// block up with it, there.
pub struct DynGripEdit {
    id: oxidraft_model::EntityId,
    grip: oxidraft_model::dynamic::DynGrip,
    at: Point,
}

impl DynGripEdit {
    pub fn new(id: oxidraft_model::EntityId, grip: oxidraft_model::dynamic::DynGrip, at: Point) -> Self {
        Self { id, grip, at }
    }
}

impl Command for DynGripEdit {
    fn prompt(&self) -> Prompt {
        Prompt::new("** PARAMETER ** Specify new location").with_keywords(&["eXit"])
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let to = match input {
            Input::Point(p) => p,
            Input::Pick { at, .. } => at,
            _ => return Ok(Step::Done),
        };
        // What an alignment grip is dropped on, and its direction there.
        let along = ctx.pick(to).filter(|o| *o != self.id).and_then(|o| {
            let segs = ctx.doc.entity(o)?.geometry.segments();
            let s = segs.iter().min_by(|a, b| a.distance_to(to).total_cmp(&b.distance_to(to)))?;
            Some(Point::ORIGIN.angle_to(s.tangent_at(s.param_of(to).clamp(0.0, 1.0))))
        });
        let values =
            ctx.doc.dynamic_grip_values(self.id, &self.grip, to, along).ok_or("Not a dynamic block reference.")?;
        let aligned = along.is_some()
            && matches!(&self.grip, oxidraft_model::dynamic::DynGrip::Param(i)
                if ctx.doc.dynamic_block(&values.block).and_then(|d| d.params.get(*i)).is_some_and(|p| matches!(p, Param::Alignment { .. })));
        ctx.doc.set_dynamic_values(self.id, values);
        if aligned {
            let shift = to - self.at;
            ctx.doc.update(self.id, |e| e.geometry = e.geometry.translated(shift));
        }
        Ok(Step::Done)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        vec![Geometry::Line { start: self.at, end: cursor }]
    }

    fn base_point(&self) -> Option<Point> {
        Some(self.at)
    }

    fn highlighted(&self) -> Vec<oxidraft_model::EntityId> {
        vec![self.id]
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

    #[test]
    fn points_lookups_arrays_and_grips() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let line = |a: (f64, f64), b: (f64, f64)| {
            BlockEntity::new(
                "0",
                Color::ByBlock,
                Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) },
            )
        };
        d.define_block(Block {
            name: "BENCH".into(),
            base: Point::ORIGIN,
            entities: vec![line((0.0, 0.0), (100.0, 0.0)), line((0.0, 10.0), (5.0, 10.0))],
        });
        d.begin_block_edit("BENCH").unwrap();
        let order = d.block_editor_order();
        // A point moving the small mark; a linear length with the mark arrayed along it.
        run(&mut e, &mut d, &["bparameter", "point", "0,10", "Mark"]);
        run(&mut e, &mut d, &["baction", "Mark"]);
        e.click(Point::new(2.5, 10.0), Point::new(2.5, 10.0), false, &mut d, 0.5);
        run(&mut e, &mut d, &[""]);
        let _ = order;
        run(&mut e, &mut d, &["bparameter", "l", "0,0", "100,0", "Length"]);
        run(&mut e, &mut d, &["baction", "Length", "stretch", "90,-5", "110,5"]);
        run(&mut e, &mut d, &["bparameter", "lookup", "0,-10", "Size"]);
        run(&mut e, &mut d, &["blookuptable", "Size", "Short", "Length=50", "Long", "Length=150", ""]);
        d.close_refedit(true);
        let def = d.dynamic_block("BENCH").unwrap().clone();
        assert!(
            matches!(&def.params[0], oxidraft_model::dynamic::Param::Point { objects, .. } if objects == &[1]),
            "{def:?}"
        );
        assert!(matches!(&def.params[2], oxidraft_model::dynamic::Param::Lookup { rows, .. } if rows.len() == 2));
        let id = d.add(Geometry::Insert {
            block: "BENCH".into(),
            placement: Placement::at(Point::new(1000.0, 0.0)),
            attributes: vec![],
            clip: None,
        });
        let ends = |d: &Document| {
            let mut x: Vec<i64> = d
                .expanded(d.entity(id).unwrap())
                .iter()
                .filter_map(|g| if let Geometry::Line { end, .. } = g { Some(end.x.round() as i64) } else { None })
                .collect();
            x.sort();
            x
        };
        assert_eq!(ends(&d), [1005, 1100]);
        // The length's grip dragged to 130.
        e.set_selection(vec![id]);
        let grips = e.grips(&d);
        assert!(grips.contains(&Point::new(1100.0, 0.0)), "{grips:?}");
        e.click(Point::new(1100.0, 0.0), Point::new(1100.0, 0.0), false, &mut d, 0.5);
        assert_eq!(e.active_name(), Some("-DYNPROP"));
        e.click(Point::new(1130.0, 7.0), Point::new(1130.0, 7.0), false, &mut d, 0.5);
        assert_eq!(ends(&d), [1005, 1130]);
        // The point's grip: the mark moves.
        e.set_selection(vec![id]);
        e.click(Point::new(1000.0, 10.0), Point::new(1000.0, 10.0), false, &mut d, 0.5);
        e.click(Point::new(1020.0, 30.0), Point::new(1020.0, 30.0), false, &mut d, 0.5);
        assert_eq!(ends(&d), [1025, 1130]);
        // The lookup's grip: its first row, Short.
        e.set_selection(vec![id]);
        e.click(Point::new(1000.0, -10.0), Point::new(1000.0, -10.0), false, &mut d, 0.5);
        assert_eq!(ends(&d), [1025, 1050]);
        // -DYNPROP takes a lookup's label.
        e.set_selection(Vec::new());
        run(&mut e, &mut d, &["-dynprop"]);
        e.click(Point::new(1040.0, 0.0), Point::new(1040.0, 0.0), false, &mut d, 0.5);
        run(&mut e, &mut d, &["Size", "Long"]);
        assert_eq!(ends(&d), [1025, 1150]);
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

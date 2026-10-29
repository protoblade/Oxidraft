use oxidraft_geom::Point;
use oxidraft_model::{Layer, snap};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

/// LAYMCUR: makes the layer of a selected object the current layer. An
/// object selected beforehand is used right away.
#[derive(Default)]
pub struct LayMcur;

impl Command for LayMcur {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select object whose layer will become current")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Selection(s) => {
                let Some(layer) = s.ids.first().and_then(|&id| ctx.doc.entity(id)).map(|e| e.layer.clone()) else {
                    return Ok(Step::Done);
                };
                ctx.doc.set_current_layer(&layer);
                ctx.messages.push(format!("{layer} is now the current layer."));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select an object.".into()),
        }
    }

    fn wants_selection(&self) -> bool {
        true
    }

    fn single_selection(&self) -> bool {
        true
    }
}

/// What LAYOFF, LAYFRZ, LAYLCK and LAYULK do to the layer of the object you pick.
#[derive(Clone, Copy, PartialEq)]
enum Action {
    Off,
    Freeze,
    Lock,
    Unlock,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Action::Off => "turned off",
            Action::Freeze => "frozen",
            Action::Lock => "locked",
            Action::Unlock => "unlocked",
        }
    }

    fn apply(self, layer: &Layer) -> Layer {
        let mut l = layer.clone();
        match self {
            Action::Off => l.on = false,
            Action::Freeze => l.frozen = true,
            Action::Lock => l.locked = true,
            Action::Unlock => l.locked = false,
        }
        l
    }
}

/// LAYOFF and LAYFRZ (pick objects until Enter; Undo puts back the last
/// layer), LAYLCK and LAYULK (one pick).
pub struct LayerPick {
    action: Action,
    /// Layers as they were before each change, for Undo.
    changed: Vec<Layer>,
    /// The current layer was picked for LAYOFF: asking whether to go on.
    confirm: Option<Layer>,
}

impl LayerPick {
    fn new(action: Action) -> Self {
        Self { action, changed: Vec::new(), confirm: None }
    }

    pub fn off() -> Self {
        Self::new(Action::Off)
    }

    pub fn freeze() -> Self {
        Self::new(Action::Freeze)
    }

    pub fn lock() -> Self {
        Self::new(Action::Lock)
    }

    pub fn unlock() -> Self {
        Self::new(Action::Unlock)
    }

    fn repeats(&self) -> bool {
        matches!(self.action, Action::Off | Action::Freeze)
    }

    fn change(&mut self, ctx: &mut Ctx, layer: Layer) -> Result<Step, String> {
        ctx.doc.update_layer(self.action.apply(&layer));
        ctx.messages.push(format!("Layer \"{}\" has been {}.", layer.name, self.action.verb()));
        self.changed.push(layer);
        Ok(if self.repeats() { Step::Continue } else { Step::Done })
    }

    fn picked(&mut self, ctx: &mut Ctx, at: Point) -> Result<Step, String> {
        let id = if self.action == Action::Unlock {
            snap::pick_locked(ctx.doc, at, ctx.pick_tolerance)
        } else {
            ctx.pick(at)
        };
        let layer = id.and_then(|id| ctx.doc.entity(id)).and_then(|e| ctx.doc.layer(&e.layer)).cloned();
        let Some(layer) = layer else { return Err("No object found.".into()) };
        let current = layer.name.eq_ignore_ascii_case(ctx.doc.current_layer());
        match self.action {
            Action::Freeze if current => {
                Err(format!("Cannot freeze layer \"{}\". It is the current layer.", layer.name))
            }
            Action::Off if current => {
                self.confirm = Some(layer);
                Ok(Step::Continue)
            }
            _ => self.change(ctx, layer),
        }
    }
}

impl Command for LayerPick {
    fn prompt(&self) -> Prompt {
        if let Some(layer) = &self.confirm {
            return Prompt::new(format!("Layer \"{}\" is the current layer, do you want to turn it off?", layer.name))
                .with_keywords(&["Yes", "No"])
                .with_default("No");
        }
        let text = format!("Select an object on the layer to be {}", self.action.verb());
        if self.repeats() { Prompt::new(text).with_keywords(&["Undo"]) } else { Prompt::new(text) }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if let Some(layer) = self.confirm.take() {
            return match input {
                Input::Keyword("Yes") => self.change(ctx, layer),
                _ => Ok(Step::Continue),
            };
        }
        match input {
            Input::Pick { at, .. } => self.picked(ctx, at),
            Input::Keyword("Undo") => {
                let Some(before) = self.changed.pop() else { return Err("Nothing to undo.".into()) };
                ctx.doc.update_layer(before);
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select an object.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        self.confirm.is_none()
    }
}

const ISO_SETTINGS: &[&str] = &["Off", "Lock and fade"];

/// LAYISO: keeps the layers of the selected objects and turns off, or locks
/// and fades, every other layer. LAYUNISO puts them back.
#[derive(Default)]
pub struct LayIso {
    settings: bool,
}

impl Command for LayIso {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        Some(format!("Current setting: {}", if vars.layiso_off { "Hide layers" } else { "Lock and fade layers" }))
    }

    fn prompt(&self) -> Prompt {
        if self.settings {
            Prompt::new("Enter setting for layers not isolated")
                .with_keywords(ISO_SETTINGS)
                .with_default("Lock and fade")
        } else {
            Prompt::new("Select objects on the layer(s) to be isolated").with_keywords(&["Settings"])
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Keyword("Settings") => self.settings = true,
            Input::Keyword("Off") if self.settings => (ctx.vars.layiso_off, self.settings) = (true, false),
            Input::Keyword(_) | Input::Enter if self.settings => (ctx.vars.layiso_off, self.settings) = (false, false),
            Input::Selection(s) => {
                let mut names: Vec<String> = Vec::new();
                for e in s.ids.iter().filter_map(|&id| ctx.doc.entity(id)) {
                    if !names.iter().any(|n| n.eq_ignore_ascii_case(&e.layer)) {
                        names.push(e.layer.clone());
                    }
                }
                if names.is_empty() {
                    return Ok(Step::Done);
                }
                ctx.vars.layiso_saved = Some((ctx.doc.layers().to_vec(), ctx.doc.current_layer().to_string()));
                for layer in ctx.doc.layers().to_vec() {
                    let isolated = names.iter().any(|n| n.eq_ignore_ascii_case(&layer.name));
                    let changed = match (isolated, ctx.vars.layiso_off) {
                        (true, _) => Layer { on: true, locked: false, ..layer.clone() },
                        (false, true) => Layer { on: false, ..layer.clone() },
                        (false, false) => Layer { locked: true, ..layer.clone() },
                    };
                    if changed != layer {
                        ctx.doc.update_layer(changed);
                    }
                }
                if !names.iter().any(|n| n.eq_ignore_ascii_case(ctx.doc.current_layer())) {
                    ctx.doc.set_current_layer(&names[0]);
                }
                ctx.messages.push(match names.as_slice() {
                    [one] => format!("Layer \"{one}\" has been isolated."),
                    _ => format!("{} layers have been isolated.", names.len()),
                });
                return Ok(Step::Done);
            }
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Select objects, then press Enter.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        !self.settings
    }
}

/// LAYUNISO: the layers LAYISO changed, as they were. Returns the message.
pub(crate) fn unisolate(doc: &mut oxidraft_model::Document, vars: &mut Vars) -> String {
    let Some((layers, current)) = vars.layiso_saved.take() else {
        return "No layers have been isolated with LAYISO.".into();
    };
    for saved in layers {
        if let Some(now) = doc.layer(&saved.name).cloned() {
            let restored = Layer { on: saved.on, locked: saved.locked, ..now.clone() };
            if restored != now {
                doc.update_layer(restored);
            }
        }
    }
    doc.set_current_layer(&current);
    "Layers isolated by LAYISO have been restored.".into()
}

/// LAYON and LAYTHW: every layer on, or every layer thawed. Returns the message.
pub(crate) fn all_layers(doc: &mut oxidraft_model::Document, thaw: bool) -> String {
    for layer in doc.layers().to_vec() {
        let changed = if thaw { Layer { frozen: false, ..layer.clone() } } else { Layer { on: true, ..layer.clone() } };
        if changed != layer {
            doc.update_layer(changed);
        }
    }
    if thaw { "All layers have been thawed.".into() } else { "All layers have been turned on.".into() }
}

//! More layer tools: LAYCUR, LAYMCH, COPYTOLAYER, LAYMRG and LAYDEL.

use oxidraft_model::EntityId;

use crate::{Command, Ctx, Input, Prompt, Step};

/// The layer of the object picked at `at`, or the one named.
fn layer_from(ctx: &Ctx, input: &Input) -> Result<String, String> {
    match input {
        Input::Pick { at, .. } => ctx
            .pick(*at)
            .and_then(|id| ctx.doc.entity(id))
            .map(|e| e.layer.clone())
            .ok_or_else(|| "No object found.".to_string()),
        Input::Text(name) => ctx
            .doc
            .layer(name.trim())
            .map(|l| l.name.clone())
            .ok_or_else(|| format!("Layer \"{}\" not found.", name.trim())),
        _ => Err("Pick an object on the layer, or enter its name.".into()),
    }
}

/// LAYCUR: the selected objects onto the current layer.
#[derive(Default)]
pub struct LayCur;

impl Command for LayCur {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects to be changed to the current layer")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Err("Select objects, then press Enter.".into()) };
        let current = ctx.doc.current_layer().to_string();
        for id in &s.ids {
            ctx.doc.update(*id, |e| e.layer = current.clone());
        }
        ctx.messages.push(format!("{} object(s) changed to layer \"{current}\" (the current layer).", s.ids.len()));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

/// LAYMCH and COPYTOLAYER: objects selected, then the layer they go onto
/// (moved, or copies of them).
pub struct ToLayer {
    copy: bool,
    ids: Option<Vec<EntityId>>,
    naming: bool,
}

impl ToLayer {
    pub fn matching() -> Self {
        Self { copy: false, ids: None, naming: false }
    }

    pub fn copying() -> Self {
        Self { copy: true, ids: None, naming: false }
    }
}

impl Command for ToLayer {
    fn prompt(&self) -> Prompt {
        match (&self.ids, self.naming) {
            (None, _) if self.copy => Prompt::new("Select objects to copy"),
            (None, _) => Prompt::new("Select objects to be changed"),
            (Some(_), true) => Prompt::new("Enter layer name"),
            (Some(_), false) => Prompt::new("Select object on destination layer").with_keywords(&["Name"]),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.ids, input) {
            (None, Input::Selection(s)) => {
                if s.ids.is_empty() {
                    return Ok(Step::Done);
                }
                self.ids = Some(s.ids);
                Ok(Step::Continue)
            }
            (Some(_), Input::Keyword(_)) => {
                self.naming = true;
                Ok(Step::Continue)
            }
            (Some(ids), input @ (Input::Pick { .. } | Input::Text(_))) => {
                let layer = layer_from(ctx, &input)?;
                let ids = ids.clone();
                for id in &ids {
                    if self.copy {
                        if let Some(e) = ctx.doc.entity(*id).cloned() {
                            let copy = ctx.doc.add_like(e.geometry.clone(), &e);
                            ctx.doc.update(copy, |c| c.layer = layer.clone());
                        }
                    } else {
                        ctx.doc.update(*id, |e| e.layer = layer.clone());
                    }
                }
                let verb = if self.copy { "copied to" } else { "changed to" };
                ctx.messages.push(format!("{} object(s) {verb} layer \"{layer}\".", ids.len()));
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err("Select objects, then the destination layer.".into()),
        }
    }

    fn wants_selection(&self) -> bool {
        self.ids.is_none()
    }

    fn wants_pick(&self) -> bool {
        self.ids.is_some() && !self.naming
    }

    fn wants_text(&self) -> bool {
        self.naming
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

#[derive(Clone, PartialEq)]
enum MergeStage {
    From,
    FromName,
    Into,
    IntoName,
    Confirm(String),
}

/// LAYMRG: layers picked (or named) merged into a target layer.
pub struct LayMrg {
    stage: MergeStage,
    from: Vec<String>,
}

impl Default for LayMrg {
    fn default() -> Self {
        Self { stage: MergeStage::From, from: Vec::new() }
    }
}

impl Command for LayMrg {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            MergeStage::From => Prompt::new("Select object on layer to merge").with_keywords(&["Name"]),
            MergeStage::FromName | MergeStage::IntoName => Prompt::new("Enter layer name"),
            MergeStage::Into => Prompt::new("Select object on target layer").with_keywords(&["Name"]),
            MergeStage::Confirm(into) => Prompt::new(format!(
                "******** WARNING ********\\nYou are about to merge {} layer(s) into layer \"{into}\".\\nDo you wish to continue?",
                self.from.len()
            ))
            .with_keywords(&["Yes", "No"])
            .with_default("No"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (MergeStage::From, Input::Keyword(_)) => self.stage = MergeStage::FromName,
            (MergeStage::Into, Input::Keyword(_)) => self.stage = MergeStage::IntoName,
            (MergeStage::From | MergeStage::FromName, input @ (Input::Pick { .. } | Input::Text(_))) => {
                let layer = layer_from(ctx, &input)?;
                if !self.from.contains(&layer) {
                    ctx.messages.push(format!(
                        "Selected layers: {}",
                        self.from.iter().chain([&layer]).cloned().collect::<Vec<_>>().join(", ")
                    ));
                    self.from.push(layer);
                }
                self.stage = MergeStage::From;
            }
            (MergeStage::From | MergeStage::FromName, Input::Enter) if !self.from.is_empty() => {
                self.stage = MergeStage::Into
            }
            (MergeStage::Into | MergeStage::IntoName, input @ (Input::Pick { .. } | Input::Text(_))) => {
                self.stage = MergeStage::Confirm(layer_from(ctx, &input)?);
            }
            (MergeStage::Confirm(into), Input::Keyword("Yes")) => {
                let moved = ctx.doc.merge_layers(&self.from, &into)?;
                ctx.messages.push(format!("{moved} object(s) merged into layer \"{into}\"."));
                return Ok(Step::Done);
            }
            (_, Input::Enter | Input::Keyword(_)) => return Ok(Step::Done),
            _ => return Err("Pick an object on a layer, or enter its name.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, MergeStage::From | MergeStage::Into)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, MergeStage::FromName | MergeStage::IntoName)
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// LAYDEL: a layer and everything on it deleted, after asking.
#[derive(Default)]
pub struct LayDel {
    naming: bool,
    layer: Option<String>,
}

impl Command for LayDel {
    fn prompt(&self) -> Prompt {
        match (&self.layer, self.naming) {
            (Some(l), _) => Prompt::new(format!(
                "******** WARNING ********\\nYou are about to delete layer \"{l}\" and everything on it.\\nDo you wish to continue?"
            ))
            .with_keywords(&["Yes", "No"])
            .with_default("No"),
            (None, true) => Prompt::new("Enter layer name"),
            (None, false) => Prompt::new("Select object on layer to delete").with_keywords(&["Name"]),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.layer, input) {
            (None, Input::Keyword(_)) => self.naming = true,
            (None, input @ (Input::Pick { .. } | Input::Text(_))) => self.layer = Some(layer_from(ctx, &input)?),
            (Some(layer), Input::Keyword("Yes")) => {
                let layer = layer.clone();
                let n = ctx.doc.delete_layer_and_objects(&layer)?;
                ctx.messages.push(format!("Deleting layer \"{layer}\": {n} object(s) deleted."));
                return Ok(Step::Done);
            }
            _ => return Ok(Step::Done),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        self.layer.is_none() && !self.naming
    }

    fn wants_text(&self) -> bool {
        self.layer.is_none() && self.naming
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Color, Document, Geometry, Layer};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    fn click(e: &mut Engine, d: &mut Document, x: f64, y: f64) {
        let p = Point::new(x, y);
        e.click(p, p, false, d, 0.5);
    }

    fn doc() -> Document {
        let mut d = Document::new();
        for (name, c) in [("Walls", 1), ("Doors", 2), ("Old", 3)] {
            d.add_layer(Layer::new(name, c));
        }
        d.add_on_layer(
            Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) },
            "Walls",
            Color::ByLayer,
        );
        d.add_on_layer(
            Geometry::Line { start: Point::new(0.0, 5.0), end: Point::new(10.0, 5.0) },
            "Doors",
            Color::ByLayer,
        );
        d.add_on_layer(
            Geometry::Line { start: Point::new(0.0, 9.0), end: Point::new(10.0, 9.0) },
            "Old",
            Color::ByLayer,
        );
        d
    }

    fn layers_of(d: &Document) -> Vec<String> {
        d.entities().map(|e| e.layer.clone()).collect()
    }

    #[test]
    fn layer_tools_move_copy_merge_and_delete() {
        let (mut e, mut d) = (Engine::new(), doc());
        // LAYMCH: the Doors line onto Walls.
        run(&mut e, &mut d, &["laymch"]);
        click(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &[""]);
        click(&mut e, &mut d, 5.0, 0.0);
        assert_eq!(layers_of(&d), ["Walls", "Walls", "Old"]);
        // COPYTOLAYER by name.
        run(&mut e, &mut d, &["copytolayer"]);
        click(&mut e, &mut d, 5.0, 9.0);
        run(&mut e, &mut d, &["", "n", "Doors"]);
        assert_eq!(layers_of(&d), ["Walls", "Walls", "Old", "Doors"]);
        // LAYMRG: Old into Walls; Old goes.
        run(&mut e, &mut d, &["laymrg"]);
        click(&mut e, &mut d, 5.0, 9.0);
        run(&mut e, &mut d, &["", "n", "Walls", "y"]);
        assert!(d.layer("Old").is_none());
        assert_eq!(d.entities().filter(|x| x.layer == "Walls").count(), 3);
        // LAYDEL: Doors and its line.
        run(&mut e, &mut d, &["laydel", "n", "Doors", "y"]);
        assert!(d.layer("Doors").is_none() && d.len() == 3);
        // LAYERP: the layer table before the last change (Doors back, empty).
        run(&mut e, &mut d, &["layerp"]);
        assert!(d.layer("Doors").is_some());
        // LAYCUR: everything onto layer 0.
        e.select_all(&d);
        run(&mut e, &mut d, &["laycur"]);
        assert!(d.entities().all(|x| x.layer == "0"));
    }
}

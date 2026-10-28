use oxidraft_model::{Color, EntityId, snap};

use crate::{Command, Ctx, Input, Prompt, Step};

enum Stage {
    Source,
    Destination { source: EntityId, layer: String, color: Color },
}

/// MATCHPROP: copies the layer and color of one object onto others, one
/// click at a time.
pub struct MatchProp {
    stage: Stage,
}

impl Default for MatchProp {
    fn default() -> Self {
        Self { stage: Stage::Source }
    }
}

impl Command for MatchProp {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Source => Prompt::new("Select source object"),
            Stage::Destination { .. } => Prompt::new("Select destination object(s)"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (Stage::Source, Input::Pick { at, .. }) => {
                let Some(e) = snap::pick(ctx.doc, at, ctx.pick_tolerance).and_then(|id| ctx.doc.entity(id)) else {
                    ctx.messages.push("No object found.".into());
                    return Ok(Step::Continue);
                };
                ctx.messages.push("Current active settings:  Color  Layer".into());
                self.stage = Stage::Destination { source: e.id, layer: e.layer.clone(), color: e.color };
                Ok(Step::Continue)
            }
            (Stage::Destination { layer, color, .. }, Input::Pick { at, .. }) => {
                if let Some(id) = snap::pick(ctx.doc, at, ctx.pick_tolerance) {
                    let (layer, color) = (layer.clone(), *color);
                    ctx.doc.update(id, |e| {
                        e.layer = layer;
                        e.color = color;
                    });
                }
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err("Select an object, or press Enter to finish.".into()),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            Stage::Destination { source, .. } => vec![source],
            Stage::Source => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

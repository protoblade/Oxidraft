use oxidraft_model::{Entity, EntityId};

use crate::{Command, Ctx, Input, Prompt, Step};

enum Stage {
    Source,
    Destination(Entity),
}

/// MATCHPROP: copies the layer, color, linetype and lineweight of one
/// object onto others, one click at a time.
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
                let Some(e) = ctx.pick(at).and_then(|id| ctx.doc.entity(id)) else {
                    ctx.messages.push("No object found.".into());
                    return Ok(Step::Continue);
                };
                ctx.messages.push("Current active settings:  Color  Layer  Ltype  Lineweight".into());
                self.stage = Stage::Destination(e.clone());
                Ok(Step::Continue)
            }
            (Stage::Destination(source), Input::Pick { at, .. }) => {
                if let Some(id) = ctx.pick(at) {
                    let source = source.clone();
                    ctx.doc.update(id, |e| {
                        e.layer = source.layer;
                        e.color = source.color;
                        e.linetype = source.linetype;
                        e.lineweight = source.lineweight;
                    });
                }
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err("Select an object, or press Enter to finish.".into()),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match &self.stage {
            Stage::Destination(source) => vec![source.id],
            Stage::Source => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

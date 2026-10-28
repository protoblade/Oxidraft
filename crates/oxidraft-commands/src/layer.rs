use crate::{Command, Ctx, Input, Prompt, Step};

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

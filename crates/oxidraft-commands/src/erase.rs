use crate::{Command, Ctx, Input, Prompt, Step};

/// ERASE: select objects, Enter to erase them.
#[derive(Default)]
pub struct Erase;

impl Command for Erase {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Selection(selection) => {
                ctx.vars.erased = selection.ids.iter().filter_map(|id| ctx.doc.entity(*id).cloned()).collect();
                for id in selection.ids {
                    ctx.doc.remove(id);
                }
                Ok(Step::Done)
            }
            _ => Err("Select objects, then press Enter.".into()),
        }
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

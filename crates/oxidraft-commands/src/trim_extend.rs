use crate::{
    Command, Ctx, Input, Prompt, Step, Vars,
    edit::{extend_at, trim_at},
};

/// TRIM and EXTEND in Quick mode: every other object is a cutting or
/// boundary edge. Shift-click does the other one, as in AutoCAD.
pub struct TrimExtend {
    extend: bool,
    /// Document changes made by each pick, for the Undo option.
    history: Vec<usize>,
}

impl TrimExtend {
    pub fn trim() -> Self {
        Self { extend: false, history: Vec::new() }
    }

    pub fn extend() -> Self {
        Self { extend: true, history: Vec::new() }
    }
}

impl Command for TrimExtend {
    fn start(&mut self, _vars: &Vars) -> Option<String> {
        Some("Current settings: Projection=UCS, Edge=None, Mode=Quick".into())
    }

    fn prompt(&self) -> Prompt {
        let text = if self.extend {
            "Select object to extend or shift-select to trim"
        } else {
            "Select object to trim or shift-select to extend"
        };
        Prompt::new(text).with_keywords(&["Undo"])
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, shift } => {
                let changes = if self.extend != shift {
                    extend_at(ctx.doc, at, ctx.pick_tolerance)?
                } else {
                    trim_at(ctx.doc, at, ctx.pick_tolerance)?
                };
                if changes > 0 {
                    self.history.push(changes);
                }
                Ok(Step::Continue)
            }
            Input::Keyword("Undo") => {
                match self.history.pop() {
                    Some(n) => {
                        for _ in 0..n {
                            ctx.doc.undo_last_in_transaction();
                        }
                    }
                    None => ctx.messages.push("All operations have been undone.".into()),
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select an object, or press Enter to exit.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

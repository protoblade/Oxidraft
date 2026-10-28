//! EATTEDIT and ATTDISP: block attributes.

use oxidraft_model::{AttributeDisplay, Document, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest};

/// EATTEDIT: picks a block reference and opens the Enhanced Attribute
/// Editor on it. A block selected beforehand is used right away.
#[derive(Default)]
pub struct EattEdit;

impl Command for EattEdit {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select a block")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Selection(s) => {
                let Some(e) = s.ids.first().and_then(|&id| ctx.doc.entity(id)) else { return Ok(Step::Done) };
                match &e.geometry {
                    Geometry::Insert { attributes, .. } if !attributes.is_empty() => {
                        ctx.ui = Some(UiRequest::EditAttributes(e.id));
                        Ok(Step::Done)
                    }
                    Geometry::Insert { .. } => Err("That block has no attributes.".into()),
                    _ => Err("That object is not a block.".into()),
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select a block.".into()),
        }
    }

    fn wants_selection(&self) -> bool {
        true
    }

    fn single_selection(&self) -> bool {
        true
    }
}

const SETTINGS: &[&str] = &["Normal", "ON", "OFF"];

/// ATTDISP: draws attributes as they're set (Normal), all of them (ON), or
/// none (OFF).
#[derive(Default)]
pub struct AttDisp {
    current: &'static str,
}

impl Command for AttDisp {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.current = match doc.attdisp() {
            AttributeDisplay::Normal => "Normal",
            AttributeDisplay::On => "ON",
            AttributeDisplay::Off => "OFF",
        };
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Enter attribute visibility setting").with_keywords(SETTINGS).with_default(self.current)
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let mode = match input {
            Input::Keyword("Normal") => AttributeDisplay::Normal,
            Input::Keyword("ON") => AttributeDisplay::On,
            Input::Keyword("OFF") => AttributeDisplay::Off,
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Choose Normal, ON or OFF.".into()),
        };
        ctx.doc.set_attdisp(mode);
        ctx.messages.push("Regenerating model.".into());
        Ok(Step::Done)
    }
}

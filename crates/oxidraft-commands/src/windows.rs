//! SYSWINDOWS: how the open drawings are laid out in the window, for the
//! app to do.

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest};

/// SYSWINDOWS: Cascade, tile Horizontal, tile Vertical or Arrange icons.
#[derive(Default)]
pub struct SysWindows;

impl Command for SysWindows {
    fn prompt(&self) -> Prompt {
        Prompt::new("Enter an option")
            .with_keywords(&["Cascade", "tile Horizontal", "tile Vertical", "Arrange icons"])
            .with_default("Cascade")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let layout = match input {
            Input::Keyword("tile Horizontal") => 1,
            Input::Keyword("tile Vertical" | "Arrange icons") => 2,
            Input::Keyword(_) | Input::Enter => 0,
            _ => return Err("Requires an option keyword.".into()),
        };
        ctx.ui = Some(UiRequest::ArrangeWindows(layout));
        Ok(Step::Done)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum WsStage {
    Option,
    Name(&'static str),
    NewName,
}

/// WORKSPACE: setCurrent, SAveas, Rename, Delete or ? (list), with the
/// workspace's name; WSSAVE goes straight to SAveas. The app keeps the
/// workspaces.
pub struct WorkspaceCmd {
    stage: WsStage,
    name: String,
}

impl WorkspaceCmd {
    pub fn new(save: bool) -> Self {
        Self { stage: if save { WsStage::Name("SAveas") } else { WsStage::Option }, name: String::new() }
    }
}

impl Command for WorkspaceCmd {
    fn prompt(&self) -> Prompt {
        match self.stage {
            WsStage::Option => Prompt::new("Enter workspace option")
                .with_keywords(&["setCurrent", "SAveas", "Rename", "Delete", "?"])
                .with_default("setCurrent"),
            WsStage::Name("SAveas") => Prompt::new("Save workspace as"),
            WsStage::Name("Rename") => Prompt::new("Enter name of workspace to rename"),
            WsStage::Name("Delete") => Prompt::new("Enter name of workspace to delete"),
            WsStage::Name(_) => Prompt::new("Enter name of workspace to make current"),
            WsStage::NewName => Prompt::new("Enter new workspace name"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let request = |option: &str, name: &str, new: &str| {
            Some(UiRequest::Workspace(option.to_string(), name.to_string(), new.to_string()))
        };
        match (self.stage, input) {
            (WsStage::Option, Input::Keyword("?")) => {
                ctx.ui = request("?", "", "");
                return Ok(Step::Done);
            }
            (WsStage::Option, Input::Keyword(k)) => {
                self.stage = WsStage::Name(match k {
                    "SAveas" => "SAveas",
                    "Rename" => "Rename",
                    "Delete" => "Delete",
                    _ => "setCurrent",
                });
            }
            (WsStage::Option, Input::Enter) => self.stage = WsStage::Name("setCurrent"),
            (WsStage::Option, Input::Text(t)) => {
                ctx.ui = request("setCurrent", &t, "");
                return Ok(Step::Done);
            }
            (WsStage::Name("Rename"), Input::Text(t)) => {
                self.name = t;
                self.stage = WsStage::NewName;
            }
            (WsStage::Name(option), Input::Text(t)) => {
                ctx.ui = request(option, &t, "");
                return Ok(Step::Done);
            }
            (WsStage::NewName, Input::Text(t)) => {
                ctx.ui = request("Rename", &self.name, &t);
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Enter a workspace name.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        !matches!(self.stage, WsStage::Option)
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

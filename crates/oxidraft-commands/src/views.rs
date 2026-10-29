//! The view: VPORTS splits model space into panes, -VIEW saves and
//! restores named views, and UCSICON and NAVBAR show or hide parts of the
//! drawing area.

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::NamedView;

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest, Vars, ViewRequest};

/// How model space's drawing area is split into panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tiling {
    Single,
    /// Two side by side.
    TwoVertical,
    /// Two one above the other.
    TwoHorizontal,
    /// Three, the large one on the right.
    ThreeRight,
    /// Three, the large one on the left.
    ThreeLeft,
    FourEqual,
}

#[derive(Clone, Copy, PartialEq)]
enum VportsStage {
    Option,
    Two,
    Three,
}

/// VPORTS (-VPORTS): SIngle, 2, 3 or 4 panes.
pub struct Vports {
    stage: VportsStage,
}

impl Default for Vports {
    fn default() -> Self {
        Self { stage: VportsStage::Option }
    }
}

impl Command for Vports {
    fn prompt(&self) -> Prompt {
        match self.stage {
            VportsStage::Option => {
                Prompt::new("Enter an option").with_keywords(&["SIngle", "2", "3", "4"]).with_default("3")
            }
            VportsStage::Two => Prompt::new("Enter a configuration option")
                .with_keywords(&["Horizontal", "Vertical"])
                .with_default("Vertical"),
            VportsStage::Three => {
                Prompt::new("Enter a configuration option").with_keywords(&["Left", "Right"]).with_default("Right")
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let tiling = match (self.stage, input) {
            (VportsStage::Option, Input::Keyword("SIngle")) => Tiling::Single,
            (VportsStage::Option, Input::Value(2.0)) => {
                self.stage = VportsStage::Two;
                return Ok(Step::Continue);
            }
            (VportsStage::Option, Input::Value(3.0)) => {
                self.stage = VportsStage::Three;
                return Ok(Step::Continue);
            }
            (VportsStage::Option, Input::Enter) => {
                self.stage = VportsStage::Three;
                return Ok(Step::Continue);
            }
            (VportsStage::Option, Input::Value(4.0)) => Tiling::FourEqual,
            (VportsStage::Two, Input::Keyword("Horizontal")) => Tiling::TwoHorizontal,
            (VportsStage::Two, Input::Keyword(_) | Input::Enter) => Tiling::TwoVertical,
            (VportsStage::Three, Input::Keyword("Left")) => Tiling::ThreeLeft,
            (VportsStage::Three, Input::Keyword(_) | Input::Enter) => Tiling::ThreeRight,
            _ => return Err("Enter SIngle, 2, 3 or 4.".into()),
        };
        ctx.ui = Some(UiRequest::Tiling(tiling));
        Ok(Step::Done)
    }
}

#[derive(Clone, PartialEq)]
enum ViewStage {
    Option,
    SaveName,
    RestoreName,
    DeleteName,
    WindowName,
    WindowFirst(String),
    WindowSecond(String, Point),
}

/// -VIEW: lists (?), saves, restores and deletes named views; Window
/// saves a window's view.
pub struct ViewCommand {
    stage: ViewStage,
}

impl Default for ViewCommand {
    fn default() -> Self {
        Self { stage: ViewStage::Option }
    }
}

impl ViewCommand {
    fn save(name: &str, center: Point, height: f64, width: f64, ctx: &mut Ctx) -> Result<Step, String> {
        let space = ctx.doc.space();
        ctx.doc.save_view(NamedView { name: name.into(), center, height, width, space })?;
        Ok(Step::Done)
    }
}

impl Command for ViewCommand {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            ViewStage::Option => {
                Prompt::new("Enter an option").with_keywords(&["?", "Delete", "Restore", "Save", "Window"])
            }
            ViewStage::SaveName | ViewStage::WindowName => Prompt::new("Enter view name to save"),
            ViewStage::RestoreName => Prompt::new("Enter view name to restore"),
            ViewStage::DeleteName => Prompt::new("Enter view name(s) to delete"),
            ViewStage::WindowFirst(_) => Prompt::new("Specify first corner"),
            ViewStage::WindowSecond(..) => Prompt::new("Specify opposite corner"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (ViewStage::Option, Input::Keyword(k)) => {
                self.stage = match k {
                    "Delete" => ViewStage::DeleteName,
                    "Restore" => ViewStage::RestoreName,
                    "Save" => ViewStage::SaveName,
                    "Window" => ViewStage::WindowName,
                    _ => {
                        let names: Vec<&str> = ctx.doc.views().iter().map(|v| v.name.as_str()).collect();
                        let list = if names.is_empty() { "No named views.".to_string() } else { names.join("\n") };
                        ctx.messages.push(format!("Saved views:\n{list}"));
                        return Ok(Step::Done);
                    }
                };
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            (ViewStage::SaveName, Input::Text(name)) => {
                // The view on screen: VIEWCTR, VIEWSIZE, and as wide as it shows.
                let (center, height) = (ctx.vars.view_center, ctx.vars.view_size);
                Self::save(&name, center, height, height * ctx.vars.view_aspect, ctx)
            }
            (ViewStage::WindowName, Input::Text(name)) => {
                oxidraft_model::check_name(&name)?;
                self.stage = ViewStage::WindowFirst(name);
                Ok(Step::Continue)
            }
            (ViewStage::WindowFirst(name), Input::Point(p)) => {
                self.stage = ViewStage::WindowSecond(name, p);
                Ok(Step::Continue)
            }
            (ViewStage::WindowSecond(name, a), Input::Point(b)) => {
                let w = Aabb::from_points(a, b);
                Self::save(&name, w.center(), w.height(), w.width(), ctx)
            }
            (ViewStage::RestoreName, Input::Text(name)) => {
                let view = ctx.doc.views().iter().find(|v| v.name.eq_ignore_ascii_case(&name)).cloned();
                let view = view.ok_or_else(|| format!("Cannot find view \"{name}\"."))?;
                if view.space != ctx.doc.space() {
                    ctx.doc.set_space(view.space);
                }
                ctx.view = Some(ViewRequest::Center { center: view.center, height: view.height });
                Ok(Step::Done)
            }
            (ViewStage::DeleteName, Input::Text(names)) => {
                for name in names.split(',') {
                    if !ctx.doc.delete_view(name.trim()) {
                        ctx.messages.push(format!("Cannot find view \"{}\".", name.trim()));
                    }
                }
                Ok(Step::Done)
            }
            (ViewStage::Option, _) => Err("Enter an option.".into()),
            (ViewStage::WindowFirst(_) | ViewStage::WindowSecond(..), _) => Err("Specify a corner.".into()),
            _ => Err("Enter a view name.".into()),
        }
    }

    fn wants_text(&self) -> bool {
        matches!(
            self.stage,
            ViewStage::SaveName | ViewStage::RestoreName | ViewStage::DeleteName | ViewStage::WindowName
        )
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// UCSICON and NAVBAR: show or hide the UCS icon or the navigation bar.
pub struct Toggle {
    name: &'static str,
    setting: fn(&mut Vars) -> &mut bool,
    current: bool,
}

impl Toggle {
    pub fn new(name: &'static str, setting: fn(&mut Vars) -> &mut bool) -> Self {
        Self { name, setting, current: true }
    }
}

impl Command for Toggle {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        let mut vars = vars.clone();
        self.current = *(self.setting)(&mut vars);
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new(format!("Enter {} state", self.name)).with_keywords(&["ON", "OFF"]).with_default(if self.current {
            "ON"
        } else {
            "OFF"
        })
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let on = match input {
            Input::Keyword(k) => k == "ON",
            Input::Enter => self.current,
            _ => return Err("Enter ON or OFF.".into()),
        };
        *(self.setting)(ctx.vars) = on;
        Ok(Step::Done)
    }
}

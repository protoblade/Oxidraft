//! System variables: SETVAR (and a variable's name typed on its own),
//! with the values the app gives it; LIMITS; FILL and QTEXT; GRID.

use oxidraft_geom::Point;
use oxidraft_model::Document;

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest, Vars};

/// A variable as SETVAR shows it: its name, value, and whether it can
/// be set.
#[derive(Debug, Clone, PartialEq)]
pub struct VarValue {
    pub name: &'static str,
    pub value: String,
    pub writable: bool,
}

/// SETVAR: a variable's name (or `?` to list them), then its new value,
/// which the app sets.
pub struct SetVar {
    name: Option<&'static str>,
    vars: Vec<VarValue>,
}

impl SetVar {
    /// SETVAR, or with `name` the variable typed on its own.
    pub fn new(name: Option<&'static str>, vars: Vec<VarValue>) -> Self {
        Self { name, vars }
    }

    fn var(&self, name: &str) -> Option<&VarValue> {
        self.vars.iter().find(|v| v.name.eq_ignore_ascii_case(name.trim()))
    }
}

impl Command for SetVar {
    fn prepare(&mut self, _doc: &Document) -> Option<String> {
        // A read-only variable typed on its own just shows its value.
        let v = self.name.and_then(|n| self.var(n)).filter(|v| !v.writable)?;
        let line = format!("{} = \"{}\" (read only)", v.name, v.value);
        self.name = None;
        self.vars.clear();
        Some(line)
    }

    fn is_finished(&self) -> bool {
        self.vars.is_empty()
    }

    fn prompt(&self) -> Prompt {
        match self.name.and_then(|n| self.var(n)) {
            Some(v) => Prompt::new(format!("Enter new value for {}", v.name)).with_default(v.value.clone()),
            None => Prompt::new("Enter variable name or [?]"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let text = match input {
            Input::Text(t) => t,
            Input::Value(v) => v.to_string(),
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        };
        match self.name {
            None if text.trim() == "?" => {
                for v in &self.vars {
                    let note = if v.writable { "" } else { "  (read only)" };
                    ctx.messages.push(format!("{:<16}{}{note}", v.name, v.value));
                }
                Ok(Step::Done)
            }
            None => {
                let v = self.var(&text).ok_or("Unknown variable name.  Type SETVAR ? for a list of variables.")?;
                if !v.writable {
                    ctx.messages.push(format!("{} = \"{}\" (read only)", v.name, v.value));
                    return Ok(Step::Done);
                }
                self.name = Some(v.name);
                Ok(Step::Continue)
            }
            Some(name) => {
                ctx.ui = Some(UiRequest::SetVar(name.to_string(), text.trim().to_string()));
                Ok(Step::Done)
            }
        }
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_with_spaces(&self) -> bool {
        self.name.is_some()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LimitsStage {
    Lower,
    Upper(Point),
}

/// LIMITS: the drawing's limits (LIMMIN, LIMMAX), and ON/OFF for
/// LIMCHECK, which keeps points inside them.
pub struct Limits {
    stage: LimitsStage,
    now: (Point, Point),
}

impl Default for Limits {
    fn default() -> Self {
        Self { stage: LimitsStage::Lower, now: (Point::ORIGIN, Point::new(420.0, 297.0)) }
    }
}

fn point_text(p: Point) -> String {
    format!("{},{}", oxidraft_model::units::length(p.x), oxidraft_model::units::length(p.y))
}

impl Command for Limits {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.now = doc.limits();
        Some("Reset Model space limits:".into())
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            LimitsStage::Lower => Prompt::new("Specify lower left corner or")
                .with_keywords(&["ON", "OFF"])
                .with_default(point_text(self.now.0)),
            LimitsStage::Upper(_) => Prompt::new("Specify upper right corner").with_default(point_text(self.now.1)),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (LimitsStage::Lower, Input::Keyword(k)) => {
                ctx.doc.set_limits_check(k == "ON");
                return Ok(Step::Done);
            }
            (LimitsStage::Lower, Input::Point(p)) => self.stage = LimitsStage::Upper(p),
            (LimitsStage::Lower, Input::Enter) => self.stage = LimitsStage::Upper(self.now.0),
            (LimitsStage::Upper(lower), input @ (Input::Point(_) | Input::Enter)) => {
                let upper = if let Input::Point(p) = input { p } else { self.now.1 };
                if upper.x <= lower.x || upper.y <= lower.y {
                    return Err("Invalid limits.".into());
                }
                ctx.doc.set_limits(lower, upper);
                return Ok(Step::Done);
            }
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }
}

/// FILL and QTEXT: fills shown filled, and text as boxes, on or off.
pub struct Mode {
    quick_text: bool,
    on: bool,
}

impl Mode {
    pub fn fill() -> Self {
        Self { quick_text: false, on: true }
    }

    pub fn quick_text() -> Self {
        Self { quick_text: true, on: false }
    }
}

impl Command for Mode {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.on = if self.quick_text { doc.quick_text() } else { doc.fill() };
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Enter mode").with_keywords(&["ON", "OFF"]).with_default(if self.on { "ON" } else { "OFF" })
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let on = match input {
            Input::Keyword(k) => k == "ON",
            Input::Enter => self.on,
            _ => return Err("Enter ON or OFF.".into()),
        };
        if self.quick_text {
            ctx.doc.set_quick_text(on);
        } else {
            ctx.doc.set_fill(on);
        }
        Ok(Step::Done)
    }
}

/// GRID: its spacing (which turns it on), ON, OFF, or the Snap spacing;
/// the app keeps it.
#[derive(Default)]
pub struct Grid {
    spacing: f64,
}

impl Command for Grid {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.spacing = vars.grid_spacing;
        None
    }

    fn prompt(&self) -> Prompt {
        let now = if self.spacing > 0.0 { oxidraft_model::units::length(self.spacing) } else { "adaptive".into() };
        Prompt::new("Specify grid spacing(X) or").with_keywords(&["ON", "OFF", "Snap", "aDaptive"]).with_default(now)
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let set = |ctx: &mut Ctx, name: &str, value: String| ctx.ui = Some(UiRequest::SetVar(name.into(), value));
        match input {
            Input::Keyword("ON") => set(ctx, "GRIDMODE", "1".into()),
            Input::Keyword("OFF") => set(ctx, "GRIDMODE", "0".into()),
            Input::Keyword("Snap") => set(ctx, "GRIDUNIT", ctx.vars.snap_spacing.to_string()),
            Input::Keyword(_) => set(ctx, "GRIDUNIT", "0".into()),
            Input::Value(v) if v > 0.0 => set(ctx, "GRIDUNIT", v.to_string()),
            Input::Enter => {}
            _ => return Err("Requires a positive spacing or an option.".into()),
        }
        Ok(Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for (i, s) in inputs.iter().enumerate() {
            if i == 0 {
                e.run(s, d);
            } else {
                e.submit(s, d, Point::ORIGIN, 0.5);
            }
        }
    }

    #[test]
    fn limits_and_limcheck() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["limits", "0,0", "100,50"]);
        assert_eq!(d.limits(), (Point::ORIGIN, Point::new(100.0, 50.0)));
        run(&mut e, &mut d, &["limits", "on"]);
        run(&mut e, &mut d, &["line", "10,10", "200,10", "20,10", ""]);
        assert!(e.history().iter().any(|l| l == "**Outside limits"));
        let lines = d.entities().filter(|x| matches!(x.geometry, oxidraft_model::Geometry::Line { .. })).count();
        assert_eq!(lines, 1, "the point outside was refused");
    }

    #[test]
    fn fill_qtext_regen_and_multiple() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["fill", "off"]);
        run(&mut e, &mut d, &["qtext", "on"]);
        assert!(!d.fill() && d.quick_text());
        run(&mut e, &mut d, &["regen"]);
        assert!(e.history().iter().any(|l| l == "Regenerating model."));
        // MULTIPLE CIRCLE: one circle, then another, until Esc.
        run(&mut e, &mut d, &["multiple", "circle", "0,0", "5", "10,0", "5"]);
        assert_eq!(e.active_name(), Some("CIRCLE"));
        e.cancel(&mut d);
        assert_eq!(d.entities().count(), 2);
        assert_eq!(e.active_name(), None);
    }

    #[test]
    fn setvar_asks_the_app() {
        let vars = vec![
            VarValue { name: "ORTHOMODE", value: "0".into(), writable: true },
            VarValue { name: "DWGNAME", value: "Drawing1.dwg".into(), writable: false },
        ];
        let (mut e, mut d) = (Engine::new(), Document::new());
        e.run_with("SETVAR", "SETVAR", Box::new(SetVar::new(None, vars.clone())), &mut d);
        e.submit("orthomode", &mut d, Point::ORIGIN, 0.5);
        assert_eq!(e.prompt(), "Enter new value for ORTHOMODE <0>:");
        e.submit("1", &mut d, Point::ORIGIN, 0.5);
        assert_eq!(e.take_ui_requests(), vec![UiRequest::SetVar("ORTHOMODE".into(), "1".into())]);
        e.run_with("SETVAR", "DWGNAME", Box::new(SetVar::new(Some("DWGNAME"), vars)), &mut d);
        assert!(!e.is_active());
        assert!(e.history().iter().any(|l| l == "DWGNAME = \"Drawing1.dwg\" (read only)"));
    }
}

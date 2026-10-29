//! -STYLE: makes or changes a text style at the command line, and makes it
//! current (STYLE's dialog is the app's).

use oxidraft_model::{Document, TextStyle};

use crate::{Command, Ctx, Input, Prompt, Step};

/// Fonts offered for text styles: AutoCAD's usual TrueType and SHX ones.
pub const FONTS: &[&str] = &[
    "arial.ttf",
    "arialn.ttf",
    "calibri.ttf",
    "times.ttf",
    "cour.ttf",
    "verdana.ttf",
    "isocp.shx",
    "isoct.shx",
    "romans.shx",
    "romand.shx",
    "simplex.shx",
    "txt.shx",
];

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Name,
    Font,
    Height,
    Width,
    Oblique,
}

pub struct StyleCmd {
    stage: Stage,
    style: TextStyle,
}

impl Default for StyleCmd {
    fn default() -> Self {
        Self { stage: Stage::Name, style: TextStyle::standard() }
    }
}

impl Command for StyleCmd {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.style = doc.current_text_style();
        None
    }

    fn prompt(&self) -> Prompt {
        let s = &self.style;
        match self.stage {
            Stage::Name => Prompt::new("Enter name of text style or [?]").with_default(&s.name),
            Stage::Font => Prompt::new("Specify full font name or font filename (TTF or SHX)").with_default(&s.font),
            Stage::Height => Prompt::new("Specify height of text").with_default(format!("{:.4}", s.height)),
            Stage::Width => Prompt::new("Specify width factor").with_default(format!("{:.4}", s.width_factor)),
            Stage::Oblique => Prompt::new("Specify obliquing angle").with_default(format!("{}", s.oblique)),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let s = &mut self.style;
        self.stage = match (self.stage, input) {
            (Stage::Name, Input::Text(t)) if t.trim() == "?" => {
                for st in ctx.doc.text_styles() {
                    ctx.messages.push(format!(
                        "Style name: \"{}\"  Font: {}  Height: {:.4}  Width factor: {:.4}  Obliquing angle: {}",
                        st.name, st.font, st.height, st.width_factor, st.oblique
                    ));
                }
                Stage::Name
            }
            (Stage::Name, Input::Text(t)) => {
                let name = t.trim();
                oxidraft_model::check_name(name)?;
                match ctx.doc.text_style(name) {
                    Some(existing) => *s = existing.clone(),
                    None => {
                        ctx.messages.push(format!("New style \"{name}\"."));
                        *s = TextStyle { name: name.into(), ..s.clone() };
                    }
                }
                Stage::Font
            }
            (Stage::Font, Input::Text(t)) => {
                s.font = t.trim().to_string();
                Stage::Height
            }
            (Stage::Height, Input::Value(v)) if v >= 0.0 => {
                s.height = v;
                Stage::Width
            }
            (Stage::Width, Input::Value(v)) if v > 0.0 => {
                s.width_factor = v;
                Stage::Oblique
            }
            (Stage::Oblique, Input::Value(v)) if v.abs() <= 85.0 => return self.finish(ctx, Some(v)),
            (Stage::Oblique, Input::Enter) => return self.finish(ctx, None),
            (Stage::Name, Input::Enter) => Stage::Font,
            (Stage::Font, Input::Enter) => Stage::Height,
            (Stage::Height, Input::Enter) => Stage::Width,
            (Stage::Width, Input::Enter) => Stage::Oblique,
            (Stage::Height, Input::Value(_)) => return Err("Requires a height of 0 or more.".into()),
            (Stage::Width, Input::Value(_)) => return Err("Requires a width factor above 0.".into()),
            (Stage::Oblique, Input::Value(_)) => return Err("Requires an angle between -85 and 85.".into()),
            _ => return Err("Invalid input.".into()),
        };
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, Stage::Name | Stage::Font)
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

impl StyleCmd {
    fn finish(&mut self, ctx: &mut Ctx, oblique: Option<f64>) -> Result<Step, String> {
        if let Some(o) = oblique {
            self.style.oblique = o;
        }
        ctx.doc.set_text_style(self.style.clone())?;
        ctx.doc.set_current_text_style(&self.style.name)?;
        ctx.messages.push(format!("\"{}\" is now the current text style.", self.style.name));
        Ok(Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn a_style_made_at_the_command_line_is_what_text_gets() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["-style", "Titles", "romans.shx", "5", "0.8", "10"]);
        let s = d.current_text_style();
        assert_eq!(
            (s.name.as_str(), s.font.as_str(), s.height, s.width_factor, s.oblique),
            ("Titles", "romans.shx", 5.0, 0.8, 10.0)
        );
        // TEXT doesn't ask for a height the style fixes.
        run(&mut e, &mut d, &["text", "0,0", "0", "PLAN", ""]);
        let Some(Geometry::Text { height, look, .. }) = d.entities().last().map(|e| e.geometry.clone()) else {
            panic!()
        };
        assert_eq!((height, look.style.as_str(), look.width_factor, look.oblique), (5.0, "Titles", 0.8, 10.0));
        // TEXT's Style option switches back to Standard, which asks.
        run(&mut e, &mut d, &["text", "s", "standard", "0,-10", "2", "0", "NOTE", ""]);
        let Some(Geometry::Text { height, look, .. }) = d.entities().last().map(|e| e.geometry.clone()) else {
            panic!()
        };
        assert_eq!((height, look.style.as_str()), (2.0, "Standard"));
        assert_eq!(d.current_text_style().name, "Standard");
    }
}

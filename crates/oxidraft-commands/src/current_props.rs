//! -COLOR, -LINETYPE and -LWEIGHT: what new objects get (CECOLOR,
//! CELTYPE, CELWEIGHT) at the command line; -LINETYPE also lists, loads
//! and creates linetypes.

use oxidraft_model::{Color, CurrentProps, Document, Linetype, Lineweight, linetype};

use crate::{Command, Ctx, Input, Prompt, Step};

/// AutoCAD's color names, by index.
const COLOR_NAMES: [&str; 7] = ["red", "yellow", "green", "cyan", "blue", "magenta", "white"];

/// A color typed: ByLayer, ByBlock, a name (red … white) or an index 1–255.
pub fn parse_color(text: &str) -> Option<Color> {
    let t = text.trim().to_ascii_lowercase();
    match t.as_str() {
        "bylayer" => return Some(Color::ByLayer),
        "byblock" => return Some(Color::ByBlock),
        _ => {}
    }
    if let Some(i) = COLOR_NAMES.iter().position(|n| *n == t) {
        return Some(Color::Index(i as u8 + 1));
    }
    t.parse::<u8>().ok().filter(|i| *i >= 1).map(Color::Index)
}

/// A color as the command line names it.
pub fn color_text(c: Color) -> String {
    match c {
        Color::ByLayer => "BYLAYER".into(),
        Color::ByBlock => "BYBLOCK".into(),
        Color::Index(i) if (1..=7).contains(&i) => COLOR_NAMES[usize::from(i) - 1].into(),
        Color::Index(i) => i.to_string(),
    }
}

/// A lineweight typed: ByLayer, ByBlock, Default, or millimeters (the
/// nearest standard weight).
pub fn parse_lineweight(text: &str) -> Option<Lineweight> {
    let t = text.trim().to_ascii_lowercase();
    match t.as_str() {
        "bylayer" => Some(Lineweight::ByLayer),
        "byblock" => Some(Lineweight::ByBlock),
        "default" => Some(Lineweight::Default),
        _ => {
            let mm = t.trim_end_matches("mm").trim().parse::<f64>().ok().filter(|v| *v >= 0.0)?;
            let wanted = (mm * 100.0).round();
            let nearest = Lineweight::STANDARD
                .into_iter()
                .min_by(|a, b| (f64::from(*a) - wanted).abs().total_cmp(&(f64::from(*b) - wanted).abs()))?;
            Some(Lineweight::Hundredths(nearest))
        }
    }
}

/// The linetypes loaded in the drawing: its own and the standard ones it
/// uses (anywhere), without Continuous.
pub fn loaded_linetypes(doc: &Document) -> Vec<Linetype> {
    let mut out: Vec<Linetype> = doc.own_linetypes().iter().filter_map(|n| doc.linetype(n)).collect();
    let used = doc
        .all_entities()
        .map(|e| e.linetype.as_str())
        .chain(doc.layers().iter().map(|l| l.linetype.as_str()))
        .chain(doc.blocks().flat_map(|b| b.entities.iter().map(|be| be.linetype.as_str())))
        .chain(std::iter::once(doc.current_props().linetype.as_str()));
    for name in used {
        if let Some(l) = doc.linetype(name).filter(|l| !l.pattern.is_empty())
            && !out.iter().any(|o| o.name.eq_ignore_ascii_case(&l.name))
        {
            out.push(l);
        }
    }
    out.sort_by_key(|l| l.name.to_ascii_uppercase());
    out
}

/// A linetype's pattern typed as in a .lin file: `A,12.7,-6.35,0,-6.35`.
fn parse_pattern(text: &str) -> Option<Vec<f64>> {
    let t = text.trim();
    let t = t.strip_prefix(['A', 'a']).map_or(t, |r| r.trim_start_matches([',', ' ']));
    let pattern: Vec<f64> = t.split(',').map(|v| v.trim().parse::<f64>().ok()).collect::<Option<_>>()?;
    (!pattern.is_empty() && pattern.iter().any(|v| *v != 0.0)).then_some(pattern)
}

fn set_current(ctx: &mut Ctx, f: impl FnOnce(&mut CurrentProps)) {
    let mut props = ctx.doc.current_props().clone();
    f(&mut props);
    ctx.doc.set_current_props(props);
}

/// -COLOR (-COL): the color new objects get.
#[derive(Default)]
pub struct ColorCmd {
    current: String,
}

impl Command for ColorCmd {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.current = color_text(doc.current_props().color);
        None
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Enter default object color (a name, 1 to 255, BYLAYER or BYBLOCK)").with_default(&self.current)
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Text(t) => {
                let color =
                    parse_color(&t).ok_or("Requires a color name, an integer from 1 to 255, BYLAYER or BYBLOCK.")?;
                set_current(ctx, |p| p.color = color);
                Ok(Step::Done)
            }
            _ => Ok(Step::Done),
        }
    }

    fn wants_text(&self) -> bool {
        true
    }
}

/// -LWEIGHT (-LW): the lineweight new objects get.
#[derive(Default)]
pub struct LweightCmd {
    current: Lineweight,
}

impl Command for LweightCmd {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.current = doc.current_props().lineweight;
        Some(format!("Current lineweight: {}", self.current.label()))
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Enter default lineweight for new objects or [?]").with_default(self.current.label())
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let text = match input {
            Input::Text(t) => t,
            Input::Value(v) => v.to_string(),
            _ => return Ok(Step::Done),
        };
        if text.trim() == "?" {
            let all: Vec<String> = Lineweight::STANDARD.iter().map(|w| Lineweight::Hundredths(*w).label()).collect();
            ctx.messages.push(format!("ByLayer, ByBlock, Default, {}", all.join(", ")));
            return Ok(Step::Continue);
        }
        let lw = parse_lineweight(&text).ok_or("Requires BYLAYER, BYBLOCK, DEFAULT or a lineweight in millimeters.")?;
        set_current(ctx, |p| p.lineweight = lw);
        Ok(Step::Done)
    }

    fn wants_text(&self) -> bool {
        true
    }
}

#[derive(Clone, PartialEq)]
enum LtStage {
    Option,
    Load,
    Set,
    CreateName,
    CreateDescription(String),
    CreatePattern(String, String),
}

/// -LINETYPE (-LT): ? lists the loaded linetypes, Load loads standard
/// ones, Set makes one current, Create defines a new one.
pub struct LinetypeCmd {
    stage: LtStage,
}

impl Default for LinetypeCmd {
    fn default() -> Self {
        Self { stage: LtStage::Option }
    }
}

impl Command for LinetypeCmd {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            LtStage::Option => Prompt::new("Enter an option").with_keywords(&["?", "Create", "Load", "Set"]),
            LtStage::Load => Prompt::new("Enter linetype(s) to load (* for all)"),
            LtStage::Set => Prompt::new("Specify linetype name or [?]").with_default("ByLayer"),
            LtStage::CreateName => Prompt::new("Enter name of linetype to create"),
            LtStage::CreateDescription(_) => Prompt::new("Descriptive text"),
            LtStage::CreatePattern(..) => Prompt::new("Enter linetype pattern (on next line): A,"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let list = |doc: &Document| {
            let names: Vec<String> = ["ByLayer", "ByBlock", linetype::CONTINUOUS]
                .into_iter()
                .map(String::from)
                .chain(loaded_linetypes(doc).into_iter().map(|l| format!("{:<16}{}", l.name, l.description)))
                .collect();
            names.join("\n")
        };
        match (self.stage.clone(), input) {
            (LtStage::Option, Input::Keyword("?")) => {
                ctx.messages.push(list(ctx.doc));
                return Ok(Step::Done);
            }
            (LtStage::Option, Input::Keyword("Create")) => self.stage = LtStage::CreateName,
            (LtStage::Option, Input::Keyword("Load")) => self.stage = LtStage::Load,
            (LtStage::Option, Input::Keyword(_)) => self.stage = LtStage::Set,
            (LtStage::Load, Input::Text(t)) => {
                let standard = linetype::standard();
                let wanted: Vec<&Linetype> = if t.trim() == "*" {
                    standard.iter().collect()
                } else {
                    let names: Vec<&str> = t.split(',').map(str::trim).collect();
                    let found: Vec<&Linetype> =
                        standard.iter().filter(|l| names.iter().any(|n| n.eq_ignore_ascii_case(&l.name))).collect();
                    for n in names.iter().filter(|n| !found.iter().any(|l| l.name.eq_ignore_ascii_case(n))) {
                        ctx.messages.push(format!("Linetype \"{n}\" not found."));
                    }
                    found
                };
                for l in &wanted {
                    ctx.doc.load_linetype((*l).clone());
                }
                ctx.messages.push(format!("{} linetype(s) loaded.", wanted.len()));
                return Ok(Step::Done);
            }
            (LtStage::Set, Input::Text(t)) if t.trim() == "?" => ctx.messages.push(list(ctx.doc)),
            (LtStage::Set, Input::Text(t)) => {
                let name = match t.trim() {
                    n if linetype::is_by_layer(n) => linetype::BYLAYER.to_string(),
                    n if linetype::is_by_block(n) => linetype::BYBLOCK.to_string(),
                    n => ctx.doc.linetype(n).map(|l| l.name).ok_or_else(|| format!("Linetype \"{n}\" not found."))?,
                };
                set_current(ctx, |p| p.linetype = name);
                return Ok(Step::Done);
            }
            (LtStage::CreateName, Input::Text(t)) => {
                oxidraft_model::check_name(t.trim())?;
                self.stage = LtStage::CreateDescription(t.trim().into());
            }
            (LtStage::CreateDescription(name), Input::Text(d)) => self.stage = LtStage::CreatePattern(name, d),
            (LtStage::CreateDescription(name), Input::Enter) => {
                self.stage = LtStage::CreatePattern(name, String::new());
            }
            (LtStage::CreatePattern(name, description), Input::Text(t)) => {
                let pattern = parse_pattern(&t).ok_or("Requires dash, dot and space lengths, like A,12.7,-6.35.")?;
                ctx.doc.load_linetype(Linetype::new(&name, &description, &pattern));
                ctx.messages.push(format!("Linetype \"{name}\" created."));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        self.stage != LtStage::Option
    }

    fn text_with_spaces(&self) -> bool {
        matches!(self.stage, LtStage::CreateDescription(_))
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Color, Document, Geometry, Lineweight};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn current_color_linetype_and_lineweight() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["-color", "red", "-lweight", "0.3", "-linetype", "s", "hidden"]);
        let p = d.current_props().clone();
        assert_eq!(
            (p.color, p.linetype.as_str(), p.lineweight),
            (Color::Index(1), "HIDDEN", Lineweight::Hundredths(30))
        );
        // New objects get them.
        run(&mut e, &mut d, &["line", "0,0", "10,0", ""]);
        let line = d.entities().find(|x| matches!(x.geometry, Geometry::Line { .. })).unwrap();
        assert_eq!(
            (line.color, line.linetype.as_str(), line.lineweight),
            (Color::Index(1), "HIDDEN", Lineweight::Hundredths(30))
        );
        // Load, then create one.
        run(&mut e, &mut d, &["-linetype", "l", "center,dashed,nosuch"]);
        assert!(d.own_linetypes().iter().any(|n| n == "CENTER"));
        run(&mut e, &mut d, &["-linetype", "c", "RAIL", "Rail -- -- --", "A,10,-2.5"]);
        assert_eq!(d.linetype("rail").unwrap().pattern, [10.0, -2.5]);
        run(&mut e, &mut d, &["-color", "bylayer", "-lweight", "bylayer", "-linetype", "s", "bylayer"]);
        assert_eq!(d.current_props(), &oxidraft_model::CurrentProps::by_layer());
    }
}

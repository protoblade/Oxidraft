//! DIMEDIT, DIMTEDIT and DIMBREAK: a linear dimension's text put back,
//! replaced, turned or moved, its extension lines slanted, and gaps cut
//! where other objects cross it.

use oxidraft_geom::{Point, Segment, curve::Seg};
use oxidraft_model::{EntityId, Geometry, annotation::DimExtra};

use crate::{Command, Ctx, Input, Prompt, Step};

/// DIMBREAK's gap at scale 1 (AutoCAD's DIMBREAK size for ISO drawings).
const BREAK_SIZE: f64 = 3.75;

/// Changes a linear dimension's extras (and text); false for other objects.
fn change(ctx: &mut Ctx, id: EntityId, f: &dyn Fn(&mut DimExtra, &mut Option<String>)) -> bool {
    let mut changed = false;
    ctx.doc.update(id, |e| {
        if let Geometry::Dimension { extra, text, .. } = &mut e.geometry {
            f(extra, text);
            changed = true;
        }
    });
    changed
}

#[derive(Clone, PartialEq)]
enum EditStage {
    Option,
    NewText,
    Angle,
    Select(EditKind),
    ObliqueAngle,
}

#[derive(Clone, PartialEq)]
enum EditKind {
    Home,
    New(Option<String>),
    Rotate(f64),
    Oblique,
}

/// DIMEDIT (DED): Home, New text, Rotate the text, or Oblique extension lines.
pub struct DimEdit {
    stage: EditStage,
    ids: Vec<EntityId>,
}

impl Default for DimEdit {
    fn default() -> Self {
        Self { stage: EditStage::Option, ids: Vec::new() }
    }
}

impl Command for DimEdit {
    fn prompt(&self) -> Prompt {
        match self.stage {
            EditStage::Option => Prompt::new("Enter type of dimension editing")
                .with_keywords(&["Home", "New", "Rotate", "Oblique"])
                .with_default("Home"),
            EditStage::NewText => Prompt::new("Enter new dimension text (<> for the measurement)"),
            EditStage::Angle => Prompt::new("Specify angle for dimension text"),
            EditStage::Select(_) => Prompt::new("Select objects"),
            EditStage::ObliqueAngle => Prompt::new("Enter obliquing angle (press ENTER for none)"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (EditStage::Option, Input::Keyword("New")) => self.stage = EditStage::NewText,
            (EditStage::Option, Input::Keyword("Rotate")) => self.stage = EditStage::Angle,
            (EditStage::Option, Input::Keyword("Oblique")) => self.stage = EditStage::Select(EditKind::Oblique),
            (EditStage::Option, Input::Keyword(_) | Input::Enter) => self.stage = EditStage::Select(EditKind::Home),
            (EditStage::NewText, Input::Text(t)) => {
                let text = (t.trim() != "<>").then_some(t);
                self.stage = EditStage::Select(EditKind::New(text));
            }
            (EditStage::Angle, Input::Value(a)) => self.stage = EditStage::Select(EditKind::Rotate(a)),
            (EditStage::Select(EditKind::Oblique), Input::Selection(s)) => {
                self.ids = s.ids;
                self.stage = EditStage::ObliqueAngle;
            }
            (EditStage::ObliqueAngle, input @ (Input::Value(_) | Input::Enter)) => {
                let angle = if let Input::Value(a) = input { Some(a) } else { None };
                let n = self.ids.iter().filter(|id| change(ctx, **id, &|x, _| x.oblique = angle)).count();
                ctx.messages.push(format!("{n} dimension(s) changed."));
                return Ok(Step::Done);
            }
            (EditStage::Select(kind), Input::Selection(s)) => {
                let n = s
                    .ids
                    .iter()
                    .filter(|id| {
                        change(ctx, **id, &|x, text| match &kind {
                            EditKind::Home => (x.text_at, x.text_angle) = (None, None),
                            EditKind::New(t) => *text = t.clone(),
                            EditKind::Rotate(a) => x.text_angle = Some(*a),
                            EditKind::Oblique => {}
                        })
                    })
                    .count();
                ctx.messages.push(format!("{n} dimension(s) changed."));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, EditStage::Select(_))
    }

    fn wants_text(&self) -> bool {
        self.stage == EditStage::NewText
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TextStage {
    Select,
    Location(EntityId),
    Angle(EntityId),
}

/// DIMTEDIT (DIMTED): moves a linear dimension's text (its line follows),
/// or sets it Left, Right, Center, Home, or at an Angle.
pub struct DimTEdit {
    stage: TextStage,
}

impl Default for DimTEdit {
    fn default() -> Self {
        Self { stage: TextStage::Select }
    }
}

impl DimTEdit {
    /// The dimension with its text at `at`, its line through it.
    fn moved(g: &Geometry, at: Point) -> Geometry {
        let mut g = g.clone();
        if let Geometry::Dimension { line, extra, .. } = &mut g {
            *line = at;
            extra.text_at = Some(at);
        }
        g
    }

    /// Where the dimension line meets each extension line, left first as
    /// the text reads.
    fn ends(g: &Geometry) -> Option<(Point, Point)> {
        let Geometry::Dimension { p1, p2, line, rotation, aligned, .. } = *g else { return None };
        let r = if aligned { p1.angle_to(p2) } else { rotation };
        let u = Point::ORIGIN.polar(1.0, r);
        let (d1, d2) = (line + u * (p1 - line).dot(u), line + u * (p2 - line).dot(u));
        let reads = Point::ORIGIN.polar(1.0, oxidraft_model::annotation::readable(r));
        Some(if (d2 - d1).dot(reads) >= 0.0 { (d1, d2) } else { (d2, d1) })
    }
}

impl Command for DimTEdit {
    fn prompt(&self) -> Prompt {
        match self.stage {
            TextStage::Select => Prompt::new("Select dimension"),
            TextStage::Location(_) => Prompt::new("Specify new location for dimension text")
                .with_keywords(&["Left", "Right", "Center", "Home", "Angle"]),
            TextStage::Angle(_) => Prompt::new("Specify angle for dimension text"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (TextStage::Select, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                if !matches!(ctx.doc.entity(id).map(|e| &e.geometry), Some(Geometry::Dimension { .. })) {
                    return Err("Select a linear dimension.".into());
                }
                self.stage = TextStage::Location(id);
            }
            (TextStage::Location(id), Input::Point(p)) => {
                let g = ctx.doc.entity(id).map(|e| e.geometry.clone()).ok_or("The dimension is gone.")?;
                ctx.doc.modify(id, Self::moved(&g, p));
                return Ok(Step::Done);
            }
            (TextStage::Location(id), Input::Keyword("Angle")) => self.stage = TextStage::Angle(id),
            (TextStage::Location(id), Input::Keyword(k)) => {
                let g = ctx.doc.entity(id).map(|e| e.geometry.clone()).ok_or("The dimension is gone.")?;
                let (left, right) = Self::ends(&g).ok_or("Select a linear dimension.")?;
                let at = match k {
                    "Left" => Some(left.lerp(right, 0.2)),
                    "Right" => Some(left.lerp(right, 0.8)),
                    _ => None,
                };
                let home = k == "Home";
                change(ctx, id, &|x, _| {
                    x.text_at = at;
                    if home {
                        x.text_angle = None;
                    }
                });
                return Ok(Step::Done);
            }
            (TextStage::Angle(id), Input::Value(a)) => {
                change(ctx, id, &|x, _| x.text_angle = Some(a));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        self.stage == TextStage::Select
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            TextStage::Location(id) | TextStage::Angle(id) => vec![id],
            TextStage::Select => Vec::new(),
        }
    }
}

/// The gaps other objects make crossing a dimension's lines (all of them,
/// or only `only`'s).
fn auto_breaks(ctx: &Ctx, id: EntityId, only: Option<EntityId>) -> Vec<(Point, Point)> {
    let Some(e) = ctx.doc.entity(id) else { return Vec::new() };
    let Geometry::Dimension { p1, p2, line, rotation, aligned, text, scale, extra } = &e.geometry else {
        return Vec::new();
    };
    let plain = DimExtra { breaks: Vec::new(), ..(**extra).clone() };
    let prims = oxidraft_model::annotation::dimension_primitives(
        *p1,
        *p2,
        *line,
        *rotation,
        *aligned,
        text.as_deref(),
        *scale,
        &plain,
    );
    let lines: Vec<Segment> = prims
        .iter()
        .filter_map(|g| if let Geometry::Line { start, end } = g { Some(Segment::new(*start, *end)) } else { None })
        .collect();
    let half = BREAK_SIZE * scale / 2.0;
    let space = e.space;
    let mut out = Vec::new();
    for other in ctx.doc.all_entities().filter(|o| o.id != id && o.space == space) {
        if only.is_some_and(|o| o != other.id) {
            continue;
        }
        for s in other.geometry.segments() {
            for l in &lines {
                let seg = Seg::Line(*l);
                for (t, _) in seg.intersections(&s) {
                    let x = l.start.lerp(l.end, t);
                    let u = (l.end - l.start) * (1.0 / l.length().max(1e-12));
                    out.push((x - u * half, x + u * half));
                }
            }
        }
    }
    out
}

#[derive(Clone, PartialEq)]
enum BreakStage {
    Select,
    Multiple,
    Option(Vec<EntityId>),
    First(EntityId),
    Second(EntityId, Point),
}

/// DIMBREAK: gaps in a dimension where objects cross it — all of them
/// (Auto), one picked, or between two points (Manual); Remove takes them out.
pub struct DimBreak {
    stage: BreakStage,
}

impl Default for DimBreak {
    fn default() -> Self {
        Self { stage: BreakStage::Select }
    }
}

impl Command for DimBreak {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            BreakStage::Select => Prompt::new("Select dimension to add/remove break").with_keywords(&["Multiple"]),
            BreakStage::Multiple => Prompt::new("Select dimensions to add/remove break"),
            BreakStage::Option(ids) if ids.len() > 1 => {
                Prompt::new("Enter an option").with_keywords(&["Break", "Remove"]).with_default("Break")
            }
            BreakStage::Option(_) => Prompt::new("Select object to break dimension")
                .with_keywords(&["Auto", "Manual", "Remove"])
                .with_default("Auto"),
            BreakStage::First(_) => Prompt::new("Specify first break point"),
            BreakStage::Second(..) => Prompt::new("Specify second break point"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let dimension = |ctx: &Ctx, id: EntityId| {
            matches!(ctx.doc.entity(id).map(|e| &e.geometry), Some(Geometry::Dimension { .. }))
        };
        let add = |ctx: &mut Ctx, id: EntityId, gaps: Vec<(Point, Point)>| {
            change(ctx, id, &|x, _| x.breaks.extend(gaps.iter().copied()));
        };
        match (self.stage.clone(), input) {
            (BreakStage::Select, Input::Keyword(_)) => self.stage = BreakStage::Multiple,
            (BreakStage::Select, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).filter(|id| dimension(ctx, *id)).ok_or("Select a linear dimension.")?;
                self.stage = BreakStage::Option(vec![id]);
            }
            (BreakStage::Multiple, Input::Selection(s)) => {
                let ids: Vec<EntityId> = s.ids.into_iter().filter(|id| dimension(ctx, *id)).collect();
                if ids.is_empty() {
                    return Ok(Step::Done);
                }
                self.stage = BreakStage::Option(ids);
            }
            (BreakStage::Option(ids), Input::Keyword("Remove")) => {
                for id in ids {
                    change(ctx, id, &|x, _| x.breaks.clear());
                }
                return Ok(Step::Done);
            }
            (BreakStage::Option(ids), Input::Keyword("Manual")) if ids.len() == 1 => {
                self.stage = BreakStage::First(ids[0]);
            }
            (BreakStage::Option(ids), Input::Keyword(_) | Input::Enter) => {
                let mut count = 0;
                for id in ids {
                    let gaps = auto_breaks(ctx, id, None);
                    count += gaps.len();
                    change(ctx, id, &|x, _| x.breaks.clear());
                    add(ctx, id, gaps);
                }
                ctx.messages.push(format!("{count} break(s) made."));
                return Ok(Step::Done);
            }
            (BreakStage::Option(ids), Input::Pick { at, .. }) if ids.len() == 1 => {
                let object =
                    ctx.pick(at).filter(|o| *o != ids[0]).ok_or("Select an object that crosses the dimension.")?;
                let gaps = auto_breaks(ctx, ids[0], Some(object));
                if gaps.is_empty() {
                    return Err("That object doesn't cross the dimension.".into());
                }
                add(ctx, ids[0], gaps);
            }
            (BreakStage::First(id), Input::Point(p)) => self.stage = BreakStage::Second(id, p),
            (BreakStage::Second(id, a), Input::Point(b)) => {
                add(ctx, id, vec![(a, b)]);
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        matches!(&self.stage, BreakStage::Select) || matches!(&self.stage, BreakStage::Option(ids) if ids.len() == 1)
    }

    fn wants_selection(&self) -> bool {
        self.stage == BreakStage::Multiple
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            BreakStage::Second(_, p) => Some(p),
            _ => None,
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match &self.stage {
            BreakStage::Option(ids) => ids.clone(),
            BreakStage::First(id) | BreakStage::Second(id, _) => vec![*id],
            _ => Vec::new(),
        }
    }
}

/// TOLERANCE (TOL): a feature control frame placed where picked. Its
/// content comes typed as AutoCAD writes it, or from the app's Geometric
/// Tolerance dialog.
#[derive(Default)]
pub struct Tolerance {
    content: Option<String>,
    height: f64,
}

impl Command for Tolerance {
    fn start(&mut self, vars: &crate::Vars) -> Option<String> {
        self.height = oxidraft_model::annotation::dim_style().text_height * vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.content {
            None => Prompt::new("Enter tolerance (as {\\Fgdt;j}%%v0.1%%vA, rows split by ^J)"),
            Some(_) => Prompt::new("Enter tolerance location"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.content, input) {
            (None, Input::Text(t)) if !t.trim().is_empty() => self.content = Some(t),
            (Some(content), Input::Point(p)) => {
                ctx.doc.add(Geometry::Tolerance {
                    position: p,
                    height: self.height,
                    rotation: 0.0,
                    content: content.clone(),
                });
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Enter the tolerance, then its location.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match &self.content {
            Some(c) => {
                vec![Geometry::Tolerance { position: cursor, height: self.height, rotation: 0.0, content: c.clone() }]
            }
            None => Vec::new(),
        }
    }

    fn wants_text(&self) -> bool {
        self.content.is_none()
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, PartialEq)]
enum JogStage {
    Select,
    Remove,
    Location(EntityId),
}

/// DIMJOGLINE (DJL): a jog in a linear dimension's line, where picked (or
/// halfway); Remove takes it out.
pub struct DimJogLine {
    stage: JogStage,
}

impl Default for DimJogLine {
    fn default() -> Self {
        Self { stage: JogStage::Select }
    }
}

impl Command for DimJogLine {
    fn prompt(&self) -> Prompt {
        match self.stage {
            JogStage::Select => Prompt::new("Select dimension to add jog").with_keywords(&["Remove"]),
            JogStage::Remove => Prompt::new("Select jog to remove"),
            JogStage::Location(_) => Prompt::new("Specify jog location (or press ENTER)"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let dimension = |ctx: &Ctx, at: Point| {
            ctx.pick(at)
                .filter(|id| matches!(ctx.doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Dimension { .. })))
        };
        match (self.stage, input) {
            (JogStage::Select, Input::Keyword(_)) => self.stage = JogStage::Remove,
            (JogStage::Select, Input::Pick { at, .. }) => {
                self.stage = JogStage::Location(dimension(ctx, at).ok_or("Select a linear dimension.")?);
            }
            (JogStage::Remove, Input::Pick { at, .. }) => {
                let id = dimension(ctx, at).ok_or("Select a jogged dimension.")?;
                change(ctx, id, &|x, _| x.jog = None);
                return Ok(Step::Done);
            }
            (JogStage::Location(id), input @ (Input::Point(_) | Input::Enter)) => {
                let at = match input {
                    Input::Point(p) => p,
                    _ => match ctx.doc.entity(id).map(|e| &e.geometry) {
                        Some(Geometry::Dimension { p1, p2, line, rotation, aligned, .. }) => {
                            let r = if *aligned { p1.angle_to(*p2) } else { *rotation };
                            let u = Point::ORIGIN.polar(1.0, r);
                            let (d1, d2) = (*line + u * (*p1 - *line).dot(u), *line + u * (*p2 - *line).dot(u));
                            d1.lerp(d2, 0.25)
                        }
                        _ => return Ok(Step::Done),
                    },
                };
                change(ctx, id, &|x, _| x.jog = Some(at));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Select a dimension.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, JogStage::Select | JogStage::Remove)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            JogStage::Location(id) => vec![id],
            _ => Vec::new(),
        }
    }
}

/// The dimension variables DIMOVERRIDE can set on a dimension.
const OVERRIDABLE: &[&str] = &["DIMTXT", "DIMASZ", "DIMEXO", "DIMEXE", "DIMGAP", "DIMDEC"];

fn set_variable(style: &mut oxidraft_model::annotation::DimStyle, name: &str, v: f64) -> Result<(), String> {
    match name {
        "DIMTXT" if v > 0.0 => style.text_height = v,
        "DIMASZ" if v >= 0.0 => style.arrow_size = v,
        "DIMEXO" if v >= 0.0 => style.ext_offset = v,
        "DIMEXE" if v >= 0.0 => style.ext_beyond = v,
        "DIMGAP" => style.gap = v,
        "DIMDEC" if (0.0..=8.0).contains(&v) && v.fract() == 0.0 => style.decimals = v as usize,
        _ => return Err(format!("Invalid value for {name}.")),
    }
    Ok(())
}

fn variable(style: &oxidraft_model::annotation::DimStyle, name: &str) -> String {
    match name {
        "DIMTXT" => format!("{:.4}", style.text_height),
        "DIMASZ" => format!("{:.4}", style.arrow_size),
        "DIMEXO" => format!("{:.4}", style.ext_offset),
        "DIMEXE" => format!("{:.4}", style.ext_beyond),
        "DIMGAP" => format!("{:.4}", style.gap),
        _ => style.decimals.to_string(),
    }
}

#[derive(Clone, PartialEq)]
enum OverrideStage {
    Name,
    Value(String),
    Select,
    Clear,
}

/// DIMOVERRIDE (DOV): dimension variables set on the selected dimensions
/// alone (DIMTXT, DIMASZ, DIMEXO, DIMEXE, DIMGAP, DIMDEC), or Cleared.
pub struct DimOverride {
    stage: OverrideStage,
    style: oxidraft_model::annotation::DimStyle,
    changed: bool,
}

impl Default for DimOverride {
    fn default() -> Self {
        Self { stage: OverrideStage::Name, style: oxidraft_model::annotation::DEFAULT_DIM_STYLE, changed: false }
    }
}

impl Command for DimOverride {
    fn prepare(&mut self, doc: &oxidraft_model::Document) -> Option<String> {
        self.style = doc.dim_style();
        None
    }

    fn prompt(&self) -> Prompt {
        match &self.stage {
            OverrideStage::Name if self.changed => Prompt::new("Enter dimension variable name to override"),
            OverrideStage::Name => {
                Prompt::new("Enter dimension variable name to override").with_keywords(&["Clear overrides"])
            }
            OverrideStage::Value(name) => {
                Prompt::new("Enter new value for dimension variable").with_default(variable(&self.style, name))
            }
            OverrideStage::Select | OverrideStage::Clear => Prompt::new("Select objects"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (OverrideStage::Name, Input::Keyword(_)) => self.stage = OverrideStage::Clear,
            (OverrideStage::Name, Input::Text(t)) => {
                let name = t.trim().to_ascii_uppercase();
                if !OVERRIDABLE.contains(&name.as_str()) {
                    return Err(format!("Can override {}.", OVERRIDABLE.join(", ")));
                }
                self.stage = OverrideStage::Value(name);
            }
            (OverrideStage::Name, Input::Enter) if self.changed => self.stage = OverrideStage::Select,
            (OverrideStage::Value(name), Input::Value(v)) => {
                set_variable(&mut self.style, &name, v)?;
                self.changed = true;
                self.stage = OverrideStage::Name;
            }
            (OverrideStage::Value(_), Input::Enter) => self.stage = OverrideStage::Name,
            (OverrideStage::Select, Input::Selection(s)) => {
                let style = self.style;
                let n = s.ids.iter().filter(|id| change(ctx, **id, &|x, _| x.style = Some(style))).count();
                ctx.messages.push(format!("{n} dimension(s) overridden."));
                return Ok(Step::Done);
            }
            (OverrideStage::Clear, Input::Selection(s)) => {
                let n = s.ids.iter().filter(|id| change(ctx, **id, &|x, _| x.style = None)).count();
                ctx.messages.push(format!("{n} dimension(s) cleared."));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, OverrideStage::Select | OverrideStage::Clear)
    }

    fn wants_text(&self) -> bool {
        self.stage == OverrideStage::Name
    }
}

/// DIMUPDATE (-DIMSTYLE Apply): the selected dimensions back to the
/// drawing's dimension style, their overrides cleared.
#[derive(Default)]
pub struct DimUpdate;

impl Command for DimUpdate {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Ok(Step::Done) };
        let n = s.ids.iter().filter(|id| change(ctx, **id, &|x, _| x.style = None)).count();
        ctx.messages.push(format!("{n} dimension(s) updated to the current style."));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry, annotation::DimExtra};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    fn click(e: &mut Engine, d: &mut Document, x: f64, y: f64) {
        let p = Point::new(x, y);
        e.click(p, p, false, d, 0.5);
    }

    fn extra(d: &Document) -> (DimExtra, Option<String>, Point) {
        d.entities()
            .find_map(|e| match &e.geometry {
                Geometry::Dimension { extra, text, line, .. } => Some(((**extra).clone(), text.clone(), *line)),
                _ => None,
            })
            .unwrap()
    }

    fn doc() -> Document {
        let mut d = Document::new();
        d.add(Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(40.0, 0.0),
            line: Point::new(20.0, 10.0),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Box::default(),
        });
        d
    }

    #[test]
    fn dimedit_and_dimtedit() {
        let (mut e, mut d) = (Engine::new(), doc());
        run(&mut e, &mut d, &["dimedit", "n", "WIDTH", "all", ""]);
        assert_eq!(extra(&d).1.as_deref(), Some("WIDTH"));
        run(&mut e, &mut d, &["dimedit", "r", "30", "all", ""]);
        assert_eq!(extra(&d).0.text_angle, Some(30.0));
        run(&mut e, &mut d, &["dimedit", "o", "all", "", "60"]);
        assert_eq!(extra(&d).0.oblique, Some(60.0));
        run(&mut e, &mut d, &["dimedit", "h", "all", ""]);
        assert_eq!(extra(&d).0.text_angle, None);
        // DIMTEDIT: the text, and the line with it, to a new place.
        run(&mut e, &mut d, &["dimtedit"]);
        click(&mut e, &mut d, 20.0, 10.0);
        run(&mut e, &mut d, &["30,15"]);
        let (x, _, line) = extra(&d);
        assert_eq!((x.text_at, line), (Some(Point::new(30.0, 15.0)), Point::new(30.0, 15.0)));
        run(&mut e, &mut d, &["dimtedit"]);
        click(&mut e, &mut d, 25.0, 15.0);
        run(&mut e, &mut d, &["h"]);
        assert_eq!(extra(&d).0.text_at, None);
    }

    #[test]
    fn tolerance_jog_and_overrides() {
        let (mut e, mut d) = (Engine::new(), doc());
        run(&mut e, &mut d, &["tolerance", "{\\Fgdt;j}%%v{\\Fgdt;n}0.1{\\Fgdt;m}%%vA%%vB", "50,50"]);
        let frame = d.entities().find_map(|x| match &x.geometry {
            g @ Geometry::Tolerance { .. } => Some(g.clone()),
            _ => None,
        });
        let prims = frame.expect("a frame").primitives().unwrap();
        let boxes = prims.iter().filter(|p| matches!(p, Geometry::Polyline { closed: true, .. })).count();
        assert_eq!(boxes, 4, "symbol, tolerance and two datums");
        assert!(prims.iter().any(|p| matches!(p, Geometry::Text { content, .. } if content == "0.1")));
        // DIMJOGLINE, then Remove.
        run(&mut e, &mut d, &["dimjogline"]);
        click(&mut e, &mut d, 25.0, 10.0);
        run(&mut e, &mut d, &["10,10"]);
        assert_eq!(extra(&d).0.jog, Some(Point::new(10.0, 10.0)));
        run(&mut e, &mut d, &["dimjogline", "r"]);
        click(&mut e, &mut d, 25.0, 10.0);
        assert_eq!(extra(&d).0.jog, None);
        // DIMOVERRIDE: bigger text on this one; DIMUPDATE puts it back.
        run(&mut e, &mut d, &["dimoverride", "dimtxt", "5", "", "all", ""]);
        assert_eq!(extra(&d).0.style.map(|s| s.text_height), Some(5.0));
        run(&mut e, &mut d, &["dimupdate", "all", ""]);
        assert_eq!(extra(&d).0.style, None);
    }

    #[test]
    fn dimbreak_cuts_where_objects_cross() {
        let (mut e, mut d) = (Engine::new(), doc());
        // A line across the dimension line and the first extension line.
        d.add(Geometry::Line { start: Point::new(-5.0, 2.0), end: Point::new(15.0, 22.0) });
        run(&mut e, &mut d, &["dimbreak"]);
        click(&mut e, &mut d, 25.0, 10.0);
        run(&mut e, &mut d, &[""]);
        let breaks = extra(&d).0.breaks;
        assert_eq!(breaks.len(), 2, "{breaks:?}");
        // The dimension line now has a gap: two pieces where it crosses.
        let g = d.entities().find(|x| matches!(x.geometry, Geometry::Dimension { .. })).unwrap().geometry.clone();
        let lines = g.primitives().unwrap().into_iter().filter(|p| matches!(p, Geometry::Line { start, end } if (start.y - 10.0).abs() < 1e-9 && (end.y - 10.0).abs() < 1e-9)).count();
        assert_eq!(lines, 2);
        run(&mut e, &mut d, &["dimbreak"]);
        click(&mut e, &mut d, 25.0, 10.0);
        run(&mut e, &mut d, &["r"]);
        assert!(extra(&d).0.breaks.is_empty());
    }
}

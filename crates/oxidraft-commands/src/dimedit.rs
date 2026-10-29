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

//! Annotation commands: TEXT, MTEXT, DIMLINEAR, DIMALIGNED and MLEADER.

use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{
    Attach, Geometry, Vertex,
    annotation::{LINE_SPACING, dimension_text, format_measurement, measurement},
    snap,
};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

fn style_line(height: f64) -> String {
    format!("Current text style:  \"Standard\"  Text height:  {height:.4}  Annotative:  No")
}

enum TextStage {
    Start,
    Height { start: Point },
    Rotation { start: Point },
    Lines { start: Point, rotation: f64, line: usize },
}

/// TEXT (DTEXT): single-line text, one object per line; an empty line ends it.
pub struct SingleText {
    stage: TextStage,
    height: f64,
}

impl Default for SingleText {
    fn default() -> Self {
        Self { stage: TextStage::Start, height: 2.5 }
    }
}

impl Command for SingleText {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.height = vars.text_height;
        Some(format!("{}  Justify:  Left", style_line(self.height)))
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            TextStage::Start => Prompt::new("Specify start point of text"),
            TextStage::Height { .. } => Prompt::new("Specify height").with_default(format!("{:.4}", self.height)),
            TextStage::Rotation { .. } => Prompt::new("Specify rotation angle of text").with_default("0"),
            TextStage::Lines { .. } => Prompt::new("Enter text"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (TextStage::Start, Input::Point(p)) => self.stage = TextStage::Height { start: p },
            (TextStage::Height { start }, input @ (Input::Value(_) | Input::Point(_) | Input::Enter)) => {
                let h = match input {
                    Input::Value(v) => v,
                    Input::Point(p) => start.distance(p),
                    _ => self.height,
                };
                if h <= EPSILON {
                    return Err("Requires a positive nonzero value.".into());
                }
                self.height = h;
                ctx.vars.text_height = h;
                self.stage = TextStage::Rotation { start: *start };
            }
            (TextStage::Rotation { start }, input @ (Input::Value(_) | Input::Point(_) | Input::Enter)) => {
                let rotation = match input {
                    Input::Value(v) => v,
                    Input::Point(p) => start.angle_to(p),
                    _ => 0.0,
                };
                self.stage = TextStage::Lines { start: *start, rotation, line: 0 };
            }
            (TextStage::Lines { start, rotation, line }, Input::Text(t)) => {
                let offset = Point::new(0.0, -(*line as f64) * self.height * LINE_SPACING);
                ctx.doc.add(Geometry::Text {
                    position: (*start + offset).rotated_about(*start, *rotation),
                    height: self.height,
                    rotation: *rotation,
                    width: 0.0,
                    attach: Attach::BottomLeft,
                    content: t,
                });
                self.stage = TextStage::Lines { start: *start, rotation: *rotation, line: line + 1 };
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Point or value required.".into()),
        }
        Ok(Step::Continue)
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            TextStage::Height { start } | TextStage::Rotation { start } => Some(start),
            _ => None,
        }
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, TextStage::Lines { .. })
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

enum MTextStage {
    First,
    Opposite { first: Point },
    Height { first: Point },
    Rotation { first: Point },
    Width { first: Point },
    Content { position: Point, width: f64 },
}

/// MTEXT: paragraph text in a box. Type `\P` for a new paragraph.
pub struct MText {
    stage: MTextStage,
    height: f64,
    rotation: f64,
}

impl Default for MText {
    fn default() -> Self {
        Self { stage: MTextStage::First, height: 2.5, rotation: 0.0 }
    }
}

impl MText {
    fn frame(first: Point, other: Point) -> Geometry {
        let corners = [first, Point::new(other.x, first.y), other, Point::new(first.x, other.y)];
        Geometry::Polyline { vertices: corners.into_iter().map(Vertex::at).collect(), closed: true }
    }
}

impl Command for MText {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.height = vars.text_height;
        Some(style_line(self.height))
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            MTextStage::First => Prompt::new("Specify first corner"),
            MTextStage::Opposite { .. } => {
                Prompt::new("Specify opposite corner").with_keywords(&["Height", "Rotation", "Width"])
            }
            MTextStage::Height { .. } => Prompt::new("Specify height").with_default(format!("{:.4}", self.height)),
            MTextStage::Rotation { .. } => {
                Prompt::new("Specify rotation angle").with_default(format!("{:.0}", self.rotation))
            }
            MTextStage::Width { .. } => Prompt::new("Specify width"),
            MTextStage::Content { .. } => Prompt::new("Enter text (\\P for a new paragraph)"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (MTextStage::First, Input::Point(p)) => self.stage = MTextStage::Opposite { first: p },
            (MTextStage::Opposite { first }, Input::Point(p)) => {
                // The text hangs from the box's top-left corner.
                let top_left = Point::new(first.x.min(p.x), first.y.max(p.y));
                self.stage = MTextStage::Content { position: top_left, width: (p.x - first.x).abs() };
            }
            (MTextStage::Opposite { first }, Input::Keyword("Height")) => {
                self.stage = MTextStage::Height { first: *first }
            }
            (MTextStage::Opposite { first }, Input::Keyword("Rotation")) => {
                self.stage = MTextStage::Rotation { first: *first }
            }
            (MTextStage::Opposite { first }, Input::Keyword("Width")) => {
                self.stage = MTextStage::Width { first: *first }
            }
            (MTextStage::Height { first }, input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(h) = input {
                    if h <= EPSILON {
                        return Err("Requires a positive nonzero value.".into());
                    }
                    self.height = h;
                    ctx.vars.text_height = h;
                }
                self.stage = MTextStage::Opposite { first: *first };
            }
            (MTextStage::Rotation { first }, input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(a) = input {
                    self.rotation = a;
                }
                self.stage = MTextStage::Opposite { first: *first };
            }
            (MTextStage::Width { first }, Input::Value(w)) if w >= 0.0 => {
                self.stage = MTextStage::Content { position: *first, width: w };
            }
            (MTextStage::Content { position, width }, Input::Text(content)) => {
                ctx.doc.add(Geometry::Text {
                    position: *position,
                    height: self.height,
                    rotation: self.rotation,
                    width: *width,
                    attach: Attach::TopLeft,
                    content,
                });
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            MTextStage::Opposite { first } => vec![Self::frame(first, cursor)],
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            MTextStage::Opposite { first } => Some(first),
            _ => None,
        }
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, MTextStage::Content { .. })
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

enum DimStage {
    First,
    PickObject,
    Second { p1: Point },
    Location { p1: Point, p2: Point },
    Text { p1: Point, p2: Point },
    Angle { p1: Point, p2: Point },
}

/// DIMLINEAR and DIMALIGNED: two extension line origins (or a line picked
/// after Enter), then where the dimension line goes.
pub struct Dimension {
    aligned: bool,
    stage: DimStage,
    /// Horizontal (0), Vertical (90) or Rotated; `None` picks from the cursor.
    forced_rotation: Option<f64>,
    text: Option<String>,
    scale: f64,
}

impl Dimension {
    pub fn linear() -> Self {
        Self { aligned: false, stage: DimStage::First, forced_rotation: None, text: None, scale: 1.0 }
    }

    pub fn aligned() -> Self {
        Self { aligned: true, ..Self::linear() }
    }

    /// AutoCAD's rule: a location above or below the points makes a
    /// horizontal dimension, one beside them a vertical one.
    fn rotation(&self, p1: Point, p2: Point, at: Point) -> f64 {
        if let Some(r) = self.forced_rotation {
            return r;
        }
        let beside = at.x < p1.x.min(p2.x) || at.x > p1.x.max(p2.x);
        let above_or_below = at.y < p1.y.min(p2.y) || at.y > p1.y.max(p2.y);
        if beside && !above_or_below { 90.0 } else { 0.0 }
    }

    fn geometry(&self, p1: Point, p2: Point, at: Point) -> Geometry {
        Geometry::Dimension {
            p1,
            p2,
            line: at,
            rotation: if self.aligned { p1.angle_to(p2) } else { self.rotation(p1, p2, at) },
            aligned: self.aligned,
            text: self.text.clone(),
            scale: self.scale,
        }
    }
}

impl Command for Dimension {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            DimStage::First => Prompt::new("Specify first extension line origin").with_default("select object"),
            DimStage::PickObject => Prompt::new("Select object to dimension"),
            DimStage::Second { .. } => Prompt::new("Specify second extension line origin"),
            DimStage::Location { .. } if self.aligned => {
                Prompt::new("Specify dimension line location").with_keywords(&["Text"])
            }
            DimStage::Location { .. } => Prompt::new("Specify dimension line location").with_keywords(&[
                "Text",
                "Horizontal",
                "Vertical",
                "Rotated",
            ]),
            DimStage::Text { p1, p2 } => {
                let rotation = self.forced_rotation.unwrap_or(0.0);
                Prompt::new("Enter dimension text").with_default(format_measurement(measurement(
                    p1,
                    p2,
                    rotation,
                    self.aligned,
                )))
            }
            DimStage::Angle { .. } => Prompt::new("Specify angle of dimension line").with_default("0"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (DimStage::First, Input::Point(p)) => self.stage = DimStage::Second { p1: p },
            (DimStage::First, Input::Enter) => self.stage = DimStage::PickObject,
            (DimStage::PickObject, Input::Pick { at, .. }) => {
                let picked = snap::pick(ctx.doc, at, ctx.pick_tolerance).and_then(|id| ctx.doc.entity(id));
                let Some(Geometry::Line { start, end }) = picked.map(|e| &e.geometry) else {
                    return Err("Select a line.".into());
                };
                self.stage = DimStage::Location { p1: *start, p2: *end };
            }
            (DimStage::Second { p1 }, Input::Point(p2)) => {
                if p1.approx_eq(p2) {
                    return Err("The two points must differ.".into());
                }
                self.stage = DimStage::Location { p1: *p1, p2 };
            }
            (DimStage::Location { p1, p2 }, Input::Point(at)) => {
                let g = self.geometry(*p1, *p2, at);
                if let Geometry::Dimension { rotation, .. } = &g {
                    let value = measurement(*p1, *p2, *rotation, self.aligned);
                    ctx.messages.push(format!("Dimension text = {}", dimension_text(self.text.as_deref(), value)));
                }
                ctx.doc.add(g);
                return Ok(Step::Done);
            }
            (DimStage::Location { p1, p2 }, Input::Keyword(k)) => match k {
                "Text" => self.stage = DimStage::Text { p1: *p1, p2: *p2 },
                "Horizontal" => self.forced_rotation = Some(0.0),
                "Vertical" => self.forced_rotation = Some(90.0),
                "Rotated" => self.stage = DimStage::Angle { p1: *p1, p2: *p2 },
                _ => return Err("Unknown option.".into()),
            },
            (DimStage::Text { p1, p2 }, input @ (Input::Text(_) | Input::Enter)) => {
                if let Input::Text(t) = input {
                    self.text = Some(t);
                }
                self.stage = DimStage::Location { p1: *p1, p2: *p2 };
            }
            (DimStage::Angle { p1, p2 }, input @ (Input::Value(_) | Input::Enter)) => {
                self.forced_rotation = Some(if let Input::Value(a) = input { a } else { 0.0 });
                self.stage = DimStage::Location { p1: *p1, p2: *p2 };
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            DimStage::Second { p1 } => vec![Geometry::Line { start: p1, end: cursor }],
            DimStage::Location { p1, p2 } => vec![self.geometry(p1, p2, cursor)],
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            DimStage::Second { p1 } => Some(p1),
            _ => None,
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, DimStage::PickObject)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, DimStage::Text { .. })
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

enum LeaderStage {
    Arrow,
    Landing { tip: Point },
    Content { tip: Point, landing: Point },
}

/// MLEADER: arrowhead location, landing location, then the text.
pub struct MLeader {
    stage: LeaderStage,
    scale: f64,
}

impl Default for MLeader {
    fn default() -> Self {
        Self { stage: LeaderStage::Arrow, scale: 1.0 }
    }
}

impl Command for MLeader {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            LeaderStage::Arrow => Prompt::new("Specify leader arrowhead location"),
            LeaderStage::Landing { .. } => Prompt::new("Specify leader landing location"),
            LeaderStage::Content { .. } => Prompt::new("Enter text"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (LeaderStage::Arrow, Input::Point(p)) => self.stage = LeaderStage::Landing { tip: p },
            (LeaderStage::Landing { tip }, Input::Point(p)) => {
                if tip.approx_eq(p) {
                    return Err("The landing must be away from the arrowhead.".into());
                }
                self.stage = LeaderStage::Content { tip: *tip, landing: p };
            }
            (LeaderStage::Content { tip, landing }, input @ (Input::Text(_) | Input::Enter)) => {
                let content = if let Input::Text(t) = input { t } else { String::new() };
                ctx.doc.add(Geometry::Leader { points: vec![*tip, *landing], content, scale: self.scale });
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Point required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            LeaderStage::Landing { tip } if !tip.approx_eq(cursor) => {
                vec![Geometry::Leader { points: vec![tip, cursor], content: String::new(), scale: self.scale }]
            }
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            LeaderStage::Landing { tip } => Some(tip),
            _ => None,
        }
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, LeaderStage::Content { .. })
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

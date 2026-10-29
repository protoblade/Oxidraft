//! DIMRADIUS, DIMDIAMETER, DIMANGULAR, DIMCONTINUE and DIMBASELINE.

use oxidraft_geom::Point;
use oxidraft_model::{
    EntityId, Geometry,
    annotation::{
        BASELINE_SPACING, angle_around, angular_measurement, dimension_text, format_angle, format_measurement,
        measurement, radial_value, two_line_angle,
    },
};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

fn picked(ctx: &Ctx, at: Point) -> Result<(EntityId, Geometry), String> {
    let id = ctx.pick(at).ok_or("No object found.")?;
    let g = ctx.doc.entity(id).map(|e| e.geometry.clone()).ok_or("No object found.")?;
    Ok((id, g))
}

/// Text option input: typed text overrides, Enter keeps the measurement.
fn text_override(input: Input, text: &mut Option<String>) -> Result<(), String> {
    match input {
        Input::Text(t) => *text = Some(t),
        Input::Enter => {}
        _ => return Err("Enter dimension text.".into()),
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum RadialStage {
    Pick,
    Location { center: Point, radius: f64 },
    Text { center: Point, radius: f64 },
}

/// DIMRADIUS and DIMDIAMETER: pick an arc or circle, then where the text goes.
pub struct DimRadial {
    diameter: bool,
    stage: RadialStage,
    text: Option<String>,
    scale: f64,
}

impl DimRadial {
    pub fn radius() -> Self {
        Self { diameter: false, stage: RadialStage::Pick, text: None, scale: 1.0 }
    }

    pub fn diameter() -> Self {
        Self { diameter: true, ..Self::radius() }
    }

    /// The dimension line points from the center toward the cursor.
    fn geometry(&self, center: Point, radius: f64, at: Point) -> Geometry {
        let angle = if at.approx_eq(center) { 0.0 } else { center.angle_to(at) };
        Geometry::RadialDimension {
            center,
            point: center.polar(radius, angle),
            text_at: at,
            diameter: self.diameter,
            text: self.text.clone(),
            scale: self.scale,
        }
    }
}

impl Command for DimRadial {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            RadialStage::Pick => Prompt::new("Select arc or circle"),
            RadialStage::Location { .. } => Prompt::new("Specify dimension line location").with_keywords(&["Text"]),
            RadialStage::Text { center, radius } => Prompt::new("Enter dimension text").with_default(radial_value(
                center,
                center.polar(radius, 0.0),
                self.diameter,
            )),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (RadialStage::Pick, Input::Pick { at, .. }) => {
                let (center, radius) = match picked(ctx, at)?.1 {
                    Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => (center, radius),
                    _ => return Err("Select an arc or circle.".into()),
                };
                let value = if self.diameter { 2.0 * radius } else { radius };
                ctx.messages.push(format!("Dimension text = {}", format_measurement(value)));
                self.stage = RadialStage::Location { center, radius };
            }
            (RadialStage::Location { center, radius }, Input::Point(at)) => {
                ctx.doc.add(self.geometry(center, radius, at));
                return Ok(Step::Done);
            }
            (RadialStage::Location { center, radius }, Input::Keyword("Text")) => {
                self.stage = RadialStage::Text { center, radius };
            }
            (RadialStage::Text { center, radius }, input) => {
                text_override(input, &mut self.text)?;
                self.stage = RadialStage::Location { center, radius };
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (RadialStage::Pick, _) => return Err("Select an arc or circle.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            RadialStage::Location { center, radius } => vec![self.geometry(center, radius, cursor)],
            _ => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, RadialStage::Pick)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, RadialStage::Text { .. })
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// What an angular dimension measures, before its arc is placed.
#[derive(Clone, Copy)]
enum Angle {
    /// A vertex and a point on each ray (three points, an arc, or a circle).
    Points { vertex: Point, p1: Point, p2: Point },
    /// Two lines that aren't parallel: which of the four angles between
    /// them depends on where the arc goes.
    Lines { a: (Point, Point), b: (Point, Point) },
}

impl Angle {
    /// Vertex and the two ray points, counterclockwise around the arc's location.
    fn rays(self, at: Point) -> (Point, Point, Point) {
        match self {
            Angle::Points { vertex, p1, p2 } => {
                let (p1, p2) = angle_around(vertex, p1, p2, at);
                (vertex, p1, p2)
            }
            Angle::Lines { a, b } => two_line_angle(a, b, at).unwrap_or((a.0, a.1, b.1)),
        }
    }
}

#[derive(Clone, Copy)]
enum AngularStage {
    Select,
    SecondLine { a: (Point, Point) },
    Vertex,
    First { vertex: Point },
    Second { vertex: Point, p1: Point },
    Location(Angle),
    Text(Angle),
}

/// DIMANGULAR: an arc, a circle and a second point, two lines, or (after
/// Enter) a vertex and two points; then where the dimension arc goes.
pub struct DimAngular {
    stage: AngularStage,
    text: Option<String>,
    scale: f64,
}

impl Default for DimAngular {
    fn default() -> Self {
        Self { stage: AngularStage::Select, text: None, scale: 1.0 }
    }
}

impl DimAngular {
    fn geometry(&self, angle: Angle, at: Point) -> Geometry {
        let (vertex, p1, p2) = angle.rays(at);
        Geometry::AngularDimension { vertex, p1, p2, arc: at, text: self.text.clone(), scale: self.scale }
    }
}

impl Command for DimAngular {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            AngularStage::Select => Prompt::new("Select arc, circle, line, or").with_default("specify vertex"),
            AngularStage::SecondLine { .. } => Prompt::new("Select second line"),
            AngularStage::Vertex => Prompt::new("Specify angle vertex"),
            AngularStage::First { .. } => Prompt::new("Specify first angle endpoint"),
            AngularStage::Second { .. } => Prompt::new("Specify second angle endpoint"),
            AngularStage::Location(_) => Prompt::new("Specify dimension arc line location").with_keywords(&["Text"]),
            AngularStage::Text(_) => Prompt::new("Enter dimension text"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (AngularStage::Select, Input::Pick { at, .. }) => {
                self.stage = match picked(ctx, at)?.1 {
                    Geometry::Arc { center, radius, start_angle, end_angle } => AngularStage::Location(Angle::Points {
                        vertex: center,
                        p1: center.polar(radius, start_angle),
                        p2: center.polar(radius, end_angle),
                    }),
                    // The pick on a circle is the first endpoint.
                    Geometry::Circle { center, radius } => {
                        AngularStage::Second { vertex: center, p1: center.polar(radius, center.angle_to(at)) }
                    }
                    Geometry::Line { start, end } => AngularStage::SecondLine { a: (start, end) },
                    _ => return Err("Select an arc, circle or line.".into()),
                };
            }
            (AngularStage::Select, Input::Enter) => self.stage = AngularStage::Vertex,
            (AngularStage::SecondLine { a }, Input::Pick { at, .. }) => {
                let Geometry::Line { start, end } = picked(ctx, at)?.1 else {
                    return Err("Select a line.".into());
                };
                let b = (start, end);
                two_line_angle(a, b, start).ok_or("Lines are parallel.")?;
                self.stage = AngularStage::Location(Angle::Lines { a, b });
            }
            (AngularStage::Vertex, Input::Point(vertex)) => self.stage = AngularStage::First { vertex },
            (AngularStage::First { vertex }, Input::Point(p1)) if !p1.approx_eq(vertex) => {
                self.stage = AngularStage::Second { vertex, p1 };
            }
            (AngularStage::Second { vertex, p1 }, Input::Point(p2)) if !p2.approx_eq(vertex) => {
                self.stage = AngularStage::Location(Angle::Points { vertex, p1, p2 });
            }
            (AngularStage::Location(angle), Input::Point(at)) => {
                let g = self.geometry(angle, at);
                if let Geometry::AngularDimension { vertex, p1, p2, .. } = &g {
                    ctx.messages
                        .push(format!("Dimension text = {}", format_angle(angular_measurement(*vertex, *p1, *p2))));
                }
                ctx.doc.add(g);
                return Ok(Step::Done);
            }
            (AngularStage::Location(angle), Input::Keyword("Text")) => self.stage = AngularStage::Text(angle),
            (AngularStage::Text(angle), input) => {
                text_override(input, &mut self.text)?;
                self.stage = AngularStage::Location(angle);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (AngularStage::Select | AngularStage::SecondLine { .. }, _) => return Err("Select an object.".into()),
            _ => return Err("Specify a point away from the vertex.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            AngularStage::First { vertex } => vec![Geometry::Line { start: vertex, end: cursor }],
            AngularStage::Second { vertex, p1 } => {
                vec![Geometry::Line { start: vertex, end: p1 }, Geometry::Line { start: vertex, end: cursor }]
            }
            AngularStage::Location(angle) => vec![self.geometry(angle, cursor)],
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            AngularStage::First { vertex } | AngularStage::Second { vertex, .. } => Some(vertex),
            _ => None,
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, AngularStage::Select | AngularStage::SecondLine { .. })
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, AngularStage::Text(_))
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// The dimension a continued or baseline dimension builds on.
#[derive(Clone, Copy)]
struct Base {
    p1: Point,
    p2: Point,
    line: Point,
    rotation: f64,
}

impl Base {
    fn of(g: &Geometry) -> Option<Base> {
        match *g {
            Geometry::Dimension { p1, p2, line, rotation, aligned, .. } => {
                Some(Base { p1, p2, line, rotation: if aligned { p1.angle_to(p2) } else { rotation } })
            }
            _ => None,
        }
    }
}

/// DIMCONTINUE and DIMBASELINE: linear dimensions chained from the last
/// one, end to end or all from its first extension line, stacked.
pub struct DimChain {
    baseline: bool,
    selecting: bool,
    base: Option<Base>,
    /// Earlier bases, for Undo.
    history: Vec<Base>,
    scale: f64,
}

impl DimChain {
    pub fn continued() -> Self {
        Self { baseline: false, selecting: false, base: None, history: Vec::new(), scale: 1.0 }
    }

    pub fn baseline() -> Self {
        Self { baseline: true, ..Self::continued() }
    }

    /// The next dimension, to the new extension line origin `p`.
    fn next(&self, base: Base, p: Point) -> Base {
        if !self.baseline {
            return Base { p1: base.p2, p2: p, ..base };
        }
        // One DIMDLI further out from the measured points.
        let n = Point::ORIGIN.polar(1.0, base.rotation + 90.0);
        let side = if (base.line - base.p1).dot(n) < 0.0 { -1.0 } else { 1.0 };
        Base { p2: p, line: base.line + n * (side * BASELINE_SPACING * self.scale), ..base }
    }

    fn geometry(&self, b: Base) -> Geometry {
        Geometry::Dimension {
            p1: b.p1,
            p2: b.p2,
            line: b.line,
            rotation: b.rotation,
            aligned: false,
            text: None,
            scale: self.scale,
        }
    }
}

impl Command for DimChain {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        if self.selecting {
            let what = if self.baseline { "base" } else { "continued" };
            return Prompt::new(format!("Select {what} dimension"));
        }
        Prompt::new("Specify a second extension line origin").with_keywords(&["Undo", "Select"])
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } if self.selecting => {
                let Some(mut base) = Base::of(&picked(ctx, at)?.1) else {
                    return Err("Select a linear dimension.".into());
                };
                // Chain from the extension line nearest the pick.
                let nearer_first = at.distance(base.p1) < at.distance(base.p2);
                if nearer_first != self.baseline {
                    (base.p1, base.p2) = (base.p2, base.p1);
                }
                self.base = Some(base);
                self.selecting = false;
            }
            Input::Point(p) if !self.selecting => {
                // Without a pick, build on the most recent linear dimension.
                let latest = || ctx.doc.entities().filter_map(|e| Base::of(&e.geometry)).last();
                let base = self.base.or_else(latest).ok_or("No linear dimension to continue; draw one first.")?;
                let next = self.next(base, p);
                ctx.doc.add(self.geometry(next));
                let value = measurement(next.p1, next.p2, next.rotation, false);
                ctx.messages.push(format!("Dimension text = {}", dimension_text(None, value)));
                self.history.push(base);
                self.base = Some(next);
            }
            Input::Keyword("Undo") => {
                if let Some(previous) = self.history.pop() {
                    ctx.doc.undo_last_in_transaction();
                    self.base = Some(previous);
                }
            }
            Input::Keyword("Select") => self.selecting = true,
            Input::Enter => return Ok(Step::Done),
            _ if self.selecting => return Err("Select a linear dimension.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.base {
            Some(base) if !self.selecting => vec![self.geometry(self.next(base, cursor))],
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        self.base.map(|b| if self.baseline { b.p1 } else { b.p2 })
    }

    fn wants_pick(&self) -> bool {
        self.selecting
    }
}

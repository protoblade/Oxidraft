//! DIMORDINATE, DIMARC, DIMJOGGED, QDIM, and CENTERMARK and CENTERLINE.

use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Color, Geometry, Lineweight, annotation::BASELINE_SPACING};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

/// DIMORDINATE: a feature's X or Y by a leader; which one goes by the
/// way the leader runs, unless Xdatum or Ydatum says.
#[derive(Default)]
pub struct DimOrdinate {
    feature: Option<Point>,
    datum: Option<bool>,
    scale: f64,
}

impl DimOrdinate {
    fn dimension(&self, feature: Point, end: Point) -> Geometry {
        let (dx, dy) = ((end.x - feature.x).abs(), (end.y - feature.y).abs());
        let x_datum = self.datum.unwrap_or(dy >= dx);
        Geometry::OrdinateDimension { feature, leader_end: end, x_datum, text: None, scale: self.scale }
    }
}

impl Command for DimOrdinate {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.feature {
            None => Prompt::new("Specify feature location"),
            Some(_) => Prompt::new("Specify leader endpoint").with_keywords(&["Xdatum", "Ydatum"]),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.feature, input) {
            (None, Input::Point(p)) => self.feature = Some(p),
            (Some(_), Input::Keyword(k)) => self.datum = Some(k == "Xdatum"),
            (Some(f), Input::Point(end)) => {
                ctx.doc.add(self.dimension(f, end));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Specify a point.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.feature.map(|f| self.dimension(f, cursor)).into_iter().collect()
    }

    fn base_point(&self) -> Option<Point> {
        self.feature
    }
}

/// DIMARC: an arc's length, its dimension arc where picked.
#[derive(Default)]
pub struct DimArc {
    arc: Option<(Point, f64, f64, f64)>,
    scale: f64,
}

impl DimArc {
    fn dimension(&self, at: Point) -> Option<Geometry> {
        let (center, radius, start_angle, end_angle) = self.arc?;
        let dim_radius = center.distance(at).max(EPSILON);
        Some(Geometry::ArcLengthDimension {
            center,
            radius,
            start_angle,
            end_angle,
            dim_radius,
            text: None,
            scale: self.scale,
        })
    }
}

impl Command for DimArc {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.arc {
            None => Prompt::new("Select arc or polyline arc segment"),
            Some(_) => Prompt::new("Specify arc length dimension location"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.arc, input) {
            (None, Input::Pick { at, .. }) => {
                let g = ctx.pick(at).and_then(|id| ctx.doc.entity(id)).map(|e| e.geometry.clone());
                let arc = match g {
                    Some(Geometry::Arc { center, radius, start_angle, end_angle }) => {
                        (center, radius, start_angle, end_angle)
                    }
                    Some(g @ Geometry::Polyline { .. }) => {
                        let seg = g.segments().into_iter().find_map(|s| match s {
                            oxidraft_geom::curve::Seg::Arc(a) if s.distance_to(at) <= ctx.pick_tolerance => Some(a),
                            _ => None,
                        });
                        let a = seg.ok_or("Pick an arc segment of the polyline.")?;
                        let (s, e) = a.ccw_angles();
                        (a.center, a.radius, s, e)
                    }
                    _ => return Err("Select an arc.".into()),
                };
                self.arc = Some(arc);
            }
            (Some(_), Input::Point(p)) => {
                let g = self.dimension(p).ok_or("Select an arc first.")?;
                ctx.doc.add(g);
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Select an arc, then where the dimension goes.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.dimension(cursor).into_iter().collect()
    }

    fn wants_pick(&self) -> bool {
        self.arc.is_none()
    }
}

/// DIMJOGGED: a large arc's radius, drawn from a stand-in center with a
/// zigzag.
#[derive(Default)]
pub struct DimJogged {
    circle: Option<(Point, f64)>,
    center_override: Option<Point>,
    point: Option<Point>,
    scale: f64,
}

impl Command for DimJogged {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match (self.circle, self.center_override, self.point) {
            (None, ..) => Prompt::new("Select arc or circle"),
            (_, None, _) => Prompt::new("Specify center location override"),
            (_, _, None) => Prompt::new("Specify dimension line location"),
            _ => Prompt::new("Specify jog location"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } if self.circle.is_none() => {
                let g = ctx.pick(at).and_then(|id| ctx.doc.entity(id)).map(|e| e.geometry.clone());
                match g {
                    Some(Geometry::Arc { center, radius, .. } | Geometry::Circle { center, radius }) => {
                        self.circle = Some((center, radius));
                    }
                    _ => return Err("Select an arc or circle.".into()),
                }
            }
            Input::Point(p) if self.circle.is_some() => {
                let (center, radius) = self.circle.unwrap_or((Point::ORIGIN, 1.0));
                match (self.center_override, self.point) {
                    (None, _) => self.center_override = Some(p),
                    (Some(_), None) => self.point = Some(center.polar(radius, center.angle_to(p))),
                    (Some(o), Some(point)) => {
                        let g = Geometry::JoggedDimension {
                            center,
                            point,
                            center_override: o,
                            jog: p,
                            text: None,
                            scale: self.scale,
                        };
                        ctx.doc.add(g);
                        return Ok(Step::Done);
                    }
                }
            }
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Select an arc or circle, then points.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match (self.circle, self.center_override, self.point) {
            (Some((center, _)), Some(o), Some(point)) => {
                vec![Geometry::JoggedDimension {
                    center,
                    point,
                    center_override: o,
                    jog: cursor,
                    text: None,
                    scale: self.scale,
                }]
            }
            (Some(_), Some(o), None) => vec![Geometry::Line { start: o, end: cursor }],
            _ => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        self.circle.is_none()
    }
}

/// QDIM: dimensions along the points of the objects selected, side by side
/// (Continuous) or all from the first (Baseline); horizontal or vertical
/// by where the dimension line goes.
pub struct QDim {
    points: Vec<Point>,
    baseline: bool,
    selecting: bool,
    scale: f64,
}

impl Default for QDim {
    fn default() -> Self {
        Self { points: Vec::new(), baseline: false, selecting: true, scale: 1.0 }
    }
}

impl QDim {
    fn dimensions(&self, at: Point) -> Vec<Geometry> {
        let Some(&first) = self.points.first() else { return Vec::new() };
        let (lo, hi) = self.points.iter().fold((first, first), |(a, b), p| {
            (Point::new(a.x.min(p.x), a.y.min(p.y)), Point::new(b.x.max(p.x), b.y.max(p.y)))
        });
        // Above or below the points: horizontal dimensions; beside them: vertical.
        let horizontal = (at.y > hi.y || at.y < lo.y) || (at.x >= lo.x && at.x <= hi.x);
        let key = |p: &Point| if horizontal { p.x } else { p.y };
        let mut points = self.points.clone();
        points.sort_by(|a, b| key(a).total_cmp(&key(b)));
        points.dedup_by(|a, b| (key(a) - key(b)).abs() < 1e-6);
        let rotation = if horizontal { 0.0 } else { 90.0 };
        let away = if horizontal {
            if at.y >= lo.y { 1.0 } else { -1.0 }
        } else if at.x >= lo.x {
            1.0
        } else {
            -1.0
        };
        let step = BASELINE_SPACING * self.scale * away;
        let dim = |p1: Point, p2: Point, line: Point| Geometry::Dimension {
            extra: Default::default(),
            p1,
            p2,
            line,
            rotation,
            aligned: false,
            text: None,
            scale: self.scale,
        };
        let mut out = Vec::new();
        for (i, w) in points.windows(2).enumerate() {
            let out_dim = if self.baseline {
                let shift = step * i as f64;
                let line = if horizontal { Point::new(at.x, at.y + shift) } else { Point::new(at.x + shift, at.y) };
                dim(points[0], w[1], line)
            } else {
                dim(w[0], w[1], at)
            };
            out.push(out_dim);
        }
        out
    }
}

/// The points QDIM dimensions: ends of lines, arcs and polyline segments,
/// and circles' centers.
fn feature_points(g: &Geometry) -> Vec<Point> {
    match g {
        Geometry::Circle { center, .. } => vec![*center],
        Geometry::Point { position } => vec![*position],
        _ => g.segments().iter().flat_map(|s| [s.start(), s.end()]).collect(),
    }
}

impl Command for QDim {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        if self.selecting {
            Prompt::new("Select geometry to dimension")
        } else {
            Prompt::new("Specify dimension line position")
                .with_keywords(&["Continuous", "Baseline"])
                .with_default(if self.baseline { "Baseline" } else { "Continuous" })
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Selection(s) if self.selecting => {
                self.points = s
                    .ids
                    .iter()
                    .filter_map(|id| ctx.doc.entity(*id))
                    .flat_map(|e| feature_points(&e.geometry))
                    .collect();
                if self.points.len() < 2 {
                    return Ok(Step::Done);
                }
                self.selecting = false;
            }
            Input::Keyword(k) => self.baseline = k == "Baseline",
            Input::Point(at) => {
                for g in self.dimensions(at) {
                    ctx.doc.add(g);
                }
                return Ok(Step::Done);
            }
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Select geometry, then where the dimensions go.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        if self.selecting { Vec::new() } else { self.dimensions(cursor) }
    }

    fn wants_selection(&self) -> bool {
        self.selecting
    }
}

/// How far center lines reach past what they mark (CENTEREXE).
const CENTER_EXTENSION: f64 = 3.5;

/// Lines in the CENTER linetype, grouped as one mark.
fn add_center_lines(ctx: &mut Ctx, lines: &[(Point, Point)]) {
    let layer = ctx.doc.current_layer().to_string();
    let ids: Vec<_> = lines
        .iter()
        .map(|&(start, end)| {
            ctx.doc.add_styled(Geometry::Line { start, end }, &layer, Color::ByLayer, "CENTER", Lineweight::ByLayer)
        })
        .collect();
    let _ = ctx.doc.add_group("", "Center mark", ids);
}

/// CENTERMARK: a cross of center lines through each circle or arc picked.
#[derive(Default)]
pub struct CenterMark;

impl Command for CenterMark {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select circle or arc to add centermark")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } => {
                let g = ctx.pick(at).and_then(|id| ctx.doc.entity(id)).map(|e| e.geometry.clone());
                let Some(Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. }) = g else {
                    return Err("Select a circle or arc.".into());
                };
                let reach = radius + CENTER_EXTENSION;
                let lines = [
                    (center - Point::new(reach, 0.0), center + Point::new(reach, 0.0)),
                    (center - Point::new(0.0, reach), center + Point::new(0.0, reach)),
                ];
                add_center_lines(ctx, &lines);
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select a circle or arc.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

/// CENTERLINE: the line midway between two lines, reaching past their ends.
#[derive(Default)]
pub struct CenterLine {
    first: Option<(Point, Point)>,
}

impl Command for CenterLine {
    fn prompt(&self) -> Prompt {
        Prompt::new(if self.first.is_none() { "Select first line" } else { "Select second line" })
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let line = |ctx: &Ctx, at: Point| match ctx.pick(at).and_then(|id| ctx.doc.entity(id)).map(|e| &e.geometry) {
            Some(Geometry::Line { start, end }) => Some((*start, *end)),
            _ => None,
        };
        match input {
            Input::Pick { at, .. } => {
                let (s2, e2) = line(ctx, at).ok_or("Select a line.")?;
                let Some((s1, e1)) = self.first else {
                    self.first = Some((s2, e2));
                    return Ok(Step::Continue);
                };
                // Pair the ends that are nearer each other.
                let (s2, e2) = if s1.distance(s2) + e1.distance(e2) <= s1.distance(e2) + e1.distance(s2) {
                    (s2, e2)
                } else {
                    (e2, s2)
                };
                let (a, b) = (s1.midpoint(s2), e1.midpoint(e2));
                let len = a.distance(b);
                if len <= EPSILON {
                    return Err("The lines' midpoints meet.".into());
                }
                let u = (b - a) * (1.0 / len);
                add_center_lines(ctx, &[(a - u * CENTER_EXTENSION, b + u * CENTER_EXTENSION)]);
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select a line.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
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

    fn click(e: &mut Engine, d: &mut Document, x: f64, y: f64) {
        let p = Point::new(x, y);
        e.click(p, p, false, d, 0.5);
    }

    #[test]
    fn ordinate_arc_and_jogged_dimensions() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["dimordinate", "30,20", "30,40"]);
        assert!(matches!(
            d.entities().last().map(|x| &x.geometry),
            Some(Geometry::OrdinateDimension { x_datum: true, .. })
        ));
        run(&mut e, &mut d, &["arc", "c", "0,0", "10,0", "0,10"]);
        run(&mut e, &mut d, &["dimarc"]);
        click(&mut e, &mut d, 7.0711, 7.0711);
        run(&mut e, &mut d, &["15,15"]);
        let arc = d.entities().last().unwrap().geometry.clone();
        let Geometry::ArcLengthDimension { radius, start_angle, end_angle, .. } = arc else { panic!("{arc:?}") };
        let length = radius * (end_angle - start_angle).rem_euclid(360.0).to_radians();
        assert!((length - 15.7080).abs() < 1e-3, "{length}");
        run(&mut e, &mut d, &["dimjogged"]);
        click(&mut e, &mut d, 0.0, 10.0);
        run(&mut e, &mut d, &["-20,-20", "5,9", "-5,-5"]);
        assert!(matches!(d.entities().last().map(|x| &x.geometry), Some(Geometry::JoggedDimension { .. })));
    }

    #[test]
    fn qdim_dimensions_along_the_points() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["l", "0,0", "10,0", "25,0", ""]);
        e.select_all(&d);
        run(&mut e, &mut d, &["qdim", "5,-10"]);
        let dims = d.entities().filter(|x| matches!(x.geometry, Geometry::Dimension { .. })).count();
        assert_eq!(dims, 2, "0-10 and 10-25");
    }

    #[test]
    fn center_marks_and_lines_are_grouped_center_lines() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["c", "0,0", "5"]);
        run(&mut e, &mut d, &["centermark"]);
        click(&mut e, &mut d, 5.0, 0.0);
        run(&mut e, &mut d, &[""]);
        let lines: Vec<_> = d.entities().filter(|x| matches!(x.geometry, Geometry::Line { .. })).collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.linetype == "CENTER"));
        assert_eq!(d.groups().len(), 1);
        run(&mut e, &mut d, &["l", "20,0", "40,0", "", "l", "20,10", "40,10", ""]);
        run(&mut e, &mut d, &["centerline"]);
        click(&mut e, &mut d, 30.0, 0.0);
        click(&mut e, &mut d, 30.0, 10.0);
        let mid = d.entities().find_map(|x| match x.geometry {
            Geometry::Line { start, end } if (start.y - 5.0).abs() < 1e-9 => Some((start, end)),
            _ => None,
        });
        let (start, end) = mid.expect("the centerline");
        assert!((start.x - 16.5).abs() < 1e-9 && (end.x - 43.5).abs() < 1e-9);
    }
}

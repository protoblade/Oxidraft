//! ALIGN (AL): moves, turns and optionally scales objects so two source
//! points land on two destination points; REVERSE turns curves round;
//! ARRAYPATH copies objects along a path.

use oxidraft_geom::{Aabb, EPSILON, Point};
use oxidraft_model::{EntityId, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step, break_join::reversed, point::at_length};

#[derive(Clone, PartialEq)]
enum AlignStage {
    Select,
    FirstSource,
    FirstDestination(Point),
    SecondSource(Point, Point),
    SecondDestination(Point, Point, Point),
    Scale([Point; 4]),
}

/// ALIGN: one pair of points moves the objects; two pairs move and turn
/// them, and scale them too if asked.
pub struct Align {
    stage: AlignStage,
    ids: Vec<EntityId>,
}

impl Default for Align {
    fn default() -> Self {
        Self { stage: AlignStage::Select, ids: Vec::new() }
    }
}

impl Align {
    fn apply(&self, ctx: &mut Ctx, s1: Point, d1: Point, turn: f64, factor: f64) -> Step {
        for &id in &self.ids {
            let Some(g) = ctx.doc.entity(id).map(|e| e.geometry.clone()) else { continue };
            let mut moved = g.translated(d1 - s1).rotated(d1, turn);
            if (factor - 1.0).abs() > EPSILON {
                moved = moved.scaled(d1, factor);
            }
            ctx.doc.modify(id, moved);
        }
        Step::Done
    }
}

impl Command for Align {
    fn prompt(&self) -> Prompt {
        match self.stage {
            AlignStage::Select => Prompt::new("Select objects"),
            AlignStage::FirstSource => Prompt::new("Specify first source point"),
            AlignStage::FirstDestination(_) => Prompt::new("Specify first destination point"),
            AlignStage::SecondSource(..) => Prompt::new("Specify second source point").with_default("continue"),
            AlignStage::SecondDestination(..) => Prompt::new("Specify second destination point"),
            AlignStage::Scale(_) => {
                Prompt::new("Scale objects based on alignment points?").with_keywords(&["Yes", "No"]).with_default("N")
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (AlignStage::Select, Input::Selection(s)) if s.ids.is_empty() => return Ok(Step::Done),
            (AlignStage::Select, Input::Selection(s)) => {
                self.ids = s.ids;
                self.stage = AlignStage::FirstSource;
            }
            (AlignStage::FirstSource, Input::Point(p)) => self.stage = AlignStage::FirstDestination(p),
            (AlignStage::FirstDestination(s1), Input::Point(p)) => self.stage = AlignStage::SecondSource(s1, p),
            // One pair: a move.
            (AlignStage::SecondSource(s1, d1), Input::Enter) => return Ok(self.apply(ctx, s1, d1, 0.0, 1.0)),
            (AlignStage::SecondSource(s1, d1), Input::Point(p)) => {
                self.stage = AlignStage::SecondDestination(s1, d1, p)
            }
            (AlignStage::SecondDestination(s1, d1, s2), Input::Point(p)) => {
                self.stage = AlignStage::Scale([s1, d1, s2, p]);
            }
            (AlignStage::Scale([s1, d1, s2, d2]), input) => {
                let scale = input == Input::Keyword("Yes");
                let turn = d1.angle_to(d2) - s1.angle_to(s2);
                let (from, to) = (s1.distance(s2), d1.distance(d2));
                let factor = if scale && from > EPSILON { to / from } else { 1.0 };
                return Ok(self.apply(ctx, s1, d1, turn, factor));
            }
            (AlignStage::Select, _) => return Err("Select objects, then press Enter.".into()),
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Specify a point.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            AlignStage::FirstDestination(s1) => vec![Geometry::Line { start: s1, end: cursor }],
            AlignStage::SecondSource(s1, d1) => vec![Geometry::Line { start: s1, end: d1 }],
            AlignStage::SecondDestination(s1, d1, s2) => {
                vec![Geometry::Line { start: s1, end: d1 }, Geometry::Line { start: s2, end: cursor }]
            }
            _ => Vec::new(),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.ids.clone()
    }

    fn wants_selection(&self) -> bool {
        self.stage == AlignStage::Select
    }
}

/// A curve run the other way, if it can be: a line, polyline or spline.
pub fn reverse(g: &Geometry) -> Option<Geometry> {
    match g {
        Geometry::Line { start, end } => Some(Geometry::Line { start: *end, end: *start }),
        Geometry::Polyline { vertices, closed } => {
            let width = oxidraft_model::constant_width(vertices).unwrap_or(0.0);
            Some(Geometry::polyline_from_segs(&reversed(&g.segments()), *closed).with_width(width))
        }
        Geometry::Spline(s) => s.reversed().map(Geometry::Spline),
        _ => None,
    }
}

/// REVERSE: lines, polylines and splines run the other way (which way
/// their linetypes' text and dashes go).
#[derive(Default)]
pub struct Reverse;

impl Command for Reverse {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select lines, polylines, or splines to reverse direction")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Err("Select objects, then press Enter.".into()) };
        let mut count = 0;
        for id in s.ids {
            if let Some(g) = ctx.doc.entity(id).and_then(|e| reverse(&e.geometry)) {
                ctx.doc.modify(id, g);
                count += 1;
            }
        }
        ctx.messages.push(format!("{count} object(s) direction has been reversed."));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

#[derive(Clone, PartialEq)]
enum PathStage {
    Select,
    Path,
    Count(EntityId),
    Associative(EntityId),
    /// Divide or Measure, and Measure's distance.
    Method(EntityId),
    Spacing(EntityId),
    Turn(EntityId, usize),
}

/// ARRAYPATH: copies of the objects spaced evenly along a path curve, the
/// first at its start (where the objects themselves go), turned with the
/// path or not; one associative array while ARRAYASSOCIATIVITY is on.
pub struct ArrayPath {
    stage: PathStage,
    ids: Vec<EntityId>,
    associative: Option<bool>,
    /// Measure: items this far apart; Divide (`None`) spreads them over it.
    spacing: Option<f64>,
    /// The path's length.
    length: f64,
}

impl Default for ArrayPath {
    fn default() -> Self {
        Self { stage: PathStage::Select, ids: Vec::new(), associative: None, spacing: None, length: 0.0 }
    }
}

impl ArrayPath {
    /// Measure's count when it fills the path.
    fn fill_count(&self) -> usize {
        match self.spacing {
            Some(d) if d > EPSILON => ((self.length / d + 1e-9).floor() as usize + 1).max(1),
            _ => 6,
        }
    }

    fn array(&self, ctx: &mut Ctx, path: EntityId, count: usize, turn: bool) -> Result<Step, String> {
        let curve = ctx.doc.entity(path).map(|e| e.geometry.clone()).ok_or("The path is gone.")?;
        let segs = curve.segments();
        let length: f64 = segs.iter().map(|s| s.length()).sum();
        if length <= EPSILON {
            return Err("The path has no length.".into());
        }
        if self.associative.unwrap_or(ctx.vars.array_assoc) {
            let layout = oxidraft_model::assoc_array::ArrayLayout::Path {
                path: Box::new(curve),
                items: count,
                align: turn,
                spacing: self.spacing,
            };
            ctx.doc.make_array(&self.ids, layout);
            return Ok(Step::Done);
        }
        let gaps = if curve.is_closed() { count } else { count - 1 };
        let step = self.spacing.unwrap_or(length / gaps.max(1) as f64);
        let (_, start_angle) = at_length(&segs, 0.0).ok_or("The path has no length.")?;
        let objects: Vec<oxidraft_model::Entity> =
            self.ids.iter().filter_map(|id| ctx.doc.entity(*id).cloned()).collect();
        let Some(base) = objects.iter().map(|e| ctx.doc.entity_bounds(e)).reduce(Aabb::union).map(|b| b.center())
        else {
            return Ok(Step::Done);
        };
        for i in 0..count {
            let Some((at, angle)) = at_length(&segs, step * i as f64) else { continue };
            for e in &objects {
                let mut g = e.geometry.translated(at - base);
                if turn {
                    g = g.rotated(at, angle - start_angle);
                }
                ctx.doc.add_like(g, e);
            }
        }
        for id in &self.ids {
            ctx.doc.remove(*id);
        }
        Ok(Step::Done)
    }
}

impl Command for ArrayPath {
    fn prompt(&self) -> Prompt {
        match self.stage {
            PathStage::Select => Prompt::new("Select objects"),
            PathStage::Path => Prompt::new("Select path curve"),
            PathStage::Count(_) if self.spacing.is_some() => Prompt::new("Enter number of items along path")
                .with_keywords(&["Fill entire path", "ASsociative", "Method"])
                .with_default(self.fill_count().to_string()),
            PathStage::Count(_) => Prompt::new("Enter number of items along path")
                .with_keywords(&["ASsociative", "Method"])
                .with_default("6"),
            PathStage::Method(_) => Prompt::new("Enter path method")
                .with_keywords(&["Divide", "Measure"])
                .with_default(if self.spacing.is_some() { "Measure" } else { "Divide" }),
            PathStage::Spacing(_) => Prompt::new("Specify the distance between items along path")
                .with_default(format!("{:.4}", self.length / 5.0)),
            PathStage::Associative(_) => Prompt::new("Create associative array")
                .with_keywords(&["Yes", "No"])
                .with_default(if self.associative == Some(false) { "No" } else { "Yes" }),
            PathStage::Turn(..) => {
                Prompt::new("Align items to the path?").with_keywords(&["Yes", "No"]).with_default("Yes")
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (PathStage::Select, Input::Selection(s)) if s.ids.is_empty() => return Ok(Step::Done),
            (PathStage::Select, Input::Selection(s)) => {
                self.ids = s.ids;
                self.stage = PathStage::Path;
            }
            (PathStage::Path, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                let fits =
                    ctx.doc.entity(id).is_some_and(|e| !e.geometry.is_infinite() && !e.geometry.segments().is_empty());
                if !fits || self.ids.contains(&id) {
                    return Err("Choose a line, arc, circle, polyline, ellipse or spline to follow.".into());
                }
                self.length =
                    ctx.doc.entity(id).map_or(0.0, |e| e.geometry.segments().iter().map(|s| s.length()).sum());
                self.stage = PathStage::Count(id);
            }
            (PathStage::Count(path), Input::Keyword("Method")) => self.stage = PathStage::Method(path),
            (PathStage::Count(path), Input::Keyword("Fill entire path")) => {
                self.stage = PathStage::Turn(path, self.fill_count());
            }
            (PathStage::Method(path), Input::Keyword("Measure")) => self.stage = PathStage::Spacing(path),
            // Enter keeps the method there is.
            (PathStage::Method(path), Input::Enter) if self.spacing.is_some() => self.stage = PathStage::Spacing(path),
            (PathStage::Method(path), Input::Keyword(_) | Input::Enter) => {
                self.spacing = None;
                self.stage = PathStage::Count(path);
            }
            (PathStage::Spacing(path), Input::Value(d)) if d > EPSILON => {
                self.spacing = Some(d);
                self.stage = PathStage::Count(path);
            }
            (PathStage::Spacing(path), Input::Enter) => {
                self.spacing = Some(self.length / 5.0);
                self.stage = PathStage::Count(path);
            }
            (PathStage::Spacing(_), _) => return Err("Requires a distance greater than 0.".into()),
            (PathStage::Count(path), Input::Value(v)) if v >= 1.0 && v.fract() == 0.0 => {
                self.stage = PathStage::Turn(path, v as usize);
            }
            (PathStage::Count(path), Input::Enter) => {
                let n = if self.spacing.is_some() { self.fill_count() } else { 6 };
                self.stage = PathStage::Turn(path, n);
            }
            (PathStage::Count(path), Input::Keyword("ASsociative")) => self.stage = PathStage::Associative(path),
            (PathStage::Associative(path), input @ (Input::Keyword(_) | Input::Enter)) => {
                self.associative = Some(input != Input::Keyword("No"));
                self.stage = PathStage::Count(path);
            }
            (PathStage::Count(_), _) => return Err("Requires a whole number of 1 or more.".into()),
            (PathStage::Turn(path, count), input) => {
                return self.array(ctx, path, count, input != Input::Keyword("No"));
            }
            (PathStage::Select, _) => return Err("Select objects, then press Enter.".into()),
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Select the path.".into()),
        }
        Ok(Step::Continue)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.ids.clone()
    }

    fn wants_selection(&self) -> bool {
        self.stage == PathStage::Select
    }

    fn wants_pick(&self) -> bool {
        self.stage == PathStage::Path
    }
}

/// CHSPACE: the objects selected go from paper space into model space
/// through a viewport, or the other way; the app knows the viewport.
#[derive(Default)]
pub struct ChangeSpace;

impl Command for ChangeSpace {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Err("Select objects, then press Enter.".into()) };
        if !s.ids.is_empty() {
            ctx.ui = Some(crate::UiRequest::ChangeSpace(s.ids));
        }
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

/// WMFOUT: the objects selected, for the app to write as a Windows
/// Metafile.
#[derive(Default)]
pub struct WmfOut;

impl Command for WmfOut {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Err("Select objects, then press Enter.".into()) };
        if !s.ids.is_empty() {
            ctx.ui = Some(crate::UiRequest::WmfOut(s.ids));
        }
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
    use oxidraft_model::{Document, Geometry, Vertex};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn align_moves_turns_and_scales() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        e.select_all(&d);
        run(&mut e, &mut d, &["al", "0,0", "5,5", "10,0", "5,25", "y"]);
        let Some(Geometry::Line { start, end }) = d.entities().next().map(|x| x.geometry.clone()) else { panic!() };
        assert!(start.approx_eq(Point::new(5.0, 5.0)) && end.approx_eq(Point::new(5.0, 25.0)), "{start:?} {end:?}");
    }

    #[test]
    fn reverse_turns_lines_and_polylines_round() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        d.add(Geometry::Polyline {
            vertices: vec![
                Vertex::at(Point::new(0.0, 5.0)),
                Vertex::at(Point::new(5.0, 5.0)),
                Vertex::at(Point::new(5.0, 9.0)),
            ],
            closed: false,
        });
        e.select_all(&d);
        run(&mut e, &mut d, &["reverse"]);
        let shapes: Vec<Geometry> = d.entities().map(|x| x.geometry.clone()).collect();
        assert!(matches!(&shapes[0], Geometry::Line { start, .. } if start.approx_eq(Point::new(10.0, 0.0))));
        assert!(
            matches!(&shapes[1], Geometry::Polyline { vertices, .. } if vertices[0].point.approx_eq(Point::new(5.0, 9.0)))
        );
    }

    #[test]
    fn arraypath_spaces_copies_along_a_line() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["c", "0,10", "1"]);
        let path = d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(30.0, 0.0) });
        let circle = d.entities().find(|x| x.id != path).unwrap().id;
        e.set_selection(vec![circle]);
        e.vars.array_assoc = false;
        run(&mut e, &mut d, &["arraypath"]);
        let on_path = Point::new(15.0, 0.0);
        e.click(on_path, on_path, false, &mut d, 0.5);
        run(&mut e, &mut d, &["4", "n"]);
        let mut centers: Vec<f64> = d
            .entities()
            .filter_map(|x| match x.geometry {
                Geometry::Circle { center, .. } => Some(center.x),
                _ => None,
            })
            .collect();
        centers.sort_by(f64::total_cmp);
        assert_eq!(centers, [0.0, 10.0, 20.0, 30.0]);
    }

    #[test]
    fn arraypath_measures_and_its_count_grip() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["c", "0,0", "1"]);
        let path = d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(35.0, 0.0) });
        let circle = d.entities().find(|x| x.id != path).unwrap().id;
        e.set_selection(vec![circle]);
        run(&mut e, &mut d, &["arraypath"]);
        let on_path = Point::new(15.0, 0.0);
        e.click(on_path, on_path, false, &mut d, 0.5);
        // Measure: 10 apart, as many as fit (0, 10, 20, 30).
        run(&mut e, &mut d, &["m", "measure", "10", "", "n"]);
        let array = d.entities().find(|x| x.array.is_some()).unwrap().id;
        let centers = |d: &Document| {
            let mut x: Vec<i64> = d
                .expanded(d.entity(array).unwrap())
                .iter()
                .filter_map(
                    |g| if let Geometry::Circle { center, .. } = g { Some(center.x.round() as i64) } else { None },
                )
                .collect();
            x.sort();
            x
        };
        assert_eq!(centers(&d), [0, 10, 20, 30], "{:?}", e.history());
        // The count grip on the last item: dragged back to 10, two items.
        e.set_selection(vec![array]);
        let last = *e.grips(&d).last().unwrap();
        assert!(last.approx_eq(Point::new(30.0, 0.0)), "{last:?}");
        e.click(last, last, false, &mut d, 0.5);
        let to = Point::new(11.0, 3.0);
        e.click(to, to, false, &mut d, 0.5);
        assert_eq!(centers(&d), [0, 10]);
        // ARRAYEDIT back to Divide: two items over the whole path.
        e.set_selection(Vec::new());
        run(&mut e, &mut d, &["arrayedit"]);
        let at = Point::new(0.0, 1.0);
        e.click(at, at, false, &mut d, 0.5);
        run(&mut e, &mut d, &["method", "divide", "x"]);
        assert_eq!(centers(&d), [0, 35]);
    }
}

//! SPLINEDIT: closes or opens a spline, joins curves to its ends, edits
//! its fit points or control vertices, raises its degree, reverses it or
//! turns it into a polyline. Undo steps back one edit at a time.

use oxidraft_geom::{Point, curve::Seg};
use oxidraft_model::{EntityId, Geometry, Spline};

use crate::{Command, Ctx, Input, Prompt, Step, break_join::JOIN_TOL};

/// AutoCAD's highest order (degree + 1).
const MAX_ORDER: usize = 26;

#[derive(Clone, Copy, PartialEq)]
enum FitOp {
    Add,
    Delete,
    Move,
}

#[derive(Clone, Copy, PartialEq)]
enum VertexOp {
    Delete,
    Move,
    Weight,
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Select,
    Main,
    Join,
    Precision,
    Fit,
    /// "Specify existing fit point".
    FitPoint(FitOp),
    /// A new fit point after this one, or where this one moves to.
    FitNew(FitOp, usize),
    Vertex,
    /// "Specify a point on the spline" for a vertex to go.
    VertexAdd,
    VertexPick(VertexOp),
    VertexNew(VertexOp, usize),
    Order,
}

pub struct SplinEdit {
    stage: Stage,
    id: Option<EntityId>,
    /// The spline as it is now (for prompts and previews).
    current: Option<Spline>,
    /// The drawing's change count before each edit, for Undo.
    marks: Vec<usize>,
}

impl Default for SplinEdit {
    fn default() -> Self {
        Self { stage: Stage::Select, id: None, current: None, marks: Vec::new() }
    }
}

/// A curve as one spline: splines as they are; lines, arcs and polylines
/// piece by piece, exactly (raised to the highest degree among them).
pub(crate) fn spline_of(g: &Geometry) -> Option<Spline> {
    if let Geometry::Spline(s) = g {
        return Some(s.clone());
    }
    let closed = matches!(g, Geometry::Polyline { closed: true, .. });
    let pieces = g
        .segments()
        .iter()
        .map(|s| match s {
            Seg::Line(l) => Spline::line(l.start, l.end),
            Seg::Arc(a) => Spline::arc(a),
        })
        .collect::<Option<Vec<_>>>()?;
    Some(chained(pieces)?.with_closed(closed))
}

/// Curves end to end, in order, as one.
fn chained(pieces: Vec<Spline>) -> Option<Spline> {
    let degree = pieces.iter().map(Spline::degree).max()?;
    let mut pieces = pieces.into_iter().map(|s| {
        let (a, b) = s.domain();
        s.with_degree(degree)?.sub(a, b)
    });
    let mut out = pieces.next()??;
    for next in pieces {
        out = out.joined(&next?)?;
    }
    Some(out)
}

/// The index of the point in `points` nearest `p`.
fn nearest(points: &[Point], p: Point) -> Option<usize> {
    (0..points.len()).min_by(|&i, &j| points[i].distance(p).total_cmp(&points[j].distance(p)))
}

impl SplinEdit {
    fn spline(&self, ctx: &Ctx) -> Option<Spline> {
        match &ctx.doc.entity(self.id?)?.geometry {
            Geometry::Spline(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Puts `s` in the drawing as one edit Undo can take back.
    fn set(&mut self, ctx: &mut Ctx, g: Geometry) {
        let Some(id) = self.id else { return };
        self.marks.push(ctx.doc.changes_in_transaction());
        ctx.doc.modify(id, g);
    }

    fn edit(&mut self, ctx: &mut Ctx, s: Option<Spline>) -> Result<(), String> {
        let s = s.ok_or("The spline can't be changed that way.")?;
        self.set(ctx, Geometry::Spline(s));
        Ok(())
    }

    fn fit_edit(&mut self, ctx: &mut Ctx, fit: &[Point], closed: bool) -> Result<(), String> {
        let least = if closed { 3 } else { 2 };
        if fit.len() < least {
            return Err(format!("A spline needs at least {least} fit points."));
        }
        self.edit(ctx, Spline::through(fit, closed))
    }

    /// Close or Open: a fit curve fitted again; otherwise its control
    /// vertices, clamped at the ends or wrapped round.
    fn toggle_closed(&mut self, ctx: &mut Ctx, s: &Spline) -> Result<(), String> {
        let closed = !s.is_closed();
        if !s.fit().is_empty() {
            return self.fit_edit(ctx, s.fit(), closed);
        }
        let (mut cv, mut w) = s.control_vertices();
        if !closed && cv.len() > 2 && cv[0].approx_eq(cv[cv.len() - 1]) {
            cv.pop();
            w.pop();
        }
        let w = if s.is_rational() { w } else { Vec::new() };
        self.edit(ctx, Spline::uniform(s.degree(), &cv, &w, closed))
    }

    /// Joins the selected open curves that meet the spline's ends.
    fn join(&mut self, ctx: &mut Ctx, ids: &[EntityId], s: &Spline) -> Result<(), String> {
        if s.is_closed() {
            return Err("Cannot join to a closed spline.".into());
        }
        let mut pieces: Vec<(EntityId, Spline)> = ids
            .iter()
            .filter(|&&id| Some(id) != self.id)
            .filter_map(|&id| {
                let g = &ctx.doc.entity(id)?.geometry;
                let open = match g {
                    Geometry::Line { .. } | Geometry::Arc { .. } => true,
                    Geometry::Polyline { closed, .. } => !closed,
                    Geometry::Spline(s) => !s.is_closed(),
                    _ => false,
                };
                Some((id, spline_of(g).filter(|_| open)?))
            })
            .collect();
        let mut chain = vec![s.clone()];
        let mut joined = Vec::new();
        while let Some(i) = pieces.iter().position(|(_, p)| {
            let (start, end) = (chain[0].start_point(), chain[chain.len() - 1].end_point());
            [p.start_point(), p.end_point()]
                .iter()
                .any(|q| q.distance(start) <= JOIN_TOL || q.distance(end) <= JOIN_TOL)
        }) {
            let (id, p) = pieces.remove(i);
            let end = chain[chain.len() - 1].end_point();
            let start = chain[0].start_point();
            let closes = chain.len() > 1 && start.distance(end) <= JOIN_TOL;
            if closes {
                break;
            }
            if p.start_point().distance(end) <= JOIN_TOL {
                chain.push(p);
            } else if p.end_point().distance(end) <= JOIN_TOL {
                chain.push(p.reversed().ok_or("Can't join that.")?);
            } else if p.end_point().distance(start) <= JOIN_TOL {
                chain.insert(0, p);
            } else {
                chain.insert(0, p.reversed().ok_or("Can't join that.")?);
            }
            joined.push(id);
        }
        if joined.is_empty() {
            ctx.messages.push("0 objects joined: none meet the spline's ends.".into());
            return Ok(());
        }
        let ends_meet = chain[0].start_point().distance(chain[chain.len() - 1].end_point()) <= JOIN_TOL;
        let whole = chained(chain).ok_or("Can't join those.")?.with_closed(ends_meet);
        self.marks.push(ctx.doc.changes_in_transaction());
        ctx.doc.modify(self.id.ok_or("No spline.")?, Geometry::Spline(whole));
        for id in &joined {
            ctx.doc.remove(*id);
        }
        ctx.messages.push(format!("{} object(s) joined to the spline.", joined.len()));
        Ok(())
    }

    fn main_input(&mut self, input: Input, ctx: &mut Ctx, s: Spline) -> Result<Step, String> {
        match input {
            Input::Keyword("Close" | "Open") => self.toggle_closed(ctx, &s)?,
            Input::Keyword("Join") => self.stage = Stage::Join,
            Input::Keyword("Fit data") => self.stage = Stage::Fit,
            Input::Keyword("Edit vertex") => self.stage = Stage::Vertex,
            Input::Keyword("convert to Polyline") => self.stage = Stage::Precision,
            Input::Keyword("Reverse") => {
                self.edit(ctx, s.reversed())?;
                ctx.messages.push("Spline direction reversed.".into());
            }
            Input::Keyword("Undo") => match self.marks.pop() {
                Some(mark) => while ctx.doc.changes_in_transaction() > mark && ctx.doc.undo_last_in_transaction() {},
                None => ctx.messages.push("All changes have been undone.".into()),
            },
            Input::Keyword(_) | Input::Enter => return Ok(Step::Done),
            _ => return Err("Enter an option.".into()),
        }
        Ok(Step::Continue)
    }

    fn fit_input(&mut self, input: Input, ctx: &mut Ctx, s: Spline) -> Result<(), String> {
        let mut fit = s.fit().to_vec();
        let closed = s.is_closed();
        match (self.stage, input) {
            (Stage::Fit, Input::Keyword("Add")) => self.stage = Stage::FitPoint(FitOp::Add),
            (Stage::Fit, Input::Keyword("Delete")) => self.stage = Stage::FitPoint(FitOp::Delete),
            (Stage::Fit, Input::Keyword("Move")) => self.stage = Stage::FitPoint(FitOp::Move),
            (Stage::Fit, Input::Keyword("Close" | "Open")) => self.fit_edit(ctx, &fit, !closed)?,
            (Stage::Fit, Input::Keyword("Purge")) => {
                self.edit(ctx, Some(s.with_fit(Vec::new())))?;
                self.stage = Stage::Main;
            }
            (Stage::Fit, _) => self.stage = Stage::Main,
            (Stage::FitPoint(op), Input::Point(p)) => {
                let i = nearest(&fit, p).ok_or("The spline has no fit points.")?;
                match op {
                    FitOp::Delete => {
                        fit.remove(i);
                        self.fit_edit(ctx, &fit, closed)?;
                    }
                    _ => self.stage = Stage::FitNew(op, i),
                }
            }
            (Stage::FitNew(FitOp::Add, i), Input::Point(p)) => {
                fit.insert(i + 1, p);
                self.fit_edit(ctx, &fit, closed)?;
                self.stage = Stage::FitNew(FitOp::Add, i + 1);
            }
            (Stage::FitNew(op, i), Input::Point(p)) => {
                *fit.get_mut(i).ok_or("No such fit point.")? = p;
                self.fit_edit(ctx, &fit, closed)?;
                self.stage = Stage::FitPoint(op);
            }
            (Stage::FitPoint(_) | Stage::FitNew(..), Input::Enter) => self.stage = Stage::Fit,
            _ => return Err("Specify a point.".into()),
        }
        Ok(())
    }

    fn vertex_input(&mut self, input: Input, ctx: &mut Ctx, s: Spline) -> Result<(), String> {
        let (mut cv, mut weights) = s.control_vertices();
        match (self.stage, input) {
            (Stage::Vertex, Input::Keyword("Add")) => self.stage = Stage::VertexAdd,
            (Stage::Vertex, Input::Keyword("Delete")) => self.stage = Stage::VertexPick(VertexOp::Delete),
            (Stage::Vertex, Input::Keyword("Elevate order")) => self.stage = Stage::Order,
            (Stage::Vertex, Input::Keyword("Move")) => self.stage = Stage::VertexPick(VertexOp::Move),
            (Stage::Vertex, Input::Keyword("Weight")) => self.stage = Stage::VertexPick(VertexOp::Weight),
            (Stage::Vertex, _) => self.stage = Stage::Main,
            (Stage::VertexAdd, Input::Point(p)) => {
                self.edit(ctx, s.with_control_added(p)).map_err(|_| "Pick a point along the spline.".to_string())?;
            }
            (Stage::VertexPick(VertexOp::Delete), Input::Point(p)) => {
                let i = nearest(&cv, p).ok_or("No vertex.")?;
                self.edit(ctx, s.with_control_deleted(i)).map_err(|_| "Too few vertices to delete one.".to_string())?;
            }
            (Stage::VertexPick(op), Input::Point(p)) => {
                self.stage = Stage::VertexNew(op, nearest(&cv, p).ok_or("No vertex.")?)
            }
            (Stage::VertexNew(VertexOp::Move, i), Input::Point(p)) => {
                cv[i] = p;
                self.edit(ctx, s.with_control(cv, weights))?;
                self.stage = Stage::VertexPick(VertexOp::Move);
            }
            (Stage::VertexNew(VertexOp::Weight, i), Input::Value(w)) if w > 0.0 => {
                weights[i] = w;
                self.edit(ctx, s.with_control(cv, weights))?;
                self.stage = Stage::VertexPick(VertexOp::Weight);
            }
            (Stage::VertexNew(VertexOp::Weight, _), Input::Value(_)) => return Err("Requires a positive value.".into()),
            (Stage::Order, Input::Value(v)) => {
                let order = v as usize;
                if v.fract() != 0.0 || order <= s.degree() + 1 || order > MAX_ORDER {
                    return Err(format!("Requires an integer between {} and {MAX_ORDER}.", s.degree() + 2));
                }
                self.edit(ctx, s.with_degree(order - 1))?;
                self.stage = Stage::Vertex;
            }
            (Stage::VertexAdd | Stage::VertexPick(_) | Stage::VertexNew(..) | Stage::Order, Input::Enter) => {
                self.stage = Stage::Vertex;
            }
            _ => return Err("Specify a point.".into()),
        }
        Ok(())
    }
}

impl Command for SplinEdit {
    fn prompt(&self) -> Prompt {
        let closed = self.current.as_ref().is_some_and(Spline::is_closed);
        let close = if closed { "Open" } else { "Close" };
        match self.stage {
            Stage::Select => Prompt::new("Select spline"),
            Stage::Main => {
                let mut keywords = vec![close, "Join"];
                if self.current.as_ref().is_some_and(|s| !s.fit().is_empty()) {
                    keywords.push("Fit data");
                }
                keywords.extend(["Edit vertex", "convert to Polyline", "Reverse", "Undo", "eXit"]);
                Prompt::new("Enter an option").with_keywords(&keywords).with_default("eXit")
            }
            Stage::Join => Prompt::new("Select any open curves to join to the source"),
            Stage::Precision => Prompt::new("Specify a precision").with_default("10"),
            Stage::Fit => Prompt::new("Enter a fit data option")
                .with_keywords(&["Add", close, "Delete", "Move", "Purge", "eXit"])
                .with_default("eXit"),
            Stage::FitPoint(_) => Prompt::new("Specify existing fit point").with_default("exit"),
            Stage::FitNew(FitOp::Add, _) => Prompt::new("Specify new fit point").with_default("exit"),
            Stage::FitNew(..) => Prompt::new("Specify new location"),
            Stage::Vertex => Prompt::new("Enter a vertex editing option")
                .with_keywords(&["Add", "Delete", "Elevate order", "Move", "Weight", "eXit"])
                .with_default("eXit"),
            Stage::VertexAdd => Prompt::new("Specify a point on the spline").with_default("exit"),
            Stage::VertexPick(_) => Prompt::new("Specify control point").with_default("exit"),
            Stage::VertexNew(VertexOp::Weight, i) => {
                let w = self.current.as_ref().map_or(1.0, |s| s.control_vertices().1.get(i).copied().unwrap_or(1.0));
                Prompt::new("Enter new weight").with_default(format!("{w:.4}"))
            }
            Stage::VertexNew(..) => Prompt::new("Specify new location"),
            Stage::Order => {
                let order = self.current.as_ref().map_or(4, |s| s.degree() + 1);
                Prompt::new(format!("Enter new order <{order}>"))
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let step = match self.stage {
            Stage::Select => {
                let Input::Pick { at, .. } = input else {
                    return if input == Input::Enter { Ok(Step::Done) } else { Err("Select a spline.".into()) };
                };
                let e = ctx.pick(at).and_then(|id| ctx.doc.entity(id)).ok_or("No object found.")?;
                let id = e.id;
                match &e.geometry {
                    Geometry::Spline(_) => {}
                    g @ Geometry::Polyline { .. } => {
                        let s = spline_of(g).ok_or("This polyline can't be made a spline.")?;
                        ctx.doc.modify(id, Geometry::Spline(s));
                        ctx.messages.push("The polyline was converted to a spline.".into());
                    }
                    _ => return Err("Select a spline or a polyline.".into()),
                }
                self.id = Some(id);
                self.stage = Stage::Main;
                Ok(Step::Continue)
            }
            Stage::Join => {
                let Input::Selection(sel) = input else { return Err("Select objects, then press Enter.".into()) };
                let s = self.spline(ctx).ok_or("The spline is gone.")?;
                self.stage = Stage::Main;
                self.join(ctx, &sel.ids, &s)?;
                Ok(Step::Continue)
            }
            Stage::Precision => {
                match input {
                    Input::Value(v) if (0.0..=99.0).contains(&v) => {}
                    Input::Enter => {}
                    _ => return Err("Requires an integer between 0 and 99.".into()),
                }
                // The arcs a spline is drawn with are already within 1e-7 of it.
                let s = self.spline(ctx).ok_or("The spline is gone.")?;
                self.set(ctx, Geometry::polyline_from_segs(s.segs(), s.is_closed()));
                return Ok(Step::Done);
            }
            stage => {
                let s = self.spline(ctx).ok_or("The spline is gone.")?;
                Ok(match stage {
                    Stage::Main => self.main_input(input, ctx, s)?,
                    Stage::Fit | Stage::FitPoint(_) | Stage::FitNew(..) => {
                        self.fit_input(input, ctx, s)?;
                        Step::Continue
                    }
                    _ => {
                        self.vertex_input(input, ctx, s)?;
                        Step::Continue
                    }
                })
            }
        };
        self.current = self.spline(ctx);
        // Fit data goes when vertices are edited: its options with it.
        if self.current.as_ref().is_some_and(|s| s.fit().is_empty()) && matches!(self.stage, Stage::Fit) {
            self.stage = Stage::Main;
        }
        step
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Some(s) = &self.current else { return Vec::new() };
        let frame = || {
            let (cv, _) = s.control_vertices();
            let vertices = cv.into_iter().map(|p| oxidraft_model::Vertex::new(p, 0.0)).collect();
            Geometry::Polyline { vertices, closed: s.is_periodic() }
        };
        let mut fit = s.fit().to_vec();
        match self.stage {
            Stage::Vertex | Stage::VertexAdd | Stage::VertexPick(_) | Stage::Order => vec![frame()],
            Stage::VertexNew(VertexOp::Move, i) => {
                let (mut cv, w) = s.control_vertices();
                cv[i] = cursor;
                [Some(frame()), s.with_control(cv, w).map(Geometry::Spline)].into_iter().flatten().collect()
            }
            Stage::FitNew(FitOp::Add, i) if i < fit.len() => {
                fit.insert(i + 1, cursor);
                Spline::through(&fit, s.is_closed()).map(Geometry::Spline).into_iter().collect()
            }
            Stage::FitNew(_, i) if i < fit.len() => {
                fit[i] = cursor;
                Spline::through(&fit, s.is_closed()).map(Geometry::Spline).into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.id.into_iter().collect()
    }

    fn wants_pick(&self) -> bool {
        self.stage == Stage::Select
    }

    fn wants_selection(&self) -> bool {
        self.stage == Stage::Join
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry, Spline};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    fn pick(e: &mut Engine, d: &mut Document, p: Point) {
        e.click(p, p, false, d, 0.5);
    }

    fn the_spline(d: &Document) -> Spline {
        d.entities()
            .find_map(|e| if let Geometry::Spline(s) = &e.geometry { Some(s.clone()) } else { None })
            .expect("a spline")
    }

    fn fit() -> Vec<Point> {
        [(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 5.0)].iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    #[test]
    fn close_undo_elevate_reverse_and_convert() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let s = Spline::through(&fit(), false).unwrap();
        let on = s.point_at(s.domain().1 * 0.3);
        d.add(Geometry::Spline(s));
        run(&mut e, &mut d, &["splinedit"]);
        pick(&mut e, &mut d, on);
        assert!(e.prompt().contains("Fit data"), "a fit spline has fit data");
        run(&mut e, &mut d, &["c"]);
        assert!(the_spline(&d).is_closed());
        assert!(e.prompt().contains("[Open/"));
        run(&mut e, &mut d, &["u"]);
        assert!(!the_spline(&d).is_closed());
        // Edit vertex: up to order 6 (degree 5); the fit points go.
        run(&mut e, &mut d, &["e", "e", "6", "x"]);
        let s = the_spline(&d);
        assert!(s.degree() == 5 && s.fit().is_empty());
        assert!(s.start_point().approx_eq(Point::new(0.0, 0.0)));
        run(&mut e, &mut d, &["r"]);
        assert!(the_spline(&d).start_point().approx_eq(Point::new(30.0, 5.0)));
        // convert to Polyline ends the command.
        run(&mut e, &mut d, &["p", ""]);
        assert!(!e.is_active());
        let g = &d.entities().next().unwrap().geometry;
        assert!(matches!(g, Geometry::Polyline { closed: false, .. }));
        assert!(g.segments()[0].start().approx_eq(Point::new(30.0, 5.0)));
        // One undo step for the whole command.
        d.undo();
        assert!(the_spline(&d).fit().len() == 4);
    }

    #[test]
    fn fit_points_and_control_vertices_are_edited() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        d.add(Geometry::Spline(Spline::through(&fit(), false).unwrap()));
        run(&mut e, &mut d, &["splinedit"]);
        pick(&mut e, &mut d, Point::new(0.0, 0.0));
        // Fit data: one added after the second, then the first moved and the last deleted.
        run(&mut e, &mut d, &["f", "a", "10,5", "15,10", "", "m", "0,0", "0,-2", "", "d", "30,5", "", "x"]);
        let s = the_spline(&d);
        let want = [(0.0, -2.0), (10.0, 5.0), (15.0, 10.0), (20.0, 0.0)];
        assert_eq!(s.fit().len(), 4);
        for (p, (x, y)) in s.fit().iter().zip(want) {
            assert!(p.approx_eq(Point::new(x, y)), "{:?}", s.fit());
        }
        // Purge: the same curve without its fit points.
        run(&mut e, &mut d, &["f", "p"]);
        let purged = the_spline(&d);
        assert!(purged.fit().is_empty() && purged.control() == s.control());
        // Edit vertex: add one, move one, weigh one, delete one.
        let n = purged.control().len();
        let at = purged.point_at(purged.domain().1 * 0.5);
        run(&mut e, &mut d, &["e", "a", &format!("{},{}", at.x, at.y), ""]);
        assert_eq!(the_spline(&d).control().len(), n + 1);
        let first = the_spline(&d).control()[0];
        run(&mut e, &mut d, &["m", &format!("{},{}", first.x, first.y), "-5,-5", ""]);
        assert!(the_spline(&d).start_point().approx_eq(Point::new(-5.0, -5.0)));
        run(&mut e, &mut d, &["w", "-5,-5", "3", ""]);
        assert!(the_spline(&d).weights()[0] == 3.0);
        run(&mut e, &mut d, &["d", "-5,-5", "", "x", "x"]);
        assert_eq!(the_spline(&d).control().len(), n);
    }

    #[test]
    fn join_and_polylines() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let s = Spline::through(&fit(), false).unwrap();
        let on = s.point_at(s.domain().1 * 0.5);
        d.add(Geometry::Spline(s));
        let line = d.add(Geometry::Line { start: Point::new(40.0, 5.0), end: Point::new(30.0, 5.0) });
        let arc = d.add(Geometry::arc(&oxidraft_geom::curve::ArcSeg {
            center: Point::new(0.0, -5.0),
            radius: 5.0,
            start: 90.0,
            sweep: 90.0,
        }));
        run(&mut e, &mut d, &["splinedit"]);
        pick(&mut e, &mut d, on);
        run(&mut e, &mut d, &["j", "all", "", "x"]);
        assert!(d.entity(line).is_none() && d.entity(arc).is_none() && d.len() == 1, "{}", e.history().join("\n"));
        let s = the_spline(&d);
        let (a, b) = (s.start_point(), s.end_point());
        assert!(a.approx_eq(Point::new(-5.0, -5.0)) && b.approx_eq(Point::new(40.0, 5.0)), "{a:?} {b:?}");
        // The arc stays exactly round.
        let u = s.closest_to(Point::new(0.0, -5.0).polar(5.0, 135.0));
        assert!((s.point_at(u).distance(Point::new(0.0, -5.0)) - 5.0).abs() < 1e-9);
        // A polyline picked becomes a spline on the same path.
        let mut d = Document::new();
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .map(|(x, y)| oxidraft_model::Vertex::new(Point::new(x, y), 0.0))
            .to_vec();
        d.add(Geometry::Polyline { vertices: square, closed: true });
        run(&mut e, &mut d, &["splinedit"]);
        pick(&mut e, &mut d, Point::new(5.0, 0.0));
        let s = the_spline(&d);
        assert!(s.is_closed() && s.point_at(s.closest_to(Point::new(10.0, 7.0))).approx_eq(Point::new(10.0, 7.0)));
        run(&mut e, &mut d, &["x"]);
    }
}

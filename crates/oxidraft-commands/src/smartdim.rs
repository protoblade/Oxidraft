//! DIM: one command for most dimensions, as AutoCAD's. Pick a line for a
//! linear or aligned dimension (a second line makes it an angle), a circle
//! or arc for its radius or diameter, or two points; it goes on until
//! Enter. Its options run the other dimension commands, line dimensions up
//! (aliGn), space them evenly (Distribute), or put new ones on a layer.
//! And DIMSPACE, which spaces parallel dimensions.

use oxidraft_geom::Point;
use oxidraft_model::{
    Color, EntityId, Geometry,
    annotation::{dim_style, two_line_angle},
    snap::{SnapKind, find_snap},
};

use crate::{
    Command, Ctx, Input, Prompt, Step, Vars,
    dims::{Angle, DimAngular, DimChain},
    dims_more::DimOrdinate,
};

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    First,
    Second(Point),
    /// Where the dimension line goes; picking another line makes an angle.
    Linear {
        p1: Point,
        p2: Point,
        line: Option<EntityId>,
    },
    Radial {
        center: Point,
        radius: f64,
        diameter: bool,
    },
    Angular {
        a: (Point, Point),
        b: (Point, Point),
    },
    /// One of the other dimension commands at work.
    Inner,
    Layer,
    AlignBase,
    AlignOthers(EntityId),
    Distribute,
    DistributeOffset,
}

pub struct SmartDim {
    stage: Stage,
    inner: Option<Box<dyn Command>>,
    scale: f64,
    /// DIMLAYER: new dimensions' layer (`None`: the current one).
    layer: Option<String>,
    /// How many changes each dimension made, for Undo.
    marks: Vec<usize>,
    chosen: Vec<EntityId>,
}

impl Default for SmartDim {
    fn default() -> Self {
        Self { stage: Stage::First, inner: None, scale: 1.0, layer: None, marks: Vec::new(), chosen: Vec::new() }
    }
}

/// A rotated dimension's direction, aligned ones along their points.
fn direction(g: &Geometry) -> Option<f64> {
    match *g {
        Geometry::Dimension { p1, p2, rotation, aligned, .. } => Some(if aligned { p1.angle_to(p2) } else { rotation }),
        _ => None,
    }
}

/// How far a dimension's line is from the origin, across its direction.
fn offset_of(g: &Geometry, r: f64) -> Option<f64> {
    let Geometry::Dimension { line, .. } = g else { return None };
    let n = Point::ORIGIN.polar(1.0, r + 90.0);
    Some(line.dot(n))
}

/// `g` with its dimension line `offset` across its direction.
fn with_offset(g: &Geometry, r: f64, offset: f64) -> Geometry {
    let mut g = g.clone();
    if let Geometry::Dimension { line, .. } = &mut g {
        let n = Point::ORIGIN.polar(1.0, r + 90.0);
        *line = *line + n * (offset - line.dot(n));
    }
    g
}

/// DIMSPACE and DIM's Distribute: the dimensions parallel to `base`
/// stacked `spacing` apart from it, in the order they're found going away
/// from it. Returns how many moved.
pub(crate) fn space_dimensions(ctx: &mut Ctx, base: EntityId, ids: &[EntityId], spacing: f64) -> usize {
    let Some(b) = ctx.doc.entity(base).map(|e| e.geometry.clone()) else { return 0 };
    let (Some(r), Some(start)) = (direction(&b), direction(&b).and_then(|r| offset_of(&b, r))) else { return 0 };
    let parallel = |g: &Geometry| {
        direction(g).is_some_and(|d| ((d - r).rem_euclid(180.0)).min(180.0 - (d - r).rem_euclid(180.0)) < 1e-6)
    };
    let mut others: Vec<(EntityId, Geometry, f64)> = ids
        .iter()
        .filter(|id| **id != base)
        .filter_map(|id| ctx.doc.entity(*id).map(|e| (*id, e.geometry.clone())))
        .filter(|(_, g)| parallel(g))
        .filter_map(|(id, g)| offset_of(&g, r).map(|o| (id, g, o)))
        .collect();
    // Each side of the base keeps its side.
    others.sort_by(|a, b| (a.2 - start).abs().total_cmp(&(b.2 - start).abs()));
    let (mut above, mut below) = (0, 0);
    for (id, g, o) in &others {
        let step = if *o >= start {
            above += 1;
            above as f64
        } else {
            below += 1;
            -(below as f64)
        };
        ctx.doc.modify(*id, with_offset(g, r, start + step * spacing));
    }
    others.len()
}

impl SmartDim {
    fn add(&mut self, ctx: &mut Ctx, g: Geometry) {
        self.marks.push(ctx.doc.changes_in_transaction());
        match &self.layer {
            Some(layer) => {
                let layer = layer.clone();
                ctx.doc.add_on_layer(g, &layer, Color::ByLayer);
            }
            None => {
                ctx.doc.add(g);
            }
        }
        self.stage = Stage::First;
    }

    /// AutoCAD's rule: beside the points a vertical dimension, above or
    /// below them a horizontal one, and for a sloping line anywhere else an
    /// aligned one.
    fn linear(&self, p1: Point, p2: Point, at: Point, from_line: bool) -> Geometry {
        let beside = at.x < p1.x.min(p2.x) || at.x > p1.x.max(p2.x);
        let above_or_below = at.y < p1.y.min(p2.y) || at.y > p1.y.max(p2.y);
        let sloping = (p2.x - p1.x).abs() > 1e-9 && (p2.y - p1.y).abs() > 1e-9;
        let (rotation, aligned) = match (beside, above_or_below) {
            (true, false) => (90.0, false),
            (false, true) => (0.0, false),
            _ if sloping && from_line => (p1.angle_to(p2), true),
            (true, true) if (p2.x - p1.x).abs() < (p2.y - p1.y).abs() => (90.0, false),
            _ => (0.0, false),
        };
        Geometry::Dimension {
            extra: Default::default(),
            p1,
            p2,
            line: at,
            rotation,
            aligned,
            text: None,
            scale: self.scale,
        }
    }

    fn radial(&self, center: Point, radius: f64, diameter: bool, at: Point) -> Geometry {
        let angle = if at.approx_eq(center) { 0.0 } else { center.angle_to(at) };
        Geometry::RadialDimension {
            center,
            point: center.polar(radius, angle),
            text_at: at,
            diameter,
            text: None,
            scale: self.scale,
        }
    }

    fn run_inner(&mut self, cmd: Box<dyn Command>, ctx: &Ctx) {
        let mut cmd = cmd;
        cmd.start(ctx.vars);
        cmd.prepare(ctx.doc);
        self.inner = Some(cmd);
        self.stage = Stage::Inner;
    }

    /// The object at `at`, unless an object snap point is there (which
    /// makes it a point to dimension from).
    fn object_at(ctx: &Ctx, at: Point) -> Option<(EntityId, Geometry)> {
        let snaps: Vec<SnapKind> = SnapKind::ALL.into_iter().filter(|k| *k != SnapKind::Nearest).collect();
        let tiny = ctx.pick_tolerance * 0.01;
        if find_snap(ctx.doc, at, tiny, &snaps, None).is_some() {
            return None;
        }
        let id = ctx.pick(at)?;
        Some((id, ctx.doc.entity(id)?.geometry.clone()))
    }
}

impl Command for SmartDim {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        Some("Current settings: DIMSTYLE = Standard  DIMLAYER = Use Current".into())
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::First => Prompt::new("Select objects or specify first extension line origin").with_keywords(&[
                "Angular",
                "Baseline",
                "Continue",
                "Ordinate",
                "aliGn",
                "Distribute",
                "Layer",
                "Undo",
            ]),
            Stage::Second(_) => Prompt::new("Specify second extension line origin"),
            Stage::Linear { .. } => Prompt::new("Specify dimension line location or second line for angle"),
            Stage::Radial { diameter, .. } => Prompt::new("Specify radius dimension location")
                .with_keywords(if diameter { &["Radius"] } else { &["Diameter"] }),
            Stage::Angular { .. } => Prompt::new("Specify angular dimension location"),
            Stage::Inner => self.inner.as_ref().map_or_else(|| Prompt::new(""), |c| c.prompt()),
            Stage::Layer => Prompt::new("Enter layer name or [.] for current")
                .with_default(self.layer.clone().unwrap_or(".".into())),
            Stage::AlignBase => Prompt::new("Select base dimension"),
            Stage::AlignOthers(_) | Stage::Distribute => Prompt::new("Select dimensions"),
            Stage::DistributeOffset => Prompt::new("Specify offset distance"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::First, Input::Keyword(k)) => match k {
                "Angular" => self.run_inner(Box::new(DimAngular::default()), ctx),
                "Baseline" => self.run_inner(Box::new(DimChain::baseline()), ctx),
                "Continue" => self.run_inner(Box::new(DimChain::continued()), ctx),
                "Ordinate" => self.run_inner(Box::new(DimOrdinate::default()), ctx),
                "aliGn" => self.stage = Stage::AlignBase,
                "Distribute" => self.stage = Stage::Distribute,
                "Layer" => self.stage = Stage::Layer,
                _ => match self.marks.pop() {
                    Some(mark) => {
                        while ctx.doc.changes_in_transaction() > mark && ctx.doc.undo_last_in_transaction() {}
                    }
                    None => ctx.messages.push("Nothing to undo.".into()),
                },
            },
            (Stage::First, Input::Point(p)) => match Self::object_at(ctx, p) {
                Some((id, Geometry::Line { start, end })) => {
                    self.stage = Stage::Linear { p1: start, p2: end, line: Some(id) };
                }
                Some((_, Geometry::Circle { center, radius })) => {
                    self.stage = Stage::Radial { center, radius, diameter: true };
                }
                Some((_, Geometry::Arc { center, radius, .. })) => {
                    self.stage = Stage::Radial { center, radius, diameter: false };
                }
                Some((_, Geometry::Polyline { .. })) | Some(_) | None => self.stage = Stage::Second(p),
            },
            (Stage::Second(p1), Input::Point(p2)) if !p1.approx_eq(p2) => {
                self.stage = Stage::Linear { p1, p2, line: None };
            }
            (Stage::Linear { p1, p2, line }, Input::Point(at)) => {
                // Another line: the angle between the two.
                if let Some(first) = line
                    && let Some((id, Geometry::Line { start, end })) = Self::object_at(ctx, at)
                    && id != first
                    && two_line_angle((p1, p2), (start, end), at).is_some()
                {
                    self.stage = Stage::Angular { a: (p1, p2), b: (start, end) };
                    return Ok(Step::Continue);
                }
                let g = self.linear(p1, p2, at, line.is_some());
                self.add(ctx, g);
            }
            (Stage::Radial { center, radius, diameter }, Input::Keyword(_)) => {
                self.stage = Stage::Radial { center, radius, diameter: !diameter };
            }
            (Stage::Radial { center, radius, diameter }, Input::Point(at)) => {
                let g = self.radial(center, radius, diameter, at);
                self.add(ctx, g);
            }
            (Stage::Angular { a, b }, Input::Point(at)) => {
                let (vertex, p1, p2) = Angle::Lines { a, b }.rays(at);
                let g = Geometry::AngularDimension { vertex, p1, p2, arc: at, text: None, scale: self.scale };
                self.add(ctx, g);
            }
            (Stage::Inner, input) => {
                let inner = self.inner.as_mut().ok_or("Nothing to dimension.")?;
                let before = ctx.doc.changes_in_transaction();
                let step = inner.input(input, ctx)?;
                if ctx.doc.changes_in_transaction() > before {
                    self.marks.push(before);
                }
                if step == Step::Done {
                    self.inner = None;
                    self.stage = Stage::First;
                }
            }
            (Stage::Layer, Input::Text(t)) => {
                let name = t.trim();
                self.layer = match name {
                    "." => None,
                    n => Some(
                        ctx.doc.layer(n).map(|l| l.name.clone()).ok_or_else(|| format!("Layer \"{n}\" not found."))?,
                    ),
                };
                self.stage = Stage::First;
            }
            (Stage::AlignBase, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                if !matches!(ctx.doc.entity(id).map(|e| &e.geometry), Some(Geometry::Dimension { .. })) {
                    return Err("Select a linear dimension.".into());
                }
                self.stage = Stage::AlignOthers(id);
            }
            (Stage::AlignOthers(base), Input::Selection(s)) => {
                let b = ctx.doc.entity(base).map(|e| e.geometry.clone()).ok_or("The base is gone.")?;
                let r = direction(&b).ok_or("Select a linear dimension.")?;
                let target = offset_of(&b, r).unwrap_or(0.0);
                let mut moved = 0;
                for id in s.ids.iter().filter(|id| **id != base) {
                    let Some(g) = ctx.doc.entity(*id).map(|e| e.geometry.clone()) else { continue };
                    if direction(&g).is_some_and(|d| ((d - r).rem_euclid(180.0)).abs() < 1e-6) {
                        ctx.doc.modify(*id, with_offset(&g, r, target));
                        moved += 1;
                    }
                }
                ctx.messages.push(format!("{moved} dimension(s) aligned."));
                self.stage = Stage::First;
            }
            (Stage::Distribute, Input::Selection(s)) => {
                self.chosen = s.ids;
                self.stage = Stage::DistributeOffset;
            }
            (Stage::DistributeOffset, Input::Value(v)) if v > 0.0 => {
                // The one nearest what it measures stays put.
                let nearest = self.chosen.iter().copied().min_by(|a, b| {
                    let off = |id: &EntityId| match ctx.doc.entity(*id).map(|e| &e.geometry) {
                        Some(g @ Geometry::Dimension { p1, .. }) => direction(g)
                            .and_then(|r| {
                                offset_of(g, r).map(|o| (o - p1.dot(Point::ORIGIN.polar(1.0, r + 90.0))).abs())
                            })
                            .unwrap_or(f64::MAX),
                        _ => f64::MAX,
                    };
                    off(a).total_cmp(&off(b))
                });
                if let Some(base) = nearest {
                    let ids = self.chosen.clone();
                    let n = space_dimensions(ctx, base, &ids, v);
                    ctx.messages.push(format!("{n} dimension(s) spaced."));
                }
                self.stage = Stage::First;
            }
            (Stage::First, Input::Enter) => return Ok(Step::Done),
            (_, Input::Enter) => self.stage = Stage::First,
            (Stage::AlignBase, _) => return Err("Select a dimension.".into()),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            Stage::Second(p1) => vec![Geometry::Line { start: p1, end: cursor }],
            Stage::Linear { p1, p2, line } => vec![self.linear(p1, p2, cursor, line.is_some())],
            Stage::Radial { center, radius, diameter } => vec![self.radial(center, radius, diameter, cursor)],
            Stage::Angular { a, b } => {
                let (vertex, p1, p2) = Angle::Lines { a, b }.rays(cursor);
                vec![Geometry::AngularDimension { vertex, p1, p2, arc: cursor, text: None, scale: self.scale }]
            }
            Stage::Inner => self.inner.as_ref().map_or_else(Vec::new, |c| c.preview(cursor)),
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Second(p) => Some(p),
            Stage::Inner => self.inner.as_ref().and_then(|c| c.base_point()),
            _ => None,
        }
    }

    fn wants_pick(&self) -> bool {
        match self.stage {
            Stage::AlignBase => true,
            Stage::Inner => self.inner.as_ref().is_some_and(|c| c.wants_pick()),
            _ => false,
        }
    }

    fn wants_selection(&self) -> bool {
        match self.stage {
            Stage::AlignOthers(_) | Stage::Distribute => true,
            Stage::Inner => self.inner.as_ref().is_some_and(|c| c.wants_selection()),
            _ => false,
        }
    }

    fn wants_text(&self) -> bool {
        match self.stage {
            Stage::Layer => true,
            Stage::Inner => self.inner.as_ref().is_some_and(|c| c.wants_text()),
            _ => false,
        }
    }

    fn text_with_spaces(&self) -> bool {
        true
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            Stage::Linear { line: Some(id), .. } | Stage::AlignOthers(id) => vec![id],
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SpaceStage {
    Base,
    Others(EntityId),
    Value(EntityId),
}

/// DIMSPACE: parallel linear dimensions spaced evenly from a base one,
/// by a distance or (Auto) twice their text height.
pub struct DimSpace {
    stage: SpaceStage,
    others: Vec<EntityId>,
}

impl Default for DimSpace {
    fn default() -> Self {
        Self { stage: SpaceStage::Base, others: Vec::new() }
    }
}

impl Command for DimSpace {
    fn prompt(&self) -> Prompt {
        match self.stage {
            SpaceStage::Base => Prompt::new("Select base dimension"),
            SpaceStage::Others(_) => Prompt::new("Select dimensions to space"),
            SpaceStage::Value(_) => Prompt::new("Enter value").with_keywords(&["Auto"]).with_default("Auto"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (SpaceStage::Base, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                if !matches!(ctx.doc.entity(id).map(|e| &e.geometry), Some(Geometry::Dimension { .. })) {
                    return Err("Select a linear dimension.".into());
                }
                self.stage = SpaceStage::Others(id);
            }
            (SpaceStage::Others(base), Input::Selection(s)) => {
                self.others = s.ids;
                self.stage = SpaceStage::Value(base);
            }
            (SpaceStage::Value(base), input @ (Input::Value(_) | Input::Keyword(_) | Input::Enter)) => {
                let spacing = match input {
                    Input::Value(v) if v >= 0.0 => v,
                    Input::Value(_) => return Err("Requires 0 or more.".into()),
                    _ => {
                        let scale = match ctx.doc.entity(base).map(|e| &e.geometry) {
                            Some(Geometry::Dimension { scale, .. }) => *scale,
                            _ => 1.0,
                        };
                        2.0 * dim_style().text_height * scale
                    }
                };
                let others = self.others.clone();
                let n = space_dimensions(ctx, base, &others, spacing);
                ctx.messages.push(format!("{n} dimension(s) spaced."));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Select a dimension.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        self.stage == SpaceStage::Base
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, SpaceStage::Others(_))
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            SpaceStage::Others(id) | SpaceStage::Value(id) => vec![id],
            _ => Vec::new(),
        }
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

    fn dims(d: &Document) -> Vec<Geometry> {
        d.entities()
            .map(|e| e.geometry.clone())
            .filter(|g| {
                matches!(
                    g,
                    Geometry::Dimension { .. } | Geometry::RadialDimension { .. } | Geometry::AngularDimension { .. }
                )
            })
            .collect()
    }

    #[test]
    fn dim_dimensions_what_is_picked_and_goes_on() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(40.0, 0.0) });
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(30.0, 30.0) });
        d.add(Geometry::Circle { center: Point::new(100.0, 0.0), radius: 10.0 });
        run(&mut e, &mut d, &["dim"]);
        // The horizontal line (not at a snap point, which would be a point), dimensioned below it.
        click(&mut e, &mut d, 13.0, 0.0);
        click(&mut e, &mut d, 20.0, -10.0);
        // The circle: a diameter.
        click(&mut e, &mut d, 100.0 + 10.0 * 0.6, 8.0);
        click(&mut e, &mut d, 120.0, 5.0);
        // The sloping line, then the horizontal one: the angle between.
        click(&mut e, &mut d, 10.0, 10.0);
        click(&mut e, &mut d, 30.0, 0.0);
        click(&mut e, &mut d, 25.0, 8.0);
        // Two points.
        run(&mut e, &mut d, &["50,20", "70,20", "60,30"]);
        assert!(e.is_active(), "DIM goes on");
        run(&mut e, &mut d, &[""]);
        let got = dims(&d);
        assert_eq!(got.len(), 4, "{got:?}");
        assert!(matches!(got[0], Geometry::Dimension { rotation: 0.0, aligned: false, .. }));
        assert!(matches!(got[1], Geometry::RadialDimension { diameter: true, .. }));
        assert!(matches!(got[2], Geometry::AngularDimension { .. }));
        assert!(
            matches!(got[3], Geometry::Dimension { p1, p2, .. } if p1 == Point::new(50.0, 20.0) && p2 == Point::new(70.0, 20.0))
        );
    }

    #[test]
    fn dimspace_and_align() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let dim = |x: f64, y: f64| Geometry::Dimension {
            extra: Default::default(),
            p1: Point::new(0.0, 0.0),
            p2: Point::new(x, 0.0),
            line: Point::new(x / 2.0, y),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
        };
        let a = d.add(dim(10.0, 5.0));
        let b = d.add(dim(20.0, 6.0));
        let c = d.add(dim(30.0, 30.0));
        let line_y = |d: &Document, id| match d.entity(id).unwrap().geometry {
            Geometry::Dimension { line, .. } => line.y,
            _ => f64::NAN,
        };
        run(&mut e, &mut d, &["dimspace"]);
        click(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &["all", "", "8"]);
        assert_eq!((line_y(&d, a), line_y(&d, b), line_y(&d, c)), (5.0, 13.0, 21.0));
        // DIM's aliGn: all on the base's line.
        run(&mut e, &mut d, &["dim", "g"]);
        click(&mut e, &mut d, 5.0, 5.0);
        run(&mut e, &mut d, &["all", "", ""]);
        assert_eq!((line_y(&d, b), line_y(&d, c)), (5.0, 5.0));
    }
}

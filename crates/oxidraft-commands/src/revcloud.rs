//! REVCLOUD: revision clouds, polylines of arcs drawn around a rectangle or
//! polygon, or made from an existing object.

use oxidraft_geom::{Point, curve::Seg};
use oxidraft_model::{EntityId, Geometry, Vertex, snap};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

/// Bulge of each arc: about 110 degrees of sweep.
const BULGE: f64 = 0.52;

const FIRST_OPTIONS: &[&str] = &["Arc length", "Object", "Rectangular", "Polygonal", "Style"];

#[derive(Clone, PartialEq)]
enum Stage {
    First,
    Opposite { first: Point },
    Polygon { points: Vec<Point> },
    ArcLength,
    Object,
    Reverse { id: EntityId },
    Style,
}

/// REVCLOUD, Rectangular or Polygonal, with its arc length and Normal or
/// Calligraphy style remembered.
pub struct RevCloud {
    stage: Stage,
    arc: f64,
    calligraphy: bool,
    polygonal: bool,
}

impl Default for RevCloud {
    fn default() -> Self {
        Self { stage: Stage::First, arc: 1.0, calligraphy: false, polygonal: false }
    }
}

/// A point every `step` along a path, starting at its start; for an open
/// path, its end too.
fn stations(segs: &[Seg], count: usize, closed: bool) -> Vec<Point> {
    let total: f64 = segs.iter().map(Seg::length).sum();
    let mut out = Vec::new();
    let last = if closed { count } else { count + 1 };
    let (mut i, mut before) = (0, 0.0);
    for k in 0..last {
        let d = total * k as f64 / count as f64;
        while i + 1 < segs.len() && before + segs[i].length() < d {
            before += segs[i].length();
            i += 1;
        }
        let len = segs[i].length();
        out.push(segs[i].point_at(if len > 0.0 { ((d - before) / len).clamp(0.0, 1.0) } else { 0.0 }));
    }
    out
}

/// The cloud along a path: arcs about `arc` long bulging outward (to the
/// right of a counterclockwise path), or inward when `reverse`.
pub(crate) fn cloud(segs: &[Seg], closed: bool, arc: f64, calligraphy: bool, reverse: bool) -> Option<Geometry> {
    let total: f64 = segs.iter().map(Seg::length).sum();
    if total <= 0.0 || arc <= 0.0 {
        return None;
    }
    let count = ((total / arc).round() as usize).max(if closed { 3 } else { 1 });
    let mut points = stations(segs, count, closed);
    // Go round counterclockwise, so outward is to the right.
    let area: f64 = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a.cross(*b)).sum();
    if closed && area < 0.0 {
        points.reverse();
    }
    let bulge = if reverse { -BULGE } else { BULGE };
    let n = points.len();
    let vertices = points
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let has_arc = closed || i + 1 < n;
            let v = Vertex::new(p, if has_arc { bulge } else { 0.0 });
            // Calligraphy arcs swell from nothing, like a pen stroke.
            let chord = p.distance(points[(i + 1) % n]);
            if calligraphy && has_arc { v.with_widths(0.0, chord * 0.15) } else { v }
        })
        .collect();
    Some(Geometry::Polyline { vertices, closed })
}

fn rectangle(a: Point, b: Point) -> Vec<Seg> {
    let c = [a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)];
    (0..4).map(|i| Seg::from_bulge(c[i], c[(i + 1) % 4], 0.0)).collect()
}

fn polygon(points: &[Point]) -> Vec<Seg> {
    (0..points.len()).map(|i| Seg::from_bulge(points[i], points[(i + 1) % points.len()], 0.0)).collect()
}

impl RevCloud {
    fn make(&self, segs: &[Seg], closed: bool, reverse: bool) -> Option<Geometry> {
        cloud(segs, closed, self.arc, self.calligraphy, reverse)
    }

    fn first_prompt(&self) -> Prompt {
        let text = if self.polygonal { "Specify start point" } else { "Specify first corner point" };
        Prompt::new(text).with_keywords(FIRST_OPTIONS).with_default("Object")
    }
}

impl Command for RevCloud {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        // Until set, arcs are a fiftieth of the view's height, to 2 significant digits.
        let fallback = {
            let raw = vars.view_size / 50.0;
            let unit = 10f64.powf(raw.log10().floor() - 1.0);
            (raw / unit).round() * unit
        };
        self.arc = vars.revcloud_arc.unwrap_or(fallback);
        self.calligraphy = vars.revcloud_calligraphy;
        self.polygonal = vars.revcloud_polygonal;
        Some(format!(
            "Minimum arc length: {0:.4}   Maximum arc length: {0:.4}   Style: {1}   Type: {2}",
            self.arc,
            if self.calligraphy { "Calligraphy" } else { "Normal" },
            if self.polygonal { "Polygonal" } else { "Rectangular" },
        ))
    }

    fn prompt(&self) -> Prompt {
        match &self.stage {
            Stage::First => self.first_prompt(),
            Stage::Opposite { .. } => Prompt::new("Specify opposite corner"),
            Stage::Polygon { points } if points.len() >= 2 => {
                Prompt::new("Specify next point").with_keywords(&["Undo"])
            }
            Stage::Polygon { .. } => Prompt::new("Specify next point"),
            Stage::ArcLength => {
                Prompt::new("Specify approximate length of arc").with_default(format!("{:.4}", self.arc))
            }
            Stage::Object => Prompt::new("Select object"),
            Stage::Reverse { .. } => Prompt::new("Reverse direction").with_keywords(&["Yes", "No"]).with_default("No"),
            Stage::Style => Prompt::new("Select arc style")
                .with_keywords(&["Normal", "Calligraphy"])
                .with_default(if self.calligraphy { "Calligraphy" } else { "Normal" }),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (Stage::First, Input::Point(p)) if self.polygonal => self.stage = Stage::Polygon { points: vec![p] },
            (Stage::First, Input::Point(first)) => self.stage = Stage::Opposite { first },
            (Stage::First, Input::Keyword("Arc length")) => self.stage = Stage::ArcLength,
            (Stage::First, Input::Keyword("Object") | Input::Enter) => self.stage = Stage::Object,
            (Stage::First, Input::Keyword("Style")) => self.stage = Stage::Style,
            (Stage::First, Input::Keyword(k @ ("Rectangular" | "Polygonal"))) => {
                self.polygonal = k == "Polygonal";
                ctx.vars.revcloud_polygonal = self.polygonal;
            }
            (Stage::ArcLength, Input::Value(v)) if v > 0.0 => {
                self.arc = v;
                ctx.vars.revcloud_arc = Some(v);
                self.stage = Stage::First;
            }
            (Stage::ArcLength, Input::Enter) => self.stage = Stage::First,
            (Stage::ArcLength, _) => return Err("Requires a positive length.".into()),
            (Stage::Style, input @ (Input::Keyword(_) | Input::Enter)) => {
                if let Input::Keyword(k) = input {
                    self.calligraphy = k == "Calligraphy";
                    ctx.vars.revcloud_calligraphy = self.calligraphy;
                }
                ctx.messages.push(format!("Arc style = {}", if self.calligraphy { "Calligraphy" } else { "Normal" }));
                self.stage = Stage::First;
            }
            (Stage::Opposite { first }, Input::Point(p)) => {
                if (p.x - first.x).abs() < 1e-9 || (p.y - first.y).abs() < 1e-9 {
                    return Err("The rectangle needs a width and a height.".into());
                }
                ctx.doc.add(self.make(&rectangle(first, p), true, false).ok_or("Cannot make a cloud there.")?);
                ctx.messages.push("Revision cloud finished.".into());
                return Ok(Step::Done);
            }
            (Stage::Polygon { mut points }, Input::Point(p)) => {
                if points.last().is_some_and(|q| q.approx_eq(p)) {
                    return Err("Point ignored: it repeats the last one.".into());
                }
                points.push(p);
                self.stage = Stage::Polygon { points };
            }
            (Stage::Polygon { mut points }, Input::Keyword("Undo")) => {
                points.pop();
                self.stage = if points.is_empty() { Stage::First } else { Stage::Polygon { points } };
            }
            (Stage::Polygon { points }, Input::Enter) => {
                if points.len() < 3 {
                    return Ok(Step::Done);
                }
                ctx.doc.add(self.make(&polygon(&points), true, false).ok_or("Cannot make a cloud there.")?);
                ctx.messages.push("Revision cloud finished.".into());
                return Ok(Step::Done);
            }
            (Stage::Object, Input::Pick { at, .. }) => {
                let id = snap::pick(ctx.doc, at, ctx.pick_tolerance).ok_or("No object found.")?;
                let g = &ctx.doc.entity(id).ok_or("No object found.")?.geometry;
                if g.segments().is_empty() || g.is_infinite() {
                    return Err(format!("A {} can't become a revision cloud.", g.name()));
                }
                self.stage = Stage::Reverse { id };
            }
            (Stage::Reverse { id }, input @ (Input::Keyword(_) | Input::Enter)) => {
                let g = ctx.doc.entity(id).map(|e| e.geometry.clone()).ok_or("Object not found.")?;
                let reverse = input == Input::Keyword("Yes");
                let cloud = self.make(&g.segments(), g.is_closed(), reverse).ok_or("Cannot make a cloud of that.")?;
                ctx.doc.modify(id, cloud);
                ctx.messages.push("Revision cloud finished.".into());
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Object, _) => return Err("Select an object.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match &self.stage {
            Stage::Opposite { first } if (cursor.x - first.x).abs() > 1e-9 && (cursor.y - first.y).abs() > 1e-9 => {
                self.make(&rectangle(*first, cursor), true, false).into_iter().collect()
            }
            Stage::Polygon { points } => {
                let mut all = points.clone();
                all.push(cursor);
                if all.len() >= 3 {
                    self.make(&polygon(&all), true, false).into_iter().collect()
                } else {
                    vec![Geometry::Line { start: all[0], end: cursor }]
                }
            }
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match &self.stage {
            Stage::Opposite { first } => Some(*first),
            Stage::Polygon { points } => points.last().copied(),
            _ => None,
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            Stage::Reverse { id } => vec![id],
            _ => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        self.stage == Stage::Object
    }
}

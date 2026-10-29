//! PEDIT: edits polylines. Close or open them, join objects to them, set
//! their width, straighten their arcs or reverse them; lines and arcs can
//! be turned into polylines first.

use oxidraft_model::{EntityId, Geometry};

use crate::{
    Command, Ctx, Input, Prompt, Step,
    break_join::{JOIN_TOL, Piece, ends, grow_chain, reversed},
};

const OPEN_OPTIONS: &[&str] = &["Close", "Join", "Width", "Decurve", "Reverse"];
const CLOSED_OPTIONS: &[&str] = &["Open", "Join", "Width", "Decurve", "Reverse"];
const MULTIPLE_OPTIONS: &[&str] = &["Close", "Open", "Join", "Width", "Decurve", "Reverse"];

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Select,
    /// A line or arc was picked: turn it into a polyline?
    Convert(EntityId),
    Multiple,
    /// Lines or arcs are among the objects selected: turn them into polylines?
    ConvertAll,
    Options,
    Join,
    Fuzz,
    Width,
}

/// PEDIT, with Close/Open, Join, Width, Decurve and Reverse, for one
/// polyline or several (Multiple).
pub struct Pedit {
    stage: Stage,
    ids: Vec<EntityId>,
    multiple: bool,
    /// Whether the one polyline being edited is closed, for its options.
    closed: bool,
}

impl Default for Pedit {
    fn default() -> Self {
        Self { stage: Stage::Select, ids: Vec::new(), multiple: false, closed: false }
    }
}

fn is_line_or_arc(g: &Geometry) -> bool {
    matches!(g, Geometry::Line { .. } | Geometry::Arc { .. })
}

fn polyline_of(g: &Geometry) -> Geometry {
    Geometry::polyline_from_segs(&g.segments(), false)
}

impl Pedit {
    fn polylines(&self, ctx: &Ctx) -> Vec<(EntityId, Geometry)> {
        self.ids
            .iter()
            .filter_map(|&id| ctx.doc.entity(id).map(|e| (id, e.geometry.clone())))
            .filter(|(_, g)| matches!(g, Geometry::Polyline { .. }))
            .collect()
    }

    /// Applies `f` to every polyline being edited.
    fn each(&self, ctx: &mut Ctx, f: impl Fn(Vec<oxidraft_model::Vertex>, bool) -> Geometry) {
        for (id, g) in self.polylines(ctx) {
            if let Geometry::Polyline { vertices, closed } = g {
                ctx.doc.modify(id, f(vertices, closed));
            }
        }
    }

    fn options(&mut self) -> Result<Step, String> {
        self.stage = Stage::Options;
        Ok(Step::Continue)
    }

    fn option(&mut self, keyword: &str, ctx: &mut Ctx) -> Result<Step, String> {
        match keyword {
            "Close" => {
                self.each(ctx, |mut vertices, _| {
                    // A last vertex on the first one becomes the closing segment.
                    if vertices.len() > 2 && vertices[0].point.distance(vertices[vertices.len() - 1].point) <= JOIN_TOL
                    {
                        vertices.pop();
                    }
                    Geometry::Polyline { vertices, closed: true }
                });
                self.closed = true;
            }
            "Open" => {
                self.each(ctx, |vertices, _| Geometry::Polyline { vertices, closed: false });
                self.closed = false;
            }
            "Decurve" => self.each(ctx, |vertices, closed| Geometry::Polyline {
                vertices: vertices.into_iter().map(|v| oxidraft_model::Vertex { bulge: 0.0, ..v }).collect(),
                closed,
            }),
            "Reverse" => self.each(ctx, |vertices, closed| {
                let width = oxidraft_model::constant_width(&vertices).unwrap_or(0.0);
                let segs = Geometry::Polyline { vertices, closed }.segments();
                Geometry::polyline_from_segs(&reversed(&segs), closed).with_width(width)
            }),
            "Width" => self.stage = Stage::Width,
            "Join" if self.multiple => self.stage = Stage::Fuzz,
            "Join" if self.closed => return Err("The polyline is closed; Open it to join objects to it.".into()),
            "Join" => self.stage = Stage::Join,
            _ => return Err("Unknown option.".into()),
        }
        Ok(Step::Continue)
    }

    /// Joins pieces to each polyline being edited: each grows from its ends
    /// with the others (Multiple) or with `extra` (one polyline).
    fn join(&mut self, ctx: &mut Ctx, extra: &[EntityId], fuzz: f64) {
        let joinable = |g: &Geometry| is_line_or_arc(g) || matches!(g, Geometry::Polyline { closed: false, .. });
        let piece = |id: EntityId| {
            let g = &ctx.doc.entity(id)?.geometry;
            let segs = g.segments();
            (joinable(g) && !segs.is_empty()).then_some((id, segs))
        };
        let mut pieces: Vec<Piece> = self.ids.iter().filter_map(|&id| piece(id)).collect();
        let own = pieces.len();
        pieces.extend(extra.iter().filter(|id| !self.ids.contains(id)).filter_map(|&id| piece(id)));
        let mut used = vec![false; pieces.len()];
        let mut added = 0;
        for i in 0..own {
            if used[i] {
                continue;
            }
            used[i] = true;
            let mut chain = pieces[i].1.clone();
            let before = chain.len();
            let joined = grow_chain(&mut chain, &pieces, &mut used, fuzz);
            if joined.is_empty() {
                continue;
            }
            let (start, end) = ends(&chain);
            let closed = start.distance(end) <= fuzz.max(JOIN_TOL);
            added += chain.len() - before;
            ctx.doc.modify(pieces[i].0, Geometry::polyline_from_segs(&chain, closed));
            for id in joined {
                ctx.doc.remove(id);
                self.ids.retain(|&x| x != id);
            }
            self.closed = closed;
        }
        let noun = if added == 1 { "segment" } else { "segments" };
        ctx.messages.push(format!("{added} {noun} added to polyline"));
    }
}

impl Command for Pedit {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select polyline").with_keywords(&["Multiple"]),
            Stage::Convert(_) => Prompt::new("Object selected is not a polyline. Do you want to turn it into one?")
                .with_keywords(&["Yes", "No"])
                .with_default("Y"),
            Stage::Multiple | Stage::Join => Prompt::new("Select objects"),
            Stage::ConvertAll => {
                Prompt::new("Convert Lines and Arcs to polylines?").with_keywords(&["Yes", "No"]).with_default("Y")
            }
            Stage::Options => {
                let keywords = match (self.multiple, self.closed) {
                    (true, _) => MULTIPLE_OPTIONS,
                    (false, true) => CLOSED_OPTIONS,
                    (false, false) => OPEN_OPTIONS,
                };
                Prompt::new("Enter an option").with_keywords(keywords).with_default("eXit")
            }
            Stage::Fuzz => Prompt::new("Enter fuzz distance").with_default("0.0000"),
            Stage::Width => Prompt::new("Specify new width for all segments"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::Select, Input::Keyword("Multiple")) => {
                self.multiple = true;
                self.stage = Stage::Multiple;
                Ok(Step::Continue)
            }
            (Stage::Select, Input::Pick { at, .. }) => {
                let Some(e) = ctx.pick(at).and_then(|id| ctx.doc.entity(id)) else {
                    return Err("No object found.".into());
                };
                self.ids = vec![e.id];
                match &e.geometry {
                    Geometry::Polyline { closed, .. } => {
                        self.closed = *closed;
                        self.options()
                    }
                    g if is_line_or_arc(g) => {
                        self.stage = Stage::Convert(e.id);
                        Ok(Step::Continue)
                    }
                    _ => Err("Select a polyline, line or arc.".into()),
                }
            }
            (Stage::Convert(_), Input::Keyword("No")) => Ok(Step::Done),
            (Stage::Convert(id), Input::Keyword(_) | Input::Enter) => {
                if let Some(g) = ctx.doc.entity(id).map(|e| polyline_of(&e.geometry)) {
                    ctx.doc.modify(id, g);
                }
                self.options()
            }
            (Stage::Multiple, Input::Selection(s)) => {
                let geometry = |id: EntityId| ctx.doc.entity(id).map(|e| e.geometry.clone());
                self.ids = s
                    .ids
                    .into_iter()
                    .filter(|&id| {
                        geometry(id).is_some_and(|g| is_line_or_arc(&g) || matches!(g, Geometry::Polyline { .. }))
                    })
                    .collect();
                if self.ids.is_empty() {
                    return Ok(Step::Done);
                }
                if self.ids.iter().any(|&id| geometry(id).is_some_and(|g| is_line_or_arc(&g))) {
                    self.stage = Stage::ConvertAll;
                    return Ok(Step::Continue);
                }
                self.options()
            }
            (Stage::ConvertAll, input @ (Input::Keyword(_) | Input::Enter)) => {
                let convert = input != Input::Keyword("No");
                for id in self.ids.clone() {
                    let Some(g) = ctx.doc.entity(id).map(|e| e.geometry.clone()) else { continue };
                    if is_line_or_arc(&g) {
                        if convert {
                            ctx.doc.modify(id, polyline_of(&g));
                        } else {
                            self.ids.retain(|&x| x != id);
                        }
                    }
                }
                if self.ids.is_empty() { Ok(Step::Done) } else { self.options() }
            }
            (Stage::Options, Input::Keyword(k)) => self.option(k, ctx),
            (Stage::Options, Input::Enter) => Ok(Step::Done),
            (Stage::Join, Input::Selection(s)) => {
                self.join(ctx, &s.ids, 0.0);
                self.options()
            }
            (Stage::Fuzz, Input::Value(v)) if v >= 0.0 => {
                self.join(ctx, &[], v);
                self.options()
            }
            (Stage::Fuzz, Input::Enter) => {
                self.join(ctx, &[], 0.0);
                self.options()
            }
            (Stage::Fuzz, _) => Err("Requires a distance of 0 or more.".into()),
            (Stage::Width, Input::Value(w)) if w >= 0.0 => {
                self.each(ctx, |vertices, closed| Geometry::Polyline { vertices, closed }.with_width(w));
                self.options()
            }
            (Stage::Width, Input::Enter) => self.options(),
            (Stage::Width, _) => Err("Requires a width of 0 or more.".into()),
            (_, Input::Enter) => Ok(Step::Done),
            (Stage::Select, _) => Err("Select a polyline, or type M for Multiple.".into()),
            (Stage::Options, _) => Err("Choose an option, or press Enter to exit.".into()),
            _ => Err("Select objects, then press Enter.".into()),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        if matches!(self.stage, Stage::Select | Stage::Multiple) { Vec::new() } else { self.ids.clone() }
    }

    fn wants_pick(&self) -> bool {
        self.stage == Stage::Select
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, Stage::Multiple | Stage::Join)
    }
}

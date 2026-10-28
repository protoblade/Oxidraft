use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Document, EntityId, Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step, Vars, ViewRequest, parse_input};

const OPTIONS: &[&str] = &["All", "Center", "Extents", "Previous", "Scale", "Window", "Object"];

/// A typed scale factor: `2x` (or `2xp`) magnifies the current view, a plain
/// number scales the drawing's extents.
fn scale_factor(text: &str) -> Option<ViewRequest> {
    let t = text.trim().to_ascii_lowercase();
    let (number, relative) = match t.strip_suffix("xp").or_else(|| t.strip_suffix('x')) {
        Some(n) => (n, true),
        None => (t.as_str(), false),
    };
    let f: f64 = number.trim().parse().ok().filter(|f: &f64| *f > 0.0)?;
    Some(if relative { ViewRequest::Magnify(f) } else { ViewRequest::OfExtents(f) })
}

#[derive(Clone, Copy)]
enum Stage {
    Start,
    /// The first corner of a window was picked.
    Corner(Point),
    FirstCorner,
    Scale,
    CenterPoint,
    Magnification(Point),
    Object,
}

/// ZOOM: a window (the default when you pick a point), a scale factor, or
/// All / Center / Extents / Previous / Scale / Window / Object. Enter zooms
/// in real time. The app carries out the view change.
pub struct Zoom {
    stage: Stage,
    /// VIEWSIZE when the command started, the default height for Center.
    view_size: f64,
    /// Objects selected before ZOOM started: Object zooms to them, and they
    /// stay selected afterwards.
    preselected: Vec<EntityId>,
}

impl Default for Zoom {
    fn default() -> Self {
        Self { stage: Stage::Start, view_size: 100.0, preselected: Vec::new() }
    }
}

/// The region the objects cover, if any of them still exist.
fn bounds_of(doc: &Document, ids: &[EntityId]) -> Option<Aabb> {
    ids.iter().filter_map(|&id| doc.entity(id)).map(|e| doc.entity_bounds(e)).reduce(Aabb::union)
}

impl Zoom {
    fn done(ctx: &mut Ctx, request: ViewRequest) -> Result<Step, String> {
        ctx.view = Some(request);
        Ok(Step::Done)
    }

    fn option(&mut self, keyword: &str, ctx: &mut Ctx) -> Result<Step, String> {
        match keyword {
            "All" | "Extents" => return Self::done(ctx, ViewRequest::Extents),
            "Previous" => return Self::done(ctx, ViewRequest::Previous),
            "Center" => self.stage = Stage::CenterPoint,
            "Scale" => self.stage = Stage::Scale,
            "Window" => self.stage = Stage::FirstCorner,
            // Objects selected beforehand are used right away.
            "Object" => match bounds_of(ctx.doc, &self.preselected) {
                Some(b) => return Self::done(ctx, ViewRequest::Window(b)),
                None => self.stage = Stage::Object,
            },
            _ => return Err("Unknown option.".into()),
        }
        Ok(Step::Continue)
    }

    /// One input, before the selection is put back.
    fn handle(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        // Typed text at the first prompt: a scale factor, a point or an option.
        let input = match (self.stage, input) {
            (Stage::Start, Input::Text(t)) => {
                if let Some(request) = scale_factor(&t) {
                    return Self::done(ctx, request);
                }
                parse_input(&t, None, OPTIONS, false)?
            }
            (_, input) => input,
        };
        match (self.stage, input) {
            (Stage::Start, Input::Enter) => {
                ctx.messages.push("Press Esc or Enter to exit, or right-click to exit.".into());
                Self::done(ctx, ViewRequest::RealtimeZoom)
            }
            (Stage::Start, Input::Keyword(k)) => self.option(k, ctx),
            (Stage::Start | Stage::FirstCorner, Input::Point(p)) => {
                self.stage = Stage::Corner(p);
                Ok(Step::Continue)
            }
            (Stage::Corner(a), Input::Point(b)) => {
                if a.approx_eq(b) {
                    return Err("The corners must differ.".into());
                }
                Self::done(ctx, ViewRequest::Window(Aabb::from_points(a, b)))
            }
            (Stage::Scale, Input::Text(t)) => {
                Self::done(ctx, scale_factor(&t).ok_or("Requires a positive number, or one followed by X.")?)
            }
            (Stage::CenterPoint, Input::Point(p)) => {
                self.stage = Stage::Magnification(p);
                Ok(Step::Continue)
            }
            (Stage::Magnification(center), Input::Text(t)) => {
                // nX magnifies the current view; a plain number is the height to show.
                let height = match scale_factor(&t) {
                    Some(ViewRequest::Magnify(f)) => self.view_size / f,
                    Some(ViewRequest::OfExtents(h)) => h,
                    _ => return Err("Requires a positive number, or one followed by X.".into()),
                };
                Self::done(ctx, ViewRequest::Center { center, height })
            }
            (Stage::Magnification(center), Input::Enter) => {
                Self::done(ctx, ViewRequest::Center { center, height: self.view_size })
            }
            (Stage::Object, Input::Selection(s)) => {
                let bounds = s.ids.iter().filter_map(|&id| ctx.doc.entity(id)).map(|e| ctx.doc.entity_bounds(e));
                match bounds.reduce(Aabb::union) {
                    Some(b) => Self::done(ctx, ViewRequest::Window(b)),
                    None => Ok(Step::Done),
                }
            }
            (_, Input::Enter) => Ok(Step::Done),
            (Stage::Object, _) => Err("Select objects, then press Enter.".into()),
            _ => Err("Point or option keyword required.".into()),
        }
    }
}

impl Command for Zoom {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.view_size = vars.view_size;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Start => Prompt::new("Specify corner of window, enter a scale factor (nX or nXP),")
                .with_keywords(OPTIONS)
                .with_default("real time"),
            Stage::Corner(_) => Prompt::new("Specify opposite corner"),
            Stage::FirstCorner => Prompt::new("Specify first corner"),
            Stage::Scale => Prompt::new("Enter a scale factor (nX or nXP)"),
            Stage::CenterPoint => Prompt::new("Specify center point"),
            Stage::Magnification(_) => {
                Prompt::new("Enter magnification or height").with_default(format!("{:.4}", self.view_size))
            }
            Stage::Object => Prompt::new("Select objects"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let step = self.handle(input, ctx);
        // Zooming doesn't deselect anything.
        if step == Ok(Step::Done) && !self.preselected.is_empty() {
            ctx.select = Some(std::mem::take(&mut self.preselected));
        }
        step
    }

    fn preselected(&mut self, ids: &[EntityId]) {
        self.preselected = ids.to_vec();
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.preselected.clone()
    }

    /// The window being dragged out.
    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Stage::Corner(a) = self.stage else { return Vec::new() };
        let corners = [a, Point::new(cursor.x, a.y), cursor, Point::new(a.x, cursor.y)];
        vec![Geometry::Polyline { vertices: corners.into_iter().map(Vertex::at).collect(), closed: true }]
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, Stage::Object)
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, Stage::Start | Stage::Scale | Stage::Magnification(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_factors() {
        assert_eq!(scale_factor("2x"), Some(ViewRequest::Magnify(2.0)));
        assert_eq!(scale_factor(".5XP"), Some(ViewRequest::Magnify(0.5)));
        assert_eq!(scale_factor("3"), Some(ViewRequest::OfExtents(3.0)));
        assert_eq!(scale_factor("-1x"), None);
        assert_eq!(scale_factor("e"), None);
    }
}

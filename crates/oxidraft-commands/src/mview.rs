//! MVIEW (MV): viewports onto model space, made in a layout.

use oxidraft_geom::{Aabb, EPSILON, Point};
use oxidraft_model::{Document, EntityId, Geometry, MODEL, Space};

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest};

const OPTIONS: &[&str] = &["ON", "OFF", "Fit", "Lock"];

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Corner,
    Other(Point),
    /// Selecting viewports to turn on (true) or off.
    Switch(bool),
    LockMode,
    /// Selecting viewports to lock (true) or unlock.
    Lock(bool),
}

/// MVIEW: a rectangular viewport from two corners, or filling the
/// printable area (Fit); ON/OFF show or hide viewports' contents, and Lock
/// keeps their view from changing.
pub struct Mview {
    stage: Stage,
    /// The layout's printable area; `None` in model space, where the
    /// command isn't allowed.
    printable: Option<Aabb>,
    /// Objects selected before: ON, OFF and Lock change those without
    /// asking.
    preselected: Vec<EntityId>,
}

impl Default for Mview {
    fn default() -> Self {
        Self { stage: Stage::Corner, printable: None, preselected: Vec::new() }
    }
}

/// A viewport framing `frame`, zoomed to model space's extents.
pub fn viewport_onto_model(doc: &Document, frame: Aabb) -> Geometry {
    let (width, height) = (frame.width(), frame.height());
    let (view_center, view_height) = match doc.space_extents(MODEL) {
        Some(e) if e.width() > EPSILON || e.height() > EPSILON => {
            (e.center(), e.height().max(e.width() * height / width.max(EPSILON)))
        }
        _ => (frame.center(), height),
    };
    Geometry::Viewport {
        center: frame.center(),
        width,
        height,
        view_center,
        view_height,
        twist: 0.0,
        frozen: Vec::new(),
        locked: false,
        on: true,
    }
}

/// The viewport in `space` whose frame `p` is inside: the topmost one
/// that's on.
pub fn viewport_at(doc: &Document, space: Space, p: Point) -> Option<EntityId> {
    doc.space_in_draw_order(space)
        .into_iter()
        .rev()
        .find(|e| match &e.geometry {
            Geometry::Viewport { center, width, height, on: true, .. } => {
                (p.x - center.x).abs() <= width / 2.0 && (p.y - center.y).abs() <= height / 2.0
            }
            _ => false,
        })
        .map(|e| e.id)
}

/// VPMAX with more than one viewport: picks the one to fill the drawing
/// area. (The app maximizes the only viewport, a selected one or the one
/// being worked in without asking.)
#[derive(Default)]
pub struct VpMax {
    done: bool,
}

impl Command for VpMax {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        if doc.layout(doc.space()).is_none() {
            self.done = true;
            return Some("** Command not allowed in Model Tab **".into());
        }
        let any = doc.entities().any(|e| matches!(e.geometry, Geometry::Viewport { on: true, .. }));
        self.done = !any;
        self.done.then(|| "No viewports to maximize.".into())
    }

    fn is_finished(&self) -> bool {
        self.done
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Select a viewport to maximize")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } | Input::Point(at) => match viewport_at(ctx.doc, ctx.doc.space(), at) {
                Some(id) => {
                    ctx.ui = Some(UiRequest::MaximizeViewport(id));
                    Ok(Step::Done)
                }
                None => Err("Click inside a viewport.".into()),
            },
            Input::Enter => Ok(Step::Done),
            _ => Err("Click inside a viewport.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

impl Mview {
    fn add(&self, frame: Aabb, ctx: &mut Ctx) -> Result<Step, String> {
        if frame.width() <= EPSILON || frame.height() <= EPSILON {
            return Err("The viewport needs a nonzero width and height.".into());
        }
        let viewport = viewport_onto_model(ctx.doc, frame);
        ctx.doc.add(viewport);
        Ok(Step::Done)
    }

    /// Sets ON/OFF or Lock on the viewports among `ids`.
    fn set(ids: &[EntityId], ctx: &mut Ctx, f: impl Fn(&mut bool, &mut bool)) {
        let mut count = 0;
        for &id in ids {
            ctx.doc.update(id, |e| {
                if let Geometry::Viewport { on, locked, .. } = &mut e.geometry {
                    f(on, locked);
                    count += 1;
                }
            });
        }
        ctx.messages.push(format!("{count} viewport(s) changed."));
    }
}

impl Command for Mview {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        self.printable = doc.layout(doc.space()).map(|l| l.paper.sheet_bounds().1);
        self.printable.is_none().then(|| "** Command not allowed in Model Tab **".into())
    }

    fn is_finished(&self) -> bool {
        self.printable.is_none()
    }

    fn preselected(&mut self, ids: &[EntityId]) {
        self.preselected = ids.to_vec();
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Corner => Prompt::new("Specify corner of viewport").with_keywords(OPTIONS).with_default("Fit"),
            Stage::Other(_) => Prompt::new("Specify opposite corner"),
            Stage::LockMode => Prompt::new("Viewport View Locking").with_keywords(&["ON", "OFF"]),
            Stage::Switch(_) | Stage::Lock(_) => Prompt::new("Select objects"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::Corner, Input::Point(p)) => self.stage = Stage::Other(p),
            (Stage::Corner, Input::Keyword("Fit") | Input::Enter) => {
                let printable = self.printable.unwrap_or(Aabb::from_points(Point::ORIGIN, Point::ORIGIN));
                return self.add(printable, ctx);
            }
            (Stage::Corner, Input::Keyword(k @ ("ON" | "OFF"))) if !self.preselected.is_empty() => {
                Self::set(&self.preselected, ctx, |on, _| *on = k == "ON");
                return Ok(Step::Done);
            }
            (Stage::Corner, Input::Keyword("ON")) => self.stage = Stage::Switch(true),
            (Stage::Corner, Input::Keyword("OFF")) => self.stage = Stage::Switch(false),
            (Stage::Corner, Input::Keyword("Lock")) => self.stage = Stage::LockMode,
            (Stage::Other(a), Input::Point(b)) => return self.add(Aabb::from_points(a, b), ctx),
            (Stage::LockMode, Input::Keyword(k)) if !self.preselected.is_empty() => {
                Self::set(&self.preselected, ctx, |_, locked| *locked = k == "ON");
                return Ok(Step::Done);
            }
            (Stage::LockMode, Input::Keyword(k)) => self.stage = Stage::Lock(k == "ON"),
            (Stage::Switch(state), Input::Selection(s)) => {
                Self::set(&s.ids, ctx, |on, _| *on = state);
                return Ok(Step::Done);
            }
            (Stage::Lock(state), Input::Selection(s)) => {
                Self::set(&s.ids, ctx, |_, locked| *locked = state);
                return Ok(Step::Done);
            }
            (Stage::LockMode, _) => return Err("Enter ON or OFF.".into()),
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Other(_), _) => return Err("Specify the opposite corner.".into()),
            _ => return Err("Specify a corner, or choose an option.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match self.stage {
            Stage::Other(a) => {
                let corners = [a, Point::new(cursor.x, a.y), cursor, Point::new(a.x, cursor.y)];
                vec![Geometry::Polyline {
                    vertices: corners.into_iter().map(oxidraft_model::Vertex::at).collect(),
                    closed: true,
                }]
            }
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Other(a) => Some(a),
            _ => None,
        }
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, Stage::Switch(_) | Stage::Lock(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use oxidraft_model::Paper;
    use oxidraft_model::layout::Layout;

    fn doc_with_layout() -> Document {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(1000.0, 500.0) });
        let paper = Paper {
            width: 297.0,
            height: 210.0,
            margins: [10.0, 10.0, 10.0, 10.0],
            millimeters: true,
            ..Paper::default()
        };
        let id = doc.add_layout(Layout { id: MODEL, name: "Layout1".into(), tab_order: 1, paper });
        doc.set_space(id);
        doc
    }

    fn run(engine: &mut Engine, doc: &mut Document, inputs: &[&str]) {
        engine.dynamic_input = false;
        for i in inputs {
            engine.submit(i, doc, Point::ORIGIN, 1.0);
        }
    }

    fn viewports(doc: &Document) -> Vec<Geometry> {
        doc.entities().filter(|e| matches!(e.geometry, Geometry::Viewport { .. })).map(|e| e.geometry.clone()).collect()
    }

    #[test]
    fn corners_make_a_viewport_onto_the_model_extents() {
        let mut doc = doc_with_layout();
        let mut engine = Engine::new();
        run(&mut engine, &mut doc, &["MV", "10,10", "110,60"]);
        let [Geometry::Viewport { center, width, height, view_center, view_height, on, locked, .. }] =
            viewports(&doc)[..]
        else {
            panic!("one viewport: {:?}", viewports(&doc));
        };
        assert_eq!((center, width, height), (Point::new(60.0, 35.0), 100.0, 50.0));
        // The model's 1000 x 500 fits 100 x 50 exactly.
        assert_eq!((view_center, view_height), (Point::new(500.0, 250.0), 500.0));
        assert!(on && !locked);
        // The model itself is untouched.
        assert_eq!(doc.space_extents(MODEL).unwrap().width(), 1000.0);
    }

    #[test]
    fn fit_fills_the_printable_area_and_lock_and_off_change_it() {
        let mut doc = doc_with_layout();
        let mut engine = Engine::new();
        run(&mut engine, &mut doc, &["MVIEW", "F"]);
        let [Geometry::Viewport { center, width, height, .. }] = viewports(&doc)[..] else { panic!() };
        assert_eq!((center, width, height), (Point::new(138.5, 95.0), 277.0, 190.0));

        run(&mut engine, &mut doc, &["MVIEW", "L", "ON"]);
        let edge = Point::new(138.5, 0.0);
        engine.click(edge, edge, false, &mut doc, 1.0);
        run(&mut engine, &mut doc, &[""]);
        run(&mut engine, &mut doc, &["MVIEW", "OFF"]);
        engine.click(edge, edge, false, &mut doc, 1.0);
        run(&mut engine, &mut doc, &[""]);
        let [Geometry::Viewport { on, locked, .. }] = viewports(&doc)[..] else { panic!() };
        assert!(locked && !on);
    }

    #[test]
    fn not_in_model_space() {
        let mut doc = Document::new();
        let mut engine = Engine::new();
        run(&mut engine, &mut doc, &["MVIEW"]);
        assert!(engine.history().iter().any(|l| l.contains("not allowed in Model Tab")));
        assert!(!engine.is_active());
    }
}

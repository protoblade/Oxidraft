//! Model space through a layout's viewport. Double-clicking inside a
//! viewport (or MSPACE) makes it the one worked in: the sheet stays on
//! screen, and drawing, zooming and panning happen in model space inside
//! its frame. VPMAX fills the drawing area with it instead. Double-clicking
//! the paper outside it (or PSPACE, VPMIN) goes back to the sheet; the
//! viewport keeps the view it was left with, unless it's locked.

use eframe::egui::{Pos2, Rect};
use oxidraft_geom::Point;
use oxidraft_model::{Document, EntityId, Geometry, MODEL, Space};

use crate::{app::OxidraftApp, view::View};

/// The viewport being worked in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ActiveViewport {
    pub layout: Space,
    pub id: EntityId,
    /// The layout's view, for its sheet around the viewport and for when
    /// it's left.
    pub page: View,
    /// VPMAX: the viewport fills the drawing area.
    pub maximized: bool,
}

/// A viewport's frame on the sheet and the part of model space it shows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Frame {
    pub center: Point,
    pub width: f64,
    pub height: f64,
    pub view_center: Point,
    pub view_height: f64,
    pub locked: bool,
    pub frozen: Vec<String>,
}

impl Frame {
    pub fn of(doc: &Document, id: EntityId) -> Option<Frame> {
        match &doc.entity(id)?.geometry {
            Geometry::Viewport { center, width, height, view_center, view_height, locked, frozen, .. }
                if *view_height > 0.0 && *height > 0.0 =>
            {
                Some(Frame {
                    center: *center,
                    width: *width,
                    height: *height,
                    view_center: *view_center,
                    view_height: *view_height,
                    locked: *locked,
                    frozen: frozen.clone(),
                })
            }
            _ => None,
        }
    }

    /// Paper units per model unit.
    fn k(&self) -> f64 {
        self.height / self.view_height
    }

    /// The view of model space that lines up with the viewport, the sheet
    /// being seen through `page`.
    pub fn model_view(&self, page: View) -> View {
        let k = self.k();
        View { scale: page.scale * k, center: self.view_center - (self.center - page.center) * (1.0 / k) }
    }

    /// The view center and height the viewport has when `model` lines up
    /// with it, the sheet being seen through `page`.
    pub fn view_of(&self, page: View, model: View) -> (Point, f64) {
        let k = model.scale / page.scale;
        (model.center + (self.center - page.center) * (1.0 / k), self.height / k)
    }

    /// The frame on screen, the sheet being seen through `page`.
    pub fn on_screen(&self, page: View, rect: Rect) -> Rect {
        let half = Point::new(self.width / 2.0, self.height / 2.0);
        Rect::from_two_pos(page.to_screen(rect, self.center - half), page.to_screen(rect, self.center + half))
    }
}

impl OxidraftApp {
    /// Works in model space through viewport `id` of the current layout.
    pub(crate) fn enter_viewport(&mut self, id: EntityId, maximized: bool) {
        if let Some(a) = self.active_viewport {
            if a.id == id && a.maximized == maximized {
                return;
            }
            // VPMAX on the viewport worked in: the same view, larger.
            self.leave_viewport();
        }
        let layout = self.doc.space();
        let Some(frame) = Frame::of(&self.doc, id) else { return };
        if self.doc.layout(layout).is_none() {
            return;
        }
        // A command and the selection belong to the space they were in.
        self.engine.cancel(&mut self.doc);
        self.engine.cancel(&mut self.doc);
        let page = self.view;
        self.doc.set_space(MODEL);
        self.view = if maximized {
            let height = f64::from(self.canvas_rect.height()).max(1.0);
            View { center: frame.view_center, scale: height / frame.view_height }
        } else {
            frame.model_view(page)
        };
        self.view_history.clear();
        self.last_viewport.insert(layout, id);
        self.active_viewport = Some(ActiveViewport { layout, id, page, maximized });
    }

    /// Back to the layout's sheet. The viewport keeps the view it was left
    /// with, unless it's locked.
    pub(crate) fn leave_viewport(&mut self) {
        let Some(a) = self.active_viewport.take() else { return };
        self.engine.cancel(&mut self.doc);
        self.engine.cancel(&mut self.doc);
        if let Some(frame) = Frame::of(&self.doc, a.id)
            && !frame.locked
        {
            let (center, height) = if a.maximized {
                (self.view.center, f64::from(self.canvas_rect.height()) / self.view.scale)
            } else {
                frame.view_of(a.page, self.view)
            };
            let tolerance = frame.view_height * 1e-9;
            if center.distance(frame.view_center) > tolerance || (height - frame.view_height).abs() > tolerance {
                self.doc.begin(if a.maximized { "VPMIN" } else { "PSPACE" });
                self.doc.update(a.id, |e| {
                    if let Geometry::Viewport { view_center, view_height, .. } = &mut e.geometry {
                        (*view_center, *view_height) = (center, height);
                    }
                });
                self.doc.commit();
            }
        }
        self.doc.set_space(a.layout);
        self.view = a.page;
        self.view_history.clear();
    }

    /// The layout shown on the status bar's tabs: the current one, or the
    /// one whose viewport is worked in.
    pub(crate) fn shown_space(&self) -> Space {
        self.active_viewport.map_or(self.doc.space(), |a| a.layout)
    }

    /// The active viewport's frame on screen, when the sheet is around it.
    pub(crate) fn active_frame(&self, rect: Rect) -> Option<(Frame, Rect)> {
        let a = self.active_viewport.filter(|a| !a.maximized)?;
        let frame = Frame::of(&self.doc, a.id)?;
        let on_screen = frame.on_screen(a.page, rect);
        Some((frame, on_screen))
    }

    /// MSPACE: the viewport worked in last, or the layout's first.
    pub(crate) fn model_space_command(&mut self) {
        if self.active_viewport.is_some() {
            return;
        }
        if self.doc.layout(self.doc.space()).is_none() {
            self.engine.log("** Command not allowed in Model Tab **");
            return;
        }
        let space = self.doc.space();
        let on = |id: &EntityId| {
            matches!(self.doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Viewport { on: true, .. }))
        };
        let last = self
            .last_viewport
            .get(&space)
            .copied()
            .filter(|id| on(id) && self.doc.entity(*id).is_some_and(|e| e.space == space));
        let first = || {
            self.doc
                .space_in_draw_order(space)
                .into_iter()
                .find(|e| matches!(e.geometry, Geometry::Viewport { on: true, .. }))
                .map(|e| e.id)
        };
        match last.or_else(first) {
            Some(id) => self.enter_viewport(id, false),
            None => self.engine.log("** There are no active model space viewports **"),
        }
    }

    /// VPMAX: the viewport worked in, a selected one, or the layout's only
    /// one fills the drawing area. Returns false when one is to be picked.
    pub(crate) fn maximize_command(&mut self) -> bool {
        if let Some(a) = self.active_viewport {
            self.enter_viewport(a.id, true);
            return true;
        }
        let space = self.doc.space();
        if self.doc.layout(space).is_none() {
            return false;
        }
        let viewport = |id: &EntityId| {
            self.doc.entity(*id).is_some_and(|e| matches!(e.geometry, Geometry::Viewport { on: true, .. }))
        };
        let selected: Vec<EntityId> = self.engine.selection().iter().copied().filter(viewport).collect();
        let all: Vec<EntityId> = self.doc.entities().map(|e| e.id).filter(viewport).collect();
        match (&selected[..], &all[..]) {
            ([id], _) | (_, [id]) => {
                let id = *id;
                self.enter_viewport(id, true);
                true
            }
            _ => false,
        }
    }

    /// VPMIN, PSPACE: back to the sheet.
    pub(crate) fn paper_space_command(&mut self, minimize: bool) {
        match self.active_viewport {
            Some(a) if a.maximized || !minimize => self.leave_viewport(),
            _ if minimize => self.engine.log("No viewport is maximized."),
            None if self.doc.layout(self.doc.space()).is_none() => {
                self.engine.log("** Command not allowed in Model Tab **")
            }
            _ => {}
        }
    }

    /// Double-clicking inside a viewport works in it; outside the one
    /// worked in, in another one or back on the sheet. Returns true if it
    /// switched.
    pub(crate) fn viewport_double_click(&mut self, pos: Pos2, rect: Rect) -> bool {
        if self.engine.is_active() {
            return false;
        }
        match self.active_viewport {
            Some(a) if a.maximized => false,
            Some(a) => {
                if self.active_frame(rect).is_some_and(|(_, r)| r.contains(pos)) {
                    return false;
                }
                let other = oxidraft_commands::viewport_at(&self.doc, a.layout, a.page.to_world(rect, pos));
                self.leave_viewport();
                if let Some(id) = other {
                    self.enter_viewport(id, false);
                }
                true
            }
            None => {
                let space = self.doc.space();
                if self.doc.layout(space).is_none() {
                    return false;
                }
                let at = self.view.to_world(rect, pos);
                // On a paper object (the viewport's own frame too): its properties.
                if oxidraft_model::snap::pick(&self.doc, at, self.pick_tolerance()).is_some() {
                    return false;
                }
                let Some(id) = oxidraft_commands::viewport_at(&self.doc, space, at) else { return false };
                self.enter_viewport(id, false);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_view_lines_up_with_the_frame_and_back() {
        let frame = Frame {
            center: Point::new(100.0, 80.0),
            width: 120.0,
            height: 60.0,
            view_center: Point::new(5000.0, 2000.0),
            view_height: 600.0,
            locked: false,
            frozen: Vec::new(),
        };
        let page = View { center: Point::new(140.0, 100.0), scale: 3.0 };
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1000.0, 700.0));
        let model = frame.model_view(page);
        // The view center shows at the frame's center, at 1:10.
        let at = model.to_screen(rect, frame.view_center);
        assert!((at - page.to_screen(rect, frame.center)).length() < 1e-3);
        assert!((model.scale - 0.3).abs() < 1e-12);
        let (center, height) = frame.view_of(page, model);
        assert!(center.distance(frame.view_center) < 1e-9 && (height - 600.0).abs() < 1e-9);
    }
}

//! The drawing area (model space): rendering and mouse handling.

use std::collections::HashMap;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, Event, FontId, MouseWheelSource, MouseWheelUnit, Painter, PointerButton, Pos2,
    Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use oxidraft_commands::{Tracked, ViewRequest};
use oxidraft_geom::{Aabb, Point, curve::Seg, intersect::clip_segment};
use oxidraft_model::{
    Geometry, HatchPattern, Lineweight, MODEL, Space, Vertex,
    annotation::text_box,
    hatch, linetype,
    snap::{self, Snap, SnapKind},
};

use crate::{
    app::OxidraftApp,
    icons::{self, Icon},
    theme,
    view::View,
    viewport::Frame,
};

/// AutoSnap aperture and pickbox radius, in screen points.
pub const APERTURE: f32 = 10.0;
/// CURSORSIZE: crosshair arm length as a percentage of the drawing area.
const CURSOR_SIZE: f32 = 5.0;
const GRIP_SIZE: f32 = 5.0;

/// A primary-button drag in progress.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Drag {
    /// Press-drag on empty space: a lasso through these points (drawing
    /// units), as in AutoCAD. Dragged off to the right it's a window lasso
    /// (objects inside it), to the left a crossing one (inside or
    /// crossing it); `mode` is set once the drag has gone either way, and
    /// Space cycles it through Window, Crossing and Fence.
    Lasso { points: Vec<Point>, mode: Option<LassoMode> },
    /// Press-drag on a grip: stretch it and drop it where the button is released.
    Grip,
}

/// What a lasso selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LassoMode {
    Window,
    Crossing,
    /// What the path crosses, as a fence.
    Fence,
}

impl LassoMode {
    pub(crate) fn next(self) -> LassoMode {
        match self {
            LassoMode::Window => LassoMode::Crossing,
            LassoMode::Crossing => LassoMode::Fence,
            LassoMode::Fence => LassoMode::Window,
        }
    }

    fn name(self) -> &'static str {
        match self {
            LassoMode::Window => "Window Lasso",
            LassoMode::Crossing => "Crossing Lasso",
            LassoMode::Fence => "Fence Lasso",
        }
    }
}

/// Selection cycling: a click on objects lying on top of each other lists
/// them, to pick the one meant (AutoCAD's Selection list).
pub(crate) struct Cycling {
    /// Where it was clicked, on screen and in the drawing, and whether
    /// Shift was down (to take the object out of the selection).
    screen: Pos2,
    at: Point,
    shift: bool,
    ids: Vec<oxidraft_model::EntityId>,
    /// The listed object the pointer is over, highlighted in the drawing.
    hovered: Option<oxidraft_model::EntityId>,
}

/// How far apart, in screen points, objects can be and still count as
/// lying on top of each other.
const OVERLAP: f32 = 2.0;

/// What the cursor resolves to this frame.
pub(crate) struct CursorState {
    screen: Pos2,
    /// Bare cursor position in drawing units.
    raw: Point,
    /// Point a click would enter: snapped, tracked or raw.
    point: Point,
    snap: Option<Snap>,
    tracked: Option<Tracked>,
}

impl OxidraftApp {
    /// The drawing area, or model space's current pane: drawing and
    /// working in it.
    pub(crate) fn canvas_pane(&mut self, ui: &mut Ui, rect: Rect) {
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        self.canvas_rect = rect;
        let painter = ui.painter_at(rect);

        // The viewport worked in was undone or erased: back on its sheet.
        if let Some(a) = self.active_viewport
            && Frame::of(&self.doc, a.id).is_none()
        {
            self.leave_viewport();
        }
        // The space changed under the view (LAYOUT, undo): its own view.
        if self.doc.space() != self.view_space {
            self.follow_space();
        }
        // Double-clicking in or out of a viewport, before the view is used.
        let switched = response.double_clicked_by(PointerButton::Primary)
            && response.interact_pointer_pos().is_some_and(|p| self.viewport_double_click(p, rect));

        // A newly opened drawing fits the view, with no earlier view to go back to.
        if self.zoom_extents_pending {
            self.zoom_extents_pending = false;
            match self.doc.extents() {
                Some(b) => self.view.zoom_extents(rect, fit_area(rect), b),
                None => self.view = View::default(),
            }
        }
        // In a locked viewport, zooming and panning move the sheet.
        let locked = self.active_frame(rect).map(|(f, _)| f).filter(|f| f.locked);
        if let (Some(_), Some(a)) = (&locked, self.active_viewport) {
            self.view = a.page;
        }
        let requests: Vec<ViewRequest> = self.pending_views.drain(..).chain(self.engine.take_view_requests()).collect();
        for r in requests {
            self.apply_view(r, rect);
        }
        self.navigate(ui, &response, rect);
        if let Some(f) = &locked
            && let Some(a) = self.active_viewport.as_mut()
        {
            a.page = self.view;
            self.view = f.model_view(a.page);
        }
        // VIEWSIZE and VIEWCTR, for ZOOM Center's default.
        self.engine.vars.view_size = f64::from(rect.height()) / self.view.scale;
        self.engine.vars.view_center = self.view.center;
        self.engine.vars.view_aspect = f64::from(rect.width() / rect.height().max(1.0));

        // While dragging a grip the pointer may leave the hover state; keep tracking it.
        let pointer = response.hover_pos().or_else(|| response.interact_pointer_pos());
        // In a viewport, the crosshair is only inside its frame.
        let frame = self.active_frame(rect).map(|(_, r)| r);
        let dragging = self.drag.is_some() || response.dragged();
        let pointer = pointer.filter(|p| dragging || frame.is_none_or(|r| r.contains(*p)));
        let cursor = pointer.map(|screen| self.resolve_cursor(rect, screen));
        self.cursor = cursor.as_ref().map(|c| c.point);
        // PNGOUT and the like: the drawing alone, no cursor, grid or grips.
        let capturing = self.capturing_image();
        let cursor = cursor.filter(|_| !capturing);
        // Real-time pan and zoom own the mouse; right-click ends them.
        if self.realtime.is_some() {
            if response.secondary_clicked() {
                self.realtime = None;
            }
        } else if !switched {
            self.handle_clicks(ui, &response, cursor.as_ref());
            // Right-click opens the shortcut menu (Enter, options, editing…). This
            // runs every frame: once open, the pointer is over the menu, not here.
            if response.secondary_clicked() {
                self.engine.cancel_window();
            }
            response.context_menu(|ui| self.shortcut_menu(ui));
        }

        // A layout: its sheet, and model space through its viewports. In a
        // viewport, the sheet stays around it (but not when maximized).
        let sheet_space = self.shown_space();
        let maximized = self.active_viewport.is_some_and(|a| a.maximized);
        let layout = if maximized { None } else { self.doc.layout(sheet_space).cloned() };
        let active = self.active_frame(rect);
        let model_view = self.view;
        if let (Some(_), Some(a)) = (&active, self.active_viewport) {
            self.view = a.page;
        }
        theme::set_paper(layout.is_some());
        match &layout {
            Some(l) => self.draw_sheet(&painter, rect, &l.paper),
            None if self.grid && !capturing => self.draw_grid(&painter, rect),
            None => {}
        }
        if layout.is_some() {
            self.draw_viewports(&painter, rect, sheet_space, self.active_viewport.map(|a| a.id));
        }
        self.view = model_view;
        // Model space in the viewport worked in shows inside its frame only.
        let (model, area, frozen) = match &active {
            Some((f, r)) => (painter.with_clip_rect(r.intersect(rect)), *r, f.frozen.clone()),
            None => (painter.clone(), rect, Vec::new()),
        };
        if self.engine.vars.ucs_icon && !capturing {
            self.draw_ucs_icon(&model, rect, area);
        }
        self.sheet_scale = self.active_viewport.filter(|_| active.is_some()).map(|a| a.page.scale);
        self.draw_entities(&model, rect, cursor.as_ref(), self.doc.space(), &frozen);
        // Comparing drawings: what's only in the other one, red.
        // XCOMPARE: what an xref's file has new, green; what it lost, red.
        if let Some(c) = self.xcomparison.as_ref().filter(|_| self.doc.space() == MODEL) {
            for g in &c.added {
                self.draw_geometry(&model, rect, g, Stroke::new(1.5, crate::compare::HERE));
            }
            for g in &c.removed {
                self.draw_geometry(&model, rect, g, Stroke::new(1.5, crate::compare::THERE));
            }
        }
        if let Some(c) = self.comparison.as_ref().filter(|_| self.doc.space() == MODEL) {
            for g in &c.only_there {
                self.draw_geometry(&model, rect, g, Stroke::new(1.5, crate::compare::THERE));
            }
        }
        self.sheet_scale = None;
        // The sheet's own objects over it, and the frame worked in.
        if let (Some((f, r)), Some(a)) = (&active, self.active_viewport) {
            f.mask_outside(&model, rect, a.page);
            self.view = a.page;
            self.draw_entities(&painter, rect, None, a.layout, &[]);
            self.view = model_view;
            painter.rect_stroke(*r, 0.0, Stroke::new(2.5, theme::accent()), StrokeKind::Middle);
        }
        if maximized {
            painter.rect_stroke(
                rect.shrink(1.5),
                0.0,
                Stroke::new(3.0, Color32::from_rgb(210, 40, 40)),
                StrokeKind::Inside,
            );
        }
        self.request_details();
        if let Some(c) = &cursor {
            for g in self.engine.preview(c.point) {
                self.draw_geometry(&model, rect, &g, Stroke::new(1.0, theme::preview()));
            }
        }
        if !capturing {
            self.draw_grips(&model, rect, cursor.as_ref());
        }
        if let Some(c) = &cursor {
            let dragging = response.dragged_by(PointerButton::Primary);
            let icon = match self.realtime {
                Some(ViewRequest::RealtimeZoom) => CursorIcon::ZoomIn,
                Some(_) if dragging => CursorIcon::Grabbing,
                Some(_) => CursorIcon::Grab,
                None if response.dragged_by(PointerButton::Middle) => CursorIcon::Grabbing,
                None => CursorIcon::None,
            };
            if self.realtime.is_none() {
                self.draw_window(&model, rect, c);
                self.draw_tracking(&model, rect, c);
                if let Some(s) = c.snap {
                    draw_snap_marker(&model, self.view.to_screen(rect, s.point), s.kind);
                }
                self.draw_cursor(&model, rect, c);
                self.draw_tooltips(&painter, rect, c);
            }
            ui.ctx().set_cursor_icon(icon);
        }
        let label = match self.doc.refedit().filter(|r| r.editor) {
            Some(r) => format!("[-][Top][2D Wireframe]   Block Editor - {}", r.block),
            None => "[-][Top][2D Wireframe]".into(),
        };
        if !capturing {
            painter.text(
                rect.left_top() + vec2(8.0, 6.0),
                Align2::LEFT_TOP,
                label,
                FontId::proportional(12.0),
                theme::label(),
            );
        }
        if self.engine.vars.nav_bar && !capturing {
            self.navigation_bar(ui, rect);
        }
        self.selection_list(ui.ctx());
        theme::set_paper(false);
    }

    /// A layout's sheet on its backdrop, its printable area dashed.
    fn draw_sheet(&self, painter: &Painter, rect: Rect, paper: &oxidraft_model::Paper) {
        painter.rect_filled(rect, 0.0, theme::backdrop());
        let (sheet, printable) = paper.sheet_bounds();
        let to = |p: Point| self.view.to_screen(rect, p);
        let on_screen = Rect::from_two_pos(to(sheet.min), to(sheet.max));
        painter.rect_filled(on_screen.translate(vec2(4.0, 4.0)), 0.0, Color32::from_black_alpha(90));
        painter.rect_filled(on_screen, 0.0, Color32::WHITE);
        let c = printable.corners();
        let edge = Stroke::new(1.0, Color32::from_gray(150));
        for i in 0..4 {
            let points = vec![to(c[i]), to(c[(i + 1) % 4])];
            painter.extend(Shape::dashed_line(&points, edge, 4.0, 3.0));
        }
    }

    /// Model space through the layout's viewports: each shows the part
    /// around its view center at its scale, without the layers it freezes.
    fn draw_viewports(&mut self, painter: &Painter, rect: Rect, space: Space, skip: Option<oxidraft_model::EntityId>) {
        let frames: Vec<Frame> = self
            .doc
            .space_in_draw_order(space)
            .into_iter()
            .filter(|e| self.doc.object_shown(e) && Some(e.id) != skip)
            .filter(|e| matches!(e.geometry, Geometry::Viewport { on: true, .. }))
            .filter_map(|e| Frame::of(&self.doc, e.id))
            .collect();
        let page = self.view;
        for frame in frames {
            let clip = frame.on_screen(page, rect).intersect(rect);
            if clip.width() <= 0.0 || clip.height() <= 0.0 {
                continue;
            }
            // Model space as the viewport shows it: its view center at the
            // viewport's center, scaled from paper to model units and turned
            // by its twist.
            self.view = frame.model_view(page);
            self.sheet_scale = Some(page.scale);
            let inside = painter.with_clip_rect(clip);
            self.draw_entities(&inside, rect, None, MODEL, &frame.frozen);
            frame.mask_outside(&inside, rect, page);
            self.sheet_scale = None;
            self.view = page;
        }
    }

    /// AutoCAD's navigation bar at the right edge of the drawing: Pan, and
    /// Zoom with its options.
    fn navigation_bar(&mut self, ui: &Ui, rect: Rect) {
        let frame = egui::Frame::NONE
            .fill(theme::ribbon().gamma_multiply(0.9))
            .corner_radius(3)
            .inner_margin(egui::Margin::same(3));
        egui::Area::new(egui::Id::unique("navigation_bar"))
            .fixed_pos(rect.right_top() + vec2(-40.0, 34.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                frame.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let panning = self.realtime == Some(ViewRequest::RealtimePan);
                    if nav_button(ui, Icon::Pan, panning, "Pan (P)").clicked() {
                        self.realtime = if panning { None } else { Some(ViewRequest::RealtimePan) };
                    }
                    let zoom = nav_button(ui, Icon::Zoom, false, "Zoom");
                    let during_command = self.engine.is_active();
                    egui::Popup::menu(&zoom).show(|ui| self.zoom_items(ui, during_command));
                });
            });
    }

    /// True when clicks pick objects (selection sets or single-object prompts)
    /// rather than points: the cursor shows the pickbox and snaps are off.
    fn picks_objects(&self) -> bool {
        self.engine.is_selecting() || self.engine.is_picking()
    }

    /// Mouse wheel zooms at the cursor (as with a mouse in AutoCAD); a trackpad
    /// pans with two fingers and zooms with a pinch. Middle drag pans,
    /// double-clicking the middle button zooms to extents.
    fn navigate(&mut self, ui: &Ui, response: &Response, rect: Rect) {
        let before = self.view;
        if response.double_clicked_by(PointerButton::Middle) {
            self.pending_views.push(ViewRequest::Extents);
        }
        if response.dragged_by(PointerButton::Middle) {
            let d = response.drag_delta();
            self.view.pan(d.x, d.y);
        }
        // Real-time PAN drags the drawing along; real-time ZOOM zooms in as
        // you drag up and out as you drag down, about the view's center.
        if let Some(mode) = self.realtime
            && response.dragged_by(PointerButton::Primary)
        {
            let d = response.drag_delta();
            match mode {
                ViewRequest::RealtimeZoom => self.view.zoom_at(rect, rect.center(), f64::from((-d.y / 150.0).exp())),
                _ => self.view.pan(d.x, d.y),
            }
        }
        self.remember_gesture(ui, before);
        let Some(hover) = response.hover_pos() else { return };
        let before = self.view;
        let events = ui.input(|i| i.events.clone());
        for e in events {
            match e {
                Event::MouseWheel { unit, delta, source, modifiers, .. } => {
                    let wheel = match source {
                        MouseWheelSource::Wheel => true,
                        MouseWheelSource::Unknown => unit != MouseWheelUnit::Point,
                        _ => false,
                    };
                    if wheel {
                        let notches = if unit == MouseWheelUnit::Point { delta.y / 50.0 } else { delta.y };
                        self.view.zoom_at(rect, hover, 1.25f64.powf(f64::from(notches)));
                    } else if modifiers.command || modifiers.ctrl {
                        self.view.zoom_at(rect, hover, f64::from((delta.y / 200.0).exp()));
                    } else {
                        self.view.pan(delta.x, delta.y);
                    }
                }
                Event::Zoom(factor) => self.view.zoom_at(rect, hover, f64::from(factor)),
                _ => {}
            }
        }
        self.remember_gesture(ui, before);
    }

    /// A run of wheel, pinch and drag moves is one step for ZOOM Previous:
    /// the view before it is remembered once the view has been still for a second.
    fn remember_gesture(&mut self, ui: &Ui, before: View) {
        if self.view == before {
            return;
        }
        let now = ui.input(|i| i.time);
        if now - self.last_gesture > 1.0 {
            self.remember_view(before);
        }
        self.last_gesture = now;
    }

    /// Keeps a view for ZOOM Previous (the last 25, as AutoCAD keeps 10).
    pub(crate) fn remember_view(&mut self, v: View) {
        if self.view_history.last() != Some(&v) {
            self.view_history.push(v);
            if self.view_history.len() > 25 {
                self.view_history.remove(0);
            }
        }
    }

    /// Carries out a ZOOM option, a PAN, or a menu's zoom.
    fn apply_view(&mut self, request: ViewRequest, rect: Rect) {
        let before = self.view;
        // In a viewport (with the sheet around it), zooms fit its frame.
        let frame = self.active_frame(rect).filter(|(f, _)| !f.locked).map(|(_, r)| r);
        let fit = frame.unwrap_or_else(|| fit_area(rect));
        let area = frame.unwrap_or(rect);
        match request {
            ViewRequest::Extents => match self.doc.extents() {
                Some(b) => self.view.zoom_extents(rect, fit, b),
                None => self.view = View::default(),
            },
            ViewRequest::Window(b) => self.view.zoom_extents(rect, fit, b),
            ViewRequest::Previous => {
                match self.view_history.pop() {
                    Some(v) => self.view = v,
                    None => self.engine.log("No previous view saved."),
                }
                return;
            }
            ViewRequest::Magnify(f) => self.view.zoom_at(rect, area.center(), f),
            ViewRequest::OfExtents(f) => {
                let mut fitted = self.view;
                if let Some(b) = self.doc.extents() {
                    fitted.zoom_extents(rect, fit, b);
                }
                self.view.scale = (fitted.scale * f).clamp(1e-6, 1e9);
            }
            ViewRequest::Center { center, height } => {
                self.view.scale = (f64::from(area.height()) / height.max(1e-9)).clamp(1e-6, 1e9);
                let offset = area.center() - rect.center();
                self.view.center =
                    center - Point::new(f64::from(offset.x) / self.view.scale, -f64::from(offset.y) / self.view.scale);
            }
            ViewRequest::RealtimeZoom | ViewRequest::RealtimePan => {
                self.realtime = Some(request);
                return;
            }
            // The drawing turned so the UCS's X axis runs across, all of it
            // in view.
            ViewRequest::Plan(angle) => {
                self.view.rotation = -angle;
                if let Some(b) = self.doc.extents() {
                    self.view.zoom_extents(rect, fit, b);
                }
            }
        }
        self.remember_view(before);
    }

    fn resolve_cursor(&self, rect: Rect, screen: Pos2) -> CursorState {
        let raw = self.view.to_world(rect, screen);
        let tolerance = self.pick_tolerance();
        if self.picks_objects() {
            return CursorState { screen, raw, point: raw, snap: None, tracked: None };
        }
        let from = self.engine.base_point();
        let snap = match self.snap_override {
            Some(Some(kind)) => snap::find_override_snap(&self.doc, raw, tolerance, kind, from),
            Some(None) => None,
            None if self.osnap => snap::find_snap(&self.doc, raw, tolerance, &self.osnap_modes, from),
            None => None,
        };
        if let Some(s) = snap {
            return CursorState { screen, raw, point: s.point, snap: Some(s), tracked: None };
        }
        // Snap mode (F9): to the snap spacing, where no object snap applies.
        let vars = &self.engine.vars;
        let on_snap = match (vars.snap && vars.snap_spacing > 0.0, vars.isoplane) {
            // Isometric snap: the points `k` apart along 30° and 150°.
            (true, Some(_)) => {
                let k = vars.snap_spacing;
                let (c, h) = (30f64.to_radians().cos() * k, 30f64.to_radians().sin() * k);
                let (u, v) = (raw.x / c, raw.y / h);
                let (i, j) = (((u + v) / 2.0).round(), ((v - u) / 2.0).round());
                Point::new((i - j) * c, (i + j) * h)
            }
            // On the UCS's snap grid.
            (true, None) => {
                let (k, ucs) = (vars.snap_spacing, self.doc.ucs());
                let p = ucs.to_ucs(raw);
                ucs.to_world(Point::new((p.x / k).round() * k, (p.y / k).round() * k))
            }
            (false, _) => raw,
        };
        let tracked = self.engine.track(on_snap, tolerance);
        CursorState { screen, raw, point: tracked.point, snap: None, tracked: Some(tracked) }
    }

    fn handle_clicks(&mut self, ui: &Ui, response: &Response, cursor: Option<&CursorState>) {
        let Some(c) = cursor else { return };
        let tolerance = self.pick_tolerance();
        let shift = ui.input(|i| i.modifiers.shift);

        // Cmd- or Ctrl-click follows an object's hyperlink.
        let follow = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        if follow
            && response.clicked()
            && !self.engine.is_active()
            && let Some(link) = snap::pick(&self.doc, c.raw, tolerance)
                .and_then(|id| self.doc.entity(id))
                .and_then(|e| e.hyperlink.clone())
        {
            let url = self.link_target(&link.url);
            self.engine.log(format!("Opening {url}"));
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            return;
        }
        // While the Selection list is open, a click in the drawing closes it.
        if self.cycling.is_some() {
            if response.clicked() || response.drag_started() {
                self.cycling = None;
            }
            return;
        }
        // Press-drag on a grip stretches it; on empty space it draws a lasso.
        if response.drag_started_by(PointerButton::Primary)
            && self.engine.is_selecting()
            && self.engine.window_start().is_none()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let start = self.view.to_world(self.canvas_rect, origin);
            let on_grip = !self.engine.is_active()
                && !shift
                && self.engine.grips(&self.doc).iter().any(|g| g.distance(start) <= tolerance);
            if on_grip {
                self.engine.click(start, start, false, &mut self.doc, tolerance);
                self.drag = Some(Drag::Grip);
            } else if snap::pick(&self.doc, start, tolerance).is_none() {
                self.drag = Some(Drag::Lasso { points: vec![start], mode: None });
            }
        }
        // The lasso follows the pointer: a point every few screen points.
        if let Some(Drag::Lasso { points, mode }) = &mut self.drag
            && let Some(&last) = points.last()
        {
            let (view, rect) = (self.view, self.canvas_rect);
            if view.to_screen(rect, last).distance(c.screen) >= 3.0 {
                points.push(c.raw);
            }
            let dx = c.screen.x - view.to_screen(rect, points[0]).x;
            if mode.is_none() && dx.abs() >= 4.0 {
                *mode = Some(if dx < 0.0 { LassoMode::Crossing } else { LassoMode::Window });
            }
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            match self.drag.take() {
                Some(Drag::Lasso { mut points, mode }) => {
                    points.push(c.raw);
                    match mode.unwrap_or(LassoMode::Window) {
                        LassoMode::Fence => self.engine.select_fence(points, shift, &mut self.doc, tolerance),
                        m => {
                            self.engine.select_lasso(points, m == LassoMode::Crossing, shift, &mut self.doc, tolerance)
                        }
                    }
                }
                // Dropped with snaps and tracking applied, like a second click.
                Some(Drag::Grip) => self.engine.click(c.point, c.raw, false, &mut self.doc, tolerance),
                None => {}
            }
        }
        let picking_point = !self.picks_objects();
        if response.clicked_by(PointerButton::Primary) {
            // Objects on top of each other: a list to pick the one meant.
            let on_grip = !self.engine.is_active()
                && !shift
                && self.engine.grips(&self.doc).iter().any(|g| g.distance(c.raw) <= tolerance);
            // At a selection prompt, or one taking a single object.
            let selecting = self.engine.is_selecting() && self.engine.window_start().is_none() && !on_grip;
            if self.selection_cycling && (selecting || self.engine.is_picking()) {
                let ids = snap::pick_overlapping(&self.doc, c.raw, tolerance, self.view.units(OVERLAP));
                if ids.len() > 1 {
                    self.cycling = Some(Cycling { screen: c.screen, at: c.raw, shift, ids, hovered: None });
                    return;
                }
            }
            self.engine.click(c.point, c.raw, shift, &mut self.doc, tolerance);
            // A one-shot snap override is used up by the point it picked.
            if picking_point {
                self.snap_override = None;
            }
        }
        // Double-clicking an object opens its properties, as in AutoCAD (its
        // two clicks have already selected it); a block with attributes
        // opens the attribute editor instead.
        if response.double_clicked_by(PointerButton::Primary)
            && !self.engine.is_active()
            && let Some(id) = snap::pick(&self.doc, c.raw, tolerance)
            && !self.edit_attributes(id)
        {
            // A block (not an xref) opens in Edit Block Definition, as in AutoCAD.
            // An array opens its properties.
            let array = self.doc.array(id).is_some();
            match self.doc.entity(id).map(|e| &e.geometry) {
                Some(Geometry::Insert { block, .. }) if self.doc.xref(block).is_none() && !array => {
                    let block = block.clone();
                    self.open_block_edit_dialog(Some(block));
                }
                // A table opens to have its cells edited, text in the text editor.
                Some(Geometry::Table { .. }) => self.open_table_editor(id),
                Some(Geometry::Text { .. }) => self.open_text_editor(id),
                _ => self.properties_open = true,
            }
        }
    }

    // ---- drawing ----------------------------------------------------------------

    /// Adaptive grid: minor lines with a major line every 5 (GRIDMAJOR).
    pub(crate) fn draw_grid(&self, painter: &Painter, rect: Rect) {
        // A set spacing while it's at least 4 pixels apart; else adaptive.
        let (minor, major) = match self.grid_spacing {
            Some(s) if s * self.view.scale >= 4.0 => (s, s * 5.0),
            _ => self.view.grid_step(10.0),
        };
        if self.engine.vars.isoplane.is_some() {
            return self.draw_iso_grid(painter, rect, minor);
        }
        if !self.doc.ucs().is_world() || self.view.rotation != 0.0 {
            return self.draw_turned_grid(painter, rect, minor, major);
        }
        let min = self.view.to_world(rect, rect.left_bottom());
        let max = self.view.to_world(rect, rect.right_top());
        let ratio = (major / minor).round() as i64;
        let is_major = |v: f64| ((v / minor).round() as i64).rem_euclid(ratio) == 0;

        let mut x = (min.x / minor).floor() * minor;
        while x <= max.x {
            let sx = self.view.to_screen(rect, Point::new(x, 0.0)).x.round() + 0.5;
            let color = if x.abs() < minor * 1e-6 {
                theme::axis_y()
            } else if is_major(x) {
                theme::grid_major()
            } else {
                theme::grid_minor()
            };
            painter.line_segment([pos2(sx, rect.top()), pos2(sx, rect.bottom())], Stroke::new(1.0, color));
            x += minor;
        }
        let mut y = (min.y / minor).floor() * minor;
        while y <= max.y {
            let sy = self.view.to_screen(rect, Point::new(0.0, y)).y.round() + 0.5;
            let color = if y.abs() < minor * 1e-6 {
                theme::axis_x()
            } else if is_major(y) {
                theme::grid_major()
            } else {
                theme::grid_minor()
            };
            painter.line_segment([pos2(rect.left(), sy), pos2(rect.right(), sy)], Stroke::new(1.0, color));
            y += minor;
        }
    }

    /// The grid along the UCS's axes, however it and the view are turned.
    fn draw_turned_grid(&self, painter: &Painter, rect: Rect, minor: f64, major: f64) {
        let ucs = self.doc.ucs();
        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
            .map(|c| ucs.to_ucs(self.view.to_world(rect, c)));
        let lo = Point::new(
            corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
        );
        let hi = Point::new(
            corners.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
            corners.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
        );
        if ((hi.x - lo.x) / minor + (hi.y - lo.y) / minor) > 4000.0 {
            return;
        }
        let ratio = (major / minor).round() as i64;
        let color = |v: f64, axis: Color32| {
            let k = (v / minor).round() as i64;
            if k == 0 {
                axis
            } else if k.rem_euclid(ratio) == 0 {
                theme::grid_major()
            } else {
                theme::grid_minor()
            }
        };
        let line = |a: Point, b: Point, c: Color32| {
            let (a, b) = (self.view.to_screen(rect, ucs.to_world(a)), self.view.to_screen(rect, ucs.to_world(b)));
            painter.line_segment([a, b], Stroke::new(1.0, c));
        };
        let mut x = (lo.x / minor).floor() * minor;
        while x <= hi.x {
            line(Point::new(x, lo.y), Point::new(x, hi.y), color(x, theme::axis_y()));
            x += minor;
        }
        let mut y = (lo.y / minor).floor() * minor;
        while y <= hi.y {
            line(Point::new(lo.x, y), Point::new(hi.x, y), color(y, theme::axis_x()));
            y += minor;
        }
    }

    /// The isometric grid: lines along 30°, 90° and 150°, through the
    /// points `spacing` apart along the isometric axes.
    fn draw_iso_grid(&self, painter: &Painter, rect: Rect, spacing: f64) {
        let gap = spacing * 30f64.to_radians().cos();
        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
            .map(|c| self.view.to_world(rect, c));
        let center = self.view.to_world(rect, rect.center());
        let reach = corners[0].distance(corners[2]);
        for along in [30.0, 90.0, 150.0] {
            let (d, n) = (Point::ORIGIN.polar(1.0, along), Point::ORIGIN.polar(1.0, along + 90.0));
            let offsets = corners.map(|c| c.dot(n) / gap);
            let lo = offsets.iter().copied().fold(f64::INFINITY, f64::min).floor() as i64;
            let hi = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil() as i64;
            if hi - lo > 2000 {
                continue;
            }
            for k in lo..=hi {
                let p = n * (k as f64 * gap);
                let mid = p + d * d.dot(center - p);
                let (a, b) = (self.view.to_screen(rect, mid - d * reach), self.view.to_screen(rect, mid + d * reach));
                let color = if k.rem_euclid(5) == 0 { theme::grid_major() } else { theme::grid_minor() };
                painter.line_segment([a, b], Stroke::new(1.0, color));
            }
        }
    }

    /// 2D UCS icon: at the origin when it's on screen, else in the lower-left corner.
    /// At the origin when it's on screen in `area` (the drawing area, or
    /// the viewport worked in), else at `area`'s lower left.
    pub(crate) fn draw_ucs_icon(&self, painter: &Painter, rect: Rect, area: Rect) {
        // At the UCS's origin when that's in view, its axes as they lie.
        let ucs = self.doc.ucs();
        let o = self.view.to_screen(rect, ucs.origin);
        let inner = area.shrink2(vec2(60.0, 60.0));
        let at = if inner.contains(o) {
            o
        } else if area == rect {
            pos2(rect.left() + 48.0, rect.bottom() - 70.0)
        } else {
            pos2(area.left() + 34.0, area.bottom() - 34.0)
        };
        let screen_dir = |angle: f64| {
            let d = self.view.to_screen(rect, ucs.origin + Point::ORIGIN.polar(1.0, angle)) - o;
            d / d.length().max(1e-6)
        };
        let (xd, yd) = (screen_dir(ucs.angle), screen_dir(ucs.angle + 90.0));
        let len = 42.0;
        let x_col = Color32::from_rgb(230, 80, 80);
        let y_col = Color32::from_rgb(80, 210, 90);
        arrow(painter, at, at + xd * len, x_col);
        arrow(painter, at, at + yd * len, y_col);
        // The square at the origin: the world coordinate system.
        if ucs.is_world() {
            painter.rect_stroke(
                Rect::from_center_size(at, vec2(7.0, 7.0)),
                0.0,
                Stroke::new(1.0, theme::label()),
                StrokeKind::Middle,
            );
        }
        painter.text(at + xd * (len + 8.0), Align2::CENTER_CENTER, "X", FontId::proportional(11.0), x_col);
        painter.text(at + yd * (len + 8.0), Align2::CENTER_CENTER, "Y", FontId::proportional(11.0), y_col);
    }

    /// Draws `space`'s objects (but those on `frozen` layers).
    pub(crate) fn draw_entities(
        &self,
        painter: &Painter,
        rect: Rect,
        cursor: Option<&CursorState>,
        space: Space,
        frozen: &[String],
    ) {
        let selected = if self.capturing_image() { Vec::new() } else { self.engine.highlighted() };
        // Rollover highlighting: the object under the pickbox gets thicker, and
        // so does everything a selection window in progress would select
        // (AutoCAD's selection preview).
        let mut hovered: Vec<_> = match (cursor, self.engine.window_start(), &self.drag) {
            (Some(c), _, Some(Drag::Lasso { points, mode })) => {
                let ring: Vec<Point> = points.iter().copied().chain([c.raw]).collect();
                match mode.unwrap_or(LassoMode::Window) {
                    LassoMode::Fence => snap::fence_select(&self.doc, &ring),
                    m => snap::polygon_select(&self.doc, &ring, m == LassoMode::Crossing),
                }
            }
            (Some(c), Some(first), _) => snap::window_select(&self.doc, first, c.raw),
            (Some(c), None, _) if self.picks_objects() => {
                snap::pick(&self.doc, c.raw, self.pick_tolerance()).into_iter().collect()
            }
            _ => Vec::new(),
        };
        // The object pointed at in the Selection list.
        hovered.extend(self.cycling.as_ref().and_then(|c| c.hovered));
        let view = self.world_rect(rect);
        // Dash patterns in screen pixels by linetype name, looked up once a frame.
        let mut patterns: HashMap<String, Option<Vec<f64>>> = HashMap::new();
        let px_per_unit = self.doc.ltscale() * self.sheet_scale.unwrap_or(self.view.scale);
        let refedit = self.doc.refedit();
        // Annotative objects: at a viewport's scale through it, 1:1 on a
        // sheet, at the current annotation scale in model space.
        let anno = match self.sheet_scale {
            Some(sheet) if space == MODEL => Some(sheet / self.view.scale),
            _ if space != MODEL => Some(1.0),
            _ => None,
        };
        for e in self.doc.space_in_draw_order(space) {
            let Some(e) = self.doc.shown_at(e, anno) else { continue };
            let e = e.as_ref();
            if !self.shows_layer(&e.layer) || !self.doc.object_shown(e) || !on_screen(&self.doc.entity_bounds(e), &view)
            {
                continue;
            }
            if frozen.iter().any(|f| f.eq_ignore_ascii_case(&e.layer)) {
                continue;
            }
            // REFEDIT: the reference being edited shows as its working set,
            // and the rest of the drawing fades (XFADECTL).
            if refedit.is_some_and(|r| r.reference == e.id) {
                continue;
            }
            // The Block Editor shows the block alone.
            if refedit.is_some_and(|r| r.editor && !r.in_working_set(e.id)) {
                continue;
            }
            let faded = refedit.is_some_and(|r| !r.in_working_set(e.id));
            // Attached drawings draw faded (AutoCAD's XDWGFADECTL).
            let xref = matches!(&e.geometry, Geometry::Insert { block, .. } if self.doc.xref(block).is_some());
            // Comparing drawings: green only here, gray in both.
            let compared = self.comparison.as_ref().filter(|_| space == MODEL);
            let compare_color =
                compared.map(|c| if c.only_here.contains(&e.id) { crate::compare::HERE } else { crate::compare::BOTH });
            // TRANSPARENCY: see-through by its own or its layer's.
            let opacity = 1.0 - f32::from(self.doc.transparency_of(e)) / 100.0;
            for s in self.doc.expanded_styled(e) {
                let mut color = compare_color.unwrap_or_else(|| theme::aci(s.color));
                if opacity < 1.0 {
                    color = color.gamma_multiply(opacity);
                }
                // Locked layers draw faded (AutoCAD's LAYLOCKFADECTL).
                if self.doc.layer(&s.layer).is_some_and(|l| l.locked) {
                    color = color.gamma_multiply(0.45);
                }
                if xref {
                    color = color.gamma_multiply(0.6);
                }
                if faded {
                    color = color.gamma_multiply(0.5);
                }
                let pattern = patterns.entry(s.linetype.clone()).or_insert_with(|| {
                    let lt = self.doc.linetype(&s.linetype).filter(|l| !l.is_continuous())?;
                    // Too fine to see at this zoom: draw it solid, as AutoCAD does.
                    (lt.length() * px_per_unit >= 3.0).then(|| lt.pattern.iter().map(|d| d * px_per_unit).collect())
                });
                let width = if self.show_lineweights { lineweight_px(s.lineweight) } else { 1.0 };
                let g = &s.geometry;
                if selected.contains(&e.id) {
                    self.draw_geometry(painter, rect, g, Stroke::new(width + 4.0, theme::selection_glow()));
                    self.draw_dashed(painter, rect, g, Stroke::new(width, color), pattern.as_deref());
                } else if hovered.contains(&e.id) {
                    self.draw_dashed(painter, rect, g, Stroke::new(width.max(2.5), color), pattern.as_deref());
                } else {
                    self.draw_dashed(painter, rect, g, Stroke::new(width, color), pattern.as_deref());
                }
            }
            if let Some(frame) = self.doc.clip_frame(&e.geometry) {
                let color = theme::aci(self.doc.layer(&e.layer).map_or(7, |l| l.color));
                let width = if selected.contains(&e.id) || hovered.contains(&e.id) { 2.5 } else { 1.0 };
                self.draw_geometry(painter, rect, &frame, Stroke::new(width, color));
            }
        }
    }

    /// Draws a curve with a dash pattern given in screen pixels; text,
    /// fills and continuous curves draw as usual.
    fn draw_dashed(&self, painter: &Painter, rect: Rect, g: &Geometry, stroke: Stroke, pattern: Option<&[f64]>) {
        let wide = matches!(g, Geometry::Polyline { vertices, .. }
            if vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0));
        let Some(pattern) = pattern.filter(|_| !wide && !g.segments().is_empty()) else {
            return self.draw_geometry(painter, rect, g, stroke);
        };
        let path: Vec<Point> =
            self.screen_path(rect, g).into_iter().map(|p| Point::new(f64::from(p.x), f64::from(p.y))).collect();
        let to_pos = |p: &Point| pos2(p.x as f32, p.y as f32);
        for run in linetype::dashes(&path, pattern, 1.0) {
            if let [dot] = run[..] {
                painter.circle_filled(to_pos(&dot), (stroke.width / 2.0).max(0.8), stroke.color);
            } else {
                painter.add(Shape::line(run.iter().map(to_pos).collect(), stroke));
            }
        }
    }

    /// The part of the drawing the canvas shows.
    pub(crate) fn world_rect(&self, rect: Rect) -> Aabb {
        Aabb::from_points(self.view.to_world(rect, rect.left_bottom()), self.view.to_world(rect, rect.right_top()))
    }

    /// A curve's lines and arcs as one screen polyline, with arcs split
    /// finely enough to look smooth at the current zoom. Construction lines
    /// and rays are cut to the view.
    fn screen_path(&self, rect: Rect, g: &Geometry) -> Vec<Pos2> {
        if g.is_infinite() {
            let Some(Seg::Line(s)) = g.segments().first().copied() else { return Vec::new() };
            let view = self.world_rect(rect.expand(4.0));
            return clip_segment(s, &view)
                .map(|c| vec![self.view.to_screen(rect, c.start), self.view.to_screen(rect, c.end)])
                .unwrap_or_default();
        }
        let mut points: Vec<Pos2> = Vec::new();
        for seg in g.segments() {
            let steps = match seg {
                Seg::Line(_) => 1,
                Seg::Arc(a) => {
                    let screen_len = a.length() * self.view.scale;
                    (screen_len / 4.0).clamp(4.0, 720.0) as usize
                }
            };
            let first = usize::from(!points.is_empty());
            for i in first..=steps {
                points.push(self.view.to_screen(rect, seg.point_at(i as f64 / steps as f64)));
            }
        }
        points
    }

    fn draw_geometry(&self, painter: &Painter, rect: Rect, g: &Geometry, stroke: Stroke) {
        if let Some(parts) = g.primitives() {
            for part in &parts {
                self.draw_geometry(painter, rect, part, stroke);
            }
            return;
        }
        match g {
            Geometry::Circle { center, radius } => {
                let r = (radius * self.view.scale) as f32;
                painter.circle_stroke(self.view.to_screen(rect, *center), r, stroke);
                return;
            }
            Geometry::Point { position } => return self.draw_point(painter, rect, *position, stroke),
            Geometry::Text { .. } => return self.draw_text(painter, rect, g, stroke),
            Geometry::Hatch { .. } => return self.draw_hatch(painter, rect, g, stroke),
            Geometry::Image { .. } => return self.draw_image(painter, rect, g, stroke),
            Geometry::Wipeout { points } => return self.draw_wipeout(painter, rect, points, stroke),
            Geometry::Polyline { vertices, closed }
                if vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0) =>
            {
                return self.draw_wide(painter, rect, vertices, *closed, stroke);
            }
            _ => {}
        }
        let points = self.screen_path(rect, g);
        if points.len() >= 2 {
            painter.add(Shape::line(points, stroke));
        }
    }

    /// A wipeout: its polygon in the background's color, hiding what's
    /// under it, and its frame unless WIPEOUTFRAME hides it.
    fn draw_wipeout(&self, painter: &Painter, rect: Rect, points: &[Point], stroke: Stroke) {
        let mut mesh = egui::Mesh::default();
        let fill = theme::background_now();
        for t in oxidraft_model::hatch::triangles(&[points.to_vec()]) {
            let i = mesh.vertices.len() as u32;
            for p in t {
                mesh.colored_vertex(self.view.to_screen(rect, p), fill);
            }
            mesh.add_triangle(i, i + 1, i + 2);
        }
        painter.add(Shape::mesh(mesh));
        if self.doc.wipeout_frame() != 0 {
            let ring: Vec<Pos2> = points.iter().map(|p| self.view.to_screen(rect, *p)).collect();
            painter.add(Shape::closed_line(ring, stroke));
        }
    }

    /// A wide or tapered polyline, filled (AutoCAD's FILLMODE on). One width
    /// throughout strokes the whole path, with mitered corners; tapers fill
    /// each segment on its own. A thicker stroke (selection, rollover)
    /// widens it by the extra pixels.
    fn draw_wide(&self, painter: &Painter, rect: Rect, vertices: &[Vertex], closed: bool, stroke: Stroke) {
        let extra = (stroke.width - 1.0).max(0.0);
        let g = Geometry::Polyline { vertices: vertices.to_vec(), closed };
        // FILLMODE off: wide segments as their outlines.
        if !self.doc.fill() && stroke.width < 4.0 {
            for (seg, v) in g.segments().iter().zip(vertices) {
                let steps = match seg {
                    Seg::Line(_) => 1,
                    Seg::Arc(a) => ((a.length() * self.view.scale / 4.0).clamp(4.0, 720.0)) as usize,
                };
                let side = |sign: f64| -> Vec<Pos2> {
                    (0..=steps)
                        .map(|i| {
                            let t = i as f64 / steps as f64;
                            let half = (v.start_width + (v.end_width - v.start_width) * t) / 2.0;
                            let (p, d) = (seg.point_at(t), seg.tangent_at(t));
                            self.view.to_screen(rect, p + Point::new(-d.y, d.x) * (half * sign))
                        })
                        .collect()
                };
                let mut ring = side(1.0);
                ring.extend(side(-1.0).into_iter().rev());
                painter.add(Shape::closed_line(ring, Stroke::new(1.0, stroke.color)));
            }
            return;
        }
        if let Some(w) = oxidraft_model::constant_width(vertices) {
            let mut path = self.screen_path(rect, &g);
            if closed && path.len() > 2 && path[0].distance(path[path.len() - 1]) < 0.01 {
                path.pop();
            }
            let width = ((w * self.view.scale) as f32 + extra).max(stroke.width);
            let stroke = Stroke::new(width, stroke.color);
            painter.add(if closed { Shape::closed_line(path, stroke) } else { Shape::line(path, stroke) });
            return;
        }
        let mut mesh = egui::Mesh::default();
        for (seg, v) in g.segments().iter().zip(vertices) {
            let steps = match seg {
                Seg::Line(_) => 1,
                Seg::Arc(a) => ((a.length() * self.view.scale / 4.0).clamp(4.0, 720.0)) as usize,
            };
            let first = mesh.vertices.len() as u32;
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                let half = (v.start_width + (v.end_width - v.start_width) * t) / 2.0;
                let half = half.max(0.5 / self.view.scale) + f64::from(extra) / 2.0 / self.view.scale;
                let (p, d) = (seg.point_at(t), seg.tangent_at(t));
                let n = Point::new(-d.y, d.x) * half;
                mesh.colored_vertex(self.view.to_screen(rect, p + n), stroke.color);
                mesh.colored_vertex(self.view.to_screen(rect, p - n), stroke.color);
            }
            for i in 0..steps as u32 {
                let k = first + 2 * i;
                mesh.add_triangle(k, k + 1, k + 2);
                mesh.add_triangle(k + 1, k + 3, k + 2);
            }
        }
        painter.add(Shape::mesh(mesh));
    }

    /// A point in the PDMODE style and PDSIZE size: by default a dot, and
    /// figures 5% of the view's height.
    fn draw_point(&self, painter: &Painter, rect: Rect, position: Point, stroke: Stroke) {
        let at = self.view.to_screen(rect, position);
        let (mode, size) = (self.doc.pdmode(), self.doc.pdsize());
        let full = match size {
            s if s > 0.0 => (s * self.view.scale) as f32,
            s if s < 0.0 => rect.height() * (-s) as f32 / 100.0,
            _ => rect.height() * 0.05,
        };
        paint_point(painter, at, mode, full, stroke);
    }

    /// Text at its attachment point. AutoCAD's text height is the capital
    /// height, about 0.7 of the font size.
    fn draw_text(&self, painter: &Painter, rect: Rect, g: &Geometry, stroke: Stroke) {
        let Geometry::Text { position, height, rotation, width, attach, content, look } = g else { return };
        let anchor = self.view.to_screen(rect, *position);
        // A selected text glows as a box instead.
        if stroke.width >= 4.0 {
            let corners = text_box(*position, *height, *rotation, *width, *attach, content, look.width_factor);
            let pts: Vec<Pos2> = corners.iter().map(|p| self.view.to_screen(rect, *p)).collect();
            painter.add(Shape::convex_polygon(pts, stroke.color.gamma_multiply(0.5), Stroke::NONE));
            return;
        }
        // QTEXTMODE: each text as its box.
        if self.doc.quick_text() {
            let corners = text_box(*position, *height, *rotation, *width, *attach, content, look.width_factor);
            let pts: Vec<Pos2> = corners.iter().map(|p| self.view.to_screen(rect, *p)).collect();
            painter.add(Shape::closed_line(pts, Stroke::new(1.0, stroke.color)));
            return;
        }
        let px = (*height * self.view.scale / 0.7) as f32;
        if px < 1.5 {
            return;
        }
        // An SHX font's text: its strokes, as AutoCAD draws them.
        let font = self.doc.text_style(&look.style).map_or("", |s| s.font.as_str());
        if let Some(shx) = oxidraft_plot::shx::shx_font(font) {
            for s in oxidraft_plot::shx::text_strokes(shx, g) {
                let pts: Vec<Pos2> = s.iter().map(|p| self.view.to_screen(rect, *p)).collect();
                painter.add(Shape::line(pts, stroke));
            }
            return;
        }
        // Few distinct sizes keep egui's glyph cache small while zooming.
        let size = 2f32.powf((px.min(400.0).log2() * 8.0).round() / 8.0);
        let text = oxidraft_model::annotation::plain_text(content);
        let wrap = if *width > 0.0 { (*width * self.view.scale) as f32 } else { f32::INFINITY };
        let galley = painter.layout(text, crate::fonts::font_id(painter.ctx(), font, size), stroke.color, wrap);
        // Anchor on the type itself, not egui's line box: "top" is the first
        // line's capital height, "bottom" the last line's baseline.
        let baseline = |row: &egui::epaint::text::PlacedRow| {
            row.pos.y + row.row.glyphs.first().map_or(row.row.size.y * 0.8, |g| g.pos.y)
        };
        let first = galley.rows.first().map_or(0.0, baseline);
        let last = galley.rows.last().map_or(0.0, baseline);
        let (top, width) = (first - size * 0.7, galley.size().x);
        let (fx, fy) = attach.fractions();
        let offset = vec2(fx as f32 * width, top + fy as f32 * (last - top));
        // World angles run counterclockwise; egui's clockwise on a y-down screen.
        let angle = -((*rotation + self.view.rotation) as f32).to_radians();
        let pos = anchor - egui::emath::Rot2::from_angle(angle) * offset;
        let shape = egui::epaint::TextShape::new(pos, galley, stroke.color).with_angle(angle);
        paint_text(painter, shape, anchor, look.width_factor as f32, look.oblique as f32, first - offset.y);
    }

    /// Solid fills as triangles; patterns as lines clipped to the boundary
    /// (too fine to see at this zoom: a faint fill instead).
    fn draw_hatch(&self, painter: &Painter, rect: Rect, g: &Geometry, stroke: Stroke) {
        let Geometry::Hatch { loops, pattern, scale, angle, origin } = g else { return };
        let polys = hatch::polygons(loops);
        // FILLMODE off: solid and gradient fills as their outlines.
        if stroke.width >= 4.0 || !self.doc.fill() && pattern.is_fill() {
            for poly in &polys {
                let pts: Vec<Pos2> = poly.iter().map(|p| self.view.to_screen(rect, *p)).collect();
                painter.add(Shape::closed_line(pts, stroke));
            }
            return;
        }
        let spacing_px = pattern.families().iter().map(|f| f.1).fold(f64::INFINITY, f64::min) * scale * self.view.scale;
        let fill = |color: egui::Color32| {
            let mut mesh = egui::Mesh::default();
            for t in hatch::triangles(&polys) {
                let i = mesh.vertices.len() as u32;
                for p in t {
                    mesh.colored_vertex(self.view.to_screen(rect, p), color);
                }
                mesh.add_triangle(i, i + 1, i + 2);
            }
            painter.add(Shape::mesh(mesh));
        };
        if let HatchPattern::Gradient { kind, from, to } = *pattern {
            // Shaded smoothly between the two colors, as the screen shows them.
            let rgb = |c: Color32| [c.r(), c.g(), c.b()];
            let (from, to) = (rgb(theme::aci(from)), rgb(theme::aci(to)));
            let mut mesh = egui::Mesh::default();
            for t in hatch::gradient_triangles(&polys, kind, *angle) {
                let i = mesh.vertices.len() as u32;
                for (p, f) in t {
                    let [r, g, b] = hatch::blend_rgb(from, to, f);
                    mesh.colored_vertex(self.view.to_screen(rect, p), Color32::from_rgb(r, g, b));
                }
                mesh.add_triangle(i, i + 1, i + 2);
            }
            painter.add(Shape::mesh(mesh));
        } else if *pattern == HatchPattern::Solid {
            fill(stroke.color);
        } else if spacing_px < 2.0 {
            fill(stroke.color.gamma_multiply(0.3));
        } else if let Some(lines) = hatch::pattern_lines(&polys, *pattern, *scale, *angle, *origin, 20_000) {
            for (a, b) in lines {
                painter.line_segment(
                    [self.view.to_screen(rect, a), self.view.to_screen(rect, b)],
                    Stroke::new(1.0, stroke.color),
                );
            }
        }
    }

    fn draw_grips(&self, painter: &Painter, rect: Rect, cursor: Option<&CursorState>) {
        for g in self.engine.grips(&self.doc) {
            let at = self.view.to_screen(rect, g);
            let hot = cursor.is_some_and(|c| c.screen.distance(at) <= APERTURE);
            let r = Rect::from_center_size(at, vec2(GRIP_SIZE, GRIP_SIZE) * 2.0);
            painter.rect(
                r,
                0.0,
                if hot { theme::grip_hover() } else { theme::grip() },
                Stroke::new(1.0, Color32::from_gray(20)),
                StrokeKind::Inside,
            );
        }
    }

    /// Selection window: blue solid for a window (left to right),
    /// green dashed for a crossing window (right to left).
    fn draw_window(&self, painter: &Painter, rect: Rect, c: &CursorState) {
        if let Some(Drag::Lasso { points, mode }) = &self.drag {
            self.draw_lasso(painter, rect, points, c, mode.unwrap_or(LassoMode::Window));
            return;
        }
        let Some(first) = self.engine.window_start() else { return };
        let a = self.view.to_screen(rect, first);
        let r = Rect::from_two_pos(a, c.screen);
        if c.raw.x < first.x {
            painter.rect_filled(r, 0.0, theme::crossing_fill());
            let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, theme::crossing_edge()), 6.0, 4.0));
        } else {
            painter.rect(r, 0.0, theme::window_fill(), Stroke::new(1.0, theme::window_edge()), StrokeKind::Middle);
        }
    }

    /// A lasso being dragged: blue and solid as a window, green and dashed
    /// when crossing, as the selection windows are; a fence is its dashed
    /// path alone. Beside the cursor, what it is and that Space changes it.
    fn draw_lasso(&self, painter: &Painter, rect: Rect, points: &[Point], c: &CursorState, mode: LassoMode) {
        let ring: Vec<Point> = points.iter().copied().chain([c.raw]).collect();
        let hint = format!("{} - Press Spacebar to cycle options", mode.name());
        painter.text(c.screen + vec2(16.0, 18.0), Align2::LEFT_TOP, hint, FontId::proportional(11.5), theme::label());
        if mode == LassoMode::Fence {
            let path: Vec<Pos2> = ring.iter().map(|p| self.view.to_screen(rect, *p)).collect();
            painter.extend(Shape::dashed_line(&path, Stroke::new(1.5, theme::crossing_edge()), 6.0, 4.0));
            return;
        }
        let crossing = mode == LassoMode::Crossing;
        let fill = if crossing { theme::crossing_fill() } else { theme::window_fill() };
        let mut mesh = egui::Mesh::default();
        for t in oxidraft_model::hatch::triangles(std::slice::from_ref(&ring)) {
            let i = mesh.vertices.len() as u32;
            for p in t {
                mesh.colored_vertex(self.view.to_screen(rect, p), fill);
            }
            mesh.add_triangle(i, i + 1, i + 2);
        }
        painter.add(Shape::mesh(mesh));
        let mut outline: Vec<Pos2> = ring.iter().map(|p| self.view.to_screen(rect, *p)).collect();
        outline.push(outline[0]);
        if crossing {
            painter.extend(Shape::dashed_line(&outline, Stroke::new(1.0, theme::crossing_edge()), 6.0, 4.0));
        } else {
            painter.add(Shape::line(outline, Stroke::new(1.0, theme::window_edge())));
        }
    }

    /// The Selection list of objects on top of each other where it was
    /// clicked: pointing at one highlights it, clicking it selects it.
    pub(crate) fn selection_list(&mut self, ctx: &egui::Context) {
        let Some(cycling) = &mut self.cycling else { return };
        let mut chosen = None;
        let doc = &self.doc;
        cycling.hovered = None;
        egui::Area::new(egui::Id::unique("selection_cycling"))
            .order(egui::Order::Foreground)
            .fixed_pos(cycling.screen + vec2(14.0, 14.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("Selection").strong());
                    ui.separator();
                    for &id in &cycling.ids {
                        let Some(e) = doc.entity(id) else { continue };
                        let color = theme::swatch(doc.display_color(e));
                        let row = ui.horizontal(|ui| {
                            let (swatch, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                            ui.painter().rect_filled(swatch, 1.0, color);
                            let item = ui.selectable_label(false, e.geometry.name());
                            ui.colored_label(theme::text_dim(), &e.layer);
                            item
                        });
                        let item = row.inner;
                        if item.hovered() || row.response.hovered() {
                            cycling.hovered = Some(id);
                        }
                        if item.clicked() {
                            chosen = Some(id);
                        }
                    }
                });
            });
        if let Some(id) = chosen {
            let (at, shift) = (cycling.at, cycling.shift);
            self.cycling = None;
            let tolerance = self.pick_tolerance();
            self.engine.select_object(id, at, shift, &mut self.doc, tolerance);
        }
    }

    /// Polar tracking path: a dotted ray from the base point through the cursor.
    fn draw_tracking(&self, painter: &Painter, rect: Rect, c: &CursorState) {
        let Some((base, angle)) = c.tracked.and_then(|t| t.polar) else { return };
        let far = base.polar(self.view.units(rect.width() + rect.height()), angle);
        let pts = [self.view.to_screen(rect, base), self.view.to_screen(rect, far)];
        painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, theme::tracking()), 3.0, 4.0));
        let at = self.view.to_screen(rect, c.point);
        painter.line_segment([at + vec2(-5.0, -5.0), at + vec2(5.0, 5.0)], Stroke::new(1.0, theme::tracking()));
        painter.line_segment([at + vec2(-5.0, 5.0), at + vec2(5.0, -5.0)], Stroke::new(1.0, theme::tracking()));
    }

    /// Crosshair with pickbox; at "Select objects:" only the pickbox shows, as in AutoCAD.
    fn draw_cursor(&self, painter: &Painter, rect: Rect, c: &CursorState) {
        let stroke = Stroke::new(1.0, theme::crosshair());
        let at = pos2(c.screen.x.round() + 0.5, c.screen.y.round() + 0.5);
        let selecting_in_command = self.engine.is_active() && self.picks_objects();
        if !selecting_in_command {
            let arm = rect.width().max(rect.height()) * CURSOR_SIZE / 100.0;
            let gap = if self.picks_objects() { self.aperture / 2.0 + 1.0 } else { 0.0 };
            for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
                painter.line_segment([at + d * gap, at + d * arm], stroke);
            }
        }
        if self.picks_objects() {
            let half = self.aperture / 2.0;
            painter.rect_stroke(Rect::from_center_size(at, vec2(half, half) * 2.0), 0.0, stroke, StrokeKind::Middle);
        }
    }

    /// Where a hyperlink goes: web addresses as they are, files relative
    /// to the drawing's folder.
    pub(crate) fn link_target(&self, url: &str) -> String {
        if url.contains("://") || url.starts_with("mailto:") {
            return url.to_string();
        }
        let path = std::path::Path::new(url);
        let full = match (path.is_relative(), self.path.as_ref().and_then(|p| p.parent())) {
            (true, Some(dir)) => dir.join(path),
            _ => path.to_path_buf(),
        };
        format!("file://{}", full.display())
    }

    /// Snap name, polar readout and dynamic input next to the cursor.
    fn draw_tooltips(&self, painter: &Painter, rect: Rect, c: &CursorState) {
        // An object's hyperlink, under the cursor while idle.
        if !self.engine.is_active()
            && c.snap.is_none()
            && !self.hyperlink_tips_off
            && let Some(link) = snap::pick(&self.doc, c.raw, self.pick_tolerance())
                .and_then(|id| self.doc.entity(id))
                .and_then(|e| e.hyperlink.as_ref())
        {
            let name = if link.description.is_empty() { &link.url } else { &link.description };
            let follow = if cfg!(target_os = "macos") { "Cmd-click to follow" } else { "Ctrl-click to follow" };
            tooltip(painter, c.screen + vec2(18.0, 18.0), &[format!("Link: {name}"), follow.to_string()], None);
        }
        if let Some(s) = c.snap {
            let at = self.view.to_screen(rect, s.point) + vec2(14.0, 14.0);
            tooltip(painter, at, &[s.kind.label().to_string()], None);
        } else if let Some((base, angle)) = c.tracked.and_then(|t| t.polar) {
            let text = format!(
                "Polar: {} < {}",
                oxidraft_model::units::length(base.distance(c.point)),
                crate::drawing_units::shown_direction(angle)
            );
            tooltip(painter, c.screen + vec2(18.0, 18.0), &[text], None);
        }
        if !self.engine.dynamic_input {
            return;
        }
        let at = c.screen
            + vec2(22.0, if c.snap.is_some() || c.tracked.is_some_and(|t| t.polar.is_some()) { -46.0 } else { 22.0 });
        let typed = (!self.command_text.is_empty()).then(|| self.command_text.clone());
        match self.engine.current_prompt() {
            Some(p) => {
                let mut fields = Vec::new();
                match (self.engine.base_point(), &typed) {
                    (_, Some(t)) => fields.push(t.clone()),
                    (Some(b), None) => {
                        fields.push(oxidraft_model::units::length(b.distance(c.point)));
                        fields.push(crate::drawing_units::shown_direction(b.angle_to(c.point)));
                    }
                    (None, None) if !self.picks_objects() => {
                        fields.push(oxidraft_model::units::length(c.point.x));
                        fields.push(oxidraft_model::units::length(c.point.y));
                    }
                    _ => {}
                }
                let mut text = p.text.clone();
                if !p.keywords.is_empty() {
                    text.push_str(" or");
                }
                tooltip(painter, at, &[text], Some(&fields));
            }
            None => {
                if let Some(t) = typed {
                    tooltip(painter, at, &[], Some(&[t]));
                }
            }
        }
    }
}

/// Cheap bounding-box test to skip drawing objects outside the view.
/// A square icon button for the navigation bar, highlighted while `on`.
fn nav_button(ui: &mut Ui, icon: Icon, on: bool, tip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    if on {
        ui.painter().rect_filled(rect, 2.0, theme::accent().gamma_multiply(0.35));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 2.0, theme::ribbon_hover());
    }
    icons::paint(ui.painter(), rect.shrink(4.0), icon, theme::text(), true);
    response.on_hover_text(tip)
}

/// The part of the canvas a zoomed region fills, clear of the viewport
/// label and the floating command line.
fn fit_area(rect: Rect) -> Rect {
    Rect::from_min_max(rect.min + vec2(24.0, 36.0), rect.max - vec2(24.0, 120.0))
}

fn on_screen(b: &Aabb, view: &Aabb) -> bool {
    b.max.x >= view.min.x && b.min.x <= view.max.x && b.max.y >= view.min.y && b.min.y <= view.max.y
}

fn arrow(painter: &Painter, from: Pos2, to: Pos2, color: Color32) {
    painter.line_segment([from, to], Stroke::new(1.5, color));
    let dir = (to - from).normalized();
    let side = vec2(-dir.y, dir.x);
    painter.add(Shape::convex_polygon(
        vec![to, to - dir * 8.0 + side * 3.5, to - dir * 8.0 - side * 3.5],
        color,
        Stroke::NONE,
    ));
}

/// Screen width of a lineweight: its size at 96 dpi, at least one pixel.
fn lineweight_px(lw: Lineweight) -> f32 {
    lw.mm().map_or(1.0, |mm| (mm * 96.0 / 25.4) as f32).max(1.0)
}

/// AutoSnap markers in AutoCAD's shapes.
fn draw_snap_marker(painter: &Painter, at: Pos2, kind: SnapKind) {
    let stroke = Stroke::new(2.0, theme::autosnap());
    let s = 6.0;
    match kind {
        SnapKind::Endpoint => {
            painter.rect_stroke(Rect::from_center_size(at, vec2(s, s) * 2.0), 0.0, stroke, StrokeKind::Middle);
        }
        SnapKind::Midpoint => {
            let pts = vec![at + vec2(0.0, -s), at + vec2(s, s * 0.8), at + vec2(-s, s * 0.8)];
            painter.add(Shape::closed_line(pts, stroke));
        }
        SnapKind::Center => {
            painter.circle_stroke(at, s, stroke);
        }
        // A circle with an X through it.
        SnapKind::Node => {
            painter.circle_stroke(at, s, stroke);
            let d = s * 0.7;
            painter.line_segment([at + vec2(-d, -d), at + vec2(d, d)], stroke);
            painter.line_segment([at + vec2(-d, d), at + vec2(d, -d)], stroke);
        }
        SnapKind::Quadrant => {
            let pts = vec![at + vec2(0.0, -s), at + vec2(s, 0.0), at + vec2(0.0, s), at + vec2(-s, 0.0)];
            painter.add(Shape::closed_line(pts, stroke));
        }
        SnapKind::Intersection => {
            painter.line_segment([at + vec2(-s, -s), at + vec2(s, s)], stroke);
            painter.line_segment([at + vec2(-s, s), at + vec2(s, -s)], stroke);
        }
        // Two overlapping squares, like AutoCAD's insertion marker.
        SnapKind::Insertion => {
            let r = Rect::from_center_size(at, vec2(s, s) * 1.4);
            painter.rect_stroke(r.translate(vec2(-2.0, 2.0)), 0.0, stroke, StrokeKind::Middle);
            painter.rect_stroke(r.translate(vec2(2.0, -2.0)), 0.0, stroke, StrokeKind::Middle);
        }
        // A right angle with its corner marked.
        SnapKind::Perpendicular => {
            let pts = vec![at + vec2(-s, -s), at + vec2(-s, s), at + vec2(s, s)];
            painter.add(Shape::line(pts, stroke));
            let corner = vec![at + vec2(-s, 0.0), at + vec2(0.0, 0.0), at + vec2(0.0, s)];
            painter.add(Shape::line(corner, stroke));
        }
        // A circle with a line touching its top.
        SnapKind::Tangent => {
            painter.circle_stroke(at, s * 0.8, stroke);
            painter.line_segment([at + vec2(-s, -s * 0.8), at + vec2(s, -s * 0.8)], stroke);
        }
        // An hourglass.
        SnapKind::Nearest => {
            let pts = vec![at + vec2(-s, -s), at + vec2(s, -s), at + vec2(-s, s), at + vec2(s, s)];
            painter.add(Shape::closed_line(pts, stroke));
        }
    }
}

/// A dark tooltip box with text lines and, optionally, input fields after them.
fn tooltip(painter: &Painter, at: Pos2, lines: &[String], fields: Option<&[String]>) {
    let font = FontId::proportional(12.0);
    let pad = vec2(6.0, 4.0);
    let mut x = at.x + pad.x;
    let mut shapes = Vec::new();
    let mut height: f32 = 18.0;
    for line in lines {
        let galley = painter.layout_no_wrap(line.clone(), font.clone(), theme::text());
        height = height.max(galley.size().y);
        shapes.push((pos2(x, at.y + pad.y), galley, false));
        x += shapes.last().map_or(0.0, |s| s.1.size().x) + 8.0;
    }
    for f in fields.unwrap_or(&[]) {
        let galley = painter.layout_no_wrap(f.clone(), font.clone(), theme::text());
        height = height.max(galley.size().y);
        shapes.push((pos2(x + 4.0, at.y + pad.y), galley, true));
        x += shapes.last().map_or(0.0, |s| s.1.size().x).max(56.0) + 12.0;
    }
    if shapes.is_empty() {
        return;
    }
    let bg = Rect::from_min_max(at, pos2(x - 8.0 + pad.x, at.y + height + pad.y * 2.0));
    painter.rect(bg, 2.0, theme::tooltip_bg(), Stroke::new(1.0, theme::tooltip_edge()), StrokeKind::Inside);
    let mut first_field = true;
    for (pos, galley, is_field) in shapes {
        if is_field {
            let w = galley.size().x.max(56.0);
            let r = Rect::from_min_size(pos - vec2(4.0, 2.0), vec2(w + 8.0, height + 4.0));
            let edge = if first_field { theme::accent() } else { theme::tooltip_edge() };
            painter.rect(r, 1.0, theme::input_bg(), Stroke::new(1.0, edge), StrokeKind::Inside);
            first_field = false;
        }
        painter.galley(pos, galley, theme::text());
    }
}

/// A point's mark in PDMODE style `mode`, `full` pixels across.
pub(crate) fn paint_point(painter: &Painter, at: Pos2, mode: i32, full: f32, stroke: Stroke) {
    let h = full / 2.0;
    match mode & 31 {
        0 => {
            painter.circle_filled(at, (stroke.width / 2.0).max(1.0), stroke.color);
        }
        2 => {
            painter.line_segment([at - vec2(h, 0.0), at + vec2(h, 0.0)], stroke);
            painter.line_segment([at - vec2(0.0, h), at + vec2(0.0, h)], stroke);
        }
        3 => {
            painter.line_segment([at - vec2(h, h), at + vec2(h, h)], stroke);
            painter.line_segment([at + vec2(-h, h), at + vec2(h, -h)], stroke);
        }
        4 => {
            painter.line_segment([at, at - vec2(0.0, h)], stroke);
        }
        _ => {}
    }
    if mode & 32 != 0 {
        painter.circle_stroke(at, h, stroke);
    }
    if mode & 64 != 0 {
        painter.rect_stroke(Rect::from_center_size(at, vec2(full, full)), 0.0, stroke, StrokeKind::Middle);
    }
}

/// Paints text stretched by `width_factor` and slanted by `oblique` degrees
/// along its own line, about `anchor` (its text style's look); plain text
/// as it is. `baseline` is where the first line sits below `anchor`, which
/// slanting leaves in place.
pub(crate) fn paint_text(
    painter: &Painter,
    shape: egui::epaint::TextShape,
    anchor: Pos2,
    width_factor: f32,
    oblique: f32,
    baseline: f32,
) {
    if (width_factor - 1.0).abs() < 1e-6 && oblique.abs() < 1e-6 {
        painter.add(shape);
        return;
    }
    let ctx = painter.ctx();
    let font_size = ctx.fonts(|f| f.font_image_size());
    let options = egui::epaint::TessellationOptions::default();
    let mut tessellator = egui::epaint::Tessellator::new(ctx.pixels_per_point(), options, font_size, Vec::new());
    let mut mesh = egui::Mesh::default();
    tessellator.tessellate_text(&shape, &mut mesh);
    let rot = egui::emath::Rot2::from_angle(shape.angle);
    let shear = oblique.to_radians().tan();
    for v in &mut mesh.vertices {
        // Along the line and down from it; screen y points down, so up is negative.
        let local = rot.inverse() * (v.pos - anchor);
        let local = vec2(local.x * width_factor - (local.y - baseline) * shear, local.y);
        v.pos = anchor + rot * local;
    }
    painter.add(Shape::mesh(mesh));
}

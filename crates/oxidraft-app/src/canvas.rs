//! The drawing area (model space): rendering and mouse handling.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, Event, FontId, MouseWheelSource, MouseWheelUnit, Painter, PointerButton, Pos2,
    Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use oxidraft_commands::Tracked;
use oxidraft_geom::{Aabb, Point, curve::Seg};
use oxidraft_model::{
    Attach, Geometry, HatchPattern,
    annotation::text_box,
    hatch,
    snap::{self, Snap, SnapKind},
};

use crate::{app::OxidraftApp, theme, view::View};

/// AutoSnap aperture and pickbox radius, in screen points.
pub const APERTURE: f32 = 10.0;
/// CURSORSIZE: crosshair arm length as a percentage of the drawing area.
const CURSOR_SIZE: f32 = 5.0;
const GRIP_SIZE: f32 = 5.0;

/// A primary-button drag in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drag {
    /// Press-drag on empty space: a selection window.
    Window,
    /// Press-drag on a grip: stretch it and drop it where the button is released.
    Grip,
}

/// What the cursor resolves to this frame.
struct CursorState {
    screen: Pos2,
    /// Bare cursor position in drawing units.
    raw: Point,
    /// Point a click would enter: snapped, tracked or raw.
    point: Point,
    snap: Option<Snap>,
    tracked: Option<Tracked>,
}

impl OxidraftApp {
    pub(crate) fn canvas(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        self.canvas_rect = rect;
        let painter = ui.painter_at(rect);

        if self.zoom_extents_pending {
            self.zoom_extents_pending = false;
            match self.doc.extents() {
                Some(b) => {
                    // Keep the drawing clear of the viewport label and the command line.
                    let fit = Rect::from_min_max(rect.min + vec2(24.0, 36.0), rect.max - vec2(24.0, 120.0));
                    self.view.zoom_extents(rect, fit, b);
                }
                None => self.view = View::default(),
            }
        }
        self.navigate(ui, &response, rect);

        // While dragging a grip the pointer may leave the hover state; keep tracking it.
        let pointer = response.hover_pos().or_else(|| response.interact_pointer_pos());
        let cursor = pointer.map(|screen| self.resolve_cursor(rect, screen));
        self.cursor = cursor.as_ref().map(|c| c.point);
        self.handle_clicks(ui, &response, cursor.as_ref());

        if self.grid {
            self.draw_grid(&painter, rect);
        }
        self.draw_ucs_icon(&painter, rect);
        self.draw_entities(&painter, rect, cursor.as_ref());
        if let Some(c) = &cursor {
            for g in self.engine.preview(c.point) {
                self.draw_geometry(&painter, rect, &g, Stroke::new(1.0, theme::PREVIEW));
            }
        }
        self.draw_grips(&painter, rect, cursor.as_ref());
        if let Some(c) = &cursor {
            self.draw_window(&painter, rect, c);
            self.draw_tracking(&painter, rect, c);
            if let Some(s) = c.snap {
                draw_snap_marker(&painter, self.view.to_screen(rect, s.point), s.kind);
            }
            self.draw_cursor(&painter, rect, c);
            self.draw_tooltips(&painter, rect, c);
            ui.ctx().set_cursor_icon(if response.dragged_by(PointerButton::Middle) {
                CursorIcon::Grabbing
            } else {
                CursorIcon::None
            });
        }
        painter.text(
            rect.left_top() + vec2(8.0, 6.0),
            Align2::LEFT_TOP,
            "[-][Top][2D Wireframe]",
            FontId::proportional(12.0),
            theme::TEXT_DIM,
        );
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
        if response.double_clicked_by(PointerButton::Middle) {
            self.zoom_extents_pending = true;
        }
        if response.dragged_by(PointerButton::Middle) {
            let d = response.drag_delta();
            self.view.pan(d.x, d.y);
        }
        let Some(hover) = response.hover_pos() else { return };
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
    }

    fn resolve_cursor(&self, rect: Rect, screen: Pos2) -> CursorState {
        let raw = self.view.to_world(rect, screen);
        let tolerance = self.pick_tolerance();
        if self.picks_objects() {
            return CursorState { screen, raw, point: raw, snap: None, tracked: None };
        }
        let snap = if self.osnap { snap::find_snap(&self.doc, raw, tolerance) } else { None };
        if let Some(s) = snap {
            return CursorState { screen, raw, point: s.point, snap: Some(s), tracked: None };
        }
        let tracked = self.engine.track(raw, tolerance);
        CursorState { screen, raw, point: tracked.point, snap: None, tracked: Some(tracked) }
    }

    fn handle_clicks(&mut self, ui: &Ui, response: &Response, cursor: Option<&CursorState>) {
        let Some(c) = cursor else { return };
        let tolerance = self.pick_tolerance();
        let shift = ui.input(|i| i.modifiers.shift);

        // Press-drag on a grip stretches it; on empty space it draws a selection window.
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
                self.engine.click(start, start, shift, &mut self.doc, tolerance);
                self.drag = Some(Drag::Window);
            }
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            match self.drag.take() {
                Some(Drag::Window) => self.engine.click(c.raw, c.raw, shift, &mut self.doc, tolerance),
                // Dropped with snaps and tracking applied, like a second click.
                Some(Drag::Grip) => self.engine.click(c.point, c.raw, false, &mut self.doc, tolerance),
                None => {}
            }
        }
        if response.clicked_by(PointerButton::Primary) {
            self.engine.click(c.point, c.raw, shift, &mut self.doc, tolerance);
        }
        // Double-clicking an object opens its properties, as in AutoCAD (its
        // two clicks have already selected it).
        if response.double_clicked_by(PointerButton::Primary)
            && !self.engine.is_active()
            && snap::pick(&self.doc, c.raw, tolerance).is_some()
        {
            self.properties_open = true;
        }
        // Right-click acts as Enter.
        if response.clicked_by(PointerButton::Secondary) {
            self.command_text.clear();
            self.submit();
        }
    }

    // ---- drawing ----------------------------------------------------------------

    /// Adaptive grid: minor lines with a major line every 5 (GRIDMAJOR).
    fn draw_grid(&self, painter: &Painter, rect: Rect) {
        let (minor, major) = self.view.grid_step(10.0);
        let min = self.view.to_world(rect, rect.left_bottom());
        let max = self.view.to_world(rect, rect.right_top());
        let ratio = (major / minor).round() as i64;
        let is_major = |v: f64| ((v / minor).round() as i64).rem_euclid(ratio) == 0;

        let mut x = (min.x / minor).floor() * minor;
        while x <= max.x {
            let sx = self.view.to_screen(rect, Point::new(x, 0.0)).x.round() + 0.5;
            let color = if x.abs() < minor * 1e-6 {
                theme::AXIS_Y
            } else if is_major(x) {
                theme::GRID_MAJOR
            } else {
                theme::GRID_MINOR
            };
            painter.line_segment([pos2(sx, rect.top()), pos2(sx, rect.bottom())], Stroke::new(1.0, color));
            x += minor;
        }
        let mut y = (min.y / minor).floor() * minor;
        while y <= max.y {
            let sy = self.view.to_screen(rect, Point::new(0.0, y)).y.round() + 0.5;
            let color = if y.abs() < minor * 1e-6 {
                theme::AXIS_X
            } else if is_major(y) {
                theme::GRID_MAJOR
            } else {
                theme::GRID_MINOR
            };
            painter.line_segment([pos2(rect.left(), sy), pos2(rect.right(), sy)], Stroke::new(1.0, color));
            y += minor;
        }
    }

    /// 2D UCS icon: at the origin when it's on screen, else in the lower-left corner.
    fn draw_ucs_icon(&self, painter: &Painter, rect: Rect) {
        let o = self.view.to_screen(rect, Point::ORIGIN);
        let inner = rect.shrink2(vec2(60.0, 60.0));
        let at = if inner.contains(o) { o } else { pos2(rect.left() + 28.0, rect.bottom() - 70.0) };
        let len = 42.0;
        let x_col = Color32::from_rgb(230, 80, 80);
        let y_col = Color32::from_rgb(80, 210, 90);
        arrow(painter, at, at + vec2(len, 0.0), x_col);
        arrow(painter, at, at - vec2(0.0, len), y_col);
        painter.rect_stroke(
            Rect::from_center_size(at, vec2(7.0, 7.0)),
            0.0,
            Stroke::new(1.0, theme::TEXT_DIM),
            StrokeKind::Middle,
        );
        painter.text(at + vec2(len + 6.0, 0.0), Align2::LEFT_CENTER, "X", FontId::proportional(11.0), x_col);
        painter.text(at - vec2(0.0, len + 6.0), Align2::CENTER_BOTTOM, "Y", FontId::proportional(11.0), y_col);
    }

    fn draw_entities(&self, painter: &Painter, rect: Rect, cursor: Option<&CursorState>) {
        let selected = self.engine.highlighted();
        // Rollover highlighting: the object under the pickbox gets thicker, and
        // so does everything a selection window in progress would select
        // (AutoCAD's selection preview).
        let hovered: Vec<_> = match (cursor, self.engine.window_start()) {
            (Some(c), Some(first)) => snap::window_select(&self.doc, first, c.raw),
            (Some(c), None) if self.picks_objects() => {
                snap::pick(&self.doc, c.raw, self.pick_tolerance()).into_iter().collect()
            }
            _ => Vec::new(),
        };
        let view =
            Aabb::from_points(self.view.to_world(rect, rect.left_bottom()), self.view.to_world(rect, rect.right_top()));
        for e in self.doc.entities() {
            if !self.doc.is_shown(&e.layer) || !on_screen(&self.doc.entity_bounds(e), &view) {
                continue;
            }
            for (g, aci, layer) in self.doc.expanded_styled(e) {
                let mut color = theme::aci(aci);
                // Locked layers draw faded (AutoCAD's LAYLOCKFADECTL).
                if self.doc.layer(&layer).is_some_and(|l| l.locked) {
                    color = color.gamma_multiply(0.45);
                }
                if selected.contains(&e.id) {
                    self.draw_geometry(painter, rect, &g, Stroke::new(5.0, theme::SELECTION_GLOW));
                    self.draw_geometry(painter, rect, &g, Stroke::new(1.0, color));
                } else if hovered.contains(&e.id) {
                    self.draw_geometry(painter, rect, &g, Stroke::new(2.5, color));
                } else {
                    self.draw_geometry(painter, rect, &g, Stroke::new(1.0, color));
                }
            }
        }
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
            Geometry::Text { .. } => return self.draw_text(painter, rect, g, stroke),
            Geometry::Hatch { .. } => return self.draw_hatch(painter, rect, g, stroke),
            _ => {}
        }
        // Lines, arcs and polylines become one screen polyline, with arcs
        // split finely enough to look smooth at the current zoom.
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
        if points.len() >= 2 {
            painter.add(Shape::line(points, stroke));
        }
    }

    /// Text at its attachment point. AutoCAD's text height is the capital
    /// height, about 0.7 of the font size.
    fn draw_text(&self, painter: &Painter, rect: Rect, g: &Geometry, stroke: Stroke) {
        let Geometry::Text { position, height, rotation, width, attach, content } = g else { return };
        let anchor = self.view.to_screen(rect, *position);
        // A selected text glows as a box instead.
        if stroke.width >= 4.0 {
            let corners = text_box(*position, *height, *rotation, *width, *attach, content);
            let pts: Vec<Pos2> = corners.iter().map(|p| self.view.to_screen(rect, *p)).collect();
            painter.add(Shape::convex_polygon(pts, stroke.color.gamma_multiply(0.5), Stroke::NONE));
            return;
        }
        let px = (*height * self.view.scale / 0.7) as f32;
        if px < 1.5 {
            return;
        }
        // Few distinct sizes keep egui's glyph cache small while zooming.
        let size = 2f32.powf((px.min(400.0).log2() * 8.0).round() / 8.0);
        let text = content.replace("\\P", "\n");
        let wrap = if *width > 0.0 { (*width * self.view.scale) as f32 } else { f32::INFINITY };
        let galley = painter.layout(text, FontId::proportional(size), stroke.color, wrap);
        // Anchor on the type itself, not egui's line box: "top" is the first
        // line's capital height, "bottom" the last line's baseline.
        let baseline = |row: &egui::epaint::text::PlacedRow| {
            row.pos.y + row.row.glyphs.first().map_or(row.row.size.y * 0.8, |g| g.pos.y)
        };
        let first = galley.rows.first().map_or(0.0, baseline);
        let last = galley.rows.last().map_or(0.0, baseline);
        let (top, width) = (first - size * 0.7, galley.size().x);
        let offset = match attach {
            Attach::TopLeft => vec2(0.0, top),
            Attach::BottomLeft => vec2(0.0, last),
            Attach::MiddleLeft => vec2(0.0, (top + last) / 2.0),
            Attach::MiddleRight => vec2(width, (top + last) / 2.0),
            Attach::BottomCenter => vec2(width / 2.0, last),
        };
        // World angles run counterclockwise; egui's clockwise on a y-down screen.
        let angle = -(*rotation as f32).to_radians();
        let pos = anchor - egui::emath::Rot2::from_angle(angle) * offset;
        painter.add(egui::epaint::TextShape::new(pos, galley, stroke.color).with_angle(angle));
    }

    /// Solid fills as triangles; patterns as lines clipped to the boundary
    /// (too fine to see at this zoom: a faint fill instead).
    fn draw_hatch(&self, painter: &Painter, rect: Rect, g: &Geometry, stroke: Stroke) {
        let Geometry::Hatch { loops, pattern, scale, angle } = g else { return };
        let polys = hatch::polygons(loops);
        if stroke.width >= 4.0 {
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
        if *pattern == HatchPattern::Solid {
            fill(stroke.color);
        } else if spacing_px < 2.0 {
            fill(stroke.color.gamma_multiply(0.3));
        } else if let Some(lines) = hatch::pattern_lines(&polys, *pattern, *scale, *angle, 20_000) {
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
                if hot { theme::GRIP_HOVER } else { theme::GRIP },
                Stroke::new(1.0, Color32::from_gray(20)),
                StrokeKind::Inside,
            );
        }
    }

    /// Selection window: blue solid for a window (left to right),
    /// green dashed for a crossing window (right to left).
    fn draw_window(&self, painter: &Painter, rect: Rect, c: &CursorState) {
        let Some(first) = self.engine.window_start() else { return };
        let a = self.view.to_screen(rect, first);
        let r = Rect::from_two_pos(a, c.screen);
        if c.raw.x < first.x {
            painter.rect_filled(r, 0.0, theme::CROSSING_FILL);
            let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, theme::CROSSING_EDGE), 6.0, 4.0));
        } else {
            painter.rect(r, 0.0, theme::WINDOW_FILL, Stroke::new(1.0, theme::WINDOW_EDGE), StrokeKind::Middle);
        }
    }

    /// Polar tracking path: a dotted ray from the base point through the cursor.
    fn draw_tracking(&self, painter: &Painter, rect: Rect, c: &CursorState) {
        let Some((base, angle)) = c.tracked.and_then(|t| t.polar) else { return };
        let far = base.polar(self.view.units(rect.width() + rect.height()), angle);
        let pts = [self.view.to_screen(rect, base), self.view.to_screen(rect, far)];
        painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, theme::TRACKING), 3.0, 4.0));
        let at = self.view.to_screen(rect, c.point);
        painter.line_segment([at + vec2(-5.0, -5.0), at + vec2(5.0, 5.0)], Stroke::new(1.0, theme::TRACKING));
        painter.line_segment([at + vec2(-5.0, 5.0), at + vec2(5.0, -5.0)], Stroke::new(1.0, theme::TRACKING));
    }

    /// Crosshair with pickbox; at "Select objects:" only the pickbox shows, as in AutoCAD.
    fn draw_cursor(&self, painter: &Painter, rect: Rect, c: &CursorState) {
        let stroke = Stroke::new(1.0, theme::CROSSHAIR);
        let at = pos2(c.screen.x.round() + 0.5, c.screen.y.round() + 0.5);
        let selecting_in_command = self.engine.is_active() && self.picks_objects();
        if !selecting_in_command {
            let arm = rect.width().max(rect.height()) * CURSOR_SIZE / 100.0;
            let gap = if self.picks_objects() { APERTURE / 2.0 + 1.0 } else { 0.0 };
            for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
                painter.line_segment([at + d * gap, at + d * arm], stroke);
            }
        }
        if self.picks_objects() {
            let half = APERTURE / 2.0;
            painter.rect_stroke(Rect::from_center_size(at, vec2(half, half) * 2.0), 0.0, stroke, StrokeKind::Middle);
        }
    }

    /// Snap name, polar readout and dynamic input next to the cursor.
    fn draw_tooltips(&self, painter: &Painter, rect: Rect, c: &CursorState) {
        if let Some(s) = c.snap {
            let at = self.view.to_screen(rect, s.point) + vec2(14.0, 14.0);
            tooltip(painter, at, &[s.kind.label().to_string()], None);
        } else if let Some((base, angle)) = c.tracked.and_then(|t| t.polar) {
            let text = format!("Polar: {:.4} < {:.0}°", base.distance(c.point), angle);
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
                        fields.push(format!("{:.4}", b.distance(c.point)));
                        fields.push(format!("{:.0}°", b.angle_to(c.point)));
                    }
                    (None, None) if !self.picks_objects() => {
                        fields.push(format!("{:.4}", c.point.x));
                        fields.push(format!("{:.4}", c.point.y));
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

/// AutoSnap markers in AutoCAD's shapes.
fn draw_snap_marker(painter: &Painter, at: Pos2, kind: SnapKind) {
    let stroke = Stroke::new(2.0, theme::AUTOSNAP);
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
        let galley = painter.layout_no_wrap(line.clone(), font.clone(), theme::TEXT);
        height = height.max(galley.size().y);
        shapes.push((pos2(x, at.y + pad.y), galley, false));
        x += shapes.last().map_or(0.0, |s| s.1.size().x) + 8.0;
    }
    for f in fields.unwrap_or(&[]) {
        let galley = painter.layout_no_wrap(f.clone(), font.clone(), theme::TEXT);
        height = height.max(galley.size().y);
        shapes.push((pos2(x + 4.0, at.y + pad.y), galley, true));
        x += shapes.last().map_or(0.0, |s| s.1.size().x).max(56.0) + 12.0;
    }
    if shapes.is_empty() {
        return;
    }
    let bg = Rect::from_min_max(at, pos2(x - 8.0 + pad.x, at.y + height + pad.y * 2.0));
    painter.rect(bg, 2.0, theme::TOOLTIP_BG, Stroke::new(1.0, theme::TOOLTIP_EDGE), StrokeKind::Inside);
    let mut first_field = true;
    for (pos, galley, is_field) in shapes {
        if is_field {
            let w = galley.size().x.max(56.0);
            let r = Rect::from_min_size(pos - vec2(4.0, 2.0), vec2(w + 8.0, height + 4.0));
            let edge = if first_field { theme::ACCENT } else { theme::TOOLTIP_EDGE };
            painter.rect(r, 1.0, theme::INPUT_BG, Stroke::new(1.0, edge), StrokeKind::Inside);
            first_field = false;
        }
        painter.galley(pos, galley, theme::TEXT);
    }
}

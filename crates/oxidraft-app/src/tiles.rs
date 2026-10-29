//! Model viewports (VPORTS): model space's drawing area split into panes,
//! each with its own view. One is current: commands, zooming and panning
//! happen there; clicking another (or Ctrl+R) makes it current.

use eframe::egui::{self, Align2, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use oxidraft_commands::Tiling;
use oxidraft_model::MODEL;

use crate::{app::OxidraftApp, theme, view::View};

/// The panes and their views; `views[active]` is kept in the app's own
/// view while it's current.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tiles {
    pub tiling: Tiling,
    pub views: Vec<View>,
    pub active: usize,
}

impl Default for Tiles {
    fn default() -> Self {
        Self { tiling: Tiling::Single, views: vec![View::default()], active: 0 }
    }
}

/// The panes `tiling` splits `r` into.
pub(crate) fn panes(tiling: Tiling, r: Rect) -> Vec<Rect> {
    let (x, y) = (r.center().x, r.center().y);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
    match tiling {
        Tiling::Single => vec![r],
        Tiling::TwoVertical => vec![rect(r.left(), r.top(), x, r.bottom()), rect(x, r.top(), r.right(), r.bottom())],
        Tiling::TwoHorizontal => vec![rect(r.left(), r.top(), r.right(), y), rect(r.left(), y, r.right(), r.bottom())],
        Tiling::ThreeRight => vec![
            rect(r.left(), r.top(), x, y),
            rect(r.left(), y, x, r.bottom()),
            rect(x, r.top(), r.right(), r.bottom()),
        ],
        Tiling::ThreeLeft => vec![
            rect(r.left(), r.top(), x, r.bottom()),
            rect(x, r.top(), r.right(), y),
            rect(x, y, r.right(), r.bottom()),
        ],
        Tiling::FourEqual => vec![
            rect(r.left(), r.top(), x, y),
            rect(x, r.top(), r.right(), y),
            rect(r.left(), y, x, r.bottom()),
            rect(x, y, r.right(), r.bottom()),
        ],
    }
}

impl OxidraftApp {
    /// VPORTS: splits model space's drawing area, each pane starting with
    /// the current view.
    pub(crate) fn set_tiling(&mut self, tiling: Tiling) {
        let count = panes(tiling, Rect::from_min_size(pos2(0.0, 0.0), vec2(1.0, 1.0))).len();
        self.tiles = Tiles { tiling, views: vec![self.view; count], active: 0 };
    }

    /// Ctrl+R: the next pane is current.
    pub(crate) fn next_pane(&mut self) {
        let count = self.tiles.views.len();
        if count > 1 {
            self.make_pane_current((self.tiles.active + 1) % count);
        }
    }

    fn make_pane_current(&mut self, i: usize) {
        let active = self.tiles.active;
        self.tiles.views[active] = self.view;
        self.tiles.active = i;
        self.view = self.tiles.views[i];
        self.engine.cancel_window();
    }

    /// The drawing area: model space's panes around the current one, which
    /// [`OxidraftApp::canvas_pane`] draws and works in.
    pub(crate) fn canvas(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        self.drawing_rect = full;
        // Previews and highlights draw dimensions in the drawing's style.
        oxidraft_model::annotation::set_dim_style(self.doc.dim_style());
        oxidraft_model::annotation::set_leader_style(self.doc.leader_style());
        oxidraft_model::units::set_current(self.doc.units());
        oxidraft_model::ucs::set_current(self.doc.ucs());
        oxidraft_model::field::set_env(self.field_env());
        self.engine.vars.viewport = self.active_viewport.as_ref().map(|v| v.id);
        self.engine.vars.grid_spacing = self.grid_spacing.unwrap_or(0.0);
        self.engine.vars.view_rotation = self.view.rotation;
        let tiled = self.tiles.tiling != Tiling::Single && self.doc.space() == MODEL && self.active_viewport.is_none();
        if !tiled {
            self.canvas_pane(ui, full);
            return;
        }
        let rects = panes(self.tiles.tiling, full);
        let active = self.tiles.active.min(rects.len() - 1);
        let mut clicked = None;
        for (i, pane) in rects.iter().enumerate().filter(|(i, _)| *i != active) {
            let response = ui.interact(*pane, egui::Id::unique(("pane", i)), Sense::click());
            if response.clicked() || response.secondary_clicked() {
                clicked = Some(i);
            }
            let current = std::mem::replace(&mut self.view, self.tiles.views[i]);
            self.draw_pane(ui, *pane);
            self.view = current;
        }
        self.canvas_pane(ui, rects[active]);
        // Each pane framed, the current one in the accent color.
        let painter = ui.painter_at(full);
        for (i, pane) in rects.iter().enumerate() {
            let (width, color) = if i == active { (2.0, theme::accent()) } else { (1.0, theme::ribbon_title()) };
            painter.rect_stroke(pane.shrink(1.0), 0.0, Stroke::new(width, color), StrokeKind::Inside);
        }
        if let Some(i) = clicked {
            self.make_pane_current(i);
        }
    }

    /// A pane that isn't current: the drawing in its own view.
    fn draw_pane(&mut self, ui: &Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        if self.grid {
            self.draw_grid(&painter, rect);
        }
        if self.engine.vars.ucs_icon {
            self.draw_ucs_icon(&painter, rect, rect);
        }
        self.draw_entities(&painter, rect, None, MODEL, &[]);
        painter.text(
            rect.left_top() + vec2(8.0, 6.0),
            Align2::LEFT_TOP,
            "[-][Top][2D Wireframe]",
            FontId::proportional(12.0),
            theme::label(),
        );
    }
}

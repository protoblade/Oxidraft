//! Several drawings in one window, each in a file tab as in AutoCAD: new
//! and opened drawings get tabs of their own, Ctrl+Tab and SWITCHWINDOWS
//! go from one to the next, CLOSE closes the current one and CLOSEALL
//! them all. SYSWINDOWS (CASCADE, TILE) lays the open drawings out over
//! the drawing area, to pick one from.

use std::path::PathBuf;

use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use oxidraft_model::MODEL;

use crate::app::OxidraftApp;
use crate::theme;

/// How SYSWINDOWS lays the drawings out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrange {
    Cascade,
    Horizontal,
    Vertical,
}

/// What a drawing asks of the window it's in.
#[derive(Debug, Clone, PartialEq)]
pub enum TabRequest {
    Switch(usize),
    New,
    Open(PathBuf),
    Close(usize),
    /// Close that drawing, asking to save its changes first.
    CloseAsking(usize),
    CloseAll,
    /// The next drawing, or the previous one.
    Next(bool),
    Arrange(Arrange),
    /// The arranged drawings put away (QVDRAWINGCLOSE).
    CloseArranged,
}

/// The drawings open in the window, and which one is current.
pub struct Drawings {
    list: Vec<OxidraftApp>,
    active: usize,
    /// The drawings laid out to pick from (SYSWINDOWS).
    arranged: Option<Arrange>,
}

impl OxidraftApp {
    /// Whether other drawings can be open beside this one.
    pub(crate) fn in_tabs(&self) -> bool {
        !self.tabs.is_empty()
    }

    /// OPEN in a window of several drawings: the file picked gets a tab.
    pub(crate) fn open_in_tab(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Drawing (DWG, DXF)", &["dwg", "dxf"])
            .add_filter("AutoCAD drawing", &["dwg"])
            .add_filter("DXF drawing", &["dxf"]);
        if let Some(path) = dialog.pick_file() {
            self.tab_request = Some(TabRequest::Open(path));
        }
    }

    /// An empty drawing with no file, nothing changed: what a new window
    /// starts with, which an opened drawing takes the place of.
    fn is_pristine(&self) -> bool {
        self.path.is_none() && !self.is_dirty() && self.doc.is_empty()
    }
}

impl Drawings {
    pub fn new(first: OxidraftApp) -> Self {
        Self { list: vec![first], active: 0, arranged: None }
    }

    pub fn current(&mut self) -> &mut OxidraftApp {
        &mut self.list[self.active]
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// The drawings' file names, in tab order.
    pub fn names(&self) -> Vec<String> {
        self.list.iter().map(OxidraftApp::file_name).collect()
    }

    /// A drawing for a new tab, with the current one's options.
    fn blank_like(&self, ctx: &egui::Context) -> OxidraftApp {
        let current = &self.list[self.active];
        let mut app = OxidraftApp::blank();
        app.untitled = self.list.iter().map(|d| d.untitled).max().unwrap_or(0) + 1;
        if let Some(path) = current.settings_path.clone() {
            app.keep_settings_in(path);
        }
        app.apply_settings(ctx, current.settings.clone());
        app
    }

    fn switch(&mut self, ctx: &egui::Context, to: usize) {
        if to >= self.list.len() || to == self.active {
            return;
        }
        // Options changed in one drawing hold in the others.
        let settings = self.list[self.active].settings.clone();
        self.active = to;
        if self.list[to].settings != settings {
            self.list[to].apply_settings(ctx, settings);
        }
    }

    /// Does what the current drawing asked.
    pub fn handle(&mut self, ctx: &egui::Context, request: TabRequest) {
        match request {
            TabRequest::Switch(i) => self.switch(ctx, i),
            TabRequest::Next(back) => {
                let n = self.list.len();
                let to = if back { (self.active + n - 1) % n } else { (self.active + 1) % n };
                self.switch(ctx, to);
            }
            TabRequest::New => {
                let app = self.blank_like(ctx);
                self.list.push(app);
                self.switch(ctx, self.list.len() - 1);
            }
            TabRequest::Open(path) => {
                // Already open: that tab.
                if let Some(i) = self.list.iter().position(|d| d.path.as_deref() == Some(path.as_path())) {
                    self.switch(ctx, i);
                    return;
                }
                let mut app = self.blank_like(ctx);
                app.open_path(path.clone());
                if app.path.as_deref() != Some(path.as_path()) {
                    // It couldn't be read: say so where you are.
                    let why = app.engine.history().last().cloned().unwrap_or_default();
                    self.list[self.active].engine.log(why);
                    return;
                }
                // An untouched empty drawing makes way for it.
                if self.list[self.active].is_pristine() {
                    self.list[self.active] = app;
                } else {
                    self.list.push(app);
                    self.switch(ctx, self.list.len() - 1);
                }
            }
            TabRequest::Close(i) => {
                if i >= self.list.len() {
                    return;
                }
                if self.list.len() == 1 {
                    // The last one: an empty drawing in its place.
                    self.list[0].new_drawing();
                    return;
                }
                self.list.remove(i);
                if self.active > i || self.active >= self.list.len() {
                    self.active = self.active.saturating_sub(1);
                }
            }
            TabRequest::CloseAsking(i) => {
                if i < self.list.len() && !self.list[i].is_dirty() {
                    return self.handle(ctx, TabRequest::Close(i));
                }
                self.switch(ctx, i);
                if let Some(app) = self.list.get_mut(i) {
                    app.after_saving(crate::files::Then::Close);
                }
            }
            TabRequest::CloseAll => {
                // Those without changes close; the first with some is asked about.
                let mut i = 0;
                while i < self.list.len() && self.list.len() > 1 {
                    if self.list[i].is_dirty() {
                        i += 1;
                    } else {
                        self.list.remove(i);
                    }
                }
                self.active = self.list.iter().position(OxidraftApp::is_dirty).unwrap_or(0);
                let app = &mut self.list[self.active];
                if app.is_dirty() {
                    app.after_saving(crate::files::Then::Close);
                } else {
                    app.new_drawing();
                }
            }
            TabRequest::Arrange(a) => self.arranged = (self.list.len() > 1).then_some(a),
            TabRequest::CloseArranged => self.arranged = None,
        }
    }

    /// The window's close button: a drawing with changes is asked about
    /// first, one at a time.
    fn closing(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.list[self.active].is_dirty() {
            return;
        }
        if let Some(i) = self.list.iter().position(OxidraftApp::is_dirty) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.switch(ctx, i);
            self.list[i].save_prompt = Some(crate::files::Then::Quit);
        }
    }

    /// The drawings laid out over the drawing area (SYSWINDOWS), each
    /// shown whole; a click makes one current, Esc goes back.
    fn arranged_ui(&mut self, ctx: &egui::Context, arrange: Arrange) {
        let area = self.list[self.active].drawing_rect;
        if !area.is_positive() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.arranged = None;
            return;
        }
        let n = self.list.len();
        let rects: Vec<Rect> = (0..n)
            .map(|i| match arrange {
                Arrange::Vertical => {
                    let w = area.width() / n as f32;
                    Rect::from_min_size(pos2(area.left() + w * i as f32, area.top()), vec2(w, area.height()))
                }
                Arrange::Horizontal => {
                    let h = area.height() / n as f32;
                    Rect::from_min_size(pos2(area.left(), area.top() + h * i as f32), vec2(area.width(), h))
                }
                Arrange::Cascade => {
                    let step = 28.0;
                    let size = (area.size() - vec2(step, step) * (n - 1) as f32).max(vec2(200.0, 150.0)) * 0.9;
                    Rect::from_min_size(area.min + vec2(step, step) * i as f32 + vec2(8.0, 8.0), size)
                }
            })
            .collect();
        let mut picked = None;
        egui::Area::new(egui::Id::unique("arranged_drawings")).order(egui::Order::Foreground).fixed_pos(area.min).show(
            ctx,
            |ui| {
                ui.painter().rect_filled(area, 0.0, theme::model_bg());
                for (i, rect) in rects.iter().enumerate() {
                    let r = ui.interact(*rect, egui::Id::unique(("arranged", i)), Sense::click());
                    let d = &self.list[i];
                    let frame = if i == self.active { theme::accent() } else { theme::text_dim() };
                    ui.painter().rect_filled(rect.shrink(2.0), 4.0, theme::model_bg());
                    let items: Vec<_> = d
                        .doc
                        .space_in_draw_order(MODEL)
                        .into_iter()
                        .flat_map(|e| d.doc.expanded_styled(e))
                        .take(20_000)
                        .collect();
                    let body = Rect::from_min_max(rect.min + vec2(0.0, 22.0), rect.max);
                    crate::blocks::paint_preview(ui, body.shrink(4.0), &items);
                    let title = Rect::from_min_size(rect.min, vec2(rect.width(), 22.0)).shrink(2.0);
                    ui.painter().rect_filled(title, 3.0, theme::tab());
                    let dirty = if d.is_dirty() { "*" } else { "" };
                    ui.painter().text(
                        title.left_center() + vec2(8.0, 0.0),
                        Align2::LEFT_CENTER,
                        format!("{}{dirty}", d.file_name()),
                        egui::FontId::proportional(12.0),
                        theme::text(),
                    );
                    let stroke = Stroke::new(if r.hovered() { 2.0 } else { 1.0 }, frame);
                    ui.painter().rect_stroke(rect.shrink(2.0), 4.0, stroke, egui::StrokeKind::Inside);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, d.file_name()));
                    if r.clicked() {
                        picked = Some(i);
                    }
                }
                ui.painter().text(
                    area.center_bottom() - vec2(0.0, 14.0),
                    Align2::CENTER_CENTER,
                    "Click a drawing to work in it; Esc goes back.",
                    egui::FontId::proportional(12.0),
                    Color32::from_gray(160),
                );
            },
        );
        if let Some(i) = picked {
            self.switch(ctx, i);
            self.arranged = None;
        }
    }

    /// One frame of the window: the current drawing, with the tabs of all.
    pub fn frame(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.closing(&ctx);
        let tabs: Vec<(String, bool)> = self.list.iter().map(|d| (d.file_name(), d.is_dirty())).collect();
        let app = &mut self.list[self.active];
        app.tabs = tabs;
        app.tab = self.active;
        eframe::App::ui(app, ui, frame);
        if let Some(request) = self.list[self.active].tab_request.take() {
            self.handle(&ctx, request);
        }
        if let Some(arrange) = self.arranged {
            self.arranged_ui(&ctx, arrange);
        }
    }
}

impl eframe::App for Drawings {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.frame(ui, frame);
    }
}

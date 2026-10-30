//! Several drawings in one window, each in a file tab as in AutoCAD: new
//! and opened drawings get tabs of their own, Ctrl+Tab and SWITCHWINDOWS
//! go from one to the next, CLOSE closes the current one and CLOSEALL
//! them all. SYSWINDOWS (CASCADE, TILE) lays the open drawings out side
//! by side, each in a window of its own to work in (the current one in
//! the main window); closing one's window puts it back in the tabs, and
//! QVDRAWINGCLOSE all of them.

use std::path::PathBuf;

use eframe::egui::{self, Rect, pos2, vec2};

use crate::app::OxidraftApp;

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
    /// A drawing opened (or gone back to) at one of its named views.
    OpenView(PathBuf, String),
}

/// The drawings open in the window, and which one is current.
pub struct Drawings {
    list: Vec<OxidraftApp>,
    active: usize,
    /// The drawings laid out side by side (SYSWINDOWS), each in its slot.
    arranged: Option<Arranged>,
}

/// Drawings in windows of their own: each one's slot (by its number in
/// the list), and where the main window was before.
struct Arranged {
    slots: Vec<(usize, Rect)>,
    main_was: Option<Rect>,
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

    /// Drawing `i`, current or not.
    pub fn drawing(&mut self, i: usize) -> Option<&mut OxidraftApp> {
        self.list.get_mut(i)
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
            // A drawing in a window of its own comes back to the tabs first.
            TabRequest::Switch(i) => {
                if self.arranged.as_ref().is_some_and(|a| a.slots.iter().any(|(d, _)| *d == i)) {
                    self.put_back(ctx);
                }
                self.switch(ctx, i)
            }
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
                self.put_back(ctx);
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
            TabRequest::OpenView(path, view) => {
                self.handle(ctx, TabRequest::Open(path.clone()));
                let app = &mut self.list[self.active];
                if app.path.as_deref() == Some(path.as_path()) {
                    app.go_to_view(&view);
                }
            }
            TabRequest::Arrange(a) => self.arrange(ctx, a),
            TabRequest::CloseArranged => self.put_back(ctx),
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

    /// SYSWINDOWS: the drawings side by side over where the main window
    /// is, the current one in it, each other in a window of its own.
    fn arrange(&mut self, ctx: &egui::Context, arrange: Arrange) {
        let n = self.list.len();
        if n < 2 {
            return;
        }
        let was = ctx.input(|i| i.viewport().outer_rect);
        let area = self
            .arranged
            .as_ref()
            .and_then(|a| a.main_was)
            .or(was)
            .unwrap_or(Rect::from_min_size(pos2(40.0, 40.0), vec2(1600.0, 1000.0)));
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
                    let step = 32.0;
                    let size = (area.size() - vec2(step, step) * (n - 1) as f32).max(vec2(640.0, 480.0));
                    Rect::from_min_size(area.min + vec2(step, step) * i as f32, size)
                }
            })
            .collect();
        // The current drawing first, in the main window.
        let order: Vec<usize> = std::iter::once(self.active).chain((0..n).filter(|i| *i != self.active)).collect();
        let slots: Vec<(usize, Rect)> = order.into_iter().zip(rects).collect();
        let main = slots[0].1;
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(main.min));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(main.size()));
        let main_was = self.arranged.as_ref().map_or(was, |a| a.main_was);
        self.arranged = Some(Arranged { slots, main_was });
    }

    /// The drawings back in the tabs, the main window where it was.
    fn put_back(&mut self, ctx: &egui::Context) {
        if let Some(a) = self.arranged.take()
            && let Some(was) = a.main_was
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(was.min));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(was.size()));
        }
    }

    /// The drawings in windows of their own, each worked in there; one
    /// whose window is closed goes back to the tabs.
    fn windows(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Some(a) = &self.arranged else { return };
        // Only where there can be windows of their own: two drawings in one
        // would share their panels.
        if ctx.embed_viewports() {
            return;
        }
        let slots = a.slots.clone();
        let tabs: Vec<(String, bool)> = self.list.iter().map(|d| (d.file_name(), d.is_dirty())).collect();
        let mut closed = Vec::new();
        let mut requests = Vec::new();
        for (i, rect) in slots.into_iter().filter(|(i, _)| *i != self.active) {
            let Some(app) = self.list.get_mut(i) else { continue };
            app.tabs = tabs.clone();
            app.tab = i;
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("{} - Oxidraft", app.file_name()))
                .with_position(rect.min)
                .with_inner_size(rect.size());
            let id = egui::ViewportId::from_hash_of(("drawing_window", i));
            let close = ctx.show_viewport_immediate(id, builder, |ui, _| {
                eframe::App::ui(app, ui, frame);
                ui.ctx().input(|i| i.viewport().close_requested())
            });
            if close {
                closed.push(i);
            }
            // What it asks of the window (a new drawing, opening one) is the main window's.
            if let Some(r) = app.tab_request.take() {
                requests.push(r);
            }
        }
        if let Some(a) = &mut self.arranged {
            a.slots.retain(|(i, _)| !closed.contains(i));
            if a.slots.len() < 2 {
                self.put_back(ctx);
            }
        }
        for r in requests {
            self.handle(ctx, r);
        }
    }

    /// The drawings in windows of their own, by their number in the list.
    pub fn in_windows(&self) -> Vec<usize> {
        self.arranged
            .as_ref()
            .map_or_else(Vec::new, |a| a.slots.iter().map(|s| s.0).filter(|i| *i != self.active).collect())
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
        self.windows(&ctx, frame);
    }
}

impl eframe::App for Drawings {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.frame(ui, frame);
    }
}

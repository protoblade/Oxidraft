use std::path::PathBuf;

use eframe::egui::{self, Key, Modifiers, Rect};
use oxidraft_commands::Engine;
use oxidraft_geom::Point;
use oxidraft_model::Document;

use crate::{
    theme,
    update::{UpdateState, Updater},
    view::View,
};

/// The main window: ribbon, file tabs, drawing area, command line, status bar.
pub struct OxidraftApp {
    pub(crate) doc: Document,
    pub(crate) engine: Engine,
    pub(crate) view: View,
    /// OSNAP (F3).
    pub(crate) osnap: bool,
    /// OSMODE: which running object snaps are on.
    pub(crate) osnap_modes: Vec<oxidraft_model::snap::SnapKind>,
    /// GRIDMODE (F7).
    pub(crate) grid: bool,
    /// LWDISPLAY: draw lines as thick as their lineweight.
    pub(crate) show_lineweights: bool,
    pub(crate) command_text: String,
    pub(crate) path: Option<PathBuf>,
    /// Document revision at the last save/open, for the `*` unsaved marker.
    pub(crate) saved_revision: u64,
    pub(crate) zoom_extents_pending: bool,
    /// A region to zoom to on the next frame (ZOOM Window).
    pub(crate) zoom_window: Option<oxidraft_geom::Aabb>,
    pub(crate) title: String,
    /// Snapped/tracked cursor in drawing units, from the last frame.
    pub(crate) cursor: Option<Point>,
    /// Drawing area from the last frame.
    pub(crate) canvas_rect: Rect,
    pub(crate) ribbon_tab: usize,
    /// F2: show more command history.
    pub(crate) history_expanded: bool,
    /// Highlighted AutoComplete suggestion.
    pub(crate) suggestion: usize,
    /// Position in typed history while recalling with the Up arrow.
    pub(crate) recall: Option<usize>,
    /// What a primary-button drag in the drawing area is doing.
    pub(crate) drag: Option<crate::canvas::Drag>,
    pub(crate) updater: Updater,
    pub(crate) update_dialog: bool,
    pub(crate) layer_manager_open: bool,
    /// One-shot snap from the shortcut menu for the next point:
    /// `Some(Some(kind))` snaps only to `kind`, `Some(None)` doesn't snap.
    pub(crate) snap_override: Option<Option<oxidraft_model::snap::SnapKind>>,
    pub(crate) properties_open: bool,
    /// Row selected in the Layer Properties Manager.
    pub(crate) selected_layer: Option<String>,
    /// Layer being renamed: (old name, edit buffer).
    pub(crate) renaming_layer: Option<(String, String)>,
    /// Text fields drawn this frame (palettes, dialogs); the command line
    /// doesn't steal keyboard focus from them.
    pub(crate) text_fields: Vec<egui::Id>,
    /// Same, from the previous frame (used before this frame's widgets exist).
    pub(crate) prev_text_fields: Vec<egui::Id>,
}

impl OxidraftApp {
    /// A new window with an empty drawing.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        Self::blank()
    }

    /// An app with an empty drawing and default settings, without reading the command line.
    pub fn blank() -> Self {
        let mut engine = Engine::new();
        engine.log(format!("Oxidraft {} - type a command, or HELP for a list.", env!("CARGO_PKG_VERSION")));
        Self {
            doc: Document::new(),
            engine,
            view: View::default(),
            osnap: true,
            osnap_modes: oxidraft_model::snap::SnapKind::DEFAULT_RUNNING.to_vec(),
            grid: true,
            show_lineweights: false,
            command_text: String::new(),
            path: None,
            saved_revision: 0,
            zoom_extents_pending: false,
            zoom_window: None,
            title: String::new(),
            cursor: None,
            canvas_rect: Rect::NOTHING,
            ribbon_tab: 0,
            history_expanded: false,
            suggestion: 0,
            recall: None,
            drag: None,
            updater: Updater::default(),
            update_dialog: false,
            layer_manager_open: false,
            snap_override: None,
            properties_open: false,
            selected_layer: None,
            renaming_layer: None,
            text_fields: Vec::new(),
            prev_text_fields: Vec::new(),
        }
    }

    /// Replaces the drawing and zooms to its extents.
    pub fn set_document(&mut self, doc: Document) {
        self.engine.cancel(&mut self.doc);
        self.doc = doc;
        self.saved_revision = self.doc.revision();
        self.zoom_extents_pending = true;
    }

    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    /// Types `text` on the command line and presses Enter.
    pub fn type_command(&mut self, text: &str) {
        self.command_text = text.to_string();
        self.submit();
    }

    /// Puts text on the command line without pressing Enter.
    pub fn set_command_text(&mut self, text: &str) {
        self.command_text = text.to_string();
    }

    /// Zooms so the window between two corners fills the view.
    pub fn zoom_to(&mut self, a: Point, b: Point) {
        self.zoom_window = Some(oxidraft_geom::Aabb::from_points(a, b));
    }

    /// Screen position of a drawing point, as of the last frame.
    pub fn screen_pos(&self, p: Point) -> eframe::egui::Pos2 {
        self.view.to_screen(self.canvas_rect, p)
    }

    pub(crate) fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("Drawing1.dxf".into(), |n| n.to_string_lossy().into_owned())
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision
    }

    pub(crate) fn pick_tolerance(&self) -> f64 {
        self.view.units(crate::canvas::APERTURE)
    }

    // ---- files -------------------------------------------------------------

    pub(crate) fn new_drawing(&mut self) {
        // First Esc ends any command, the second clears the selection.
        self.engine.cancel(&mut self.doc);
        self.engine.cancel(&mut self.doc);
        self.doc = Document::new();
        self.saved_revision = 0;
        self.path = None;
        self.view = View::default();
    }

    pub(crate) fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("DXF drawing", &["dxf"]).pick_file() {
            self.open_path(path);
        }
    }

    pub fn open_path(&mut self, path: PathBuf) {
        match oxidraft_io_dxf::load_file(&path) {
            Ok(imported) => {
                self.set_document(imported.document);
                let mut msg = format!("Opened {} ({} objects)", path.display(), self.doc.len());
                if imported.skipped > 0 {
                    msg.push_str(&format!(", skipped {} unsupported objects", imported.skipped));
                }
                self.engine.log(msg);
                self.path = Some(path);
            }
            Err(e) => self.engine.log(format!("Could not open {}: {e}", path.display())),
        }
    }

    pub(crate) fn save(&mut self, save_as: bool) {
        self.engine.cancel(&mut self.doc);
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let dialog = rfd::FileDialog::new().add_filter("DXF drawing", &["dxf"]).set_file_name(self.file_name());
                match dialog.save_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        match oxidraft_io_dxf::save_file(&self.doc, &path) {
            Ok(()) => {
                self.engine.log(format!("Saved {}", path.display()));
                self.path = Some(path);
                self.saved_revision = self.doc.revision();
            }
            Err(e) => self.engine.log(format!("Could not save {}: {e}", path.display())),
        }
    }

    // ---- commands ------------------------------------------------------------

    /// Starts a command from a ribbon button or menu, like clicking in AutoCAD.
    pub(crate) fn run(&mut self, name: &str) {
        self.command_text.clear();
        if !self.run_app_command(name) {
            self.engine.run(name, &mut self.doc);
        }
    }

    /// Commands about files and the view, which the engine doesn't know about.
    fn run_app_command(&mut self, name: &str) -> bool {
        match name.trim().to_ascii_uppercase().as_str() {
            "ZE" | "ZOOM" => self.zoom_extents_pending = true,
            "SAVE" | "QSAVE" => self.save(false),
            "SAVEAS" => self.save(true),
            "OPEN" => self.open_dialog(),
            "NEW" => self.new_drawing(),
            "LAYER" | "LA" => self.layer_manager_open = true,
            "PROPERTIES" | "PR" | "PROPS" | "MO" | "CH" => self.properties_open = true,
            _ => return false,
        }
        true
    }

    /// Enter on the command line.
    pub(crate) fn submit(&mut self) {
        let mut text = std::mem::take(&mut self.command_text);
        self.recall = None;
        if !self.engine.is_active() {
            // AutoComplete: Enter accepts the highlighted suggestion.
            if let Some(c) = Engine::suggest(&text).get(self.suggestion) {
                text = c.name.to_string();
            }
            self.suggestion = 0;
            if self.run_app_command(&text) {
                self.engine.log(format!("Command: {}", text.trim().to_uppercase()));
                return;
            }
        }
        let cursor = self.cursor.unwrap_or(Point::ORIGIN);
        let tolerance = self.pick_tolerance();
        self.engine.submit(&text, &mut self.doc, cursor, tolerance);
    }

    /// True while the keyboard belongs to a palette or dialog text field.
    pub(crate) fn editing_field(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.focused()).is_some_and(|f| self.prev_text_fields.contains(&f) || self.text_fields.contains(&f))
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let none = Modifiers::NONE;
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Num1)) {
            self.properties_open = !self.properties_open;
        }
        if self.editing_field(ctx) {
            return;
        }
        let empty = self.command_text.is_empty();
        let (esc, f2, f3, f7, f8, f10, f12) = ctx.input_mut(|i| {
            (
                i.consume_key(none, Key::Escape),
                i.consume_key(none, Key::F2),
                i.consume_key(none, Key::F3),
                i.consume_key(none, Key::F7),
                i.consume_key(none, Key::F8),
                i.consume_key(none, Key::F10),
                i.consume_key(none, Key::F12),
            )
        });
        let (undo, redo, save, open, new, delete) = ctx.input_mut(|i| {
            (
                i.consume_key(cmd, Key::Z),
                i.consume_key(cmd | Modifiers::SHIFT, Key::Z) || i.consume_key(cmd, Key::Y),
                i.consume_key(cmd, Key::S),
                i.consume_key(cmd, Key::O),
                i.consume_key(cmd, Key::N),
                empty && (i.consume_key(none, Key::Delete) || i.consume_key(none, Key::Backspace)),
            )
        });
        let select_all = empty && ctx.input_mut(|i| i.consume_key(cmd, Key::A));

        if esc {
            if !self.command_text.is_empty() {
                self.command_text.clear();
            }
            self.engine.cancel(&mut self.doc);
            self.suggestion = 0;
            self.recall = None;
            self.drag = None;
        }
        if f2 {
            self.history_expanded = !self.history_expanded;
        }
        if f3 {
            self.osnap = !self.osnap;
            self.log_toggle("Osnap", self.osnap);
        }
        if f7 {
            self.grid = !self.grid;
            self.log_toggle("Grid", self.grid);
        }
        if f8 {
            self.engine.set_ortho(!self.engine.ortho());
            self.log_toggle("Ortho", self.engine.ortho());
        }
        if f10 {
            self.engine.set_polar(!self.engine.polar());
            self.log_toggle("Polar", self.engine.polar());
        }
        if f12 {
            self.engine.dynamic_input = !self.engine.dynamic_input;
            self.log_toggle("Dynamic Input", self.engine.dynamic_input);
        }
        if redo {
            self.run("REDO");
        } else if undo {
            self.run("UNDO");
        }
        if save {
            self.save(false);
        }
        if open {
            self.open_dialog();
        }
        if new {
            self.new_drawing();
        }
        if select_all {
            self.engine.select_all(&self.doc);
        }
        if delete && !self.engine.is_active() && !self.engine.selection().is_empty() {
            self.run("ERASE");
        }
    }

    pub(crate) fn log_toggle(&mut self, name: &str, on: bool) {
        self.engine.log(format!("<{name} {}>", if on { "on" } else { "off" }));
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let dirty = if self.is_dirty() { "*" } else { "" };
        let title = format!("Oxidraft — {}{dirty}", self.file_name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl eframe::App for OxidraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.prev_text_fields = std::mem::take(&mut self.text_fields);
        self.handle_shortcuts(&ctx);
        self.update_title(&ctx);

        self.top_bar(ui);
        self.ribbon(ui);
        self.status_bar(ui);
        self.file_tabs(ui);
        self.properties_palette(ui);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::MODEL_BG)).show(ui, |ui| {
            self.canvas(ui);
        });
        self.layer_manager(&ctx);
        self.update_window(&ctx);
        self.command_line(&ctx);
    }
}

impl OxidraftApp {
    pub fn updater(&self) -> &Updater {
        &self.updater
    }

    pub fn show_update_dialog(&mut self) {
        self.update_dialog = true;
    }

    /// App menu > Check for Updates…
    pub(crate) fn check_for_updates(&mut self, ctx: &egui::Context) {
        self.update_dialog = true;
        self.updater.check(ctx, true);
    }

    /// The update dialog: offers, downloads and installs a newer release.
    fn update_window(&mut self, ctx: &egui::Context) {
        if !self.update_dialog {
            return;
        }
        let state = self.updater.state();
        let current = crate::update::CURRENT_VERSION;
        let mut open = true;
        egui::Window::new("Oxidraft Update")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(380.0);
                match &state {
                    UpdateState::Idle | UpdateState::Checking { .. } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Checking for updates...");
                        });
                    }
                    UpdateState::UpToDate => {
                        ui.label(format!("You're running the latest version ({current})."));
                        if ui.button("OK").clicked() {
                            self.update_dialog = false;
                        }
                    }
                    UpdateState::Available(release) => {
                        let version = release.version().map_or(release.tag_name.clone(), |v| v.to_string());
                        ui.label(egui::RichText::new(format!("Oxidraft {version} is available")).strong());
                        ui.label(format!("You have {current}."));
                        if let Some(notes) = release.body.as_deref().filter(|n| !n.trim().is_empty()) {
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                ui.label(notes);
                            });
                        }
                        ui.add_space(6.0);
                        if crate::update::is_dev_build() {
                            ui.colored_label(
                                theme::TEXT_DIM,
                                "This is a development build. Update the source with git pull and rebuild.",
                            );
                        }
                        ui.horizontal(|ui| {
                            if !crate::update::is_dev_build() && ui.button("Download and Install").clicked() {
                                self.updater.install(ctx, release.clone());
                            }
                            if ui.button("View Release").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(&release.html_url));
                            }
                            if ui.button("Later").clicked() {
                                self.update_dialog = false;
                            }
                        });
                    }
                    UpdateState::Installing(release) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("Downloading and installing {}...", release.tag_name));
                        });
                    }
                    UpdateState::Installed(version) => {
                        ui.label(format!("Oxidraft {version} is installed. Restart to start using it."));
                        if self.is_dirty() {
                            ui.colored_label(theme::GRIP_HOVER, "Save your drawing before restarting.");
                        }
                        ui.horizontal(|ui| {
                            if self.is_dirty() && ui.button("Save").clicked() {
                                self.save(false);
                            }
                            let restart = ui.add_enabled(!self.is_dirty(), egui::Button::new("Restart Now"));
                            if restart.clicked() {
                                match crate::update::restart() {
                                    Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                    Err(e) => self.updater.set(UpdateState::Failed(format!("Could not restart: {e}"))),
                                }
                            }
                            if ui.button("Later").clicked() {
                                self.update_dialog = false;
                            }
                        });
                    }
                    UpdateState::Failed(message) => {
                        ui.label(message);
                        ui.horizontal(|ui| {
                            if ui.button("View Releases").clicked() {
                                let url = format!("https://github.com/{}/releases", crate::update::REPO);
                                ctx.open_url(egui::OpenUrl::new_tab(url));
                            }
                            if ui.button("Close").clicked() {
                                self.update_dialog = false;
                            }
                        });
                    }
                }
            });
        if !open {
            self.update_dialog = false;
        }
    }
}

use std::path::PathBuf;

use eframe::egui::{self, Key, Modifiers, Rect};
use oxidraft_commands::{Engine, QuickSelect, UiRequest, ViewRequest};
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
    /// The DWG version the drawing was opened from, which saving keeps
    /// (`None` for DXF and new drawings).
    pub(crate) dwg_version: Option<oxidraft_io_dwg::Version>,
    /// Document revision at the last save/open, for the `*` unsaved marker.
    pub(crate) saved_revision: u64,
    pub(crate) zoom_extents_pending: bool,
    /// View changes to make on the next frame (from menus and buttons;
    /// commands' come from the engine).
    pub(crate) pending_views: Vec<ViewRequest>,
    /// The Quick Select dialog's choices while it's open.
    pub(crate) quick_select: Option<QuickSelect>,
    pub(crate) attribute_editor: Option<crate::attedit::AttributeEditor>,
    /// XATTACH's Attach External Reference dialog while it's open.
    pub(crate) attach_dialog: Option<crate::xref::AttachDialog>,
    /// The External References palette, and the reference selected in it.
    pub(crate) xref_palette_open: bool,
    pub(crate) xref_selected: Option<String>,
    /// What each attached drawing attaches itself, as read with it.
    pub(crate) nested_xrefs: std::collections::HashMap<String, Vec<(String, String, oxidraft_model::XrefStatus)>>,
    /// REFCLOSE's Save for an xref, waiting to be confirmed.
    pub(crate) xref_edit_save: Option<crate::xref::XrefEditSave>,
    /// Images and PDF pages: their pixels as read so far, the reader, and
    /// a count that changes whenever pictures come or go.
    pub(crate) rasters: std::collections::HashMap<String, crate::underlay::Raster>,
    pub(crate) loader: crate::underlay::Loader,
    pub(crate) raster_revision: u64,
    pub(crate) image_attach_dialog: Option<crate::underlay::ImageAttachDialog>,
    /// Images picked in the External References palette.
    pub(crate) image_selected: Vec<String>,
    /// The Plot dialog while it's open.
    pub(crate) plot: Option<crate::plot::PlotDialog>,
    /// The last plot's settings, which the next one starts from.
    pub(crate) plot_settings: oxidraft_plot::PlotSettings,
    /// The DXF text last copied here, for PASTECLIP when the system clipboard can't be read.
    pub(crate) clipboard: Option<String>,
    /// Earlier views, for ZOOM Previous.
    pub(crate) view_history: Vec<View>,
    /// Real-time PAN or ZOOM: the left mouse button moves the view.
    pub(crate) realtime: Option<ViewRequest>,
    /// When the last wheel, pinch or pan gesture happened (seconds).
    pub(crate) last_gesture: f64,
    pub(crate) title: String,
    /// Snapped/tracked cursor in drawing units, from the last frame.
    pub(crate) cursor: Option<Point>,
    /// Drawing area from the last frame.
    pub(crate) canvas_rect: Rect,
    /// Top of the command line's history and input from the last frame.
    pub(crate) command_line_top: f32,
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
            dwg_version: None,
            saved_revision: 0,
            zoom_extents_pending: false,
            pending_views: Vec::new(),
            quick_select: None,
            attribute_editor: None,
            attach_dialog: None,
            xref_palette_open: false,
            xref_selected: None,
            nested_xrefs: Default::default(),
            xref_edit_save: None,
            rasters: Default::default(),
            loader: Default::default(),
            raster_revision: 0,
            image_attach_dialog: None,
            image_selected: Vec::new(),
            plot: None,
            plot_settings: oxidraft_plot::PlotSettings {
                area: oxidraft_plot::PlotArea::Display(oxidraft_geom::Aabb::from_points(Point::ORIGIN, Point::ORIGIN)),
                ..Default::default()
            },
            clipboard: None,
            view_history: Vec::new(),
            realtime: None,
            last_gesture: f64::NEG_INFINITY,
            title: String::new(),
            cursor: None,
            canvas_rect: Rect::NOTHING,
            command_line_top: f32::INFINITY,
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
        self.view_history.clear();
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
        self.pending_views.push(ViewRequest::Window(oxidraft_geom::Aabb::from_points(a, b)));
    }

    /// Top of the command line's history and input, in screen points.
    pub fn command_line_top(&self) -> f32 {
        self.command_line_top
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
        self.dwg_version = None;
        self.rasters.clear();
        self.image_selected.clear();
        self.nested_xrefs.clear();
        self.view = View::default();
    }

    pub(crate) fn open_dialog(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Drawing (DWG, DXF)", &["dwg", "dxf"])
            .add_filter("AutoCAD drawing", &["dwg"])
            .add_filter("DXF drawing", &["dxf"]);
        if let Some(path) = dialog.pick_file() {
            self.open_path(path);
        }
    }

    /// Opens a DWG or DXF file (by its extension).
    pub fn open_path(&mut self, path: PathBuf) {
        let dwg = oxidraft_io_dwg::is_dwg(&path);
        let loaded = if dwg {
            oxidraft_io_dwg::load_file(&path).map(|o| (o.imported, Some(o.version)))
        } else {
            oxidraft_io_dxf::load_file(&path).map(|i| (i, None)).map_err(|e| e.to_string())
        };
        match loaded {
            Ok((imported, version)) => {
                self.set_document(imported.document);
                let mut msg = format!("Opened {} ({} objects", path.display(), self.doc.len());
                if let Some(v) = version {
                    msg.push_str(&format!(", {} DWG", v.name()));
                }
                msg.push(')');
                if imported.skipped > 0 {
                    msg.push_str(&format!(", skipped {} unsupported objects", imported.skipped));
                }
                self.engine.log(msg);
                self.path = Some(path);
                self.dwg_version = version;
                self.xref_selected = None;
                self.load_xrefs();
                self.load_pictures();
                // Reading what's attached isn't an edit, and can't be undone.
                self.doc.forget_history();
                self.saved_revision = self.doc.revision();
            }
            Err(e) => self.engine.log(format!("Could not open {}: {e}", path.display())),
        }
    }

    pub(crate) fn save(&mut self, save_as: bool) {
        self.engine.cancel(&mut self.doc);
        // The working set would be saved as part of the drawing.
        if self.doc.refedit().is_some() {
            self.engine.log("** Save or discard the reference edits first (REFCLOSE) **".to_string());
            return;
        }
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                // The current format comes first.
                let (dxf, dwg) = (("DXF drawing", ["dxf"]), ("AutoCAD drawing", ["dwg"]));
                let [first, second] = if self.dwg_version.is_some() { [dwg, dxf] } else { [dxf, dwg] };
                let dialog = rfd::FileDialog::new()
                    .add_filter(first.0, &first.1)
                    .add_filter(second.0, &second.1)
                    .set_file_name(self.file_name());
                match dialog.save_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        let saved = if oxidraft_io_dwg::is_dwg(&path) {
            let version = self.dwg_version.unwrap_or_default();
            oxidraft_io_dwg::save_file(&self.doc, &path, version).map(|()| Some(version))
        } else {
            oxidraft_io_dxf::save_file(&self.doc, &path).map(|()| None).map_err(|e| e.to_string())
        };
        match saved {
            Ok(version) => {
                let format = version.map_or(String::new(), |v| format!(" as {} DWG", v.name()));
                self.engine.log(format!("Saved {}{format}", path.display()));
                self.path = Some(path);
                self.dwg_version = version;
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
            "ZE" => self.pending_views.push(ViewRequest::Extents),
            "PASTECLIP" => self.paste_command(false),
            "PASTEORIG" => self.paste_command(true),
            "SAVE" | "QSAVE" => self.save(false),
            "SAVEAS" => self.save(true),
            "OPEN" => self.open_dialog(),
            "NEW" => self.new_drawing(),
            "LAYER" | "LA" => self.layer_manager_open = true,
            "PLOT" | "PRINT" => self.open_plot(false),
            "EXTERNALREFERENCES" | "ER" | "XREF" | "XR" => self.xref_palette_open = true,
            "XATTACH" | "XA" => self.xattach(),
            "IMAGEATTACH" | "IAT" => self.image_attach(),
            "PDFATTACH" => self.pdf_attach(),
            "PREVIEW" | "PRE" => self.open_plot(true),
            "QSELECT" => self.open_quick_select(),
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

    /// Opens what commands asked for. Returns true if there was anything.
    fn handle_ui_requests(&mut self) -> bool {
        let requests = self.engine.take_ui_requests();
        for r in &requests {
            match *r {
                UiRequest::EditAttributes(id) => {
                    self.edit_attributes(id);
                }
                UiRequest::TextWindow => self.history_expanded = true,
                UiRequest::SaveXrefEdit => self.prepare_xref_edit_save(),
                UiRequest::PlotWindow(window) => {
                    if let Some(d) = &mut self.plot {
                        if let Some(w) = window {
                            d.settings.area = oxidraft_plot::PlotArea::Window(w);
                        }
                        d.picking = false;
                    }
                }
            }
        }
        !requests.is_empty()
    }

    /// Where dialogs may go: above the command line, which floats over
    /// everything else.
    pub(crate) fn dialog_area(&self, ctx: &egui::Context) -> Rect {
        let screen = ctx.content_rect();
        let bottom = (self.command_line_top - 6.0).clamp(screen.top() + 200.0, screen.bottom());
        Rect::from_min_max(screen.min, egui::pos2(screen.right(), bottom))
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
        // The Plot dialog and its preview keep Esc and Enter to themselves.
        if self.plot_keys(ctx) {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::P)) {
            self.open_plot(false);
            return;
        }
        if self.editing_field(ctx) {
            return;
        }
        // Esc or Enter ends real-time pan or zoom, and nothing else.
        if self.realtime.is_some()
            && ctx.input_mut(|i| i.consume_key(none, Key::Escape) || i.consume_key(none, Key::Enter))
        {
            self.realtime = None;
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
        self.handle_clipboard_keys(&ctx);
        self.receive_pictures(&ctx);
        self.update_title(&ctx);

        self.top_bar(ui);
        self.ribbon(ui);
        self.status_bar(ui);
        self.file_tabs(ui);
        self.properties_palette(ui);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::MODEL_BG)).show(ui, |ui| {
            self.canvas(ui);
        });
        self.handle_ui_requests();
        self.layer_manager(&ctx);
        self.quick_select_dialog(&ctx);
        self.attribute_editor(&ctx);
        self.plot_dialog(&ctx);
        self.attach_dialog(&ctx);
        self.image_attach_dialog(&ctx);
        self.xref_palette(&ctx);
        self.xref_edit_save_dialog(&ctx);
        self.update_window(&ctx);
        self.command_line(&ctx);
        self.publish_copied(&ctx);
        // Requests from what was typed this frame show on the next.
        if self.handle_ui_requests() {
            ctx.request_repaint();
        }
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

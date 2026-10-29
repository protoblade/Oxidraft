use std::path::PathBuf;

use eframe::egui::{self, Key, Modifiers, Rect};
use oxidraft_commands::{Engine, QuickSelect, UiRequest, ViewRequest};
use oxidraft_geom::Point;
use oxidraft_model::Document;

use crate::{
    theme::{self, ColorTheme},
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
    /// The options (color theme, drawing background), where they're kept
    /// between runs (`None`: not kept, as in tests), and the Options dialog.
    pub(crate) settings: crate::settings::Settings,
    pub(crate) settings_path: Option<std::path::PathBuf>,
    pub(crate) options: Option<crate::settings::OptionsDialog>,
    /// REFCLOSE's Save for an xref, waiting to be confirmed.
    pub(crate) xref_edit_save: Option<crate::xref::XrefEditSave>,
    /// The Edit Block Definition dialog (its selected block), the block
    /// editor's view to go back to and the drawing's revision when last
    /// saved, the "Changes Not Saved" question, the Blocks palette, and the
    /// library drawings it has read.
    pub(crate) block_edit_dialog: Option<String>,
    pub(crate) block_editor_view: Option<crate::view::View>,
    pub(crate) block_editor_saved: u64,
    pub(crate) block_close_prompt: bool,
    pub(crate) blocks_palette: Option<crate::blocks::BlocksPalette>,
    pub(crate) library_drawings: crate::blocks::LibraryDrawings,
    /// Images and PDF pages: their pixels as read so far, the reader, and
    /// a count that changes whenever pictures come or go.
    pub(crate) rasters: std::collections::HashMap<String, crate::underlay::Raster>,
    /// The parts of pictures on screen read sharper than their overviews,
    /// what drawing found it wants this frame, and a count to tell the
    /// latest request from older ones.
    pub(crate) details: std::collections::HashMap<String, crate::underlay::Detail>,
    pub(crate) detail_wants: std::cell::RefCell<Vec<crate::underlay::DetailWant>>,
    pub(crate) detail_generation: u64,
    /// The layers with pictures on them that showed when last looked at,
    /// and the drawing's revision then: a picture whose layer is turned on
    /// loads.
    pub(crate) shown_image_layers: Option<(u64, std::collections::HashSet<String>)>,
    /// Each space's view when it was left: model space and the layouts
    /// keep their own.
    pub(crate) space_views: std::collections::HashMap<oxidraft_model::Space, View>,
    /// The viewport worked in (MSPACE, VPMAX), and each layout's last one.
    pub(crate) active_viewport: Option<crate::viewport::ActiveViewport>,
    pub(crate) last_viewport: std::collections::HashMap<oxidraft_model::Space, oxidraft_model::EntityId>,
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
    /// The floating Properties palette's place, and where the pointer
    /// holds it while it's dragged by its title.
    pub(crate) properties_pos: egui::Pos2,
    pub(crate) properties_grab: Option<egui::Vec2>,
    /// Rows selected in the Layer Properties Manager.
    pub(crate) selected_layers: Vec<String>,
    /// Where a Shift-click's run of layers starts.
    pub(crate) layer_anchor: Option<String>,
    /// Layer being renamed: (old name, edit buffer).
    pub(crate) renaming_layer: Option<(String, String)>,
    /// Text fields drawn this frame (palettes, dialogs); the command line
    /// doesn't steal keyboard focus from them.
    pub(crate) text_fields: Vec<egui::Id>,
    /// Same, from the previous frame (used before this frame's widgets exist).
    pub(crate) prev_text_fields: Vec<egui::Id>,
    /// The widget that had the keyboard at the end of the last frame.
    pub(crate) last_focus: Option<egui::Id>,
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
            block_edit_dialog: None,
            block_editor_view: None,
            block_editor_saved: 0,
            block_close_prompt: false,
            blocks_palette: None,
            library_drawings: Default::default(),
            settings: Default::default(),
            settings_path: None,
            options: None,
            rasters: Default::default(),
            details: Default::default(),
            detail_wants: Default::default(),
            detail_generation: 0,
            shown_image_layers: None,
            space_views: Default::default(),
            active_viewport: None,
            last_viewport: Default::default(),
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
            properties_pos: egui::pos2(320.0, 220.0),
            properties_grab: None,
            selected_layers: Vec::new(),
            layer_anchor: None,
            renaming_layer: None,
            text_fields: Vec::new(),
            prev_text_fields: Vec::new(),
            last_focus: None,
        }
    }

    /// The Model and layout tabs: draws in `space` from now on, with the
    /// view it had when left (a layout seen first shows its whole sheet).
    pub fn switch_space(&mut self, space: oxidraft_model::Space) {
        // Out of a viewport first: back on its layout.
        self.leave_viewport();
        if space == self.doc.space() {
            return;
        }
        // A command and the selection belong to the space they were in.
        self.engine.cancel(&mut self.doc);
        self.engine.cancel(&mut self.doc);
        self.space_views.insert(self.doc.space(), self.view);
        self.doc.set_space(space);
        match (self.space_views.get(&space), self.doc.layout(space)) {
            (Some(view), _) => self.view = *view,
            (None, Some(layout)) => {
                let (sheet, _) = layout.paper.sheet_bounds();
                self.pending_views.push(ViewRequest::Window(sheet));
            }
            (None, None) => self.zoom_extents_pending = true,
        }
    }

    /// A first layout for a drawing that has none, as AutoCAD's Layout1 is:
    /// an A4 sheet, landscape.
    pub(crate) fn first_layout(&mut self) -> oxidraft_model::Space {
        let paper = oxidraft_model::Paper {
            size_name: "ISO_A4_(297.00_x_210.00_MM)".into(),
            width: 297.0,
            height: 210.0,
            margins: [7.5, 20.0, 7.5, 20.0],
            rotation: 0,
            millimeters: true,
            raw: Vec::new(),
        };
        let layout =
            oxidraft_model::layout::Layout { id: oxidraft_model::MODEL, name: "Layout1".into(), tab_order: 1, paper };
        self.doc.add_layout(layout)
    }

    /// Keeps the options in `path` from now on, as the app does in the
    /// user's configuration folder.
    pub fn keep_settings_in(&mut self, path: PathBuf) {
        self.settings_path = Some(path);
    }

    /// Replaces the drawing and zooms to its extents.
    pub fn set_document(&mut self, doc: Document) {
        self.engine.cancel(&mut self.doc);
        self.doc = doc;
        self.saved_revision = self.doc.revision();
        self.zoom_extents_pending = true;
        self.shown_image_layers = None;
        self.space_views.clear();
        self.active_viewport = None;
        self.last_viewport.clear();
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
        self.shown_image_layers = None;
        self.space_views.clear();
        self.active_viewport = None;
        self.last_viewport.clear();
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
        if self.in_block_editor() {
            self.engine.log("** Close the Block Editor first (BCLOSE) **".to_string());
            return;
        }
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
            oxidraft_io_dwg::save_dxf(&self.doc, &path).map(|()| None)
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
            "OPTIONS" | "OP" => self.open_options(),
            "BEDIT" | "BE" => self.open_block_edit_dialog(None),
            "BSAVE" => self.save_block(),
            "BCLOSE" => self.close_block_editor(None),
            "BLOCKSPALETTE" => self.open_blocks_palette(),
            "PROPERTIES" | "PR" | "PROPS" | "MO" | "CH" => self.properties_open = true,
            "MSPACE" | "MS" => self.model_space_command(),
            "PSPACE" | "PS" => self.paper_space_command(false),
            "VPMIN" => self.paper_space_command(true),
            "VPMAX" => return self.maximize_command(),
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
            let printed = self.engine.history().len();
            if self.run_app_command(&text) {
                self.engine.log_at(printed, format!("Command: {}", text.trim().to_uppercase()));
                return;
            }
        }
        let cursor = self.cursor.unwrap_or(Point::ORIGIN);
        let tolerance = self.pick_tolerance();
        self.engine.submit(&text, &mut self.doc, cursor, tolerance);
    }

    /// Opens what commands asked for. Returns true if there was anything.
    fn handle_ui_requests(&mut self, ctx: &egui::Context) -> bool {
        let requests = self.engine.take_ui_requests();
        for r in &requests {
            match *r {
                UiRequest::EditAttributes(id) => {
                    self.edit_attributes(id);
                }
                UiRequest::TextWindow => self.history_expanded = true,
                UiRequest::MaximizeViewport(id) => self.enter_viewport(id, true),
                UiRequest::SaveXrefEdit => self.prepare_xref_edit_save(),
                UiRequest::ColorTheme(n) => {
                    let color_theme = if n == 1 { ColorTheme::Light } else { ColorTheme::Dark };
                    self.apply_settings(ctx, crate::settings::Settings { color_theme, ..self.settings.clone() });
                }
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

    /// Esc closes the topmost open dialog as its Cancel would: the
    /// Enhanced Attribute Editor, Save Reference Edits, Options, Quick
    /// Select or an Attach dialog. Returns true if it closed one.
    fn close_dialog_on_escape(&mut self, ctx: &egui::Context) -> bool {
        let open = self.attribute_editor.is_some()
            || self.block_edit_dialog.is_some()
            || self.block_close_prompt
            || self.xref_edit_save.is_some()
            || self.options.is_some()
            || self.quick_select.is_some()
            || self.attach_dialog.is_some()
            || self.image_attach_dialog.is_some();
        if !open || !ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            return false;
        }
        if self.xref_edit_save.take().is_some() {
            self.engine.log("Reference edits not saved; still editing.".to_string());
        } else if !std::mem::take(&mut self.block_close_prompt)
            && self.block_edit_dialog.take().is_none()
            && self.attribute_editor.take().is_none()
            && self.options.take().is_none()
            && self.quick_select.take().is_none()
            && self.attach_dialog.take().is_none()
        {
            self.image_attach_dialog = None;
        }
        true
    }

    /// True while the keyboard belongs to a palette or dialog text field.
    /// egui takes the keyboard from a field as soon as Esc is pressed, so
    /// the field that had it last frame still counts: that Esc is its.
    pub(crate) fn editing_field(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.focused())
            .or(self.last_focus)
            .is_some_and(|f| self.prev_text_fields.contains(&f) || self.text_fields.contains(&f))
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
        // Esc is the open dialog's Cancel, even while one of its fields has
        // the keyboard.
        if self.close_dialog_on_escape(ctx) {
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
            // With no command to end, Esc also closes the Properties palette
            // (along with the selection it showed).
            if !self.engine.is_active() {
                self.properties_open = false;
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
        self.load_images_turned_on();
        self.update_title(&ctx);

        self.top_bar(ui);
        self.ribbon(ui);
        self.status_bar(ui);
        self.file_tabs(ui);
        self.properties_palette(ui);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::model_bg())).show(ui, |ui| {
            self.canvas(ui);
        });
        self.handle_ui_requests(&ctx);
        self.layer_manager(&ctx);
        self.quick_select_dialog(&ctx);
        self.attribute_editor(&ctx);
        self.plot_dialog(&ctx);
        self.attach_dialog(&ctx);
        self.image_attach_dialog(&ctx);
        self.xref_palette(&ctx);
        self.xref_edit_save_dialog(&ctx);
        self.options_dialog(&ctx);
        self.block_edit_dialog(&ctx);
        self.block_close_dialog(&ctx);
        self.blocks_palette(&ctx);
        self.update_window(&ctx);
        self.command_line(&ctx);
        self.publish_copied(&ctx);
        // Requests from what was typed this frame show on the next.
        if self.handle_ui_requests(&ctx) {
            ctx.request_repaint();
        }
        self.last_focus = ctx.memory(|m| m.focused());
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
                                theme::text_dim(),
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
                            ui.colored_label(theme::grip_hover(), "Save your drawing before restarting.");
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

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
    /// GRIDUNIT: the grid's spacing; `None` adapts to the zoom.
    pub(crate) grid_spacing: Option<f64>,
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
    /// The space `view` is of; when the drawing's changes under it (LAYOUT,
    /// undo), the view follows.
    pub(crate) view_space: oxidraft_model::Space,
    /// A layout tab being renamed in place, a layout about to be deleted,
    /// and the Page Setup dialog.
    pub(crate) renaming_layout: Option<crate::layouts::Renaming>,
    pub(crate) delete_layout_prompt: Option<oxidraft_model::Space>,
    pub(crate) page_setup: Option<crate::layouts::PageSetup>,
    pub(crate) move_or_copy: Option<crate::layouts::MoveOrCopy>,
    /// TABLE's Insert Table dialog (with the settings it last had), and
    /// the table editor.
    pub(crate) insert_table: Option<crate::tables::InsertTable>,
    pub(crate) table_settings: crate::tables::InsertTable,
    pub(crate) table_editor: Option<crate::tables::TableEditor>,
    /// VPORTS: model space's panes; VIEW's View Manager; whether the file
    /// tabs and the layout tabs show.
    pub(crate) tiles: crate::tiles::Tiles,
    pub(crate) view_manager: Option<crate::named_views::ViewManager>,
    pub(crate) show_file_tabs: bool,
    pub(crate) show_layout_tabs: bool,
    /// PURGE's and ALIASEDIT's dialogs.
    pub(crate) purge: Option<crate::manage::PurgeDialog>,
    /// COMPARE: the drawing compared with, while comparing.
    pub(crate) comparison: Option<crate::compare::Comparison>,
    pub(crate) xcomparison: Option<crate::compare::XComparison>,
    /// DIMSTYLE's dialog (the style and DIMSCALE being set), and FIND's.
    pub(crate) dim_style_dialog: Option<(oxidraft_model::annotation::DimStyle, f64)>,
    pub(crate) find_replace: Option<crate::styles::FindReplace>,
    pub(crate) alias_dialog: Option<crate::manage::AliasDialog>,
    /// LAYWALK: the layers picked to show while it's open.
    pub(crate) layer_walk: Option<crate::layer_walk::LayerWalk>,
    /// EXPORTPDF's and PUBLISH's dialogs.
    pub(crate) export_pdf: Option<crate::publish::ExportPdf>,
    /// PLOTTERMANAGER's dialog.
    pub(crate) plotter_manager: Option<crate::plotters::PlotterManager>,
    /// CUI's dialog.
    pub(crate) cui: Option<crate::customize::CuiDialog>,
    /// The Sheet Set Manager (SHEETSET).
    pub(crate) sheet_set: Option<crate::sheet_set::SheetSetPalette>,
    /// RIBBONCLOSE and COMMANDLINEHIDE.
    pub(crate) ribbon_hidden: bool,
    pub(crate) command_line_hidden: bool,
    /// The palettes HIDEPALETTES hid, for SHOWPALETTES.
    pub(crate) hidden_palettes: Option<crate::more_app::HiddenPalettes>,
    /// LOGFILEON: the log file, and how much of the history is in it.
    pub(crate) log_file: Option<(std::path::PathBuf, usize)>,
    /// PLOTSTAMP.
    pub(crate) plot_stamp: bool,
    /// FONTMAP's font mapping file.
    pub(crate) font_map: String,
    /// AutoLISP: what APPLOAD and the command line defined.
    pub(crate) lisp: crate::lisp_host::LispThread,
    /// VSLIDE's slide, shown until REDRAW.
    pub(crate) slide: Option<oxidraft_plot::slide::Slide>,
    /// DATALINK's file, and the newest object before its table was placed.
    pub(crate) pending_data_link: Option<(String, oxidraft_model::EntityId)>,
    /// QVLAYOUT's list open.
    pub(crate) qv_layouts: bool,
    /// APERTURE: how far object snaps reach, in screen points.
    pub(crate) aperture: f32,
    /// SAVETIME (minutes between automatic saves; 0 none), the last one,
    /// and DRAWINGRECOVERY's window.
    pub(crate) save_time: u32,
    pub(crate) last_autosave: std::time::Instant,
    pub(crate) recovery_open: bool,
    /// AUTOCOMPLETE off, and HYPERLINKOPTIONS' tooltips off.
    pub(crate) autocomplete_off: bool,
    pub(crate) hyperlink_tips_off: bool,
    pub(crate) publish: Option<crate::publish::PublishDialog>,
    /// UNITS: the settings being chosen.
    pub(crate) units_dialog: Option<oxidraft_model::Units>,
    pub(crate) rename: Option<crate::manage::RenameDialog>,
    pub(crate) wblock: Option<crate::wblock::WblockDialog>,
    pub(crate) quickcalc: Option<crate::quickcalc::QuickCalc>,
    pub(crate) color_dialog: Option<crate::object_props::ColorDialog>,
    pub(crate) linetype_manager: Option<crate::object_props::LinetypeManager>,
    pub(crate) lineweight_dialog: Option<crate::object_props::LineweightDialog>,
    pub(crate) text_style_dialog: Option<crate::text_styles::TextStyleDialog>,
    pub(crate) drafting_settings: Option<crate::drafting::DraftingSettings>,
    pub(crate) point_style: Option<crate::drafting::PointStyle>,
    pub(crate) tolerance_dialog: Option<crate::tolerance::ToleranceDialog>,
    pub(crate) field_dialog: Option<crate::fields::FieldDialog>,
    pub(crate) underlay_layers_dialog: Option<crate::ulayers::UnderlayLayers>,
    pub(crate) pdf_import_dialog: Option<crate::pdf_import::PdfImportDialog>,
    pub(crate) array_dialog: Option<crate::classic::ArrayDialog>,
    pub(crate) group_dialog: Option<crate::classic::GroupDialog>,
    pub(crate) layout_wizard: Option<crate::classic::LayoutWizard>,
    pub(crate) paste_special: Option<crate::paste_scales::PasteSpecial>,
    pub(crate) scale_list_dialog: Option<crate::paste_scales::ScaleList>,
    /// ANNOAUTOSCALE: a new annotation scale is added to annotative objects.
    pub(crate) anno_autoscale: bool,
    /// TOOLPALETTES, while open.
    pub(crate) tool_palettes: Option<crate::tool_palettes::ToolPalettesWindow>,
    /// ADCENTER: DesignCenter, while it's open.
    pub(crate) design_center: Option<crate::design_center::DesignCenter>,
    /// STYLESMANAGER, the Plot Style Table Editor, and where plot style
    /// tables are kept (else beside the settings).
    pub(crate) styles_manager: Option<crate::plot_styles::StylesManager>,
    pub(crate) pen_table_editor: Option<crate::plot_styles::PenTableEditor>,
    pub(crate) plot_styles_dir: Option<PathBuf>,
    pub(crate) ucs_dialog: Option<crate::ucs_dialog::UcsDialog>,
    /// The last script run, for RSCRIPT.
    pub(crate) last_script: Option<String>,
    /// PNGOUT and the like: the image being saved.
    pub(crate) image_out: Option<crate::image_out::ImageOut>,
    /// CLEANSCREENON (Ctrl+0): the ribbon and palettes hidden.
    pub(crate) clean_screen: bool,
    /// When the drawing was opened or started, for TIME.
    pub(crate) opened_at: std::time::Instant,
    pub(crate) spell: Option<crate::spell::SpellDialog>,
    pub(crate) inspect_dialog: Option<crate::diminspect::InspectDialog>,
    pub(crate) text_editor: Option<crate::fields::TextEditor>,
    /// When the drawing's file was made and saved, for fields.
    pub(crate) file_times: crate::fields::FileTimes,
    pub(crate) style_dialog: Option<crate::annotation_styles::StyleDialog>,
    pub(crate) attribute_manager: Option<crate::battman::AttributeManager>,
    pub(crate) layer_states_dialog: Option<crate::layer_states::LayerStatesDialog>,
    /// "Save changes?" before NEW, OPEN, CLOSE or QUIT; and quitting once answered.
    pub(crate) save_prompt: Option<crate::files::Then>,
    pub(crate) quit_now: bool,
    /// The drawings open in this window, for the file tabs (name, and
    /// whether it has changes), and which tab this one is; set each frame
    /// by [`crate::Drawings`].
    pub(crate) tabs: Vec<(String, bool)>,
    pub(crate) tab: usize,
    /// What this drawing asked of the window this frame: another tab,
    /// a new or opened drawing, closing.
    pub(crate) tab_request: Option<crate::drawings::TabRequest>,
    /// The number in a new drawing's name (Drawing1, Drawing2, ...).
    pub(crate) untitled: u32,
    pub(crate) properties_dialog: Option<crate::files::PropertiesDialog>,
    pub(crate) from_template: Option<crate::layouts::FromTemplate>,
    /// The space the ribbon last saw: a layout brings its Layout tab up.
    pub(crate) ribbon_space: oxidraft_model::Space,
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
    /// The whole drawing area, all model viewports together: where the
    /// command line floats.
    pub(crate) drawing_rect: Rect,
    /// Drawing model space through a viewport: the sheet's screen points
    /// per paper unit, which linetypes go by there (PSLTSCALE 1).
    pub(crate) sheet_scale: Option<f64>,
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
    /// SELECTIONCYCLING: a click on objects on top of each other lists
    /// them to choose from (Ctrl+W), and the list while it's open.
    pub(crate) selection_cycling: bool,
    pub(crate) cycling: Option<crate::canvas::Cycling>,
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
            grid_spacing: None,
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
            view_space: oxidraft_model::MODEL,
            renaming_layout: None,
            delete_layout_prompt: None,
            page_setup: None,
            move_or_copy: None,
            insert_table: None,
            table_settings: Default::default(),
            table_editor: None,
            tiles: Default::default(),
            view_manager: None,
            show_file_tabs: true,
            show_layout_tabs: true,
            purge: None,
            comparison: None,
            xcomparison: None,
            dim_style_dialog: None,
            find_replace: None,
            alias_dialog: None,
            layer_walk: None,
            export_pdf: None,
            plotter_manager: None,
            cui: None,
            sheet_set: None,
            ribbon_hidden: false,
            command_line_hidden: false,
            hidden_palettes: None,
            log_file: None,
            plot_stamp: false,
            font_map: String::new(),
            lisp: Default::default(),
            slide: None,
            pending_data_link: None,
            qv_layouts: false,
            aperture: crate::canvas::APERTURE,
            save_time: 10,
            last_autosave: std::time::Instant::now(),
            recovery_open: false,
            autocomplete_off: false,
            hyperlink_tips_off: false,
            publish: None,
            units_dialog: None,
            rename: None,
            wblock: None,
            quickcalc: None,
            color_dialog: None,
            linetype_manager: None,
            lineweight_dialog: None,
            text_style_dialog: None,
            drafting_settings: None,
            point_style: None,
            tolerance_dialog: None,
            field_dialog: None,
            underlay_layers_dialog: None,
            pdf_import_dialog: None,
            array_dialog: None,
            group_dialog: None,
            layout_wizard: None,
            paste_special: None,
            scale_list_dialog: None,
            anno_autoscale: false,
            tool_palettes: None,
            design_center: None,
            styles_manager: None,
            pen_table_editor: None,
            plot_styles_dir: None,
            ucs_dialog: None,
            last_script: None,
            image_out: None,
            clean_screen: false,
            opened_at: std::time::Instant::now(),
            spell: None,
            inspect_dialog: None,
            text_editor: None,
            file_times: Default::default(),
            style_dialog: None,
            attribute_manager: None,
            layer_states_dialog: None,
            save_prompt: None,
            quit_now: false,
            tabs: Vec::new(),
            tab: 0,
            tab_request: None,
            untitled: 1,
            properties_dialog: None,
            from_template: None,
            ribbon_space: oxidraft_model::MODEL,
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
            drawing_rect: Rect::NOTHING,
            sheet_scale: None,
            command_line_top: f32::INFINITY,
            ribbon_tab: 0,
            history_expanded: false,
            suggestion: 0,
            recall: None,
            drag: None,
            selection_cycling: true,
            cycling: None,
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
        self.show_space_view();
    }

    /// A first layout for a drawing that has none, as AutoCAD's Layout1 is:
    /// an A4 sheet, landscape.
    pub(crate) fn first_layout(&mut self) -> oxidraft_model::Space {
        let paper = oxidraft_model::Paper::a4_landscape();
        let layout =
            oxidraft_model::layout::Layout { id: oxidraft_model::MODEL, name: "Layout1".into(), tab_order: 1, paper };
        self.doc.add_layout(layout)
    }

    /// Keeps the options in `path` from now on, as the app does in the
    /// user's configuration folder.
    pub fn keep_settings_in(&mut self, path: PathBuf) {
        self.settings_path = Some(path);
        self.load_plot_styles();
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
        self.tiles = Default::default();
        self.comparison = None;
        self.view_space = self.doc.space();
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

    /// What's typed on the command line, not yet entered.
    pub fn command_text(&self) -> &str {
        &self.command_text
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
            .map_or_else(|| format!("Drawing{}.dxf", self.untitled.max(1)), |n| n.to_string_lossy().into_owned())
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision
    }

    pub(crate) fn pick_tolerance(&self) -> f64 {
        self.view.units(self.aperture)
    }

    // ---- files -------------------------------------------------------------

    pub(crate) fn new_drawing(&mut self) {
        // First Esc ends any command, the second clears the selection.
        self.engine.cancel(&mut self.doc);
        self.engine.cancel(&mut self.doc);
        self.doc = Document::new();
        self.file_times = Default::default();
        self.opened_at = std::time::Instant::now();
        self.shown_image_layers = None;
        self.space_views.clear();
        self.active_viewport = None;
        self.last_viewport.clear();
        self.tiles = Default::default();
        self.view_space = oxidraft_model::MODEL;
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
            oxidraft_io_dwg::load_dxf(&path).map(|i| (i, None))
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
                self.file_times = crate::fields::FileTimes::of(&path);
                self.opened_at = std::time::Instant::now();
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
                self.drop_autosave();
                self.just_saved();
                self.path = Some(path);
                self.dwg_version = version;
                self.saved_revision = self.doc.revision();
            }
            Err(e) => self.engine.log(format!("Could not save {}: {e}", path.display())),
        }
    }

    // ---- commands ------------------------------------------------------------

    /// Starts a command from a ribbon button or menu, like clicking in AutoCAD.
    /// A button's macro may pick the command's options too, as AutoCAD's
    /// "^C^C_MVIEW _L _ON" does: "MVIEW L ON".
    pub(crate) fn run(&mut self, name: &str) {
        self.command_text.clear();
        let mut words = name.split_whitespace();
        let Some(command) = words.next() else { return };
        if !self.run_app_command(command) {
            self.engine.run(command, &mut self.doc);
            let (cursor, tolerance) = (self.cursor.unwrap_or(Point::ORIGIN), self.pick_tolerance());
            for option in words {
                self.engine.submit(option, &mut self.doc, cursor, tolerance);
            }
        }
    }

    /// Commands about files and the view, which the engine doesn't know about.
    fn run_app_command(&mut self, name: &str) -> bool {
        // A slide shows until the drawing is drawn again.
        if matches!(
            name.trim().to_ascii_uppercase().as_str(),
            "REDRAW" | "REDRAWALL" | "REGEN" | "REGENALL" | "R" | "RA"
        ) {
            self.slide = None;
        }
        match name.trim().to_ascii_uppercase().as_str() {
            "ZE" => self.pending_views.push(ViewRequest::Extents),
            "PASTECLIP" => self.paste_command(false),
            "PASTEORIG" => self.paste_command(true),
            "PASTEBLOCK" => self.paste_block_command(),
            "SAVE" | "QSAVE" => self.save(false),
            "SAVEAS" => self.save(true),
            // With other drawings open beside it, new and opened ones get
            // tabs of their own.
            "OPEN" | "DXFIN" if self.in_tabs() => self.open_in_tab(),
            "NEW" | "QNEW" if self.in_tabs() => self.tab_request = Some(crate::drawings::TabRequest::New),
            "CLOSEALL" if self.in_tabs() => self.tab_request = Some(crate::drawings::TabRequest::CloseAll),
            "SWITCHWINDOWS" => self.tab_request = Some(crate::drawings::TabRequest::Next(false)),
            "CASCADE" => {
                self.tab_request = Some(crate::drawings::TabRequest::Arrange(crate::drawings::Arrange::Cascade))
            }
            "TILE" => self.tab_request = Some(crate::drawings::TabRequest::Arrange(crate::drawings::Arrange::Vertical)),
            "OPEN" => self.after_saving(crate::files::Then::Open),
            "NEW" => self.after_saving(crate::files::Then::New),
            "CLOSE" | "CLOSEALL" => self.after_saving(crate::files::Then::Close),
            "QUIT" | "EXIT" => self.after_saving(crate::files::Then::Quit),
            "RECOVER" => self.after_saving(crate::files::Then::Recover),
            "SCRIPT" | "SCR" => self.script_dialog(),
            "DWGPROPS" => self.open_drawing_properties(),
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
            "PAGESETUP" => self.page_setup_command(),
            "TABLE" | "TB" => self.open_insert_table(),
            "VIEW" | "V" => self.open_view_manager(),
            "PURGE" | "PU" => self.open_purge(),
            "ALIASEDIT" => self.open_aliases(),
            "LAYWALK" => self.open_layer_walk(),
            "EXPORTPDF" => self.open_export_pdf(),
            "EXPORTDWFX" => self.open_export(oxidraft_plot::Plotter::Dwfx),
            "EXPORTDWF" => {
                self.engine.log("DWF6 isn't written: the sheets go to DWFx, which DWF viewers open.");
                self.open_export(oxidraft_plot::Plotter::Dwfx);
            }
            "PLOTTERMANAGER" => self.open_plotter_manager(),
            "CUI" => self.open_cui(),
            "UNISOLATEOBJECTS" | "UNHIDE" => {
                let n = self.doc.unhide_objects();
                self.engine.log(format!("{n} object(s) shown again."));
            }
            "TEXTSCR" => self.history_expanded = true,
            "COPYHIST" => {
                let text = self.engine.history().join("\n");
                match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
                    Ok(()) => self.engine.log("Command history copied to the clipboard."),
                    Err(e) => self.engine.log(format!("Could not copy: {e}")),
                }
            }
            "LOGFILEON" => self.log_file_on(),
            "LOGFILEOFF" => {
                self.write_log();
                self.log_file = None;
            }
            "ABOUT" => self.about(),
            "RIBBON" => self.ribbon_hidden = false,
            "RIBBONCLOSE" => self.ribbon_hidden = true,
            "COMMANDLINE" => self.command_line_hidden = false,
            "COMMANDLINEHIDE" => self.command_line_hidden = true,
            "HIDEPALETTES" | "POFF" => self.hide_palettes(true),
            "SHOWPALETTES" | "PON" => self.hide_palettes(false),
            "PROPERTIESCLOSE" | "PRCLOSE" => self.properties_open = false,
            "LAYERCLOSE" => self.layer_manager_open = false,
            "EXTERNALREFERENCESCLOSE" => self.xref_palette_open = false,
            "QCCLOSE" => self.quickcalc = None,
            "ARRAYCLOSE" => self.engine.log("No array is being edited in place: ARRAYEDIT changes one."),
            "TIFOUT" => self.image_out_command("TIFF", &["tif", "tiff"]),
            "EXPORTLAYOUT" => self.export_layout(),
            "DWGCONVERT" => self.dwg_convert(),
            "PLOTSTAMP" => self.toggle_plot_stamp(),
            "WHOHAS" => self.who_has(),
            "EATTEXT" | "ATTEXT" | "DATAEXTRACTION" | "DX" => self.extract_attributes(),
            "WMFIN" => self.wmf_in(),
            "PUBLISHTOWEB" => self.publish_to_web(),
            "PSETUPIN" => self.psetupin(),
            "QVDRAWING" => {
                self.tab_request = Some(crate::drawings::TabRequest::Arrange(crate::drawings::Arrange::Vertical))
            }
            "QVDRAWINGCLOSE" => self.tab_request = Some(crate::drawings::TabRequest::CloseArranged),
            "QVLAYOUT" => self.qv_layouts = true,
            "QVLAYOUTCLOSE" => self.qv_layouts = false,
            "CUIEXPORT" => self.cui_export(),
            "CUIIMPORT" | "CUILOAD" => self.cui_import(),
            "CUIUNLOAD" => {
                let settings = crate::settings::Settings {
                    shortcuts: Vec::new(),
                    quick_access: Vec::new(),
                    ..self.settings.clone()
                };
                self.keep_settings(settings);
                self.engine.log("Back to the built-in shortcuts and Quick Access Toolbar.");
            }
            "AUTOCOMPLETE" => {
                self.autocomplete_off = !self.autocomplete_off;
                self.log_toggle("AutoComplete", !self.autocomplete_off);
            }
            "HYPERLINKOPTIONS" => {
                self.hyperlink_tips_off = !self.hyperlink_tips_off;
                self.log_toggle("Hyperlink tooltips", !self.hyperlink_tips_off);
            }
            "MARKUPCLOSE" => self.markup_close(),
            "DATALINK" | "DL" => self.data_link(),
            "DATALINKUPDATE" | "DLU" => {
                self.data_link_update();
            }
            "BAUTHORPALETTE" | "BAUTHORPALETTECLOSE" | "BTESTBLOCK" | "BACTIONBAR" | "BCYCLEORDER" | "BGRIPSET" => {
                self.engine.log(
                    "Block authoring is on the Block Editor's Dynamic panel (BPARAMETER, BACTION, BVSTATE, BVHIDE, BVSHOW); \
                     insert the block to test it, and change its values in Properties or with -DYNPROP.",
                )
            }
            "BATTORDER" => self.open_attribute_manager(),
            "DGNIMPORT" => self.dgn_import(),
            "DGNATTACH" => self.dgn_attach(),
            "DGNEXPORT" => self.dgn_export(),
            "DGNCLIP" => self.run("XCLIP"),
            "DGNADJUST" => self.engine.log("A DGN file is attached as a block: LAYER and XCLIP work on it."),
            "DWFATTACH" => self.dwf_attach(),
            "DWFCLIP" => self.run("XCLIP"),
            "DWFADJUST" | "DWFLAYERS" => self
                .engine
                .log("A DWFx sheet is attached as a block: LAYER and XCLIP work on it; its colors are its own."),
            "DWFFORMAT" => self.engine.log("DWF files are written as DWFx (XPS compatible)."),
            "OPENDWFMARKUP" => self.markup_import(),
            "MSLIDE" => self.make_slide(),
            "APPLOAD" => self.appload(),
            "VSLIDE" => self.view_slide_file(),
            "DRAWINGRECOVERY" | "DRM" => self.recovery_open = true,
            "DRAWINGRECOVERYHIDE" => self.recovery_open = false,
            "GETENV" => self.engine.log(
                std::env::vars()
                    .filter(|(k, _)| k.starts_with("OXIDRAFT"))
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            "TABLET" | "TOOLBAR" | "TASKBAR" | "MENU" | "GRAPHICSCONFIG" | "SECURITYOPTIONS" | "REINIT"
            | "UPDATETHUMBSNOW" | "BLOCKICON" | "PARTIALOPEN" | "OLELINKS" | "OLESCALE" | "PLOTSTYLE"
            | "CONVERTCTB" | "CONVERTPSTYLES" | "EXPORTSETTINGS" | "WMFOPTS" | "IMAGEQUALITY" | "TPNAVIGATE"
            | "TARGETPOINT" => self.engine.log(not_applicable(name)),
            "EXPORTTOAUTOCAD" | "AECTOACAD" => self.export(false),
            "SHEETSET" | "SSM" => self.toggle_sheet_set(),
            "SHEETSETHIDE" => self.sheet_set = None,
            "OPENSHEETSET" => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Sheet set", &["oxss"]).pick_file()
                    && let Err(e) = self.open_sheet_set(&path)
                {
                    self.engine.log(e);
                }
            }
            "NEWSHEETSET" => {
                if let Some(path) =
                    rfd::FileDialog::new().add_filter("Sheet set", &["oxss"]).set_file_name("Sheets.oxss").save_file()
                {
                    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Err(e) = self.new_sheet_set(&path, &name) {
                        self.engine.log(e);
                    }
                }
            }
            "ARCHIVE" => self.archive_command(),
            "SHARE" => self.share(),
            "MARKUP" | "MARKUPIMPORT" => self.markup_import(),
            "PUBLISH" => self.open_publish(),
            "UNITS" | "UN" => self.open_units(),
            "RENAME" | "REN" => self.open_rename(),
            "WBLOCK" | "W" => self.open_wblock(),
            "QUICKCALC" | "QC" => self.open_quickcalc(),
            "COLOR" | "COL" | "COLOUR" => self.open_color(Vec::new()),
            "LINETYPE" | "LT" | "LTYPE" => self.open_linetype_manager(),
            "LWEIGHT" | "LW" | "LINEWEIGHT" => self.open_lineweight_settings(),
            "STYLE" | "ST" | "DDSTYLE" => self.open_text_style(),
            "DSETTINGS" | "DS" | "SE" | "DDRMODES" => self.open_drafting_settings(0),
            "OSNAP" | "OS" | "DDOSNAP" => self.open_drafting_settings(2),
            "PTYPE" | "DDPTYPE" => self.open_point_style(),
            "TOLERANCE" | "TOL" => self.open_tolerance(),
            "FIELD" => self.open_field_dialog(false),
            "STATUS" => self.status(),
            "ARRAYCLASSIC" => self.open_array_dialog(),
            "CLASSICGROUP" => self.open_group_dialog(),
            "LAYOUTWIZARD" => self.open_layout_wizard(),
            "PASTESPEC" | "PA" => self.open_paste_special(),
            "SCALELISTEDIT" | "-SCALELISTEDIT" => self.open_scale_list(),
            "FILTER" | "FI" => self.open_quick_select(),
            "ADCENTER" | "ADC" | "DC" => self.toggle_design_center(),
            "TOOLPALETTES" | "TP" => self.toggle_tool_palettes(),
            "TOOLPALETTESCLOSE" => self.tool_palettes = None,
            "ADCCLOSE" => self.design_center = None,
            "ETRANSMIT" => self.etransmit(),
            "STYLESMANAGER" => self.open_styles_manager(),
            "UCSMAN" | "UC" => self.open_ucs_dialog(),
            "RSCRIPT" => self.rerun_script(),
            "PNGOUT" => self.image_out_command("PNG", &["png"]),
            "JPGOUT" => self.image_out_command("JPEG", &["jpg", "jpeg"]),
            "BMPOUT" => self.image_out_command("BMP", &["bmp"]),
            "TIME" => self.time(),
            "ATTACH" => self.attach(),
            "IMPORT" | "IMP" => self.import(),
            "PDFIMPORT" => self.pdf_import(),
            "ULAYERS" => self.open_underlay_layers(),
            "INSERTOBJ" | "IO" => self.insert_object(),
            "EXPORT" | "EXP" => self.export(false),
            "DXFOUT" => self.export(true),
            "DXFIN" => self.after_saving(crate::files::Then::Open),
            "QNEW" => self.after_saving(crate::files::Then::New),
            "SAVEALL" => self.save(false),
            "CLEANSCREENON" => self.clean_screen = true,
            "CLEANSCREENOFF" => self.clean_screen = false,
            "SETVAR" | "SET" => self.setvar(None),
            "SPELL" | "SP" => self.open_spell(),
            "DIMINSPECT" => self.open_inspect_dialog(),
            "MLEADERSTYLE" | "MLS" => self.open_leader_style(),
            "BATTMAN" => self.open_attribute_manager(),
            "LAYERSTATE" | "LAS" => self.open_layer_states(),
            "TABLESTYLE" | "TS" => self.open_table_style(),
            "SELECTALL" | "AI_SELALL" => self.engine.select_all(&self.doc),
            "COMPARE" => self.compare_command(),
            "DIMSTYLE" | "D" | "DST" => self.open_dim_style(),
            "FIND" => self.find_replace = Some(Default::default()),
            "COMPAREEXIT" => {
                self.comparison = None;
                self.xcomparison = None;
            }
            "FILETAB" => self.show_file_tabs = true,
            "FILETABCLOSE" => self.show_file_tabs = false,
            // Viewports and layouts are made on the sheets: out of the
            // viewport worked in first.
            "MVIEW" | "MV" | "LAYOUT" | "LO" => {
                self.leave_viewport();
                return false;
            }
            // A system variable typed on its own, unless a command has its name.
            other => match crate::sysvars::find(other).filter(|_| !oxidraft_commands::is_command(other)) {
                Some(var) => self.setvar(Some(var)),
                None => return false,
            },
        }
        true
    }

    /// Enter on the command line.
    pub(crate) fn submit(&mut self) {
        let mut text = std::mem::take(&mut self.command_text);
        self.recall = None;
        if self.lisp_typed(&text) || self.lisp_command(&text) {
            return;
        }
        if !self.engine.is_active() {
            text = text.trim().trim_start_matches(['_', '.']).to_string();
            // AutoComplete: Enter accepts the highlighted suggestion.
            // The user's own aliases first; then AutoComplete's pick.
            let expanded = self.engine.expand_alias(&text);
            if expanded != text.trim() {
                text = expanded;
            } else if !self.autocomplete_off
                && let Some(c) = Engine::suggest(&text).get(self.suggestion)
            {
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
        // What was typed may answer what a LISP program waits for.
        self.lisp_pump();
    }

    /// Opens what commands asked for. Returns true if there was anything.
    fn handle_ui_requests(&mut self, ctx: &egui::Context) -> bool {
        let requests = self.engine.take_ui_requests();
        let any = !requests.is_empty();
        for r in requests {
            match r {
                UiRequest::EditAttributes(id) => {
                    self.edit_attributes(id);
                }
                UiRequest::TextWindow => self.history_expanded = true,
                UiRequest::MaximizeViewport(id) => self.enter_viewport(id, true),
                UiRequest::EditTable(id) => self.open_table_editor(id),
                UiRequest::EditText(id) => self.open_text_editor(id),
                UiRequest::SetVar(name, value) => self.set_var(&name, &value),
                UiRequest::OpenXref(name) => self.open_xref(&name),
                UiRequest::CompareXref(name) => self.xcompare(&name),
                UiRequest::Tiling(tiling) => self.set_tiling(tiling),
                UiRequest::ChangeSpace(ids) => self.change_space(&ids),
                UiRequest::WmfOut(ids) => self.wmf_out(&ids),
                UiRequest::RunCommand(c) => self.run(&c),
                UiRequest::PasteHyperlink(ids) => self.paste_as_hyperlink(&ids),
                UiRequest::RestoreView(name) => self.go_to_view(&name),
                UiRequest::TableExport(id) => self.table_export(id),
                UiRequest::OpenUrl(url) => {
                    if let Err(e) = crate::more_app::open_url(&url) {
                        self.engine.log(format!("Could not open {url}: {e}"));
                    }
                }
                UiRequest::Workspace(option, name, new) => self.workspace_option(&option, &name, &new),
                UiRequest::ArrangeWindows(n) => {
                    let arrange = match n {
                        1 => crate::drawings::Arrange::Horizontal,
                        2 => crate::drawings::Arrange::Vertical,
                        _ => crate::drawings::Arrange::Cascade,
                    };
                    self.tab_request = Some(crate::drawings::TabRequest::Arrange(arrange));
                }
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
        any
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
            || self.image_attach_dialog.is_some()
            || self.delete_layout_prompt.is_some()
            || self.move_or_copy.is_some()
            || self.insert_table.is_some()
            || self.table_editor.is_some()
            || self.view_manager.is_some()
            || self.purge.is_some()
            || self.dim_style_dialog.is_some()
            || self.find_replace.is_some()
            || self.alias_dialog.is_some()
            || self.from_template.is_some()
            || self.cycling.is_some();
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
            && self.delete_layout_prompt.take().is_none()
            && self.move_or_copy.take().is_none()
            && self.insert_table.take().is_none()
            && self.table_editor.take().is_none()
            && self.view_manager.take().is_none()
            && self.purge.take().is_none()
            && self.dim_style_dialog.take().is_none()
            && self.find_replace.take().is_none()
            && self.alias_dialog.take().is_none()
            && self.from_template.take().is_none()
            && self.cycling.take().is_none()
        {
            self.image_attach_dialog = None;
        }
        true
    }

    /// True while the keyboard belongs to a palette or dialog text field.
    /// egui takes the keyboard from a field as soon as Esc is pressed, so
    /// the field that had it last frame still counts: that Esc is its.
    pub(crate) fn editing_field(&self, ctx: &egui::Context) -> bool {
        // Any text field but the command line's, a number being typed in
        // a dialog among them.
        let command_line = egui::Id::unique("command_line_input");
        let typing = ctx.memory(|m| m.focused()).is_some_and(|f| f != command_line) && ctx.text_edit_focused();
        typing
            || ctx
                .memory(|m| m.focused())
                .or(self.last_focus)
                .is_some_and(|f| self.prev_text_fields.contains(&f) || self.text_fields.contains(&f))
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Your own shortcuts (CUI) come first.
        self.custom_shortcuts(ctx);
        if self.slide.is_some() && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.slide = None;
        }
        let cmd = Modifiers::COMMAND;
        let none = Modifiers::NONE;
        // Space while dragging a lasso cycles Window, Crossing and Fence,
        // instead of reaching the command line.
        if let Some(crate::canvas::Drag::Lasso { mode, .. }) = &mut self.drag
            && ctx.input_mut(|i| {
                let pressed = i.consume_key(none, Key::Space);
                i.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == " "));
                pressed
            })
        {
            *mode = Some(mode.unwrap_or(crate::canvas::LassoMode::Window).next());
        }
        // Ctrl+Shift+A: group selection on or off.
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::A)) {
            self.toggle_group_selection();
        }
        // Ctrl+R: the next model viewport is current.
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::R)) {
            self.next_pane();
        }
        // Ctrl+W, as in AutoCAD (Cmd+W is left to the window).
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::W)) {
            self.selection_cycling = !self.selection_cycling;
            self.log_toggle("Selection cycling", self.selection_cycling);
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Num1)) {
            self.properties_open = !self.properties_open;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Num8)) {
            self.toggle_quickcalc();
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::K)) {
            self.run("HYPERLINK");
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Num2)) {
            self.toggle_design_center();
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Num3)) {
            self.toggle_tool_palettes();
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Num0)) {
            self.clean_screen = !self.clean_screen;
        }
        // The Plot dialog and its preview keep Esc and Enter to themselves.
        if self.plot_keys(ctx) {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::P)) {
            self.open_plot(false);
            return;
        }
        // Esc closes an open shortcut menu (or dropdown), and does nothing else.
        if egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(none, Key::Escape)) {
            egui::Popup::close_all(ctx);
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
        let (esc, f2, f3, f7, f8, f9, f10, f12) = ctx.input_mut(|i| {
            (
                i.consume_key(none, Key::Escape),
                i.consume_key(none, Key::F2),
                i.consume_key(none, Key::F3),
                i.consume_key(none, Key::F7),
                i.consume_key(none, Key::F8),
                i.consume_key(none, Key::F9),
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
        if f9 {
            self.engine.vars.snap = !self.engine.vars.snap;
            self.log_toggle("Snap", self.engine.vars.snap);
        }
        // F5 or Ctrl+E: the next isometric plane, while drafting isometrically.
        if ctx.input_mut(|i| i.consume_key(none, Key::F5) || i.consume_key(Modifiers::CTRL, Key::E))
            && let Some(plane) = self.engine.vars.isoplane
        {
            let next = plane.next();
            self.engine.vars.isoplane = Some(next);
            self.engine.log(format!("<Isoplane {}>", next.name()));
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
            self.run("OPEN");
        }
        if new {
            self.run("NEW");
        }
        // Ctrl+Tab and Ctrl+Shift+Tab: the next or previous drawing.
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab)) {
            self.tab_request = Some(crate::drawings::TabRequest::Next(true));
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Tab)) {
            self.tab_request = Some(crate::drawings::TabRequest::Next(false));
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
        // Lengths and angles everywhere show in the drawing's units.
        oxidraft_model::units::set_current(self.doc.units());
        // The text styles' fonts, for the drawing area and plot previews.
        let fonts = self.doc.text_styles().iter().map(|s| s.font.as_str());
        crate::fonts::load(&ctx, self.path.as_deref().and_then(std::path::Path::parent), fonts);
        self.prev_text_fields = std::mem::take(&mut self.text_fields);
        self.handle_shortcuts(&ctx);
        self.handle_clipboard_keys(&ctx);
        self.receive_pictures(&ctx);
        self.load_images_turned_on();
        self.image_out_frame(&ctx);
        self.update_title(&ctx);

        self.top_bar(ui);
        // Clean screen (Ctrl+0): the drawing, the command line and the status bar.
        if !self.clean_screen && !self.ribbon_hidden {
            self.ribbon(ui);
        }
        self.status_bar(ui);
        if self.show_file_tabs && !self.clean_screen {
            self.file_tabs(ui);
        }
        if !self.clean_screen {
            self.properties_palette(ui);
        }
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
        self.page_setup_dialog(&ctx);
        self.move_or_copy_dialog(&ctx);
        self.insert_table_dialog(&ctx);
        self.table_editor_dialog(&ctx);
        self.view_manager_dialog(&ctx);
        self.purge_dialog(&ctx);
        self.dim_style_dialog(&ctx);
        self.find_dialog(&ctx);
        self.compare_bar(&ctx);
        self.xcompare_bar(&ctx);
        self.alias_dialog(&ctx);
        self.layer_walk_dialog(&ctx);
        self.export_pdf_dialog(&ctx);
        self.plotter_manager_dialog(&ctx);
        self.cui_dialog(&ctx);
        self.sheet_set_palette(&ctx);
        self.quick_view_layouts(&ctx);
        self.drawing_recovery(&ctx);
        self.autosave_tick();
        self.finish_data_link();
        self.publish_dialog(&ctx);
        self.units_dialog(&ctx);
        self.rename_dialog(&ctx);
        self.wblock_dialog(&ctx);
        self.quickcalc_palette(&ctx);
        self.color_dialog(&ctx);
        self.linetype_manager_dialog(&ctx);
        self.lineweight_dialog(&ctx);
        self.text_style_dialog(&ctx);
        self.drafting_settings_dialog(&ctx);
        self.point_style_dialog(&ctx);
        self.tolerance_dialog(&ctx);
        self.text_editor(&ctx);
        self.field_dialog(&ctx);
        self.underlay_layers_dialog(&ctx);
        self.pdf_import_dialog(&ctx);
        self.array_dialog(&ctx);
        self.group_dialog(&ctx);
        self.layout_wizard(&ctx);
        self.paste_special_dialog(&ctx);
        self.scale_list_dialog(&ctx);
        self.tool_palettes_window(&ctx);
        self.design_center(&ctx);
        self.styles_manager(&ctx);
        self.pen_table_editor(&ctx);
        self.ucs_dialog(&ctx);
        self.spell_dialog(&ctx);
        self.inspect_dialog(&ctx);
        self.annotation_style_dialog(&ctx);
        self.attribute_manager_dialog(&ctx);
        self.layer_states_dialog(&ctx);
        self.drawing_properties_dialog(&ctx);
        self.save_prompt_dialog(&ctx);
        self.insert_layouts_dialog(&ctx);
        self.delete_layout_dialog(&ctx);
        self.block_edit_dialog(&ctx);
        self.block_close_dialog(&ctx);
        self.blocks_palette(&ctx);
        self.update_window(&ctx);
        // Not in an image of the drawing (PNGOUT).
        if !self.capturing_image() && !self.command_line_hidden {
            self.command_line(&ctx);
        }
        self.write_log();
        self.publish_copied(&ctx);
        // Requests from what was typed this frame show on the next.
        if self.handle_ui_requests(&ctx) {
            ctx.request_repaint();
        }
        // A LISP program goes on once the user has answered it.
        self.lisp_pump();
        if self.lisp.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
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

/// What a command with nothing to do in Oxidraft says, and what to use.
fn not_applicable(name: &str) -> String {
    let why = match name.trim().to_ascii_uppercase().as_str() {
        "TABLET" => "digitizing tablets aren't supported",
        "TOOLBAR" | "MENU" => "Oxidraft has the ribbon; CUI adds shortcuts and Quick Access Toolbar commands",
        "TASKBAR" => "each drawing has a file tab (Ctrl+Tab goes between them)",
        "GRAPHICSCONFIG" => "drawing is always hardware accelerated",
        "SECURITYOPTIONS" => "drawings aren't password protected",
        "REINIT" | "UPDATETHUMBSNOW" | "BLOCKICON" => "there's nothing to refresh",
        "PARTIALOPEN" => "OPEN reads the whole drawing",
        "OLELINKS" | "OLESCALE" => "INSERTOBJ links files rather than embedding OLE objects",
        "PLOTSTYLE" | "CONVERTCTB" | "CONVERTPSTYLES" => {
            "plot styles are color-dependent (.ctb); STYLESMANAGER edits them"
        }
        "EXPORTSETTINGS" | "WMFOPTS" => "EXPORTDWFX and WMFOUT need no settings",
        "IMAGEQUALITY" => "images always show at full quality",
        "TPNAVIGATE" => "TOOLPALETTES opens the palettes, with their tabs",
        "TARGETPOINT" => "there are no lights in 2D drafting",
        _ => "it has no use here",
    };
    format!("{}: {why}.", name.trim().to_ascii_uppercase())
}

//! The command engine.
//!
//! Every drafting command is a small state machine: it shows a prompt
//! ("Specify first point:"), takes an [`Input`] (a point, a number, a keyword,
//! Enter), edits the [`Document`] and either asks for more or finishes. The
//! [`Engine`] owns the active command, parses what the user types on the
//! command line, and wraps each command run in one undo transaction.

mod annotate;
mod arc;
mod array;
mod attribute;
mod block;
mod break_join;
mod chamfer;
mod circle;
mod clipboard;
mod common;
mod dims;
mod donut;
mod draworder;
mod edit;
mod ellipse;
mod engine;
mod erase;
mod explode;
mod fillet;
mod grip;
mod hatch;
mod input;
mod inquiry;
mod layer;
mod lengthen;
mod line;
mod list;
mod matchprop;
mod mirror;
mod move_copy;
mod offset;
mod pedit;
mod pline;
mod plot;
mod point;
mod polygon;
mod rectang;
mod refedit;
mod revcloud;
mod rotate;
mod scale;
mod select;
mod spline;
mod stretch;
mod trim_extend;
mod underlay;
mod xclip;
mod xline;
mod xref;
mod zoom;

pub use clipboard::{clip_of, paste};
pub use engine::{COMMANDS, CommandInfo, Engine, Tracked};
pub use input::{Input, parse_input};
pub use select::{QuickOperator, QuickProperty, QuickSelect, SimilarMode, color_label, object_type, select_similar};

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Document, EntityId, Geometry};

/// The result of a "Select objects:" prompt.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selection {
    pub ids: Vec<EntityId>,
    /// The selection windows used, in drawing units. STRETCH moves only the
    /// points inside them.
    pub windows: Vec<Aabb>,
}

/// What a command asks for next.
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub text: String,
    /// Options shown as `[Close/Undo]`. Typing the capital letters (`C`, `X`
    /// for `eXit`) or a prefix of the word selects one.
    pub keywords: Vec<&'static str>,
    /// What Enter does, shown as `<Exit>`.
    pub default: Option<String>,
}

impl Prompt {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), keywords: Vec::new(), default: None }
    }

    pub fn with_keywords(mut self, keywords: &[&'static str]) -> Self {
        self.keywords = keywords.to_vec();
        self
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }
}

impl std::fmt::Display for Prompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.text)?;
        if !self.keywords.is_empty() {
            write!(f, " or [{}]", self.keywords.join("/"))?;
        }
        if let Some(d) = &self.default {
            write!(f, " <{d}>")?;
        }
        write!(f, ":")
    }
}

/// A change of view a command asks for. The app owns the view and makes
/// the change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewRequest {
    /// The whole drawing (ZOOM Extents and All).
    Extents,
    /// Fit a region (ZOOM Window and Object).
    Window(Aabb),
    /// The view before the last change (ZOOM Previous).
    Previous,
    /// Magnify the current view by a factor (ZOOM nX, In, Out).
    Magnify(f64),
    /// Show the extents magnified by a factor (ZOOM n).
    OfExtents(f64),
    /// Center on a point, showing `height` drawing units top to bottom.
    Center { center: Point, height: f64 },
    /// Real-time zoom: drag up to zoom in, down to zoom out.
    RealtimeZoom,
    /// Real-time pan: drag to move the view.
    RealtimePan,
}

/// Something a command asks the app to show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiRequest {
    /// The Enhanced Attribute Editor for a block reference (EATTEDIT).
    EditAttributes(EntityId),
    /// The full command history, for long output like LIST's.
    TextWindow,
    /// Back to the Plot dialog with the window picked for it (`None`: as
    /// it was).
    PlotWindow(Option<Aabb>),
    /// REFCLOSE's Save for an xref: its file is to be written.
    SaveXrefEdit,
    /// COLORTHEME: 0 dark, 1 light.
    ColorTheme(u8),
}

/// Result of feeding one input to a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Continue,
    Done,
}

/// Settings that persist between commands, like AutoCAD system variables.
#[derive(Debug, Clone, PartialEq)]
pub struct Vars {
    /// OFFSETDIST: `None` means "Through" (offset through a picked point).
    pub offset_distance: Option<f64>,
    /// FILLETRAD.
    pub fillet_radius: f64,
    /// CHAMFERA and CHAMFERB: the first and second chamfer distances.
    pub chamfer_a: f64,
    pub chamfer_b: f64,
    /// INSNAME: the last block inserted.
    pub insert_name: Option<String>,
    /// TEXTSIZE: default height for new text.
    pub text_height: f64,
    /// DIMSCALE: overall scale of new dimensions and leaders.
    pub dim_scale: f64,
    /// HPNAME, HPSCALE, HPANG: the hatch pattern and its scale and angle.
    pub hatch_pattern: oxidraft_model::HatchPattern,
    pub hatch_scale: f64,
    pub hatch_angle: f64,
    /// SELECTSIMILARMODE.
    pub select_similar: SimilarMode,
    /// VIEWSIZE and VIEWCTR: the height and center of the view, kept up to
    /// date by the app.
    pub view_size: f64,
    pub view_center: Point,
    /// LAYISOMODE: true turns the other layers off; false locks and fades them.
    pub layiso_off: bool,
    /// The layers as they were before LAYISO, and the current layer, for LAYUNISO.
    pub layiso_saved: Option<(Vec<oxidraft_model::Layer>, String)>,
    /// PLINEWID: the width of new polylines.
    pub pline_width: f64,
    /// SPLMETHOD: true draws splines by control vertices, false through fit points.
    pub spline_cv: bool,
    /// POLYSIDES, and whether POLYGON last drew inscribed in its circle.
    pub polygon_sides: usize,
    pub polygon_inscribed: bool,
    /// DONUTID and DONUTOD: a donut's inside and outside diameters.
    pub donut_inside: f64,
    pub donut_outside: f64,
    /// REVCLOUD's arc length (`None` until set: a fraction of the view),
    /// and whether its arcs are drawn in the Calligraphy style.
    pub revcloud_arc: Option<f64>,
    pub revcloud_calligraphy: bool,
    /// REVCLOUDCREATEMODE: Polygonal (true) or Rectangular clouds.
    pub revcloud_polygonal: bool,
    /// COLORTHEME: 0 dark, 1 light. The app keeps it; it's here for the prompt.
    pub color_theme: u8,
}

impl Default for Vars {
    fn default() -> Self {
        Self {
            offset_distance: None,
            fillet_radius: 0.0,
            chamfer_a: 0.0,
            chamfer_b: 0.0,
            insert_name: None,
            text_height: 2.5,
            dim_scale: 1.0,
            hatch_pattern: oxidraft_model::HatchPattern::Ansi31,
            hatch_scale: 1.0,
            hatch_angle: 0.0,
            select_similar: SimilarMode::default(),
            view_size: 100.0,
            view_center: Point::ORIGIN,
            layiso_off: false,
            layiso_saved: None,
            pline_width: 0.0,
            spline_cv: false,
            polygon_sides: 4,
            polygon_inscribed: true,
            donut_inside: 0.5,
            donut_outside: 1.0,
            revcloud_arc: None,
            revcloud_calligraphy: false,
            revcloud_polygonal: false,
            color_theme: 0,
        }
    }
}

/// What a command can see and touch while handling input.
pub struct Ctx<'a> {
    pub doc: &'a mut Document,
    pub vars: &'a mut Vars,
    /// Current cursor position (already ortho-constrained), for direct distance entry.
    pub cursor: Point,
    /// Pick radius in drawing units for selecting objects.
    pub pick_tolerance: f64,
    /// Lines to print in the command history.
    pub messages: Vec<String>,
    /// A change of view for the app to make.
    pub view: Option<ViewRequest>,
    /// Objects to leave selected when the command ends (SELECTSIMILAR).
    pub select: Option<Vec<EntityId>>,
    /// A dialog or window for the app to open.
    pub ui: Option<UiRequest>,
    /// What COPYCLIP and friends copied, for the clipboard.
    pub copied: Option<Document>,
}

pub trait Command {
    /// Called once when the command starts, with the current settings.
    /// Returns a line to print before the first prompt.
    fn start(&mut self, _vars: &Vars) -> Option<String> {
        None
    }

    /// Called once after [`Command::start`], for commands whose prompts
    /// show something stored in the drawing. Returns a line to print.
    fn prepare(&mut self, _doc: &Document) -> Option<String> {
        None
    }

    /// Checked after [`Command::prepare`]: true if there's nothing to do,
    /// so the command ends without prompting.
    fn is_finished(&self) -> bool {
        false
    }

    /// For commands that don't ask to select objects: the objects selected
    /// before the command started (ZOOM's Object option uses them).
    fn preselected(&mut self, _ids: &[EntityId]) {}

    fn prompt(&self) -> Prompt;

    /// Handle one input. `Err` prints the message and re-prompts.
    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String>;

    /// Rubber-band geometry to draw while the cursor moves.
    fn preview(&self, _cursor: Point) -> Vec<Geometry> {
        Vec::new()
    }

    /// Point that ortho mode and relative (`@`) input measure from.
    fn base_point(&self) -> Option<Point> {
        None
    }

    /// Entities to draw highlighted besides the engine's selection set.
    fn highlighted(&self) -> Vec<EntityId> {
        Vec::new()
    }

    /// True while the command is at a "Select objects:" prompt. The engine
    /// then handles picks and windows itself and sends [`Input::Selection`]
    /// on Enter.
    fn wants_selection(&self) -> bool {
        false
    }

    /// For selection prompts that take one object: the first pick (or
    /// window) ends the selection without Enter.
    fn single_selection(&self) -> bool {
        false
    }

    /// True at a "Select object to …:" prompt that takes one object. Clicks
    /// then arrive as [`Input::Pick`] at the bare cursor, without snaps.
    fn wants_pick(&self) -> bool {
        false
    }

    /// True at a prompt that takes free text, like "Enter block name:". Typed
    /// input then arrives as [`Input::Text`] instead of being parsed as
    /// coordinates or keywords.
    fn wants_text(&self) -> bool {
        false
    }

    /// For text prompts: true if spaces are part of the text (MTEXT contents)
    /// rather than acting as Enter (block names).
    fn text_with_spaces(&self) -> bool {
        false
    }
}

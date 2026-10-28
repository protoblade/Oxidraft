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
mod block;
mod break_join;
mod chamfer;
mod circle;
mod common;
mod dims;
mod edit;
mod engine;
mod erase;
mod explode;
mod fillet;
mod grip;
mod hatch;
mod input;
mod inquiry;
mod lengthen;
mod line;
mod matchprop;
mod mirror;
mod move_copy;
mod offset;
mod pline;
mod rectang;
mod rotate;
mod scale;
mod stretch;
mod trim_extend;

pub use engine::{COMMANDS, CommandInfo, Engine, Tracked};
pub use input::{Input, parse_input};

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
}

pub trait Command {
    /// Called once when the command starts, with the current settings.
    /// Returns a line to print before the first prompt.
    fn start(&mut self, _vars: &Vars) -> Option<String> {
        None
    }

    /// Called once after [`Command::start`], for commands whose prompts
    /// show something stored in the drawing.
    fn prepare(&mut self, _doc: &Document) {}

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

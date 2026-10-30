//! What AutoLISP's getpoint, getdist, getkword, entsel, ssget and the like
//! ask for: a prompt that waits for the user and hands back the answer.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Mutex;

use oxidraft_geom::Point;
use oxidraft_model::{EntityId, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AskKind {
    Point,
    Corner,
    Dist,
    Angle,
    Real,
    Int,
    Str { spaces: bool },
    Kword,
    EntSel,
    SsGet,
}

/// One question, with what INITGET set for it.
#[derive(Debug, Clone)]
pub struct Ask {
    pub kind: AskKind,
    pub prompt: String,
    pub base: Option<Point>,
    pub keywords: Vec<String>,
    /// INITGET 1: Enter isn't an answer.
    pub no_null: bool,
    /// INITGET 2 and 4.
    pub no_zero: bool,
    pub no_negative: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Point(Point),
    Real(f64),
    Int(i64),
    Text(String),
    Keyword(String),
    Entity(EntityId, Point),
    Selection(Vec<EntityId>),
    /// Enter, or a pick that missed.
    Nothing,
    Cancelled,
}

/// Where the answer lands; [`Answer::Cancelled`] if the prompt is
/// cancelled first.
pub type Slot = Rc<RefCell<Option<Answer>>>;

/// Keywords as prompts need them, kept for the rest of the run.
fn keyword(k: &str) -> &'static str {
    static KEPT: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let set = kept.get_or_insert_with(HashSet::new);
    match set.get(k) {
        Some(s) => s,
        None => {
            let s: &'static str = Box::leak(k.to_string().into_boxed_str());
            set.insert(s);
            s
        }
    }
}

pub struct LispAsk {
    ask: Ask,
    slot: Slot,
    keywords: Vec<&'static str>,
    /// getdist and getangle with no base: the first point.
    first: Option<Point>,
}

impl LispAsk {
    pub fn new(ask: Ask, slot: Slot) -> Self {
        let keywords = ask.keywords.iter().map(|k| keyword(k)).collect();
        Self { ask, slot, keywords, first: None }
    }

    fn base(&self) -> Option<Point> {
        self.ask.base.or(self.first)
    }

    fn answer(&mut self, a: Answer) -> Result<Step, String> {
        *self.slot.borrow_mut() = Some(a);
        Ok(Step::Done)
    }

    fn number(&mut self, v: f64) -> Result<Step, String> {
        if self.ask.no_zero && v == 0.0 {
            return Err("Value must be nonzero.".into());
        }
        if self.ask.no_negative && v < 0.0 {
            return Err("Value must be positive.".into());
        }
        match self.ask.kind {
            AskKind::Int if v.fract() != 0.0 => Err("Requires an integer value.".into()),
            AskKind::Int => self.answer(Answer::Int(v as i64)),
            AskKind::Angle => self.answer(Answer::Real(v.to_radians())),
            _ => self.answer(Answer::Real(v)),
        }
    }
}

impl Drop for LispAsk {
    fn drop(&mut self) {
        let mut slot = self.slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Answer::Cancelled);
        }
    }
}

impl Command for LispAsk {
    fn prompt(&self) -> Prompt {
        // The engine adds the colon.
        let given = self.ask.prompt.trim().trim_end_matches(':').trim_end();
        let text = if self.ask.kind != AskKind::SsGet && !given.is_empty() {
            // The options the program wrote show once, from the keywords.
            match (given.find('['), given.find(']')) {
                (Some(a), Some(b)) if a < b && !self.keywords.is_empty() => {
                    format!("{} {}", given[..a].trim_end(), given[b + 1..].trim_start()).trim().to_string()
                }
                _ => given.to_string(),
            }
        } else {
            match (self.ask.kind, self.first) {
                (AskKind::Dist | AskKind::Angle, Some(_)) => "Specify second point",
                (AskKind::Point | AskKind::Corner | AskKind::Dist | AskKind::Angle, _) => "Specify point",
                (AskKind::EntSel, _) => "Select object",
                (AskKind::SsGet, _) => "Select objects",
                _ => "Enter a value",
            }
            .to_string()
        };
        Prompt::new(text).with_keywords(&self.keywords)
    }

    fn base_point(&self) -> Option<Point> {
        self.base()
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Some(b) = self.base() else { return Vec::new() };
        match self.ask.kind {
            AskKind::Corner => {
                let c = [b, Point::new(cursor.x, b.y), cursor, Point::new(b.x, cursor.y)];
                (0..4).map(|i| Geometry::Line { start: c[i], end: c[(i + 1) % 4] }).collect()
            }
            AskKind::Point | AskKind::Dist | AskKind::Angle => vec![Geometry::Line { start: b, end: cursor }],
            _ => Vec::new(),
        }
    }

    fn wants_selection(&self) -> bool {
        self.ask.kind == AskKind::SsGet
    }

    fn wants_pick(&self) -> bool {
        self.ask.kind == AskKind::EntSel
    }

    fn wants_text(&self) -> bool {
        matches!(self.ask.kind, AskKind::Str { .. })
    }

    fn text_with_spaces(&self) -> bool {
        matches!(self.ask.kind, AskKind::Str { spaces: true })
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Enter if self.ask.no_null => Err("Requires a value.".into()),
            Input::Enter if matches!(self.ask.kind, AskKind::Str { .. }) => self.answer(Answer::Text(String::new())),
            Input::Enter => self.answer(Answer::Nothing),
            Input::Keyword(k) => self.answer(Answer::Keyword(k.to_string())),
            Input::Text(t) => self.answer(Answer::Text(t)),
            Input::Selection(s) => self.answer(Answer::Selection(s.ids)),
            Input::Pick { at, .. } => match ctx.pick(at) {
                Some(id) => self.answer(Answer::Entity(id, at)),
                None => {
                    ctx.messages.push("Missed.".into());
                    self.answer(Answer::Nothing)
                }
            },
            Input::Value(v) => match self.ask.kind {
                AskKind::Dist | AskKind::Angle | AskKind::Real | AskKind::Int => self.number(v),
                _ => Err("Requires a point.".into()),
            },
            Input::Point(p) => match self.ask.kind {
                AskKind::Point | AskKind::Corner => self.answer(Answer::Point(p)),
                AskKind::Dist | AskKind::Angle => match self.base() {
                    Some(b) if self.ask.kind == AskKind::Dist => self.number(b.distance(p)),
                    Some(b) => {
                        let a = (p.y - b.y).atan2(p.x - b.x).rem_euclid(std::f64::consts::TAU);
                        self.answer(Answer::Real(a))
                    }
                    None => {
                        self.first = Some(p);
                        Ok(Step::Continue)
                    }
                },
                AskKind::EntSel => match ctx.pick(p) {
                    Some(id) => self.answer(Answer::Entity(id, p)),
                    None => self.answer(Answer::Nothing),
                },
                _ => Err("Requires numeric value.".into()),
            },
        }
    }
}

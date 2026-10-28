//! DRAWORDER, TEXTTOFRONT and HATCHTOBACK: which objects draw over which.

use oxidraft_model::{EntityId, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step};

const OPTIONS: &[&str] = &["Above objects", "Under objects", "Front", "Back"];

#[derive(Clone, PartialEq)]
enum Stage {
    Select,
    Option(Vec<EntityId>),
    Reference { ids: Vec<EntityId>, above: bool },
}

/// DRAWORDER: moves the selected objects to the front or back, or just
/// above or under other objects.
pub struct DrawOrder {
    stage: Stage,
    /// Started as one of the ribbon's or menu's buttons: Front, Back, Above
    /// or Under, so the option isn't asked.
    preset: Option<&'static str>,
}

impl DrawOrder {
    pub fn new(preset: Option<&'static str>) -> Self {
        Self { stage: Stage::Select, preset }
    }

    fn apply(&mut self, ids: Vec<EntityId>, option: &str, ctx: &mut Ctx) -> Step {
        match option {
            "Front" => ctx.doc.bring_to_front(&ids),
            "Back" => ctx.doc.send_to_back(&ids),
            other => {
                self.stage = Stage::Reference { ids, above: other == "Above objects" };
                return Step::Continue;
            }
        }
        Step::Done
    }
}

impl Command for DrawOrder {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select objects"),
            Stage::Option(_) => Prompt::new("Enter object ordering option").with_keywords(OPTIONS).with_default("Back"),
            Stage::Reference { .. } => Prompt::new("Select reference objects"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (Stage::Select, Input::Selection(s)) if s.ids.is_empty() => Ok(Step::Done),
            (Stage::Select, Input::Selection(s)) => match self.preset {
                Some(option) => Ok(self.apply(s.ids, option, ctx)),
                None => {
                    self.stage = Stage::Option(s.ids);
                    Ok(Step::Continue)
                }
            },
            (Stage::Option(ids), Input::Keyword(k)) => Ok(self.apply(ids, k, ctx)),
            (Stage::Option(ids), Input::Enter) => Ok(self.apply(ids, "Back", ctx)),
            (Stage::Reference { ids, above }, Input::Selection(s)) => {
                let reference: Vec<EntityId> = s.ids.into_iter().filter(|id| !ids.contains(id)).collect();
                if reference.is_empty() {
                    return Ok(Step::Done);
                }
                if above {
                    ctx.doc.place_above(&ids, &reference);
                } else {
                    ctx.doc.place_under(&ids, &reference);
                }
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            (Stage::Option(_), _) => Err("Choose an option, or press Enter for Back.".into()),
            _ => Err("Select objects, then press Enter.".into()),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match &self.stage {
            Stage::Option(ids) | Stage::Reference { ids, .. } => ids.clone(),
            Stage::Select => Vec::new(),
        }
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, Stage::Select | Stage::Reference { .. })
    }
}

/// TEXTTOFRONT: text, dimensions and leaders over everything else.
#[derive(Default)]
pub struct TextToFront;

impl Command for TextToFront {
    fn prompt(&self) -> Prompt {
        Prompt::new("Bring to front").with_keywords(&["Text", "Dimensions", "Leaders", "All"]).with_default("All")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let which = match input {
            Input::Keyword(k) => k,
            Input::Enter => "All",
            _ => return Err("Choose Text, Dimensions, Leaders or All.".into()),
        };
        let wanted = |g: &Geometry| match g {
            Geometry::Text { .. } => matches!(which, "Text" | "All"),
            Geometry::Dimension { .. } | Geometry::RadialDimension { .. } | Geometry::AngularDimension { .. } => {
                matches!(which, "Dimensions" | "All")
            }
            Geometry::Leader { .. } => matches!(which, "Leaders" | "All"),
            _ => false,
        };
        let ids: Vec<EntityId> = ctx.doc.entities().filter(|e| wanted(&e.geometry)).map(|e| e.id).collect();
        ctx.doc.bring_to_front(&ids);
        ctx.messages.push(format!("{} object(s) brought to front.", ids.len()));
        Ok(Step::Done)
    }
}

/// HATCHTOBACK: every hatch behind everything else. Returns the message.
pub(crate) fn hatch_to_back(doc: &mut oxidraft_model::Document) -> String {
    let ids: Vec<EntityId> =
        doc.entities().filter(|e| matches!(e.geometry, Geometry::Hatch { .. })).map(|e| e.id).collect();
    doc.send_to_back(&ids);
    format!("{} hatch object(s) sent to back.", ids.len())
}

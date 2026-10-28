use oxidraft_geom::Point;
use oxidraft_model::{Entity, Geometry};

use crate::{
    Command, Ctx, Input, Prompt, Step,
    common::{SELECT, apply, selected_entities},
};

enum Stage {
    Select,
    First,
    Second(Point),
    EraseSource(Point, Point),
}

/// MIRROR: select, two points on the mirror line, keep or erase the source.
pub struct Mirror {
    stage: Stage,
    selected: Vec<Entity>,
}

impl Default for Mirror {
    fn default() -> Self {
        Self { stage: Stage::Select, selected: Vec::new() }
    }
}

impl Command for Mirror {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select objects"),
            Stage::First => Prompt::new("Specify first point of mirror line"),
            Stage::Second(_) => Prompt::new("Specify second point of mirror line"),
            Stage::EraseSource(..) => {
                Prompt::new("Erase source objects?").with_keywords(&["Yes", "No"]).with_default("No")
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (Stage::Select, Input::Selection(s)) => {
                self.selected = selected_entities(&s, ctx);
                self.stage = Stage::First;
                Ok(if self.selected.is_empty() { Step::Done } else { Step::Continue })
            }
            (Stage::Select, _) => Err(SELECT.into()),
            (Stage::First, Input::Point(p)) => {
                self.stage = Stage::Second(p);
                Ok(Step::Continue)
            }
            (Stage::Second(a), Input::Point(b)) => {
                if a.approx_eq(b) {
                    return Err("The mirror line needs two different points.".into());
                }
                self.stage = Stage::EraseSource(*a, b);
                Ok(Step::Continue)
            }
            (Stage::EraseSource(a, b), input @ (Input::Keyword(_) | Input::Enter)) => {
                let (a, b) = (*a, *b);
                let erase = input == Input::Keyword("Yes");
                apply(&self.selected, !erase, ctx, |g| g.mirrored(a, b));
                Ok(Step::Done)
            }
            (Stage::First | Stage::Second(_), Input::Enter) => Ok(Step::Done),
            (Stage::EraseSource(..), _) => Err("Enter Y or N.".into()),
            _ => Err("Point required.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Stage::Second(a) = self.stage else { return Vec::new() };
        if a.approx_eq(cursor) {
            return Vec::new();
        }
        let mut out: Vec<Geometry> = self.selected.iter().map(|e| e.geometry.mirrored(a, cursor)).collect();
        out.push(Geometry::Line { start: a, end: cursor });
        out
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::Second(a) => Some(a),
            _ => None,
        }
    }

    fn highlighted(&self) -> Vec<oxidraft_model::EntityId> {
        self.selected.iter().map(|e| e.id).collect()
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, Stage::Select)
    }
}

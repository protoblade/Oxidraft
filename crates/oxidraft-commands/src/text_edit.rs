//! SCALETEXT and JUSTIFYTEXT: text's height, and the point it hangs
//! from, changed for many at once.

use oxidraft_geom::Point;
use oxidraft_model::{Attach, EntityId, Geometry, annotation::text_box};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

/// Text (and attribute definitions) among `ids`.
fn texts(ctx: &Ctx, ids: &[EntityId]) -> Vec<EntityId> {
    ids.iter()
        .copied()
        .filter(|id| {
            matches!(
                ctx.doc.entity(*id).map(|e| &e.geometry),
                Some(Geometry::Text { .. } | Geometry::AttributeDefinition { .. })
            )
        })
        .collect()
}

#[derive(Clone, PartialEq)]
enum ScaleStage {
    Select,
    Base,
    Height,
    Factor,
}

/// SCALETEXT: text to a new height, or scaled by a factor, each about its
/// own insertion point (Existing).
pub struct ScaleText {
    stage: ScaleStage,
    ids: Vec<EntityId>,
    height: f64,
}

impl Default for ScaleText {
    fn default() -> Self {
        Self { stage: ScaleStage::Select, ids: Vec::new(), height: 2.5 }
    }
}

impl ScaleText {
    fn apply(&self, ctx: &mut Ctx, f: impl Fn(f64) -> f64) -> Step {
        for id in &self.ids {
            ctx.doc.update(*id, |e| match &mut e.geometry {
                Geometry::Text { height, .. } | Geometry::AttributeDefinition { height, .. } => *height = f(*height),
                _ => {}
            });
        }
        ctx.messages.push(format!("{} object(s) changed.", self.ids.len()));
        Step::Done
    }
}

impl Command for ScaleText {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.height = vars.text_height;
        None
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            ScaleStage::Select => Prompt::new("Select objects"),
            ScaleStage::Base => Prompt::new("Enter a base point option for scaling")
                .with_keywords(&["Existing"])
                .with_default("Existing"),
            ScaleStage::Height => Prompt::new("Specify new model height")
                .with_keywords(&["Scale factor"])
                .with_default(format!("{:.4}", self.height)),
            ScaleStage::Factor => Prompt::new("Specify scale factor").with_default("2"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (ScaleStage::Select, Input::Selection(s)) => {
                self.ids = texts(ctx, &s.ids);
                if self.ids.is_empty() {
                    return Ok(Step::Done);
                }
                self.stage = ScaleStage::Base;
            }
            (ScaleStage::Base, _) => self.stage = ScaleStage::Height,
            (ScaleStage::Height, Input::Keyword(_)) => self.stage = ScaleStage::Factor,
            (ScaleStage::Height, Input::Value(h)) if h > 0.0 => {
                ctx.vars.text_height = h;
                return Ok(self.apply(ctx, |_| h));
            }
            (ScaleStage::Height, Input::Enter) => {
                let h = self.height;
                return Ok(self.apply(ctx, |_| h));
            }
            (ScaleStage::Factor, Input::Value(k)) if k > 0.0 => return Ok(self.apply(ctx, |h| h * k)),
            (ScaleStage::Factor, Input::Enter) => return Ok(self.apply(ctx, |h| h * 2.0)),
            (ScaleStage::Select, _) => return Err("Select objects, then press Enter.".into()),
            _ => return Err("Requires a positive number.".into()),
        }
        Ok(Step::Continue)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.ids.clone()
    }

    fn wants_selection(&self) -> bool {
        self.stage == ScaleStage::Select
    }
}

const JUSTIFY: &[(&str, Attach)] = &[
    ("Left", Attach::BottomLeft),
    ("Center", Attach::BottomCenter),
    ("Right", Attach::BottomRight),
    ("TL", Attach::TopLeft),
    ("TC", Attach::TopCenter),
    ("TR", Attach::TopRight),
    ("ML", Attach::MiddleLeft),
    ("MC", Attach::MiddleCenter),
    ("MR", Attach::MiddleRight),
    ("BL", Attach::BottomLeft),
    ("BC", Attach::BottomCenter),
    ("BR", Attach::BottomRight),
];

/// JUSTIFYTEXT: text hangs from another of its points, without moving.
#[derive(Default)]
pub struct JustifyText {
    ids: Option<Vec<EntityId>>,
}

/// `g` hung from `attach` instead, its box where it was.
fn justified(g: &Geometry, to: Attach) -> Geometry {
    let mut g = g.clone();
    if let Geometry::Text { position, height, rotation, width, attach, content, look } = &mut g {
        let now = text_box(*position, *height, *rotation, *width, *attach, content, look);
        let at_origin = text_box(Point::ORIGIN, *height, *rotation, *width, to, content, look);
        *position = now[0] - at_origin[0];
        *attach = to;
    }
    g
}

impl Command for JustifyText {
    fn prompt(&self) -> Prompt {
        match self.ids {
            None => Prompt::new("Select objects"),
            Some(_) => Prompt::new("Enter a justification option")
                .with_keywords(&["Left", "Center", "Right", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"])
                .with_default("Existing"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.ids, input) {
            (None, Input::Selection(s)) => {
                let ids = texts(ctx, &s.ids);
                if ids.is_empty() {
                    return Ok(Step::Done);
                }
                self.ids = Some(ids);
                Ok(Step::Continue)
            }
            (Some(ids), Input::Keyword(k)) => {
                let to = JUSTIFY.iter().find(|j| j.0 == k).map(|j| j.1).ok_or("Choose a justification.")?;
                for id in ids.clone() {
                    if let Some(g) = ctx.doc.entity(id).map(|e| justified(&e.geometry, to)) {
                        ctx.doc.modify(id, g);
                    }
                }
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err("Select objects, then a justification.".into()),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.ids.clone().unwrap_or_default()
    }

    fn wants_selection(&self) -> bool {
        self.ids.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use oxidraft_model::Document;

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn scaletext_and_justifytext() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let t = d.add(Geometry::Text {
            look: Default::default(),
            position: Point::new(10.0, 10.0),
            height: 2.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "ROOM".into(),
        });
        e.select_all(&d);
        run(&mut e, &mut d, &["scaletext", "", "s", "3"]);
        let before = d.entity(t).unwrap().geometry.clone();
        let Geometry::Text { height, .. } = before else { panic!() };
        assert_eq!(height, 6.0);
        e.select_all(&d);
        run(&mut e, &mut d, &["justifytext", "mc"]);
        let after = d.entity(t).unwrap().geometry.clone();
        let Geometry::Text { attach, position, height, rotation, width, content, .. } = &after else { panic!() };
        assert_eq!(*attach, Attach::MiddleCenter);
        // The text didn't move: its box is where it was.
        let Geometry::Text { position: p0, attach: a0, .. } = before else { panic!() };
        let (old, new) = (
            text_box(p0, 6.0, 0.0, 0.0, a0, "ROOM", &Default::default()),
            text_box(*position, *height, *rotation, *width, *attach, content, &Default::default()),
        );
        assert!(old.iter().zip(&new).all(|(a, b)| a.distance(*b) < 1e-9));
    }
}

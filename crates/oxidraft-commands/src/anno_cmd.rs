//! OBJECTSCALE (-OBJECTSCALE): annotation scales added to annotative
//! objects or taken from them; ANNOUPDATE: annotative objects sized for
//! the current scale again.

use oxidraft_model::EntityId;
use oxidraft_model::annoscale::{parse_scale, scale_name, supports};

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Select,
    Option,
    Add,
    Delete,
}

/// OBJECTSCALE: the selected annotative objects' scales, added or deleted
/// by name (? lists them).
pub struct ObjectScale {
    stage: Stage,
    ids: Vec<EntityId>,
}

impl Default for ObjectScale {
    fn default() -> Self {
        Self { stage: Stage::Select, ids: Vec::new() }
    }
}

impl ObjectScale {
    fn list(&self, ctx: &mut Ctx) {
        for id in &self.ids {
            let Some(a) = ctx.doc.entity(*id).and_then(|e| e.annotative.as_deref()) else { continue };
            let names: Vec<String> = a.scales.iter().map(|s| scale_name(*s)).collect();
            ctx.messages.push(format!("Supported scales: {}", names.join(", ")));
        }
    }
}

impl Command for ObjectScale {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select annotative objects"),
            Stage::Option => Prompt::new("Enter an option").with_keywords(&["Add", "Delete", "?"]).with_default("Add"),
            Stage::Add => Prompt::new("Enter named scale to add or [?]"),
            Stage::Delete => Prompt::new("Enter named scale to delete or [?]"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::Select, Input::Selection(s)) => {
                self.ids = s
                    .ids
                    .into_iter()
                    .filter(|id| ctx.doc.entity(*id).is_some_and(|e| e.annotative.is_some()))
                    .collect();
                if self.ids.is_empty() {
                    ctx.messages.push("No annotative objects selected.".into());
                    return Ok(Step::Done);
                }
                ctx.messages.push(format!("{} annotative object(s) selected.", self.ids.len()));
                self.stage = Stage::Option;
            }
            (Stage::Option, Input::Keyword("?")) => self.list(ctx),
            (Stage::Option, Input::Keyword("Delete")) => self.stage = Stage::Delete,
            (Stage::Option, Input::Keyword(_) | Input::Enter) => self.stage = Stage::Add,
            (Stage::Add | Stage::Delete, Input::Text(t)) if t.trim() == "?" => self.list(ctx),
            (stage @ (Stage::Add | Stage::Delete), Input::Text(t)) => {
                let scale = parse_scale(&t).ok_or("Enter a scale like 1:50.")?;
                let current = ctx.doc.anno_scale();
                let mut n = 0;
                for id in &self.ids {
                    let mut kept_last = false;
                    ctx.doc.update(*id, |e| {
                        let Some(a) = e.annotative.as_mut() else { return };
                        if stage == Stage::Add && !supports(&a.scales, scale) {
                            a.scales.push(scale);
                            n += 1;
                        } else if stage == Stage::Delete && supports(&a.scales, scale) {
                            if a.scales.len() == 1 {
                                kept_last = true;
                                return;
                            }
                            a.scales.retain(|s| (s - scale).abs() > 1e-9 * scale.max(1.0));
                            n += 1;
                        }
                        // Sized for the current scale once it supports it.
                        if supports(&a.scales, current) && (a.shown - current).abs() > 1e-12 {
                            e.geometry = e.geometry.annotation_scaled(current / a.shown);
                            a.shown = current;
                        }
                    });
                    if kept_last {
                        ctx.messages.push("An annotative object keeps at least one scale.".into());
                    }
                }
                let done = if stage == Stage::Add { "added to" } else { "deleted from" };
                ctx.messages.push(format!("{} {done} {n} object(s).", scale_name(scale)));
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        self.stage == Stage::Select
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, Stage::Add | Stage::Delete)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.ids.clone()
    }
}

/// ANNOUPDATE: the selected annotative objects sized for the current
/// annotation scale again.
#[derive(Default)]
pub struct AnnoUpdate;

impl Command for AnnoUpdate {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Ok(Step::Done) };
        let current = ctx.doc.anno_scale();
        let mut n = 0;
        for id in &s.ids {
            ctx.doc.update(*id, |e| {
                let Some(a) = e.annotative.as_mut() else { return };
                if supports(&a.scales, current) && (a.shown - current).abs() > 1e-12 {
                    e.geometry = e.geometry.annotation_scaled(current / a.shown);
                    a.shown = current;
                    n += 1;
                }
            });
        }
        ctx.messages.push(format!("{n} object(s) updated."));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

/// ANNORESET: every scale's representation of the annotative objects
/// back where the current scale's is (grip editing moves one on its own).
#[derive(Default)]
pub struct AnnoReset;

impl Command for AnnoReset {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select annotative objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Ok(Step::Done) };
        let n = s.ids.iter().filter(|id| ctx.doc.anno_reset(**id)).count();
        ctx.messages.push(format!("{n} object(s) reset."));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use oxidraft_geom::Point;
    use oxidraft_model::{Attach, Document, Geometry};

    use crate::Engine;

    #[test]
    fn objectscale_adds_and_deletes() {
        let mut d = Document::new();
        let id = d.add(Geometry::Text {
            look: Default::default(),
            position: Point::ORIGIN,
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "A".into(),
        });
        d.set_annotative(id, true);
        d.set_anno_scale(100.0, false);
        let mut e = Engine::new();
        for t in ["objectscale", "all", "", "", "1:100", "", "objectscale", "all", "", "d", "1:1", ""] {
            e.submit(t, &mut d, Point::ORIGIN, 0.5);
        }
        assert!(!e.is_active());
        let a = d.entity(id).unwrap().annotative.clone().unwrap();
        assert_eq!(a.scales, vec![100.0]);
        assert!(
            matches!(d.entity(id).unwrap().geometry, Geometry::Text { height, .. } if (height - 250.0).abs() < 1e-9)
        );
    }
}

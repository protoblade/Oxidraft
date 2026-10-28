use oxidraft_geom::Point;
use oxidraft_model::{EntityId, Geometry, snap};

use crate::{Command, Ctx, Input, Prompt, Step, Vars, edit::offset_toward};

enum Stage {
    Distance,
    Select,
    Side { id: EntityId, geometry: Geometry },
}

/// OFFSET: parallel copies of lines, arcs, circles and polylines, at a
/// distance (remembered in OFFSETDIST) or through a point.
pub struct Offset {
    stage: Stage,
    /// `None` = Through.
    distance: Option<f64>,
    /// Objects created by each offset, for the Undo option.
    history: Vec<usize>,
}

impl Default for Offset {
    fn default() -> Self {
        Self { stage: Stage::Distance, distance: None, history: Vec::new() }
    }
}

impl Offset {
    fn results(&self, g: &Geometry, side: Point) -> Vec<Geometry> {
        let d = self.distance.unwrap_or_else(|| g.distance_to(side));
        offset_toward(g, d, side)
    }

    fn undo(&mut self, ctx: &mut Ctx) {
        match self.history.pop() {
            Some(n) => {
                for _ in 0..n {
                    ctx.doc.undo_last_in_transaction();
                }
            }
            None => ctx.messages.push("Nothing to undo.".into()),
        }
        self.stage = Stage::Select;
    }
}

impl Command for Offset {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.distance = vars.offset_distance;
        Some("Current settings: Erase source=No  Layer=Source  OFFSETGAPTYPE=0".into())
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Distance => Prompt::new("Specify offset distance")
                .with_keywords(&["Through"])
                .with_default(self.distance.map_or("Through".into(), |d| format!("{d:.4}"))),
            Stage::Select => {
                Prompt::new("Select object to offset").with_keywords(&["eXit", "Undo"]).with_default("eXit")
            }
            Stage::Side { .. } if self.distance.is_none() => {
                Prompt::new("Specify through point").with_keywords(&["eXit", "Undo"]).with_default("eXit")
            }
            Stage::Side { .. } => {
                Prompt::new("Specify point on side to offset").with_keywords(&["eXit", "Undo"]).with_default("eXit")
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (Stage::Distance, Input::Value(d)) if d > 0.0 => {
                self.distance = Some(d);
                ctx.vars.offset_distance = Some(d);
                self.stage = Stage::Select;
            }
            (Stage::Distance, Input::Value(_)) => return Err("Requires a positive value or Through.".into()),
            (Stage::Distance, Input::Keyword("Through")) => {
                self.distance = None;
                ctx.vars.offset_distance = None;
                self.stage = Stage::Select;
            }
            (Stage::Distance, Input::Enter) => self.stage = Stage::Select,
            (Stage::Select, Input::Pick { at, .. }) => match snap::pick(ctx.doc, at, ctx.pick_tolerance) {
                Some(id) => {
                    let geometry = ctx.doc.entity(id).map(|e| e.geometry.clone()).ok_or("Object not found.")?;
                    self.stage = Stage::Side { id, geometry };
                }
                None => ctx.messages.push("No object found.".into()),
            },
            (Stage::Side { id, geometry }, Input::Point(p)) => {
                let results = self.results(geometry, p);
                if results.is_empty() {
                    self.stage = Stage::Select;
                    return Err("Cannot offset that object by this distance.".into());
                }
                let (layer, color) =
                    ctx.doc.entity(*id).map(|e| (e.layer.clone(), e.color)).ok_or("Object not found.")?;
                self.history.push(results.len());
                for g in results {
                    ctx.doc.add_on_layer(g, &layer, color);
                }
                self.stage = Stage::Select;
            }
            (Stage::Select | Stage::Side { .. }, Input::Keyword("Undo")) => self.undo(ctx),
            (Stage::Select | Stage::Side { .. }, Input::Enter | Input::Keyword("eXit")) => return Ok(Step::Done),
            (Stage::Select, _) => return Err("Select one object, or press Enter to exit.".into()),
            _ => return Err("Point or option keyword required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match &self.stage {
            Stage::Side { geometry, .. } => self.results(geometry, cursor),
            _ => Vec::new(),
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        match self.stage {
            Stage::Side { id, .. } => vec![id],
            _ => Vec::new(),
        }
    }

    fn wants_pick(&self) -> bool {
        matches!(self.stage, Stage::Select)
    }
}

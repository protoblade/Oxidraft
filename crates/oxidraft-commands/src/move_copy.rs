use oxidraft_geom::Point;
use oxidraft_model::{Entity, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step, common::selected_entities};

/// MOVE and COPY: select objects, base point, second point.
pub struct MoveCopy {
    copy: bool,
    selected: Vec<Entity>,
    selecting: bool,
    base: Option<Point>,
    copies: usize,
}

impl MoveCopy {
    pub fn moving() -> Self {
        Self { copy: false, selected: Vec::new(), selecting: true, base: None, copies: 0 }
    }

    pub fn copying() -> Self {
        Self { copy: true, ..Self::moving() }
    }

    fn place(&mut self, d: Point, ctx: &mut Ctx) -> Step {
        if self.copy {
            for e in &self.selected {
                ctx.doc.add_like(e.geometry.translated(d), e);
            }
            self.copies += 1;
            Step::Continue
        } else {
            for e in &self.selected {
                ctx.doc.modify(e.id, e.geometry.translated(d));
            }
            Step::Done
        }
    }
}

impl Command for MoveCopy {
    fn prompt(&self) -> Prompt {
        match (self.selecting, self.base, self.copies) {
            (true, _, _) => Prompt::new("Select objects"),
            (false, None, _) => Prompt::new("Specify base point"),
            (false, Some(_), 0) => Prompt::new("Specify second point").with_default("use first point as displacement"),
            (false, Some(_), _) => {
                Prompt::new("Specify second point").with_keywords(&["eXit", "Undo"]).with_default("eXit")
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if self.selecting {
            let Input::Selection(selection) = input else { return Err("Select objects, then press Enter.".into()) };
            self.selected = selected_entities(&selection, ctx);
            self.selecting = false;
            return Ok(if self.selected.is_empty() { Step::Done } else { Step::Continue });
        }
        let Some(base) = self.base else {
            return match input {
                Input::Point(p) => {
                    self.base = Some(p);
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err("Point required.".into()),
            };
        };
        match input {
            Input::Point(p) => Ok(self.place(p - base, ctx)),
            Input::Value(v) => {
                if base.approx_eq(ctx.cursor) {
                    return Err("Move the cursor to show a direction.".into());
                }
                Ok(self.place(base.polar(v, base.angle_to(ctx.cursor)) - base, ctx))
            }
            // AutoCAD: Enter at the first second-point prompt uses the base point as the displacement.
            Input::Enter if self.copies == 0 => {
                self.place(base, ctx);
                Ok(Step::Done)
            }
            Input::Enter | Input::Keyword("eXit") => Ok(Step::Done),
            Input::Keyword("Undo") => {
                for _ in 0..self.selected.len() {
                    ctx.doc.undo_last_in_transaction();
                }
                self.copies -= 1;
                Ok(Step::Continue)
            }
            _ => Err("Point or option keyword required.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Some(base) = self.base else { return Vec::new() };
        let d = cursor - base;
        let mut out: Vec<Geometry> = self.selected.iter().map(|e| e.geometry.translated(d)).collect();
        out.push(Geometry::Line { start: base, end: cursor });
        out
    }

    fn base_point(&self) -> Option<Point> {
        self.base
    }

    fn highlighted(&self) -> Vec<oxidraft_model::EntityId> {
        self.selected.iter().map(|e| e.id).collect()
    }

    fn wants_selection(&self) -> bool {
        self.selecting
    }
}

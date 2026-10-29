//! TABLE (TB): places a table set up in the Insert Table dialog, then
//! opens it to fill in; TABLEDIT edits a table's cells.

use oxidraft_geom::Point;
use oxidraft_model::Geometry;

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest};

/// Placing a table: its upper-left corner at the point picked.
pub struct TableInsert {
    table: Geometry,
}

impl TableInsert {
    pub fn new(table: Geometry) -> Self {
        Self { table }
    }

    fn at(&self, p: Point) -> Geometry {
        match &self.table {
            Geometry::Table { position, .. } => self.table.translated(p - *position),
            g => g.clone(),
        }
    }
}

impl Command for TableInsert {
    fn prompt(&self) -> Prompt {
        Prompt::new("Specify insertion point")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => {
                let id = ctx.doc.add(self.at(p));
                ctx.ui = Some(UiRequest::EditTable(id));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Specify the table's upper-left corner.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.at(cursor).primitives().unwrap_or_default()
    }
}

/// TABLEDIT: the table picked opens to have its cells edited.
#[derive(Default)]
pub struct TableEdit;

impl Command for TableEdit {
    fn prompt(&self) -> Prompt {
        Prompt::new("Pick a table cell")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } => {
                let table = ctx
                    .pick(at)
                    .filter(|id| ctx.doc.entity(*id).is_some_and(|e| matches!(e.geometry, Geometry::Table { .. })));
                let id = table.ok_or("That isn't a table.")?;
                ctx.ui = Some(UiRequest::EditTable(id));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Pick a table.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

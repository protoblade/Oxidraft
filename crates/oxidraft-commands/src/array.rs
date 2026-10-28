use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Entity, EntityId, Geometry};

use crate::{
    Command, Ctx, Input, Prompt, Step,
    common::{SELECT, apply, selected_entities},
};

/// AutoCAD's MAXARRAY: the most copies one array may make.
const MAX_ITEMS: f64 = 100_000.0;

/// The offsets of every copy but the original in a rectangular array.
fn grid(rows: usize, columns: usize, row_distance: f64, column_distance: f64) -> Vec<Point> {
    let cells = (0..rows).flat_map(|r| (0..columns).map(move |c| (r, c)));
    cells.skip(1).map(|(r, c)| Point::new(c as f64 * column_distance, r as f64 * row_distance)).collect()
}

#[derive(Clone, Copy, PartialEq)]
pub enum ArrayKind {
    Rectangular,
    Polar,
}

#[derive(Clone, Copy)]
enum Stage {
    Select,
    Kind,
    Rows,
    Columns,
    RowDistance,
    /// A unit cell from its first corner.
    CellCorner(Point),
    ColumnDistance,
    Center,
    Items,
    Fill,
    Rotate,
}

/// ARRAY (the command-line -ARRAY): copies objects in rows and columns, or
/// around a center. ARRAYRECT and ARRAYPOLAR skip the type prompt. The
/// copies are ordinary objects, not an associative array.
pub struct Array {
    stage: Stage,
    kind: Option<ArrayKind>,
    selected: Vec<Entity>,
    rows: usize,
    columns: usize,
    row_distance: f64,
    center: Point,
    items: usize,
    fill: f64,
}

impl Array {
    pub fn new(kind: Option<ArrayKind>) -> Self {
        Self {
            stage: Stage::Select,
            kind,
            selected: Vec::new(),
            rows: 1,
            columns: 1,
            row_distance: 0.0,
            center: Point::ORIGIN,
            items: 0,
            fill: 360.0,
        }
    }

    fn start_kind(&mut self, kind: ArrayKind) {
        self.kind = Some(kind);
        self.stage = match kind {
            ArrayKind::Rectangular => Stage::Rows,
            ArrayKind::Polar => Stage::Center,
        };
    }

    fn copy_grid(&self, column_distance: f64, ctx: &mut Ctx) -> Step {
        for d in grid(self.rows, self.columns, self.row_distance, column_distance) {
            apply(&self.selected, true, ctx, |g| g.translated(d));
        }
        Step::Done
    }

    fn copy_polar(&self, rotate: bool, ctx: &mut Ctx) -> Step {
        // A full circle spaces items evenly; a partial fill puts the last on its end.
        let step = if self.fill.abs() >= 360.0 - 1e-9 {
            self.fill / self.items as f64
        } else {
            self.fill / (self.items - 1) as f64
        };
        // Unrotated copies move as a group, by where their middle goes.
        let middle =
            self.selected.iter().map(|e| e.geometry.bounds()).reduce(Aabb::union).map_or(self.center, |b| b.center());
        for i in 1..self.items {
            let angle = step * i as f64;
            let (center, shift) = (self.center, middle.rotated_about(self.center, angle) - middle);
            apply(&self.selected, true, ctx, |g| if rotate { g.rotated(center, angle) } else { g.translated(shift) });
        }
        Step::Done
    }

    /// A count typed at a prompt: a whole number from `min` up.
    fn count(v: f64, min: f64) -> Result<usize, String> {
        if v < min || v.fract() != 0.0 || v > MAX_ITEMS {
            return Err(format!("Requires a whole number from {min} to {MAX_ITEMS}."));
        }
        Ok(v as usize)
    }
}

impl Command for Array {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select objects"),
            Stage::Kind => Prompt::new("Enter the type of array")
                .with_keywords(&["Rectangular", "Polar"])
                .with_default("Rectangular"),
            Stage::Rows => Prompt::new("Enter the number of rows (---)").with_default(self.rows.to_string()),
            Stage::Columns => Prompt::new("Enter the number of columns (|||)").with_default(self.columns.to_string()),
            Stage::RowDistance => Prompt::new("Enter the distance between rows or specify unit cell (---)"),
            Stage::CellCorner(_) => Prompt::new("Specify other corner"),
            Stage::ColumnDistance => Prompt::new("Specify the distance between columns (|||)"),
            Stage::Center => Prompt::new("Specify center point of array"),
            Stage::Items => Prompt::new("Enter the number of items in the array"),
            Stage::Fill => Prompt::new("Specify the angle to fill (+=ccw, -=cw)").with_default("360"),
            Stage::Rotate => Prompt::new("Rotate arrayed objects?").with_keywords(&["Yes", "No"]).with_default("Y"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (Stage::Select, Input::Selection(s)) => {
                self.selected = selected_entities(&s, ctx);
                if self.selected.is_empty() {
                    return Ok(Step::Done);
                }
                match self.kind {
                    Some(kind) => self.start_kind(kind),
                    None => self.stage = Stage::Kind,
                }
            }
            (Stage::Select, _) => return Err(SELECT.into()),
            (Stage::Kind, Input::Keyword("Polar")) => self.start_kind(ArrayKind::Polar),
            (Stage::Kind, Input::Keyword(_) | Input::Enter) => self.start_kind(ArrayKind::Rectangular),
            (Stage::Rows, input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(v) = input {
                    self.rows = Self::count(v, 1.0)?;
                }
                self.stage = Stage::Columns;
            }
            (Stage::Columns, input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(v) = input {
                    self.columns = Self::count(v, 1.0)?;
                }
                if (self.rows * self.columns) as f64 > MAX_ITEMS {
                    return Err(format!("An array can't have more than {MAX_ITEMS} items."));
                }
                self.stage = match (self.rows, self.columns) {
                    (1, 1) => return Ok(Step::Done),
                    (1, _) => Stage::ColumnDistance,
                    _ => Stage::RowDistance,
                };
            }
            (Stage::RowDistance, Input::Value(d)) => {
                self.row_distance = d;
                if self.columns == 1 {
                    return Ok(self.copy_grid(0.0, ctx));
                }
                self.stage = Stage::ColumnDistance;
            }
            (Stage::RowDistance, Input::Point(p)) => self.stage = Stage::CellCorner(p),
            (Stage::CellCorner(first), Input::Point(p)) => {
                self.row_distance = p.y - first.y;
                return Ok(self.copy_grid(p.x - first.x, ctx));
            }
            (Stage::ColumnDistance, Input::Value(d)) => return Ok(self.copy_grid(d, ctx)),
            (Stage::Center, Input::Point(p)) => {
                self.center = p;
                self.stage = Stage::Items;
            }
            (Stage::Items, Input::Value(v)) => {
                self.items = Self::count(v, 2.0)?;
                self.stage = Stage::Fill;
            }
            (Stage::Fill, input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(a) = input {
                    if a == 0.0 || a.abs() > 360.0 {
                        return Err("The angle to fill must be nonzero, up to 360.".into());
                    }
                    self.fill = a;
                }
                self.stage = Stage::Rotate;
            }
            (Stage::Rotate, Input::Keyword("No")) => return Ok(self.copy_polar(false, ctx)),
            (Stage::Rotate, Input::Keyword(_) | Input::Enter) => return Ok(self.copy_polar(true, ctx)),
            (_, Input::Enter) => return Ok(Step::Done),
            (Stage::Center | Stage::CellCorner(_), _) => return Err("Point required.".into()),
            _ => return Err("Requires a number or an option keyword.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Stage::CellCorner(first) = self.stage else { return Vec::new() };
        let mut out: Vec<Geometry> = grid(self.rows, self.columns, cursor.y - first.y, cursor.x - first.x)
            .into_iter()
            .flat_map(|d| self.selected.iter().map(move |e| e.geometry.translated(d)))
            .collect();
        out.push(Geometry::Line { start: first, end: cursor });
        out
    }

    fn base_point(&self) -> Option<Point> {
        match self.stage {
            Stage::CellCorner(first) => Some(first),
            _ => None,
        }
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.selected.iter().map(|e| e.id).collect()
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, Stage::Select)
    }
}

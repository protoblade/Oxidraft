use oxidraft_geom::{Aabb, Point};
use oxidraft_model::assoc_array::ArrayLayout;
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
    /// Whether the array is one associative object, then back to `resume`.
    Associative,
}

/// ARRAY: copies objects in rows and columns, or around a center.
/// ARRAYRECT and ARRAYPOLAR skip the type prompt. While ARRAYASSOCIATIVITY
/// is on (or ASsociative says so) the array is one object ARRAYEDIT
/// changes; -ARRAY makes ordinary copies.
pub struct Array {
    stage: Stage,
    resume: Stage,
    /// Chosen with ASsociative; else ARRAYASSOCIATIVITY decides.
    associative: Option<bool>,
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
            resume: Stage::Select,
            associative: None,
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

    /// -ARRAY: ordinary copies.
    pub fn classic() -> Self {
        Self { associative: Some(false), ..Self::new(None) }
    }

    fn associative(&self, ctx: &Ctx) -> bool {
        self.associative.unwrap_or(ctx.vars.array_assoc)
    }

    /// The objects made one associative array.
    fn make(&self, layout: ArrayLayout, ctx: &mut Ctx) -> Step {
        let ids: Vec<EntityId> = self.selected.iter().map(|e| e.id).collect();
        ctx.doc.make_array(&ids, layout);
        Step::Done
    }

    fn start_kind(&mut self, kind: ArrayKind) {
        self.kind = Some(kind);
        self.stage = match kind {
            ArrayKind::Rectangular => Stage::Rows,
            ArrayKind::Polar => Stage::Center,
        };
    }

    fn copy_grid(&self, column_distance: f64, ctx: &mut Ctx) -> Step {
        if self.associative(ctx) {
            let layout = ArrayLayout::Rect {
                rows: self.rows,
                columns: self.columns,
                row_spacing: self.row_distance,
                column_spacing: column_distance,
            };
            return self.make(layout, ctx);
        }
        for d in grid(self.rows, self.columns, self.row_distance, column_distance) {
            apply(&self.selected, true, ctx, |g| g.translated(d));
        }
        Step::Done
    }

    fn copy_polar(&self, rotate: bool, ctx: &mut Ctx) -> Step {
        if self.associative(ctx) {
            let layout = ArrayLayout::Polar { center: self.center, items: self.items, fill: self.fill, rotate };
            return self.make(layout, ctx);
        }
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
            Stage::Rows => Prompt::new("Enter the number of rows (---)")
                .with_keywords(&["ASsociative"])
                .with_default(self.rows.to_string()),
            Stage::Columns => Prompt::new("Enter the number of columns (|||)").with_default(self.columns.to_string()),
            Stage::RowDistance => Prompt::new("Enter the distance between rows or specify unit cell (---)"),
            Stage::CellCorner(_) => Prompt::new("Specify other corner"),
            Stage::ColumnDistance => Prompt::new("Specify the distance between columns (|||)"),
            Stage::Center => Prompt::new("Specify center point of array").with_keywords(&["ASsociative"]),
            Stage::Items => Prompt::new("Enter the number of items in the array"),
            Stage::Fill => Prompt::new("Specify the angle to fill (+=ccw, -=cw)").with_default("360"),
            Stage::Rotate => Prompt::new("Rotate arrayed objects?").with_keywords(&["Yes", "No"]).with_default("Y"),
            Stage::Associative => Prompt::new("Create associative array")
                .with_keywords(&["Yes", "No"])
                .with_default(if self.associative == Some(false) { "No" } else { "Yes" }),
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
            (Stage::Rows | Stage::Center, Input::Keyword("ASsociative")) => {
                self.resume = self.stage;
                self.stage = Stage::Associative;
            }
            (Stage::Associative, input @ (Input::Keyword(_) | Input::Enter)) => {
                self.associative = Some(input != Input::Keyword("No"));
                self.stage = self.resume;
            }
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

#[derive(Clone, Copy, PartialEq)]
enum EditStage {
    Select,
    Option,
    Rows,
    RowSpacing,
    Columns,
    ColumnSpacing,
    Items,
    AngleBetween,
    Fill,
    Rotate,
    Align,
}

/// ARRAYEDIT: changes an associative array's layout — its rows and
/// columns, its items around a center or along a path, how they turn —
/// one option after another, until eXit.
pub struct ArrayEdit {
    stage: EditStage,
    id: Option<EntityId>,
    /// The array's layout as it is now.
    layout: Option<ArrayLayout>,
    /// A count typed, waiting for its spacing.
    count: usize,
}

impl Default for ArrayEdit {
    fn default() -> Self {
        Self { stage: EditStage::Select, id: None, layout: None, count: 0 }
    }
}

/// A number for a prompt's default.
fn shown(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl ArrayEdit {
    fn change(&mut self, ctx: &mut Ctx, f: impl FnOnce(&mut ArrayLayout)) -> Result<Step, String> {
        let (Some(id), Some(mut layout)) = (self.id, self.layout.clone()) else { return Ok(Step::Done) };
        f(&mut layout);
        if layout.count() > oxidraft_model::assoc_array::MAX_ITEMS {
            return Err(format!("An array can't have more than {MAX_ITEMS} items."));
        }
        ctx.doc.set_array_layout(id, layout.clone());
        self.layout = Some(layout);
        self.stage = EditStage::Option;
        Ok(Step::Continue)
    }

    fn polar(&self) -> (usize, f64) {
        match self.layout {
            Some(ArrayLayout::Polar { items, fill, .. }) => (items, fill),
            _ => (2, 360.0),
        }
    }
}

impl Command for ArrayEdit {
    fn prompt(&self) -> Prompt {
        let layout = self.layout.as_ref();
        match self.stage {
            EditStage::Select => Prompt::new("Select array"),
            EditStage::Option => {
                let keywords: &[&str] = match layout {
                    Some(ArrayLayout::Rect { .. }) => &["Rows", "COLumns", "eXit"],
                    Some(ArrayLayout::Polar { .. }) => {
                        &["ITems", "Angle between", "Fill angle", "ROTate items", "eXit"]
                    }
                    _ => &["Items", "Align items", "eXit"],
                };
                Prompt::new("Enter an option").with_keywords(keywords).with_default("eXit")
            }
            EditStage::Rows | EditStage::Columns => {
                let (rows, columns) = match layout {
                    Some(ArrayLayout::Rect { rows, columns, .. }) => (*rows, *columns),
                    _ => (1, 1),
                };
                if self.stage == EditStage::Rows {
                    Prompt::new("Enter the number of rows").with_default(rows.to_string())
                } else {
                    Prompt::new("Enter the number of columns").with_default(columns.to_string())
                }
            }
            EditStage::RowSpacing | EditStage::ColumnSpacing => {
                let (row, column) = match layout {
                    Some(ArrayLayout::Rect { row_spacing, column_spacing, .. }) => (*row_spacing, *column_spacing),
                    _ => (0.0, 0.0),
                };
                if self.stage == EditStage::RowSpacing {
                    Prompt::new("Specify the distance between rows").with_default(shown(row))
                } else {
                    Prompt::new("Specify the distance between columns").with_default(shown(column))
                }
            }
            EditStage::Items => {
                let n = layout.map_or(0, ArrayLayout::count);
                let along = if matches!(layout, Some(ArrayLayout::Path { .. })) { " along path" } else { " in array" };
                Prompt::new(format!("Enter number of items{along}")).with_default(n.to_string())
            }
            EditStage::AngleBetween => {
                let (items, fill) = self.polar();
                let between = if fill.abs() >= 360.0 - 1e-9 { fill / items as f64 } else { fill / (items - 1) as f64 };
                Prompt::new("Specify the angle between items").with_default(shown(between))
            }
            EditStage::Fill => {
                Prompt::new("Specify the angle to fill (+=ccw, -=cw)").with_default(shown(self.polar().1))
            }
            EditStage::Rotate => Prompt::new("Rotate arrayed items?").with_keywords(&["Yes", "No"]).with_default("Yes"),
            EditStage::Align => {
                Prompt::new("Align arrayed items to the path?").with_keywords(&["Yes", "No"]).with_default("Yes")
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let count = |v: f64, min: f64| Array::count(v, min);
        match (self.stage, input) {
            (EditStage::Select, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                let layout = ctx.doc.array(id).map(|a| a.layout.clone()).ok_or("Not an associative array.")?;
                ctx.messages.push(format!("{} array, {} items", layout.name(), layout.count()));
                (self.id, self.layout, self.stage) = (Some(id), Some(layout), EditStage::Option);
            }
            (EditStage::Select, Input::Enter) | (EditStage::Option, Input::Enter | Input::Keyword("eXit")) => {
                return Ok(Step::Done);
            }
            (EditStage::Select, _) => return Err("Select an array.".into()),
            (EditStage::Option, Input::Keyword(k)) => {
                self.stage = match k {
                    "Rows" => EditStage::Rows,
                    "COLumns" => EditStage::Columns,
                    "ITems" | "Items" => EditStage::Items,
                    "Angle between" => EditStage::AngleBetween,
                    "Fill angle" => EditStage::Fill,
                    "ROTate items" => EditStage::Rotate,
                    _ => EditStage::Align,
                }
            }
            (EditStage::Rows | EditStage::Columns, input @ (Input::Value(_) | Input::Enter)) => {
                let Some(ArrayLayout::Rect { rows, columns, .. }) = self.layout else { return Ok(Step::Done) };
                let rows_now = self.stage == EditStage::Rows;
                self.count = match input {
                    Input::Value(v) => count(v, 1.0)?,
                    _ if rows_now => rows,
                    _ => columns,
                };
                self.stage = if rows_now { EditStage::RowSpacing } else { EditStage::ColumnSpacing };
            }
            (EditStage::RowSpacing | EditStage::ColumnSpacing, input @ (Input::Value(_) | Input::Enter)) => {
                let (n, rows_now) = (self.count, self.stage == EditStage::RowSpacing);
                let typed = if let Input::Value(d) = input { Some(d) } else { None };
                return self.change(ctx, |l| {
                    if let ArrayLayout::Rect { rows, columns, row_spacing, column_spacing } = l {
                        let (count, spacing) = if rows_now { (rows, row_spacing) } else { (columns, column_spacing) };
                        *count = n;
                        *spacing = typed.unwrap_or(*spacing);
                    }
                });
            }
            (EditStage::Items, Input::Value(v)) => {
                let n = count(v, 1.0)?;
                return self.change(ctx, |l| match l {
                    ArrayLayout::Polar { items, .. } => *items = n.max(2),
                    ArrayLayout::Path { items, .. } => *items = n,
                    ArrayLayout::Rect { .. } => {}
                });
            }
            (EditStage::AngleBetween, Input::Value(a)) => {
                let (items, _) = self.polar();
                if a == 0.0 || (a * (items - 1) as f64).abs() > 360.0 + 1e-9 {
                    return Err("The items would go past a full turn.".into());
                }
                // Items a turn apart fill the circle evenly.
                let all = a * items as f64;
                let fill_to = if (all.abs() - 360.0).abs() < 1e-9 { all } else { a * (items - 1) as f64 };
                return self.change(ctx, |l| {
                    if let ArrayLayout::Polar { fill, .. } = l {
                        *fill = fill_to;
                    }
                });
            }
            (EditStage::Fill, Input::Value(a)) => {
                if a == 0.0 || a.abs() > 360.0 {
                    return Err("The angle to fill must be nonzero, up to 360.".into());
                }
                return self.change(ctx, |l| {
                    if let ArrayLayout::Polar { fill, .. } = l {
                        *fill = a;
                    }
                });
            }
            (EditStage::Rotate | EditStage::Align, input @ (Input::Keyword(_) | Input::Enter)) => {
                let yes = input != Input::Keyword("No");
                return self.change(ctx, |l| match l {
                    ArrayLayout::Polar { rotate, .. } => *rotate = yes,
                    ArrayLayout::Path { align, .. } => *align = yes,
                    ArrayLayout::Rect { .. } => {}
                });
            }
            (_, Input::Enter) => self.stage = EditStage::Option,
            _ => return Err("Requires a number or an option keyword.".into()),
        }
        Ok(Step::Continue)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.id.into_iter().collect()
    }

    fn wants_pick(&self) -> bool {
        self.stage == EditStage::Select
    }
}

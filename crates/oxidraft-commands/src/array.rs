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
    /// Source: the item whose objects are edited.
    SourceItem,
    /// REPlace: the objects, their base point, then the items they replace.
    ReplaceObjects,
    ReplaceBase,
    ReplaceItems,
    /// A path array's Divide or Measure, and Measure's distance.
    Method,
    PathSpacing,
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
    /// REPlace's objects and their base point.
    replacement: Vec<EntityId>,
    replace_base: Point,
}

impl Default for ArrayEdit {
    fn default() -> Self {
        Self {
            stage: EditStage::Select,
            id: None,
            layout: None,
            count: 0,
            replacement: Vec::new(),
            replace_base: Point::ORIGIN,
        }
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
            EditStage::SourceItem => Prompt::new("Select item in array"),
            EditStage::Method => {
                let measure = matches!(layout, Some(ArrayLayout::Path { spacing: Some(_), .. }));
                Prompt::new("Enter path method").with_keywords(&["Divide", "Measure"]).with_default(if measure {
                    "Measure"
                } else {
                    "Divide"
                })
            }
            EditStage::PathSpacing => {
                let now = match layout {
                    Some(ArrayLayout::Path { path, items, spacing, .. }) => spacing.unwrap_or_else(|| {
                        let length: f64 = path.segments().iter().map(|s| s.length()).sum();
                        length / items.saturating_sub(1).max(1) as f64
                    }),
                    _ => 1.0,
                };
                Prompt::new("Specify the distance between items along path").with_default(shown(now))
            }
            EditStage::ReplaceObjects => Prompt::new("Select replacement objects"),
            EditStage::ReplaceBase => Prompt::new("Select base point of replacement objects").with_default("centroid"),
            EditStage::ReplaceItems => {
                Prompt::new("Replace array items").with_keywords(&["Source objects"]).with_default("done")
            }
            EditStage::Option => {
                let keywords: &[&str] = match layout {
                    Some(ArrayLayout::Rect { .. }) => &["Source", "REPlace", "Rows", "COLumns", "RESet", "eXit"],
                    Some(ArrayLayout::Polar { .. }) => {
                        &["Source", "REPlace", "ITems", "Angle between", "Fill angle", "ROTate items", "RESet", "eXit"]
                    }
                    _ => &["Source", "REPlace", "Method", "Items", "Align items", "RESet", "eXit"],
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
            (EditStage::Option, Input::Keyword("RESet")) => {
                let id = self.id.ok_or("Select an array.")?;
                if ctx.doc.reset_array_items(id) {
                    ctx.messages.push("Replaced items show the source objects again.".into());
                }
            }
            (EditStage::SourceItem, Input::Pick { at, .. }) => {
                let id = self.id.ok_or("Select an array.")?;
                let item = ctx.doc.array_item_at(id, at).ok_or("No item found.")?;
                let n = ctx.doc.begin_array_source_edit(id, item)?;
                ctx.messages.push(format!("{n} object(s) to edit."));
                ctx.messages.push("Use ARRAYCLOSE to save or discard the changes to the array's source.".into());
                return Ok(Step::Done);
            }
            (EditStage::ReplaceObjects, Input::Selection(s)) => {
                if s.ids.is_empty() {
                    self.stage = EditStage::Option;
                    return Ok(Step::Continue);
                }
                let bounds = s.ids.iter().filter_map(|id| ctx.doc.entity(*id)).map(|e| ctx.doc.entity_bounds(e));
                self.replace_base = bounds.reduce(Aabb::union).map_or(Point::ORIGIN, |b| b.center());
                self.replacement = s.ids;
                self.stage = EditStage::ReplaceBase;
            }
            (EditStage::ReplaceBase, input @ (Input::Point(_) | Input::Enter)) => {
                if let Input::Point(p) = input {
                    self.replace_base = p;
                }
                self.stage = EditStage::ReplaceItems;
            }
            (EditStage::ReplaceItems, Input::Pick { at, .. } | Input::Point(at)) => {
                let id = self.id.ok_or("Select an array.")?;
                let item = ctx.doc.array_item_at(id, at).ok_or("No item found.")?;
                ctx.doc.replace_array_items(id, &self.replacement, self.replace_base, Some(item));
            }
            (EditStage::ReplaceItems, Input::Keyword(_)) => {
                let id = self.id.ok_or("Select an array.")?;
                ctx.doc.replace_array_items(id, &self.replacement, self.replace_base, None);
                ctx.messages.push("Every item shows the replacement objects.".into());
                self.stage = EditStage::Option;
            }
            (EditStage::ReplaceItems, Input::Enter) => self.stage = EditStage::Option,
            (EditStage::Method, Input::Keyword("Measure")) => self.stage = EditStage::PathSpacing,
            (EditStage::Method, Input::Keyword(_)) => {
                return self.change(ctx, |l| {
                    if let ArrayLayout::Path { spacing, .. } = l {
                        *spacing = None;
                    }
                });
            }
            (EditStage::PathSpacing, Input::Value(d)) if d > 0.0 => {
                return self.change(ctx, |l| {
                    if let ArrayLayout::Path { spacing, .. } = l {
                        *spacing = Some(d);
                    }
                });
            }
            (EditStage::Option, Input::Keyword(k)) => {
                self.stage = match k {
                    "Source" => EditStage::SourceItem,
                    "Method" => EditStage::Method,
                    "REPlace" => EditStage::ReplaceObjects,
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
        matches!(self.stage, EditStage::Select | EditStage::SourceItem | EditStage::ReplaceItems)
    }

    fn wants_selection(&self) -> bool {
        self.stage == EditStage::ReplaceObjects
    }
}

/// Dragging an array's count, spacing or fill grip (AutoCAD's
/// `** COLUMN COUNT **` and the like): the array laid out again where the
/// grip lands, or by the number typed.
pub struct ArrayGripEdit {
    id: EntityId,
    grip: oxidraft_model::assoc_array::ArrayGrip,
    at: Point,
    layout: ArrayLayout,
    /// The first item's base, the array's x and y in the world, and its scale.
    origin: Point,
    /// A polar array's center, in the world.
    center: Point,
    /// The reference's placement and its block's base: the world into the
    /// layout's coordinates.
    placement: oxidraft_model::Placement,
    outer_base: Point,
    x: Point,
    y: Point,
    scale: f64,
    /// The first item's objects in the world, for the preview.
    first: Vec<Geometry>,
}

impl ArrayGripEdit {
    pub fn new(
        doc: &oxidraft_model::Document,
        id: EntityId,
        grip: oxidraft_model::assoc_array::ArrayGrip,
        at: Point,
    ) -> Option<Self> {
        let array = doc.array(id)?;
        let Geometry::Insert { placement, .. } = &doc.entity(id)?.geometry else { return None };
        let (item, block) = doc.array_item(id, 0)?;
        let b = doc.block(&block)?;
        let first = b.entities.iter().map(|be| item.place(b.base, &be.geometry)).collect();
        let turn = |p: Point| p.rotated_about(Point::ORIGIN, placement.rotation);
        let x = turn(Point::new(if placement.mirror { -1.0 } else { 1.0 }, 0.0));
        let outer = match &doc.entity(id)?.geometry {
            Geometry::Insert { block, .. } => doc.block(block)?.base,
            _ => return None,
        };
        let center = match &array.layout {
            ArrayLayout::Polar { center, .. } => placement.apply(outer, *center),
            _ => placement.position,
        };
        Some(Self {
            id,
            grip,
            at,
            layout: array.layout.clone(),
            origin: placement.position,
            center,
            placement: *placement,
            outer_base: outer,
            x,
            y: turn(Point::new(0.0, 1.0)),
            scale: placement.scale,
            first,
        })
    }

    /// The layout with the grip at `p`.
    fn layout_at(&self, p: Point) -> Option<ArrayLayout> {
        use oxidraft_model::assoc_array::ArrayGrip;
        let d = p - self.origin;
        let (along_x, along_y) = (d.dot(self.x) / self.scale, d.dot(self.y) / self.scale);
        // A count by the spacing (a stand-in for none), which it then keeps.
        let count = |along: f64, spacing: &mut f64, other: f64| {
            *spacing = oxidraft_model::assoc_array::grip_spacing(*spacing, other);
            ((along / *spacing).round() as i64 + 1).clamp(1, MAX_ITEMS as i64) as usize
        };
        let mut layout = self.layout.clone();
        match (&mut layout, self.grip) {
            (ArrayLayout::Rect { columns, column_spacing, row_spacing, .. }, ArrayGrip::Columns) => {
                *columns = count(along_x, column_spacing, *row_spacing)
            }
            (ArrayLayout::Rect { column_spacing, .. }, ArrayGrip::ColumnSpacing) => *column_spacing = along_x,
            (ArrayLayout::Rect { rows, row_spacing, column_spacing, .. }, ArrayGrip::Rows) => {
                *rows = count(along_y, row_spacing, *column_spacing)
            }
            (ArrayLayout::Rect { row_spacing, .. }, ArrayGrip::RowSpacing) => *row_spacing = along_y,
            // As many items as fit up to where the grip is, the spacing kept.
            (ArrayLayout::Path { path, items, spacing, .. }, ArrayGrip::Items) => {
                let segs = path.segments();
                let length: f64 = segs.iter().map(|s| s.length()).sum();
                let gaps = if path.is_closed() { *items } else { items.saturating_sub(1) };
                let step = spacing.unwrap_or(length / gaps.max(1) as f64);
                let along = oxidraft_model::assoc_array::length_at(path, self.placement.local(self.outer_base, p));
                let fit = if step > 1e-12 { (along / step).round() as usize + 1 } else { 1 };
                *items = fit.clamp(1, MAX_ITEMS as usize);
            }
            // The last item lands where the grip does.
            (ArrayLayout::Polar { fill, .. }, ArrayGrip::Fill) => {
                let a = (self.center.angle_to(p) - self.center.angle_to(self.origin)).rem_euclid(360.0);
                *fill = a.max(1e-6);
            }
            _ => return None,
        }
        (layout.count() <= MAX_ITEMS as usize).then_some(layout)
    }

    /// The layout with the number typed: a count, a spacing or an angle.
    fn layout_typed(&self, v: f64) -> Option<ArrayLayout> {
        use oxidraft_model::assoc_array::ArrayGrip;
        let mut layout = self.layout.clone();
        let n = (v.round() as i64).clamp(1, MAX_ITEMS as i64) as usize;
        match (&mut layout, self.grip) {
            (ArrayLayout::Rect { columns, .. }, ArrayGrip::Columns) => *columns = n,
            (ArrayLayout::Rect { column_spacing, .. }, ArrayGrip::ColumnSpacing) => *column_spacing = v,
            (ArrayLayout::Rect { rows, .. }, ArrayGrip::Rows) => *rows = n,
            (ArrayLayout::Rect { row_spacing, .. }, ArrayGrip::RowSpacing) => *row_spacing = v,
            (ArrayLayout::Polar { fill, .. }, ArrayGrip::Fill) if v != 0.0 && v.abs() <= 360.0 => *fill = v,
            (ArrayLayout::Path { items, .. }, ArrayGrip::Items) => *items = n,
            _ => return None,
        }
        (layout.count() <= MAX_ITEMS as usize).then_some(layout)
    }
}

impl Command for ArrayGripEdit {
    fn prompt(&self) -> Prompt {
        use oxidraft_model::assoc_array::ArrayGrip;
        let what = match self.grip {
            ArrayGrip::Columns => "** COLUMN COUNT ** Specify number of columns",
            ArrayGrip::ColumnSpacing => "** COLUMN SPACING ** Specify the distance between columns",
            ArrayGrip::Rows => "** ROW COUNT ** Specify number of rows",
            ArrayGrip::RowSpacing => "** ROW SPACING ** Specify the distance between rows",
            ArrayGrip::Fill => "** FILL ANGLE ** Specify the angle to fill",
            ArrayGrip::Items => "** ITEM COUNT ** Specify number of items along path",
            ArrayGrip::Base => "** MOVE ** Specify base point",
        };
        Prompt::new(what).with_keywords(&["eXit"])
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let layout = match input {
            Input::Point(p) => self.layout_at(p),
            Input::Value(v) => self.layout_typed(v),
            Input::Enter | Input::Keyword(_) => return Ok(Step::Done),
            _ => return Err("Point or number required.".into()),
        };
        let layout = layout.ok_or("That would make no array.")?;
        ctx.doc.set_array_layout(self.id, layout);
        Ok(Step::Done)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let Some(layout) = self.layout_at(cursor) else { return Vec::new() };
        let base = Point::ORIGIN;
        let first = layout.items(base).first().copied().unwrap_or((base, 0.0));
        layout
            .items(base)
            .into_iter()
            .take(2000)
            .flat_map(|(at, turn)| {
                let d = at - first.0;
                let shift = (self.x * d.x + self.y * d.y) * self.scale;
                let pivot = self.origin;
                self.first.iter().map(move |g| g.rotated(pivot, turn - first.1).translated(shift))
            })
            .collect()
    }

    fn base_point(&self) -> Option<Point> {
        Some(self.at)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        vec![self.id]
    }
}

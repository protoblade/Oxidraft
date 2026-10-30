//! Associative arrays (ARRAYRECT, ARRAYPOLAR and ARRAYPATH while
//! ARRAYASSOCIATIVITY is on): one object, a block reference, whose
//! anonymous block holds a reference to the item block (the source
//! objects) at each item. ARRAYEDIT changes the layout and the block is
//! made again. Other programs see a block reference, as with AutoCAD's.
//! Items can be replaced by other objects one by one (ARRAYEDIT REPlace),
//! the source objects edited in place (ARRAYEDIT Source, ARRAYCLOSE), and
//! a path array follows its path when the path changes.

use oxidraft_geom::{Aabb, EPSILON, Point, curve::Seg};

use crate::{Block, BlockEntity, Color, Document, Entity, EntityId, Geometry, Lineweight, Placement, linetype};

/// AutoCAD's MAXARRAY: the most items one array may have.
pub const MAX_ITEMS: usize = 100_000;

/// An associative array's layout, in its block's coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum ArrayLayout {
    Rect {
        rows: usize,
        columns: usize,
        row_spacing: f64,
        column_spacing: f64,
    },
    /// `fill` degrees around `center`: a full turn spaces the items evenly,
    /// less puts the last at its end.
    Polar {
        center: Point,
        items: usize,
        fill: f64,
        rotate: bool,
    },
    /// Spaced evenly along a curve, the first at its start, turned with it
    /// or not.
    Path {
        path: Box<Geometry>,
        items: usize,
        align: bool,
    },
}

/// What makes a block reference an associative array.
#[derive(Debug, Clone, PartialEq)]
pub struct AssocArray {
    pub layout: ArrayLayout,
    /// The block holding one item: the source objects, around its base.
    pub item: String,
    /// Items replaced by other objects: (item number, the block holding
    /// them, around its base).
    pub replaced: Vec<(usize, String)>,
}

impl AssocArray {
    /// The block item `i` shows.
    pub fn item_block(&self, i: usize) -> &str {
        self.replaced.iter().find(|(n, _)| *n == i).map_or(self.item.as_str(), |(_, b)| b.as_str())
    }
}

/// What an array's grip changes: its place (the base grip, moving it),
/// the rows and columns and their spacing, or a polar array's fill angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayGrip {
    Base,
    Columns,
    ColumnSpacing,
    Rows,
    RowSpacing,
    Fill,
}

/// The spacing a count grip goes by: the rows' or columns' own, or for a
/// single row or column with none, the other's (else 1).
pub fn grip_spacing(spacing: f64, other: f64) -> f64 {
    if spacing.abs() > EPSILON {
        spacing
    } else if other.abs() > EPSILON {
        other.abs()
    } else {
        1.0
    }
}

/// Whether two curves run the same way through the same points (a path
/// kept as a polyline is the line or arc it was).
fn same_curve(a: &Geometry, b: &Geometry) -> bool {
    let (sa, sb) = (a.segments(), b.segments());
    sa.len() == sb.len()
        && a.is_closed() == b.is_closed()
        && sa.iter().zip(&sb).all(|(x, y)| {
            x.start().approx_eq(y.start()) && x.end().approx_eq(y.end()) && (x.bulge() - y.bulge()).abs() < 1e-6
        })
}

/// One placement after another: `inner` in a block placed by `outer`.
fn compose(outer: &Placement, outer_base: Point, inner: &Placement) -> Placement {
    Placement {
        position: outer.apply(outer_base, inner.position),
        scale: outer.scale * inner.scale,
        rotation: if outer.mirror { outer.rotation - inner.rotation } else { outer.rotation + inner.rotation },
        mirror: outer.mirror != inner.mirror,
    }
}

/// The point `d` along the curve's segments, and its direction (degrees).
pub fn at_length(segs: &[Seg], mut d: f64) -> Option<(Point, f64)> {
    for s in segs {
        let len = s.length();
        if d <= len + EPSILON {
            let t = if len > EPSILON { (d / len).clamp(0.0, 1.0) } else { 0.0 };
            return Some((s.point_at(t), Point::ORIGIN.angle_to(s.tangent_at(t))));
        }
        d -= len;
    }
    None
}

impl ArrayLayout {
    pub fn name(&self) -> &'static str {
        match self {
            ArrayLayout::Rect { .. } => "Rectangular",
            ArrayLayout::Polar { .. } => "Polar",
            ArrayLayout::Path { .. } => "Path",
        }
    }

    pub fn count(&self) -> usize {
        match self {
            ArrayLayout::Rect { rows, columns, .. } => rows * columns,
            ArrayLayout::Polar { items, .. } | ArrayLayout::Path { items, .. } => *items,
        }
    }

    /// Where each item's base goes and how far it turns (degrees), the
    /// item block's base being `base`.
    pub fn items(&self, base: Point) -> Vec<(Point, f64)> {
        match self {
            ArrayLayout::Rect { rows, columns, row_spacing, column_spacing } => (0..*rows)
                .flat_map(|r| {
                    (0..*columns)
                        .map(move |c| (base + Point::new(c as f64 * column_spacing, r as f64 * row_spacing), 0.0))
                })
                .collect(),
            ArrayLayout::Polar { center, items, fill, rotate } => {
                let step = if fill.abs() >= 360.0 - 1e-9 {
                    fill / *items as f64
                } else {
                    fill / items.saturating_sub(1).max(1) as f64
                };
                (0..*items)
                    .map(|i| {
                        let angle = step * i as f64;
                        (base.rotated_about(*center, angle), if *rotate { angle } else { 0.0 })
                    })
                    .collect()
            }
            ArrayLayout::Path { path, items, align } => {
                let segs = path.segments();
                let length: f64 = segs.iter().map(|s| s.length()).sum();
                let Some((_, start)) = at_length(&segs, 0.0) else { return Vec::new() };
                let gaps = if path.is_closed() { *items } else { items.saturating_sub(1) };
                let step = length / gaps.max(1) as f64;
                (0..*items)
                    .filter_map(|i| at_length(&segs, step * i as f64))
                    .map(|(at, angle)| (at, if *align { angle - start } else { 0.0 }))
                    .collect()
            }
        }
    }
}

impl Document {
    /// An anonymous block name not taken yet: `*U` and a number.
    pub fn unused_anonymous_name(&self) -> String {
        (1..).map(|i| format!("*U{i}")).find(|n| self.block(n).is_none()).unwrap_or_default()
    }

    /// An array's block: a reference to its item block (or the item's
    /// replacement) at each item.
    fn array_block(&self, array: &AssocArray) -> Option<Block> {
        let item = self.block(&array.item)?;
        let entities = array
            .layout
            .items(item.base)
            .into_iter()
            .enumerate()
            .map(|(i, (at, turn))| BlockEntity {
                layer: "0".into(),
                color: Color::ByBlock,
                linetype: linetype::BYBLOCK.into(),
                lineweight: Lineweight::ByBlock,
                geometry: Geometry::Insert {
                    block: self.block(array.item_block(i)).map_or_else(|| item.name.clone(), |b| b.name.clone()),
                    placement: Placement { position: at, scale: 1.0, rotation: turn, mirror: false },
                    attributes: Vec::new(),
                    clip: None,
                },
            })
            .collect();
        Some(Block { name: self.unused_anonymous_name(), base: item.base, entities })
    }

    /// Makes `ids` into an associative array laid out by `layout` (their
    /// middle its base point): a block reference on the current layer in
    /// their place.
    pub fn make_array(&mut self, ids: &[EntityId], layout: ArrayLayout) -> Option<EntityId> {
        let sources: Vec<Entity> = ids.iter().filter_map(|id| self.entity(*id).cloned()).collect();
        let base = sources.iter().map(|e| self.entity_bounds(e)).reduce(Aabb::union)?.center();
        let item = Block {
            name: self.unused_anonymous_name(),
            base,
            entities: sources
                .iter()
                .map(|e| BlockEntity {
                    layer: e.layer.clone(),
                    color: e.color,
                    linetype: e.linetype.clone(),
                    lineweight: e.lineweight,
                    geometry: e.geometry.clone(),
                })
                .collect(),
        };
        let array = AssocArray { layout, item: item.name.clone(), replaced: Vec::new() };
        self.define_block(item);
        let block = self.array_block(&array)?;
        let name = block.name.clone();
        self.define_block(block);
        for e in &sources {
            self.remove(e.id);
        }
        let g = Geometry::Insert { block: name, placement: Placement::at(base), attributes: Vec::new(), clip: None };
        let id = self.add(g);
        self.update(id, |e| e.array = Some(Box::new(array)));
        Some(id)
    }

    /// The array `id` is, if it is one.
    pub fn array(&self, id: EntityId) -> Option<&AssocArray> {
        self.entity(id)?.array.as_deref()
    }

    /// Lays the array `id` out again by `layout` (ARRAYEDIT). Replaced
    /// items past the new count go.
    pub fn set_array_layout(&mut self, id: EntityId, layout: ArrayLayout) -> bool {
        let Some(array) = self.array(id) else { return false };
        let n = layout.count();
        let replaced = array.replaced.iter().filter(|(i, _)| *i < n).cloned().collect();
        let array = AssocArray { layout, item: array.item.clone(), replaced };
        self.set_array(id, array)
    }

    /// Makes array `id` `array`, its block made again.
    fn set_array(&mut self, id: EntityId, array: AssocArray) -> bool {
        let Some(Geometry::Insert { block: old, .. }) = self.entity(id).map(|e| e.geometry.clone()) else {
            return false;
        };
        let dropped: Vec<String> = self
            .array(id)
            .map(|a| {
                std::iter::once(a.item.clone())
                    .chain(a.replaced.iter().map(|(_, b)| b.clone()))
                    .filter(|b| *b != array.item && !array.replaced.iter().any(|(_, r)| r == b))
                    .collect()
            })
            .unwrap_or_default();
        let Some(block) = self.array_block(&array) else { return false };
        let name = block.name.clone();
        self.define_block(block);
        self.update(id, |e| {
            if let Geometry::Insert { block, .. } = &mut e.geometry {
                *block = name;
            }
            e.array = Some(Box::new(array));
        });
        self.remove_unused_block(&old);
        for b in dropped {
            self.remove_unused_block(&b);
        }
        true
    }

    /// Where item `i` of array `id` is in the world, and the block it shows.
    pub fn array_item(&self, id: EntityId, i: usize) -> Option<(Placement, String)> {
        let e = self.entity(id)?;
        let (Some(_), Geometry::Insert { block, placement, .. }) = (e.array.as_deref(), &e.geometry) else {
            return None;
        };
        let outer = self.block(block)?;
        let Geometry::Insert { block: item, placement: inner, .. } = &outer.entities.get(i)?.geometry else {
            return None;
        };
        Some((compose(placement, outer.base, inner), item.clone()))
    }

    /// Array `id`'s grips, the base grip first: for a rectangular one the
    /// second item of the first row and column (their spacing) and the last
    /// (their count), for a polar one the last item (its fill angle).
    pub fn array_grips(&self, id: EntityId) -> Vec<(Point, ArrayGrip)> {
        let Some(e) = self.entity(id) else { return Vec::new() };
        let (Some(array), Geometry::Insert { block, placement, .. }) = (e.array.as_deref(), &e.geometry) else {
            return Vec::new();
        };
        let (Some(outer), Some(item)) = (self.block(block), self.block(&array.item)) else { return Vec::new() };
        let world = |p: Point| placement.apply(outer.base, p);
        let mut grips = vec![(placement.position, ArrayGrip::Base)];
        match &array.layout {
            ArrayLayout::Rect { rows, columns, row_spacing, column_spacing } => {
                let (cs, rs) =
                    (grip_spacing(*column_spacing, *row_spacing), grip_spacing(*row_spacing, *column_spacing));
                let at = |r: usize, c: usize| world(item.base + Point::new(c as f64 * cs, r as f64 * rs));
                if *columns > 2 {
                    grips.push((at(0, 1), ArrayGrip::ColumnSpacing));
                }
                grips.push((at(0, (*columns).max(2) - 1), ArrayGrip::Columns));
                if *rows > 2 {
                    grips.push((at(1, 0), ArrayGrip::RowSpacing));
                }
                grips.push((at((*rows).max(2) - 1, 0), ArrayGrip::Rows));
            }
            ArrayLayout::Polar { items, .. } if *items > 1 => {
                if let Some((last, _)) = array.layout.items(item.base).last() {
                    grips.push((world(*last), ArrayGrip::Fill));
                }
            }
            _ => {}
        }
        grips
    }

    /// The number of the item of array `id` nearest `at`.
    pub fn array_item_at(&self, id: EntityId, at: Point) -> Option<usize> {
        let count = self.array(id)?.layout.count();
        (0..count)
            .filter_map(|i| {
                let (placement, block) = self.array_item(id, i)?;
                let b = self.block(&block)?;
                let d = b
                    .entities
                    .iter()
                    .map(|be| placement.place(b.base, &be.geometry).distance_to(at))
                    .fold(placement.position.distance(at), f64::min);
                Some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// ARRAYEDIT REPlace: `objects` (left in the drawing) stand in for item
    /// `item` of array `id`, or for every item (the source objects) when
    /// it's `None`; `base` of theirs lands where an item's base goes.
    pub fn replace_array_items(
        &mut self,
        id: EntityId,
        objects: &[EntityId],
        base: Point,
        item: Option<usize>,
    ) -> bool {
        let Some(mut array) = self.array(id).cloned() else { return false };
        let sources: Vec<Entity> = objects.iter().filter_map(|id| self.entity(*id).cloned()).collect();
        if sources.is_empty() {
            return false;
        }
        let block = Block {
            name: self.unused_anonymous_name(),
            base,
            entities: sources
                .iter()
                .map(|e| BlockEntity {
                    layer: e.layer.clone(),
                    color: e.color,
                    linetype: e.linetype.clone(),
                    lineweight: e.lineweight,
                    geometry: e.geometry.clone(),
                })
                .collect(),
        };
        let name = block.name.clone();
        // The item block's base is where items are laid out from.
        let item_base = self.block(&array.item).map_or(base, |b| b.base);
        let block = Block { base, ..block };
        self.define_block(block);
        match item {
            Some(i) => {
                array.replaced.retain(|(n, _)| *n != i);
                array.replaced.push((i, name));
            }
            None => {
                // The new source keeps the layout where it was.
                if let Some(b) = self.block(&name).cloned() {
                    let shift = item_base - base;
                    let moved = Block {
                        base: item_base,
                        entities: b
                            .entities
                            .iter()
                            .map(|be| BlockEntity { geometry: be.geometry.translated(shift), ..be.clone() })
                            .collect(),
                        ..b
                    };
                    self.define_block(moved);
                }
                array.item = name;
                array.replaced.clear();
            }
        }
        self.set_array(id, array)
    }

    /// ARRAYEDIT RESet: replaced items show the source objects again.
    pub fn reset_array_items(&mut self, id: EntityId) -> bool {
        let Some(mut array) = self.array(id).cloned() else { return false };
        if array.replaced.is_empty() {
            return false;
        }
        array.replaced.clear();
        self.set_array(id, array)
    }

    /// ARRAYEDIT Source: edits item `item`'s objects of array `id` in place
    /// (as REFEDIT), every item showing the change once ARRAYCLOSE saves it.
    pub fn begin_array_source_edit(&mut self, id: EntityId, item: usize) -> Result<usize, String> {
        let (placement, block) = self.array_item(id, item).ok_or("Not an associative array.")?;
        let reference = self.add(Geometry::Insert { block, placement, attributes: Vec::new(), clip: None });
        match self.begin_refedit(reference) {
            Ok(n) => {
                if let Some(r) = self.refedit().cloned() {
                    self.set_refedit(Some(crate::refedit::RefEdit { array: Some(id), ..r }));
                }
                Ok(n)
            }
            Err(e) => {
                self.remove(reference);
                Err(e)
            }
        }
    }

    /// Path arrays follow their paths: a curve changed from `before` moves
    /// the items of every array laid along it.
    pub(crate) fn follow_array_paths(&mut self, changes: &[(EntityId, Geometry)]) {
        let arrays: Vec<(EntityId, Geometry, Placement, Point)> = self
            .entities()
            .filter_map(|e| {
                let (ArrayLayout::Path { path, .. }, Geometry::Insert { block, placement, .. }) =
                    (&e.array.as_deref()?.layout, &e.geometry)
                else {
                    return None;
                };
                let base = self.block(block)?.base;
                Some((e.id, placement.place(base, path), *placement, base))
            })
            .collect();
        if arrays.is_empty() {
            return;
        }
        for (changed, before) in changes {
            let Some(now) = self.entity(*changed).map(|e| e.geometry.clone()) else { continue };
            for (id, path, placement, base) in &arrays {
                if *id == *changed || !same_curve(path, before) {
                    continue;
                }
                let Some(ArrayLayout::Path { items, align, .. }) = self.array(*id).map(|a| a.layout.clone()) else {
                    continue;
                };
                // Into the array block's own coordinates.
                let local = now.map_points(&|p| placement.local(*base, p), 1.0 / placement.scale, placement.mirror);
                self.set_array_layout(*id, ArrayLayout::Path { path: Box::new(local), items, align });
            }
        }
    }

    /// The array's items as plain objects, placed where the reference
    /// puts them (EXPLODE): (source object, its geometry there).
    pub fn array_objects(&self, e: &Entity) -> Vec<(BlockEntity, Geometry)> {
        let (Some(array), Geometry::Insert { block, placement, .. }) = (e.array.as_deref(), &e.geometry) else {
            return Vec::new();
        };
        let Some(outer) = self.block(block) else { return Vec::new() };
        let _ = array;
        let mut out = Vec::new();
        for be in &outer.entities {
            let Geometry::Insert { placement: inner, block: shown, .. } = &be.geometry else { continue };
            let Some(item) = self.block(shown) else { continue };
            for source in &item.entities {
                let placed = placement.place(outer.base, &inner.place(item.base, &source.geometry));
                out.push((source.clone(), placed));
            }
        }
        out
    }

    /// Drops an anonymous block nothing refers to any more.
    pub fn remove_unused_block(&mut self, name: &str) {
        let refers = |g: &Geometry| matches!(g, Geometry::Insert { block, .. } if block.eq_ignore_ascii_case(name));
        let used = self.all_entities().any(|e| {
            refers(&e.geometry)
                || e.array.as_ref().is_some_and(|a| a.item == name || a.replaced.iter().any(|(_, b)| b == name))
        }) || self.blocks().any(|b| b.entities.iter().any(|be| refers(&be.geometry)));
        if !used && name.starts_with('*') {
            self.remove_block(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle_doc() -> (Document, EntityId) {
        let mut d = Document::new();
        d.begin("x");
        let c = d.add(Geometry::Circle { center: Point::new(0.0, 0.0), radius: 1.0 });
        d.commit();
        (d, c)
    }

    fn centers(d: &Document, id: EntityId) -> Vec<(i64, i64)> {
        let e = d.entity(id).unwrap();
        let mut v: Vec<(i64, i64)> = d
            .expanded(e)
            .iter()
            .filter_map(|g| match g {
                Geometry::Circle { center, .. } => Some((center.x.round() as i64, center.y.round() as i64)),
                _ => None,
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn rectangular_array_is_one_object_and_edits() {
        let (mut d, c) = circle_doc();
        d.begin("array");
        let layout = ArrayLayout::Rect { rows: 2, columns: 3, row_spacing: 10.0, column_spacing: 20.0 };
        let a = d.make_array(&[c], layout).unwrap();
        d.commit();
        assert_eq!(d.len(), 1);
        assert_eq!(centers(&d, a), [(0, 0), (0, 10), (20, 0), (20, 10), (40, 0), (40, 10)]);
        let old = match &d.entity(a).unwrap().geometry {
            Geometry::Insert { block, .. } => block.clone(),
            _ => unreachable!(),
        };
        d.begin("edit");
        let layout = ArrayLayout::Rect { rows: 1, columns: 2, row_spacing: 10.0, column_spacing: 5.0 };
        assert!(d.set_array_layout(a, layout));
        d.commit();
        assert_eq!(centers(&d, a), [(0, 0), (5, 0)]);
        assert!(d.block(&old).is_none(), "the old layout's block went");
        // Moved, it keeps its layout.
        d.begin("move");
        d.update(a, |e| e.geometry = e.geometry.translated(Point::new(100.0, 0.0)));
        d.commit();
        assert_eq!(centers(&d, a), [(100, 0), (105, 0)]);
        let objects = d.array_objects(d.entity(a).unwrap());
        assert_eq!(objects.len(), 2);
        d.undo();
        d.undo();
        assert_eq!(centers(&d, a), [(0, 0), (0, 10), (20, 0), (20, 10), (40, 0), (40, 10)]);
    }

    #[test]
    fn polar_and_path_layouts() {
        let polar = ArrayLayout::Polar { center: Point::new(0.0, 10.0), items: 4, fill: 360.0, rotate: true };
        let at: Vec<(i64, i64)> =
            polar.items(Point::ORIGIN).iter().map(|(p, _)| (p.x.round() as i64, p.y.round() as i64)).collect();
        assert_eq!(at, [(0, 0), (10, 10), (0, 20), (-10, 10)]);
        let line = Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(30.0, 0.0) };
        let path = ArrayLayout::Path { path: Box::new(line), items: 4, align: false };
        let xs: Vec<f64> = path.items(Point::ORIGIN).iter().map(|(p, _)| p.x).collect();
        assert_eq!(xs, [0.0, 10.0, 20.0, 30.0]);
    }

    #[test]
    fn a_path_array_follows_its_path() {
        let (mut d, c) = circle_doc();
        d.begin("path");
        let path = d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(30.0, 0.0) });
        let line = d.entity(path).unwrap().geometry.clone();
        let layout = ArrayLayout::Path { path: Box::new(line), items: 4, align: false };
        let a = d.make_array(&[c], layout).unwrap();
        d.commit();
        assert_eq!(centers(&d, a), [(0, 0), (10, 0), (20, 0), (30, 0)]);
        // The path grows and turns: the items go along it, in the same step.
        d.begin("stretch");
        d.modify(path, Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(0.0, 60.0) });
        d.commit();
        assert_eq!(centers(&d, a), [(0, 0), (0, 20), (0, 40), (0, 60)]);
        d.undo();
        assert_eq!(centers(&d, a), [(0, 0), (10, 0), (20, 0), (30, 0)]);
    }

    #[test]
    fn items_are_replaced_and_the_source_edited() {
        let (mut d, c) = circle_doc();
        d.begin("array");
        let layout = ArrayLayout::Rect { rows: 1, columns: 3, row_spacing: 10.0, column_spacing: 20.0 };
        let a = d.make_array(&[c], layout).unwrap();
        d.commit();
        // A square stands in for the middle item, its corner on the item's base.
        d.begin("replace");
        let square = d.add(Geometry::Polyline {
            vertices: [(50.0, 50.0), (52.0, 50.0), (52.0, 52.0), (50.0, 52.0)]
                .map(|(x, y)| crate::Vertex::at(Point::new(x, y)))
                .to_vec(),
            closed: true,
        });
        assert_eq!(d.array_item_at(a, Point::new(21.0, 0.0)), Some(1));
        assert!(d.replace_array_items(a, &[square], Point::new(50.0, 50.0), Some(1)));
        d.commit();
        assert_eq!(centers(&d, a), [(0, 0), (40, 0)], "the middle one isn't a circle now");
        let e = d.entity(a).unwrap();
        assert!(d.expanded(e).iter().any(|g| matches!(g, Geometry::Polyline { vertices, .. }
            if vertices[0].point.approx_eq(Point::new(20.0, 0.0)))));
        // Editing the source: a second circle in the item shows in every item but the replaced one.
        d.begin("source");
        d.begin_array_source_edit(a, 0).unwrap();
        d.add(Geometry::Circle { center: Point::new(0.0, 5.0), radius: 1.0 });
        d.close_refedit(true);
        d.commit();
        assert_eq!(centers(&d, a), [(0, 0), (0, 5), (40, 0), (40, 5)]);
        assert!(d.refedit().is_none());
        assert_eq!(
            d.entities().filter(|e| matches!(e.geometry, Geometry::Insert { .. })).count(),
            1,
            "no reference left"
        );
        d.begin("reset");
        assert!(d.reset_array_items(a));
        d.commit();
        assert_eq!(centers(&d, a), [(0, 0), (0, 5), (20, 0), (20, 5), (40, 0), (40, 5)]);
    }
}

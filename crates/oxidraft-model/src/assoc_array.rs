//! Associative arrays (ARRAYRECT, ARRAYPOLAR and ARRAYPATH while
//! ARRAYASSOCIATIVITY is on): one object, a block reference, whose
//! anonymous block holds a reference to the item block (the source
//! objects) at each item. ARRAYEDIT changes the layout and the block is
//! made again. Other programs see a block reference, as with AutoCAD's.

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

    /// An array's block: a reference to its item block at each item.
    fn array_block(&self, array: &AssocArray) -> Option<Block> {
        let item = self.block(&array.item)?;
        let entities = array
            .layout
            .items(item.base)
            .into_iter()
            .map(|(at, turn)| BlockEntity {
                layer: "0".into(),
                color: Color::ByBlock,
                linetype: linetype::BYBLOCK.into(),
                lineweight: Lineweight::ByBlock,
                geometry: Geometry::Insert {
                    block: item.name.clone(),
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
        let array = AssocArray { layout, item: item.name.clone() };
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

    /// Lays the array `id` out again by `layout` (ARRAYEDIT).
    pub fn set_array_layout(&mut self, id: EntityId, layout: ArrayLayout) -> bool {
        let Some(e) = self.entity(id) else { return false };
        let (Some(array), Geometry::Insert { block: old, .. }) = (e.array.as_deref(), &e.geometry) else {
            return false;
        };
        let (array, old) = (AssocArray { layout, item: array.item.clone() }, old.clone());
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
        true
    }

    /// The array's items as plain objects, placed where the reference
    /// puts them (EXPLODE): (source object, its geometry there).
    pub fn array_objects(&self, e: &Entity) -> Vec<(BlockEntity, Geometry)> {
        let (Some(array), Geometry::Insert { block, placement, .. }) = (e.array.as_deref(), &e.geometry) else {
            return Vec::new();
        };
        let (Some(outer), Some(item)) = (self.block(block), self.block(&array.item)) else { return Vec::new() };
        let mut out = Vec::new();
        for be in &outer.entities {
            let Geometry::Insert { placement: inner, .. } = &be.geometry else { continue };
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
        let used = self.all_entities().any(|e| refers(&e.geometry) || e.array.as_ref().is_some_and(|a| a.item == name))
            || self.blocks().any(|b| b.entities.iter().any(|be| refers(&be.geometry)));
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
}

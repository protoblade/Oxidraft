//! Where a space's objects are, so the few near the cursor or in the view
//! are found without looking at all the others: their bounds, in draw
//! order, packed into a tree (sort-tile-recursive). Built again when the
//! drawing changes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use oxidraft_geom::{Aabb, Point, curve::Seg};

use crate::entity::{Entity, EntityId, Geometry};
use crate::layout::Space;

/// Children per node.
const FAN: usize = 16;

/// One space's objects: in draw order, with their bounds.
pub(crate) struct SpatialIndex {
    ids: Vec<EntityId>,
    bounds: Vec<Aabb>,
    /// Objects found everywhere: without bounds that can be trusted (an
    /// annotative object draws at other scales somewhere else; an XLINE has
    /// no end).
    everywhere: Vec<u32>,
    /// Item numbers grouped leaf by leaf.
    order: Vec<u32>,
    /// From the leaves (over `order`) up to the root; each node's children
    /// are a run of the level below.
    levels: Vec<Vec<Node>>,
}

#[derive(Clone, Copy)]
struct Node {
    bounds: Aabb,
    start: u32,
    end: u32,
}

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    a.max.x >= b.min.x && a.min.x <= b.max.x && a.max.y >= b.min.y && a.min.y <= b.max.y
}

fn finite(b: &Aabb) -> bool {
    [b.min.x, b.min.y, b.max.x, b.max.y].iter().all(|v| v.is_finite())
}

/// Puts `items` in sort-tile-recursive order by their centers: in vertical
/// slabs left to right, bottom to top within each, so that each run of
/// `FAN` is a compact tile.
fn tile(items: &mut [u32], center: impl Fn(u32) -> Point) {
    let leaves = items.len().div_ceil(FAN);
    let slabs = (leaves as f64).sqrt().ceil().max(1.0) as usize;
    items.sort_by(|a, b| center(*a).x.total_cmp(&center(*b).x));
    for slab in items.chunks_mut(slabs * FAN) {
        slab.sort_by(|a, b| center(*a).y.total_cmp(&center(*b).y));
    }
}

fn union(nodes: impl Iterator<Item = Aabb>) -> Aabb {
    nodes.reduce(Aabb::union).unwrap_or(Aabb::from_points(Point::ORIGIN, Point::ORIGIN))
}

impl SpatialIndex {
    /// `items` in draw order, each with its bounds, or none when it's to be
    /// found everywhere.
    pub(crate) fn build(items: Vec<(EntityId, Option<Aabb>)>) -> Self {
        let mut ids = Vec::with_capacity(items.len());
        let mut bounds = Vec::with_capacity(items.len());
        let mut everywhere = Vec::new();
        let mut order = Vec::new();
        for (i, (id, b)) in items.into_iter().enumerate() {
            ids.push(id);
            match b.filter(finite) {
                Some(b) => {
                    bounds.push(b);
                    order.push(i as u32);
                }
                None => {
                    bounds.push(Aabb::from_points(Point::ORIGIN, Point::ORIGIN));
                    everywhere.push(i as u32);
                }
            }
        }
        tile(&mut order, |i| bounds[i as usize].center());
        let mut level: Vec<Node> = order
            .chunks(FAN)
            .enumerate()
            .map(|(n, run)| Node {
                bounds: union(run.iter().map(|i| bounds[*i as usize])),
                start: (n * FAN) as u32,
                end: (n * FAN + run.len()) as u32,
            })
            .collect();
        let mut levels = Vec::new();
        while level.len() > 1 {
            // This level in tile order, so each parent's children are a run.
            let mut at: Vec<u32> = (0..level.len() as u32).collect();
            tile(&mut at, |i| level[i as usize].bounds.center());
            level = at.iter().map(|i| level[*i as usize]).collect();
            let parents = level
                .chunks(FAN)
                .enumerate()
                .map(|(n, run)| Node {
                    bounds: union(run.iter().map(|c| c.bounds)),
                    start: (n * FAN) as u32,
                    end: (n * FAN + run.len()) as u32,
                })
                .collect();
            levels.push(std::mem::replace(&mut level, parents));
        }
        levels.push(level);
        Self { ids, bounds, everywhere, order, levels }
    }

    /// The objects whose bounds reach `area`, and those found everywhere,
    /// in draw order.
    pub(crate) fn query(&self, area: &Aabb) -> Vec<EntityId> {
        let mut found: Vec<u32> = self.everywhere.clone();
        let top = self.levels.len() - 1;
        let mut stack: Vec<(usize, u32)> = (0..self.levels[top].len() as u32).map(|i| (top, i)).collect();
        while let Some((level, i)) = stack.pop() {
            let node = self.levels[level][i as usize];
            if !overlaps(&node.bounds, area) {
                continue;
            }
            if level == 0 {
                let items = &self.order[node.start as usize..node.end as usize];
                found.extend(items.iter().filter(|i| overlaps(&self.bounds[**i as usize], area)));
            } else {
                stack.extend((node.start..node.end).map(|c| (level - 1, c)));
            }
        }
        found.sort_unstable();
        found.into_iter().map(|i| self.ids[i as usize]).collect()
    }
}

fn at(p: Point) -> Aabb {
    Aabb::from_points(p, p)
}

/// A curve's bounds with the centers of its arcs, which it snaps to.
fn reach(g: &Geometry) -> Aabb {
    let b = g.bounds();
    match g {
        Geometry::Line { .. }
        | Geometry::Circle { .. }
        | Geometry::Point { .. }
        | Geometry::Text { .. }
        | Geometry::Image { .. }
        | Geometry::Wipeout { .. }
        | Geometry::Spline(_) => b,
        Geometry::Polyline { vertices, .. } if vertices.iter().all(|v| v.bulge == 0.0) => b,
        Geometry::Ellipse(e) => b.union(at(e.center)),
        _ => g.segments().iter().fold(b, |b, s| match s {
            Seg::Arc(a) => b.union(at(a.center)),
            Seg::Line(_) => b,
        }),
    }
}

/// The bounds an object is found by: its curves, block contents and
/// clipping frame, the centers of its arcs, and a block reference's
/// insertion point (it snaps and picks at those too). None to be found
/// everywhere.
pub(crate) fn findable_bounds(doc: &crate::Document, e: &Entity) -> Option<Aabb> {
    if e.annotative.is_some() || e.geometry.is_infinite() {
        return None;
    }
    let mut parts = doc.expanded(e);
    parts.extend(doc.clip_frame(&e.geometry));
    let b = parts.iter().map(reach).reduce(Aabb::union).unwrap_or_else(|| e.geometry.bounds());
    Some(match &e.geometry {
        Geometry::Insert { placement, .. } => b.union(at(placement.position)),
        _ => b,
    })
}

/// The indexes of a drawing's spaces, for the revision they were made at.
/// A copy of the drawing starts without them.
#[derive(Default)]
pub(crate) struct SpatialCache(Mutex<(u64, HashMap<Space, Arc<SpatialIndex>>)>);

impl Clone for SpatialCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for SpatialCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SpatialCache")
    }
}

impl SpatialCache {
    /// The index of `space` at `revision`, made with `build` if there's none yet.
    pub(crate) fn get(&self, revision: u64, space: Space, build: impl FnOnce() -> SpatialIndex) -> Arc<SpatialIndex> {
        let mut cache = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.0 != revision {
            *cache = (revision, HashMap::new());
        }
        cache.1.entry(space).or_insert_with(|| Arc::new(build())).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64) -> Aabb {
        Aabb::from_points(Point::new(x, y), Point::new(x + 1.0, y + 1.0))
    }

    #[test]
    fn finds_what_reaches_an_area_in_draw_order() {
        // A 100 by 100 grid of unit squares, numbered in draw order, and one
        // object found everywhere.
        let mut items: Vec<(EntityId, Option<Aabb>)> = (0..10_000)
            .map(|n| {
                (EntityId(n + 1), Some(square(f64::from((n % 100) as u32) * 2.0, f64::from((n / 100) as u32) * 2.0)))
            })
            .collect();
        items.insert(500, (EntityId(99_999), None));
        let index = SpatialIndex::build(items.clone());
        let area = Aabb::from_points(Point::new(10.5, 20.5), Point::new(14.2, 22.0));
        let brute: Vec<EntityId> =
            items.iter().filter(|(_, b)| b.is_none_or(|b| overlaps(&b, &area))).map(|(id, _)| *id).collect();
        let found = index.query(&area);
        assert_eq!(found, brute);
        assert_eq!(found.len(), 1 + 3 * 2, "the squares at x 10, 12, 14 by y 20, 22, and the one everywhere");
        // Far away: only the one found everywhere.
        assert_eq!(index.query(&square(1e6, 1e6)), vec![EntityId(99_999)]);
        // An empty space.
        assert!(SpatialIndex::build(Vec::new()).query(&area).is_empty());
    }
}

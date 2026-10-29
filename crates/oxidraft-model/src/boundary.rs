//! Finding the hatch boundary around a picked point (HATCH's "Pick internal
//! point"), like AutoCAD's BOUNDARY detection.
//!
//! Every curve is split where it meets another, giving a planar graph. Walking
//! its faces (always taking the next edge clockwise) yields closed loops; the
//! smallest counterclockwise loop around the point is the boundary. Shapes not
//! connected to it but lying inside (a column in a room) become islands.

use std::collections::HashMap;

use oxidraft_geom::{EPSILON, Point, curve::Seg};

use crate::{
    Document, Geometry,
    entity::Vertex,
    hatch::{contains, signed_area},
};

struct HalfEdge {
    origin: usize,
    dest: usize,
    seg: Seg,
    /// Direction of travel leaving `origin`, in degrees.
    angle: f64,
}

struct Face {
    edges: Vec<usize>,
    polygon: Vec<Point>,
    area: f64,
    component: usize,
}

/// Merges endpoints that are within `tol` of each other.
struct Vertices {
    points: Vec<Point>,
    grid: HashMap<(i64, i64), Vec<usize>>,
    tol: f64,
}

impl Vertices {
    fn id(&mut self, p: Point) -> usize {
        let cell = ((p.x / self.tol).floor() as i64, (p.y / self.tol).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &i in self.grid.get(&(cell.0 + dx, cell.1 + dy)).into_iter().flatten() {
                    if self.points[i].distance(p) <= self.tol {
                        return i;
                    }
                }
            }
        }
        self.points.push(p);
        self.grid.entry(cell).or_default().push(self.points.len() - 1);
        self.points.len() - 1
    }
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut j = i;
    while parent[j] != r {
        let next = parent[j];
        parent[j] = r;
        j = next;
    }
    r
}

/// The curves that can bound a hatch: shown lines, arcs, circles and
/// polylines (block contents included), not text, dimensions or hatches.
fn boundary_curves(doc: &Document) -> Vec<Seg> {
    doc.entities()
        .filter(|e| doc.object_shown(e))
        .filter(|e| {
            !matches!(
                e.geometry,
                Geometry::Text { .. }
                    | Geometry::Dimension { .. }
                    | Geometry::RadialDimension { .. }
                    | Geometry::AngularDimension { .. }
                    | Geometry::Leader { .. }
                    | Geometry::Hatch { .. }
            )
        })
        .flat_map(|e| doc.expanded(e))
        .flat_map(|g| g.segments())
        .filter(|s| s.length() > EPSILON)
        .collect()
}

fn tessellate(segs: &[Seg]) -> Vec<Point> {
    let mut pts = Vec::new();
    for seg in segs {
        let steps = match seg {
            Seg::Line(_) => 1,
            Seg::Arc(a) => (a.sweep.abs() / 5.0).ceil().max(1.0) as usize,
        };
        pts.extend((0..steps).map(|i| seg.point_at(i as f64 / steps as f64)));
    }
    pts
}

/// The boundary loops around `p`: the enclosing loop first, then islands.
/// `None` if `p` isn't enclosed.
pub fn boundary_at(doc: &Document, p: Point) -> Option<Vec<Vec<Vertex>>> {
    let curves = boundary_curves(doc);
    let size = doc.extents().map_or(1.0, |b| b.width().max(b.height()).max(1.0));
    let tol = size * 1e-9;

    // Split each curve at every crossing; closed circles also at their halves
    // so no edge starts and ends at the same vertex.
    let mut pieces: Vec<Seg> = Vec::new();
    for (i, seg) in curves.iter().enumerate() {
        let mut cuts = vec![0.0, 1.0];
        if matches!(seg, Seg::Arc(a) if a.is_full()) {
            cuts.extend([0.5]);
        }
        for (j, other) in curves.iter().enumerate() {
            if i != j {
                cuts.extend(seg.intersections(other).into_iter().map(|(t, _)| t));
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        pieces.extend(cuts.windows(2).map(|w| seg.sub(w[0], w[1])).filter(|s| s.length() > tol * 10.0));
    }

    let mut verts = Vertices { points: Vec::new(), grid: HashMap::new(), tol: tol * 1000.0 };
    let mut half: Vec<HalfEdge> = Vec::new();
    let mut seen: HashMap<(usize, usize, i64, i64), ()> = HashMap::new();
    for seg in pieces {
        let (a, b) = (verts.id(seg.start()), verts.id(seg.end()));
        if a == b {
            continue;
        }
        // Drop exact duplicates (overlapping objects).
        let mid = seg.point_at(0.5);
        let key = (a.min(b), a.max(b), (mid.x / verts.tol).round() as i64, (mid.y / verts.tol).round() as i64);
        if seen.insert(key, ()).is_some() {
            continue;
        }
        let rev = seg.reversed();
        half.push(HalfEdge { origin: a, dest: b, seg, angle: Point::ORIGIN.angle_to(seg.tangent_at(0.0)) });
        half.push(HalfEdge { origin: b, dest: a, seg: rev, angle: Point::ORIGIN.angle_to(rev.tangent_at(0.0)) });
    }
    if half.is_empty() {
        return None;
    }

    // Outgoing half-edges around each vertex, counterclockwise.
    let mut around: Vec<Vec<usize>> = vec![Vec::new(); verts.points.len()];
    for (h, e) in half.iter().enumerate() {
        around[e.origin].push(h);
    }
    for list in &mut around {
        list.sort_by(|&a, &b| half[a].angle.total_cmp(&half[b].angle));
    }
    let twin = |h: usize| h ^ 1;
    // Arriving along h, leave by the edge just clockwise of h's twin: this
    // keeps the face on the left.
    let next = |h: usize| {
        let at = &around[half[h].dest];
        let i = at.iter().position(|&x| x == twin(h)).unwrap_or(0);
        at[(i + at.len() - 1) % at.len()]
    };

    let mut parent: Vec<usize> = (0..verts.points.len()).collect();
    for e in &half {
        let (a, b) = (find(&mut parent, e.origin), find(&mut parent, e.dest));
        parent[a] = b;
    }

    let mut used = vec![false; half.len()];
    let mut faces = Vec::new();
    for start in 0..half.len() {
        if used[start] {
            continue;
        }
        let mut edges = Vec::new();
        let mut h = start;
        while !used[h] && edges.len() <= half.len() {
            used[h] = true;
            edges.push(h);
            h = next(h);
        }
        let segs: Vec<Seg> = edges.iter().map(|&h| half[h].seg).collect();
        let polygon = tessellate(&segs);
        let area = signed_area(&polygon);
        let component = find(&mut parent, half[edges[0]].origin);
        faces.push(Face { edges, polygon, area, component });
    }

    let min_area = tol * size;
    let outer = faces
        .iter()
        .filter(|f| f.area > min_area && contains(std::slice::from_ref(&f.polygon), p))
        .min_by(|a, b| a.area.total_cmp(&b.area))?;
    let to_loop = |f: &Face| -> Vec<Vertex> {
        f.edges.iter().map(|&h| Vertex::new(half[h].seg.start(), half[h].seg.bulge())).collect()
    };
    let mut loops = vec![to_loop(outer)];
    // Islands: the outside of every other shape that sits inside the boundary.
    for f in &faces {
        if f.area < -min_area
            && f.component != outer.component
            && contains(std::slice::from_ref(&outer.polygon), f.polygon[0])
        {
            loops.push(to_loop(f));
        }
    }
    Some(loops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hatch::polygons;

    fn line(d: &mut Document, a: (f64, f64), b: (f64, f64)) {
        d.add(Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) });
    }

    fn area(loops: &[Vec<Vertex>]) -> f64 {
        // Even-odd area: outer minus islands.
        let polys = polygons(loops);
        let a: Vec<f64> = polys.iter().map(|p| signed_area(p).abs()).collect();
        a[0] - a[1..].iter().sum::<f64>()
    }

    #[test]
    fn finds_a_room_made_of_separate_lines() {
        let mut d = Document::new();
        // Two rooms sharing a wall; the walls overshoot like real drafting.
        line(&mut d, (-1.0, 0.0), (21.0, 0.0));
        line(&mut d, (0.0, -1.0), (0.0, 11.0));
        line(&mut d, (-1.0, 10.0), (21.0, 10.0));
        line(&mut d, (20.0, -1.0), (20.0, 11.0));
        line(&mut d, (10.0, 0.0), (10.0, 10.0));
        let loops = boundary_at(&d, Point::new(15.0, 5.0)).unwrap();
        assert_eq!(loops.len(), 1);
        assert!((area(&loops) - 100.0).abs() < 1e-6);
        assert!(boundary_at(&d, Point::new(30.0, 5.0)).is_none(), "outside everything");
    }

    #[test]
    fn circles_inside_become_islands() {
        let mut d = Document::new();
        d.add(Geometry::Polyline {
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
                .iter()
                .map(|&(x, y)| Vertex::at(Point::new(x, y)))
                .collect(),
            closed: true,
        });
        d.add(Geometry::Circle { center: Point::new(3.0, 3.0), radius: 1.0 });
        d.add(Geometry::Circle { center: Point::new(7.0, 7.0), radius: 1.0 });
        let loops = boundary_at(&d, Point::new(5.0, 2.0)).unwrap();
        assert_eq!(loops.len(), 3, "the square plus two column islands");
        assert!((area(&loops) - (100.0 - 2.0 * std::f64::consts::PI)).abs() < 0.05);

        // Picking inside a column hatches just the column.
        let loops = boundary_at(&d, Point::new(3.0, 3.0)).unwrap();
        assert_eq!(loops.len(), 1);
        assert!((area(&loops) - std::f64::consts::PI).abs() < 0.02);
    }

    #[test]
    fn arcs_close_a_boundary() {
        let mut d = Document::new();
        line(&mut d, (0.0, 0.0), (10.0, 0.0));
        d.add(Geometry::Arc { center: Point::new(5.0, 0.0), radius: 5.0, start_angle: 0.0, end_angle: 180.0 });
        let loops = boundary_at(&d, Point::new(5.0, 2.0)).unwrap();
        assert!((area(&loops) - std::f64::consts::PI * 12.5).abs() < 0.1);
    }
}

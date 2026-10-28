//! Hatch patterns and the geometry behind drawing them: point-in-area tests,
//! pattern lines clipped to the boundary, and triangles for solid fills.
//! Boundaries are filled even-odd, so islands inside islands alternate
//! (AutoCAD's "Normal" island style).

use oxidraft_geom::{Point, curve::Seg};

use crate::entity::Vertex;

/// The predefined patterns Oxidraft knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HatchPattern {
    Solid,
    /// 45° lines (iron, brick, general section).
    #[default]
    Ansi31,
    /// 45° crosshatch.
    Ansi37,
    /// Horizontal lines.
    Line,
    /// Square grid.
    Net,
}

/// Line spacing of the line patterns at scale 1 (1/8", as in acad.pat).
const SPACING: f64 = 3.175;

impl HatchPattern {
    pub const ALL: [HatchPattern; 5] =
        [HatchPattern::Solid, HatchPattern::Ansi31, HatchPattern::Ansi37, HatchPattern::Line, HatchPattern::Net];

    pub fn name(self) -> &'static str {
        match self {
            HatchPattern::Solid => "SOLID",
            HatchPattern::Ansi31 => "ANSI31",
            HatchPattern::Ansi37 => "ANSI37",
            HatchPattern::Line => "LINE",
            HatchPattern::Net => "NET",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name.trim()))
    }

    /// Line families as (angle in degrees, spacing at scale 1).
    pub fn families(self) -> &'static [(f64, f64)] {
        match self {
            HatchPattern::Solid => &[],
            HatchPattern::Ansi31 => &[(45.0, SPACING)],
            HatchPattern::Ansi37 => &[(45.0, SPACING), (135.0, SPACING)],
            HatchPattern::Line => &[(0.0, SPACING)],
            HatchPattern::Net => &[(0.0, SPACING), (90.0, SPACING)],
        }
    }
}

/// Segments of one closed boundary loop.
pub fn loop_segments(vertices: &[Vertex]) -> Vec<Seg> {
    let n = vertices.len();
    (0..n).map(|i| Seg::from_bulge(vertices[i].point, vertices[(i + 1) % n].point, vertices[i].bulge)).collect()
}

/// Boundary loops as polygons, with arcs split into short chords.
pub fn polygons(loops: &[Vec<Vertex>]) -> Vec<Vec<Point>> {
    loops
        .iter()
        .map(|l| {
            let mut pts = Vec::new();
            for seg in loop_segments(l) {
                let steps = match seg {
                    Seg::Line(_) => 1,
                    Seg::Arc(a) => (a.sweep.abs() / 5.0).ceil().max(1.0) as usize,
                };
                pts.extend((0..steps).map(|i| seg.point_at(i as f64 / steps as f64)));
            }
            pts
        })
        .filter(|p: &Vec<Point>| p.len() >= 3)
        .collect()
}

/// Even-odd point-in-area test.
pub fn contains(polys: &[Vec<Point>], p: Point) -> bool {
    let mut inside = false;
    for poly in polys {
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
                inside = !inside;
            }
        }
    }
    inside
}

/// Signed area of a polygon (positive when counterclockwise).
pub fn signed_area(poly: &[Point]) -> f64 {
    (0..poly.len()).map(|i| poly[i].cross(poly[(i + 1) % poly.len()])).sum::<f64>() / 2.0
}

/// The pattern's lines clipped to the boundary, or `None` when there would be
/// more than `max_lines` of them (AutoCAD refuses such dense hatches too).
pub fn pattern_lines(
    polys: &[Vec<Point>],
    pattern: HatchPattern,
    scale: f64,
    angle: f64,
    max_lines: usize,
) -> Option<Vec<(Point, Point)>> {
    let mut out = Vec::new();
    for &(family_angle, spacing) in pattern.families() {
        let a = family_angle + angle;
        let spacing = spacing * scale;
        if spacing <= 0.0 {
            return None;
        }
        // Rotate the boundary so this family's lines are horizontal.
        let rotated: Vec<Vec<Point>> =
            polys.iter().map(|p| p.iter().map(|q| q.rotated_about(Point::ORIGIN, -a)).collect()).collect();
        let (lo, hi) =
            rotated.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        if !lo.is_finite() {
            return Some(out);
        }
        let first = (lo / spacing).ceil() as i64;
        let last = (hi / spacing).floor() as i64;
        if (last - first) as usize + out.len() > max_lines {
            return None;
        }
        for k in first..=last {
            let y = k as f64 * spacing;
            let mut xs: Vec<f64> = Vec::new();
            for poly in &rotated {
                for i in 0..poly.len() {
                    let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
                    if (p.y > y) != (q.y > y) {
                        xs.push(p.x + (y - p.y) * (q.x - p.x) / (q.y - p.y));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            for &[x0, x1] in xs.as_chunks::<2>().0 {
                let back = |x: f64| Point::new(x, y).rotated_about(Point::ORIGIN, a);
                out.push((back(x0), back(x1)));
            }
        }
    }
    Some(out)
}

/// Triangles covering the boundary: the largest loop is the outside, the
/// others are holes.
pub fn triangles(polys: &[Vec<Point>]) -> Vec<[Point; 3]> {
    let Some(outer) =
        (0..polys.len()).max_by(|&a, &b| signed_area(&polys[a]).abs().total_cmp(&signed_area(&polys[b]).abs()))
    else {
        return Vec::new();
    };
    let mut order = vec![outer];
    order.extend((0..polys.len()).filter(|&i| i != outer));
    let mut coords = Vec::new();
    let mut holes = Vec::new();
    let mut points = Vec::new();
    for (k, &i) in order.iter().enumerate() {
        if k > 0 {
            holes.push(points.len());
        }
        for p in &polys[i] {
            coords.extend([p.x, p.y]);
            points.push(*p);
        }
    }
    let Ok(indices) = earcutr::earcut(&coords, &holes, 2) else { return Vec::new() };
    indices.as_chunks::<3>().0.iter().map(|&[a, b, c]| [points[a], points[b], points[c]]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, size: f64) -> Vec<Vertex> {
        [(x0, y0), (x0 + size, y0), (x0 + size, y0 + size), (x0, y0 + size)]
            .iter()
            .map(|&(x, y)| Vertex::at(Point::new(x, y)))
            .collect()
    }

    #[test]
    fn even_odd_with_an_island() {
        let polys = polygons(&[square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0)]);
        assert!(contains(&polys, Point::new(1.0, 1.0)));
        assert!(!contains(&polys, Point::new(5.0, 5.0)), "inside the island");
        assert!(!contains(&polys, Point::new(11.0, 5.0)));
    }

    #[test]
    fn horizontal_lines_skip_the_island() {
        let polys = polygons(&[square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0)]);
        // Scale 0.25 puts lines 0.79375 apart, so some cross the island at y 4..6.
        let lines = pattern_lines(&polys, HatchPattern::Line, 0.25, 0.0, 1000).unwrap();
        assert!(lines.iter().all(|(a, b)| (a.y - b.y).abs() < 1e-9));
        let crossing: Vec<_> = lines.iter().filter(|(a, _)| a.y > 4.0 && a.y < 6.0).collect();
        assert_eq!(crossing.len(), 4, "two rows, each split in two by the island");
        assert!(crossing.iter().all(|(a, b)| b.x <= 4.0 + 1e-9 || a.x >= 6.0 - 1e-9));
        assert!(pattern_lines(&polys, HatchPattern::Line, 0.001, 0.0, 1000).is_none(), "too dense");
    }

    #[test]
    fn solid_triangles_cover_the_area_minus_holes() {
        let polys = polygons(&[square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0)]);
        let area: f64 = triangles(&polys).iter().map(|t| signed_area(t).abs()).sum();
        assert!((area - 96.0).abs() < 1e-6);
    }

    #[test]
    fn pattern_names() {
        assert_eq!(HatchPattern::from_name("ansi31"), Some(HatchPattern::Ansi31));
        assert_eq!(HatchPattern::from_name("bogus"), None);
    }
}

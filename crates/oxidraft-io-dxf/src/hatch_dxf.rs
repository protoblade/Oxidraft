//! HATCH entities, which the dxf crate doesn't know: read from the file's
//! text, and written by swapping placeholder POINTs (which the crate gives
//! handles and owners) for HATCH data.

use oxidraft_geom::{EPSILON, Point};
use oxidraft_model::{Color, Geometry, HatchPattern, Lineweight, Vertex, linetype};

use crate::{color_from_raw, eol, linetype_from_dxf, lineweight_from_dxf};

/// A HATCH read from the file, with where it belongs.
pub(crate) struct RawHatch {
    /// Its handle, which orders it among the other objects, and how many
    /// objects come before it in its section.
    pub handle: u64,
    pub position: usize,
    /// The block it's in, or `None` for the drawing itself.
    pub block: Option<String>,
    pub paper_space: bool,
    pub layer: String,
    pub color: Color,
    pub linetype: String,
    pub lineweight: Lineweight,
    pub geometry: Geometry,
}

/// Steps through one entity's (code, value) pairs.
struct Cursor<'a> {
    pairs: &'a [(i32, &'a str)],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<i32> {
        self.pairs.get(self.at).map(|p| p.0)
    }

    /// The value if the next pair has `code`.
    fn take(&mut self, code: i32) -> Option<&'a str> {
        let (c, v) = *self.pairs.get(self.at)?;
        (c == code).then(|| {
            self.at += 1;
            v
        })
    }

    fn num(&mut self, code: i32) -> Option<f64> {
        self.take(code)?.parse().ok()
    }

    fn int(&mut self, code: i32) -> Option<i64> {
        self.take(code)?.parse().ok()
    }

    /// Skips to the next pair with `code` and takes it.
    fn find(&mut self, code: i32) -> Option<&'a str> {
        while let Some(c) = self.peek() {
            if c == code {
                return self.take(code);
            }
            self.at += 1;
        }
        None
    }

    fn point(&mut self, x: i32) -> Option<Point> {
        Some(Point::new(self.num(x)?, self.num(x + 10)?))
    }
}

/// One edge of an edge-defined boundary path, as polyline vertices (the
/// edge's end is the next edge's start).
fn edge(c: &mut Cursor) -> Option<Vec<Vertex>> {
    match c.int(72)? {
        1 => {
            let start = c.point(10)?;
            c.point(11)?;
            Some(vec![Vertex::at(start)])
        }
        2 => {
            let (center, radius) = (c.point(10)?, c.num(40)?);
            let (s, e) = (c.num(50)?, c.num(51)?);
            let ccw = c.int(73).unwrap_or(1) != 0;
            // Clockwise arcs store their angles negated.
            let mut span = (e - s).rem_euclid(360.0);
            if span <= EPSILON {
                span = 360.0;
            }
            let (start, sweep) = if ccw { (s, span) } else { (-s, -span) };
            Some(vec![Vertex::new(center.polar(radius, start), (sweep.to_radians() / 4.0).tan())])
        }
        3 => {
            let (center, major) = (c.point(10)?, c.point(11)?);
            let (ratio, s, e) = (c.num(40)?, c.num(50)?, c.num(51)?);
            let ccw = c.int(73).unwrap_or(1) != 0;
            let minor = Point::new(-major.y, major.x) * ratio;
            let mut span = (e - s).rem_euclid(360.0);
            if span <= EPSILON {
                span = 360.0;
            }
            let (start, sweep) = if ccw { (s, span) } else { (-s, -span) };
            let steps = 32;
            Some(
                (0..steps)
                    .map(|i| {
                        let t = (start + sweep * f64::from(i) / f64::from(steps)).to_radians();
                        Vertex::at(center + major * t.cos() + minor * t.sin())
                    })
                    .collect(),
            )
        }
        // A spline, approximated by its fit points or else its control points.
        4 => {
            c.int(94)?;
            c.int(73)?;
            c.int(74)?;
            let knots = c.int(95)?;
            c.int(96)?;
            for _ in 0..knots {
                c.num(40)?;
            }
            let mut control = Vec::new();
            while let Some(code) = c.peek().filter(|code| [10, 42].contains(code)) {
                if code == 10 {
                    control.push(c.point(10)?);
                } else {
                    c.num(42);
                }
            }
            let mut fit = Vec::new();
            if let Some(n) = c.int(97) {
                for _ in 0..n {
                    fit.push(c.point(11)?);
                }
                c.point(12);
                c.point(13);
            }
            let mut points = if fit.len() >= 2 { fit } else { control };
            points.pop();
            Some(points.into_iter().map(Vertex::at).collect())
        }
        _ => None,
    }
}

/// The boundary loops, pattern, scale and angle of one HATCH's AcDbHatch data.
fn hatch_geometry(c: &mut Cursor, name: &str, solid: bool) -> Option<Geometry> {
    let paths = c.find(91)?.parse::<usize>().ok()?;
    let mut loops = Vec::new();
    for _ in 0..paths {
        let flags = c.find(92)?.parse::<i64>().ok()?;
        let mut vertices = Vec::new();
        if flags & 2 != 0 {
            let bulges = c.int(72)? != 0;
            c.int(73)?;
            for _ in 0..c.int(93)? {
                let p = c.point(10)?;
                let bulge = if bulges { c.num(42).unwrap_or(0.0) } else { 0.0 };
                vertices.push(Vertex::new(p, bulge));
            }
        } else {
            for _ in 0..c.int(93)? {
                vertices.extend(edge(c)?);
            }
        }
        if let Some(n) = c.int(97) {
            for _ in 0..n {
                c.take(330);
            }
        }
        if vertices.len() >= 2 {
            loops.push(vertices);
        }
    }
    if loops.is_empty() {
        return None;
    }
    let (mut scale, mut angle) = (1.0, 0.0);
    while let Some(code) = c.peek() {
        match code {
            52 => angle = c.num(52).unwrap_or(0.0),
            41 => scale = c.num(41).unwrap_or(1.0),
            _ => c.at += 1,
        }
    }
    // Patterns Oxidraft doesn't have show as ANSI31 at their scale and angle.
    let pattern = if solid { HatchPattern::Solid } else { HatchPattern::from_name(name).unwrap_or_default() };
    Some(Geometry::Hatch { loops, pattern, scale: if scale > 0.0 { scale } else { 1.0 }, angle })
}

/// Every HATCH in a text DXF file.
pub(crate) fn read_hatches(dxf: &[u8]) -> Vec<RawHatch> {
    if dxf.starts_with(b"AutoCAD Binary DXF") {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.lines().collect();
    let pairs: Vec<(i32, &str)> =
        lines.chunks(2).filter_map(|p| Some((p[0].trim().parse().ok()?, p.get(1)?.trim()))).collect();
    let mut out = Vec::new();
    let (mut section, mut block): (&str, Option<String>) = ("", None);
    let mut position = 0;
    let mut i = 0;
    while i < pairs.len() {
        let (code, value) = pairs[i];
        i += 1;
        if code != 0 {
            continue;
        }
        let end = pairs[i..].iter().position(|p| p.0 == 0).map_or(pairs.len(), |n| i + n);
        let body = &pairs[i..end];
        match value {
            "SECTION" => {
                section = body.iter().find(|p| p.0 == 2).map_or("", |p| p.1);
                position = 0;
            }
            "BLOCK" => block = body.iter().find(|p| p.0 == 2).map(|p| p.1.to_string()),
            "ENDBLK" => block = None,
            "HATCH" if section == "ENTITIES" || section == "BLOCKS" => {
                let mut h = RawHatch {
                    handle: 0,
                    position,
                    block: if section == "BLOCKS" { block.clone() } else { None },
                    paper_space: false,
                    layer: "0".into(),
                    color: Color::ByLayer,
                    linetype: linetype::BYLAYER.into(),
                    lineweight: Lineweight::ByLayer,
                    geometry: Geometry::Hatch {
                        loops: Vec::new(),
                        pattern: HatchPattern::Solid,
                        scale: 1.0,
                        angle: 0.0,
                    },
                };
                let (mut name, mut solid) = ("", false);
                // Common entity data and the pattern name come before the boundary.
                for &(c, v) in body.iter().take_while(|p| p.0 != 91) {
                    match c {
                        5 => h.handle = u64::from_str_radix(v, 16).unwrap_or(0),
                        8 => h.layer = v.to_string(),
                        6 => h.linetype = linetype_from_dxf(v),
                        62 => h.color = color_from_raw(v.parse().unwrap_or(256)),
                        370 => h.lineweight = lineweight_from_dxf(v.parse().unwrap_or(0)),
                        67 => h.paper_space = v == "1",
                        2 => name = v,
                        70 => solid = v == "1",
                        _ => {}
                    }
                }
                if let Some(g) = hatch_geometry(&mut Cursor { pairs: body, at: 0 }, name, solid) {
                    h.geometry = g;
                    out.push(h);
                }
            }
            "ENDSEC" | "SEQEND" | "VERTEX" | "ATTRIB" => {}
            _ if section == "ENTITIES" => position += 1,
            _ => {}
        }
        if value == "HATCH" && section == "ENTITIES" {
            position += 1;
        }
        i = end;
    }
    out
}

/// A float as DXF text.
fn num(v: f64) -> String {
    let s = format!("{v}");
    if s.contains(['.', 'e', 'E']) { s } else { format!("{s}.0") }
}

/// The AcDbHatch data of a hatch: polyline boundary paths and, for line
/// patterns, the pattern lines scaled and rotated into drawing units.
pub(crate) fn hatch_data(g: &Geometry) -> Vec<(i32, String)> {
    let Geometry::Hatch { loops, pattern, scale, angle } = g else { return Vec::new() };
    let solid = *pattern == HatchPattern::Solid;
    let mut out: Vec<(i32, String)> = vec![
        (100, "AcDbHatch".into()),
        (10, num(0.0)),
        (20, num(0.0)),
        (30, num(0.0)),
        (210, num(0.0)),
        (220, num(0.0)),
        (230, num(1.0)),
        (2, pattern.name().into()),
        (70, i32::from(solid).to_string()),
        (71, "0".into()),
        (91, loops.len().to_string()),
    ];
    for l in loops {
        let bulges = l.iter().any(|v| v.bulge.abs() > 1e-12);
        out.extend([
            (92, "2".into()),
            (72, i32::from(bulges).to_string()),
            (73, "1".into()),
            (93, l.len().to_string()),
        ]);
        for v in l {
            out.extend([(10, num(v.point.x)), (20, num(v.point.y))]);
            if bulges {
                out.push((42, num(v.bulge)));
            }
        }
        out.push((97, "0".into()));
    }
    out.extend([(75, "0".into()), (76, "1".into())]);
    if !solid {
        out.extend([(52, num(*angle)), (41, num(*scale)), (77, "0".into())]);
        let families = pattern.families();
        out.push((78, families.len().to_string()));
        for &(family_angle, spacing) in families {
            let a = family_angle + angle;
            // The offset between lines, perpendicular to them.
            let offset = Point::ORIGIN.polar(spacing * scale, a + 90.0);
            out.extend([
                (53, num(a)),
                (43, num(0.0)),
                (44, num(0.0)),
                (45, num(offset.x)),
                (46, num(offset.y)),
                (79, "0".into()),
            ]);
        }
    }
    out.push((98, "0".into()));
    out
}

/// The z of the POINT that stands in for a hatch until [`insert_hatches`]
/// swaps it; no 2D drawing puts a real point there.
pub(crate) const PLACEHOLDER_Z: f64 = -7.0e7;

/// Swaps each placeholder POINT for a HATCH. The crate writes the POINT's
/// handle, owner and common entity data; its X coordinate is the index into
/// `hatches` of the AcDbHatch data that replaces its own.
pub(crate) fn insert_hatches(dxf: &[u8], hatches: &[Vec<(i32, String)>]) -> Vec<u8> {
    if hatches.is_empty() {
        return dxf.to_vec();
    }
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let pairs: Vec<&[&str]> = lines.chunks(2).collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < pairs.len() {
        let pair = pairs[i];
        let is_point = pair[0].trim() == "0" && pair.get(1).is_some_and(|v| v.trim() == "POINT");
        if !is_point {
            pair.iter().for_each(|l| out.push_str(l));
            i += 1;
            continue;
        }
        let end = (i + 1..pairs.len()).find(|&j| pairs[j][0].trim() == "0").unwrap_or(pairs.len());
        let entity = &pairs[i..end];
        let subclass =
            entity.iter().position(|p| p[0].trim() == "100" && p.get(1).is_some_and(|v| v.trim() == "AcDbPoint"));
        let value =
            |code: &str| entity.iter().find(|p| p[0].trim() == code).and_then(|p| p.get(1)?.trim().parse::<f64>().ok());
        let index = value("10").filter(|_| value("30") == Some(PLACEHOLDER_Z)).map(|x| x as usize);
        let (Some(subclass), Some(data)) = (subclass, index.and_then(|n| hatches.get(n))) else {
            entity.iter().for_each(|p| p.iter().for_each(|l| out.push_str(l)));
            i = end;
            continue;
        };
        let end_of_line = eol(pair[0]);
        out.push_str(pair[0]);
        out.push_str(&format!("HATCH{end_of_line}"));
        for p in &entity[1..subclass] {
            p.iter().for_each(|l| out.push_str(l));
        }
        for (code, value) in data {
            out.push_str(&format!("{code:>3}{end_of_line}{value}{end_of_line}"));
        }
        i = end;
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_edge_boundaries_with_clockwise_arcs() {
        // A 10x10 square whose top edge is a clockwise half circle, stored with
        // negated angles: from 0° clockwise to -180°, dipping into the square.
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nHATCH\n8\nFill\n62\n3\n100\nAcDbHatch\n2\nSOLID\n70\n1\n91\n1\n92\n1\n93\n4\n\
72\n1\n10\n0\n20\n0\n11\n10\n21\n0\n\
72\n1\n10\n10\n20\n0\n11\n10\n21\n10\n\
72\n2\n10\n5\n20\n10\n40\n5\n50\n0\n51\n180\n73\n0\n\
72\n1\n10\n0\n20\n10\n11\n0\n21\n0\n\
97\n0\n75\n0\n76\n1\n98\n0\n0\nENDSEC\n0\nEOF\n";
        let hatches = read_hatches(dxf.as_bytes());
        assert_eq!(hatches.len(), 1);
        let h = &hatches[0];
        assert_eq!((h.layer.as_str(), h.color, h.block.as_deref()), ("Fill", Color::Index(3), None));
        let Geometry::Hatch { loops, pattern: HatchPattern::Solid, .. } = &h.geometry else { panic!() };
        let l = &loops[0];
        assert_eq!(l.len(), 4);
        // The arc starts at (10,10) and turns clockwise: bulge -1.
        assert!(l[2].point.approx_eq(Point::new(10.0, 10.0)) && (l[2].bulge + 1.0).abs() < 1e-9, "{:?}", l[2]);
        let area = Geometry::Polyline { vertices: l.clone(), closed: true }.area().unwrap();
        assert!((area - (100.0 - std::f64::consts::PI * 12.5)).abs() < 1e-6, "{area}");
    }
}

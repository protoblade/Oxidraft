//! MicroStation DGN V8 files: a compound file whose first model's
//! elements (points, lines, line strings, shapes, curves, B-spline curves,
//! point strings, arcs, ellipses and text; complex chains and shapes as
//! polylines, where their parts meet; cells' components one by one; shared
//! cells as their definitions' elements, placed) come in
//! as DGNIMPORT and DGNATTACH bring V7's, a layer for each level by its
//! name. The layout is as ezdgn's V8 format notes set it out (MIT,
//! clean-room): zlib pages of objects, a model header with the units, a
//! common 0x68-byte element header, each element's fields after it.

use std::io::Read;

use oxidraft_geom::{Point, ellipse::Ellipse};
use oxidraft_io_dwg::compound_file::{
    BinaryRecord, CompoundEntry, CompoundStorage, CompoundStreamContent, StructuredStoragePayload,
};
use oxidraft_model::{Attach, Geometry, Placement, Vertex};

fn u16_at(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

fn f64_at(d: &[u8], i: usize) -> Option<f64> {
    Some(f64::from_le_bytes(d.get(i..i + 8)?.try_into().ok()?)).filter(|v| v.is_finite())
}

fn inflate(d: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(d).read_to_end(&mut out).ok()?;
    Some(out)
}

fn storage<'a>(s: &'a CompoundStorage, name: &str) -> Option<&'a CompoundStorage> {
    s.entries.iter().find_map(|e| match e {
        CompoundEntry::Storage(c) if c.name == name => Some(c),
        _ => None,
    })
}

fn stream(s: &CompoundStorage, name: &str) -> Option<Vec<u8>> {
    s.entries.iter().find_map(|e| match e {
        CompoundEntry::Stream(c) if c.name == name => Some(c.content.encode()),
        _ => None,
    })
}

/// A storage's numbered page streams (`$1`, `$2`, …), in order.
fn pages(s: &CompoundStorage) -> Vec<Vec<u8>> {
    let mut found: Vec<(u32, Vec<u8>)> = s
        .entries
        .iter()
        .filter_map(|e| match e {
            CompoundEntry::Stream(c) => Some((c.name.strip_prefix('$')?.parse().ok()?, c.content.encode())),
            _ => None,
        })
        .collect();
    found.sort_by_key(|(n, _)| *n);
    found.into_iter().map(|(_, d)| d).collect()
}

/// An object as a page has it: its type and flags, its bytes, and where
/// its attributes start.
type Raw = (u32, Vec<u8>, usize);

/// A page's objects: after its 16-byte header (the count first), zlib;
/// each object a 4-byte framing word, then its type and flags, its length
/// and its attribute boundary (both in 16-bit words): (type and flags,
/// the object, where its attributes start).
fn objects(page: &[u8]) -> Vec<Raw> {
    let (Some(count), Some(bytes)) = (u32_at(page, 0), page.get(16..).and_then(inflate)) else { return Vec::new() };
    let mut out = Vec::new();
    let mut at = 0;
    for _ in 0..count {
        let start = at + 4;
        let (Some(kind), Some(words), Some(attributes)) =
            (u32_at(&bytes, start), u32_at(&bytes, start + 4), u32_at(&bytes, start + 8))
        else {
            break;
        };
        let end = start + 2 * words as usize;
        let Some(object) = bytes.get(start..end) else { break };
        out.push((kind, object.to_vec(), (2 * attributes as usize).min(object.len())));
        at = end;
    }
    out
}

/// The model's units: its global origin (UOR) and master units per UOR,
/// from its type-66 header object.
fn units(header: &[u8]) -> Option<(Point, f64)> {
    let bytes = inflate(header)?;
    // The object ends the stream, after a zero word.
    let record = (0..bytes.len().saturating_sub(16)).step_by(4).find_map(|d| {
        let r = d + 4;
        let size = 2 * u32_at(&bytes, r + 4)? as usize;
        (u32_at(&bytes, d)? == 0 && u32_at(&bytes, r)? as u16 == 66 && r + size == bytes.len()).then(|| &bytes[r..])
    })?;
    let per_master = f64_at(record, 0xe0).filter(|v| *v > 0.0)?;
    Some((Point::new(f64_at(record, 0xc8)?, f64_at(record, 0xd0)?), 1.0 / per_master))
}

/// A level's name: its 0x56d2 linkage among `attributes` (its kind in the
/// signature's high half, its length in words less one in the low byte;
/// UTF-16 after `ff fd`, 8-bit after `ff fe 01 00` or plain).
fn level_name(attributes: &[u8]) -> Option<String> {
    let mut at = 0;
    while let Some(signature) = u32_at(attributes, at) {
        let len = 2 * ((signature & 0xff) as usize + 1);
        if signature >> 16 == 0x56d2 {
            let n = u32_at(attributes, at + 8)? as usize;
            let payload = attributes.get(at + 12..at + 12 + n)?;
            return Some(text_of(payload));
        }
        at += len;
    }
    None
}

/// Text as V8 keeps it: UTF-16 after `ff fd`, Windows-1252 after
/// `ff fe 01 00`, else UTF-8 or Windows-1252.
fn text_of(b: &[u8]) -> String {
    let latin = |b: &[u8]| b.iter().map(|c| char::from(*c)).collect::<String>();
    let s = if let Some(rest) = b.strip_prefix(&[0xff, 0xfd]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else if let Some(rest) = b.strip_prefix(&[0xff, 0xfe, 1, 0]) {
        latin(rest)
    } else {
        std::str::from_utf8(b).map_or_else(|_| latin(b), str::to_string)
    };
    s.trim_end_matches('\0').to_string()
}

/// The turn (degrees) about z of a 3D element's orientation quaternion.
fn quaternion_turn(q: [f64; 4]) -> f64 {
    (2.0 * q[3].atan2(q[0])).to_degrees()
}

/// One element as Oxidraft's geometry (a point string's points): `object`
/// its bytes up to its attributes; `to` its points in master units.
fn element(kind: u16, object: &[u8], scale: f64, to: &dyn Fn(Point) -> Point) -> Option<Vec<Geometry>> {
    if object.len() < 0x68 {
        return None;
    }
    let three_d = u32_at(object, 0x28)? & 0x800 != 0;
    let width = if three_d { 24 } else { 16 };
    let point = |at: usize| Some(to(Point::new(f64_at(object, at)?, f64_at(object, at + 8)?)));
    let points = || {
        let n = u32_at(object, 0x68)? as usize;
        (0..n).map(|k| point(0x70 + k * width)).collect::<Option<Vec<Point>>>()
    };
    let vertices = |closed: bool| {
        let mut points = points()?;
        if closed && points.len() > 2 && points[0].approx_eq(points[points.len() - 1]) {
            points.pop();
        }
        (points.len() >= 2)
            .then(|| Geometry::Polyline { vertices: points.into_iter().map(Vertex::at).collect(), closed })
    };
    // An ellipse or arc: its axes, turn and center, sweep from `start`.
    let conic = |axes: usize, turn_at: usize, center_at: usize, start: f64, sweep: f64| {
        let (a, b) = (f64_at(object, axes)? * scale, f64_at(object, axes + 8)? * scale);
        let turn = if three_d {
            quaternion_turn([
                f64_at(object, turn_at)?,
                f64_at(object, turn_at + 8)?,
                f64_at(object, turn_at + 16)?,
                f64_at(object, turn_at + 24)?,
            ])
        } else {
            f64_at(object, turn_at)?.to_degrees()
        };
        let center = point(center_at)?;
        let (from, to) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
        let full = sweep.abs() >= 360.0 - 1e-9;
        Some(if (a - b).abs() < 1e-9 * a.max(1.0) {
            if full {
                Geometry::Circle { center, radius: a }
            } else {
                Geometry::Arc {
                    center,
                    radius: a,
                    start_angle: (from + turn).rem_euclid(360.0),
                    end_angle: (to + turn).rem_euclid(360.0),
                }
            }
        } else {
            // The longer axis the major one, the angles from it.
            let (major, ratio, shift) = if a >= b {
                (Point::new(a, 0.0).rotated_about(Point::ORIGIN, turn), b / a, 0.0)
            } else {
                (Point::new(0.0, b).rotated_about(Point::ORIGIN, turn), a / b, 90.0)
            };
            let mut el = Ellipse::full(center, major, ratio);
            if !full {
                (el.start, el.end) = ((from - shift).rem_euclid(360.0), (to - shift).rem_euclid(360.0));
            }
            Geometry::Ellipse(el)
        })
    };
    let one = match kind {
        // A line of no length is a point.
        3 => {
            let (start, end) = (point(0x68)?, point(0x68 + width)?);
            if start.approx_eq(end) { Geometry::Point { position: start } } else { Geometry::Line { start, end } }
        }
        4 => vertices(false)?,
        6 => vertices(true)?,
        // A curve through its points, the first two and last two setting
        // its ends' tangents.
        11 => {
            let all = points()?;
            let inner = all.get(2..all.len().saturating_sub(2)).unwrap_or(&[]);
            match oxidraft_geom::spline::Spline::through(inner, false) {
                Some(s) if inner.len() >= 2 => Geometry::Spline(s),
                _ => vertices(false)?,
            }
        }
        // A point string: its points.
        22 => return Some(points()?.into_iter().map(|position| Geometry::Point { position }).collect()),
        15 => conic(0x68, 0x78, if three_d { 0x98 } else { 0x80 }, 0.0, 360.0)?,
        16 => {
            let (start, sweep) = (f64_at(object, 0x68)?.to_degrees(), f64_at(object, 0x70)?.to_degrees());
            conic(0x78, 0x88, if three_d { 0xa8 } else { 0x90 }, start, if sweep == 0.0 { 360.0 } else { sweep })?
        }
        17 => {
            let n = usize::from(u16_at(object, 0x6e)?);
            let at = if three_d { 0xca } else { 0xaa };
            let content = text_of(object.get(at..at + n)?);
            let height = f64_at(object, 0x78)? * 6.0 / 1000.0 * scale;
            let rotation = if three_d { 0.0 } else { f64_at(object, 0x90)?.to_degrees() };
            Geometry::Text {
                position: point(if three_d { 0xb0 } else { 0x98 })?,
                height,
                rotation,
                width: 0.0,
                attach: Attach::BottomLeft,
                content,
                look: Default::default(),
            }
        }
        _ => return None,
    };
    Some(vec![one])
}

/// An element's layer: its level's name.
fn layer_of(levels: &std::collections::HashMap<u32, String>, object: &[u8]) -> String {
    let level = u32_at(object, 0x0c).unwrap_or(0);
    levels.get(&level).cloned().unwrap_or_else(|| format!("Level {level}"))
}

/// An element's color, 0 where it's its level's (ByLevel, and ByCell).
fn color_of(object: &[u8]) -> u8 {
    match u32_at(object, 0x34).unwrap_or(0) {
        c if c >= 0xffff_fffe => 0,
        c => (c & 0xff) as u8,
    }
}

/// A B-spline curve of `order` through the poles of `object` (type 21),
/// its knots uniform (not clamped to its ends, as MicroStation's are).
fn bspline(object: &[u8], order: usize, to: &dyn Fn(Point) -> Point) -> Option<Geometry> {
    let width = if u32_at(object, 0x28)? & 0x800 != 0 { 24 } else { 16 };
    let n = u32_at(object, 0x68)? as usize;
    let poles = (0..n)
        .map(|k| Some(to(Point::new(f64_at(object, 0x70 + k * width)?, f64_at(object, 0x78 + k * width)?))))
        .collect::<Option<Vec<Point>>>()?;
    let knots = (0..n + order).map(|k| k as f64).collect();
    oxidraft_geom::spline::Spline::new(order - 1, poles, knots, Vec::new(), false).map(Geometry::Spline)
}

/// What a DGN V8 file's first model holds: (geometry in master units, its
/// level's name, its color; 0 where it's the level's).
pub fn read_dgn8(data: &[u8]) -> Result<Vec<(Geometry, String, u8)>, String> {
    let root = StructuredStoragePayload::decode(data).compound_file.ok_or("Not a readable DGN V8 file.")?.root;
    let models = storage(&root, "Dgn-Md").ok_or("This compound file isn't a DGN V8 drawing.")?;
    let mut storages: Vec<&CompoundStorage> = models
        .entries
        .iter()
        .filter_map(|e| match e {
            CompoundEntry::Storage(s) if s.name.starts_with('#') => Some(s),
            _ => None,
        })
        .collect();
    storages.sort_by(|a, b| a.name.cmp(&b.name));
    let model = storages.first().ok_or("The DGN V8 file has no model.")?;
    let (origin, scale) =
        stream(model, "Dgn~Mh").as_deref().and_then(units).ok_or("The DGN V8 model's units can't be read.")?;
    let to = |p: Point| Point::new((p.x - origin.x) * scale, (p.y - origin.y) * scale);
    // Levels' names, from the named objects (type 95, kind 1).
    let mut levels = std::collections::HashMap::new();
    for page in storage(&root, "Dgn^Nm").map(pages).unwrap_or_default() {
        for (kind, object, attributes) in objects(&page) {
            if kind as u16 == 95
                && u32_at(&object, 0x0c) == Some(1)
                && let (Some(id), Some(name)) = (u32_at(&object, 0x20), level_name(&object[attributes..]))
            {
                levels.insert(id, name);
            }
        }
    }
    // Shared cells' definitions (type 34, with the named objects): each
    // its components after it, in its own units.
    let named: Vec<Raw> =
        storage(&root, "Dgn^Nm").map(pages).unwrap_or_default().iter().flat_map(|p| objects(p)).collect();
    let mut cells = std::collections::HashMap::new();
    for (k, (kind, object, attributes)) in named.iter().enumerate() {
        if *kind as u16 != 34 {
            continue;
        }
        let Some(name) = level_name(&object[*attributes..]) else { continue };
        let parts: Vec<Raw> = named[k + 1..].iter().take_while(|(kind, ..)| kind & COMPONENT != 0).cloned().collect();
        let base = Point::new(f64_at(object, 0xe8).unwrap_or(0.0), f64_at(object, 0xf0).unwrap_or(0.0));
        cells.insert(name, (base, geometry(&parts, 1.0, &|p| p, &levels)));
    }
    let model: Vec<Raw> =
        storage(model, "Dgn^G").map(pages).unwrap_or_default().iter().flat_map(|p| objects(p)).collect();
    let mut out = geometry(&model, scale, &to, &levels);
    // Shared cells placed (type 35, 3D): their definition by name, its
    // transform (rows of a 3×3, in UORs per its unit) and origin.
    for (kind, object, attributes) in &model {
        if *kind as u16 != 35 || u32_at(object, 0x28).is_none_or(|f| f & 0x800 == 0) {
            continue;
        }
        let Some((base, parts)) = level_name(&object[*attributes..]).and_then(|n| cells.get(&n)) else { continue };
        let at = |o: usize| f64_at(object, o);
        let (Some(a), Some(b), Some(c), Some(d), Some(x), Some(y)) =
            (at(0xa0), at(0xa8), at(0xb8), at(0xc0), at(0xe8), at(0xf0))
        else {
            continue;
        };
        let mirror = a * d - b * c < 0.0;
        let (sx, sy) = if mirror { (-a, -c) } else { (a, c) };
        let placement = Placement {
            position: to(Point::new(x, y)),
            scale: a.hypot(c) * scale,
            rotation: sy.atan2(sx).to_degrees(),
            mirror,
        };
        out.extend(parts.iter().map(|(g, layer, color)| (placement.place(*base, g), layer.clone(), *color)));
    }
    Ok(out)
}

/// A type's flags for an element that's another's component (a complex
/// element's, a cell's, a shared cell definition's), and for one that has
/// components (its count at 0x68).
const COMPONENT: u32 = 0x4000_0000;
const HEADER: u32 = 0x2000_0000;

/// Elements as Oxidraft's geometry, with their levels' names and colors;
/// `scale` and `to` as for [`element`].
fn geometry(
    objects: &[Raw],
    scale: f64,
    to: &dyn Fn(Point) -> Point,
    levels: &std::collections::HashMap<u32, String>,
) -> Vec<(Geometry, String, u8)> {
    let mut out = Vec::new();
    let geometry_of = |(kind, object, attributes): &Raw| {
        element(*kind as u16, &object[..(*attributes).max(0x68).min(object.len())], scale, to)
    };
    // A B-spline curve's order, from its header (type 27), for its poles
    // (type 21) after it.
    let mut order = None;
    let mut k = 0;
    while let Some(raw @ (kind, object, _)) = objects.get(k) {
        k += 1;
        // A complex chain or shape (types 12 and 14): one polyline, where
        // its components (lines, line strings, arcs) run end to end; else
        // they come in one by one.
        if matches!(*kind as u16, 12 | 14) && kind & HEADER != 0 {
            let count = u32_at(object, 0x68).unwrap_or(0) as usize;
            let parts = objects.get(k..k + count).filter(|p| p.iter().all(|(c, ..)| c & COMPONENT != 0));
            let pieces: Option<Vec<Geometry>> =
                parts.and_then(|p| p.iter().map(geometry_of).collect::<Option<Vec<_>>>()).map(|v| v.concat());
            if let Some(g) = pieces.and_then(|p| joined(&p, *kind as u16 == 14)) {
                out.push((g, layer_of(levels, object), color_of(object)));
                k += count;
            }
            continue;
        }
        match *kind as u16 {
            27 => order = u32_at(object, 0x6c).map(|p| (p & 0x0f) as usize + 2),
            21 => {
                if let Some(order) = order.take()
                    && let Some(g) = bspline(object, order, to)
                {
                    out.push((g, layer_of(levels, object), color_of(object)));
                }
                continue;
            }
            _ => {}
        }
        let Some(parts) = geometry_of(raw) else { continue };
        let (layer, color) = (layer_of(levels, object), color_of(object));
        out.extend(parts.into_iter().map(|g| (g, layer.clone(), color)));
    }
    out
}

/// Lines, line strings and arcs that run end to end (each either way
/// round) as one polyline, `closed` as a shape is.
fn joined(pieces: &[Geometry], closed: bool) -> Option<Geometry> {
    let near = |a: Point, b: Point| a.distance(b) <= 1e-9 * (1.0 + a.x.abs().max(a.y.abs()));
    let (mut points, mut bulges): (Vec<Point>, Vec<f64>) = (Vec::new(), Vec::new());
    for g in pieces {
        // Its segments: (start, end, bulge).
        let mut segments: Vec<(Point, Point, f64)> = match g {
            Geometry::Line { start, end } => vec![(*start, *end, 0.0)],
            Geometry::Polyline { vertices, closed: false } => {
                vertices.windows(2).map(|w| (w[0].point, w[1].point, w[0].bulge)).collect()
            }
            Geometry::Arc { center, radius, start_angle, end_angle } => {
                let at = |a: f64| *center + Point::new(a.to_radians().cos(), a.to_radians().sin()) * *radius;
                let sweep = (end_angle - start_angle).rem_euclid(360.0);
                vec![(at(*start_angle), at(*end_angle), (sweep / 4.0).to_radians().tan())]
            }
            _ => return None,
        };
        let (first, last) = (segments.first()?.0, segments.last()?.1);
        match points.last() {
            None => points.push(first),
            Some(&p) if near(p, first) => {}
            // The other way round.
            Some(&p) if near(p, last) => {
                segments = segments.into_iter().rev().map(|(s, e, b)| (e, s, -b)).collect();
            }
            Some(_) => return None,
        }
        for (_, end, bulge) in segments {
            bulges.push(bulge);
            points.push(end);
        }
    }
    if points.len() < 2 {
        return None;
    }
    if closed && near(points[0], points[points.len() - 1]) {
        points.pop();
    }
    bulges.resize(points.len(), 0.0);
    let vertices = points.into_iter().zip(bulges).map(|(p, b)| Vertex::new(p, b)).collect();
    Some(Geometry::Polyline { vertices, closed })
}

/// The drawing V8 files are written on: GDAL's V8 test drawing (MIT/X,
/// `assets/dgn`), its header, settings and level table kept, its model's
/// elements replaced.
const SEED: &[u8] = include_bytes!("../assets/dgn/seed_v8.dgn");

fn storage_mut<'a>(s: &'a mut CompoundStorage, name: &str) -> Option<&'a mut CompoundStorage> {
    s.entries.iter_mut().find_map(|e| match e {
        CompoundEntry::Storage(c) if c.name == name => Some(c),
        _ => None,
    })
}

fn set_stream(s: &mut CompoundStorage, name: &str, data: &[u8]) {
    for e in &mut s.entries {
        if let CompoundEntry::Stream(c) = e
            && c.name == name
        {
            c.content = CompoundStreamContent::Binary(BinaryRecord::split(data, 4096));
        }
    }
}

fn deflate(d: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(d);
    e.finish().unwrap_or_default()
}

/// A page of `objects` (format 2, page 1), each after its framing word.
fn write_page(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for o in objects {
        body.extend(0u32.to_le_bytes());
        body.extend(o);
    }
    let n = objects.len() as u32;
    let mut out: Vec<u8> = [n, 2, 1, n].iter().flat_map(|v| v.to_le_bytes()).collect();
    out.extend(deflate(&body));
    out
}

/// An object made on `seed`: its header and `fields` after it, none of the
/// seed's linkages (a fill, say); its ID, level, color and range set, its
/// line style and weight 0.
fn made(seed: &Raw, fields: &[u8], id: u64, level: u32, color: u32, range: [i64; 4]) -> Vec<u8> {
    let mut o = seed.1[..0x68].to_vec();
    o.extend(fields);
    if o.len() % 2 == 1 {
        o.push(0);
    }
    let words = (o.len() / 2) as u32;
    let attribute_words = words;
    let mut put = |at: usize, v: &[u8]| o[at..at + v.len()].copy_from_slice(v);
    put(4, &words.to_le_bytes());
    put(8, &attribute_words.to_le_bytes());
    put(0x0c, &level.to_le_bytes());
    put(0x10, &id.to_le_bytes());
    put(0x2c, &0u32.to_le_bytes());
    put(0x30, &0u32.to_le_bytes());
    put(0x34, &color.to_le_bytes());
    for (k, v) in [range[0], range[1], 0, range[2], range[3], 0].into_iter().enumerate() {
        put(0x38 + 8 * k, &v.to_le_bytes());
    }
    o
}

/// A level's name as V8 keeps it: a 0x56d2 linkage, UTF-16 after `ff fd`.
fn name_linkage(name: &str) -> Vec<u8> {
    let payload: Vec<u8> = [0xff, 0xfd].into_iter().chain(name.encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let total = (12 + payload.len()).next_multiple_of(4);
    let mut out = ((0x56d2u32 << 16) | 0x1000 | (total / 2 - 1) as u32).to_le_bytes().to_vec();
    out.extend(1u32.to_le_bytes());
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend(payload);
    out.resize(total, 0);
    out
}

/// The drawing's model space as a DGN V8 file (2D elements, a drawing
/// unit a master unit): lines, line strings, shapes, circles, arcs,
/// ellipses and text, polylines with arcs as complex chains and shapes
/// (of lines and arcs), everything else as line strings; a level for each
/// layer (its name and color).
pub fn write_dgn8(doc: &oxidraft_model::Document) -> Result<Vec<u8>, String> {
    use oxidraft_model::Color;
    let lost = || "The DGN V8 seed can't be read.".to_string();
    let mut cfb = StructuredStoragePayload::decode(SEED).compound_file.ok_or_else(lost)?;
    let (origin, scale) = {
        let models = storage(&cfb.root, "Dgn-Md").ok_or_else(lost)?;
        let model = storage(models, "#000000").ok_or_else(lost)?;
        stream(model, "Dgn~Mh").as_deref().and_then(units).ok_or_else(lost)?
    };
    let per = 1.0 / scale;
    let uor = |p: Point| [origin.x + p.x * per, origin.y + p.y * per];
    let seed_graphics = {
        let models = storage(&cfb.root, "Dgn-Md").ok_or_else(lost)?;
        let model = storage(models, "#000000").ok_or_else(lost)?;
        storage(model, "Dgn^G").map(pages).unwrap_or_default().iter().flat_map(|p| objects(p)).collect::<Vec<_>>()
    };
    let named = storage(&cfb.root, "Dgn^Nm").map(pages).unwrap_or_default();
    let mut named: Vec<Raw> = named.iter().flat_map(|p| objects(p)).collect();
    // Each kind's seed: a 2D, standalone element of it.
    let seed = |kind: u16| {
        seed_graphics
            .iter()
            .find(|(k, o, _)| {
                *k as u16 == kind && k & 0x6000_0000 == 0 && u32_at(o, 0x28).is_some_and(|f| f & 0x800 == 0)
            })
            .cloned()
    };
    let seeds: Vec<(u16, Raw)> = [3u16, 4, 6, 15, 16, 17]
        .into_iter()
        .map(|k| seed(k).map(|s| (k, s)))
        .collect::<Option<_>>()
        .ok_or_else(lost)?;
    let seed_of = |k: u16| &seeds.iter().find(|(x, _)| *x == k).expect("every kind seeded").1;
    // Complex chains' and shapes' headers: the seed's first of each.
    let header_seed = |kind: u16| {
        seed_graphics.iter().find(|(k, ..)| *k as u16 == kind && k & HEADER != 0 && k & COMPONENT == 0).cloned()
    };
    let (chain_seed, shape_seed) = (header_seed(12).ok_or_else(lost)?, header_seed(14).ok_or_else(lost)?);
    let id_of = |o: &[u8]| u64::from_le_bytes(o[0x10..0x18].try_into().unwrap_or_default());
    let mut next = seed_graphics.iter().chain(&named).map(|(_, o, _)| id_of(o)).max().unwrap_or(0) + 1;
    // Levels: the seed's (by name), and one more for each other layer.
    let level_seed =
        named.iter().find(|(k, o, _)| *k as u16 == 95 && u32_at(o, 0x0c) == Some(1)).cloned().ok_or_else(lost)?;
    let mut levels: Vec<(String, u32)> = named
        .iter()
        .filter(|(k, o, _)| *k as u16 == 95 && u32_at(o, 0x0c) == Some(1))
        .filter_map(|(_, o, a)| Some((level_name(&o[*a..])?.to_ascii_lowercase(), u32_at(o, 0x20)?)))
        .collect();
    let default = levels.first().map_or(64, |l| l.1);
    let mut level_id = levels.iter().map(|l| l.1).max().unwrap_or(64) + 1;
    for layer in doc.layers() {
        let key = layer.name.to_ascii_lowercase();
        if key == "0" || levels.iter().any(|(n, _)| *n == key) {
            continue;
        }
        let mut o = level_seed.1[..level_seed.2].to_vec();
        let attributes = (o.len() / 2) as u32;
        o.extend(name_linkage(&layer.name));
        let words = (o.len() / 2) as u32;
        o[4..8].copy_from_slice(&words.to_le_bytes());
        o[8..12].copy_from_slice(&attributes.to_le_bytes());
        o[0x10..0x18].copy_from_slice(&next.to_le_bytes());
        o[0x20..0x24].copy_from_slice(&level_id.to_le_bytes());
        o[0x48..0x4c].copy_from_slice(&u32::from(layer.color).to_le_bytes());
        o[0x5c..0x60].copy_from_slice(&level_id.to_le_bytes());
        let a = attributes as usize * 2;
        named.push((level_seed.0, o, a));
        levels.push((key, level_id));
        next += 1;
        level_id += 1;
    }
    // The model's elements.
    let model_id = id_of(&seed_of(3).1);
    let mut out: Vec<Vec<u8>> = Vec::new();
    let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
    let f = |v: f64| v.to_le_bytes();
    for e in doc.space_in_draw_order(oxidraft_model::MODEL) {
        let key = e.layer.to_ascii_lowercase();
        let level = levels.iter().find(|(n, _)| *n == key).map_or(default, |l| l.1);
        let color = match e.color {
            Color::Index(i) => u32::from(i),
            Color::ByBlock => 0xffff_fffe,
            _ => 0xffff_ffff,
        };
        for g in doc.expanded(e) {
            let range_of = |g: &Geometry| {
                let b = g.bounds();
                let (lo, hi) = (uor(b.min), uor(b.max));
                [lo[0].floor() as i64, lo[1].floor() as i64, hi[0].ceil() as i64, hi[1].ceil() as i64]
            };
            let range = range_of(&g);
            let b = g.bounds();
            let (lo, hi) = (uor(b.min), uor(b.max));
            (low, high) = ([low[0].min(lo[0]), low[1].min(lo[1])], [high[0].max(hi[0]), high[1].max(hi[1])]);
            let point = |p: Point| uor(p).iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
            // An element made on `seed`, its type's flags with `flags`.
            let mut push = |seed: &Raw, flags: u32, range: [i64; 4], fields: Vec<u8>| {
                let mut o = made(seed, &fields, next, level, color, range);
                o[0x18..0x20].copy_from_slice(&model_id.to_le_bytes());
                let kind = u32::from_le_bytes([o[0], o[1], o[2], o[3]]) | flags;
                o[0..4].copy_from_slice(&kind.to_le_bytes());
                out.push(o);
                next += 1;
            };
            let points = |pts: &[Point]| {
                let mut d = (pts.len() as u32).to_le_bytes().to_vec();
                d.extend([0u8; 4]);
                for p in pts {
                    d.extend(point(*p));
                }
                d
            };
            match &g {
                Geometry::Point { position } => {
                    push(seed_of(3), 0, range, [point(*position), point(*position)].concat())
                }
                Geometry::Line { start, end } => push(seed_of(3), 0, range, [point(*start), point(*end)].concat()),
                Geometry::Polyline { vertices, closed } if vertices.iter().all(|v| v.bulge == 0.0) => {
                    let mut pts: Vec<Point> = vertices.iter().map(|v| v.point).collect();
                    if *closed && pts.len() >= 3 {
                        pts.push(pts[0]);
                        push(seed_of(6), 0, range, points(&pts));
                    } else if pts.len() >= 2 {
                        push(seed_of(4), 0, range, points(&pts));
                    }
                }
                Geometry::Circle { center, radius } => push(
                    seed_of(15),
                    0,
                    range,
                    [f(radius * per), f(radius * per), f(0.0)].concat().into_iter().chain(point(*center)).collect(),
                ),
                Geometry::Arc { center, radius, start_angle, end_angle } => {
                    let sweep = (end_angle - start_angle).rem_euclid(360.0);
                    let d: Vec<u8> =
                        [f(start_angle.to_radians()), f(sweep.to_radians()), f(radius * per), f(radius * per), f(0.0)]
                            .concat()
                            .into_iter()
                            .chain(point(*center))
                            .collect();
                    push(seed_of(16), 0, range, d);
                }
                Geometry::Ellipse(el) => {
                    let (a, turn) = (el.major.length(), el.major.y.atan2(el.major.x));
                    let axes = [f(a * per), f(a * el.ratio * per)].concat();
                    if el.is_full() {
                        push(seed_of(15), 0, range, [axes, f(turn).to_vec(), point(el.center)].concat());
                    } else {
                        let sweep = (el.end - el.start).rem_euclid(360.0);
                        let head = [f(el.start.to_radians()), f(sweep.to_radians())].concat();
                        push(seed_of(16), 0, range, [head, axes, f(turn).to_vec(), point(el.center)].concat());
                    }
                }
                Geometry::Text { position, height, rotation, content, .. } => {
                    // The seed's text fields (font, justification and the
                    // rest) with this one's size, turn, place and text.
                    let latin = content.chars().all(|c| u32::from(c) < 256);
                    let mut text: Vec<u8> = if content.is_ascii() {
                        content.bytes().collect()
                    } else if latin {
                        [0xff, 0xfe, 1, 0].into_iter().chain(content.chars().map(|c| u32::from(c) as u8)).collect()
                    } else {
                        content.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).collect()
                    };
                    text.truncate(0xffff);
                    let mut d = seed_of(17).1[0x68..0xaa].to_vec();
                    let mult = height * per / (6.0 / 1000.0);
                    d[0x6e - 0x68..0x70 - 0x68].copy_from_slice(&(text.len() as u16).to_le_bytes());
                    d[0x70 - 0x68..0x78 - 0x68].copy_from_slice(&f(mult));
                    d[0x78 - 0x68..0x80 - 0x68].copy_from_slice(&f(mult));
                    d[0x90 - 0x68..0x98 - 0x68].copy_from_slice(&f(rotation.to_radians()));
                    d[0x98 - 0x68..0xa8 - 0x68].copy_from_slice(&point(*position));
                    d[0xa8 - 0x68..0xaa - 0x68].copy_from_slice(&0u16.to_le_bytes());
                    d.extend(text);
                    push(seed_of(17), 0, range, d);
                }
                // With arcs: a complex chain (a complex shape, closed) of
                // lines and arcs, after its header (their count).
                Geometry::Polyline { closed, .. } => {
                    let segments = g.segments();
                    let header = if *closed { &shape_seed } else { &chain_seed };
                    let count = [(segments.len() as u32).to_le_bytes(), [0; 4]].concat();
                    push(header, 0, range, count);
                    for seg in &segments {
                        let part = Geometry::from_seg(seg);
                        match seg {
                            oxidraft_geom::curve::Seg::Line(l) => {
                                push(seed_of(3), COMPONENT, range_of(&part), [point(l.start), point(l.end)].concat())
                            }
                            oxidraft_geom::curve::Seg::Arc(a) => {
                                let d = [
                                    f(a.start.to_radians()),
                                    f(a.sweep.to_radians()),
                                    f(a.radius * per),
                                    f(a.radius * per),
                                    f(0.0),
                                ]
                                .concat();
                                push(seed_of(16), COMPONENT, range_of(&part), [d, point(a.center)].concat());
                            }
                        }
                    }
                }
                // Everything else as line strings of its segments.
                _ => {
                    let mut pts: Vec<Point> = Vec::new();
                    for seg in g.segments() {
                        let n = match &seg {
                            oxidraft_geom::curve::Seg::Arc(a) => ((a.sweep.abs() / 10.0).ceil() as usize).max(2),
                            _ => 1,
                        };
                        for k in 0..=n {
                            let p = seg.point_at(k as f64 / n as f64);
                            if pts.last().is_none_or(|q| !q.approx_eq(p)) {
                                pts.push(p);
                            }
                        }
                    }
                    if pts.len() >= 2 {
                        push(seed_of(4), 0, range, points(&pts));
                    }
                }
            }
        }
    }
    // The model's extents in its header, the highest ID in the file's.
    let models = storage_mut(&mut cfb.root, "Dgn-Md").ok_or_else(lost)?;
    let model = storage_mut(models, "#000000").ok_or_else(lost)?;
    let mut header = stream(model, "Dgn~Mh").and_then(|h| inflate(&h)).ok_or_else(lost)?;
    if low[0] <= high[0] {
        // The type-66 object ends the stream; its extents at 0x90.
        let at = (0..header.len().saturating_sub(16)).step_by(4).find(|&d| {
            let r = d + 4;
            u32_at(&header, d) == Some(0)
                && u32_at(&header, r).is_some_and(|k| k as u16 == 66)
                && u32_at(&header, r + 4).is_some_and(|w| r + 2 * w as usize == header.len())
        });
        if let Some(d) = at {
            let r = d + 4 + 0x90;
            for (k, v) in [low[0], low[1], 0.0, high[0], high[1], 0.0].into_iter().enumerate() {
                header[r + 8 * k..r + 8 * k + 8].copy_from_slice(&(v.round() as i64).to_le_bytes());
            }
        }
    }
    // The model named Default, with no description (the seed's were
    // my_model and my_description): its header's two name linkages.
    let mut at = 0;
    while let Some(k) = (at..header.len().saturating_sub(4)).find(|&k| header[k + 2..k + 4] == [0xd2, 0x56]) {
        let size = 2 * (usize::from(header[k]) + 1);
        let room = size.saturating_sub(12);
        let payload: Vec<u8> = match u32_at(&header, k + 4) {
            Some(1) => [0xff, 0xfd].into_iter().chain("Default".encode_utf16().flat_map(u16::to_le_bytes)).collect(),
            _ => vec![0xff, 0xfd],
        };
        if payload.len() <= room && k + size <= header.len() {
            header[k + 8..k + 12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            header[k + 12..k + size].fill(0);
            header[k + 12..k + 12 + payload.len()].copy_from_slice(&payload);
        }
        at = k + size.max(4);
    }
    set_stream(model, "Dgn~Mh", &deflate(&header));
    if let Some(g) = storage_mut(model, "Dgn^G") {
        set_stream(g, "$1", &write_page(&out));
    }
    // The model index: its one entry, named Default too.
    if let Some(ix) = storage_mut(&mut cfb.root, "Dgn^Ix")
        && let Some(index) = stream(ix, "Dgn~Mix").and_then(|d| inflate(&d))
        && index.len() >= 48
    {
        let name: Vec<u8> = "Default".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut entry = index[16..48].to_vec();
        // The name, and a description of one blank (four-byte aligned).
        entry[0x10..0x12].copy_from_slice(&((32 + name.len() + 2) as u16).to_le_bytes());
        entry[0x12..0x14].copy_from_slice(&(name.len() as u16).to_le_bytes());
        entry[0x14..0x18].copy_from_slice(&2u32.to_le_bytes());
        entry.extend(&name);
        entry.extend([0, 0]);
        let mut out = index[..16].to_vec();
        out[8..12].copy_from_slice(&1u32.to_le_bytes());
        out.extend(entry);
        set_stream(ix, "Dgn~Mix", &deflate(&out));
    }
    if let Some(n) = storage_mut(&mut cfb.root, "Dgn^Nm") {
        let objects: Vec<Vec<u8>> = named.into_iter().map(|(_, o, _)| o).collect();
        set_stream(n, "$1", &write_page(&objects));
    }
    if let Some(h) = stream(&cfb.root, "Dgn~H")
        && let Some(mut body) = h.get(0x14..).and_then(inflate)
        && body.len() >= 0x130
    {
        body[0x128..0x130].copy_from_slice(&(next - 1).to_le_bytes());
        let mut raw = h[..0x14].to_vec();
        raw.extend(deflate(&body));
        set_stream(&mut cfb.root, "Dgn~H", &raw);
    }
    Ok(cfb.encode())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use oxidraft_io_dwg::compound_file::{CompoundFile, CompoundStream};
    use std::io::Write;

    fn zlib(d: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(d).unwrap();
        e.finish().unwrap()
    }

    fn put(b: &mut [u8], at: usize, v: &[u8]) {
        b[at..at + v.len()].copy_from_slice(v);
    }

    /// An element: the common header (level, color, 3D flag) and `fields`
    /// after it, then `attributes`.
    pub(crate) fn object(
        kind: u16,
        level: u32,
        color: u32,
        three_d: bool,
        fields: &[u8],
        attributes: &[u8],
    ) -> Vec<u8> {
        let mut o = vec![0u8; 0x68];
        o.extend(fields);
        if o.len() % 2 == 1 {
            o.push(0);
        }
        let attribute_words = o.len() / 2;
        o.extend(attributes);
        if o.len() % 2 == 1 {
            o.push(0);
        }
        let words = o.len() / 2;
        put(&mut o, 0, &u32::from(kind).to_le_bytes());
        put(&mut o, 4, &(words as u32).to_le_bytes());
        put(&mut o, 8, &(attribute_words as u32).to_le_bytes());
        put(&mut o, 0x0c, &level.to_le_bytes());
        put(&mut o, 0x28, &(if three_d { 0x800u32 } else { 0 }).to_le_bytes());
        put(&mut o, 0x34, &color.to_le_bytes());
        o
    }

    /// A page of objects: its header, then them (each after a framing
    /// word), zlib-compressed.
    pub(crate) fn page(objects: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for o in objects {
            body.extend(0u32.to_le_bytes());
            body.extend(o);
        }
        let mut p = Vec::new();
        for v in [objects.len() as u32, 3, 1, objects.len() as u32] {
            p.extend(v.to_le_bytes());
        }
        p.extend(zlib(&body));
        p
    }

    fn doubles(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn stream_entry(name: &str, data: Vec<u8>) -> CompoundEntry {
        CompoundEntry::Stream(CompoundStream {
            name: name.into(),
            class_id: [0; 16],
            state_bits: 0,
            creation_time: 0,
            modified_time: 0,
            content: CompoundStreamContent::Binary(BinaryRecord::split(&data, 4096)),
        })
    }

    fn storage_entry(name: &str, entries: Vec<CompoundEntry>) -> CompoundEntry {
        CompoundEntry::Storage(CompoundStorage { name: name.into(), entries, ..CompoundStorage::root() })
    }

    /// A DGN V8 file of one model: 1000 UOR a master unit, its origin at
    /// (100, 200) UOR; `elements` in it; level 7 named "Walls".
    pub(crate) fn dgn8(elements: &[Vec<u8>]) -> Vec<u8> {
        let mut header = vec![0u8; 0xe8];
        put(&mut header, 0, &66u32.to_le_bytes());
        put(&mut header, 4, &((0xe8 / 2) as u32).to_le_bytes());
        put(&mut header, 8, &((0xe8 / 2) as u32).to_le_bytes());
        put(&mut header, 0xc8, &doubles(&[100.0, 200.0, 0.0]));
        put(&mut header, 0xe0, &1000.0f64.to_le_bytes());
        let mut mh = 0u32.to_le_bytes().to_vec();
        mh.extend(header);
        // The level's name: a 0x56d2 linkage, UTF-16 after ff fd.
        let name: Vec<u8> = [0xff, 0xfd].into_iter().chain("Walls".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let mut linkage = Vec::new();
        let total = (12 + name.len()).next_multiple_of(2);
        linkage.extend(((0x56d2u32 << 16) | 0x1000 | (total / 2 - 1) as u32).to_le_bytes());
        linkage.extend(1u32.to_le_bytes());
        linkage.extend((name.len() as u32).to_le_bytes());
        linkage.extend(&name);
        linkage.resize(total, 0);
        let mut level = object(95, 1, 0, false, &[0u8; 0x60], &linkage);
        put(&mut level, 0x20, &7u32.to_le_bytes());
        let model = storage_entry(
            "#000000",
            vec![stream_entry("Dgn~Mh", zlib(&mh)), storage_entry("Dgn^G", vec![stream_entry("$1", page(elements))])],
        );
        let mut root = CompoundStorage::root();
        root.entries = vec![
            stream_entry("Dgn~H", zlib(b"header")),
            stream_entry("Dgn~S", zlib(b"settings")),
            storage_entry("Dgn-Md", vec![model]),
            storage_entry("Dgn^Nm", vec![stream_entry("$1", page(&[level]))]),
        ];
        CompoundFile { root, ..CompoundFile::default() }.encode()
    }

    #[test]
    fn a_v8_models_elements() {
        // UOR for master point (x, y): origin + 1000 per unit.
        let uor = |x: f64, y: f64| [100.0 + x * 1000.0, 200.0 + y * 1000.0];
        let line = object(3, 7, 3, false, &doubles(&[uor(0.0, 0.0), uor(10.0, 5.0)].concat()), &[]);
        let mut string = 3u32.to_le_bytes().to_vec();
        string.extend([0u8; 4]);
        string.extend(doubles(&[uor(0.0, 0.0), uor(4.0, 0.0), uor(4.0, 3.0)].concat()));
        let shape = object(6, 2, 0xffff_ffff, false, &string, &[]);
        let circle =
            object(15, 2, 1, false, &doubles(&[2000.0, 2000.0, 0.0, uor(20.0, 20.0)[0], uor(20.0, 20.0)[1]]), &[]);
        let arc = object(
            16,
            2,
            1,
            false,
            &doubles(&[0.0, std::f64::consts::FRAC_PI_2, 3000.0, 3000.0, 0.0, uor(30.0, 0.0)[0], uor(30.0, 0.0)[1]]),
            &[],
        );
        // Text: 2.5 high (its multiplier, 6/1000 UOR each), turned 30°.
        let mut text = vec![0u8; 0xaa - 0x68];
        text[0x6e - 0x68..0x70 - 0x68].copy_from_slice(&4u16.to_le_bytes());
        text[0x78 - 0x68..0x80 - 0x68].copy_from_slice(&(2.5 * 1000.0 * 1000.0 / 6.0f64).to_le_bytes());
        text[0x90 - 0x68..0x98 - 0x68].copy_from_slice(&30f64.to_radians().to_le_bytes());
        text[0x98 - 0x68..0xa8 - 0x68].copy_from_slice(&doubles(&uor(5.0, 60.0)));
        text.extend(b"ROOM");
        let text = object(17, 7, 0, false, &text, &[]);
        // A 3D line: x, y, z each.
        let line3 = object(
            3,
            2,
            0,
            true,
            &doubles(&[uor(0.0, 0.0)[0], uor(0.0, 0.0)[1], 0.0, uor(1.0, 1.0)[0], uor(1.0, 1.0)[1], 5.0]),
            &[],
        );
        let got = read_dgn8(&dgn8(&[line, shape, circle, arc, text, line3])).unwrap();
        assert_eq!(got.len(), 6, "{got:?}");
        assert!(matches!(&got[0], (Geometry::Line { start, end }, layer, 3)
            if start.approx_eq(Point::ORIGIN) && end.approx_eq(Point::new(10.0, 5.0)) && layer == "Walls"));
        assert!(matches!(&got[1], (Geometry::Polyline { vertices, closed: true }, layer, 0)
            if vertices.len() == 3 && layer == "Level 2"));
        assert!(
            matches!(&got[2], (Geometry::Circle { center, radius }, ..) if center.approx_eq(Point::new(20.0, 20.0)) && (radius - 2.0).abs() < 1e-9)
        );
        assert!(matches!(&got[3], (Geometry::Arc { radius, start_angle, end_angle, .. }, ..)
            if (radius - 3.0).abs() < 1e-9 && start_angle.abs() < 1e-9 && (end_angle - 90.0).abs() < 1e-9));
        assert!(matches!(&got[4], (Geometry::Text { content, height, rotation, position, .. }, ..)
            if content == "ROOM" && (height - 2.5).abs() < 1e-9 && (rotation - 30.0).abs() < 1e-9 && position.approx_eq(Point::new(5.0, 60.0))));
        assert!(matches!(&got[5], (Geometry::Line { end, .. }, ..) if end.approx_eq(Point::new(1.0, 1.0))));
        assert!(read_dgn8(b"not a compound file").is_err());
    }

    #[test]
    fn written_as_v8_and_read_back() {
        use oxidraft_model::{Color, Document, Layer};
        let mut doc = Document::new();
        doc.add_layer(Layer::new("Walls", 1));
        let p = |x: f64, y: f64| Point::new(x, y);
        doc.add_on_layer(Geometry::Line { start: p(0.0, 0.0), end: p(100.0, 50.0) }, "Walls", Color::ByLayer);
        doc.add(Geometry::Polyline {
            vertices: [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)].map(Vertex::at).to_vec(),
            closed: true,
        });
        doc.add(Geometry::Circle { center: p(20.0, 20.0), radius: 5.0 });
        doc.add(Geometry::Arc { center: p(50.0, 0.0), radius: 10.0, start_angle: 30.0, end_angle: 120.0 });
        let mut el = Ellipse::full(p(0.0, 40.0), p(8.0, 0.0), 0.5);
        (el.start, el.end) = (0.0, 90.0);
        doc.add(Geometry::Ellipse(el));
        doc.add_on_layer(
            Geometry::Text {
                position: p(5.0, 60.0),
                height: 2.5,
                rotation: 30.0,
                width: 0.0,
                attach: Attach::BottomLeft,
                content: "Säule".into(),
                look: Default::default(),
            },
            "Walls",
            Color::Index(3),
        );
        let bulged = vec![Vertex::new(p(0.0, 0.0), 1.0), Vertex::at(p(10.0, 0.0))];
        doc.add(Geometry::Polyline { vertices: bulged.clone(), closed: false });
        let shape = vec![Vertex::at(p(30.0, 0.0)), Vertex::new(p(40.0, 0.0), -0.5), Vertex::at(p(40.0, 10.0))];
        doc.add(Geometry::Polyline { vertices: shape.clone(), closed: true });
        let data = write_dgn8(&doc).unwrap();
        let got = read_dgn8(&data).unwrap();
        assert_eq!(got.len(), 8, "{got:?}");
        assert!(
            matches!(&got[0], (Geometry::Line { end, .. }, layer, 0) if end.approx_eq(p(100.0, 50.0)) && layer == "Walls")
        );
        assert!(
            matches!(&got[1], (Geometry::Polyline { vertices, closed: true }, layer, 0) if vertices.len() == 3 && layer == "Default")
        );
        assert!(
            matches!(&got[2], (Geometry::Circle { center, radius }, ..) if center.approx_eq(p(20.0, 20.0)) && (radius - 5.0).abs() < 1e-9)
        );
        assert!(
            matches!(&got[3], (Geometry::Arc { start_angle, end_angle, .. }, ..) if (start_angle - 30.0).abs() < 1e-9 && (end_angle - 120.0).abs() < 1e-9)
        );
        assert!(
            matches!(&got[4], (Geometry::Ellipse(e), ..) if e.major.approx_eq(p(8.0, 0.0)) && (e.ratio - 0.5).abs() < 1e-9 && (e.end - 90.0).abs() < 1e-9)
        );
        assert!(matches!(&got[5], (Geometry::Text { content, height, rotation, .. }, layer, 3)
            if content == "Säule" && (height - 2.5).abs() < 1e-9 && (rotation - 30.0).abs() < 1e-9 && layer == "Walls"));
        // Polylines with arcs as complex chains and shapes, back whole.
        let same = |g: &Geometry, want: &[Vertex], closed: bool| match g {
            Geometry::Polyline { vertices, closed: c } => {
                *c == closed
                    && vertices.len() == want.len()
                    && vertices
                        .iter()
                        .zip(want)
                        .all(|(v, w)| v.point.approx_eq(w.point) && (v.bulge - w.bulge).abs() < 1e-9)
            }
            _ => false,
        };
        assert!(same(&got[6].0, &bulged, false), "{:?}", got[6]);
        assert!(same(&got[7].0, &shape, true), "{:?}", got[7]);
        // In the file: every object framed whole, IDs one each, the highest
        // in the header; none of the seed's own elements.
        let root = StructuredStoragePayload::decode(&data).compound_file.unwrap().root;
        let model = storage(storage(&root, "Dgn-Md").unwrap(), "#000000").unwrap();
        let page = pages(storage(model, "Dgn^G").unwrap()).remove(0);
        let objs = objects(&page);
        assert_eq!(objs.len() as u32, u32_at(&page, 0).unwrap());
        // Complex chains and shapes as GDAL's drawing has them: a header
        // with its count, its components (flagged so) after it.
        let at = objs.iter().position(|(k, ..)| *k == 0x3000_000c).expect("a complex chain");
        assert_eq!((objs[at].1.len(), u32_at(&objs[at].1, 0x68)), (112, Some(1)));
        assert_eq!(objs[at + 1].0, 0x5000_0010);
        let at = objs.iter().position(|(k, ..)| *k == 0x3000_000e).expect("a complex shape");
        assert_eq!(u32_at(&objs[at].1, 0x68), Some(3));
        let parts: Vec<u32> = objs[at + 1..at + 4].iter().map(|o| o.0).collect();
        assert_eq!(parts, [0x5000_0003, 0x5000_0010, 0x5000_0003]);
        let named = objects(&pages(storage(&root, "Dgn^Nm").unwrap()).remove(0));
        let mut ids: Vec<u64> =
            objs.iter().chain(&named).map(|(_, o, _)| u64::from_le_bytes(o[0x10..0x18].try_into().unwrap())).collect();
        let highest = *ids.iter().max().unwrap();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), objs.len() + named.len(), "no ID twice");
        let h = stream(&root, "Dgn~H").unwrap();
        let body = inflate(&h[0x14..]).unwrap();
        assert_eq!(u64::from_le_bytes(body[0x128..0x130].try_into().unwrap()), highest);
        assert!(!got.iter().any(|(g, _, _)| matches!(g, Geometry::Text { content, .. } if content == "myTéxt")));
        // The model named Default, in its index and its header.
        let utf16 = |t: &str| t.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        let has = |b: &[u8], n: &[u8]| b.windows(n.len()).any(|w| w == n);
        let index = inflate(&stream(storage(&root, "Dgn^Ix").unwrap(), "Dgn~Mix").unwrap()).unwrap();
        assert_eq!(index.len(), 16 + 32 + 16);
        assert_eq!(u16::from_le_bytes([index[16 + 0x10], index[16 + 0x11]]) as usize, index.len() - 16);
        assert!(has(&index, &utf16("Default")) && !has(&index, &utf16("my_model")));
        let header = inflate(&stream(model, "Dgn~Mh").unwrap()).unwrap();
        assert!(has(&header, &utf16("Default")) && !has(&header, &utf16("my_")));
    }

    /// GDAL's V8 test drawing, its contents as GDAL's reference lists them.
    #[test]
    fn gdals_v8_drawing() {
        let got = read_dgn8(include_bytes!("../assets/dgn/seed_v8.dgn")).unwrap();
        let p = |x: f64, y: f64| Point::new(x, y);
        let has = |want: &dyn Fn(&Geometry) -> bool| got.iter().filter(|(g, _, _)| want(g)).count();
        // A point on level 1 (color 3), the rest on level 64, "Default".
        assert!(
            matches!(&got[0], (Geometry::Point { position }, layer, 3) if position.approx_eq(p(0.0, 1.0)) && layer == "Level 1")
        );
        assert!(got[1..].iter().all(|(_, layer, _)| layer == "Default"));
        assert_eq!(
            has(
                &|g| matches!(g, Geometry::Line { start, end } if start.approx_eq(p(0.0, 1.0)) && end.approx_eq(p(3.0, 4.0)))
            ),
            1
        );
        assert_eq!(
            has(&|g| matches!(g, Geometry::Text { content, rotation, height, position, .. }
            if content == "myTéxt" && (rotation + 45.0).abs() < 1e-9 && (height - 1.0).abs() < 1e-9 && position.approx_eq(p(0.0, 1.0)))),
            1
        );
        assert_eq!(
            has(
                &|g| matches!(g, Geometry::Polyline { vertices, closed: false } if vertices.len() == 3 && vertices[2].point.approx_eq(p(6.0, 7.0)))
            ),
            2
        );
        // Point strings' points.
        assert_eq!(has(&|g| matches!(g, Geometry::Point { position } if position.approx_eq(p(3.0, 4.0)))), 2);
        // Ellipses 1 by 2 about (0, 1), one an arc from 10° (at 0.985, 1.347).
        assert_eq!(
            has(
                &|g| matches!(g, Geometry::Ellipse(e) if e.major.approx_eq(p(0.0, 2.0)) && (e.ratio - 0.5).abs() < 1e-9)
            ),
            2
        );
        let arc = got.iter().find_map(|(g, _, _)| match g {
            Geometry::Ellipse(e) if !e.is_full() => Some(*e),
            _ => None,
        });
        let at = arc.expect("the elliptical arc").point_at(280.0);
        assert!(
            (at.x - 10f64.to_radians().cos()).abs() < 1e-9
                && (at.y - (1.0 + 2.0 * 10f64.to_radians().sin())).abs() < 1e-9,
            "{at:?}"
        );
        let unit_circle = |g: &Geometry| matches!(g, Geometry::Circle { center, radius } if center.approx_eq(p(0.0, 1.0)) && (radius - 1.0).abs() < 1e-9);
        assert_eq!(has(&unit_circle), 3);
        // The third the shared cell's: "Named definition" (a circle of 1
        // about its base point), placed at 0,1 (GDAL gives its origin).
        assert!(got.last().is_some_and(|(g, layer, _)| unit_circle(g) && layer == "Default"));
        assert_eq!(
            has(
                &|g| matches!(g, Geometry::Arc { start_angle, end_angle, .. } if (start_angle - 55.0).abs() < 1e-9 && (end_angle - 235.0).abs() < 1e-9)
            ),
            1
        );
        // A curve through its points, from (1, 1) as GDAL draws it.
        assert_eq!(
            has(&|g| matches!(g, Geometry::Spline(s) if s.control().first().is_some_and(|c| c.approx_eq(p(1.0, 1.0))))),
            2
        );
        // B-spline curves (cubic, uniform knots): from (P0 + 4 P1 + P2) / 6,
        // where GDAL's start.
        let curves: Vec<Point> = got
            .iter()
            .filter_map(|(g, _, _)| match g {
                Geometry::Spline(s) if s.control().len() > 4 => Some(s.start_point()),
                _ => None,
            })
            .collect();
        assert_eq!(curves.len(), 2, "{curves:?}");
        assert!(curves.iter().all(|c| c.distance(p(-7.0 / 6.0, 5.5 / 6.0)) < 1e-9), "{curves:?}");
        // Complex chains and shapes as GDAL gives them: two lines end to
        // end (2D and 3D); a line and an arc on through (3, 0); a closed
        // one whose arc bulges up through (0.5, 1.5). Those whose parts
        // don't meet come in as their parts.
        let polyline = |want: &[(f64, f64, f64)], closed: bool| {
            has(&|g| match g {
                Geometry::Polyline { vertices, closed: c } => {
                    *c == closed
                        && vertices.len() == want.len()
                        && vertices
                            .iter()
                            .zip(want)
                            .all(|(v, w)| v.point.approx_eq(p(w.0, w.1)) && (v.bulge - w.2).abs() < 1e-9)
                }
                _ => false,
            })
        };
        assert_eq!(polyline(&[(0.0, 1.0, 0.0), (2.0, 3.0, 0.0), (4.0, 5.0, 0.0)], false), 2);
        assert_eq!(polyline(&[(0.0, 1.0, 0.0), (2.0, 1.0, 1.0), (4.0, 1.0, 0.0)], false), 1);
        assert_eq!(polyline(&[(0.0, 0.0, 0.0), (0.0, 1.0, -1.0), (1.0, 1.0, 0.0)], true), 1);
        // A cell's shapes (one inside the other).
        assert_eq!(
            has(
                &|g| matches!(g, Geometry::Polyline { vertices, closed: true } if vertices[0].point.approx_eq(p(0.1, 0.1)))
            ),
            2
        );
    }
}

//! MicroStation DGN V8 files: a compound file whose first model's
//! elements (points, lines, line strings, shapes, curves, B-spline curves,
//! point strings, arcs, ellipses and text;
//! the components of complex chains, shapes and cells, one by one) come in
//! as DGNIMPORT and DGNATTACH bring V7's, a layer for each level by its
//! name. The layout is as ezdgn's V8 format notes set it out (MIT,
//! clean-room): zlib pages of objects, a model header with the units, a
//! common 0x68-byte element header, each element's fields after it.

use std::io::Read;

use oxidraft_geom::{Point, ellipse::Ellipse};
use oxidraft_io_dwg::compound_file::{CompoundEntry, CompoundStorage, StructuredStoragePayload};
use oxidraft_model::{Attach, Geometry, Vertex};

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

/// A page's objects: after its 16-byte header (the count first), zlib;
/// each object a 4-byte framing word, then its type and flags, its length
/// and its attribute boundary (both in 16-bit words): (type and flags,
/// the object, where its attributes start).
fn objects(page: &[u8]) -> Vec<(u32, Vec<u8>, usize)> {
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
    let mut out = Vec::new();
    // A B-spline curve's order, from its header (type 27), for its poles
    // (type 21) after it.
    let mut order = None;
    for page in storage(model, "Dgn^G").map(pages).unwrap_or_default() {
        for (kind, object, attributes) in objects(&page) {
            match kind as u16 {
                27 => order = u32_at(&object, 0x6c).map(|p| (p & 0x0f) as usize + 2),
                21 => {
                    if let Some(order) = order.take()
                        && let Some(g) = bspline(&object, order, &to)
                    {
                        out.push((g, layer_of(&levels, &object), color_of(&object)));
                    }
                    continue;
                }
                _ => {}
            }
            let Some(parts) = element(kind as u16, &object[..attributes.max(0x68).min(object.len())], scale, &to)
            else {
                continue;
            };
            let (layer, color) = (layer_of(&levels, &object), color_of(&object));
            out.extend(parts.into_iter().map(|g| (g, layer.clone(), color)));
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use oxidraft_io_dwg::compound_file::{BinaryRecord, CompoundFile, CompoundStream, CompoundStreamContent};
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

    /// GDAL's V8 test drawing, its contents as GDAL's reference lists them.
    #[test]
    fn gdals_v8_drawing() {
        let got = read_dgn8(include_bytes!("../tests/fixtures/dgnv8/test_dgnv8.dgn")).unwrap();
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
        assert_eq!(
            has(
                &|g| matches!(g, Geometry::Circle { center, radius } if center.approx_eq(p(0.0, 1.0)) && (radius - 1.0).abs() < 1e-9)
            ),
            2
        );
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
        // A complex chain's arc, and a cell's shapes (one inside the other).
        assert_eq!(
            has(
                &|g| matches!(g, Geometry::Arc { center, start_angle, .. } if center.approx_eq(p(3.0, 1.0)) && (start_angle - 180.0).abs() < 1e-9)
            ),
            1
        );
        assert_eq!(
            has(
                &|g| matches!(g, Geometry::Polyline { vertices, closed: true } if vertices[0].point.approx_eq(p(0.1, 0.1)))
            ),
            2
        );
    }
}

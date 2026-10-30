//! MicroStation DGN files, version 7 (and V8's, see `dgn8`): DGNIMPORT
//! brings their lines, line strings, shapes, curves, arcs, ellipses and
//! text in, on a layer for each level; DGNATTACH places one as a block
//! reference; DGNEXPORT writes a V7 file of lines, line strings, shapes,
//! arcs and text.

use std::path::Path;

use oxidraft_geom::{Point, ellipse::Ellipse};
use oxidraft_model::{Attach, Block, BlockEntity, Color, Document, Geometry, Placement, Vertex};

use crate::app::OxidraftApp;

/// A 32-bit integer as DGN keeps them (two 16-bit words, high first).
fn int32(p: &[u8]) -> i32 {
    (u32::from(p[2]) | u32::from(p[3]) << 8 | u32::from(p[0]) << 16 | u32::from(p[1]) << 24) as i32
}

fn put_int32(v: i32) -> [u8; 4] {
    let b = (v as u32).to_le_bytes();
    [b[2], b[3], b[0], b[1]]
}

/// A VAX D-float as an IEEE double.
fn vax_double(s: &[u8]) -> f64 {
    let hi = u32::from_le_bytes([s[2], s[3], s[0], s[1]]);
    let lo = u32::from_le_bytes([s[6], s[7], s[4], s[5]]);
    let sign = hi & 0x8000_0000;
    let mut exponent = (hi >> 23) & 0xff;
    if exponent != 0 {
        exponent = exponent + 1023 - 129;
    }
    let round = lo & 7;
    let mut new_lo = ((lo >> 3) & 0x1fff_ffff) | (hi << 29);
    if round != 0 {
        new_lo |= 1;
    }
    let new_hi = ((hi >> 3) & 0x000f_ffff) | (exponent << 20) | sign;
    f64::from_bits(u64::from(new_hi) << 32 | u64::from(new_lo))
}

/// An IEEE double as a VAX D-float.
fn put_vax_double(v: f64) -> [u8; 8] {
    if v == 0.0 {
        return [0; 8];
    }
    let bits = v.to_bits();
    let (hi, lo) = ((bits >> 32) as u32, bits as u32);
    let sign = hi & 0x8000_0000;
    let exponent = ((hi >> 20) & 0x7ff) + 129 - 1023;
    let new_hi = sign | (exponent & 0xff) << 23 | (hi & 0x000f_ffff) << 3 | lo >> 29;
    let new_lo = lo << 3;
    let (h, l) = (new_hi.to_le_bytes(), new_lo.to_le_bytes());
    [h[2], h[3], h[0], h[1], l[2], l[3], l[0], l[1]]
}

/// What a DGN file holds: (geometry in master units, its level's layer,
/// its color; 0 for the level's). A V8 file (a compound file) its first
/// model's.
pub fn read_dgn(data: &[u8]) -> Result<Vec<(Geometry, String, u8)>, String> {
    if data.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return crate::dgn8::read_dgn8(data);
    }
    if data.len() < 4 || data[1] & 0x7f != 9 {
        return Err("Not a DGN file.".into());
    }
    let (mut scale, mut origin, mut three_d) = (1.0, (0.0, 0.0), false);
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= data.len() {
        if data[at] == 0xff && data[at + 1] == 0xff {
            break;
        }
        let level = data[at] & 0x3f;
        let kind = data[at + 1] & 0x7f;
        let deleted = data[at + 1] & 0x80 != 0;
        let words = usize::from(u16::from_le_bytes([data[at + 2], data[at + 3]]));
        let size = (words + 2) * 2;
        let Some(e) = data.get(at..at + size) else { break };
        at += size;
        if deleted {
            continue;
        }
        // The design file header: 2D or 3D, units, global origin.
        if kind == 9 && e.len() >= 1264 {
            three_d = e[1214] & 0x40 != 0;
            let (su, uor) = (f64::from(int32(&e[1112..])), f64::from(int32(&e[1116..])));
            if su > 0.0 && uor > 0.0 {
                scale = 1.0 / (su * uor);
            }
            origin = (vax_double(&e[1240..]) * scale, vax_double(&e[1248..]) * scale);
            continue;
        }
        if e.len() < 36 {
            continue;
        }
        let color = (u16::from_le_bytes([e[34], e[35]]) >> 8) as u8;
        let dims = if three_d { 3 } else { 2 };
        let point =
            |p: &[u8]| Point::new(f64::from(int32(p)) * scale - origin.0, f64::from(int32(&p[4..])) * scale - origin.1);
        let g = match kind {
            3 if e.len() >= 36 + 8 * dims => {
                Some(Geometry::Line { start: point(&e[36..]), end: point(&e[36 + 4 * dims..]) })
            }
            4 | 6 | 11 if e.len() >= 38 => {
                let n = usize::from(u16::from_le_bytes([e[36], e[37]]));
                let pts: Vec<Point> =
                    (0..n).filter_map(|i| e.get(38 + i * 4 * dims..38 + i * 4 * dims + 8).map(point)).collect();
                (pts.len() >= 2).then(|| Geometry::Polyline {
                    vertices: pts.into_iter().map(Vertex::at).collect(),
                    closed: kind == 6,
                })
            }
            15 | 16 => {
                let arc = kind == 16;
                let base = if arc { 44 } else { 36 };
                let need = base + if three_d { 56 } else { 36 };
                if e.len() < need {
                    None
                } else {
                    let (a, b) = (vax_double(&e[base..]) * scale, vax_double(&e[base + 8..]) * scale);
                    let (rotation, origin_at) = if three_d {
                        (0.0, base + 32)
                    } else {
                        (f64::from(int32(&e[base + 16..])) / 360_000.0, base + 20)
                    };
                    let center = Point::new(
                        vax_double(&e[origin_at..]) * scale - origin.0,
                        vax_double(&e[origin_at + 8..]) * scale - origin.1,
                    );
                    let (start, sweep) = if arc {
                        let raw = int32(&e[40..]);
                        let sweep = f64::from(raw & 0x7fff_ffff) / 360_000.0 * if raw < 0 { -1.0 } else { 1.0 };
                        (f64::from(int32(&e[36..])) / 360_000.0, if sweep == 0.0 { 360.0 } else { sweep })
                    } else {
                        (0.0, 360.0)
                    };
                    let (from, to) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
                    let full = sweep.abs() >= 360.0 - 1e-9;
                    Some(if (a - b).abs() < 1e-9 * a.max(1.0) {
                        if full {
                            Geometry::Circle { center, radius: a }
                        } else {
                            Geometry::Arc {
                                center,
                                radius: a,
                                start_angle: (from + rotation).rem_euclid(360.0),
                                end_angle: (to + rotation).rem_euclid(360.0),
                            }
                        }
                    } else {
                        let major = Point::new(a, 0.0).rotated_about(Point::ORIGIN, rotation);
                        let mut el = Ellipse::full(center, major, b / a.max(1e-12));
                        if !full {
                            (el.start, el.end) = (from.rem_euclid(360.0), to.rem_euclid(360.0));
                        }
                        Geometry::Ellipse(el)
                    })
                }
            }
            17 => {
                let (height_at, rot_at, origin_at, count_at) =
                    if three_d { (42, 46, 62, 74) } else { (42, 46, 50, 58) };
                if e.len() <= count_at + 2 {
                    None
                } else {
                    let height = f64::from(int32(&e[height_at..])) * scale * 6.0 / 1000.0;
                    let rotation = if three_d { 0.0 } else { f64::from(int32(&e[rot_at..])) / 360_000.0 };
                    let n = usize::from(e[count_at]);
                    let text: String =
                        e.get(count_at + 2..count_at + 2 + n).unwrap_or(&[]).iter().map(|b| char::from(*b)).collect();
                    Some(Geometry::Text {
                        position: point(&e[origin_at..]),
                        height,
                        rotation,
                        width: 0.0,
                        attach: Attach::BottomLeft,
                        content: text,
                        look: Default::default(),
                    })
                }
            }
            _ => None,
        };
        out.extend(g.map(|g| (g, format!("Level {level}"), color)));
    }
    Ok(out)
}

/// An element: its header (level, type, size, range, symbology) and data.
fn element(kind: u8, level: u8, color: u8, range: (Point, Point), data: &[u8]) -> Vec<u8> {
    let mut e = vec![level.clamp(1, 63), kind, 0, 0];
    let uor = |v: f64| put_int32((v * UOR).round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32);
    for v in [range.0.x, range.0.y, 0.0, range.1.x, range.1.y, 0.0] {
        e.extend_from_slice(&uor(v));
    }
    // Graphic group, attributes' index, properties; then the symbology.
    e.extend_from_slice(&[0; 6]);
    e.extend_from_slice(&(u16::from(color) << 8).to_le_bytes());
    e.extend_from_slice(data);
    if e.len() % 2 == 1 {
        e.push(0);
    }
    // Words to follow the first two.
    let words = (e.len() / 2 - 2) as u16;
    e[2..4].copy_from_slice(&words.to_le_bytes());
    e
}

/// Units of resolution a master unit, as written.
const UOR: f64 = 1000.0;

fn uor_point(p: Point) -> Vec<u8> {
    let v = |c: f64| put_int32((c * UOR).round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32);
    [v(p.x), v(p.y)].concat()
}

/// The drawing's model space as a V7 2D DGN file, a level for each of
/// its first 63 layers.
pub fn write_dgn(doc: &Document) -> Vec<u8> {
    // The design file header: 2D, 1 sub unit a master unit, 1000 UORs a
    // sub unit, no global origin.
    let mut tcb = vec![0u8; 1536];
    tcb[1112..1116].copy_from_slice(&put_int32(1));
    tcb[1116..1120].copy_from_slice(&put_int32(1000));
    tcb[1120..1122].copy_from_slice(b"mu");
    tcb[1122..1124].copy_from_slice(b"su");
    let mut out = element(9, 8, 0, (Point::ORIGIN, Point::ORIGIN), &tcb[36..]);
    out[0] = 0x08;
    let layers: Vec<String> = doc.layers().iter().map(|l| l.name.to_ascii_lowercase()).collect();
    for e in doc.space_in_draw_order(oxidraft_model::MODEL) {
        let level = layers.iter().position(|l| *l == e.layer.to_ascii_lowercase()).map_or(1, |i| (i % 63 + 1) as u8);
        let color = match e.color {
            Color::Index(i) => i,
            _ => doc.layer(&e.layer).map_or(7, |l| l.color),
        };
        for g in doc.expanded(e) {
            let b = g.bounds();
            let range = (b.min, b.max);
            match &g {
                Geometry::Line { start, end } => {
                    out.extend(element(3, level, color, range, &[uor_point(*start), uor_point(*end)].concat()));
                }
                Geometry::Text { position, height, rotation, content, .. } => {
                    let text: Vec<u8> =
                        content.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).take(255).collect();
                    let mult = put_int32((height / (6.0 / 1000.0) * UOR).round() as i32);
                    let mut d = vec![1u8, 0];
                    d.extend_from_slice(&mult);
                    d.extend_from_slice(&mult);
                    d.extend_from_slice(&put_int32((rotation * 360_000.0).round() as i32));
                    d.extend(uor_point(*position));
                    d.extend_from_slice(&[text.len() as u8, 0]);
                    d.extend(text);
                    out.extend(element(17, level, color, range, &d));
                }
                Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => {
                    let (start, sweep) = match &g {
                        Geometry::Arc { start_angle, end_angle, .. } => {
                            (*start_angle, (end_angle - start_angle).rem_euclid(360.0))
                        }
                        _ => (0.0, 360.0),
                    };
                    let mut d = put_int32((start * 360_000.0).round() as i32).to_vec();
                    d.extend_from_slice(&put_int32((sweep * 360_000.0).round() as i32));
                    d.extend_from_slice(&put_vax_double(radius * UOR));
                    d.extend_from_slice(&put_vax_double(radius * UOR));
                    d.extend_from_slice(&put_int32(0));
                    d.extend_from_slice(&put_vax_double(center.x * UOR));
                    d.extend_from_slice(&put_vax_double(center.y * UOR));
                    out.extend(element(16, level, color, range, &d));
                }
                _ => {
                    // Everything else as line strings of its segments.
                    let mut pts: Vec<Point> = Vec::new();
                    for seg in g.segments() {
                        let n = match &seg {
                            oxidraft_geom::curve::Seg::Arc(a) => ((a.sweep.abs() / 10.0).ceil() as usize).max(2),
                            _ => 1,
                        };
                        for i in 0..=n {
                            let p = seg.point_at(i as f64 / n as f64);
                            if pts.last().is_none_or(|q| !q.approx_eq(p)) {
                                pts.push(p);
                            }
                        }
                    }
                    for chunk in pts.chunks(101).filter(|c| c.len() >= 2) {
                        let mut d = (chunk.len() as u16).to_le_bytes().to_vec();
                        for p in chunk {
                            d.extend(uor_point(*p));
                        }
                        out.extend(element(4, level, color, range, &d));
                    }
                }
            }
        }
    }
    out.extend_from_slice(&[0xff, 0xff]);
    out
}

impl OxidraftApp {
    fn read_dgn_file(file: &Path) -> Result<Vec<(Geometry, String, u8)>, String> {
        let data = std::fs::read(file).map_err(|e| format!("Could not read {}: {e}", file.display()))?;
        read_dgn(&data)
    }

    /// DGNIMPORT: a DGN file's elements as objects, a layer a level.
    pub(crate) fn dgn_import(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("DGN", &["dgn"]).pick_file() else { return };
        match self.dgn_import_file(&file) {
            Ok(n) => self.engine.log(format!("{n} object(s) brought in from {}", file.display())),
            Err(e) => self.engine.log(e),
        }
    }

    pub fn dgn_import_file(&mut self, file: &Path) -> Result<usize, String> {
        let objects = Self::read_dgn_file(file)?;
        self.engine.cancel(&mut self.doc);
        self.doc.begin("DGNIMPORT");
        let n = objects.len();
        for (g, layer, color) in objects {
            let color = if color == 0 { Color::ByLayer } else { Color::Index(color) };
            self.doc.add_on_layer(g, &layer, color);
        }
        self.doc.commit();
        Ok(n)
    }

    /// DGNATTACH: a DGN file placed at 0,0 as a block reference.
    pub(crate) fn dgn_attach(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("DGN", &["dgn"]).pick_file() else { return };
        let objects = match Self::read_dgn_file(&file) {
            Ok(o) => o,
            Err(e) => return self.engine.log(e),
        };
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let base = oxidraft_model::sanitize_name(&format!("DGN-{stem}"));
        let name = (1..)
            .map(|i| if i == 1 { base.clone() } else { format!("{base}({i})") })
            .find(|n| self.doc.block(n).is_none())
            .unwrap_or(base);
        let entities = objects
            .into_iter()
            .map(|(g, layer, color)| {
                let color = if color == 0 { Color::ByBlock } else { Color::Index(color) };
                BlockEntity::new(layer, color, g)
            })
            .collect();
        self.engine.cancel(&mut self.doc);
        self.doc.begin("DGNATTACH");
        self.doc.define_block(Block { name: name.clone(), base: Point::ORIGIN, entities });
        self.doc.add(Geometry::Insert {
            block: name,
            placement: Placement::at(Point::ORIGIN),
            attributes: Vec::new(),
            clip: None,
        });
        self.doc.commit();
        self.engine.log(format!("{} attached at 0,0 as a block reference.", file.display()));
    }

    /// DGNEXPORT: model space to a DGN file picked, V8 or V7.
    pub(crate) fn dgn_export(&mut self, v8: bool) {
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(path) =
            rfd::FileDialog::new().add_filter("DGN", &["dgn"]).set_file_name(format!("{stem}.dgn")).save_file()
        else {
            return;
        };
        if let Err(e) = self.dgn_export_to(&path, v8) {
            self.engine.log(e);
        }
    }

    pub fn dgn_export_to(&mut self, path: &Path, v8: bool) -> Result<(), String> {
        let data = if v8 { crate::dgn8::write_dgn8(&self.doc)? } else { write_dgn(&self.doc) };
        std::fs::write(path, data).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        self.engine.log(format!("Saved {} (DGN {})", path.display(), if v8 { "V8" } else { "V7" }));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vax_numbers_both_ways() {
        for v in [1.0, -2.5, 1234.5678, 0.001, 1e9] {
            assert_eq!(vax_double(&put_vax_double(v)), v, "{v}");
        }
        assert_eq!(int32(&put_int32(-123_456)), -123_456);
    }

    #[test]
    fn a_dgn_written_reads_back() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 50.0) });
        doc.add(Geometry::Circle { center: Point::new(20.0, 20.0), radius: 5.0 });
        doc.add(Geometry::Arc { center: Point::new(50.0, 0.0), radius: 10.0, start_angle: 0.0, end_angle: 90.0 });
        doc.add(Geometry::Polyline {
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)].map(|(x, y)| Vertex::at(Point::new(x, y))).to_vec(),
            closed: false,
        });
        doc.add(Geometry::Text {
            position: Point::new(5.0, 60.0),
            height: 2.5,
            rotation: 30.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "ROOM".into(),
            look: Default::default(),
        });
        let back = read_dgn(&write_dgn(&doc)).unwrap();
        let has = |f: &dyn Fn(&Geometry) -> bool| back.iter().any(|(g, _, _)| f(g));
        assert!(has(&|g| matches!(g, Geometry::Line { end, .. } if end.approx_eq(Point::new(100.0, 50.0)))));
        assert!(has(
            &|g| matches!(g, Geometry::Circle { center, radius } if center.approx_eq(Point::new(20.0, 20.0)) && (radius - 5.0).abs() < 1e-9)
        ));
        assert!(has(&|g| matches!(g, Geometry::Arc { end_angle, .. } if (end_angle - 90.0).abs() < 1e-6)));
        assert!(has(&|g| matches!(g, Geometry::Polyline { vertices, .. } if vertices.len() == 3)));
        assert!(has(
            &|g| matches!(g, Geometry::Text { content, height, rotation, .. } if content == "ROOM" && (height - 2.5).abs() < 1e-3 && (rotation - 30.0).abs() < 1e-6)
        ));
        assert!(read_dgn(&[0xD0, 0xCF, 0x11, 0xE0, 0, 0]).unwrap_err().contains("DGN V8"));
    }
}

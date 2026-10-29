//! A plotted sheet as a Windows Metafile (WMFOUT): a placeable metafile
//! of pens and polylines, brushes and polygons, and text in the style's
//! typeface, turned as it's drawn; other programs scale it as they place
//! it.

use crate::sheet::{Item, Rgb, Sheet};

/// A metafile being written: its records, and the largest one's size.
struct Wmf {
    records: Vec<u8>,
    largest: u32,
    /// Logical units per millimeter, and the sheet's height (WMF runs down).
    k: f64,
    height: f64,
}

impl Wmf {
    fn record(&mut self, function: u16, params: &[u16]) {
        let words = 3 + params.len() as u32;
        self.largest = self.largest.max(words);
        self.records.extend_from_slice(&words.to_le_bytes());
        self.records.extend_from_slice(&function.to_le_bytes());
        for p in params {
            self.records.extend_from_slice(&p.to_le_bytes());
        }
    }

    fn x(&self, mm: f64) -> u16 {
        (mm * self.k).round().clamp(-32767.0, 32767.0) as i16 as u16
    }

    fn y(&self, mm: f64) -> u16 {
        ((self.height - mm) * self.k).round().clamp(-32767.0, 32767.0) as i16 as u16
    }

    fn color(rgb: Rgb) -> [u16; 2] {
        [u16::from(rgb[0]) | (u16::from(rgb[1]) << 8), u16::from(rgb[2])]
    }

    /// A pen (object 0) or brush (object 1) made and chosen.
    fn pen(&mut self, rgb: Rgb, width_mm: f64) {
        let [lo, hi] = Self::color(rgb);
        let w = (width_mm * self.k).round().clamp(0.0, 32767.0) as u16;
        self.record(0x02FA, &[0, w, 0, lo, hi]);
        self.record(0x012D, &[0]);
    }

    fn brush(&mut self, rgb: Rgb) {
        let [lo, hi] = Self::color(rgb);
        self.record(0x02FC, &[0, lo, hi, 0]);
        self.record(0x012D, &[1]);
    }

    fn delete(&mut self, object: u16) {
        self.record(0x01F0, &[object]);
    }

    fn points(&self, pts: &[oxidraft_geom::Point]) -> Vec<u16> {
        pts.iter().flat_map(|p| [self.x(p.x), self.y(p.y)]).collect()
    }

    fn items(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Stroke { paths, width, color, .. } => {
                    self.pen(*color, *width);
                    for p in paths {
                        let mut pts = p.points.clone();
                        if p.closed && pts.len() > 2 {
                            pts.push(pts[0]);
                        }
                        for chunk in pts.chunks(8000).filter(|c| c.len() >= 2) {
                            let mut params = vec![chunk.len() as u16];
                            params.extend(self.points(chunk));
                            self.record(0x0325, &params);
                        }
                    }
                    self.delete(0);
                }
                Item::Fill { polygons, color } => {
                    let polygons: Vec<_> = polygons.iter().filter(|p| p.len() >= 3).collect();
                    if polygons.is_empty() {
                        continue;
                    }
                    self.pen(*color, 0.0);
                    self.brush(*color);
                    // Even-odd, as the sheet fills.
                    self.record(0x0106, &[1]);
                    let mut params = vec![polygons.len() as u16];
                    params.extend(polygons.iter().map(|p| p.len() as u16));
                    for p in &polygons {
                        params.extend(self.points(p));
                    }
                    self.record(0x0538, &params);
                    self.delete(1);
                    self.delete(0);
                }
                Item::Text { at, rotation, size, text, color, width_factor, font, .. } => {
                    let face = crate::font::typeface(font);
                    // The font's height is its em, negative: the character height.
                    let height = -((size * self.k).round().clamp(1.0, 32767.0) as i16);
                    let width = if (*width_factor - 1.0).abs() > 1e-9 {
                        // Average character width, as Windows measures it.
                        (size * self.k * 0.5 * width_factor).round().clamp(1.0, 32767.0) as i16
                    } else {
                        0
                    };
                    let tenths = (rotation.rem_euclid(360.0) * 10.0).round() as i16;
                    // Then weight, and italic, underline and the like (all 0).
                    let mut params = vec![height as u16, width as u16, tenths as u16, tenths as u16, 400, 0, 0, 0, 0];
                    let mut name = [0u8; 32];
                    let family = face.family();
                    for (i, b) in family.bytes().take(31).enumerate() {
                        name[i] = b;
                    }
                    params.extend(name.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
                    self.record(0x02FB, &params);
                    self.record(0x012D, &[0]);
                    let [lo, hi] = Self::color(*color);
                    self.record(0x0209, &[lo, hi]);
                    // Transparent, from the baseline's start.
                    self.record(0x0102, &[1]);
                    self.record(0x012E, &[24]);
                    let bytes: Vec<u8> = text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect();
                    let mut params = vec![self.y(at.y), self.x(at.x), bytes.len() as u16, 0];
                    let mut padded = bytes.clone();
                    if padded.len() % 2 == 1 {
                        padded.push(0);
                    }
                    params.extend(padded.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
                    self.record(0x0A32, &params);
                    self.delete(0);
                }
                Item::Clipped { clip, items, .. } => {
                    self.record(0x001E, &[]);
                    let (l, t, r, b) = (self.x(clip.min.x), self.y(clip.max.y), self.x(clip.max.x), self.y(clip.min.y));
                    self.record(0x0416, &[b, r, t, l]);
                    self.items(items);
                    self.record(0x0127, &[(-1i16) as u16]);
                }
                Item::Image { .. } => {}
            }
        }
    }
}

/// The sheet as a placeable metafile's bytes.
pub fn wmf(sheet: &Sheet) -> Vec<u8> {
    let l = &sheet.layout;
    // As fine as the coordinates' 16 bits allow, up to 1440 to the inch.
    let per_inch = (32000.0 / (l.width.max(l.height) / 25.4)).min(1440.0).floor();
    let k = per_inch / 25.4;
    let mut w = Wmf { records: Vec::new(), largest: 0, k, height: l.height };
    let (width, height) = ((l.width * k).round() as u16, (l.height * k).round() as u16);
    // Anisotropic mapping over the sheet, so programs can scale it.
    w.record(0x0103, &[8]);
    w.record(0x020B, &[0, 0]);
    w.record(0x020C, &[height, width]);
    w.items(&sheet.items);
    w.record(0x0000, &[]);
    let mut out = Vec::new();
    // The placeable header: its key, bounds, units to the inch, checksum.
    let header: [u16; 10] = [0xCDD7, 0x9AC6, 0, 0, 0, width, height, per_inch as u16, 0, 0];
    let checksum = header.iter().fold(0u16, |a, w| a ^ w);
    for v in header {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&checksum.to_le_bytes());
    // The metafile header: in memory, 9 words, version 3.
    let size = 9 + w.records.len() as u32 / 2;
    for v in [1u16, 9, 0x0300] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&w.largest.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend(w.records);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{PlotArea, PlotSettings};
    use oxidraft_geom::Point;
    use oxidraft_model::{Attach, Document, Geometry};

    /// The function of each record after the headers.
    fn functions(data: &[u8]) -> Vec<u16> {
        let mut at = 22 + 18;
        let mut out = Vec::new();
        while at + 6 <= data.len() {
            let words = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
            out.push(u16::from_le_bytes([data[at + 4], data[at + 5]]));
            at += words * 2;
        }
        out
    }

    #[test]
    fn records_for_lines_fills_and_text() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 50.0) });
        let square: Vec<_> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .iter()
            .map(|&(x, y)| oxidraft_model::Vertex::at(Point::new(x, y)))
            .collect();
        doc.add(Geometry::Hatch {
            loops: vec![square],
            pattern: oxidraft_model::HatchPattern::Solid,
            scale: 1.0,
            angle: 0.0,
            origin: Point::ORIGIN,
        });
        doc.add(Geometry::Text {
            position: Point::new(10.0, 40.0),
            height: 5.0,
            rotation: 30.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "WALL".into(),
            look: Default::default(),
        });
        let settings = PlotSettings { area: PlotArea::Extents, ..PlotSettings::default() };
        let sheet = crate::render(&doc, oxidraft_model::MODEL, &settings).unwrap();
        let data = wmf(&sheet);
        assert_eq!(&data[..4], &[0xD7, 0xCD, 0xC6, 0x9A], "placeable");
        let f = functions(&data);
        for want in [0x0103, 0x02FA, 0x0325, 0x02FC, 0x0538, 0x02FB, 0x0A32] {
            assert!(f.contains(&want), "{want:#06X} in {f:X?}");
        }
        assert_eq!(f.last(), Some(&0), "ends with EOF");
        // The size in the header is the file's, in words.
        let size = u32::from_le_bytes([data[28], data[29], data[30], data[31]]) as usize;
        assert_eq!(size * 2, data.len() - 22);
    }
}

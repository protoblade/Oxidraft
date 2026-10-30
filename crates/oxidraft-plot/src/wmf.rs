//! A plotted sheet as a Windows Metafile (WMFOUT): a placeable metafile
//! of pens and polylines, brushes and polygons, text in the style's
//! typeface, turned as it's drawn, and pictures as bitmaps over their
//! bounds; other programs scale it as they place it.

use oxidraft_geom::Point;

use crate::sheet::{Item, Rgb, Sheet};

/// A metafile being written: its records, and the largest one's size.
struct Wmf {
    records: Vec<u8>,
    largest: u32,
    /// Logical units per millimeter, and the sheet's height (WMF runs down).
    k: f64,
    height: f64,
    pictures: std::collections::HashMap<String, std::sync::Arc<crate::Picture>>,
}

/// The most pixels a side a picture keeps: metafiles stay a sensible size.
const MOST_PIXELS: u32 = 2000;

/// A picture turned or slanted over the parallelogram from `origin` along
/// `u` and `v`, made again upright over its bounds (`bounds` min, its
/// size), the rest white: metafiles don't turn bitmaps.
fn upright(p: &crate::Picture, origin: Point, u: Point, v: Point, min: Point, size: Point) -> crate::Picture {
    // As fine as the picture is along its sides, up to MOST_PIXELS.
    let k = (f64::from(p.width) / u.length().max(1e-9)).max(f64::from(p.height) / v.length().max(1e-9));
    let k = k.min(f64::from(MOST_PIXELS) / size.x.max(size.y).max(1e-9));
    let (w, h) = (((size.x * k).ceil() as u32).max(1), ((size.y * k).ceil() as u32).max(1));
    let det = u.x * v.y - u.y * v.x;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let q = Point::new(min.x + (f64::from(x) + 0.5) / k, min.y + size.y - (f64::from(y) + 0.5) / k) - origin;
            // Where it is along u (s) and v (t).
            let (s, t) = ((q.x * v.y - q.y * v.x) / det, (u.x * q.y - u.y * q.x) / det);
            if det.abs() < 1e-12 || !(0.0..1.0).contains(&s) || !(0.0..1.0).contains(&t) {
                rgba.extend([255, 255, 255, 0]);
                continue;
            }
            let (sx, sy) = ((s * f64::from(p.width)) as u32, ((1.0 - t) * f64::from(p.height)) as u32);
            let i = ((sy.min(p.height - 1) * p.width + sx.min(p.width - 1)) * 4) as usize;
            rgba.extend_from_slice(&p.rgba[i..i + 4]);
        }
    }
    crate::Picture { width: w, height: h, rgba }
}

/// A device-independent bitmap of the picture: 24-bit, rows bottom to top,
/// over white; scaled down to fit [`MOST_PIXELS`].
fn dib(p: &crate::Picture, gray: bool) -> (u32, u32, Vec<u8>) {
    let shrink = (p.width.max(p.height) as f64 / MOST_PIXELS as f64).max(1.0);
    let (w, h) = (((p.width as f64 / shrink) as u32).max(1), ((p.height as f64 / shrink) as u32).max(1));
    let row = (w as usize * 3).div_ceil(4) * 4;
    let mut out = Vec::with_capacity(40 + row * h as usize);
    for v in [40u32, w, h] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    for v in [0u32, (row * h as usize) as u32, 0, 0, 0, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for y in (0..h).rev() {
        let sy = ((y as f64 * shrink) as u32).min(p.height - 1);
        let start = out.len();
        for x in 0..w {
            let sx = ((x as f64 * shrink) as u32).min(p.width - 1);
            let i = ((sy * p.width + sx) * 4) as usize;
            let px = [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2], p.rgba[i + 3]];
            let [r, g, b] = crate::png::over_white(px, gray);
            out.extend_from_slice(&[b, g, r]);
        }
        out.resize(start + row, 0);
    }
    (w, h, out)
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
                // Over the bounds of its parallelogram: metafiles don't turn bitmaps.
                Item::Image { name, origin, u, v, gray, .. } => {
                    let Some(p) = self.pictures.get(name).cloned() else { continue };
                    if p.width == 0 || p.height == 0 || p.rgba.len() < (p.width * p.height * 4) as usize {
                        continue;
                    }
                    let corners = [*origin, *origin + *u, *origin + *u + *v, *origin + *v];
                    let (min_x, max_x) =
                        corners.iter().fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c.x), b.max(c.x)));
                    let (min_y, max_y) =
                        corners.iter().fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c.y), b.max(c.y)));
                    // Turned, slanted or flipped: made upright over its bounds.
                    let square = u.y.abs() < 1e-9 && v.x.abs() < 1e-9 && u.x > 0.0 && v.y > 0.0;
                    let p = if square {
                        p
                    } else {
                        let (min, size) = (Point::new(min_x, min_y), Point::new(max_x - min_x, max_y - min_y));
                        std::sync::Arc::new(upright(&p, *origin, *u, *v, min, size))
                    };
                    let (w, h, bits) = dib(&p, *gray);
                    let (left, top) = (self.x(min_x), self.y(max_y));
                    let width = (self.x(max_x) as i16).saturating_sub(left as i16) as u16;
                    let height = (self.y(min_y) as i16).saturating_sub(top as i16) as u16;
                    // SRCCOPY, RGB colors, the whole source, the destination.
                    let mut params = vec![0x0020, 0x00CC, 0, h as u16, w as u16, 0, 0, height, width, top, left];
                    params.extend(bits.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])));
                    self.record(0x0F43, &params);
                }
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
    let mut w = Wmf { records: Vec::new(), largest: 0, k, height: l.height, pictures: sheet.pictures.clone() };
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

    fn picture_sheet() -> crate::Sheet {
        let mut doc = Document::new();
        doc.add(Geometry::Image {
            adjust: Default::default(),
            image: "ortho".into(),
            origin: Point::new(0.0, 0.0),
            u: Point::new(100.0, 0.0),
            v: Point::new(0.0, 50.0),
            clip: Vec::new(),
        });
        let picture =
            std::sync::Arc::new(crate::Picture { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 255] });
        let settings = PlotSettings { area: PlotArea::Extents, ..PlotSettings::default() };
        crate::render_with_pictures(&doc, oxidraft_model::MODEL, &settings, &|_| Some(picture.clone())).unwrap()
    }

    #[test]
    fn pictures_as_bitmaps() {
        let data = wmf(&picture_sheet());
        assert!(functions(&data).contains(&0x0F43), "StretchDIB");
        // Its bitmap: 2 by 1, 24-bit, red then blue (BGR), the row padded to 8 bytes.
        let at = data.windows(4).position(|w| w == [40, 0, 0, 0]).unwrap();
        assert_eq!(&data[at + 4..at + 12], &[2, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(&data[at + 40..at + 48], &[0, 0, 255, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn a_turned_picture_made_upright() {
        // Red then blue left to right, turned a quarter: red at the bottom.
        let p = crate::Picture { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 255] };
        let out = upright(
            &p,
            Point::new(10.0, 0.0),
            Point::new(0.0, 10.0),
            Point::new(-5.0, 0.0),
            Point::new(5.0, 0.0),
            Point::new(5.0, 10.0),
        );
        let px = |x: u32, y: u32| {
            let i = ((y * out.width + x) * 4) as usize;
            [out.rgba[i], out.rgba[i + 1], out.rgba[i + 2]]
        };
        let (w, h) = (out.width, out.height);
        assert!(h > w, "standing up: {w} by {h}");
        assert_eq!(px(w / 2, h - 1), [255, 0, 0], "red at the bottom");
        assert_eq!(px(w / 2, 0), [0, 0, 255], "blue at the top");
    }
}

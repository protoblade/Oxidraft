//! Plotted sheets as DWF 6 (EXPORTDWF, the DWF6 ePlot plotter): the
//! `(DWF V06.00)` package — a zip of a manifest and, for each sheet, an
//! ePlot section with its descriptor (the paper) and its graphics as a W2D
//! stream. The stream is written in W2D's readable opcodes: lines and
//! polylines with absolute coordinates, colors, line weights, filled
//! contours, text in its font and pictures as PNG. Logical coordinates
//! are thousandths of a millimeter on the sheet.

use std::fmt::Write as _;

use oxidraft_geom::Point;

use crate::font;
use crate::sheet::{Item, Rgb, Sheet};
use crate::zip::zip;

/// Logical units (W2D integers) per millimeter.
pub const UNITS_PER_MM: f64 = 1000.0;

fn logical(v: f64) -> i64 {
    (v * UNITS_PER_MM).round() as i64
}

/// A point in logical units, which run from zero up.
fn xy(p: Point) -> String {
    format!("{},{}", logical(p.x).max(0), logical(p.y).max(0))
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A W2D quoted string: `'` and `\` escaped.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Angles in 360/65536ths of a degree.
fn angle(degrees: f64) -> i64 {
    (degrees.rem_euclid(360.0) * 65536.0 / 360.0).round() as i64 % 65536
}

/// An object id as DWF writes them (a GUID's form), from `seed`.
fn object_id(seed: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut words = [0u64; 2];
    for (k, w) in words.iter_mut().enumerate() {
        for b in seed.bytes().chain(std::iter::once(k as u8)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        *w = h;
    }
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        words[0] >> 32,
        (words[0] >> 16) & 0xffff,
        words[0] & 0xffff,
        words[1] >> 48,
        words[1] & 0xffff_ffff_ffff
    )
}

/// The W2D writer: the stream, and what's set (to set only changes).
struct W2d {
    out: Vec<u8>,
    color: Option<Rgb>,
    weight: Option<i64>,
    /// The current point, which binary records' points are relative to.
    current: (i64, i64),
}

impl W2d {
    fn new() -> Self {
        Self { out: b"(W2D V06.00)".to_vec(), color: None, weight: None, current: (0, 0) }
    }

    fn text(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    fn set_color(&mut self, c: Rgb) {
        if self.color != Some(c) {
            self.text(&format!("(Color {},{},{},255)", c[0], c[1], c[2]));
            self.color = Some(c);
        }
    }

    fn set_weight(&mut self, mm: f64) {
        let w = logical(mm);
        if self.weight != Some(w) {
            self.text(&format!("(LineWeight {w})"));
            self.weight = Some(w);
        }
    }

    /// A polyline through `points` (closed back to the first).
    fn polyline(&mut self, points: &[Point], closed: bool) {
        if points.len() < 2 {
            return;
        }
        let mut all: Vec<String> = points.iter().map(|p| xy(*p)).collect();
        if closed {
            all.push(xy(points[0]));
        }
        self.text(&format!("P {} {}\n", all.len(), all.join(" ")));
    }

    /// Filled areas, even-odd: one contour set.
    fn fill(&mut self, polygons: &[Vec<Point>]) {
        let polygons: Vec<&Vec<Point>> = polygons.iter().filter(|p| p.len() >= 3).collect();
        if polygons.is_empty() {
            return;
        }
        let counts: Vec<String> = polygons.iter().map(|p| p.len().to_string()).collect();
        let points: Vec<String> = polygons.iter().flat_map(|p| p.iter().map(|q| xy(*q))).collect();
        self.text(&format!("(Contour {} {} {})\n", polygons.len(), counts.join(" "), points.join(" ")));
    }

    /// Sets the current point (binary `O`).
    fn origin(&mut self, x: i64, y: i64) {
        self.out.push(b'O');
        self.out.extend_from_slice(&(x as i32).to_le_bytes());
        self.out.extend_from_slice(&(y as i32).to_le_bytes());
        self.current = (x, y);
    }

    /// A point relative to the current one, which it becomes.
    fn relative(&mut self, x: i64, y: i64) {
        let (dx, dy) = (x - self.current.0, y - self.current.1);
        self.out.extend_from_slice(&(dx as i32).to_le_bytes());
        self.out.extend_from_slice(&(dy as i32).to_le_bytes());
        self.current = (x, y);
    }

    /// A PNG picture over the box from `min` to `max` (extended binary
    /// 0x000C).
    fn png(&mut self, id: i32, columns: u32, rows: u32, min: Point, max: Point, png: &[u8]) {
        self.origin(0, 0);
        let mut body = Vec::new();
        body.extend_from_slice(&0x000Cu16.to_le_bytes());
        body.extend_from_slice(&(columns.min(65535) as u16).to_le_bytes());
        body.extend_from_slice(&(rows.min(65535) as u16).to_le_bytes());
        let start = self.out.len();
        self.relative(logical(min.x).max(0), logical(min.y).max(0));
        self.relative(logical(max.x).max(0), logical(max.y).max(0));
        let points = self.out.split_off(start);
        body.extend_from_slice(&points);
        body.extend_from_slice(&id.to_le_bytes());
        body.extend_from_slice(&(png.len() as i32).to_le_bytes());
        body.extend_from_slice(png);
        body.push(b'}');
        self.out.push(b'{');
        self.out.extend_from_slice(&(body.len() as i32).to_le_bytes());
        self.out.extend_from_slice(&body);
    }

    fn items(&mut self, items: &[Item], sheet: &Sheet, images: &mut i32) {
        for item in items {
            match item {
                Item::Stroke { paths, width, color, .. } => {
                    self.set_color(*color);
                    self.set_weight(*width);
                    for p in paths {
                        self.polyline(&p.points, p.closed);
                    }
                }
                Item::Fill { polygons, color } => {
                    self.set_color(*color);
                    self.fill(polygons);
                }
                Item::Text { at, rotation, size, text, color, width_factor, oblique, font: key } => {
                    self.set_color(*color);
                    let face = font::typeface(key);
                    let name = if face.is_default() { "Arial".to_string() } else { face.name().to_string() };
                    self.text(&format!(
                        "(Font (Name {}) (Height {}) (Rotation {}) (Widthscale {}) (Oblique {}))",
                        quoted(&name),
                        logical(*size),
                        angle(*rotation),
                        (width_factor * 1024.0).round() as i64,
                        angle(*oblique)
                    ));
                    self.text(&format!("(Text {} {})\n", xy(*at), quoted(text)));
                }
                Item::Image { name, origin, u, v, clip: _, gray } => {
                    let Some(p) = sheet.pictures.get(name) else { continue };
                    let corners = [*origin, *origin + *u, *origin + *v, *origin + *u + *v];
                    let min = Point::new(
                        corners.iter().map(|c| c.x).fold(f64::INFINITY, f64::min),
                        corners.iter().map(|c| c.y).fold(f64::INFINITY, f64::min),
                    );
                    let max = Point::new(
                        corners.iter().map(|c| c.x).fold(f64::NEG_INFINITY, f64::max),
                        corners.iter().map(|c| c.y).fold(f64::NEG_INFINITY, f64::max),
                    );
                    *images += 1;
                    self.png(*images, p.width, p.height, min, max, &crate::png::png(p, *gray));
                }
                // A viewport's contents (W2D has no clipping here beyond
                // the paper's).
                Item::Clipped { items, .. } => self.items(items, sheet, images),
            }
        }
    }
}

/// The W2D stream of one sheet.
pub fn w2d(sheet: &Sheet) -> Vec<u8> {
    let mut w = W2d::new();
    let mut images = 0;
    w.items(&sheet.items, sheet, &mut images);
    w.text("(EndOfDWF)");
    w.out
}

/// Sheets as one DWF 6 file, a section each.
pub fn dwf(sheets: &[Sheet], title: &str) -> Vec<u8> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut sections = String::new();
    for (i, sheet) in sheets.iter().enumerate() {
        let l = &sheet.layout;
        let id = object_id(&format!("{title}/{i}"));
        let folder = format!("com.autodesk.dwf.ePlot_{id}");
        let stream = w2d(sheet);
        let name = if sheets.len() > 1 { format!("{title} - {}", i + 1) } else { title.to_string() };
        let mm = 1.0 / UNITS_PER_MM;
        let transform = format!("{mm} 0 0 0 0 {mm} 0 0 0 0 1 0 0 0 0 1");
        let clip = format!("{} {} {} {}", l.clip.min.x, l.clip.min.y, l.clip.max.x, l.clip.max.y);
        let descriptor = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
             <ePlot:Page xmlns:ePlot=\"DWF-ePlot:1.2\" version=\"1.2\" plotOrder=\"{}\" name=\"{}\" objectId=\"{id}\">\
             <ePlot:Paper show=\"true\" units=\"mm\" width=\"{}\" height=\"{}\" clip=\"{clip}\" color=\"255 255 255\"/>\
             <ePlot:Resources>\
             <ePlot:GraphicResource role=\"2d streaming graphics\" mime=\"application/x-w2d\" href=\"{folder}\\{id}.w2d\" \
             size=\"{}\" transform=\"{transform}\" clip=\"{clip}\" objectId=\"{}\"/>\
             </ePlot:Resources></ePlot:Page>",
            i + 1,
            escape_xml(&name),
            l.width,
            l.height,
            stream.len(),
            object_id(&format!("{title}/{i}/w2d")),
        );
        let _ = write!(
            sections,
            "<dwf:Section type=\"com.autodesk.dwf.ePlot\" name=\"{folder}\" title=\"{}\" version=\"1.2\" objectId=\"{id}\">\
             <dwf:Source provider=\"Oxidraft\" href=\"\" objectId=\"\"/>\
             <dwf:Toc>\
             <dwf:Resource role=\"descriptor\" mime=\"text/xml\" href=\"{folder}\\descriptor.xml\"/>\
             <dwf:Resource role=\"2d streaming graphics\" mime=\"application/x-w2d\" href=\"{folder}\\{id}.w2d\"/>\
             </dwf:Toc></dwf:Section>",
            escape_xml(&name)
        );
        files.push((format!("{folder}/descriptor.xml"), descriptor.into_bytes()));
        files.push((format!("{folder}/{id}.w2d"), stream));
    }
    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <dwf:Manifest xmlns:dwf=\"DWF-Manifest:6.0\" version=\"6.0\" objectId=\"{}\">\
         <dwf:Properties><dwf:Property name=\"Creator\" value=\"Oxidraft\"/>\
         <dwf:Property name=\"Title\" value=\"{}\"/></dwf:Properties>\
         <dwf:Interfaces><dwf:Interface name=\"ePlot\" href=\"\" objectId=\"{}\"/></dwf:Interfaces>\
         <dwf:Sections>{sections}</dwf:Sections></dwf:Manifest>",
        object_id(title),
        escape_xml(title),
        object_id(&format!("{title}/interface")),
    );
    let mut package = vec![("manifest.xml".to_string(), manifest.into_bytes())];
    package.extend(files);
    let mut out = b"(DWF V06.00)".to_vec();
    out.extend(zip(&package));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet::Path;

    fn sheet() -> Sheet {
        let layout = crate::setup::Layout {
            width: 297.0,
            height: 210.0,
            window: oxidraft_geom::Aabb::from_points(Point::ORIGIN, Point::new(297.0, 210.0)),
            scale: 1.0,
            origin: Point::ORIGIN,
            clip: oxidraft_geom::Aabb::from_points(Point::new(5.0, 5.0), Point::new(292.0, 205.0)),
            printable: oxidraft_geom::Aabb::from_points(Point::new(5.0, 5.0), Point::new(292.0, 205.0)),
        };
        Sheet {
            layout,
            items: vec![
                Item::Stroke {
                    paths: vec![Path { points: vec![Point::new(10.0, 10.0), Point::new(100.0, 50.0)], closed: false }],
                    width: 0.35,
                    color: [255, 0, 0],
                    wide: false,
                },
                Item::Fill {
                    polygons: vec![vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0), Point::new(5.0, 5.0)]],
                    color: [0, 0, 255],
                },
                Item::Text {
                    at: Point::new(20.0, 30.0),
                    rotation: 90.0,
                    size: 2.5,
                    text: "It's".into(),
                    color: [0, 0, 0],
                    width_factor: 1.0,
                    oblique: 0.0,
                    font: String::new(),
                },
            ],
            pictures: Default::default(),
        }
    }

    #[test]
    fn a_w2d_stream_in_readable_opcodes() {
        let text = String::from_utf8(w2d(&sheet())).unwrap();
        assert!(text.starts_with("(W2D V06.00)") && text.ends_with("(EndOfDWF)"));
        assert!(text.contains("(Color 255,0,0,255)(LineWeight 350)P 2 10000,10000 100000,50000\n"), "{text}");
        assert!(text.contains("(Contour 1 3 0,0 5000,0 5000,5000)"));
        assert!(text.contains("(Rotation 16384)") && text.contains("(Text 20000,30000 'It\\'s')"));
    }

    #[test]
    fn a_package_with_its_manifest_and_sections() {
        let data = dwf(&[sheet(), sheet()], "Plan");
        assert!(data.starts_with(b"(DWF V06.00)PK"));
        // The zip's files, stored or deflated.
        let zip = &data[12..];
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut at = 0;
        while zip.get(at..at + 4) == Some(&0x0403_4b50u32.to_le_bytes()) {
            let u16_at = |i: usize| u16::from_le_bytes([zip[i], zip[i + 1]]) as usize;
            let u32_at = |i: usize| u32::from_le_bytes([zip[i], zip[i + 1], zip[i + 2], zip[i + 3]]) as usize;
            let (method, size, name_len) = (u16_at(at + 8), u32_at(at + 18), u16_at(at + 26));
            let name = String::from_utf8_lossy(&zip[at + 30..at + 30 + name_len]).into_owned();
            let body = &zip[at + 30 + name_len..at + 30 + name_len + size];
            let content = if method == 8 {
                let mut v = Vec::new();
                std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(body), &mut v).unwrap();
                v
            } else {
                body.to_vec()
            };
            files.push((name, content));
            at += 30 + name_len + size;
        }
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names.len(), 5, "{names:?}");
        let manifest = String::from_utf8(files[0].1.clone()).unwrap();
        assert_eq!(manifest.matches("<dwf:Section ").count(), 2);
        assert!(names.iter().any(|n| n.ends_with("descriptor.xml")) && names.iter().any(|n| n.ends_with(".w2d")));
    }
}

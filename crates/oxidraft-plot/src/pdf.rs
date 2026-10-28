//! A plotted sheet as a one-page PDF: vector lines and fills, and text in
//! the embedded typeface, so it can be searched and copied.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;

use crate::{
    font::FONT,
    sheet::{Item, Path, Rgb, Sheet},
};
use flate2::{Compression, write::ZlibEncoder};

/// PDF points per millimeter.
const PT_PER_MM: f64 = 72.0 / 25.4;

/// A number as short as PDF allows: no exponent, no trailing zeros.
fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn compress(data: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(data).expect("writing to memory");
    z.finish().expect("writing to memory")
}

/// Builds the file object by object, keeping each one's offset for the
/// cross-reference table.
struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    fn object(&mut self, id: usize, body: &str) {
        self.offsets[id] = self.out.len();
        self.out.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
    }

    fn stream(&mut self, id: usize, dict: &str, data: &[u8]) {
        self.offsets[id] = self.out.len();
        let head = format!("{id} 0 obj\n<< {dict} /Length {} /Filter /FlateDecode >>\nstream\n", data.len());
        self.out.extend_from_slice(head.as_bytes());
        self.out.extend_from_slice(data);
        self.out.extend_from_slice(b"\nendstream\nendobj\n");
    }
}

fn color(rgb: Rgb) -> String {
    rgb.map(|c| num(f64::from(c) / 255.0)).join(" ")
}

fn path_ops(ops: &mut String, path: &Path) {
    for (i, p) in path.points.iter().enumerate() {
        let _ = writeln!(ops, "{} {} {}", num(p.x), num(p.y), if i == 0 { "m" } else { "l" });
    }
    if path.closed {
        ops.push_str("h\n");
    }
}

/// What a page draws with besides lines: the glyphs its text uses (with
/// the character each one shows), and its pictures (name, in grays).
#[derive(Default)]
struct Used {
    glyphs: BTreeMap<u16, char>,
    pictures: Vec<(String, bool)>,
}

/// The page's drawing operators, and what they use.
fn content(sheet: &Sheet) -> (String, Used) {
    let mut ops = String::new();
    let mut used = Used::default();
    let clip = sheet.layout.clip;
    // Millimeters to points; round ends and joins; plot only inside the window.
    let _ = writeln!(ops, "{0} 0 0 {0} 0 0 cm", num(PT_PER_MM));
    ops.push_str("1 J 1 j\n");
    let _ =
        writeln!(ops, "{} {} {} {} re W n", num(clip.min.x), num(clip.min.y), num(clip.width()), num(clip.height()));
    for item in &sheet.items {
        match item {
            Item::Stroke { paths, width, color: c, wide } => {
                ops.push_str(if *wide { "q 0 J 0 j 10 M\n" } else { "q\n" });
                let _ = writeln!(ops, "{} RG {} w", color(*c), num(*width));
                for p in paths {
                    path_ops(&mut ops, p);
                }
                ops.push_str("S Q\n");
            }
            Item::Fill { polygons, color: c } => {
                let _ = writeln!(ops, "{} rg", color(*c));
                for p in polygons {
                    path_ops(&mut ops, &Path { points: p.clone(), closed: true });
                }
                ops.push_str("f*\n");
            }
            Item::Text { at, rotation, size, text, color: c } => {
                let (sin, cos) = rotation.to_radians().sin_cos();
                let mut hex = String::new();
                for ch in text.chars() {
                    let g = FONT.glyph(ch);
                    used.glyphs.entry(g).or_insert(ch);
                    let _ = write!(hex, "{g:04X}");
                }
                let _ = writeln!(
                    ops,
                    "BT {} rg /F1 {} Tf {} {} {} {} {} {} Tm <{hex}> Tj ET",
                    color(*c),
                    num(*size),
                    num(cos),
                    num(sin),
                    num(-sin),
                    num(cos),
                    num(at.x),
                    num(at.y)
                );
            }
            // The picture fills the unit square; the matrix stretches that
            // over its parallelogram.
            Item::Image { name, origin, u, v, clip, gray } => {
                if !sheet.pictures.contains_key(name) {
                    continue;
                }
                let key = (name.clone(), *gray);
                let index = used.pictures.iter().position(|p| *p == key).unwrap_or_else(|| {
                    used.pictures.push(key);
                    used.pictures.len() - 1
                });
                ops.push_str("q\n");
                if clip.len() >= 3 {
                    path_ops(&mut ops, &Path { points: clip.clone(), closed: true });
                    ops.push_str("W n\n");
                }
                let m = [u.x, u.y, v.x, v.y, origin.x, origin.y].map(num).join(" ");
                let _ = writeln!(ops, "{m} cm /Im{index} Do Q");
            }
        }
    }
    (ops, used)
}

/// A picture's pixels for PDF: RGB (or gray) over a white background.
fn picture_samples(p: &crate::Picture, gray: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.rgba.len() / 4 * if gray { 1 } else { 3 });
    for px in p.rgba.as_chunks::<4>().0 {
        let a = u32::from(px[3]);
        let over_white = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        let (r, g, b) = (over_white(px[0]), over_white(px[1]), over_white(px[2]));
        if gray {
            out.push(((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8);
        } else {
            out.extend([r, g, b]);
        }
    }
    out
}

/// The CMap that tells readers which character each glyph is, for search
/// and copy.
fn to_unicode(glyphs: &BTreeMap<u16, char>) -> String {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = glyphs.iter().collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(s, "{} beginbfchar", chunk.len());
        for (g, c) in chunk {
            let units: String = c.encode_utf16(&mut [0; 2]).iter().map(|u| format!("{u:04X}")).collect();
            let _ = writeln!(s, "<{g:04X}> <{units}>");
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}

/// Escapes text for a PDF string literal.
fn literal(text: &str) -> String {
    let mut s = String::from("(");
    for c in text.chars().filter(char::is_ascii) {
        if matches!(c, '(' | ')' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push(')');
    s
}

/// The sheet as a PDF file. `title` goes in the document properties.
pub fn pdf(sheet: &Sheet, title: &str) -> Vec<u8> {
    let (ops, used) = content(sheet);
    let glyphs = &used.glyphs;
    let with_text = !glyphs.is_empty();
    // 1 catalog, 2 pages, 3 page, 4 content, 5 info; 6-10 the font; then pictures.
    let first_picture = if with_text { 11 } else { 6 };
    let count = first_picture + used.pictures.len();
    let mut w = Writer { out: b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec(), offsets: vec![0; count] };
    let (width, height) = (sheet.layout.width * PT_PER_MM, sheet.layout.height * PT_PER_MM);
    w.object(1, "<< /Type /Catalog /Pages 2 0 R >>");
    w.object(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let mut fonts = if with_text { "/Font << /F1 6 0 R >>".to_string() } else { String::new() };
    if !used.pictures.is_empty() {
        let refs: Vec<String> = (0..used.pictures.len()).map(|i| format!("/Im{i} {} 0 R", first_picture + i)).collect();
        let _ = write!(fonts, " /XObject << {} >>", refs.join(" "));
    }
    w.object(
        3,
        &format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << {fonts} >> /Contents 4 0 R >>",
            num(width),
            num(height)
        ),
    );
    w.stream(4, "", &compress(ops.as_bytes()));
    w.object(5, &format!("<< /Title {} /Producer (Oxidraft {}) >>", literal(title), env!("CARGO_PKG_VERSION")));
    if with_text {
        let name = FONT.name();
        let em = |v: f64| num(v * 1000.0);
        w.object(
            6,
            &format!(
                "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
                 /DescendantFonts [7 0 R] /ToUnicode 10 0 R >>"
            ),
        );
        let widths: String =
            glyphs.keys().map(|&g| format!("{g} [{}]", em(FONT.advance(g)))).collect::<Vec<_>>().join(" ");
        w.object(
            7,
            &format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
                 /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                 /FontDescriptor 8 0 R /DW 500 /W [{widths}] /CIDToGIDMap /Identity >>"
            ),
        );
        let [x0, y0, x1, y1] = FONT.bbox();
        w.object(
            8,
            &format!(
                "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle 0 \
                 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 9 0 R >>",
                em(x0),
                em(y0),
                em(x1),
                em(y1),
                em(FONT.ascent()),
                em(FONT.descent()),
                em(FONT.cap_height())
            ),
        );
        w.stream(9, &format!("/Length1 {}", FONT.data().len()), &compress(FONT.data()));
        w.stream(10, "", &compress(to_unicode(glyphs).as_bytes()));
    }
    for (i, (name, gray)) in used.pictures.iter().enumerate() {
        let Some(p) = sheet.pictures.get(name) else { continue };
        let space = if *gray { "/DeviceGray" } else { "/DeviceRGB" };
        let dict = format!(
            "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {space} /BitsPerComponent 8 /Interpolate true",
            p.width, p.height
        );
        w.stream(first_picture + i, &dict, &compress(&picture_samples(p, *gray)));
    }
    let xref = w.out.len();
    let mut tail = format!("xref\n0 {count}\n0000000000 65535 f \n");
    for offset in &w.offsets[1..] {
        let _ = writeln!(tail, "{offset:010} 00000 n ");
    }
    let _ = write!(tail, "trailer\n<< /Size {count} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n");
    w.out.extend_from_slice(tail.as_bytes());
    w.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{PlotArea, PlotSettings};
    use oxidraft_geom::{Aabb, Point};
    use oxidraft_model::{Attach, Document, Geometry};

    #[test]
    fn pictures_become_image_objects() {
        use oxidraft_model::Geometry;
        let mut doc = Document::new();
        doc.add(Geometry::Image {
            image: "ortho".into(),
            origin: Point::new(0.0, 0.0),
            u: Point::new(100.0, 0.0),
            v: Point::new(0.0, 50.0),
            clip: vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(50.0, 50.0)],
        });
        doc.add(Geometry::Image {
            image: "missing".into(),
            origin: Point::ORIGIN,
            u: Point::new(1.0, 0.0),
            v: Point::new(0.0, 1.0),
            clip: Vec::new(),
        });
        let picture =
            std::sync::Arc::new(crate::Picture { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 0] });
        let settings = PlotSettings {
            area: PlotArea::Window(Aabb::from_points(Point::ORIGIN, Point::new(100.0, 50.0))),
            ..PlotSettings::default()
        };
        let lookup = |n: &str| (n == "ortho").then(|| picture.clone());
        let sheet = crate::render_with_pictures(&doc, &settings, &lookup).unwrap();
        assert_eq!(sheet.items.len(), 1, "the unloaded one plots nothing");
        let bytes = pdf(&sheet, "ortho");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/XObject << /Im0 6 0 R >>"));
        assert!(text.contains("/Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB"));
        let (ops, _) = content(&sheet);
        assert!(ops.contains("W n") && ops.contains("cm /Im0 Do Q"), "{ops}");
        // A transparent pixel shows the white paper.
        assert_eq!(picture_samples(&picture, false), vec![255, 0, 0, 255, 255, 255]);
        assert_eq!(picture_samples(&picture, true), vec![76, 255]);
    }

    #[test]
    fn a_well_formed_page_with_embedded_text() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(100.0, 50.0) });
        doc.add(Geometry::Text {
            position: Point::new(10.0, 10.0),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "Działka (12)".into(),
        });
        let settings = PlotSettings {
            area: PlotArea::Window(Aabb::from_points(Point::ORIGIN, Point::new(100.0, 50.0))),
            ..PlotSettings::default()
        };
        let sheet = crate::render(&doc, &settings).unwrap();
        let bytes = pdf(&sheet, "Plan (ground floor)");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.4") && text.trim_end().ends_with("%%EOF"));
        assert!(text.contains("/MediaBox [0 0 841.8898 595.2756]"), "A4 landscape");
        assert!(text.contains("/FontFile2 9 0 R") && text.contains("/Title (Plan \\(ground floor\\))"));
        // Every object starts where the cross-reference table says.
        let find = |needle: &[u8]| bytes.windows(needle.len()).rposition(|w| w == needle).unwrap();
        let xref = find(b"\nxref\n") + 1;
        let table = String::from_utf8_lossy(&bytes[xref..]).to_string();
        for (i, line) in table.lines().skip(3).take(10).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            assert!(bytes[offset..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
        }
        let start: usize = table[table.find("startxref\n").unwrap() + 10..].lines().next().unwrap().parse().unwrap();
        assert_eq!(start, xref);
        // The glyphs shown map back to their characters.
        let (_, used) = content(&sheet);
        assert!(used.glyphs.values().any(|&c| c == 'ł'));
        assert!(to_unicode(&used.glyphs).contains("<0142>"));
    }
}

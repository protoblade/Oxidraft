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

/// The page's drawing operators, and the glyphs its text uses (with the
/// character each one shows).
fn content(sheet: &Sheet) -> (String, BTreeMap<u16, char>) {
    let mut ops = String::new();
    let mut glyphs = BTreeMap::new();
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
                    glyphs.entry(g).or_insert(ch);
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
        }
    }
    (ops, glyphs)
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
    let (ops, glyphs) = content(sheet);
    let with_text = !glyphs.is_empty();
    // 1 catalog, 2 pages, 3 page, 4 content, 5 info; 6-10 the font.
    let count = if with_text { 11 } else { 6 };
    let mut w = Writer { out: b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec(), offsets: vec![0; count] };
    let (width, height) = (sheet.layout.width * PT_PER_MM, sheet.layout.height * PT_PER_MM);
    w.object(1, "<< /Type /Catalog /Pages 2 0 R >>");
    w.object(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let fonts = if with_text { "/Font << /F1 6 0 R >>" } else { "" };
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
        w.stream(10, "", &compress(to_unicode(&glyphs).as_bytes()));
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
        let (_, glyphs) = content(&sheet);
        assert!(glyphs.values().any(|&c| c == 'ł'));
        assert!(to_unicode(&glyphs).contains("<0142>"));
    }
}

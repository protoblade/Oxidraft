//! A plotted sheet as a one-page PDF: vector lines and fills, and text in
//! the embedded typeface, so it can be searched and copied.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;

use crate::{
    font::{self, Typeface},
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

/// What a page draws with besides lines: the glyphs its text uses in
/// each typeface (with the character each one shows), and its pictures
/// (name, in grays).
#[derive(Default)]
struct Used {
    glyphs: BTreeMap<String, BTreeMap<u16, char>>,
    pictures: Vec<(String, bool)>,
}

/// The typefaces used, in the order their /F resources are numbered.
fn typefaces(fonts: &BTreeMap<String, BTreeMap<u16, char>>) -> Vec<&'static Typeface> {
    fonts.keys().map(|k| font::typeface(k)).collect()
}

/// A typeface's resource number: its place among the fonts used.
fn font_number(fonts: &[String], key: &str) -> usize {
    fonts.iter().position(|k| k == key).unwrap_or(0) + 1
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
    item_ops(&mut ops, &mut used, sheet, &sheet.items);
    (ops, used)
}

/// The operators that draw `items`, and what they use.
fn item_ops(ops: &mut String, used: &mut Used, sheet: &Sheet, items: &[Item]) {
    for item in items {
        match item {
            Item::Stroke { paths, width, color: c, wide } => {
                ops.push_str(if *wide { "q 0 J 0 j 10 M\n" } else { "q\n" });
                let _ = writeln!(ops, "{} RG {} w", color(*c), num(*width));
                for p in paths {
                    path_ops(ops, p);
                }
                ops.push_str("S Q\n");
            }
            Item::Fill { polygons, color: c } => {
                let _ = writeln!(ops, "{} rg", color(*c));
                for p in polygons {
                    path_ops(ops, &Path { points: p.clone(), closed: true });
                }
                ops.push_str("f*\n");
            }
            Item::Text { at, rotation, size, text, color: c, width_factor, oblique, font: key } => {
                // Rotation after stretching along the line and slanting.
                let (sin, cos) = rotation.to_radians().sin_cos();
                let t = oblique.to_radians().tan();
                let (a, b, cc, d) = (cos * width_factor, sin * width_factor, cos * t - sin, sin * t + cos);
                let face = font::typeface(key);
                let key = face.key().to_string();
                let mut hex = String::new();
                for ch in text.chars() {
                    let g = face.glyph(ch);
                    used.glyphs.entry(key.clone()).or_default().entry(g).or_insert(ch);
                    let _ = write!(hex, "{g:04X}");
                }
                // Numbered once the page's fonts are all known.
                let _ = writeln!(
                    ops,
                    "BT {} rg /F{{{key}}} {} Tf {} {} {} {} {} {} Tm <{hex}> Tj ET",
                    color(*c),
                    num(*size),
                    num(a),
                    num(b),
                    num(cc),
                    num(d),
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
                    path_ops(ops, &Path { points: clip.clone(), closed: true });
                    ops.push_str("W n\n");
                }
                let m = [u.x, u.y, v.x, v.y, origin.x, origin.y].map(num).join(" ");
                let _ = writeln!(ops, "{m} cm /Im{index} Do Q");
            }
            // Cut to the frame, and to a non-rectangular viewport's boundary.
            Item::Clipped { clip, shape, items } => {
                let _ = writeln!(
                    ops,
                    "q {} {} {} {} re W n",
                    num(clip.min.x),
                    num(clip.min.y),
                    num(clip.width()),
                    num(clip.height())
                );
                if shape.len() >= 3 {
                    path_ops(ops, &Path { points: shape.clone(), closed: true });
                    ops.push_str("W n\n");
                }
                item_ops(ops, used, sheet, items);
                ops.push_str("Q\n");
            }
        }
    }
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
    pdf_pages(std::slice::from_ref(sheet), title)
}

/// A typeface as objects `id` to `id + 4`: the font, its glyphs' widths,
/// its descriptor, the font file, and what each glyph reads as.
fn embed_font(w: &mut Writer, id: usize, face: &Typeface, glyphs: &BTreeMap<u16, char>) {
    let name = face.name();
    let em = |v: f64| num(v * 1000.0);
    // TrueType outlines, or an OpenType font's CFF ones.
    let (cid_type, file_key, file_dict) = if face.is_cff() {
        ("CIDFontType0", "FontFile3", "/Subtype /OpenType".to_string())
    } else {
        ("CIDFontType2", "FontFile2", format!("/Length1 {}", face.data().len()))
    };
    w.object(
        id,
        &format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
             /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>",
            id + 1,
            id + 4
        ),
    );
    let widths: String = glyphs.keys().map(|&g| format!("{g} [{}]", em(face.advance(g)))).collect::<Vec<_>>().join(" ");
    let gid_map = if face.is_cff() { "" } else { " /CIDToGIDMap /Identity" };
    w.object(
        id + 1,
        &format!(
            "<< /Type /Font /Subtype /{cid_type} /BaseFont /{name} \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor {} 0 R /DW 500 /W [{widths}]{gid_map} >>",
            id + 2
        ),
    );
    let [x0, y0, x1, y1] = face.bbox();
    w.object(
        id + 2,
        &format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle 0 \
             /Ascent {} /Descent {} /CapHeight {} /StemV 80 /{file_key} {} 0 R >>",
            em(x0),
            em(y0),
            em(x1),
            em(y1),
            em(face.ascent()),
            em(face.descent()),
            em(face.cap_height()),
            id + 3
        ),
    );
    w.stream(id + 3, &file_dict, &compress(face.data()));
    w.stream(id + 4, "", &compress(to_unicode(glyphs).as_bytes()));
}

/// Sheets as one PDF file, a page each (EXPORTPDF's All Layouts,
/// PUBLISH). The typeface is embedded once for all of them.
pub fn pdf_pages(sheets: &[Sheet], title: &str) -> Vec<u8> {
    let mut pages: Vec<(String, Used)> = sheets.iter().map(content).collect();
    let mut fonts: BTreeMap<String, BTreeMap<u16, char>> = BTreeMap::new();
    for (_, used) in &pages {
        for (key, glyphs) in &used.glyphs {
            fonts.entry(key.clone()).or_default().extend(glyphs.iter().map(|(g, c)| (*g, *c)));
        }
    }
    let keys: Vec<String> = fonts.keys().cloned().collect();
    // Each text's font by its number among them.
    for (ops, _) in &mut pages {
        for key in &keys {
            *ops = ops.replace(&format!("/F{{{key}}} "), &format!("/F{} ", font_number(&keys, key)));
        }
    }
    // 1 catalog, 2 pages, 3 info; 5 for each font; then each page, its
    // content and its pictures.
    let first_font = 4;
    let mut ids = Vec::new();
    let mut next = first_font + 5 * fonts.len();
    for (_, used) in &pages {
        ids.push(next);
        next += 2 + used.pictures.len();
    }
    // OpenType (CFF) fonts came with PDF 1.6.
    let version = if typefaces(&fonts).iter().any(|f| f.is_cff()) { "1.6" } else { "1.4" };
    let mut head = format!("%PDF-{version}\n").into_bytes();
    head.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");
    let mut w = Writer { out: head, offsets: vec![0; next] };
    w.object(1, "<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = ids.iter().map(|id| format!("{id} 0 R")).collect();
    w.object(2, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), ids.len()));
    w.object(3, &format!("<< /Title {} /Producer (Oxidraft {}) >>", literal(title), env!("CARGO_PKG_VERSION")));
    for (i, (face, glyphs)) in typefaces(&fonts).into_iter().zip(fonts.values()).enumerate() {
        embed_font(&mut w, first_font + 5 * i, face, glyphs);
    }
    let resources_fonts: String =
        (0..keys.len()).map(|i| format!("/F{} {} 0 R", i + 1, first_font + 5 * i)).collect::<Vec<_>>().join(" ");
    for ((sheet, (ops, used)), &id) in sheets.iter().zip(&pages).zip(&ids) {
        let mut resources =
            if used.glyphs.is_empty() { String::new() } else { format!("/Font << {resources_fonts} >>") };
        if !used.pictures.is_empty() {
            let refs: Vec<String> = (0..used.pictures.len()).map(|i| format!("/Im{i} {} 0 R", id + 2 + i)).collect();
            let _ = write!(resources, " /XObject << {} >>", refs.join(" "));
        }
        let (width, height) = (sheet.layout.width * PT_PER_MM, sheet.layout.height * PT_PER_MM);
        w.object(
            id,
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << {resources} >> /Contents {} 0 R >>",
                num(width),
                num(height),
                id + 1
            ),
        );
        w.stream(id + 1, "", &compress(ops.as_bytes()));
        for (i, (name, gray)) in used.pictures.iter().enumerate() {
            let Some(p) = sheet.pictures.get(name) else { continue };
            let space = if *gray { "/DeviceGray" } else { "/DeviceRGB" };
            let dict = format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {space} /BitsPerComponent 8 /Interpolate true",
                p.width, p.height
            );
            w.stream(id + 2 + i, &dict, &compress(&picture_samples(p, *gray)));
        }
    }
    let xref = w.out.len();
    let mut tail = format!("xref\n0 {next}\n0000000000 65535 f \n");
    for offset in &w.offsets[1..] {
        let _ = writeln!(tail, "{offset:010} 00000 n ");
    }
    let _ = write!(tail, "trailer\n<< /Size {next} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n");
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
            adjust: Default::default(),
            image: "ortho".into(),
            origin: Point::new(0.0, 0.0),
            u: Point::new(100.0, 0.0),
            v: Point::new(0.0, 50.0),
            clip: vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(50.0, 50.0)],
        });
        doc.add(Geometry::Image {
            adjust: Default::default(),
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
        // IMAGEFRAME 1 plots the frames too, the unloaded one's as well.
        let framed = crate::render_with_pictures(&doc, oxidraft_model::MODEL, &settings, &lookup).unwrap();
        let strokes = framed.items.iter().filter(|i| matches!(i, crate::Item::Stroke { .. })).count();
        assert!(strokes >= 1, "{:?}", framed.items);
        doc.set_picture_frame(false, 2);
        let sheet = crate::render_with_pictures(&doc, oxidraft_model::MODEL, &settings, &lookup).unwrap();
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
            look: Default::default(),
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
        let sheet = crate::render(&doc, oxidraft_model::MODEL, &settings).unwrap();
        let bytes = pdf(&sheet, "Plan (ground floor)");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.4") && text.trim_end().ends_with("%%EOF"));
        assert!(text.contains("/MediaBox [0 0 841.8898 595.2756]"), "A4 landscape");
        assert!(text.contains("/FontFile2 7 0 R") && text.contains("/Title (Plan \\(ground floor\\))"));
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
        // Several sheets: a page each, the typeface once.
        let small = PlotSettings { paper: crate::PAPERS[0], ..settings };
        let two = crate::render(&doc, oxidraft_model::MODEL, &small).unwrap();
        let bytes = pdf_pages(&[sheet.clone(), two], "Both");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Kids [9 0 R 11 0 R] /Count 2"));
        assert_eq!(text.matches("/Type /Page ").count(), 2);
        assert_eq!(text.matches("/FontFile2").count(), 1);
        let xref = bytes.windows(6).rposition(|w| w == b"\nxref\n").unwrap() + 1;
        let table = String::from_utf8_lossy(&bytes[xref..]).to_string();
        assert!(table.starts_with("xref\n0 13\n"));
        for (i, line) in table.lines().skip(3).take(12).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            assert!(bytes[offset..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
        }
        // A narrow, slanted style: the text matrix stretches and shears.
        let mut styled = Document::new();
        styled.add(Geometry::Text {
            position: Point::new(10.0, 10.0),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "A".into(),
            look: oxidraft_model::TextLook {
                style: "Narrow".into(),
                width_factor: 0.5,
                oblique: 45.0,
                ..Default::default()
            },
        });
        let narrow = crate::render(&styled, oxidraft_model::MODEL, &settings).unwrap();
        let (ops, _) = content(&narrow);
        assert!(ops.contains(" 0.5 0 1 1 "), "{ops}");
        // The glyphs shown map back to their characters.
        let (_, used) = content(&sheet);
        // In the Standard style's font (Arial where there is one).
        let default = used.glyphs.values().next().unwrap();
        assert!(default.values().any(|&c| c == 'ł'));
        assert!(to_unicode(default).contains("<0142>"));
    }

    #[test]
    fn a_styles_font_is_embedded_beside_the_default() {
        // The bundled typeface as a style's font file, beside the drawing.
        let dir = std::env::temp_dir().join(format!("oxidraft-pdf-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Stencil.ttf"), epaint_default_fonts::UBUNTU_LIGHT).unwrap();
        crate::font::add_search_dir(&dir);
        let mut doc = Document::new();
        let mut style = oxidraft_model::TextStyle::new("Signs");
        style.font = "stencil.ttf".into();
        doc.set_text_style(style).unwrap();
        for (style, y) in [("Signs", 10.0), ("Standard", 20.0)] {
            doc.add(Geometry::Text {
                position: Point::new(10.0, y),
                height: 2.5,
                rotation: 0.0,
                width: 0.0,
                attach: Attach::BottomLeft,
                content: "EXIT".into(),
                look: oxidraft_model::TextLook { style: style.into(), ..Default::default() },
            });
        }
        let settings = PlotSettings { area: PlotArea::Extents, ..PlotSettings::default() };
        let sheet = crate::render(&doc, oxidraft_model::MODEL, &settings).unwrap();
        let fonts: Vec<&str> = sheet
            .items
            .iter()
            .filter_map(|i| if let Item::Text { font, .. } = i { Some(font.as_str()) } else { None })
            .collect();
        assert_eq!(fonts.len(), 2);
        assert!(fonts.contains(&"stencil.ttf"), "{fonts:?}");
        let pdf = String::from_utf8_lossy(&pdf_pages(&[sheet], "t")).into_owned();
        assert!(pdf.contains("/Font << /F1 4 0 R /F2 9 0 R >>"), "two fonts");
        assert!(pdf.contains("/F1 ") && pdf.contains("/F2 ") && !pdf.contains("/F{"));
        std::fs::remove_dir_all(dir).ok();
    }
}

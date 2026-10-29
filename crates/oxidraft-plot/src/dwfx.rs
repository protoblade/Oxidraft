//! Plotted sheets as DWFx (EXPORTDWFX, the DWFx ePlot plotter): the XML
//! Paper Specification package Autodesk's DWFx builds on, a fixed page a
//! sheet, its lines and fills as paths and its text as glyphs in the
//! embedded fonts. Viewers of DWFx and XPS files open it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use oxidraft_geom::Point;

use crate::font::{self, Typeface};
use crate::sheet::{Item, Rgb, Sheet};
use crate::zip::zip;

/// XPS units (1/96 inch) per millimeter.
const PX_PER_MM: f64 = 96.0 / 25.4;

fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn color(rgb: Rgb) -> String {
    format!("#FF{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Path data for `points` on a sheet `height` high (XPS runs down).
fn data(paths: &[(Vec<Point>, bool)], height: f64) -> String {
    let at = |p: &Point| format!("{},{}", num(p.x * PX_PER_MM), num((height - p.y) * PX_PER_MM));
    let mut d = String::new();
    for (points, closed) in paths.iter().filter(|(p, _)| p.len() >= 2) {
        let _ = write!(d, "M {} L", at(&points[0]));
        for p in &points[1..] {
            let _ = write!(d, " {}", at(p));
        }
        d.push_str(if *closed { " Z " } else { " " });
    }
    d.trim_end().to_string()
}

/// Fonts numbered across the pages (by key), and those a page uses.
struct Fonts {
    numbers: BTreeMap<String, usize>,
    on_page: BTreeSet<usize>,
}

/// The page's elements.
fn elements(out: &mut String, items: &[Item], height: f64, fonts: &mut Fonts) {
    for item in items {
        match item {
            Item::Stroke { paths, width, color: c, wide } => {
                let paths: Vec<(Vec<Point>, bool)> = paths.iter().map(|p| (p.points.clone(), p.closed)).collect();
                let d = data(&paths, height);
                if d.is_empty() {
                    continue;
                }
                let (cap, join) = if *wide { ("Square", "Miter") } else { ("Round", "Round") };
                let _ = writeln!(
                    out,
                    "<Path Data=\"{d}\" Stroke=\"{}\" StrokeThickness=\"{}\" StrokeStartLineCap=\"{cap}\" \
                     StrokeEndLineCap=\"{cap}\" StrokeLineJoin=\"{join}\"/>",
                    color(*c),
                    num((width * PX_PER_MM).max(0.5))
                );
            }
            Item::Fill { polygons, color: c } => {
                let paths: Vec<(Vec<Point>, bool)> = polygons.iter().map(|p| (p.clone(), true)).collect();
                let d = data(&paths, height);
                if !d.is_empty() {
                    let _ = writeln!(out, "<Path Data=\"F 0 {d}\" Fill=\"{}\"/>", color(*c));
                }
            }
            Item::Text { at, rotation, size, text, color: c, width_factor, oblique, font: key } => {
                let face = font::typeface(key);
                let next = fonts.numbers.len() + 1;
                let n = *fonts.numbers.entry(face.key().to_string()).or_insert(next);
                fonts.on_page.insert(n);
                // Text space to the page, which runs down.
                let (sin, cos) = rotation.to_radians().sin_cos();
                let t = oblique.to_radians().tan();
                let (a, b, cc, d) = (cos * width_factor, sin * width_factor, cos * t - sin, sin * t + cos);
                let unicode = if text.starts_with('{') { format!("{{}}{text}") } else { text.clone() };
                let _ = writeln!(
                    out,
                    "<Glyphs FontUri=\"/Resources/Fonts/{n}.ttf\" FontRenderingEmSize=\"{}\" OriginX=\"0\" OriginY=\"0\" \
                     UnicodeString=\"{}\" Fill=\"{}\" RenderTransform=\"{},{},{},{},{},{}\"/>",
                    num(size * PX_PER_MM),
                    escape(&unicode),
                    color(*c),
                    num(a),
                    num(-b),
                    num(-cc),
                    num(d),
                    num(at.x * PX_PER_MM),
                    num((height - at.y) * PX_PER_MM)
                );
            }
            Item::Clipped { clip, shape, items } => {
                let frame = if shape.len() >= 3 { shape.clone() } else { clip.corners().to_vec() };
                let _ = writeln!(out, "<Canvas Clip=\"{}\">", data(&[(frame, true)], height));
                elements(out, items, height, fonts);
                out.push_str("</Canvas>\n");
            }
            // Pictures aren't carried yet.
            Item::Image { .. } => {}
        }
    }
}

const XPS: &str = "http://schemas.microsoft.com/xps/2005/06";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Sheets as one DWFx file, a page each.
pub fn dwfx(sheets: &[Sheet], title: &str) -> Vec<u8> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut fonts = Fonts { numbers: BTreeMap::new(), on_page: BTreeSet::new() };
    let mut pages = String::new();
    for (i, sheet) in sheets.iter().enumerate() {
        let n = i + 1;
        let l = &sheet.layout;
        let mut body = String::new();
        fonts.on_page.clear();
        elements(&mut body, &sheet.items, l.height, &mut fonts);
        let clip = &l.clip;
        let page = format!(
            "<FixedPage xmlns=\"{XPS}\" Width=\"{}\" Height=\"{}\" xml:lang=\"und\">\n\
             <Path Data=\"M 0,0 L {w},0 {w},{h} 0,{h} Z\" Fill=\"#FFFFFFFF\"/>\n\
             <Canvas Clip=\"{}\">\n{body}</Canvas>\n</FixedPage>\n",
            num(l.width * PX_PER_MM),
            num(l.height * PX_PER_MM),
            data(&[(clip.corners().to_vec(), true)], l.height),
            w = num(l.width * PX_PER_MM),
            h = num(l.height * PX_PER_MM),
        );
        files.push((format!("Documents/1/Pages/{n}.fpage"), page.into_bytes()));
        let rels: String = fonts
            .on_page
            .iter()
            .map(|f| {
                format!(
                    "<Relationship Id=\"F{f}\" Type=\"{XPS}/required-resource\" Target=\"/Resources/Fonts/{f}.ttf\"/>"
                )
            })
            .collect();
        files.push((
            format!("Documents/1/Pages/_rels/{n}.fpage.rels"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"{RELS}\">{rels}</Relationships>"
            )
            .into_bytes(),
        ));
        let _ = write!(pages, "<PageContent Source=\"Pages/{n}.fpage\"/>");
    }
    for (key, n) in &fonts.numbers {
        let face: &Typeface = font::typeface(key);
        files.push((format!("Resources/Fonts/{n}.ttf"), face.data().to_vec()));
    }
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
    let mut package = vec![
        (
            "[Content_Types].xml".to_string(),
            format!(
                "{head}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
                 <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
                 <Default Extension=\"fdseq\" ContentType=\"application/vnd.ms-package.xps-fixeddocumentsequence+xml\"/>\
                 <Default Extension=\"fdoc\" ContentType=\"application/vnd.ms-package.xps-fixeddocument+xml\"/>\
                 <Default Extension=\"fpage\" ContentType=\"application/vnd.ms-package.xps-fixedpage+xml\"/>\
                 <Default Extension=\"ttf\" ContentType=\"application/vnd.ms-opentype\"/>\
                 <Default Extension=\"xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
                 </Types>"
            )
            .into_bytes(),
        ),
        (
            "_rels/.rels".to_string(),
            format!(
                "{head}<Relationships xmlns=\"{RELS}\">\
                 <Relationship Id=\"R0\" Type=\"{XPS}/fixedrepresentation\" Target=\"/FixedDocumentSequence.fdseq\"/>\
                 <Relationship Id=\"R1\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"/docProps/core.xml\"/>\
                 </Relationships>"
            )
            .into_bytes(),
        ),
        (
            "docProps/core.xml".to_string(),
            format!(
                "{head}<coreProperties xmlns=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
                 xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>Oxidraft</dc:creator></coreProperties>",
                escape(title)
            )
            .into_bytes(),
        ),
        (
            "FixedDocumentSequence.fdseq".to_string(),
            format!(
                "{head}<FixedDocumentSequence xmlns=\"{XPS}\"><DocumentReference Source=\"/Documents/1/FixedDocument.fdoc\"/></FixedDocumentSequence>"
            )
            .into_bytes(),
        ),
        (
            "Documents/1/FixedDocument.fdoc".to_string(),
            format!("{head}<FixedDocument xmlns=\"{XPS}\">{pages}</FixedDocument>").into_bytes(),
        ),
    ];
    package.extend(files);
    zip(&package)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{PlotArea, PlotSettings};
    use oxidraft_model::{Attach, Document, Geometry};

    /// The names and contents of a zip's files, as stored (deflated or not).
    fn unzip(data: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 0;
        while data.get(at..at + 4) == Some(&0x0403_4b50u32.to_le_bytes()) {
            let u16_at = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]) as usize;
            let u32_at = |i: usize| u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
            let (method, size, name_len) = (u16_at(at + 8), u32_at(at + 18), u16_at(at + 26));
            let name = String::from_utf8_lossy(&data[at + 30..at + 30 + name_len]).into_owned();
            let body = &data[at + 30 + name_len..at + 30 + name_len + size];
            let content = if method == 8 {
                let mut d = flate2::read::DeflateDecoder::new(body);
                let mut v = Vec::new();
                std::io::Read::read_to_end(&mut d, &mut v).unwrap();
                v
            } else {
                body.to_vec()
            };
            out.push((name, content));
            at += 30 + name_len + size;
        }
        out
    }

    #[test]
    fn a_page_of_paths_and_glyphs() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 50.0) });
        doc.add(Geometry::Text {
            position: Point::new(10.0, 40.0),
            height: 5.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "A & B".into(),
            look: Default::default(),
        });
        let settings = PlotSettings { area: PlotArea::Extents, ..PlotSettings::default() };
        let sheet = crate::render(&doc, oxidraft_model::MODEL, &settings).unwrap();
        let files = unzip(&dwfx(&[sheet], "Plan"));
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        for want in ["[Content_Types].xml", "_rels/.rels", "FixedDocumentSequence.fdseq", "Documents/1/Pages/1.fpage"] {
            assert!(names.contains(&want), "{names:?}");
        }
        assert!(names.contains(&"Resources/Fonts/1.ttf"), "the font goes in: {names:?}");
        let page = String::from_utf8(files.iter().find(|(n, _)| n.ends_with("1.fpage")).unwrap().1.clone()).unwrap();
        assert!(page.contains("<Path Data=\"M ") && page.contains("UnicodeString=\"A &amp; B\""), "{page}");
        let rels =
            String::from_utf8(files.iter().find(|(n, _)| n.ends_with("1.fpage.rels")).unwrap().1.clone()).unwrap();
        assert!(rels.contains("/Resources/Fonts/1.ttf"));
    }
}

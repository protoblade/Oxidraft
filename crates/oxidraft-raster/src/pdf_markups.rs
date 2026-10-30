//! A PDF's markups — its annotations: comments (notes and text boxes) and
//! what reviewers drew on it (lines, rectangles, ellipses, polygons and
//! clouds, polylines, ink and highlights) — read so MARKUPIMPORT can bring
//! them in as text and lines, as AutoCAD's Markup Assist offers a markup's
//! text to insert, here without Autodesk's cloud: the PDF says what each
//! markup is. In points from the page's lower-left corner.

use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::dict::keys::{ANNOTS, C, CONTENTS, INKLIST, RECT, SUBTYPE, VERTICES};
use hayro::hayro_syntax::object::{Array, Dict, Name, Rect};

/// One markup, in points from the page's lower-left corner.
#[derive(Debug, Clone, PartialEq)]
pub enum Markup {
    /// A comment's text from its top-left corner, `height` points high.
    Text { at: (f64, f64), height: f64, text: String, color: [u8; 3] },
    /// Lines through points, closed or not.
    Shape { points: Vec<(f64, f64)>, closed: bool, color: [u8; 3] },
}

fn text(bytes: &[u8]) -> String {
    match bytes.strip_prefix(&[0xFE, 0xFF]) {
        Some(rest) => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
            String::from_utf16_lossy(&units)
        }
        None => bytes.iter().map(|b| char::from(*b)).collect(),
    }
}

fn numbers(a: Option<Array<'_>>) -> Vec<f64> {
    a.map(|a| a.iter::<f64>().collect()).unwrap_or_default()
}

/// An annotation's color (gray, RGB or CMYK, 0 to 1), red if it has none.
fn color(d: &Dict<'_>) -> [u8; 3] {
    let c = numbers(d.get::<Array<'_>>(C));
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    match c.as_slice() {
        [g] => [byte(*g); 3],
        [r, g, b] => [byte(*r), byte(*g), byte(*b)],
        [c, m, y, k] => [byte((1.0 - c) * (1.0 - k)), byte((1.0 - m) * (1.0 - k)), byte((1.0 - y) * (1.0 - k))],
        _ => [255, 0, 0],
    }
}

/// A text box's font size, from its default appearance (`/Helv 12 Tf`).
fn font_size(d: &Dict<'_>) -> Option<f64> {
    let da = d.get::<hayro::hayro_syntax::object::String<'_>>(b"DA".as_slice())?;
    let da = text(da.as_bytes());
    let words: Vec<&str> = da.split_whitespace().collect();
    let at = words.iter().position(|w| *w == "Tf")?;
    words.get(at.checked_sub(1)?)?.parse().ok().filter(|s: &f64| *s > 0.0)
}

fn pairs(v: &[f64]) -> Vec<(f64, f64)> {
    v.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect()
}

/// The markups on page `page` (from 0) of the PDF `data`.
pub fn pdf_markups(data: &[u8], page: usize) -> Vec<Markup> {
    let Ok(pdf) = Pdf::new(data.to_vec()) else { return Vec::new() };
    let Some(p) = pdf.pages().get(page) else { return Vec::new() };
    let mb = p.media_box();
    let shift = |(x, y): (f64, f64)| (x - mb.x0, y - mb.y0);
    let Some(annots) = p.raw().get::<Array<'_>>(ANNOTS) else { return Vec::new() };
    let mut out = Vec::new();
    for a in annots.iter::<Dict<'_>>() {
        let Some(kind) = a.get::<Name<'_>>(SUBTYPE) else { continue };
        let rect = a.get::<Rect>(RECT);
        let color = color(&a);
        let contents = a.get::<hayro::hayro_syntax::object::String<'_>>(CONTENTS).map(|s| text(s.as_bytes()));
        let shape = |points: Vec<(f64, f64)>, closed: bool| Markup::Shape {
            points: points.into_iter().map(shift).collect(),
            closed,
            color,
        };
        match kind.as_ref() {
            b"Text" | b"FreeText" => {
                let (Some(r), Some(t)) = (rect, contents.filter(|t| !t.trim().is_empty())) else { continue };
                let height = font_size(&a).unwrap_or(10.0);
                out.push(Markup::Text { at: shift((r.x0, r.y1)), height, text: t.replace('\r', "\n"), color });
            }
            b"Square" => {
                if let Some(r) = rect {
                    out.push(shape(vec![(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)], true));
                }
            }
            b"Circle" => {
                if let Some(r) = rect {
                    let (cx, cy, rx, ry) =
                        ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0, (r.x1 - r.x0) / 2.0, (r.y1 - r.y0) / 2.0);
                    let points = (0..48)
                        .map(|k| {
                            let t = f64::from(k) * std::f64::consts::TAU / 48.0;
                            (cx + rx * t.cos(), cy + ry * t.sin())
                        })
                        .collect();
                    out.push(shape(points, true));
                }
            }
            b"Line" => {
                let l = numbers(a.get::<Array<'_>>(b"L".as_slice()));
                if l.len() == 4 {
                    out.push(shape(pairs(&l), false));
                }
            }
            b"Polygon" | b"PolyLine" => {
                let v = pairs(&numbers(a.get::<Array<'_>>(VERTICES)));
                if v.len() >= 2 {
                    out.push(shape(v, kind.as_ref() == b"Polygon"));
                }
            }
            b"Ink" => {
                if let Some(ink) = a.get::<Array<'_>>(INKLIST) {
                    for stroke in ink.iter::<Array<'_>>() {
                        let v = pairs(&numbers(Some(stroke)));
                        if v.len() >= 2 {
                            out.push(shape(v, false));
                        }
                    }
                }
            }
            b"Highlight" | b"Underline" | b"StrikeOut" | b"Squiggly" => {
                let q = numbers(a.get::<Array<'_>>(b"QuadPoints".as_slice()));
                for quad in q.as_chunks::<8>().0 {
                    // A quad's corners: its top two, then its bottom two.
                    let c = [(quad[0], quad[1]), (quad[2], quad[3]), (quad[6], quad[7]), (quad[4], quad[5])];
                    out.push(shape(c.to_vec(), true));
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF with `annots` (their dictionaries) on it.
    pub(crate) fn pdf_with(annots: &[&str]) -> Vec<u8> {
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        ];
        let refs: Vec<String> = (0..annots.len()).map(|i| format!("{} 0 R", 4 + i)).collect();
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] /Annots [{}] >>", refs.join(" ")));
        objects.extend(annots.iter().map(|a| a.to_string()));
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes(),
        );
        out
    }

    #[test]
    fn comments_and_drawn_markups() {
        let pdf = pdf_with(&[
            "<< /Type /Annot /Subtype /FreeText /Rect [100 300 300 340] /Contents (Move door 200mm) /DA (/Helv 14 Tf 0 0 1 rg) /C [0 0 1] >>",
            "<< /Type /Annot /Subtype /Square /Rect [10 10 50 30] /C [1 0 0] >>",
            "<< /Type /Annot /Subtype /Ink /Rect [0 0 100 100] /InkList [[0 0 10 10 20 0]] >>",
            "<< /Type /Annot /Subtype /Line /Rect [0 0 100 100] /L [5 5 95 95] /C [0 1 0] >>",
            "<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] >>",
        ]);
        let m = pdf_markups(&pdf, 0);
        assert_eq!(m.len(), 4, "{m:?}");
        assert_eq!(
            m[0],
            Markup::Text { at: (100.0, 340.0), height: 14.0, text: "Move door 200mm".into(), color: [0, 0, 255] }
        );
        assert!(matches!(&m[1], Markup::Shape { points, closed: true, color: [255, 0, 0] } if points.len() == 4));
        assert!(matches!(&m[2], Markup::Shape { points, closed: false, .. } if points.len() == 3));
        assert!(matches!(&m[3], Markup::Shape { points, .. } if points == &[(5.0, 5.0), (95.0, 95.0)]));
        assert!(pdf_markups(b"not a pdf", 0).is_empty());
    }
}

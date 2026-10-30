//! DWFx sheets read back: a DWFx (or XPS) package's fixed page — its
//! paths and glyphs, through nested canvases' transforms — as lines,
//! polylines, solid fills and text in millimeters from the sheet's lower
//! left. DWFATTACH places one as a block reference; MARKUP takes DWFx
//! markups too. DWF 6 files go through [`crate::w2d`].

use std::path::Path;

use oxidraft_geom::Point;
use oxidraft_model::{Attach, Block, BlockEntity, Color, Geometry, HatchPattern, Placement, Vertex};

use crate::app::OxidraftApp;

/// Millimeters per XPS unit (1/96 inch).
const MM: f64 = 25.4 / 96.0;

/// An affine transform: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy)]
struct Matrix([f64; 6]);

impl Matrix {
    const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn parse(s: &str) -> Option<Matrix> {
        let v: Vec<f64> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        let a: [f64; 6] = v.try_into().ok()?;
        Some(Matrix(a))
    }

    /// `self` applied after `inner`.
    fn then(self, inner: Matrix) -> Matrix {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = inner.0;
        Matrix([
            a2 * a + b2 * c,
            a2 * b + b2 * d,
            c2 * a + d2 * c,
            c2 * b + d2 * d,
            e2 * a + f2 * c + e,
            e2 * b + f2 * d + f,
        ])
    }

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
}

/// An attribute's value in a tag's text.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    tag[at..].split('"').next()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// `#AARRGGBB` or `#RRGGBB` as RGB.
fn rgb(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().strip_prefix('#')?;
    let hex = if hex.len() == 8 { &hex[2..] } else { hex };
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Path data (absolute and relative M, L, H, V, C, Q, Z; arcs as lines
/// to their ends) as figures: points and whether closed.
fn figures(data: &str) -> Vec<(Vec<(f64, f64)>, bool)> {
    let mut tokens: Vec<String> = Vec::new();
    let mut num = String::new();
    for ch in data.chars() {
        if ch.is_ascii_alphabetic() && ch != 'e' && ch != 'E' {
            if !num.is_empty() {
                tokens.push(std::mem::take(&mut num));
            }
            tokens.push(ch.to_string());
        } else if ch == ',' || ch.is_whitespace() {
            if !num.is_empty() {
                tokens.push(std::mem::take(&mut num));
            }
        } else if ch == '-' && !num.is_empty() && !num.ends_with(['e', 'E']) {
            tokens.push(std::mem::take(&mut num));
            num.push(ch);
        } else {
            num.push(ch);
        }
    }
    if !num.is_empty() {
        tokens.push(num);
    }
    let mut out: Vec<(Vec<(f64, f64)>, bool)> = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    let (mut x, mut y) = (0.0, 0.0);
    let mut cmd = 'M';
    let mut i = 0;
    let n = |i: usize| tokens.get(i).and_then(|t| t.parse::<f64>().ok());
    while i < tokens.len() {
        if let Some(c) = tokens[i].chars().next().filter(|c| c.is_ascii_alphabetic()) {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                // Closed: the pen goes back to where the figure began.
                if let Some(first) = current.first() {
                    (x, y) = *first;
                }
                if current.len() >= 2 {
                    out.push((std::mem::take(&mut current), true));
                }
                current.clear();
                continue;
            }
            if c == 'F' {
                i += 1;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (bx, by) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (Some(a), Some(b)) = (n(i), n(i + 1)) else { break };
                if current.len() >= 2 {
                    out.push((std::mem::take(&mut current), false));
                }
                current.clear();
                (x, y) = (bx + a, by + b);
                current.push((x, y));
                i += 2;
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let (Some(a), Some(b)) = (n(i), n(i + 1)) else { break };
                (x, y) = (bx + a, by + b);
                current.push((x, y));
                i += 2;
            }
            'H' => {
                let Some(a) = n(i) else { break };
                x = bx + a;
                current.push((x, y));
                i += 1;
            }
            'V' => {
                let Some(b) = n(i) else { break };
                y = by + b;
                current.push((x, y));
                i += 1;
            }
            'C' | 'Q' => {
                let k = if cmd.eq_ignore_ascii_case(&'C') { 6 } else { 4 };
                let v: Option<Vec<f64>> = (0..k).map(|j| n(i + j)).collect();
                let Some(v) = v else { break };
                let p0 = (x, y);
                let pts: Vec<(f64, f64)> = v.chunks(2).map(|p| (bx + p[0], by + p[1])).collect();
                for s in 1..=8 {
                    let t = f64::from(s) / 8.0;
                    let lerp = |a: (f64, f64), b: (f64, f64)| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                    let q = if k == 6 {
                        let (a, b, c) = (lerp(p0, pts[0]), lerp(pts[0], pts[1]), lerp(pts[1], pts[2]));
                        lerp(lerp(a, b), lerp(b, c))
                    } else {
                        lerp(lerp(p0, pts[0]), lerp(pts[0], pts[1]))
                    };
                    current.push(q);
                }
                (x, y) = *pts.last().unwrap_or(&p0);
                i += k;
            }
            'A' => {
                let (Some(ex), Some(ey)) = (n(i + 5), n(i + 6)) else { break };
                (x, y) = (bx + ex, by + ey);
                current.push((x, y));
                i += 7;
            }
            _ => i += 1,
        }
    }
    if current.len() >= 2 {
        out.push((current, false));
    }
    out
}

/// A page's drawing: (geometry in millimeters, y up, and its RGB).
pub fn page_objects(xml: &str) -> Vec<(Geometry, [u8; 3])> {
    let height = xml
        .find("<FixedPage")
        .and_then(|i| attr(&xml[i..], "Height"))
        .and_then(|h| h.parse::<f64>().ok())
        .unwrap_or(0.0);
    let to = move |(x, y): (f64, f64)| Point::new(x * MM, (height - y) * MM);
    let mut stack = vec![Matrix::IDENTITY];
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        let tail = &rest[start + 1..];
        let end = tail.find('>').unwrap_or(tail.len());
        let tag = &tail[..end];
        rest = &tail[end..];
        let transform = |tag: &str| attr(tag, "RenderTransform").and_then(Matrix::parse).unwrap_or(Matrix::IDENTITY);
        let top = *stack.last().unwrap_or(&Matrix::IDENTITY);
        if tag.starts_with("Canvas") {
            stack.push(top.then(transform(tag)));
            if tag.ends_with('/') {
                stack.pop();
            }
        } else if tag.starts_with("/Canvas") {
            if stack.len() > 1 {
                stack.pop();
            }
        } else if tag.starts_with("Path ") || tag.starts_with("Path\n") {
            let m = top.then(transform(tag));
            let data = attr(tag, "Data").map(str::to_string).or_else(|| {
                // <Path.Data><PathGeometry Figures="..."/> after the tag.
                let body = &rest[..rest.find("</Path>").unwrap_or(0)];
                body.find("Figures=\"").map(|i| body[i + 9..].split('"').next().unwrap_or("").to_string())
            });
            let Some(data) = data else { continue };
            let figs: Vec<(Vec<Point>, bool)> = figures(&data)
                .into_iter()
                .map(|(p, c)| (p.into_iter().map(|q| to(m.apply(q.0, q.1))).collect(), c))
                .collect();
            if let Some(fill) = attr(tag, "Fill").and_then(rgb).filter(|c| *c != [255, 255, 255]) {
                let loops: Vec<Vec<Vertex>> = figs
                    .iter()
                    .filter(|(p, _)| p.len() >= 3)
                    .map(|(p, _)| p.iter().map(|q| Vertex::at(*q)).collect())
                    .collect();
                if !loops.is_empty() {
                    out.push((
                        Geometry::Hatch {
                            loops,
                            pattern: HatchPattern::Solid,
                            scale: 1.0,
                            angle: 0.0,
                            origin: Point::ORIGIN,
                        },
                        fill,
                    ));
                }
            }
            if let Some(stroke) = attr(tag, "Stroke").and_then(rgb) {
                for (pts, closed) in figs {
                    let g = if pts.len() == 2 && !closed {
                        Geometry::Line { start: pts[0], end: pts[1] }
                    } else {
                        Geometry::Polyline { vertices: pts.into_iter().map(Vertex::at).collect(), closed }
                    };
                    out.push((g, stroke));
                }
            }
        } else if tag.starts_with("Glyphs") {
            let Some(text) = attr(tag, "UnicodeString").map(|t| unescape(t.trim_start_matches("{}"))) else { continue };
            let m = top.then(transform(tag));
            let size = attr(tag, "FontRenderingEmSize").and_then(|s| s.parse::<f64>().ok()).unwrap_or(12.0);
            let ox = attr(tag, "OriginX").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let oy = attr(tag, "OriginY").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let at = to(m.apply(ox, oy));
            let along = to(m.apply(ox + 1.0, oy)) - at;
            let up = to(m.apply(ox, oy - 1.0)) - at;
            let scale = up.length();
            let color = attr(tag, "Fill").and_then(rgb).unwrap_or([0, 0, 0]);
            out.push((
                Geometry::Text {
                    position: at,
                    height: size * scale * 0.7,
                    rotation: Point::ORIGIN.angle_to(along),
                    width: 0.0,
                    attach: Attach::BottomLeft,
                    content: text,
                    look: Default::default(),
                },
                color,
            ));
        }
    }
    out
}

/// A DWF, DWFx or XPS file's first sheet's drawing.
pub fn read_any_dwf(data: &[u8]) -> Result<Vec<(Geometry, [u8; 3])>, String> {
    if data.starts_with(b"(DWF V") { crate::w2d::read_dwf(data) } else { read_dwfx(data) }
}

/// A DWFx (or XPS) file's first page's drawing.
pub fn read_dwfx(data: &[u8]) -> Result<Vec<(Geometry, [u8; 3])>, String> {
    let files = crate::datalink::unzip(data);
    if files.is_empty() {
        return Err("Not a DWF or DWFx file.".into());
    }
    let mut pages: Vec<&(String, Vec<u8>)> =
        files.iter().filter(|(n, _)| n.to_ascii_lowercase().ends_with(".fpage")).collect();
    pages.sort_by(|a, b| a.0.cmp(&b.0));
    let page = pages.first().ok_or("The DWFx file has no sheets.")?;
    Ok(page_objects(&String::from_utf8_lossy(&page.1)))
}

impl OxidraftApp {
    /// DWFATTACH: a DWFx sheet picked, placed at 0,0 as a block reference.
    pub(crate) fn dwf_attach(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("DWF", &["dwf", "dwfx", "xps"]).pick_file() else { return };
        match self.dwf_attach_file(&file) {
            Ok(n) => self.engine.log(format!(
                "{} attached at 0,0 in millimeters: {n} objects. MOVE or SCALE it into place.",
                file.display()
            )),
            Err(e) => self.engine.log(e),
        }
    }

    pub fn dwf_attach_file(&mut self, file: &Path) -> Result<usize, String> {
        let data = std::fs::read(file).map_err(|e| format!("Could not read {}: {e}", file.display()))?;
        let objects = read_any_dwf(&data)?;
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let base = oxidraft_model::sanitize_name(&format!("DWF-{stem}"));
        let name = (1..)
            .map(|i| if i == 1 { base.clone() } else { format!("{base}({i})") })
            .find(|n| self.doc.block(n).is_none())
            .unwrap_or(base);
        let n = objects.len();
        let entities = objects
            .into_iter()
            .map(|(g, c)| BlockEntity::new("0", Color::Index(crate::pdf_import::nearest_aci(c)), g))
            .collect();
        self.engine.cancel(&mut self.doc);
        self.doc.begin("DWFATTACH");
        self.doc.define_block(Block { name: name.clone(), base: Point::ORIGIN, entities });
        self.doc.add(Geometry::Insert {
            block: name,
            placement: Placement::at(Point::ORIGIN),
            attributes: Vec::new(),
            clip: None,
        });
        self.doc.commit();
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dwfx_written_reads_back() {
        let mut doc = oxidraft_model::Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 50.0) });
        doc.add(Geometry::Text {
            position: Point::new(10.0, 40.0),
            height: 5.0,
            rotation: 30.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "WALL".into(),
            look: Default::default(),
        });
        let settings = oxidraft_plot::PlotSettings { area: oxidraft_plot::PlotArea::Extents, ..Default::default() };
        let sheet = oxidraft_plot::render(&doc, oxidraft_model::MODEL, &settings).unwrap();
        let objects = read_dwfx(&oxidraft_plot::dwfx::dwfx(&[sheet], "t")).unwrap();
        let text = objects.iter().find_map(|(g, _)| match g {
            Geometry::Text { content, rotation, .. } => Some((content.clone(), *rotation)),
            _ => None,
        });
        let (content, rotation) = text.expect("the text");
        assert_eq!(content, "WALL");
        assert!((rotation - 30.0).abs() < 0.5, "{rotation}");
        assert!(objects.iter().any(|(g, _)| matches!(g, Geometry::Polyline { .. } | Geometry::Line { .. })));
    }

    #[test]
    fn path_data_with_curves_and_relative_moves() {
        let f = figures("M 0,0 L 10,0 10,10 Z m 5,5 h 3 v 3 C 9,9 10,10 12,12");
        assert!(f[0].1);
        assert_eq!(f[0].0.len(), 3);
        assert_eq!(f[1].0[..3], [(5.0, 5.0), (8.0, 5.0), (8.0, 8.0)]);
        assert_eq!(*f[1].0.last().unwrap(), (12.0, 12.0));
    }
}

//! Text, dimensions and leaders: their layout in drawing units, and how
//! dimensions and leaders break down into lines, arrowheads and text.

use oxidraft_geom::{EPSILON, Point};

use crate::entity::{Geometry, Vertex};
use crate::hatch::HatchPattern;

/// Which point of a text's box its position refers to (MTEXT attachment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Attach {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    /// Also single-line TEXT's default: the start of the baseline.
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Attach {
    /// Where the attachment point sits in the text box, as fractions of its
    /// width (from the left) and height (from the top).
    pub fn fractions(self) -> (f64, f64) {
        let x = match self {
            Attach::TopLeft | Attach::MiddleLeft | Attach::BottomLeft => 0.0,
            Attach::TopCenter | Attach::MiddleCenter | Attach::BottomCenter => 0.5,
            Attach::TopRight | Attach::MiddleRight | Attach::BottomRight => 1.0,
        };
        let y = match self {
            Attach::TopLeft | Attach::TopCenter | Attach::TopRight => 0.0,
            Attach::MiddleLeft | Attach::MiddleCenter | Attach::MiddleRight => 0.5,
            Attach::BottomLeft | Attach::BottomCenter | Attach::BottomRight => 1.0,
        };
        (x, y)
    }
}

/// Replaces AutoCAD's `%%` control codes in TEXT and attribute values:
/// %%d (degree), %%p (plus/minus), %%c (diameter), %%nnn (character code);
/// overline and underline toggles (%%o, %%u) are dropped.
pub fn decode_control_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("%%") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let mut chars = after.chars();
        match chars.next().map(|c| c.to_ascii_lowercase()) {
            Some('d') => out.push('°'),
            Some('p') => out.push('±'),
            Some('c') => out.push('Ø'),
            Some('%') => out.push('%'),
            Some('o' | 'u') => {}
            Some(c) if c.is_ascii_digit() => {
                let digits: String = after.chars().take(3).take_while(char::is_ascii_digit).collect();
                if let Some(ch) = digits.parse::<u32>().ok().and_then(char::from_u32) {
                    out.push(ch);
                }
                rest = &after[digits.len()..];
                continue;
            }
            _ => {
                out.push_str("%%");
                rest = after;
                continue;
            }
        }
        rest = chars.as_str();
    }
    out.push_str(rest);
    out
}

/// Line-to-line distance as a multiple of the text height (AutoCAD's MTEXT
/// line spacing factor 1.0).
pub const LINE_SPACING: f64 = 5.0 / 3.0;
/// Average character width as a multiple of the text height, for layout
/// estimates (selection boxes, extents) without font metrics.
const CHAR_WIDTH: f64 = 0.6;

/// The text's lines: MTEXT paragraph breaks (`\P`) and newlines.
pub fn text_lines(content: &str) -> Vec<String> {
    content.replace("\\P", "\n").lines().map(str::to_string).collect()
}

/// Estimated size (width, height) of a text box in drawing units.
pub fn text_extent(content: &str, height: f64, wrap_width: f64) -> (f64, f64) {
    let mut lines = 0usize;
    let mut widest: f64 = 0.0;
    for line in text_lines(content) {
        let w = line.chars().count().max(1) as f64 * CHAR_WIDTH * height;
        if wrap_width > EPSILON && w > wrap_width {
            lines += (w / wrap_width).ceil() as usize;
            widest = wrap_width;
        } else {
            lines += 1;
            widest = widest.max(w);
        }
    }
    let lines = lines.max(1);
    (widest, height + (lines - 1) as f64 * height * LINE_SPACING)
}

/// The text box's corners in world coordinates (counterclockwise from its
/// bottom-left before rotation).
pub fn text_box(
    position: Point,
    height: f64,
    rotation: f64,
    wrap_width: f64,
    attach: Attach,
    content: &str,
) -> [Point; 4] {
    let (w, h) = text_extent(content, height, wrap_width);
    let (fx, fy) = attach.fractions();
    let (x0, y0) = (-fx * w, -h * (1.0 - fy));
    [(x0, y0), (x0 + w, y0), (x0 + w, y0 + h), (x0, y0 + h)]
        .map(|(x, y)| (position + Point::new(x, y)).rotated_about(position, rotation))
}

/// Dimension style values for scale 1, like AutoCAD's ISO-25 style
/// (with a decimal point instead of a comma).
pub struct DimStyle {
    pub text_height: f64,
    pub arrow_size: f64,
    /// DIMEXO: gap between the measured point and the extension line.
    pub ext_offset: f64,
    /// DIMEXE: extension past the dimension line.
    pub ext_beyond: f64,
    /// DIMGAP: space between the dimension line and its text.
    pub gap: f64,
    /// DIMDEC.
    pub decimals: usize,
}

pub const DIM_STYLE: DimStyle =
    DimStyle { text_height: 2.5, arrow_size: 2.5, ext_offset: 0.625, ext_beyond: 1.25, gap: 0.625, decimals: 2 };

pub fn format_measurement(v: f64) -> String {
    format!("{v:.prec$}", prec = DIM_STYLE.decimals)
}

/// The measured value of a linear dimension along `rotation` (or along
/// `p1 -> p2` when aligned).
pub fn measurement(p1: Point, p2: Point, rotation: f64, aligned: bool) -> f64 {
    if aligned { p1.distance(p2) } else { (p2 - p1).dot(Point::ORIGIN.polar(1.0, rotation)).abs() }
}

/// The displayed text: an override with `<>` standing for the measurement,
/// as in AutoCAD, or the measurement itself.
pub fn dimension_text(text: Option<&str>, measured: f64) -> String {
    let value = format_measurement(measured);
    match text {
        Some(t) => t.replace("<>", &value),
        None => value,
    }
}

/// A closed filled arrowhead with its tip at `tip`, pointing against `into`
/// (the unit direction from the tip back along the line).
pub fn arrowhead(tip: Point, into: Point, size: f64) -> Geometry {
    let base = tip + into * size;
    let side = Point::new(-into.y, into.x) * (size / 6.0);
    let loop_ = vec![Vertex::at(tip), Vertex::at(base + side), Vertex::at(base - side)];
    Geometry::Hatch { loops: vec![loop_], pattern: HatchPattern::Solid, scale: 1.0, angle: 0.0 }
}

/// Readable text angle in (-90, 90]: text along a line never reads upside down.
fn readable(angle: f64) -> f64 {
    let a = angle.rem_euclid(360.0);
    if a > 90.0 && a <= 270.0 {
        a - 180.0
    } else if a > 270.0 {
        a - 360.0
    } else {
        a
    }
}

/// Extension lines, dimension line, arrowheads and text of a linear dimension.
pub fn dimension_primitives(
    p1: Point,
    p2: Point,
    line: Point,
    rotation: f64,
    aligned: bool,
    text: Option<&str>,
    scale: f64,
) -> Vec<Geometry> {
    let s = &DIM_STYLE;
    let r = if aligned { p1.angle_to(p2) } else { rotation };
    let u = Point::ORIGIN.polar(1.0, r);
    let d1 = line + u * (p1 - line).dot(u);
    let d2 = line + u * (p2 - line).dot(u);
    let mut out = Vec::new();
    for (p, d) in [(p1, d1), (p2, d2)] {
        let v = d - p;
        let len = v.length();
        if len > s.ext_offset * scale {
            let v = v * (1.0 / len);
            out.push(Geometry::Line { start: p + v * (s.ext_offset * scale), end: d + v * (s.ext_beyond * scale) });
        }
    }
    out.push(Geometry::Line { start: d1, end: d2 });
    let len = d1.distance(d2);
    if len > EPSILON {
        let w = (d2 - d1) * (1.0 / len);
        out.push(arrowhead(d1, w, s.arrow_size * scale));
        out.push(arrowhead(d2, -w, s.arrow_size * scale));
    }
    let angle = readable(r);
    let up = Point::ORIGIN.polar(1.0, angle + 90.0);
    out.push(Geometry::Text {
        position: d1.midpoint(d2) + up * (s.gap * scale),
        height: s.text_height * scale,
        rotation: angle,
        width: 0.0,
        attach: Attach::BottomCenter,
        content: dimension_text(text, measurement(p1, p2, rotation, aligned)),
    });
    out
}

/// Leader line, arrowhead, landing and text of a multileader.
pub fn leader_primitives(points: &[Point], content: &str, scale: f64) -> Vec<Geometry> {
    let s = &DIM_STYLE;
    let [first, second, ..] = points else { return Vec::new() };
    let (last, prev) = (points[points.len() - 1], points[points.len() - 2]);
    let side = if last.x >= prev.x { 1.0 } else { -1.0 };
    let has_text = !content.trim().is_empty();
    let mut path: Vec<Vertex> = points.iter().copied().map(Vertex::at).collect();
    let land_end = last + Point::new(side * s.arrow_size * scale, 0.0);
    if has_text {
        path.push(Vertex::at(land_end));
    }
    let mut out = vec![Geometry::Polyline { vertices: path, closed: false }];
    let d = *second - *first;
    if d.length() > EPSILON {
        out.push(arrowhead(*first, d * (1.0 / d.length()), s.arrow_size * scale));
    }
    if has_text {
        out.push(Geometry::Text {
            position: land_end + Point::new(side * s.gap * scale, 0.0),
            height: s.text_height * scale,
            rotation: 0.0,
            width: 0.0,
            attach: if side > 0.0 { Attach::MiddleLeft } else { Attach::MiddleRight },
            content: content.to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_extent_counts_lines_and_wraps() {
        let (w, h) = text_extent("abcd", 2.0, 0.0);
        assert!((w - 4.8).abs() < 1e-9 && (h - 2.0).abs() < 1e-9);
        let (_, h2) = text_extent("ab\\Pcd", 2.0, 0.0);
        assert!((h2 - (2.0 + 2.0 * LINE_SPACING)).abs() < 1e-9);
        let (w3, h3) = text_extent("abcdefghij", 1.0, 3.0);
        assert!((w3 - 3.0).abs() < 1e-9 && h3 > 1.0, "long lines wrap at the width");
    }

    #[test]
    fn dimension_measures_and_places_text() {
        let prims = dimension_primitives(
            Point::new(0.0, 0.0),
            Point::new(10.0, 3.0),
            Point::new(5.0, 8.0),
            0.0,
            false,
            None,
            1.0,
        );
        let text = prims.iter().find_map(|g| match g {
            Geometry::Text { content, position, .. } => Some((content.clone(), *position)),
            _ => None,
        });
        let (content, position) = text.unwrap();
        assert_eq!(content, "10.00");
        assert!((position.y - (8.0 + DIM_STYLE.gap)).abs() < 1e-9, "text sits above the dimension line");
        // Two extension lines, the dimension line and two arrowheads.
        assert_eq!(prims.iter().filter(|g| matches!(g, Geometry::Line { .. })).count(), 3);
        assert_eq!(prims.iter().filter(|g| matches!(g, Geometry::Hatch { .. })).count(), 2);

        assert!((measurement(Point::ORIGIN, Point::new(3.0, 4.0), 0.0, true) - 5.0).abs() < 1e-12);
        assert_eq!(dimension_text(Some("<> TYP"), 5.0), "5.00 TYP");
    }

    #[test]
    fn control_codes() {
        assert_eq!(decode_control_codes("45%%d"), "45°");
        assert_eq!(decode_control_codes("%%c20 %%p0.5"), "Ø20 ±0.5");
        assert_eq!(decode_control_codes("%%OTOP%%o 100%%%"), "TOP 100%");
        assert_eq!(decode_control_codes("%%065B"), "AB");
        assert_eq!(decode_control_codes("50%"), "50%");
    }

    #[test]
    fn readable_angles() {
        assert_eq!(readable(180.0), 0.0);
        assert_eq!(readable(270.0), 90.0);
        assert_eq!(readable(300.0), -60.0);
    }

    #[test]
    fn leader_lands_toward_its_text() {
        let prims = leader_primitives(&[Point::ORIGIN, Point::new(10.0, 10.0)], "NOTE", 1.0);
        let Some(Geometry::Text { attach, position, .. }) = prims.last() else { panic!() };
        assert_eq!(*attach, Attach::MiddleLeft);
        assert!(position.x > 10.0);
        let prims = leader_primitives(&[Point::ORIGIN, Point::new(-10.0, 10.0)], "NOTE", 1.0);
        assert!(matches!(prims.last(), Some(Geometry::Text { attach: Attach::MiddleRight, .. })));
    }
}

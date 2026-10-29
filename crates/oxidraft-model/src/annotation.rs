//! Text, dimensions and leaders: their layout in drawing units, and how
//! dimensions and leaders break down into lines, arrowheads and text.

use oxidraft_geom::{
    EPSILON, Point, Segment,
    curve::{ArcSeg, Seg, carrier_points},
};

use crate::entity::{Geometry, Vertex};
use crate::hatch::HatchPattern;

/// The text style every drawing has.
pub const STANDARD: &str = "Standard";

/// A text style (STYLE): the font named, a fixed height (0: asked for each
/// text), how wide the characters are and how slanted.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub name: String,
    /// The font file or family AutoCAD would use (Oxidraft draws its own
    /// typeface; the name is kept for AutoCAD).
    pub font: String,
    pub height: f64,
    pub width_factor: f64,
    /// Degrees from upright, positive leaning right.
    pub oblique: f64,
}

impl TextStyle {
    pub fn standard() -> Self {
        Self::new(STANDARD)
    }

    pub fn new(name: &str) -> Self {
        Self { name: name.into(), font: "arial.ttf".into(), height: 0.0, width_factor: 1.0, oblique: 0.0 }
    }
}

/// A text's style and how its characters are drawn: as wide and slanted as
/// the style had them when the text was made (or as set since).
#[derive(Debug, Clone, PartialEq)]
pub struct TextLook {
    pub style: String,
    pub width_factor: f64,
    pub oblique: f64,
}

impl Default for TextLook {
    fn default() -> Self {
        Self { style: STANDARD.into(), width_factor: 1.0, oblique: 0.0 }
    }
}

impl TextLook {
    pub fn of(style: &TextStyle) -> Self {
        Self { style: style.name.clone(), width_factor: style.width_factor, oblique: style.oblique }
    }

    /// Drawn as plain upright characters of their usual width.
    pub fn is_plain(&self) -> bool {
        (self.width_factor - 1.0).abs() < 1e-9 && self.oblique.abs() < 1e-9
    }
}

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

/// What text shows: MTEXT's inline formatting taken out (fonts, heights,
/// colors and the like: `\f…;`, `\H…;`, `\C…;`, braces, underline and
/// overline toggles), `\U+XXXX` turned into its character, stacked
/// fractions (`\S1/2;`) into their text, and paragraph breaks (`\P`) into
/// newlines. The drawing keeps the codes; this is only how it's shown.
pub fn plain_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let until_semicolon = |chars: &mut std::iter::Peekable<std::str::Chars>| {
        let mut taken = String::new();
        for c in chars.by_ref() {
            if c == ';' {
                break;
            }
            taken.push(c);
        }
        taken
    };
    while let Some(c) = chars.next() {
        if c == '{' || c == '}' {
            continue;
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('P') => out.push('\n'),
            Some('~') => out.push('\u{a0}'),
            Some(c @ ('\\' | '{' | '}')) => out.push(c),
            Some('U' | 'u') if chars.peek() == Some(&'+') => {
                chars.next();
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            // A character in another code page: left out.
            Some('M' | 'm') if chars.peek() == Some(&'+') => {
                chars.by_ref().take(6).for_each(drop);
            }
            Some('S') => out.push_str(&until_semicolon(&mut chars).replace(['^', '#'], "/")),
            Some('A' | 'C' | 'c' | 'F' | 'f' | 'H' | 'Q' | 'T' | 'W' | 'p') => {
                until_semicolon(&mut chars);
            }
            Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') => {}
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The text's lines, as shown (see [`plain_text`]).
pub fn text_lines(content: &str) -> Vec<String> {
    plain_text(content).lines().map(str::to_string).collect()
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
    width_factor: f64,
) -> [Point; 4] {
    // Wider characters: fewer to a wrapped line, each line wider.
    let wf = if width_factor > 0.0 { width_factor } else { 1.0 };
    let (w, h) = text_extent(content, height, wrap_width / wf);
    let w = w * wf;
    let (fx, fy) = attach.fractions();
    let (x0, y0) = (-fx * w, -h * (1.0 - fy));
    [(x0, y0), (x0 + w, y0), (x0 + w, y0 + h), (x0, y0 + h)]
        .map(|(x, y)| (position + Point::new(x, y)).rotated_about(position, rotation))
}

/// Dimension style values for scale 1 (DIMSTYLE); the default is like
/// AutoCAD's ISO-25 style (with a decimal point instead of a comma).
#[derive(Debug, Clone, Copy, PartialEq)]
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

pub const DEFAULT_DIM_STYLE: DimStyle =
    DimStyle { text_height: 2.5, arrow_size: 2.5, ext_offset: 0.625, ext_beyond: 1.25, gap: 0.625, decimals: 2 };

impl Default for DimStyle {
    fn default() -> Self {
        DEFAULT_DIM_STYLE
    }
}

thread_local! {
    /// The dimension style dimensions are drawn in: the drawing's, set
    /// while it's drawn (see [`with_dim_style`]).
    static CURRENT: std::cell::Cell<DimStyle> = const { std::cell::Cell::new(DEFAULT_DIM_STYLE) };
}

/// The dimension style in effect.
pub fn dim_style() -> DimStyle {
    CURRENT.get()
}

/// Makes `style` the one dimensions are drawn in, on this thread.
pub fn set_dim_style(style: DimStyle) {
    CURRENT.set(style);
}

/// Runs `f` with `style` in effect, then puts the one before back.
pub fn with_dim_style<T>(style: DimStyle, f: impl FnOnce() -> T) -> T {
    let before = CURRENT.replace(style);
    let out = f();
    CURRENT.set(before);
    out
}

pub fn format_measurement(v: f64) -> String {
    format!("{v:.prec$}", prec = dim_style().decimals)
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
pub fn readable(angle: f64) -> f64 {
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
#[allow(clippy::too_many_arguments)]
pub fn dimension_primitives(
    p1: Point,
    p2: Point,
    line: Point,
    rotation: f64,
    aligned: bool,
    text: Option<&str>,
    scale: f64,
    extra: &DimExtra,
) -> Vec<Geometry> {
    let s = &dim_style();
    let r = if aligned { p1.angle_to(p2) } else { rotation };
    let u = Point::ORIGIN.polar(1.0, r);
    // Where each extension line meets the dimension line: straight across,
    // or slanted (Oblique) along its own angle.
    let meet = |p: Point| {
        if let Some(o) = extra.oblique {
            let v = Point::ORIGIN.polar(1.0, o);
            let n = Point::ORIGIN.polar(1.0, r + 90.0);
            let across = v.dot(n);
            if across.abs() > 1e-9 {
                return p + v * ((line - p).dot(n) / across);
            }
        }
        line + u * (p - line).dot(u)
    };
    let (d1, d2) = (meet(p1), meet(p2));
    let mut out = Vec::new();
    for (p, d) in [(p1, d1), (p2, d2)] {
        let v = d - p;
        let len = v.length();
        if len > s.ext_offset * scale {
            let v = v * (1.0 / len);
            out.push(Geometry::Line { start: p + v * (s.ext_offset * scale), end: d + v * (s.ext_beyond * scale) });
        }
    }
    // Text moved along the line (DIMTEDIT) takes the line with it past an end.
    let text_mid = extra.text_at.map_or(d1.midpoint(d2), |t| d1 + u * (t - d1).dot(u));
    let along = |p: Point| (p - d1).dot(u);
    let (lo, hi) = (along(d1).min(along(d2)).min(along(text_mid)), along(d1).max(along(d2)).max(along(text_mid)));
    out.push(Geometry::Line { start: d1 + u * lo, end: d1 + u * hi });
    let mut out = cut_breaks(out, &extra.breaks);
    let len = d1.distance(d2);
    if len > EPSILON {
        let w = (d2 - d1) * (1.0 / len);
        out.push(arrowhead(d1, w, s.arrow_size * scale));
        out.push(arrowhead(d2, -w, s.arrow_size * scale));
    }
    let angle = extra.text_angle.unwrap_or_else(|| readable(r));
    let up = Point::ORIGIN.polar(1.0, readable(r) + 90.0);
    out.push(Geometry::Text {
        look: Default::default(),
        position: text_mid + up * (s.gap * scale),
        height: s.text_height * scale,
        rotation: angle,
        width: 0.0,
        attach: Attach::BottomCenter,
        content: dimension_text(text, measurement(p1, p2, rotation, aligned)),
    });
    out
}

/// What DIMEDIT, DIMTEDIT and DIMBREAK change on a linear dimension.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DimExtra {
    /// Where its text sits when moved along its line (DIMTEDIT).
    pub text_at: Option<Point>,
    /// The text's angle when set (DIMTEDIT Angle, DIMEDIT Rotate).
    pub text_angle: Option<f64>,
    /// The extension lines' angle when slanted (DIMEDIT Oblique).
    pub oblique: Option<f64>,
    /// Gaps cut in its lines (DIMBREAK): each from one point to another
    /// on the same line.
    pub breaks: Vec<(Point, Point)>,
}

impl DimExtra {
    pub fn is_plain(&self) -> bool {
        *self == DimExtra::default()
    }

    /// The same, with its points moved by `f` and angles turned by `turn`.
    pub fn mapped(&self, f: &dyn Fn(Point) -> Point, turn: &dyn Fn(f64) -> f64) -> DimExtra {
        DimExtra {
            text_at: self.text_at.map(f),
            text_angle: self.text_angle.map(turn),
            oblique: self.oblique.map(turn),
            breaks: self.breaks.iter().map(|(a, b)| (f(*a), f(*b))).collect(),
        }
    }
}

/// `lines` with the gaps cut out that lie along them.
pub fn cut_breaks(lines: Vec<Geometry>, breaks: &[(Point, Point)]) -> Vec<Geometry> {
    if breaks.is_empty() {
        return lines;
    }
    let mut out = Vec::new();
    for g in lines {
        let Geometry::Line { start, end } = g else {
            out.push(g);
            continue;
        };
        let d = end - start;
        let len2 = d.dot(d);
        if len2 <= EPSILON * EPSILON {
            out.push(g);
            continue;
        }
        let on = |p: Point| (p - start).cross(d).abs() / len2.sqrt() < 1e-6 * (1.0 + len2.sqrt());
        let mut gaps: Vec<(f64, f64)> = breaks
            .iter()
            .filter(|(a, b)| on(*a) && on(*b))
            .map(|(a, b)| {
                let (ta, tb) = ((*a - start).dot(d) / len2, (*b - start).dot(d) / len2);
                (ta.min(tb).max(0.0), ta.max(tb).min(1.0))
            })
            .filter(|(a, b)| b > a)
            .collect();
        gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut t = 0.0;
        for (a, b) in gaps {
            if a > t {
                out.push(Geometry::Line { start: start + d * t, end: start + d * a });
            }
            t = t.max(b);
        }
        if t < 1.0 {
            out.push(Geometry::Line { start: start + d * t, end });
        }
    }
    out
}

/// DIMDLI: spacing between stacked baseline dimensions, at scale 1.
pub const BASELINE_SPACING: f64 = 3.75;

/// The value a radius or diameter dimension shows, with its R or Ø prefix.
pub fn radial_value(center: Point, point: Point, diameter: bool) -> String {
    let r = center.distance(point);
    if diameter { format!("Ø{}", format_measurement(2.0 * r)) } else { format!("R{}", format_measurement(r)) }
}

/// The angle an angular dimension measures, counterclockwise from the ray
/// through `p1` to the ray through `p2`, in degrees.
pub fn angular_measurement(vertex: Point, p1: Point, p2: Point) -> f64 {
    (vertex.angle_to(p2) - vertex.angle_to(p1)).rem_euclid(360.0)
}

/// The two ray points of an angle at `vertex`, ordered so the
/// counterclockwise sweep from the first to the second contains `at`.
pub fn angle_around(vertex: Point, p1: Point, p2: Point, at: Point) -> (Point, Point) {
    let sweep = ArcSeg::ccw(vertex, 1.0, vertex.angle_to(p1), vertex.angle_to(p2));
    if sweep.param_of_angle(vertex.angle_to(at)) <= 1.0 { (p1, p2) } else { (p2, p1) }
}

/// The vertex and ray points of the angle between two lines (extended to
/// meet) whose sector contains `at`: one of the four angles they make.
/// `None` for parallel lines.
pub fn two_line_angle(a: (Point, Point), b: (Point, Point), at: Point) -> Option<(Point, Point, Point)> {
    let (sa, sb) = (Seg::Line(Segment::new(a.0, a.1)), Seg::Line(Segment::new(b.0, b.1)));
    let x = *carrier_points(&sa, &sb).first()?;
    let unit = |p: Point| p * (1.0 / p.length().max(EPSILON));
    let (d1, d2) = (unit(a.1 - a.0), unit(b.1 - b.0));
    // Write the location as α·d1 + β·d2: their signs pick the rays.
    let (v, det) = (at - x, d1.cross(d2));
    let (alpha, beta) = (v.cross(d2) / det, d1.cross(v) / det);
    let (r1, r2) = (d1 * alpha.signum(), d2 * beta.signum());
    // A point on each ray: the line's far end that way, if it reaches out.
    let on_ray = |line: (Point, Point), r: Point| {
        let reach = (line.0 - x).dot(r).max((line.1 - x).dot(r));
        x + r * if reach > EPSILON { reach } else { 1.0 }
    };
    let (p1, p2) = (on_ray(a, r1), on_ray(b, r2));
    Some(if r1.cross(r2) >= 0.0 { (x, p1, p2) } else { (x, p2, p1) })
}

/// An angle as dimensions show it (DIMADEC 0).
pub fn format_angle(degrees: f64) -> String {
    format!("{degrees:.0}°")
}

/// Text override with `<>` standing for an already formatted value.
fn override_text(text: Option<&str>, value: String) -> String {
    match text {
        Some(t) => t.replace("<>", &value),
        None => value,
    }
}

/// Dimension line, arrowheads and text of a radius or diameter dimension.
/// Text inside the circle sits on the dimension line; text outside gets a
/// line out to it from the arrowhead on the circle.
pub fn radial_primitives(
    center: Point,
    point: Point,
    text_at: Point,
    diameter: bool,
    text: Option<&str>,
    scale: f64,
) -> Vec<Geometry> {
    let s = &dim_style();
    let r = center.distance(point);
    if r <= EPSILON {
        return Vec::new();
    }
    let u = (point - center) * (1.0 / r);
    let content = override_text(text, radial_value(center, point, diameter));
    let mut out = Vec::new();
    if center.distance(text_at) > r && point.distance(text_at) > EPSILON {
        let w = (text_at - point) * (1.0 / point.distance(text_at));
        out.push(Geometry::Line { start: point, end: text_at });
        out.push(arrowhead(point, w, s.arrow_size * scale));
        let right = text_at.x >= point.x;
        out.push(Geometry::Text {
            look: Default::default(),
            position: text_at + Point::new(if right { s.gap } else { -s.gap } * scale, 0.0),
            height: s.text_height * scale,
            rotation: 0.0,
            width: 0.0,
            attach: if right { Attach::MiddleLeft } else { Attach::MiddleRight },
            content,
        });
        return out;
    }
    let start = if diameter { center - u * r } else { center };
    out.push(Geometry::Line { start, end: point });
    out.push(arrowhead(point, -u, s.arrow_size * scale));
    if diameter {
        out.push(arrowhead(start, u, s.arrow_size * scale));
    }
    let angle = readable(Point::ORIGIN.angle_to(u));
    let up = Point::ORIGIN.polar(1.0, angle + 90.0);
    let along = text_at - center;
    out.push(Geometry::Text {
        look: Default::default(),
        position: center + u * along.dot(u) + up * (s.gap * scale),
        height: s.text_height * scale,
        rotation: angle,
        width: 0.0,
        attach: Attach::BottomCenter,
        content,
    });
    out
}

/// Extension lines, dimension arc, arrowheads and text of an angular
/// dimension at `vertex`, counterclockwise from the ray through `p1` to the
/// ray through `p2`, with its arc through `arc`.
pub fn angular_primitives(
    vertex: Point,
    p1: Point,
    p2: Point,
    arc: Point,
    text: Option<&str>,
    scale: f64,
) -> Vec<Geometry> {
    let s = &dim_style();
    let r = vertex.distance(arc);
    let (a1, a2) = (vertex.angle_to(p1), vertex.angle_to(p2));
    let sweep = angular_measurement(vertex, p1, p2);
    if r <= EPSILON || sweep <= 1e-9 {
        return Vec::new();
    }
    let mut out = Vec::new();
    // Extension lines run out from the measured points when the arc is beyond them.
    for (p, a) in [(p1, a1), (p2, a2)] {
        let (l, u) = (vertex.distance(p), Point::ORIGIN.polar(1.0, a));
        if r > l + s.ext_offset * scale {
            out.push(Geometry::Line {
                start: vertex + u * (l + s.ext_offset * scale),
                end: vertex + u * (r + s.ext_beyond * scale),
            });
        }
    }
    out.push(Geometry::Arc { center: vertex, radius: r, start_angle: a1, end_angle: a2 });
    out.push(arrowhead(vertex.polar(r, a1), Point::ORIGIN.polar(1.0, a1 + 90.0), s.arrow_size * scale));
    out.push(arrowhead(vertex.polar(r, a2), Point::ORIGIN.polar(1.0, a2 - 90.0), s.arrow_size * scale));
    // Text along the arc's middle, outside it, never upside down.
    let mid = a1 + sweep / 2.0;
    let angle = readable(mid - 90.0);
    let outward = Point::ORIGIN.polar(1.0, angle + 90.0).dot(Point::ORIGIN.polar(1.0, mid)) > 0.0;
    out.push(Geometry::Text {
        look: Default::default(),
        position: vertex.polar(r + s.gap * scale, mid),
        height: s.text_height * scale,
        rotation: angle,
        width: 0.0,
        attach: if outward { Attach::BottomCenter } else { Attach::TopCenter },
        content: override_text(text, format_angle(sweep)),
    });
    out
}

/// Leader line, arrowhead, landing and text of a multileader.
pub fn leader_primitives(points: &[Point], content: &str, scale: f64) -> Vec<Geometry> {
    let s = &dim_style();
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
            look: Default::default(),
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

/// An ordinate dimension (DIMORDINATE): the feature's X (with `x_datum`)
/// or Y, by a leader from near the feature to `end` (jogging to reach it),
/// with the value beyond the leader's end.
pub fn ordinate_primitives(feature: Point, end: Point, x_datum: bool, text: Option<&str>, scale: f64) -> Vec<Geometry> {
    let s = &dim_style();
    let value = override_text(text, format_measurement(if x_datum { feature.x } else { feature.y }));
    // Along the leader, away from the feature; across it, where the jog goes.
    let (along, across) =
        if x_datum { (end.y - feature.y, end.x - feature.x) } else { (end.x - feature.x, end.y - feature.y) };
    let dir = if along >= 0.0 { 1.0 } else { -1.0 };
    let at = |a: f64, c: f64| {
        if x_datum { Point::new(feature.x + c, feature.y + a) } else { Point::new(feature.x + a, feature.y + c) }
    };
    let start = at(dir * s.ext_offset * scale, 0.0);
    let mut points = vec![start];
    if across.abs() > EPSILON {
        // A jog in the middle third, as AutoCAD draws one.
        let mid = (along - dir * s.ext_offset * scale) / 2.0 + dir * s.ext_offset * scale;
        let half = (s.arrow_size * scale).min((along.abs() - s.ext_offset * scale).abs() / 3.0);
        points.push(at(mid - dir * half, 0.0));
        points.push(at(mid + dir * half, across));
    }
    points.push(end);
    let mut out: Vec<Geometry> = points.windows(2).map(|w| Geometry::Line { start: w[0], end: w[1] }).collect();
    let rotation = if x_datum { 90.0 } else { 0.0 };
    let past = at(along + dir * s.gap * scale, across);
    let attach = if dir > 0.0 { Attach::MiddleLeft } else { Attach::MiddleRight };
    out.push(Geometry::Text {
        look: Default::default(),
        position: past,
        height: s.text_height * scale,
        rotation,
        width: 0.0,
        attach,
        content: value,
    });
    out
}

/// An arc length dimension (DIMARC): extension lines out from the arc's
/// ends to a dimension arc at `dim_radius`, arrows at its ends, and the
/// length with the arc symbol over it.
pub fn arc_length_primitives(
    center: Point,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    dim_radius: f64,
    text: Option<&str>,
    scale: f64,
) -> Vec<Geometry> {
    let s = &dim_style();
    let sweep = (end_angle - start_angle).rem_euclid(360.0);
    let length = radius * sweep.to_radians();
    let content = override_text(text, format_measurement(length));
    let out_dir = if dim_radius >= radius { 1.0 } else { -1.0 };
    let mut out = Vec::new();
    for a in [start_angle, end_angle] {
        let from = center.polar(radius + out_dir * s.ext_offset * scale, a);
        let to = center.polar(dim_radius + out_dir * s.ext_beyond * scale, a);
        out.push(Geometry::Line { start: from, end: to });
    }
    out.push(Geometry::Arc { center, radius: dim_radius, start_angle, end_angle });
    let size = s.arrow_size * scale;
    // Arrowheads point along the arc, into each end.
    let tangent = |a: f64| Point::ORIGIN.polar(1.0, a + 90.0);
    out.push(arrowhead(center.polar(dim_radius, start_angle), tangent(start_angle), size));
    out.push(arrowhead(center.polar(dim_radius, end_angle), tangent(end_angle) * -1.0, size));
    let mid = start_angle + sweep / 2.0;
    let rotation = readable(mid - 90.0);
    let up = Point::ORIGIN.polar(1.0, rotation + 90.0);
    let at = center.polar(dim_radius, mid) + up * (s.gap * scale);
    let height = s.text_height * scale;
    out.push(Geometry::Text {
        look: Default::default(),
        position: at,
        height,
        rotation,
        width: 0.0,
        attach: Attach::BottomCenter,
        content: content.clone(),
    });
    // The arc symbol over the text.
    let over = at + up * (height * 1.6);
    out.push(Geometry::Arc {
        center: over,
        radius: height * 0.45,
        start_angle: rotation + 20.0,
        end_angle: rotation + 160.0,
    });
    out
}

/// A jogged radius dimension (DIMJOGGED): from a stand-in center, a line
/// with a zigzag at `jog` out to the point on the arc, an arrow there, and
/// the radius by it.
pub fn jogged_primitives(
    center: Point,
    point: Point,
    center_override: Point,
    jog: Point,
    text: Option<&str>,
    scale: f64,
) -> Vec<Geometry> {
    let s = &dim_style();
    let r = center.distance(point);
    let content = override_text(text, format!("R{}", format_measurement(r)));
    let d = point - center_override;
    let len = d.length();
    if len <= EPSILON {
        return Vec::new();
    }
    let u = d * (1.0 / len);
    let n = Point::new(-u.y, u.x);
    // The zigzag: across the line where `jog` is, at the jog angle (45°).
    let t = (jog - center_override).dot(u).clamp(0.0, len);
    let h = s.arrow_size * scale;
    let a = center_override + u * t;
    let (b, c) = (a + n * h + u * (h * 0.5), a + n * h - u * (h * 0.5) + u * h);
    let mut out = vec![
        Geometry::Line { start: center_override, end: a },
        Geometry::Line { start: a, end: b },
        Geometry::Line { start: b, end: c },
        Geometry::Line { start: c, end: point + n * h },
        Geometry::Line { start: point + n * h, end: point },
    ];
    out.push(arrowhead(point, u * -1.0, h));
    let rotation = readable(Point::ORIGIN.angle_to(u));
    out.push(Geometry::Text {
        look: Default::default(),
        position: center_override.lerp(point, 0.35) + Point::ORIGIN.polar(s.gap * scale, rotation + 90.0),
        height: s.text_height * scale,
        rotation,
        width: 0.0,
        attach: Attach::BottomCenter,
        content,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::plain_text;

    #[test]
    fn mtext_formatting_is_shown_as_its_text() {
        let raw = "{\\fArial|b0|i1|c204|p34;\\U+041F\\U+0423\\U+041F} \\H2.5x;\\C1;\\U+043A\\U+0432.17\\P\\S1/2; \\Lline\\l a\\\\b";
        assert_eq!(plain_text(raw), "ПУП кв.17\n1/2 line a\\b");
    }

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
            &DimExtra::default(),
        );
        let text = prims.iter().find_map(|g| match g {
            Geometry::Text { content, position, .. } => Some((content.clone(), *position)),
            _ => None,
        });
        let (content, position) = text.unwrap();
        assert_eq!(content, "10.00");
        assert!((position.y - (8.0 + dim_style().gap)).abs() < 1e-9, "text sits above the dimension line");
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

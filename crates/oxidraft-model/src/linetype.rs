//! Linetypes (dashed, center, hidden, …) and lineweights.

use oxidraft_geom::{EPSILON, Point};

/// An object's linetype is its layer's.
pub const BYLAYER: &str = "ByLayer";
/// Inside a block: the block reference's linetype.
pub const BYBLOCK: &str = "ByBlock";
pub const CONTINUOUS: &str = "Continuous";

pub fn is_by_layer(name: &str) -> bool {
    name.eq_ignore_ascii_case(BYLAYER)
}

pub fn is_by_block(name: &str) -> bool {
    name.eq_ignore_ascii_case(BYBLOCK)
}

/// A named dash pattern.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Linetype {
    pub name: String,
    pub description: String,
    /// Dash (positive), gap (negative) and dot (zero) lengths at LTSCALE 1,
    /// repeated along the curve. Empty for Continuous.
    pub pattern: Vec<f64>,
    /// The shapes and texts a complex linetype draws besides its dashes.
    pub embedded: Vec<Embedded>,
}

/// A shape or a text a complex linetype draws where one of its pattern's
/// elements ends (a `[...]` after that element in a .lin file).
#[derive(Debug, Clone, PartialEq)]
pub struct Embedded {
    /// The element of the pattern it follows.
    pub element: usize,
    pub content: EmbeddedContent,
    /// S=: a shape's size; a text's height (times its style's, if that has
    /// one).
    pub scale: f64,
    /// R=, A= or U=, in degrees.
    pub rotation: f64,
    pub turn: EmbeddedTurn,
    /// X= along the line and Y= across it, at LTSCALE 1.
    pub offset: Point,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EmbeddedContent {
    /// Shape `number` of shape file `file` (ltypeshp.shx, as AutoCAD keeps
    /// the file's name).
    Shape { number: u16, file: String },
    /// `text` in text style `style`.
    Text { text: String, style: String },
}

/// What an embedded shape's or text's rotation is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddedTurn {
    /// R=: the line's direction.
    #[default]
    Relative,
    /// A=: the X axis.
    Absolute,
    /// U=: the line's direction, turned around where it would read upside
    /// down.
    Upright,
}

impl Embedded {
    /// The direction (degrees) it's drawn in where the line runs at `along`,
    /// and whether it's turned around to stay upright: then it turns about
    /// its middle, keeping its place.
    pub fn direction(&self, along: f64) -> (f64, bool) {
        match self.turn {
            EmbeddedTurn::Absolute => (self.rotation, false),
            EmbeddedTurn::Relative => (along + self.rotation, false),
            EmbeddedTurn::Upright => {
                let a = (along + self.rotation).rem_euclid(360.0);
                let down = a > 90.0 + EPSILON && a <= 270.0 + EPSILON;
                if down { (a - 180.0, true) } else { (a, false) }
            }
        }
    }
}

impl Linetype {
    pub fn new(name: &str, description: &str, pattern: &[f64]) -> Self {
        Self { name: name.into(), description: description.into(), pattern: pattern.to_vec(), embedded: Vec::new() }
    }

    pub fn continuous() -> Self {
        Self::new(CONTINUOUS, "Solid line", &[])
    }

    /// Length of one repeat of the pattern.
    pub fn length(&self) -> f64 {
        self.pattern.iter().map(|d| d.abs()).sum()
    }

    pub fn is_continuous(&self) -> bool {
        (self.pattern.iter().all(|&d| d > 0.0) && self.embedded.is_empty()) || self.length() <= EPSILON
    }

    /// The text styles its texts are in.
    pub fn text_styles(&self) -> impl Iterator<Item = &str> {
        self.embedded.iter().filter_map(|e| match &e.content {
            EmbeddedContent::Text { style, .. } => Some(style.as_str()),
            EmbeddedContent::Shape { .. } => None,
        })
    }
}

/// Where along `path` the shapes and texts of `lt` (scaled by `scale`)
/// fall: each one's base point, after its offset, and the direction
/// (degrees) the path runs there.
pub fn embedded_along<'a>(path: &[Point], lt: &'a Linetype, scale: f64) -> Vec<(&'a Embedded, Point, f64)> {
    let total = lt.length() * scale;
    if path.len() < 2 || total <= EPSILON || lt.embedded.is_empty() {
        return Vec::new();
    }
    // Where each element ends in one repeat of the pattern.
    let ends: Vec<f64> = lt
        .pattern
        .iter()
        .scan(0.0, |at, d| {
            *at += d.abs() * scale;
            Some(*at)
        })
        .collect();
    let length: f64 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    // Too many to draw: none, as AutoCAD draws a line too dense to see.
    if length / total > 10_000.0 {
        return Vec::new();
    }
    let mut stops: Vec<(f64, &Embedded)> = Vec::new();
    let mut start = 0.0;
    while start < length + EPSILON {
        for e in &lt.embedded {
            if let Some(end) = ends.get(e.element) {
                let at = start + end;
                if at <= length + EPSILON {
                    stops.push((at, e));
                }
            }
        }
        start += total;
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::with_capacity(stops.len());
    let (mut seg, mut walked) = (0, 0.0);
    for (at, e) in stops {
        // The piece of the path `at` is on (the last one, past its end).
        while seg + 2 < path.len() && walked + path[seg].distance(path[seg + 1]) < at {
            walked += path[seg].distance(path[seg + 1]);
            seg += 1;
        }
        let (a, b) = (path[seg], path[seg + 1]);
        let piece = a.distance(b);
        let p = if piece > EPSILON { a.lerp(b, ((at - walked) / piece).min(1.0)) } else { a };
        let along = (b.y - a.y).atan2(b.x - a.x).to_degrees();
        let shift = (e.offset * scale).rotated_about(Point::ORIGIN, along);
        out.push((e, p + shift, along));
    }
    out
}

/// The common linetypes of AutoCAD's acadiso.lin, in millimeters. Drawings
/// can define their own (or other scales of these) in their linetype table.
pub fn standard() -> Vec<Linetype> {
    vec![
        Linetype::new(
            "BORDER",
            "Border __ __ . __ __ . __ __ . __ __ . __ __ .",
            &[12.7, -6.35, 12.7, -6.35, 0.0, -6.35],
        ),
        Linetype::new("CENTER", "Center ____ _ ____ _ ____ _ ____ _ ____ _ ____", &[31.75, -6.35, 6.35, -6.35]),
        Linetype::new("DASHDOT", "Dash dot __ . __ . __ . __ . __ . __ . __ . __", &[12.7, -6.35, 0.0, -6.35]),
        Linetype::new("DASHED", "Dashed __ __ __ __ __ __ __ __ __ __ __ __ __ _", &[12.7, -6.35]),
        Linetype::new(
            "DIVIDE",
            "Divide ____ . . ____ . . ____ . . ____ . . ____",
            &[12.7, -6.35, 0.0, -6.35, 0.0, -6.35],
        ),
        Linetype::new("DOT", "Dot . . . . . . . . . . . . . . . . . . . . . . . .", &[0.0, -6.35]),
        Linetype::new("HIDDEN", "Hidden __ __ __ __ __ __ __ __ __ __ __ __ __ __", &[6.35, -3.175]),
        Linetype::new(
            "PHANTOM",
            "Phantom ______  __  __  ______  __  __  ______",
            &[31.75, -6.35, 6.35, -6.35, 6.35, -6.35],
        ),
    ]
}

/// Splits a path into the dashes of `pattern` scaled by `scale`, starting
/// at the path's start. A dot comes back as a run of one point.
pub fn dashes(path: &[Point], pattern: &[f64], scale: f64) -> Vec<Vec<Point>> {
    let total: f64 = pattern.iter().map(|d| d.abs()).sum::<f64>() * scale;
    if path.len() < 2 || total <= EPSILON {
        return vec![path.to_vec()];
    }
    let mut out: Vec<Vec<Point>> = Vec::new();
    let (mut element, mut left) = (0, pattern[0].abs() * scale);
    let mut current: Option<Vec<Point>> = (pattern[0] > 0.0).then(|| vec![path[0]]);
    if pattern[0] == 0.0 {
        out.push(vec![path[0]]);
    }
    for w in path.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut seg = a.distance(b);
        while seg > EPSILON {
            if left > seg {
                left -= seg;
                if let Some(run) = &mut current {
                    run.push(b);
                }
                break;
            }
            // The current element ends inside this piece of the path.
            let p = a.lerp(b, left / seg);
            seg -= left;
            a = p;
            if let Some(mut run) = current.take() {
                run.push(p);
                out.push(run);
            }
            element = (element + 1) % pattern.len();
            let d = pattern[element];
            left = d.abs() * scale;
            if d > 0.0 {
                current = Some(vec![p]);
            } else if d == 0.0 {
                out.push(vec![p]);
            }
        }
    }
    out.extend(current.filter(|run| run.len() >= 2));
    out
}

/// How see-through an object is (TRANSPARENCY): its layer's, its block
/// reference's, or a percentage, 0 (opaque) to 90.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transparency {
    #[default]
    ByLayer,
    ByBlock,
    Percent(u8),
}

impl Transparency {
    /// DXF group 440: ByBlock 0x01000000, a value 0x02000000 plus its
    /// opacity 0–255; ByLayer is written as no value at all (0).
    pub fn to_dxf(self) -> i32 {
        match self {
            Transparency::ByLayer => 0,
            Transparency::ByBlock => 0x0100_0000,
            Transparency::Percent(p) => 0x0200_0000 | Self::opacity(p),
        }
    }

    pub fn from_dxf(v: i32) -> Self {
        match v {
            0x0100_0000 => Transparency::ByBlock,
            v if v & 0x0200_0000 != 0 => {
                let alpha = f64::from(v & 0xFF);
                Transparency::Percent(((1.0 - alpha / 255.0) * 100.0).round().clamp(0.0, 90.0) as u8)
            }
            _ => Transparency::ByLayer,
        }
    }

    /// 0–255 opacity of a percentage of transparency.
    pub fn opacity(percent: u8) -> i32 {
        (255.0 * (100.0 - f64::from(percent.min(90))) / 100.0).round() as i32
    }

    pub fn label(self) -> String {
        match self {
            Transparency::ByLayer => "ByLayer".into(),
            Transparency::ByBlock => "ByBlock".into(),
            Transparency::Percent(p) => p.to_string(),
        }
    }
}

/// Line weight as DXF stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lineweight {
    #[default]
    ByLayer,
    ByBlock,
    /// LWDEFAULT: 0.25 mm.
    Default,
    /// Hundredths of a millimeter.
    Hundredths(u16),
}

impl Lineweight {
    /// The lineweights AutoCAD offers, in hundredths of a millimeter.
    pub const STANDARD: [u16; 24] =
        [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

    /// From DXF group 370: -1 ByLayer, -2 ByBlock, -3 Default.
    pub fn from_dxf(v: i16) -> Self {
        match v {
            -2 => Lineweight::ByBlock,
            -3 => Lineweight::Default,
            v if v >= 0 => Lineweight::Hundredths(v as u16),
            _ => Lineweight::ByLayer,
        }
    }

    pub fn to_dxf(self) -> i16 {
        match self {
            Lineweight::ByLayer => -1,
            Lineweight::ByBlock => -2,
            Lineweight::Default => -3,
            Lineweight::Hundredths(v) => v as i16,
        }
    }

    /// Millimeters (Default is 0.25), or `None` for ByLayer and ByBlock.
    pub fn mm(self) -> Option<f64> {
        match self {
            Lineweight::Default => Some(0.25),
            Lineweight::Hundredths(v) => Some(f64::from(v) / 100.0),
            _ => None,
        }
    }

    pub fn label(self) -> String {
        match self {
            Lineweight::ByLayer => "ByLayer".into(),
            Lineweight::ByBlock => "ByBlock".into(),
            Lineweight::Default => "Default".into(),
            Lineweight::Hundredths(v) => format!("{:.2} mm", f64::from(v) / 100.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_follow_the_pattern_around_corners() {
        let path = [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0)];
        // 4 on, 2 off: 0-4, 6-10, then 12-16 (2 up the corner) and 18-20.
        let runs = dashes(&path, &[4.0, -2.0], 1.0);
        assert_eq!(runs.len(), 4);
        assert_eq!(runs[0], vec![Point::new(0.0, 0.0), Point::new(4.0, 0.0)]);
        assert_eq!(runs[1], vec![Point::new(6.0, 0.0), Point::new(10.0, 0.0)]);
        assert_eq!(runs[2], vec![Point::new(10.0, 2.0), Point::new(10.0, 6.0)]);
        assert_eq!(runs[3], vec![Point::new(10.0, 8.0), Point::new(10.0, 10.0)]);
        // Dots are single points.
        let dots = dashes(&path[..2], &[0.0, -2.5], 1.0);
        assert_eq!(dots.len(), 5);
        assert!(dots.iter().all(|d| d.len() == 1));
    }

    #[test]
    fn shapes_and_texts_fall_where_their_elements_end() {
        let gas = Embedded {
            element: 1,
            content: EmbeddedContent::Text { text: "GAS".into(), style: "Standard".into() },
            scale: 1.0,
            rotation: 0.0,
            turn: EmbeddedTurn::Upright,
            offset: Point::new(-1.0, 0.5),
        };
        let lt = Linetype { embedded: vec![gas], ..Linetype::new("GAS", "", &[4.0, -2.0, -2.0]) };
        assert!(!lt.is_continuous());
        // Along a line running left: 6 and 14 from its start, then 1 back
        // and 0.5 to its left.
        let path = [Point::new(20.0, 0.0), Point::new(0.0, 0.0)];
        let found = embedded_along(&path, &lt, 1.0);
        assert_eq!(found.len(), 2);
        assert!(found[0].1.approx_eq(Point::new(15.0, -0.5)) && found[1].1.approx_eq(Point::new(7.0, -0.5)));
        // Upright, it's turned around to read left to right.
        let (angle, turned) = found[0].0.direction(found[0].2);
        assert!(angle.abs() < 1e-9 && turned, "{angle}");
        // Twice as big: the second would be past the end.
        assert_eq!(embedded_along(&path, &lt, 2.0).len(), 1);
        // A pattern of dashes alone, with something on it, isn't continuous.
        let ticks = Linetype { embedded: lt.embedded.clone(), ..Linetype::new("TICKS", "", &[1.0, 1.0]) };
        assert!(!ticks.is_continuous() && Linetype::new("SOLID", "", &[1.0]).is_continuous());
    }

    #[test]
    fn lineweight_dxf_values() {
        for lw in [Lineweight::ByLayer, Lineweight::ByBlock, Lineweight::Default, Lineweight::Hundredths(50)] {
            assert_eq!(Lineweight::from_dxf(lw.to_dxf()), lw);
        }
        assert_eq!(Lineweight::Hundredths(50).label(), "0.50 mm");
        assert_eq!(Lineweight::Default.mm(), Some(0.25));
    }
}

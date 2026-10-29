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
#[derive(Debug, Clone, PartialEq)]
pub struct Linetype {
    pub name: String,
    pub description: String,
    /// Dash (positive), gap (negative) and dot (zero) lengths at LTSCALE 1,
    /// repeated along the curve. Empty for Continuous.
    pub pattern: Vec<f64>,
}

impl Linetype {
    pub fn new(name: &str, description: &str, pattern: &[f64]) -> Self {
        Self { name: name.into(), description: description.into(), pattern: pattern.to_vec() }
    }

    pub fn continuous() -> Self {
        Self::new(CONTINUOUS, "Solid line", &[])
    }

    /// Length of one repeat of the pattern.
    pub fn length(&self) -> f64 {
        self.pattern.iter().map(|d| d.abs()).sum()
    }

    pub fn is_continuous(&self) -> bool {
        self.pattern.iter().all(|&d| d > 0.0) || self.length() <= EPSILON
    }
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
    fn lineweight_dxf_values() {
        for lw in [Lineweight::ByLayer, Lineweight::ByBlock, Lineweight::Default, Lineweight::Hundredths(50)] {
            assert_eq!(Lineweight::from_dxf(lw.to_dxf()), lw);
        }
        assert_eq!(Lineweight::Hundredths(50).label(), "0.50 mm");
        assert_eq!(Lineweight::Default.mm(), Some(0.25));
    }
}

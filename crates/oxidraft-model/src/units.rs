//! UNITS: how lengths and angles show (decimal, feet and inches,
//! fractions; degrees, minutes and seconds, grads, radians, bearings), and
//! what a drawing unit is. The drawing's units are in effect while it's
//! worked on (see [`set_current`]), so prompts and listings use them.

use std::cell::Cell;

/// How lengths show ($LUNITS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthType {
    Scientific,
    Decimal,
    /// Feet and decimal inches: 1'-3.50".
    Engineering,
    /// Feet and fractional inches: 1'-3 1/2".
    Architectural,
    /// Whole units and a fraction: 15 1/2.
    Fractional,
}

impl LengthType {
    pub const ALL: [LengthType; 5] = [
        LengthType::Architectural,
        LengthType::Decimal,
        LengthType::Engineering,
        LengthType::Fractional,
        LengthType::Scientific,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LengthType::Scientific => "Scientific",
            LengthType::Decimal => "Decimal",
            LengthType::Engineering => "Engineering",
            LengthType::Architectural => "Architectural",
            LengthType::Fractional => "Fractional",
        }
    }

    /// The $LUNITS value.
    pub fn code(self) -> i16 {
        match self {
            LengthType::Scientific => 1,
            LengthType::Decimal => 2,
            LengthType::Engineering => 3,
            LengthType::Architectural => 4,
            LengthType::Fractional => 5,
        }
    }

    pub fn from_code(code: i16) -> LengthType {
        match code {
            1 => LengthType::Scientific,
            3 => LengthType::Engineering,
            4 | 6 => LengthType::Architectural,
            5 | 7 => LengthType::Fractional,
            _ => LengthType::Decimal,
        }
    }

    /// True if lengths are in inches shown as feet and inches.
    pub fn feet_and_inches(self) -> bool {
        matches!(self, LengthType::Engineering | LengthType::Architectural)
    }
}

/// How angles show ($AUNITS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleType {
    DecimalDegrees,
    /// 45d30'15".
    DegMinSec,
    Grads,
    Radians,
    /// Bearings: N 45d E.
    Surveyor,
}

impl AngleType {
    pub const ALL: [AngleType; 5] =
        [AngleType::DecimalDegrees, AngleType::DegMinSec, AngleType::Grads, AngleType::Radians, AngleType::Surveyor];

    pub fn name(self) -> &'static str {
        match self {
            AngleType::DecimalDegrees => "Decimal Degrees",
            AngleType::DegMinSec => "Deg/Min/Sec",
            AngleType::Grads => "Grads",
            AngleType::Radians => "Radians",
            AngleType::Surveyor => "Surveyor's Units",
        }
    }

    pub fn code(self) -> i16 {
        self as i16
    }

    pub fn from_code(code: i16) -> AngleType {
        match code {
            1 => AngleType::DegMinSec,
            2 => AngleType::Grads,
            3 => AngleType::Radians,
            4 => AngleType::Surveyor,
            _ => AngleType::DecimalDegrees,
        }
    }
}

/// $INSUNITS values and names, with each unit in meters (none for
/// Unitless).
pub const INSERTION_UNITS: &[(i16, &str, f64)] = &[
    (0, "Unitless", 0.0),
    (1, "Inches", 0.0254),
    (2, "Feet", 0.3048),
    (3, "Miles", 1609.344),
    (4, "Millimeters", 0.001),
    (5, "Centimeters", 0.01),
    (6, "Meters", 1.0),
    (7, "Kilometers", 1000.0),
    (8, "Microinches", 0.0254e-6),
    (9, "Mils", 0.0254e-3),
    (10, "Yards", 0.9144),
    (11, "Angstroms", 1e-10),
    (12, "Nanometers", 1e-9),
    (13, "Microns", 1e-6),
    (14, "Decimeters", 0.1),
    (15, "Decameters", 10.0),
    (16, "Hectometers", 100.0),
    (17, "Gigameters", 1e9),
    (18, "Astronomical Units", 1.495_978_707e11),
    (19, "Light Years", 9.460_730_472_580_8e15),
    (20, "Parsecs", 3.085_677_581_49e16),
    (21, "US Survey Feet", 1200.0 / 3937.0),
];

/// The name of an $INSUNITS value.
pub fn insertion_name(code: i16) -> &'static str {
    INSERTION_UNITS.iter().find(|u| u.0 == code).map_or("Unitless", |u| u.1)
}

/// How much a drawing in `from` units is scaled to go into one in `to`
/// units (1 if either is unitless).
pub fn conversion(from: i16, to: i16) -> f64 {
    let meters = |c: i16| INSERTION_UNITS.iter().find(|u| u.0 == c).map_or(0.0, |u| u.2);
    let (a, b) = (meters(from), meters(to));
    if a > 0.0 && b > 0.0 { a / b } else { 1.0 }
}

/// A drawing's units settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Units {
    pub length: LengthType,
    /// Decimal places, or for fractions the denominator's power of two.
    pub length_precision: u8,
    pub angle: AngleType,
    pub angle_precision: u8,
    /// Angles measured clockwise ($ANGDIR).
    pub clockwise: bool,
    /// The direction of angle 0, in degrees counterclockwise from east
    /// ($ANGBASE).
    pub base_angle: f64,
    /// What a drawing unit is ($INSUNITS).
    pub insertion: i16,
}

pub const DEFAULT_UNITS: Units = Units {
    length: LengthType::Decimal,
    length_precision: 4,
    angle: AngleType::DecimalDegrees,
    angle_precision: 0,
    clockwise: false,
    base_angle: 0.0,
    insertion: 4,
};

impl Default for Units {
    fn default() -> Self {
        DEFAULT_UNITS
    }
}

/// `v` rounded to `1 / denominator`, as whole part and numerator.
fn fraction(v: f64, denominator: u32) -> (f64, u32) {
    let n = (v * f64::from(denominator)).round();
    let whole = (n / f64::from(denominator)).floor();
    (whole, (n - whole * f64::from(denominator)) as u32)
}

/// "1/2"-style fraction text (reduced), or "" for none.
fn fraction_text(numerator: u32, denominator: u32) -> String {
    if numerator == 0 {
        return String::new();
    }
    let mut d = denominator;
    let mut n = numerator;
    while n.is_multiple_of(2) && d.is_multiple_of(2) {
        n /= 2;
        d /= 2;
    }
    format!("{n}/{d}")
}

impl Units {
    /// A length as these units show it.
    pub fn length(&self, v: f64) -> String {
        let p = usize::from(self.length_precision.min(8));
        let sign = if v < 0.0 && v.abs() >= 0.5 * 10f64.powi(-(p as i32)) { "-" } else { "" };
        let a = v.abs();
        let denominator = 1u32 << p.min(8);
        match self.length {
            LengthType::Decimal => format!("{v:.p$}"),
            LengthType::Scientific => {
                if a == 0.0 {
                    return format!("{:.p$}E+00", 0.0);
                }
                let mut e = a.log10().floor() as i32;
                let mut m = a / 10f64.powi(e);
                if format!("{m:.p$}").starts_with("10") {
                    m /= 10.0;
                    e += 1;
                }
                let es = if e < 0 { '-' } else { '+' };
                format!("{sign}{m:.p$}E{es}{:02}", e.abs())
            }
            LengthType::Engineering => {
                let inches = format!("{:.p$}", a % 12.0);
                let (mut feet, mut inches) = ((a / 12.0).floor(), inches.parse::<f64>().unwrap_or(0.0));
                if inches >= 12.0 {
                    feet += 1.0;
                    inches -= 12.0;
                }
                format!("{sign}{feet:.0}'-{inches:.p$}\"")
            }
            LengthType::Architectural => {
                let (total, numerator) = fraction(a, denominator);
                let (feet, inches) = ((total / 12.0).floor(), total % 12.0);
                let f = fraction_text(numerator, denominator);
                let inch = if f.is_empty() { format!("{inches:.0}") } else { format!("{inches:.0} {f}") };
                format!("{sign}{feet:.0}'-{inch}\"")
            }
            LengthType::Fractional => {
                let (whole, numerator) = fraction(a, denominator);
                let f = fraction_text(numerator, denominator);
                match (whole, f.is_empty()) {
                    (w, true) => format!("{sign}{w:.0}"),
                    (0.0, false) => format!("{sign}{f}"),
                    (w, false) => format!("{sign}{w:.0} {f}"),
                }
            }
        }
    }

    /// The angle shown for a direction `deg` degrees counterclockwise from
    /// east: from the base angle, the way angles are measured.
    pub fn direction(&self, deg: f64) -> String {
        if self.angle == AngleType::Surveyor {
            return self.bearing(deg);
        }
        let a = (deg - self.base_angle) * if self.clockwise { -1.0 } else { 1.0 };
        self.angle(a.rem_euclid(360.0))
    }

    /// An angle (a size, not a direction) as these units show it.
    pub fn angle(&self, deg: f64) -> String {
        let p = usize::from(self.angle_precision.min(8));
        match self.angle {
            AngleType::DecimalDegrees => format!("{deg:.p$}"),
            AngleType::Grads => format!("{:.p$}g", deg * 400.0 / 360.0),
            AngleType::Radians => format!("{:.p$}r", deg.to_radians()),
            AngleType::DegMinSec | AngleType::Surveyor => dms(deg, p),
        }
    }

    /// A direction as a bearing: N or S, the angle off it, then E or W.
    fn bearing(&self, deg: f64) -> String {
        let d = deg.rem_euclid(360.0);
        let p = usize::from(self.angle_precision.min(8));
        let near = |a: f64| (d - a).abs() < 1e-9 || (d - a - 360.0).abs() < 1e-9;
        for (a, name) in [(0.0, "E"), (90.0, "N"), (180.0, "W"), (270.0, "S")] {
            if near(a) {
                return name.into();
            }
        }
        let (ns, off, ew) = match d {
            d if d < 90.0 => ("N", 90.0 - d, "E"),
            d if d < 180.0 => ("N", d - 90.0, "W"),
            d if d < 270.0 => ("S", 270.0 - d, "W"),
            d => ("S", d - 270.0, "E"),
        };
        format!("{ns} {} {ew}", dms(off, p))
    }

    /// An area: square units, and for feet and inches square feet too.
    pub fn area(&self, v: f64) -> String {
        let p = usize::from(self.length_precision.min(8));
        match self.length {
            LengthType::Scientific => Units { length_precision: self.length_precision, ..*self }.length(v),
            LengthType::Engineering | LengthType::Architectural => {
                format!("{v:.p$} square in. ({:.p$} square ft.)", v / 144.0)
            }
            _ => format!("{v:.p$}"),
        }
    }

    /// A point's coordinates: "x,y".
    pub fn point(&self, x: f64, y: f64) -> String {
        format!("{},{}", self.length(x), self.length(y))
    }
}

/// Degrees, minutes and seconds: precision 0 whole degrees, 1–2 with
/// minutes, 3–4 with seconds, more with decimals of seconds.
fn dms(deg: f64, precision: usize) -> String {
    let sign = if deg < 0.0 { "-" } else { "" };
    let a = deg.abs();
    match precision {
        0 => format!("{sign}{:.0}d", a),
        1 | 2 => {
            let total = (a * 60.0).round();
            format!("{sign}{:.0}d{:.0}'", (total / 60.0).floor(), total % 60.0)
        }
        _ => {
            let decimals = precision.saturating_sub(4);
            let scale = 10f64.powi(decimals as i32);
            let total = (a * 3600.0 * scale).round() / scale;
            let (d, rest) = ((total / 3600.0).floor(), total % 3600.0);
            let (m, s) = ((rest / 60.0).floor(), rest % 60.0);
            format!("{sign}{d:.0}d{m:.0}'{s:.decimals$}\"")
        }
    }
}

/// A length typed in any of the units' forms: decimals, fractions (1/2,
/// 3-1/2), or feet and inches (5', 5'6", 5'-6 1/2", 6-1/2"; a foot is 12
/// units).
pub fn parse_length(text: &str) -> Option<f64> {
    let t = text.trim();
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    if !t.contains(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let (sign, t) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    // A number with a fraction: 3-1/2, 3 1/2, 1/2.
    let mixed = |s: &str| -> Option<f64> {
        let s = s.trim();
        if s.is_empty() {
            return Some(0.0);
        }
        if let Ok(v) = s.parse::<f64>() {
            return Some(v);
        }
        let (whole, frac) = match s.rsplit_once(['-', ' ']) {
            Some((w, f)) => (w.trim().parse::<f64>().ok()?, f),
            None => (0.0, s),
        };
        let (n, d) = frac.split_once('/')?;
        let (n, d) = (n.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?);
        (d != 0.0).then(|| whole + n / d)
    };
    let v = match t.split_once('\'') {
        Some((feet, inches)) => {
            let inches = inches.trim().trim_start_matches('-').trim_end_matches('"');
            feet.trim().parse::<f64>().ok()? * 12.0 + mixed(inches)?
        }
        None => mixed(t.trim_end_matches('"'))?,
    };
    Some(sign * v)
}

/// An angle typed as degrees (45, 45d30'15"), grads (50g), radians
/// (0.785r) or a bearing (N45d30'E), in degrees.
pub fn parse_angle(text: &str) -> Option<f64> {
    let t = text.trim();
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    let upper = t.to_ascii_uppercase();
    if let Some(b) = bearing(&upper) {
        return Some(b);
    }
    if let Some(g) = upper.strip_suffix('G') {
        return g.trim().parse::<f64>().ok().map(|g| g * 360.0 / 400.0);
    }
    if let Some(r) = upper.strip_suffix('R') {
        return r.trim().parse::<f64>().ok().map(f64::to_degrees);
    }
    parse_dms(&upper)
}

fn parse_dms(t: &str) -> Option<f64> {
    let (sign, t) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t),
    };
    let (d, rest) = t.split_once('D').unwrap_or((t, ""));
    let (m, rest) = rest.split_once('\'').unwrap_or((rest, ""));
    let s = rest.trim_end_matches('"');
    let part = |s: &str| if s.trim().is_empty() { Some(0.0) } else { s.trim().parse::<f64>().ok() };
    Some(sign * (part(d)? + part(m)? / 60.0 + part(s)? / 3600.0))
}

/// A direction typed at a polar coordinate (`d<angle`), in degrees
/// counterclockwise from east: measured from the base angle the way the
/// units measure angles, unless it's a bearing.
pub fn parse_direction(text: &str) -> Option<f64> {
    if let Some(b) = bearing(&text.trim().to_ascii_uppercase()) {
        return Some(b);
    }
    let u = current();
    let a = parse_angle(text)?;
    Some(u.base_angle + if u.clockwise { -a } else { a })
}

/// N45d30'E and the like: directions counterclockwise from east.
fn bearing(t: &str) -> Option<f64> {
    let first = t.chars().next()?;
    let last = t.chars().last()?;
    if !matches!(first, 'N' | 'S') || !matches!(last, 'E' | 'W') || t.len() < 3 {
        return None;
    }
    let off = parse_dms(t[1..t.len() - 1].trim())?;
    Some(match (first, last) {
        ('N', 'E') => 90.0 - off,
        ('N', _) => 90.0 + off,
        ('S', 'W') => 270.0 - off,
        _ => 270.0 + off,
    })
}

thread_local! {
    /// The units of the drawing being worked on.
    static CURRENT: Cell<Units> = const { Cell::new(DEFAULT_UNITS) };
}

/// The units in effect.
pub fn current() -> Units {
    CURRENT.get()
}

/// Puts a drawing's units in effect (the app and the command engine do,
/// as they work on it).
pub fn set_current(units: Units) {
    CURRENT.set(units);
}

/// A length in the units in effect.
pub fn length(v: f64) -> String {
    current().length(v)
}

/// An area in the units in effect.
pub fn area(v: f64) -> String {
    current().area(v)
}

/// A direction in the units in effect.
pub fn direction(deg: f64) -> String {
    current().direction(deg)
}

/// An angle's size in the units in effect.
pub fn angle(deg: f64) -> String {
    current().angle(deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(length: LengthType, precision: u8) -> Units {
        Units { length, length_precision: precision, ..DEFAULT_UNITS }
    }

    #[test]
    fn lengths_show_as_the_units_say() {
        assert_eq!(units(LengthType::Decimal, 2).length(15.5), "15.50");
        assert_eq!(units(LengthType::Engineering, 2).length(15.5), "1'-3.50\"");
        assert_eq!(units(LengthType::Engineering, 1).length(23.97), "2'-0.0\"", "carries into the feet");
        assert_eq!(units(LengthType::Architectural, 4).length(15.5), "1'-3 1/2\"");
        assert_eq!(units(LengthType::Architectural, 4).length(24.0), "2'-0\"");
        assert_eq!(units(LengthType::Architectural, 4).length(0.5), "0'-0 1/2\"");
        assert_eq!(units(LengthType::Architectural, 0).length(-15.4), "-1'-3\"");
        assert_eq!(units(LengthType::Fractional, 3).length(15.375), "15 3/8");
        assert_eq!(units(LengthType::Fractional, 3).length(0.25), "1/4");
        assert_eq!(units(LengthType::Scientific, 2).length(1234.5), "1.23E+03");
        assert_eq!(units(LengthType::Scientific, 1).length(0.0123), "1.2E-02");
    }

    #[test]
    fn angles_show_as_the_units_say() {
        let u = |angle, angle_precision| Units { angle, angle_precision, ..DEFAULT_UNITS };
        assert_eq!(u(AngleType::DecimalDegrees, 2).angle(45.5), "45.50");
        assert_eq!(u(AngleType::DegMinSec, 4).angle(45.5042), "45d30'15\"");
        assert_eq!(u(AngleType::DegMinSec, 2).angle(45.5), "45d30'");
        assert_eq!(u(AngleType::Grads, 1).angle(90.0), "100.0g");
        assert_eq!(u(AngleType::Radians, 3).angle(180.0), "3.142r");
        assert_eq!(u(AngleType::Surveyor, 0).direction(45.0), "N 45d E");
        assert_eq!(u(AngleType::Surveyor, 0).direction(200.0), "S 70d W");
        assert_eq!(u(AngleType::Surveyor, 0).direction(90.0), "N");
        // Measured clockwise from north.
        let compass = Units { clockwise: true, base_angle: 90.0, ..DEFAULT_UNITS };
        assert_eq!(compass.direction(0.0), "90");
        assert_eq!(compass.direction(45.0), "45");
    }

    #[test]
    fn lengths_and_angles_parse_in_any_form() {
        assert_eq!(parse_length("12.5"), Some(12.5));
        assert_eq!(parse_length("5'"), Some(60.0));
        assert_eq!(parse_length("5'6\""), Some(66.0));
        assert_eq!(parse_length("5'-6 1/2\""), Some(66.5));
        assert_eq!(parse_length("6-1/2\""), Some(6.5));
        assert_eq!(parse_length("3/4"), Some(0.75));
        assert_eq!(parse_length("-1'"), Some(-12.0));
        assert_eq!(parse_length("abc"), None);
        assert_eq!(parse_angle("45d30'"), Some(45.5));
        assert_eq!(parse_angle("100g"), Some(90.0));
        assert!((parse_angle("3.14159265r").unwrap() - 180.0).abs() < 1e-6);
        assert_eq!(parse_angle("N45dE"), Some(45.0));
        assert_eq!(parse_angle("S30dW"), Some(240.0));
        assert!((conversion(1, 4) - 25.4).abs() < 1e-12, "inches into millimeters");
        assert_eq!(conversion(0, 4), 1.0);
    }
}

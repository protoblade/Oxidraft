use oxidraft_geom::Point;

/// One answer to a prompt.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Point(Point),
    /// A bare number: a distance, radius, etc.
    Value(f64),
    /// One of the prompt's keywords, normalized to its full spelling.
    Keyword(&'static str),
    /// Enter (or Space) with nothing typed.
    Enter,
    /// The objects chosen at a "Select objects:" prompt.
    Selection(crate::Selection),
    /// A click at a single-object prompt (see [`crate::Command::wants_pick`]).
    Pick {
        at: Point,
        shift: bool,
    },
    /// Free text, such as a block name (see [`crate::Command::wants_text`]).
    Text(String),
}

/// Parses command-line text typed at a prompt.
///
/// Accepted forms, as in AutoCAD:
/// - `x,y` absolute point
/// - `@dx,dy` relative to `last`
/// - `d<angle` absolute polar, `@d<angle` relative polar
/// - `12.5` a value
/// - a keyword: its capital letters (`X` for `eXit`) or a prefix of it (`cl` for `Close`)
///
/// With dynamic input on (`dyn_relative`), coordinates are relative to `last`
/// unless prefixed with `#`, as in AutoCAD's dynamic input tooltip.
pub fn parse_input(
    text: &str,
    last: Option<Point>,
    keywords: &[&'static str],
    dyn_relative: bool,
) -> Result<Input, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(Input::Enter);
    }

    let (relative, body) = if let Some(rest) = t.strip_prefix('@') {
        (true, rest.trim())
    } else if let Some(rest) = t.strip_prefix('#') {
        (false, rest.trim())
    } else {
        let looks_like_point = t.contains(',') || t.contains('<');
        (dyn_relative && last.is_some() && looks_like_point, t)
    };
    let base = if relative { last.ok_or("No last point for relative (@) input.")? } else { Point::ORIGIN };

    if relative && body.is_empty() {
        return Ok(Input::Point(base));
    }
    if let Some((a, b)) = body.split_once(',') {
        let (x, y) = (num(a)?, num(b)?);
        return Ok(Input::Point(base + Point::new(x, y)));
    }
    if let Some((d, a)) = body.split_once('<') {
        let angle = oxidraft_model::units::parse_direction(a).ok_or_else(|| format!("Not an angle: {}", a.trim()))?;
        return Ok(Input::Point(base.polar(num(d)?, angle)));
    }
    if !relative && let Some(v) = oxidraft_model::units::parse_length(body) {
        return Ok(Input::Value(v));
    }

    let upper = t.to_ascii_uppercase();
    let abbreviation = |k: &str| k.chars().filter(char::is_ascii_uppercase).collect::<String>();
    keywords
        .iter()
        .find(|k| abbreviation(k) == upper)
        .or_else(|| keywords.iter().find(|k| k.to_ascii_uppercase().starts_with(&upper)))
        .map(|&k| Input::Keyword(k))
        .ok_or_else(|| format!("Invalid input: {t}"))
}

/// A number or a length in any of the units' forms (5'6", 3-1/2).
fn num(s: &str) -> Result<f64, String> {
    oxidraft_model::units::parse_length(s).ok_or_else(|| format!("Not a number: {}", s.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KW: &[&str] = &["Close", "Undo", "eXit"];

    fn parse(t: &str, last: Option<Point>) -> Result<Input, String> {
        parse_input(t, last, KW, false)
    }

    #[test]
    fn parses_points_values_and_keywords() {
        let last = Some(Point::new(10.0, 10.0));
        assert_eq!(parse("3,4", last), Ok(Input::Point(Point::new(3.0, 4.0))));
        assert_eq!(parse("@3,-4", last), Ok(Input::Point(Point::new(13.0, 6.0))));
        assert_eq!(parse("25", last), Ok(Input::Value(25.0)));
        assert_eq!(parse("", last), Ok(Input::Enter));
        assert_eq!(parse("c", last), Ok(Input::Keyword("Close")));
        assert_eq!(parse("UNDO", last), Ok(Input::Keyword("Undo")));
        assert_eq!(parse("x", last), Ok(Input::Keyword("eXit")));
        assert!(parse("q", last).is_err());
    }

    #[test]
    fn parses_polar() {
        let Ok(Input::Point(p)) = parse("@10<90", Some(Point::new(1.0, 1.0))) else {
            panic!("expected a point");
        };
        assert!(p.approx_eq(Point::new(1.0, 11.0)));
    }

    #[test]
    fn dynamic_input_is_relative_unless_hash() {
        let last = Some(Point::new(10.0, 10.0));
        assert_eq!(parse_input("5,0", last, KW, true), Ok(Input::Point(Point::new(15.0, 10.0))));
        assert_eq!(parse_input("#5,0", last, KW, true), Ok(Input::Point(Point::new(5.0, 0.0))));
        assert_eq!(parse_input("5,0", None, KW, true), Ok(Input::Point(Point::new(5.0, 0.0))));
    }

    #[test]
    fn relative_without_last_point_is_an_error() {
        assert!(parse("@1,1", None).is_err());
    }
}

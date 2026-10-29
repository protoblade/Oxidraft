//! Fields (FIELD): text that shows a value worked out as it's drawn — the
//! date, the drawing's file name or properties, an object's area — kept in
//! the text as AutoCAD's field codes: `%<\AcVar Date \f "M/d/yyyy">%`.

use std::cell::RefCell;
use std::ops::Range;

use crate::{Document, EntityId, Geometry, LengthType, units};

/// A moment, as fields show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Moment {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Moment {
    /// 0 for Sunday.
    fn weekday(&self) -> usize {
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let m = self.month.clamp(1, 12) as usize;
        let y = if m < 3 { self.year - 1 } else { self.year };
        (y + y / 4 - y / 100 + y / 400 + T[m - 1] + self.day as i32).rem_euclid(7) as usize
    }
}

/// What fields need from outside the drawing: the time, the file and who's
/// working. The app keeps it up to date.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldEnv {
    pub now: Moment,
    /// The drawing's file, or its name while it has none.
    pub path: String,
    pub created: Option<Moment>,
    pub saved: Option<Moment>,
    pub plotted: Option<Moment>,
    pub login: String,
}

thread_local! {
    static ENV: RefCell<FieldEnv> = RefCell::new(FieldEnv::default());
}

/// Sets what fields show for the time, the file and the user.
pub fn set_env(env: FieldEnv) {
    ENV.with(|e| *e.borrow_mut() = env);
}

pub fn env() -> FieldEnv {
    ENV.with(|e| e.borrow().clone())
}

/// What a field shows when it can't be worked out, as in AutoCAD.
pub const UNKNOWN: &str = "####";

/// Whether `text` has a field in it.
pub fn has_fields(text: &str) -> bool {
    text.contains("%<\\")
}

/// Each field in `text`: where it is (`%<` to `>%`) and its code (what's
/// between). Fields inside a field's code (`%<\_ObjId 42>%`) are part of it.
pub fn fields(text: &str) -> Vec<(Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = text[at..].find("%<\\").map(|p| p + at) {
        let mut depth = 0;
        let mut i = start;
        let mut end = None;
        while i < text.len() {
            if text[i..].starts_with("%<") {
                depth += 1;
                i += 2;
            } else if text[i..].starts_with(">%") {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            } else {
                i += text[i..].chars().next().map_or(1, char::len_utf8);
            }
        }
        let Some(end) = end else { break };
        out.push((start..end, &text[start + 2..end - 2]));
        at = end;
    }
    out
}

/// `text` with its fields' values in place of their codes.
pub fn evaluate(text: &str, doc: &Document) -> String {
    if !has_fields(text) {
        return text.to_string();
    }
    let mut out = String::new();
    let mut last = 0;
    for (range, code) in fields(text) {
        out.push_str(&text[last..range.start]);
        out.push_str(&value(code, doc).unwrap_or_else(|| UNKNOWN.to_string()));
        last = range.end;
    }
    out.push_str(&text[last..]);
    out
}

/// A field code's parts: `\AcVar Date \f "M/d/yyyy"` is ("AcVar", "Date",
/// "M/d/yyyy").
fn parse(code: &str) -> (&str, &str, &str) {
    let code = code.trim().trim_start_matches('\\');
    let (evaluator, rest) = code.split_once(char::is_whitespace).unwrap_or((code, ""));
    let (name, format) = match rest.find("\\f ") {
        Some(p) => (&rest[..p], rest[p + 3..].trim().trim_matches('"')),
        None => (rest, ""),
    };
    (evaluator, name.trim(), format)
}

/// The value a field code stands for.
pub fn value(code: &str, doc: &Document) -> Option<String> {
    let (evaluator, name, format) = parse(code);
    let env = env();
    match evaluator {
        "AcVar" => {
            let date = |m: Option<Moment>| m.map_or_else(|| "----".to_string(), |m| format_date(m, format));
            let props = doc.properties();
            Some(match name.to_ascii_lowercase().as_str() {
                "date" => format_date(env.now, format),
                "createdate" => date(env.created),
                "savedate" => date(env.saved),
                "plotdate" => date(env.plotted.or(Some(env.now))),
                "filename" => text_case(&file_name(&env.path, format), format),
                "dwgname" => file_name(&env.path, "%fn6"),
                "dwgprefix" => file_name(&env.path, "%fn1"),
                "login" | "loginname" => text_case(&env.login, format),
                "author" => text_case(&props.author, format),
                "title" => text_case(&props.title, format),
                "subject" => text_case(&props.subject, format),
                "keywords" => text_case(&props.keywords, format),
                "comments" => text_case(&props.comments, format),
                "ltscale" => number(doc.ltscale(), format, false),
                _ => return None,
            })
        }
        "AcObjProp" => {
            let id = object_id(name)?;
            let property = name.rsplit_once(").")?.1;
            object_property(doc, id, property, format)
        }
        _ => None,
    }
}

/// The object an object field is about: `Object(%<\_ObjId 42>%).Area`.
pub fn object_id(name: &str) -> Option<EntityId> {
    let p = name.find("\\_ObjId ")?;
    let digits: String = name[p + 8..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().map(EntityId)
}

/// The code of a field showing `property` of object `id`.
pub fn object_code(id: EntityId, property: &str, format: &str) -> String {
    let format = if format.is_empty() { String::new() } else { format!(" \\f \"{format}\"") };
    format!("%<\\AcObjProp Object(%<\\_ObjId {}>%).{property}{format}>%", id.0)
}

/// The properties an object field can show of `g`.
pub fn object_properties(g: &Geometry) -> Vec<&'static str> {
    let mut out = Vec::new();
    match g {
        Geometry::Circle { .. } => out.extend(["Area", "Circumference", "Diameter", "Radius"]),
        Geometry::Arc { .. } => out.extend(["ArcLength", "Radius"]),
        Geometry::Text { .. } => out.push("TextString"),
        _ => {
            if g.area().is_some() {
                out.push("Area");
            }
            if g.length() > 0.0 {
                out.push("Length");
            }
        }
    }
    out.push("Layer");
    out
}

fn object_property(doc: &Document, id: EntityId, property: &str, format: &str) -> Option<String> {
    let e = doc.entity(id)?;
    let g = &e.geometry;
    let radius = match g {
        Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. } => Some(*radius),
        _ => None,
    };
    Some(match property.to_ascii_lowercase().as_str() {
        "area" => number(g.area()?, format, true),
        "length" | "arclength" | "circumference" | "perimeter" => number(g.length(), format, false),
        "radius" => number(radius?, format, false),
        "diameter" => number(radius? * 2.0, format, false),
        "layer" => text_case(&e.layer, format),
        "textstring" | "contents" => match g {
            Geometry::Text { content, .. } if !has_fields(content) => text_case(content, format),
            _ => return None,
        },
        _ => return None,
    })
}

/// A format's `%xxN` setting: `%lu2%pr3` has `lu` 2 and `pr` 3.
fn setting(format: &str, key: &str) -> Option<u32> {
    let p = format.find(&format!("%{key}"))?;
    let digits: String = format[p + 1 + key.len()..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// A format's `%xx[...]` setting, as text.
fn bracketed<'a>(format: &'a str, key: &str) -> Option<&'a str> {
    let p = format.find(&format!("%{key}["))?;
    let rest = &format[p + key.len() + 2..];
    rest.find(']').map(|end| &rest[..end])
}

/// A number in a field's units (`%lu`), precision (`%pr`), conversion
/// factor (`%ct8[f]`) and prefix and suffix (`%ps[pre,post]`); otherwise
/// the drawing's units.
fn number(v: f64, format: &str, area: bool) -> String {
    let mut u = units::current();
    if let Some(lu) = setting(format, "lu") {
        u.length = match lu {
            1 => LengthType::Scientific,
            3 => LengthType::Engineering,
            4 => LengthType::Architectural,
            5 => LengthType::Fractional,
            _ => LengthType::Decimal,
        };
    }
    if let Some(pr) = setting(format, "pr") {
        u.length_precision = pr.min(8) as u8;
    }
    let factor = bracketed(format, "ct8").and_then(|f| f.trim().parse::<f64>().ok()).unwrap_or(1.0);
    let v = v * factor;
    let shown = if area && matches!(u.length, LengthType::Decimal) {
        format!("{v:.p$}", p = usize::from(u.length_precision))
    } else if area {
        u.area(v)
    } else {
        u.length(v)
    };
    match bracketed(format, "ps").and_then(|ps| ps.split_once(',')) {
        Some((pre, post)) => format!("{pre}{shown}{post}"),
        None => shown,
    }
}

/// Text in a field's case (`%tc1` upper, `%tc2` lower, `%tc3` first
/// capital, `%tc4` title case).
fn text_case(s: &str, format: &str) -> String {
    match setting(format, "tc") {
        Some(1) => s.to_uppercase(),
        Some(2) => s.to_lowercase(),
        Some(3) => {
            let mut c = s.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        }
        Some(4) => s
            .split(' ')
            .map(|w| {
                let mut c = w.chars();
                c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect())
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => s.to_string(),
    }
}

/// The parts of a file's path a Filename field shows (`%fn`: 1 the folder,
/// 2 the name, 4 the extension; all of it when not given).
fn file_name(path: &str, format: &str) -> String {
    let bits = setting(format, "fn").unwrap_or(7);
    let p = std::path::Path::new(path);
    let mut out = String::new();
    if bits & 1 != 0
        && let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty())
    {
        out.push_str(&dir.display().to_string());
        out.push(std::path::MAIN_SEPARATOR);
    }
    if bits & 2 != 0 {
        let name = if bits & 4 != 0 { p.file_name() } else { p.file_stem() };
        out.push_str(&name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    }
    out
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// A moment in an AutoCAD date format: `M/d/yyyy`, `dddd, MMMM d, yyyy`,
/// `h:mm tt`, `HH:mm:ss`, ... (M is the month, m the minute).
pub fn format_date(m: Moment, format: &str) -> String {
    let format = if format.is_empty() { "M/d/yyyy" } else { format };
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        let hour12 = if m.hour.is_multiple_of(12) { 12 } else { m.hour % 12 };
        let month = MONTHS[(m.month.clamp(1, 12) - 1) as usize];
        let day = DAYS[m.weekday()];
        let num = |v: u32, n: usize| if n >= 2 { format!("{v:02}") } else { v.to_string() };
        match c {
            'y' if n >= 3 => out.push_str(&format!("{:04}", m.year)),
            'y' => out.push_str(&format!("{:02}", m.year.rem_euclid(100))),
            'M' if n >= 4 => out.push_str(month),
            'M' if n == 3 => out.push_str(&month[..3]),
            'M' => out.push_str(&num(m.month, n)),
            'd' if n >= 4 => out.push_str(day),
            'd' if n == 3 => out.push_str(&day[..3]),
            'd' => out.push_str(&num(m.day, n)),
            'H' => out.push_str(&num(m.hour, n)),
            'h' => out.push_str(&num(hour12, n)),
            'm' => out.push_str(&num(m.minute, n)),
            's' => out.push_str(&num(m.second, n)),
            't' => out.push_str(if m.hour < 12 { &"AM"[..n.min(2)] } else { &"PM"[..n.min(2)] }),
            _ => {
                for _ in 0..n {
                    out.push(c);
                }
            }
        }
        i += n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawingProperties;
    use oxidraft_geom::Point;

    fn moment() -> Moment {
        Moment { year: 2026, month: 10, day: 29, hour: 14, minute: 5, second: 9 }
    }

    #[test]
    fn dates_format_as_in_autocad() {
        let m = moment();
        assert_eq!(format_date(m, "M/d/yyyy"), "10/29/2026");
        assert_eq!(format_date(m, "dddd, MMMM d, yyyy"), "Thursday, October 29, 2026");
        assert_eq!(format_date(m, "dd-MMM-yy"), "29-Oct-26");
        assert_eq!(format_date(m, "h:mm tt"), "2:05 PM");
        assert_eq!(format_date(m, "HH:mm:ss"), "14:05:09");
        assert_eq!(format_date(m, "yyyy-MM-dd"), "2026-10-29");
    }

    #[test]
    fn fields_evaluate_in_text() {
        set_env(FieldEnv { now: moment(), path: "/plans/House.dwg".into(), login: "pat".into(), ..Default::default() });
        let mut doc = Document::new();
        doc.set_properties(DrawingProperties { title: "ground floor".into(), ..Default::default() });
        let text = r#"Plotted %<\AcVar Date \f "MMMM d, yyyy">% from %<\AcVar Filename \f "%fn2">%"#;
        assert_eq!(evaluate(text, &doc), "Plotted October 29, 2026 from House");
        assert_eq!(evaluate(r#"%<\AcVar Title \f "%tc4">%"#, &doc), "Ground Floor");
        assert_eq!(evaluate(r#"%<\AcVar Filename>%"#, &doc), format!("/plans{}House.dwg", std::path::MAIN_SEPARATOR));
        assert_eq!(evaluate(r#"by %<\AcVar Login>%: %<\AcVar Nothing>%"#, &doc), "by pat: ####");
        assert_eq!(evaluate("no fields", &doc), "no fields");
    }

    #[test]
    fn object_fields_show_its_properties() {
        let mut doc = Document::new();
        let id = doc.add(Geometry::Circle { center: Point::new(0.0, 0.0), radius: 2.0 });
        let code = object_code(id, "Area", "%lu2%pr2%ps[Area: , sq]");
        assert_eq!(fields(&code).len(), 1);
        assert_eq!(evaluate(&code, &doc), "Area: 12.57 sq");
        assert_eq!(evaluate(&object_code(id, "Diameter", "%lu2%pr1"), &doc), "4.0");
        doc.remove(id);
        assert_eq!(evaluate(&code, &doc), UNKNOWN);
    }
}

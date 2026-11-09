//! Complex linetypes: the shapes and texts a linetype draws besides its
//! dashes. AutoCAD writes them in the LTYPE record after the element they
//! follow (49, then 74 its kind, 75 the shape's number, 340 the STYLE it's
//! from, 46 scale, 50 rotation in degrees, 44 and 45 offset, 9 a text's
//! string), and keeps each shape file as an unnamed STYLE record of flag 1.
//! The dxf crate reads and writes those groups only as separate lists, so
//! they're written into its file's text and read from the file's text.

use std::collections::HashMap;

use dxf::Drawing;
use oxidraft_geom::Point;
use oxidraft_model::{Embedded, EmbeddedContent, EmbeddedTurn, Linetype};

/// The shape files `linetypes` draw shapes from, each once, as unnamed
/// STYLE records (the dxf crate writes their flag 0; [`write_elements`]
/// makes it 1).
pub(crate) fn add_shape_styles(drawing: &mut Drawing, linetypes: &[Linetype]) {
    let mut files: Vec<&str> = Vec::new();
    for e in linetypes.iter().flat_map(|l| &l.embedded) {
        if let EmbeddedContent::Shape { file, .. } = &e.content
            && !files.iter().any(|f| f.eq_ignore_ascii_case(file))
        {
            files.push(file);
        }
    }
    for file in files {
        drawing.add_style(dxf::tables::Style {
            name: String::new(),
            primary_font_file_name: file.to_string(),
            big_font_file_name: String::new(),
            ..Default::default()
        });
    }
}

/// Text with its `\U+XXXX` escapes (as the dxf crate writes what isn't
/// ASCII, for versions before AutoCAD 2007) as their characters.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\\U+") {
        out.push_str(&rest[..at]);
        let code = rest.get(at + 3..at + 7).and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32);
        match code {
            Some(c) => {
                out.push(c);
                rest = &rest[at + 7..];
            }
            None => {
                out.push_str("\\U+");
                rest = &rest[at + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text with what isn't ASCII as `\U+XXXX` escapes, which every reader
/// takes, whatever the file's version.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match u32::from(c) {
            // Past four digits: as it is.
            code if c.is_ascii() || code > 0xFFFF => out.push(c),
            code => out.push_str(&format!("\\U+{code:04X}")),
        }
    }
    out
}

/// A DXF real as AutoCAD writes one: always with a decimal point.
fn real(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 { format!("{v:.1}") } else { format!("{v}") }
}

/// The 74 flags of an element: 1 rotation absolute, 2 a text, 4 a shape,
/// 8 upright.
fn flags(e: &Embedded) -> i16 {
    let kind = match e.content {
        EmbeddedContent::Shape { .. } => 4,
        EmbeddedContent::Text { .. } => 2,
    };
    kind | match e.turn {
        EmbeddedTurn::Relative => 0,
        EmbeddedTurn::Absolute => 1,
        EmbeddedTurn::Upright => 8,
    }
}

/// The STYLE records of a DXF file's text: (handle, name, font file).
fn styles(lines: &[&str]) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut in_style = false;
    for pair in lines.chunks(2) {
        let (code, value) = (pair[0].trim(), pair.get(1).map_or("", |v| v.trim_end_matches(['\r', '\n'])));
        match code {
            "0" => {
                in_style = value.trim() == "STYLE";
                if in_style {
                    out.push(Default::default());
                }
            }
            "5" if in_style => out.last_mut().unwrap().0 = value.trim().to_string(),
            "2" if in_style => out.last_mut().unwrap().1 = unescape(value),
            "3" if in_style => out.last_mut().unwrap().2 = unescape(value),
            _ => {}
        }
    }
    out
}

/// Writes each linetype's element types (74) after its dash lengths, and
/// its shapes and texts after the elements they follow, into the dxf
/// crate's file; shape files' STYLE records get their flag.
pub(crate) fn write_elements(dxf: &[u8], linetypes: &[Linetype]) -> Vec<u8> {
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let styles = styles(&lines);
    let handle_of = |e: &Embedded| -> String {
        let found = match &e.content {
            EmbeddedContent::Shape { file, .. } => {
                styles.iter().find(|s| s.1.trim().is_empty() && s.2.eq_ignore_ascii_case(file))
            }
            EmbeddedContent::Text { style, .. } => {
                let name = crate::style_to_dxf(style);
                styles.iter().find(|s| s.1.eq_ignore_ascii_case(&name))
            }
        };
        found
            .or_else(|| styles.iter().find(|s| s.1.eq_ignore_ascii_case("STANDARD")))
            .map_or_else(|| "0".into(), |s| s.0.clone())
    };
    let mut out = String::with_capacity(text.len() + 1024);
    let mut record = String::new();
    let mut linetype: Option<&Linetype> = None;
    let mut element = 0;
    let mut unnamed_style = false;
    for pair in lines.chunks(2) {
        let (code, value) = (pair[0].trim(), pair.get(1).map_or("", |v| v.trim()));
        let end = crate::eol(pair[0]);
        if code == "0" {
            record = value.to_string();
            (linetype, element, unnamed_style) = (None, 0, false);
        }
        match (record.as_str(), code) {
            ("LTYPE", "2") => {
                let name = unescape(value);
                linetype = linetypes.iter().find(|l| l.name.eq_ignore_ascii_case(&name));
            }
            ("LTYPE", "49") => {
                pair.iter().for_each(|l| out.push_str(l));
                let embedded = linetype.and_then(|l| l.embedded.iter().find(|e| e.element == element));
                element += 1;
                let Some(e) = embedded else {
                    out.push_str(&format!(" 74{end}0{end}"));
                    continue;
                };
                let number = match &e.content {
                    EmbeddedContent::Shape { number, .. } => *number as i16,
                    EmbeddedContent::Text { .. } => 0,
                };
                out.push_str(&format!(" 74{end}{}{end} 75{end}{number}{end}", flags(e)));
                out.push_str(&format!("340{end}{}{end}", handle_of(e)));
                for (code, v) in [(46, e.scale), (50, e.rotation), (44, e.offset.x), (45, e.offset.y)] {
                    out.push_str(&format!(" {code}{end}{}{end}", real(v)));
                }
                if let EmbeddedContent::Text { text, .. } = &e.content {
                    out.push_str(&format!("  9{end}{}{end}", escape(text)));
                }
                continue;
            }
            ("STYLE", "2") => unnamed_style = value.is_empty(),
            ("STYLE", "70") if unnamed_style => {
                out.push_str(&format!("{}1{end}", pair[0]));
                continue;
            }
            _ => {}
        }
        pair.iter().for_each(|l| out.push_str(l));
    }
    out.into_bytes()
}

/// An element's shape or text as read, before its STYLE is known.
struct RawElement {
    element: usize,
    flags: i16,
    number: i16,
    style: String,
    scale: f64,
    rotation: f64,
    offset: Point,
    text: String,
}

/// Each linetype's shapes and texts in a DXF file's text, by its name in
/// capitals.
pub(crate) fn read_elements(dxf: &[u8]) -> HashMap<String, Vec<Embedded>> {
    if dxf.starts_with(b"AutoCAD Binary DXF") {
        return HashMap::new();
    }
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut found: Vec<(String, Vec<RawElement>)> = Vec::new();
    let mut in_ltype = false;
    let mut element: Option<usize> = None;
    for pair in lines.chunks(2) {
        let (code, value) = (pair[0].trim(), pair.get(1).map_or("", |v| v.trim_end_matches(['\r', '\n'])));
        if code == "0" {
            in_ltype = value.trim() == "LTYPE";
            if in_ltype {
                found.push((String::new(), Vec::new()));
                element = None;
            }
            continue;
        }
        if !in_ltype {
            continue;
        }
        let Some((name, raw)) = found.last_mut() else { continue };
        let number = || value.trim().parse::<f64>().ok();
        let last = raw.last_mut().filter(|r| Some(r.element) == element);
        match (code, last) {
            ("2", _) => *name = unescape(value),
            ("49", _) => element = Some(element.map_or(0, |e| e + 1)),
            ("74", _) => {
                let flags = value.trim().parse::<i16>().unwrap_or(0);
                if let Some(element) = element.filter(|_| flags & 6 != 0) {
                    raw.push(RawElement {
                        element,
                        flags,
                        number: 0,
                        style: String::new(),
                        scale: 1.0,
                        rotation: 0.0,
                        offset: Point::ORIGIN,
                        text: String::new(),
                    });
                }
            }
            ("75", Some(r)) => r.number = value.trim().parse().unwrap_or(0),
            ("340", Some(r)) => r.style = value.trim().to_string(),
            ("46", Some(r)) => r.scale = number().unwrap_or(1.0),
            ("50", Some(r)) => r.rotation = number().unwrap_or(0.0),
            ("44", Some(r)) => r.offset.x = number().unwrap_or(0.0),
            ("45", Some(r)) => r.offset.y = number().unwrap_or(0.0),
            ("9", Some(r)) => r.text = unescape(value),
            _ => {}
        }
    }
    let styles = styles(&lines);
    let style = |handle: &str| styles.iter().find(|s| s.0.eq_ignore_ascii_case(handle));
    found
        .into_iter()
        .filter(|(name, raw)| !name.is_empty() && !raw.is_empty())
        .map(|(name, raw)| {
            let embedded = raw
                .into_iter()
                .map(|r| {
                    let content = if r.flags & 4 != 0 {
                        let file = style(&r.style).map(|s| s.2.clone()).unwrap_or_default();
                        EmbeddedContent::Shape { number: r.number as u16, file }
                    } else {
                        let style = style(&r.style).map_or_else(|| "STANDARD".into(), |s| s.1.clone());
                        EmbeddedContent::Text { text: r.text, style: crate::style_from_dxf(&style) }
                    };
                    let turn = if r.flags & 1 != 0 {
                        EmbeddedTurn::Absolute
                    } else if r.flags & 8 != 0 {
                        EmbeddedTurn::Upright
                    } else {
                        EmbeddedTurn::Relative
                    };
                    Embedded {
                        element: r.element,
                        content,
                        scale: r.scale,
                        rotation: r.rotation,
                        turn,
                        offset: r.offset,
                    }
                })
                .collect();
            (name.to_ascii_uppercase(), embedded)
        })
        .collect()
}

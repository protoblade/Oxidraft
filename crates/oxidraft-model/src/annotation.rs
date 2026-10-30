//! Text, dimensions and leaders: their layout in drawing units, and how
//! dimensions and leaders break down into lines, arrowheads and text.

use oxidraft_geom::{
    Aabb, EPSILON, Point, Segment,
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
    /// An SHX font's big font (Asian characters), empty for none.
    pub big_font: String,
    /// Text made in it is annotative: its height is the paper height.
    pub annotative: bool,
}

impl TextStyle {
    pub fn standard() -> Self {
        Self::new(STANDARD)
    }

    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            font: "arial.ttf".into(),
            height: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            big_font: String::new(),
            annotative: false,
        }
    }
}

/// A text's style and how its characters are drawn: as wide and slanted as
/// the style had them when the text was made (or as set since).
#[derive(Debug, Clone, PartialEq)]
pub struct TextLook {
    pub style: String,
    pub width_factor: f64,
    pub oblique: f64,
    /// MTEXT's line spacing, times the usual.
    pub line_spacing: f64,
    /// MTEXT's columns, when it has them.
    pub columns: Option<Columns>,
}

/// MTEXT in columns: how many, each one's width, the gutter between them,
/// and (static columns) each one's height; without heights the lines are
/// shared out evenly.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Columns {
    pub count: usize,
    pub width: f64,
    pub gutter: f64,
    pub heights: Vec<f64>,
}

impl Default for TextLook {
    fn default() -> Self {
        Self { style: STANDARD.into(), width_factor: 1.0, oblique: 0.0, line_spacing: 1.0, columns: None }
    }
}

impl TextLook {
    pub fn of(style: &TextStyle) -> Self {
        Self {
            style: style.name.clone(),
            width_factor: style.width_factor,
            oblique: style.oblique,
            ..Default::default()
        }
    }

    /// Laid out piece by piece (see `oxidraft_plot::rich`): text formatted
    /// inline, in columns or spaced otherwise than usual.
    pub fn needs_pieces(&self, content: &str) -> bool {
        is_formatted(content)
            || self.columns.as_ref().is_some_and(|c| c.count > 1)
            || (self.line_spacing - 1.0).abs() > 1e-9
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
    mtext_runs(s, 1.0).into_iter().map(|r| r.text).collect()
}

/// A stretch of MTEXT in one format (see [`mtext_runs`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub format: RunFormat,
    /// A stacked fraction (`\S1/2;`): its top, its bottom, and how it's
    /// stacked — `/` over a bar, `#` across a slash, `^` with no bar. Its
    /// text is the two with a `/` between.
    pub stack: Option<(String, String, char)>,
}

/// How a [`Run`] looks, as MTEXT's inline codes set it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunFormat {
    /// `\f` (a TrueType family) or `\F` (a font file); `None` for the
    /// style's font.
    pub font: Option<String>,
    pub bold: bool,
    pub italic: bool,
    /// `\H`: times the object's height.
    pub height: f64,
    /// `\C`: an AutoCAD color index; `None` for the object's color.
    pub color: Option<u8>,
    /// `\L`, `\O` and `\K`.
    pub underline: bool,
    pub overline: bool,
    pub strike: bool,
    /// `\W`: the width factor, `\Q`: the obliquing angle (degrees); `None`
    /// for the object's.
    pub width_factor: Option<f64>,
    pub oblique: Option<f64>,
    /// `\T`: the space between characters, as a multiple of the usual.
    pub tracking: f64,
    /// `\p`: the paragraph's first line's indent (from the left indent)
    /// and its left indent, in multiples of the text's height.
    pub indent: (f64, f64),
    /// `\p…r…;`: its right indent, in multiples of the text's height: its
    /// lines wrap that far in from the text's width.
    pub right_indent: f64,
    /// `\p…b…,a…;`: the space before and after the paragraph, in
    /// multiples of the text's height.
    pub spacing: (f64, f64),
    /// `\p…sm…;` or `\p…se…;`: the paragraph's own line spacing, a
    /// multiple of the usual (`m`) or exactly so many heights (`e`); `None`
    /// for the text's.
    pub line_spacing: Option<(char, f64)>,
    /// `\p…t…;`: its tab stops, in multiples of the text's height from the
    /// left, and how what follows one lines up with it: `l` starting at it,
    /// `c` centered on it, `r` ending at it, `d` its decimal point at it.
    pub tabs: Vec<(f64, char)>,
    /// `\pq…;`: how its lines align: `l`eft, `c`enter or `r`ight.
    pub align: char,
}

impl Default for RunFormat {
    fn default() -> Self {
        Self {
            font: None,
            bold: false,
            italic: false,
            height: 1.0,
            color: None,
            underline: false,
            overline: false,
            strike: false,
            width_factor: None,
            oblique: None,
            tracking: 1.0,
            indent: (0.0, 0.0),
            right_indent: 0.0,
            spacing: (0.0, 0.0),
            line_spacing: None,
            tabs: Vec::new(),
            align: 'l',
        }
    }
}

impl RunFormat {
    /// The font to look for: the family's bold or italic face when asked
    /// for (`Arial Bold`), as font files name them.
    pub fn font_name(&self) -> Option<String> {
        let f = self.font.as_ref()?;
        Some(match (self.bold, self.italic) {
            (true, true) => format!("{f} Bold Italic"),
            (true, false) => format!("{f} Bold"),
            (false, true) => format!("{f} Italic"),
            _ => f.clone(),
        })
    }
}

/// Whether MTEXT `s` changes font, height, color or lines inline, so it
/// can't be shown in one format.
pub fn is_formatted(s: &str) -> bool {
    ["\\f", "\\F", "\\H", "\\C", "\\L", "\\O", "\\K", "\\W", "\\Q", "\\T", "\\S", "\\p", "^I", "\t"]
        .iter()
        .any(|code| s.contains(code))
}

/// MTEXT's text in runs of one format each: fonts (`\f…;`, `\F…;`),
/// heights (`\H2.5;` absolute, `\H2x;` relative; `height` is the object's),
/// colors (`\C1;`), underline, overline and strikethrough (`\L`/`\l`,
/// `\O`/`\o`, `\K`/`\k`), braces keeping a change inside them;
/// `\P` is a newline, `\U+XXXX` a character and `\S1/2;` its text.
pub fn mtext_runs(s: &str, height: f64) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut format = RunFormat::default();
    let mut saved: Vec<RunFormat> = Vec::new();
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
    let push = |runs: &mut Vec<Run>, format: &RunFormat, text: &str| match runs.last_mut() {
        Some(last) if last.format == *format && last.stack.is_none() => last.text.push_str(text),
        _ => runs.push(Run { text: text.to_string(), format: format.clone(), stack: None }),
    };
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                saved.push(format.clone());
                continue;
            }
            '}' => {
                if let Some(f) = saved.pop() {
                    format = f;
                }
                continue;
            }
            '\\' => {}
            // `^I` is a tab.
            '^' if chars.peek() == Some(&'I') => {
                chars.next();
                push(&mut runs, &format, "\t");
                continue;
            }
            c => {
                push(&mut runs, &format, c.encode_utf8(&mut [0; 4]));
                continue;
            }
        }
        match chars.next() {
            Some('P') => push(&mut runs, &format, "\n"),
            Some('~') => push(&mut runs, &format, "\u{a0}"),
            Some(c @ ('\\' | '{' | '}')) => push(&mut runs, &format, c.encode_utf8(&mut [0; 4])),
            Some('U' | 'u') if chars.peek() == Some(&'+') => {
                chars.next();
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    push(&mut runs, &format, ch.encode_utf8(&mut [0; 4]));
                }
            }
            // A character in another code page: left out.
            Some('M' | 'm') if chars.peek() == Some(&'+') => {
                chars.by_ref().take(6).for_each(drop);
            }
            Some('S') => {
                let value = until_semicolon(&mut chars);
                match value.find(['/', '#', '^']) {
                    Some(at) => {
                        let kind = value[at..].chars().next().unwrap_or('/');
                        let (top, bottom) = (value[..at].to_string(), value[at + 1..].to_string());
                        let text = format!("{top}/{bottom}");
                        runs.push(Run { text, format: format.clone(), stack: Some((top, bottom, kind)) });
                    }
                    None => push(&mut runs, &format, &value),
                }
            }
            Some(code @ ('f' | 'F')) => {
                let value = until_semicolon(&mut chars);
                let mut parts = value.split('|');
                let name = parts.next().unwrap_or("").trim();
                format.font = (!name.is_empty()).then(|| name.to_string());
                format.bold = false;
                format.italic = false;
                if code == 'f' {
                    for flag in parts {
                        match flag.trim().split_at(flag.trim().len().min(1)) {
                            ("b", v) => format.bold = v == "1",
                            ("i", v) => format.italic = v == "1",
                            _ => {}
                        }
                    }
                }
            }
            Some('H') => {
                let value = until_semicolon(&mut chars);
                let (number, relative) = match value.strip_suffix(['x', 'X']) {
                    Some(n) => (n, true),
                    None => (value.as_str(), false),
                };
                if let Ok(v) = number.trim().parse::<f64>()
                    && v > 0.0
                {
                    format.height = if relative { format.height * v } else { v / height.max(1e-9) };
                }
            }
            Some('C') => {
                format.color = match until_semicolon(&mut chars).trim().parse::<u32>() {
                    Ok(n @ 1..=255) => Some(n as u8),
                    _ => None,
                };
            }
            Some(code @ ('W' | 'Q' | 'T')) => {
                let value = until_semicolon(&mut chars);
                let (number, relative) = match value.strip_suffix(['x', 'X']) {
                    Some(n) => (n, true),
                    None => (value.as_str(), false),
                };
                if let Ok(v) = number.trim().parse::<f64>() {
                    match code {
                        'W' if v > 0.0 => {
                            format.width_factor =
                                Some(if relative { format.width_factor.unwrap_or(1.0) * v } else { v });
                        }
                        'Q' => format.oblique = Some(v.clamp(-85.0, 85.0)),
                        // AutoCAD's tracking runs from 0.75 to 4.
                        'T' => format.tracking = (if relative { format.tracking * v } else { v }).clamp(0.75, 4.0),
                        _ => {}
                    }
                }
            }
            // Paragraphs: `i` the first line's indent, `l` the left one, `r`
            // the right one, `b` and `a` the space before and after it, `sm`
            // and `se` its line spacing (`s*` the text's), `t` the tab stops
            // (everything after it is stops too: numbers, centered `c`,
            // right-aligned `r` and decimal `d` ones), `q` how its lines
            // align.
            Some('p') => {
                let mut in_tabs = false;
                // A tab stop: `4`, `c4`, `r4` or `d4`.
                let stop = |item: &str| {
                    let kind = item
                        .chars()
                        .next()
                        .map(|c| c.to_ascii_lowercase())
                        .filter(|c| matches!(c, 'c' | 'r' | 'd'))
                        .unwrap_or('l');
                    item.trim_start_matches(['c', 'r', 'd', 'C', 'R', 'D']).parse::<f64>().ok().map(|v| (v, kind))
                };
                for item in until_semicolon(&mut chars).trim_start_matches('x').split(',') {
                    let item = item.trim();
                    if in_tabs {
                        format.tabs.extend(stop(item));
                        continue;
                    }
                    let (code, v) = item.split_at(item.len().min(1));
                    match (code, v.parse::<f64>()) {
                        ("i", Ok(v)) => format.indent.0 = v,
                        ("l", Ok(v)) => format.indent.1 = v,
                        ("r", Ok(v)) => format.right_indent = v,
                        ("b", Ok(v)) => format.spacing.0 = v,
                        ("a", Ok(v)) => format.spacing.1 = v,
                        ("s", _) => {
                            format.line_spacing = match v.split_at(v.len().min(1)) {
                                (kind @ ("m" | "e"), n) => {
                                    n.parse::<f64>().ok().map(|n| (kind.chars().next().unwrap_or('m'), n))
                                }
                                _ => None,
                            }
                        }
                        ("t", _) => {
                            format.tabs = stop(v).into_iter().collect();
                            in_tabs = true;
                        }
                        ("q", _) => format.align = v.chars().next().unwrap_or('l'),
                        _ => {}
                    }
                }
            }
            Some('A' | 'c') => {
                until_semicolon(&mut chars);
            }
            Some('L') => format.underline = true,
            Some('l') => format.underline = false,
            Some('O') => format.overline = true,
            Some('o') => format.overline = false,
            Some('K') => format.strike = true,
            Some('k') => format.strike = false,
            Some(other) => {
                push(&mut runs, &format, "\\");
                push(&mut runs, &format, other.encode_utf8(&mut [0; 4]));
            }
            None => push(&mut runs, &format, "\\"),
        }
    }
    runs
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

/// How wide a line of text is in a font, in multiples of the text's
/// height; `None` when it can't be measured.
pub type Measure = fn(font: &str, line: &str) -> Option<f64>;

static MEASURE: std::sync::RwLock<Option<Measure>> = std::sync::RwLock::new(None);
static STYLE_FONTS: std::sync::RwLock<Vec<(String, String)>> = std::sync::RwLock::new(Vec::new());

/// Measures text boxes in their fonts from now on (the app gives the
/// fonts' metrics); without it they're estimated.
pub fn set_measure(measure: Measure) {
    if let Ok(mut m) = MEASURE.write() {
        *m = Some(measure);
    }
}

/// The text styles' fonts that text boxes are measured in: the drawing
/// being worked on's.
pub fn set_style_fonts(styles: &[TextStyle]) {
    let fonts: Vec<(String, String)> = styles.iter().map(|s| (s.name.to_lowercase(), s.font.clone())).collect();
    if STYLE_FONTS.read().is_ok_and(|f| *f != fonts)
        && let Ok(mut f) = STYLE_FONTS.write()
    {
        *f = fonts;
    }
}

/// A line's width in multiples of the text height, in style `style`'s font.
fn line_width(style: &str, line: &str) -> f64 {
    if line.is_empty() {
        return CHAR_WIDTH;
    }
    let measure = MEASURE.read().ok().and_then(|m| *m);
    let measured = measure.and_then(|m| {
        let fonts = STYLE_FONTS.read().ok()?;
        let style = style.to_lowercase();
        let font = fonts.iter().find(|(s, _)| *s == style).map_or("", |(_, f)| f.as_str());
        m(font, line)
    });
    measured.unwrap_or(line.chars().count() as f64 * CHAR_WIDTH)
}

/// [`text_extent`] measured in style `style`'s font, lines wrapped
/// between words as the drawing area wraps them.
fn text_extent_in(content: &str, height: f64, wrap_width: f64, style: &str) -> (f64, f64) {
    let width = |s: &str| line_width(style, s) * height;
    let (mut lines, mut widest) = (0usize, 0.0f64);
    for paragraph in text_lines(content) {
        if wrap_width <= EPSILON {
            lines += 1;
            widest = widest.max(width(&paragraph));
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && width(&candidate) > wrap_width {
                lines += 1;
                widest = widest.max(width(&line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        lines += 1;
        widest = widest.max(width(&line));
    }
    let lines = lines.max(1);
    (widest, height + (lines - 1) as f64 * height * LINE_SPACING)
}

/// The space MTEXT's paragraphs put between them (`\pb…;`, `\pa…;`), in
/// multiples of its height.
pub fn paragraph_gaps(content: &str) -> f64 {
    if !content.contains("\\p") {
        return 0.0;
    }
    let runs = mtext_runs(content, 1.0);
    // Each paragraph's format: its first run's.
    let mut formats: Vec<&RunFormat> = Vec::new();
    let mut start = true;
    for r in &runs {
        for c in r.text.chars() {
            if start {
                formats.push(&r.format);
            }
            start = c == '\n';
        }
    }
    formats.windows(2).map(|w| w[0].spacing.1 + w[1].spacing.0).sum()
}

/// The text box's corners in world coordinates (counterclockwise from its
/// bottom-left before rotation), measured in its style's font.
pub fn text_box(
    position: Point,
    height: f64,
    rotation: f64,
    wrap_width: f64,
    attach: Attach,
    content: &str,
    look: &TextLook,
) -> [Point; 4] {
    // Wider characters: fewer to a wrapped line, each line wider.
    let wf = if look.width_factor > 0.0 { look.width_factor } else { 1.0 };
    let (w, h) = text_extent_in(content, height, wrap_width / wf, &look.style);
    let mut w = w * wf;
    let mut h = height + (h - height) * look.line_spacing + paragraph_gaps(content) * height;
    // In columns: as wide as all of them, as high as the tallest.
    if let Some(c) = look.columns.as_ref().filter(|c| c.count > 1) {
        w = c.width * c.count as f64 + c.gutter * (c.count - 1) as f64;
        h = c.heights.iter().copied().fold(height + (h - height) / c.count as f64, f64::max);
    }
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
    /// Dimensions made in it are annotative.
    pub annotative: bool,
}

pub const DEFAULT_DIM_STYLE: DimStyle = DimStyle {
    text_height: 2.5,
    arrow_size: 2.5,
    ext_offset: 0.625,
    ext_beyond: 1.25,
    gap: 0.625,
    decimals: 2,
    annotative: false,
};

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
    Geometry::Hatch {
        origin: Default::default(),
        loops: vec![loop_],
        pattern: HatchPattern::Solid,
        scale: 1.0,
        angle: 0.0,
    }
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
    if let Some(style) = extra.style {
        let plain = DimExtra { style: None, ..extra.clone() };
        return with_dim_style(style, || dimension_primitives(p1, p2, line, rotation, aligned, text, scale, &plain));
    }
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
    match extra.jog.map(|j| d1 + u * (j - d1).dot(u)).filter(|j| along(*j) > lo && along(*j) < hi) {
        // A zigzag across the line where it's jogged.
        Some(j) => {
            let (w, n) = (s.text_height * scale * 0.75, Point::ORIGIN.polar(s.text_height * scale * 0.75, r + 90.0));
            let zig = [j - u * w, j - u * (w * 0.3) + n, j + u * (w * 0.3) - n, j + u * w];
            out.push(Geometry::Line { start: d1 + u * lo, end: zig[0] });
            for k in 0..3 {
                out.push(Geometry::Line { start: zig[k], end: zig[k + 1] });
            }
            out.push(Geometry::Line { start: zig[3], end: d1 + u * hi });
        }
        None => out.push(Geometry::Line { start: d1 + u * lo, end: d1 + u * hi }),
    }
    let mut out = cut_breaks(out, &extra.breaks);
    let len = d1.distance(d2);
    if len > EPSILON {
        let w = (d2 - d1) * (1.0 / len);
        out.push(arrowhead(d1, w, s.arrow_size * scale));
        out.push(arrowhead(d2, -w, s.arrow_size * scale));
    }
    let angle = extra.text_angle.unwrap_or_else(|| readable(r));
    let up = Point::ORIGIN.polar(1.0, readable(r) + 90.0);
    let value = Geometry::Text {
        look: Default::default(),
        position: text_mid + up * (s.gap * scale),
        height: s.text_height * scale,
        rotation: angle,
        width: 0.0,
        attach: Attach::BottomCenter,
        content: dimension_text(text, measurement(p1, p2, rotation, aligned)),
    };
    if let Some(i) = &extra.inspection {
        out.extend(inspection_frame(&value, i, s.gap * scale));
    }
    out.push(value);
    out
}

/// DIMINSPECT's shape around a dimension's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectionShape {
    #[default]
    Round,
    Angular,
    None,
}

/// DIMINSPECT: how often a dimension is to be checked, in a frame around
/// its text: a label to its left, the inspection rate to its right.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inspection {
    pub shape: InspectionShape,
    pub label: String,
    pub rate: String,
}

/// The frame, label and rate around a dimension's `value` text (bottom
/// centered), `gap` clear of it.
fn inspection_frame(value: &Geometry, i: &Inspection, gap: f64) -> Vec<Geometry> {
    let Geometry::Text { position, height, rotation, content, .. } = value else { return Vec::new() };
    let width = |s: &str| {
        let [a, b, ..] = text_box(Point::ORIGIN, *height, 0.0, 0.0, Attach::BottomLeft, s, &TextLook::default());
        a.distance(b)
    };
    // Clear of the text on each side, a little more than above and below.
    let pad = gap + height * 0.3;
    let half = width(content) / 2.0 + pad;
    let section = |s: &str| if s.is_empty() { 0.0 } else { width(s) + 2.0 * pad };
    let (left, right) = (-half - section(&i.label), half + section(&i.rate));
    let (bottom, top) = (-gap, height + gap);
    let at = |x: f64, y: f64| (*position + Point::new(x, y)).rotated_about(*position, *rotation);
    let line = |a: (f64, f64), b: (f64, f64)| Geometry::Line { start: at(a.0, a.1), end: at(b.0, b.1) };
    let text = |x: f64, s: &str| Geometry::Text {
        look: Default::default(),
        position: at(x, 0.0),
        height: *height,
        rotation: *rotation,
        width: 0.0,
        attach: Attach::BottomCenter,
        content: s.to_string(),
    };
    let mut out = Vec::new();
    if !i.label.is_empty() {
        out.push(line((-half, bottom), (-half, top)));
        out.push(text((left - half) / 2.0, &i.label));
    }
    if !i.rate.is_empty() {
        out.push(line((half, bottom), (half, top)));
        out.push(text((right + half) / 2.0, &i.rate));
    }
    if i.shape == InspectionShape::None {
        return out;
    }
    out.push(line((left, bottom), (right, bottom)));
    out.push(line((left, top), (right, top)));
    let r = (top - bottom) / 2.0;
    let mid = bottom + r;
    match i.shape {
        InspectionShape::Round => {
            for (x, start) in [(left, 90.0), (right, -90.0)] {
                let (start_angle, end_angle) = (rotation + start, rotation + start + 180.0);
                out.push(Geometry::Arc { center: at(x, mid), radius: r, start_angle, end_angle });
            }
        }
        _ => {
            for (x, dx) in [(left, -r), (right, r)] {
                out.push(line((x, top), (x + dx, mid)));
                out.push(line((x + dx, mid), (x, bottom)));
            }
        }
    }
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
    /// A jog in its dimension line here (DIMJOGLINE).
    pub jog: Option<Point>,
    /// Its own style, overriding the drawing's (DIMOVERRIDE).
    pub style: Option<DimStyle>,
    /// Inspection data around its text (DIMINSPECT).
    pub inspection: Option<Inspection>,
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
            jog: self.jog.map(f),
            style: self.style,
            inspection: self.inspection.clone(),
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

/// A feature control frame's pieces (TOLERANCE), as AutoCAD writes them:
/// rows split by `^J`, boxes by `%%v`, and GD&T symbols as `{\Fgdt;x}`.
#[derive(Debug, Clone, PartialEq)]
enum FramePiece {
    Symbol(char),
    Text(String),
}

fn frame_cells(content: &str) -> Vec<Vec<Vec<FramePiece>>> {
    content
        .split("^J")
        .flat_map(|r| r.split('\n'))
        .map(|row| {
            row.split("%%v")
                .map(|cell| {
                    let mut pieces = Vec::new();
                    let mut rest = cell;
                    while !rest.is_empty() {
                        match rest.find("{\\Fgdt;") {
                            Some(0) => {
                                let body = &rest[7..];
                                let c = body.chars().next().unwrap_or(' ');
                                pieces.push(FramePiece::Symbol(c.to_ascii_lowercase()));
                                rest = body.find('}').map_or("", |i| &body[i + 1..]);
                            }
                            Some(i) => {
                                pieces.push(FramePiece::Text(rest[..i].to_string()));
                                rest = &rest[i..];
                            }
                            None => {
                                pieces.push(FramePiece::Text(rest.to_string()));
                                rest = "";
                            }
                        }
                    }
                    pieces
                })
                .collect()
        })
        .collect()
}

/// The GD&T characteristic symbols TOLERANCE offers: their font letter and name.
pub const GDT_SYMBOLS: &[(char, &str)] = &[
    ('j', "Position"),
    ('r', "Concentricity"),
    ('i', "Symmetry"),
    ('f', "Parallelism"),
    ('b', "Perpendicularity"),
    ('a', "Angularity"),
    ('g', "Cylindricity"),
    ('c', "Flatness"),
    ('e', "Circularity"),
    ('u', "Straightness"),
    ('d', "Profile of a surface"),
    ('k', "Profile of a line"),
    ('h', "Circular runout"),
    ('t', "Total runout"),
];

/// A GD&T symbol drawn about `c`, `k` its size (the text height).
fn gdt_symbol(sym: char, c: Point, k: f64) -> Vec<Geometry> {
    let p = |x: f64, y: f64| c + Point::new(x * k, y * k);
    let line = |a: Point, b: Point| Geometry::Line { start: a, end: b };
    let circle = |r: f64| Geometry::Circle { center: c, radius: r * k };
    let arrow = |from: Point, to: Point| {
        let d = to - from;
        vec![line(from, to), arrowhead(to, d * (-1.0 / d.length().max(1e-12)), 0.35 * k)]
    };
    match sym {
        'j' => vec![circle(0.5), line(p(-0.75, 0.0), p(0.75, 0.0)), line(p(0.0, -0.75), p(0.0, 0.75))],
        'r' => vec![circle(0.3), circle(0.6)],
        'i' => {
            vec![line(p(-0.4, 0.4), p(0.4, 0.4)), line(p(-0.7, 0.0), p(0.7, 0.0)), line(p(-0.4, -0.4), p(0.4, -0.4))]
        }
        'f' => vec![line(p(-0.6, -0.5), p(-0.1, 0.5)), line(p(0.1, -0.5), p(0.6, 0.5))],
        'b' => vec![line(p(-0.6, -0.5), p(0.6, -0.5)), line(p(0.0, -0.5), p(0.0, 0.6))],
        'a' => vec![line(p(-0.6, -0.5), p(0.6, -0.5)), line(p(-0.6, -0.5), p(0.5, 0.4))],
        'g' => vec![circle(0.4), line(p(-0.75, -0.6), p(-0.25, 0.6)), line(p(0.25, -0.6), p(0.75, 0.6))],
        'c' => {
            let pts = [p(-0.6, -0.35), p(0.35, -0.35), p(0.6, 0.35), p(-0.35, 0.35)];
            (0..4).map(|i| line(pts[i], pts[(i + 1) % 4])).collect()
        }
        'e' => vec![circle(0.55)],
        'u' => vec![line(p(-0.7, 0.0), p(0.7, 0.0))],
        'd' | 'k' => {
            let mut out =
                vec![Geometry::Arc { center: p(0.0, -0.35), radius: 0.6 * k, start_angle: 0.0, end_angle: 180.0 }];
            if sym == 'd' {
                out.push(line(p(-0.6, -0.35), p(0.6, -0.35)));
            }
            out
        }
        'h' => arrow(p(-0.4, -0.6), p(0.4, 0.6)),
        't' => {
            let mut out = arrow(p(-0.6, -0.6), p(0.0, 0.6));
            out.extend(arrow(p(0.0, -0.6), p(0.6, 0.6)));
            out.push(line(p(-0.6, -0.6), p(0.0, -0.6)));
            out
        }
        _ => Vec::new(),
    }
}

/// A feature control frame's boxes, symbols and text, its first row's
/// left middle at `position`.
pub fn tolerance_primitives(position: Point, height: f64, rotation: f64, content: &str) -> Vec<Geometry> {
    let k = height;
    let row_h = 2.0 * k;
    let mut out = Vec::new();
    let text_at = |at: Point, s: &str, attach: Attach| Geometry::Text {
        look: Default::default(),
        position: at,
        height: k,
        rotation: 0.0,
        width: 0.0,
        attach,
        content: s.to_string(),
    };
    let text = |at: Point, s: &str| text_at(at, s, Attach::MiddleLeft);
    for (r, row) in frame_cells(content).iter().enumerate() {
        let y = -(r as f64) * row_h;
        let mut x = 0.0;
        for cell in row.iter().filter(|c| !c.is_empty()) {
            // A characteristic symbol alone: a square box; else as wide as its contents.
            let alone = matches!(cell.as_slice(), [FramePiece::Symbol(c)] if GDT_SYMBOLS.iter().any(|s| s.0 == *c));
            let mut inner = Vec::new();
            let mut w = 0.4 * k;
            for piece in cell {
                match piece {
                    FramePiece::Symbol(c) if alone => inner.extend(gdt_symbol(*c, Point::new(row_h / 2.0, y), k)),
                    // Ø, and material conditions circled.
                    FramePiece::Symbol('n') => {
                        inner.push(text(Point::new(x + w, y), "Ø"));
                        w += text_extent("Ø", k, 0.0).0 * 1.25 + 0.15 * k;
                    }
                    FramePiece::Symbol(c @ ('m' | 'l' | 's' | 'p')) => {
                        let letter = c.to_ascii_uppercase().to_string();
                        let center = Point::new(x + w + 0.25 * k + 0.6 * k, y);
                        inner.push(Geometry::Circle { center, radius: 0.6 * k });
                        inner.push(text_at(center, &letter, Attach::MiddleCenter));
                        w += 1.6 * k;
                    }
                    FramePiece::Symbol(_) => {}
                    FramePiece::Text(t) => {
                        // Room as the drawing area's type sets it, which runs wider.
                        inner.push(text(Point::new(x + w, y), t));
                        w += text_extent(t, k, 0.0).0 * 1.25;
                    }
                }
            }
            let width = if alone { row_h } else { (w + 0.4 * k).max(row_h) };
            let corners = [
                Point::new(x, y - k),
                Point::new(x + width, y - k),
                Point::new(x + width, y + k),
                Point::new(x, y + k),
            ];
            out.push(Geometry::Polyline { vertices: corners.iter().map(|p| Vertex::at(*p)).collect(), closed: true });
            out.extend(inner);
            x += width;
        }
    }
    // Placed and turned.
    out.into_iter().map(|g| g.translated(position).rotated(position, rotation)).collect()
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
pub fn leader_primitives(points: &[Point], content: &str, scale: f64, blocks: bool) -> Vec<Geometry> {
    let s = &dim_style();
    let ls = leader_style();
    let [first, second, ..] = points else { return Vec::new() };
    let (last, prev) = (points[points.len() - 1], points[points.len() - 2]);
    let side = if last.x >= prev.x { 1.0 } else { -1.0 };
    let has_text = !content.trim().is_empty() && !blocks;
    let mut path: Vec<Vertex> = points.iter().copied().map(Vertex::at).collect();
    let landing = if ls.landing_on { ls.landing } else { 0.0 };
    let land_end = last + Point::new(side * landing * scale, 0.0);
    if (has_text || blocks) && landing > 0.0 {
        path.push(Vertex::at(land_end));
    }
    let mut out = vec![Geometry::Polyline { vertices: path, closed: false }];
    let d = *second - *first;
    if d.length() > EPSILON {
        out.extend(leader_arrow(ls.arrowhead, *first, d * (1.0 / d.length()), ls.arrow_size * scale));
    }
    if has_text {
        out.push(Geometry::Text {
            look: Default::default(),
            position: land_end + Point::new(side * s.gap * scale, 0.0),
            height: ls.text_height * scale,
            rotation: 0.0,
            width: 0.0,
            attach: if side > 0.0 { Attach::MiddleLeft } else { Attach::MiddleRight },
            content: content.to_string(),
        });
    }
    out
}

/// One block at a multileader's end (its content Block, as MLEADERSTYLE
/// sets): its name and its attribute's value.
#[derive(Debug, Clone, PartialEq)]
pub struct LeaderBlock {
    pub name: String,
    pub value: String,
}

/// A multileader's blocks: one, or several gathered by MLEADERCOLLECT,
/// side by side from the landing, `per_row` to a row.
#[derive(Debug, Clone, PartialEq)]
pub struct LeaderBlocks {
    pub items: Vec<LeaderBlock>,
    pub per_row: usize,
}

impl LeaderBlocks {
    pub fn one(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { items: vec![LeaderBlock { name: name.into(), value: value.into() }], per_row: 1 }
    }
}

/// Where each of a multileader's blocks goes: its extents' middle on the
/// landing, the next beside it (away from the leader), rows downwards.
/// `extents` gives a block's extents in its own coordinates.
pub fn leader_block_places(
    points: &[Point],
    blocks: &LeaderBlocks,
    scale: f64,
    extents: impl Fn(&str) -> Option<Aabb>,
) -> Vec<(usize, crate::Placement)> {
    let [.., prev, last] = points else { return Vec::new() };
    let ls = leader_style();
    let side = if last.x >= prev.x { 1.0 } else { -1.0 };
    let landing = if ls.landing_on { ls.landing } else { 0.0 };
    let land_end = *last + Point::new(side * landing * scale, 0.0);
    let unit = Aabb::from_points(Point::new(-0.5, -0.5), Point::new(0.5, 0.5));
    let boxes: Vec<Aabb> = blocks.items.iter().map(|b| extents(&b.name).unwrap_or(unit)).collect();
    let w = boxes.iter().map(|b| b.max.x - b.min.x).fold(0.0, f64::max) * scale;
    let h = boxes.iter().map(|b| b.max.y - b.min.y).fold(0.0, f64::max) * scale;
    let per_row = blocks.per_row.max(1);
    boxes
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let (row, column) = (i / per_row, i % per_row);
            let middle = land_end + Point::new(side * w * (column as f64 + 0.5), -h * row as f64);
            (i, crate::Placement { position: middle - b.center() * scale, scale, rotation: 0.0, mirror: false })
        })
        .collect()
}

/// MLEADERSTYLE's ready-made blocks for Block content: a tag number in a
/// shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagShape {
    Circle,
    Box,
    Hexagon,
    Triangle,
    Slot,
}

impl TagShape {
    pub const ALL: [TagShape; 5] =
        [TagShape::Circle, TagShape::Box, TagShape::Hexagon, TagShape::Triangle, TagShape::Slot];

    /// Its block's name, as AutoCAD's.
    pub fn block_name(self) -> &'static str {
        match self {
            TagShape::Circle => "_TagCircle",
            TagShape::Box => "_TagBox",
            TagShape::Hexagon => "_TagHexagon",
            TagShape::Triangle => "_TagTriangle",
            TagShape::Slot => "_TagSlot",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TagShape::Circle => "Circle",
            TagShape::Box => "Box",
            TagShape::Hexagon => "Hexagon",
            TagShape::Triangle => "Triangle",
            TagShape::Slot => "Slot",
        }
    }

    pub fn from_block_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.block_name().eq_ignore_ascii_case(name))
    }

    /// The shape around the origin, sized for 2.5 high text.
    pub fn outline(self) -> Geometry {
        let polygon = |n: usize, r: f64, turn: f64| Geometry::Polyline {
            vertices: (0..n)
                .map(|i| {
                    Vertex::at(Point::new(r, 0.0).rotated_about(Point::ORIGIN, turn + 360.0 * i as f64 / n as f64))
                })
                .collect(),
            closed: true,
        };
        match self {
            TagShape::Circle => Geometry::Circle { center: Point::ORIGIN, radius: 3.0 },
            TagShape::Box => polygon(4, 3.0 * 2f64.sqrt(), 45.0),
            TagShape::Hexagon => polygon(6, 3.5, 0.0),
            // Its middle a little low, so the number sits in its wide part.
            TagShape::Triangle => polygon(3, 6.0, 90.0).translated(Point::new(0.0, 1.0)),
            TagShape::Slot => Geometry::Polyline {
                vertices: vec![
                    Vertex::new(Point::new(-3.0, -2.5), 0.0),
                    Vertex::new(Point::new(3.0, -2.5), 1.0),
                    Vertex::new(Point::new(3.0, 2.5), 0.0),
                    Vertex::new(Point::new(-3.0, 2.5), 1.0),
                ],
                closed: true,
            },
        }
    }
}

/// MLEADEREDIT's added leaders: from each arrowhead in `more` straight to
/// where the leader's own lines end.
pub fn more_leaders(points: &[Point], more: &[Point], scale: f64) -> Vec<Geometry> {
    let Some(end) = points.last() else { return Vec::new() };
    let ls = leader_style();
    let mut out = Vec::new();
    for tip in more {
        let d = *end - *tip;
        if d.length() <= EPSILON {
            continue;
        }
        out.push(Geometry::Line { start: *tip, end: *end });
        out.extend(leader_arrow(ls.arrowhead, *tip, d * (1.0 / d.length()), ls.arrow_size * scale));
    }
    out
}

/// A leader's arrowhead (MLEADERSTYLE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeaderArrow {
    #[default]
    ClosedFilled,
    Open,
    Dot,
    None,
}

impl LeaderArrow {
    pub const ALL: [LeaderArrow; 4] =
        [LeaderArrow::ClosedFilled, LeaderArrow::Open, LeaderArrow::Dot, LeaderArrow::None];

    pub fn name(self) -> &'static str {
        match self {
            LeaderArrow::ClosedFilled => "Closed filled",
            LeaderArrow::Open => "Open",
            LeaderArrow::Dot => "Dot",
            LeaderArrow::None => "None",
        }
    }

    pub fn code(self) -> f64 {
        self as u8 as f64
    }

    pub fn from_code(v: f64) -> Self {
        Self::ALL.get(v.max(0.0) as usize).copied().unwrap_or_default()
    }
}

/// How multileaders look (MLEADERSTYLE): the arrowhead and its size, the
/// text height, and the landing (dogleg) to the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeaderStyle {
    pub arrowhead: LeaderArrow,
    pub arrow_size: f64,
    pub text_height: f64,
    pub landing_on: bool,
    pub landing: f64,
    /// Content: a tag block, or (none) multiline text.
    pub block: Option<TagShape>,
    /// Multileaders made in it are annotative.
    pub annotative: bool,
}

pub const DEFAULT_LEADER_STYLE: LeaderStyle = LeaderStyle {
    arrowhead: LeaderArrow::ClosedFilled,
    arrow_size: 2.5,
    text_height: 2.5,
    landing_on: true,
    landing: 2.5,
    block: None,
    annotative: false,
};

impl Default for LeaderStyle {
    fn default() -> Self {
        DEFAULT_LEADER_STYLE
    }
}

thread_local! {
    /// The leader style leaders are drawn in: the drawing's, while it's drawn.
    static LEADER: std::cell::Cell<LeaderStyle> = const { std::cell::Cell::new(DEFAULT_LEADER_STYLE) };
}

pub fn leader_style() -> LeaderStyle {
    LEADER.get()
}

pub fn set_leader_style(style: LeaderStyle) {
    LEADER.set(style);
}

/// Runs `f` with `style` as the leader style.
pub fn with_leader_style<T>(style: LeaderStyle, f: impl FnOnce() -> T) -> T {
    let before = LEADER.replace(style);
    let out = f();
    LEADER.set(before);
    out
}

/// A leader's arrowhead at `tip`, pointing back along `into`.
fn leader_arrow(kind: LeaderArrow, tip: Point, into: Point, size: f64) -> Vec<Geometry> {
    match kind {
        LeaderArrow::ClosedFilled => vec![arrowhead(tip, into, size)],
        LeaderArrow::Open => {
            let side = Point::new(-into.y, into.x) * (size / 3.0);
            let base = tip + into * size;
            vec![Geometry::Line { start: base + side, end: tip }, Geometry::Line { start: tip, end: base - side }]
        }
        // A filled disc: a closed polyline as wide as its diameter.
        LeaderArrow::Dot => {
            let r = size / 4.0;
            let ring =
                [Vertex::new(tip + Point::new(r / 2.0, 0.0), 1.0), Vertex::new(tip - Point::new(r / 2.0, 0.0), 1.0)];
            vec![Geometry::Polyline { vertices: ring.iter().map(|v| v.with_widths(r, r)).collect(), closed: true }]
        }
        LeaderArrow::None => Vec::new(),
    }
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
    fn mtext_runs_keep_each_format() {
        let runs = mtext_runs("Plain {\\fArial|b1|i0;\\H2x;\\C1;Big} \\H5;\\Lunder\\l end", 2.5);
        let texts: Vec<&str> = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["Plain ", "Big", " ", "under", " end"]);
        let big = &runs[1].format;
        assert_eq!((big.font.as_deref(), big.bold, big.height, big.color), (Some("Arial"), true, 2.0, Some(1)));
        assert_eq!(big.font_name().as_deref(), Some("Arial Bold"));
        assert_eq!(runs[2].format, RunFormat::default(), "the brace ended the change");
        assert!((runs[3].format.height - 2.0).abs() < 1e-9, "5 is twice 2.5");
        assert!(runs[3].format.underline && !runs[4].format.underline);
        assert!(is_formatted("{\\fArial;x}") && !is_formatted("a\\Pb"));
        let runs = mtext_runs("a{\\W0.5;\\Q15;\\T2;b}c", 1.0);
        let f = &runs[1].format;
        assert_eq!((f.width_factor, f.oblique, f.tracking), (Some(0.5), Some(15.0), 2.0));
        assert_eq!(runs[2].format, RunFormat::default());
        // Paragraphs: indents, spacing, line spacing and tab stops.
        let f = &mtext_runs("\\pxi-1,l2,r1,b0.5,a1,sm1.5,qc,t2,c4,r6,D8;x", 1.0)[0].format;
        assert_eq!(
            (f.indent, f.right_indent, f.spacing, f.line_spacing),
            ((-1.0, 2.0), 1.0, (0.5, 1.0), Some(('m', 1.5)))
        );
        assert_eq!(f.tabs, [(2.0, 'l'), (4.0, 'c'), (6.0, 'r'), (8.0, 'd')]);
        assert_eq!(f.align, 'c');
        assert_eq!(mtext_runs("\\pxse2;x", 1.0)[0].format.line_spacing, Some(('e', 2.0)));
        assert_eq!(paragraph_gaps("{\\pxa1;a}\\P{\\pxb2;b}\\Pc"), 3.0);
    }

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
        let prims = leader_primitives(&[Point::ORIGIN, Point::new(10.0, 10.0)], "NOTE", 1.0, false);
        let Some(Geometry::Text { attach, position, .. }) = prims.last() else { panic!() };
        assert_eq!(*attach, Attach::MiddleLeft);
        assert!(position.x > 10.0);
        let prims = leader_primitives(&[Point::ORIGIN, Point::new(-10.0, 10.0)], "NOTE", 1.0, false);
        assert!(matches!(prims.last(), Some(Geometry::Text { attach: Attach::MiddleRight, .. })));
    }
}

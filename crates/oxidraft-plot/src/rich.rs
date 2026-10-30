//! MTEXT that changes its look inline (fonts, heights, colors, width
//! factor, obliquing, tracking, underline and the like), laid out piece by
//! piece in the drawing's coordinates, so the drawing area and plots put
//! every piece in the same place. Pieces in an SHX font are drawn as its
//! strokes, the rest as text.

use oxidraft_geom::Point;
use oxidraft_model::Geometry;
use oxidraft_model::annotation::{LINE_SPACING, RunFormat, mtext_runs};

use crate::font::{self, Typeface};
use crate::shx::{self, ShxFont};

/// One piece of a line in one format.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub text: String,
    /// The start of its baseline, in the drawing.
    pub at: Point,
    /// Degrees, counterclockwise.
    pub rotation: f64,
    /// How high its capitals stand, in the drawing.
    pub height: f64,
    /// Its font: a TrueType face's name or an SHX file.
    pub font: String,
    /// The style's big font, for an SHX piece.
    pub big_font: String,
    /// An AutoCAD color index; `None` for the object's color.
    pub color: Option<u8>,
    pub width_factor: f64,
    pub oblique: f64,
    /// How long it runs along the baseline, in the drawing.
    pub width: f64,
    pub underline: bool,
    pub overline: bool,
    pub strike: bool,
}

impl Piece {
    pub fn shx(&self) -> Option<&'static ShxFont> {
        shx::shx_font(&self.font)
    }

    /// Its strokes, when its font is an SHX one.
    pub fn strokes(&self) -> Vec<Vec<Point>> {
        let Some(font) = self.shx() else { return Vec::new() };
        let big = shx::shx_font(&self.big_font);
        let slant = self.oblique.to_radians().tan();
        font.line_with(big, &self.text)
            .into_iter()
            .map(|s| s.into_iter().map(|p| self.place(p.x * self.width_factor + p.y * slant, p.y)).collect())
            .collect()
    }

    /// A point `x` along and `y` up from the start of its baseline, in
    /// multiples of its height.
    pub fn place(&self, x: f64, y: f64) -> Point {
        self.at + (Point::new(x, y) * self.height).rotated_about(Point::ORIGIN, self.rotation)
    }

    /// The lines it's underlined, overlined and struck through with.
    pub fn rules(&self) -> Vec<(Point, Point)> {
        let along = self.width / self.height;
        [(self.underline, -0.2), (self.overline, 1.15), (self.strike, 0.45)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, y)| (self.place(0.0, y), self.place(along, y)))
            .collect()
    }
}

/// The font a run shows in: its bold or italic face, its family, or the
/// style's font; and whether that's an SHX font.
fn run_font(f: &RunFormat, style_font: &str) -> String {
    let found = |n: &String| shx::shx_font(n).is_some() || !font::typeface(n).is_default();
    f.font_name().filter(found).or_else(|| f.font.clone().filter(found)).unwrap_or_else(|| style_font.to_string())
}

/// The space tracking adds after each character: AutoCAD's 2 opens the
/// gaps between letters by about a quarter of their height.
fn gap(tracking: f64, height: f64) -> f64 {
    (tracking - 1.0) * 0.25 * height
}

/// How wide `text` is in `font`, in multiples of the height.
fn text_width(font_name: &str, big: Option<&ShxFont>, text: &str) -> f64 {
    match shx::shx_font(font_name) {
        Some(s) => s.width_with(big, text),
        None => font::typeface(font_name).width(text) / 0.7,
    }
}

/// Text `g` (formatted MTEXT) as pieces; `style_font` and `big_font` are
/// its style's.
pub fn formatted_pieces(g: &Geometry, style_font: &str, big_font: &str) -> Vec<Piece> {
    let Geometry::Text { position, height, rotation, width, attach, content, look } = g else { return Vec::new() };
    let wf = if look.width_factor > 0.0 { look.width_factor } else { 1.0 };
    let big = shx::shx_font(big_font);
    let style_shx = shx::shx_font(style_font);
    let style_face: &Typeface = font::typeface(style_font);
    let runs = mtext_runs(content, *height);
    let fonts: Vec<String> = runs.iter().map(|r| run_font(&r.format, style_font)).collect();
    // Each character with its run, in the order plain_text has them.
    let chars: Vec<(char, usize)> =
        runs.iter().enumerate().flat_map(|(i, r)| r.text.chars().map(move |c| (c, i))).collect();
    let wrap = if *width > 0.0 { width / (height * wf) } else { 0.0 };
    let lines = font::wrap_lines(content, wrap, |s| text_width(style_font, big, s));
    // A line's pieces, matched to the characters in order.
    let mut k = 0;
    let lines: Vec<Vec<(String, usize)>> = lines
        .iter()
        .map(|line| {
            let mut out: Vec<(String, usize)> = Vec::new();
            for c in line.chars() {
                while k < chars.len() && chars[k].0 != c {
                    k += 1;
                }
                let run = chars.get(k).map_or(0, |x| x.1);
                k += 1;
                match out.last_mut() {
                    Some((text, r)) if *r == run => text.push(c),
                    _ => out.push((c.to_string(), run)),
                }
            }
            out
        })
        .collect();
    let run_wf = |r: usize| runs[r].format.width_factor.unwrap_or(1.0) * wf;
    // A piece's width in the drawing, its characters spaced by its tracking.
    let piece_width = |text: &str, r: usize| {
        let f = &runs[r].format;
        let n = text.chars().count() as f64;
        text_width(&fonts[r], big, text) * height * f.height * run_wf(r) + n * gap(f.tracking, height * f.height)
    };
    let line_width = |line: &[(String, usize)]| line.iter().map(|(t, r)| piece_width(t, *r)).sum::<f64>();
    let tallest = |line: &[(String, usize)]| line.iter().map(|(_, r)| runs[*r].format.height).fold(1.0, f64::max);
    let widest = lines.iter().map(|l| line_width(l)).fold(0.0, f64::max);
    // Baselines below the top: a tall line pushes those under it down.
    let step = if style_shx.is_some() { LINE_SPACING } else { style_face.row_height() / 0.7 };
    let mut baselines = Vec::with_capacity(lines.len());
    let mut y = height * lines.first().map_or(1.0, |l| tallest(l));
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            y += height * step * tallest(line);
        }
        baselines.push(y);
    }
    let span = baselines.last().copied().unwrap_or(*height);
    let (fx, fy) = attach.fractions();
    let mut out = Vec::new();
    for (line, base) in lines.iter().zip(&baselines) {
        let mut x = -fx * widest;
        for (text, r) in line {
            let f = &runs[*r].format;
            let w = piece_width(text, *r);
            let start = *position + Point::new(x, fy * span - base).rotated_about(Point::ORIGIN, *rotation);
            let piece = Piece {
                text: text.clone(),
                at: start,
                rotation: *rotation,
                height: height * f.height,
                font: fonts[*r].clone(),
                big_font: big_font.to_string(),
                color: f.color,
                width_factor: run_wf(*r),
                oblique: f.oblique.unwrap_or(look.oblique),
                width: w,
                underline: f.underline,
                overline: f.overline,
                strike: f.strike,
            };
            if (f.tracking - 1.0).abs() < 1e-9 {
                out.push(piece);
            } else {
                // Tracked: each character on its own, spaced out.
                let mut cx = 0.0;
                for c in text.chars() {
                    let one = c.to_string();
                    let cw = text_width(&piece.font, big, &one) * piece.height * piece.width_factor
                        + gap(f.tracking, piece.height);
                    let at = start + Point::new(cx, 0.0).rotated_about(Point::ORIGIN, *rotation);
                    let rules = c != ' ';
                    out.push(Piece {
                        text: one,
                        at,
                        width: cw,
                        underline: piece.underline && rules,
                        overline: piece.overline && rules,
                        strike: piece.strike && rules,
                        ..piece.clone()
                    });
                    cx += cw;
                }
            }
            x += w;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_model::Attach;

    fn text(content: &str) -> Geometry {
        Geometry::Text {
            look: Default::default(),
            position: Point::new(0.0, 0.0),
            height: 2.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: content.into(),
        }
    }

    #[test]
    fn pieces_follow_width_factor_obliquing_and_tracking() {
        let narrow = formatted_pieces(&text("AB{\\W0.5;CD}"), "", "");
        assert_eq!(narrow.len(), 2);
        let (full, half) = (formatted_pieces(&text("{\\H1x;CD}"), "", "")[0].width, narrow[1].width);
        assert!((half - full / 2.0).abs() < 1e-9, "{half} {full}");
        assert!(narrow[1].at.x > narrow[0].at.x && narrow[1].at.y == 0.0);
        let slanted = formatted_pieces(&text("{\\Q20;X}"), "", "");
        assert_eq!(slanted[0].oblique, 20.0);
        // Tracked: one piece a character, spread wider than untracked.
        let tracked = formatted_pieces(&text("{\\T2;ABC}"), "", "");
        assert_eq!(tracked.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["A", "B", "C"]);
        let untracked = formatted_pieces(&text("{\\T1;ABC}\\L"), "", "");
        assert!(tracked[2].at.x > untracked[0].width * 0.6, "{tracked:?}");
    }
}

//! The typeface plotted text is set in: the one the drawing area shows
//! text in (egui's Ubuntu Light), so a plot reads like the screen.

use std::sync::LazyLock;

use ttf_parser::{Face, GlyphId};

pub struct Typeface {
    face: Face<'static>,
    upem: f64,
}

pub static FONT: LazyLock<Typeface> = LazyLock::new(|| {
    let face = Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0).expect("the bundled typeface parses");
    Typeface { upem: f64::from(face.units_per_em()), face }
});

impl Typeface {
    /// The font file, to embed.
    pub fn data(&self) -> &'static [u8] {
        epaint_default_fonts::UBUNTU_LIGHT
    }

    pub fn name(&self) -> &'static str {
        "Ubuntu-Light"
    }

    /// The glyph for `c` (0, the missing-glyph box, if there's none).
    pub fn glyph(&self, c: char) -> u16 {
        self.face.glyph_index(c).map_or(0, |g| g.0)
    }

    /// How far a glyph advances the pen, in ems.
    pub fn advance(&self, glyph: u16) -> f64 {
        self.face.glyph_hor_advance(GlyphId(glyph)).map_or(0.0, |a| f64::from(a) / self.upem)
    }

    /// The width of a line of text, in ems.
    pub fn width(&self, text: &str) -> f64 {
        text.chars().map(|c| self.advance(self.glyph(c))).sum()
    }

    /// Above the baseline, in ems (where egui puts the first baseline).
    pub fn ascent(&self) -> f64 {
        f64::from(self.face.ascender()) / self.upem
    }

    /// Below the baseline, in ems (negative).
    pub fn descent(&self) -> f64 {
        f64::from(self.face.descender()) / self.upem
    }

    /// From one baseline to the next, in ems, as egui spaces rows.
    pub fn row_height(&self) -> f64 {
        self.ascent() - self.descent() + f64::from(self.face.line_gap()) / self.upem
    }

    pub fn cap_height(&self) -> f64 {
        self.face.capital_height().map_or(0.7, |h| f64::from(h) / self.upem)
    }

    /// The bounding box of every glyph, in ems: min x, min y, max x, max y.
    pub fn bbox(&self) -> [f64; 4] {
        let b = self.face.global_bounding_box();
        [b.x_min, b.y_min, b.x_max, b.y_max].map(|v| f64::from(v) / self.upem)
    }
}

/// Text broken into lines: at paragraph marks, and where a line would run
/// past `wrap` ems (0 for no wrapping), between words.
pub fn lines(content: &str, wrap: f64) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in oxidraft_model::annotation::plain_text(content).split('\n') {
        if wrap <= 0.0 {
            out.push(paragraph.to_string());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && FONT.width(&candidate) > wrap {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_and_wrapping() {
        assert!(FONT.glyph('A') != 0 && FONT.glyph('ł') != 0, "Latin Extended is covered");
        assert!((0.6..0.8).contains(&FONT.cap_height()));
        assert!(FONT.row_height() > FONT.ascent());
        assert_eq!(lines("A\\PB C", 0.0), vec!["A", "B C"]);
        let w = FONT.width("WALL WALL");
        assert_eq!(lines("WALL WALL WALL", w + 0.01), vec!["WALL WALL", "WALL"]);
    }
}

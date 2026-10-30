//! AutoCAD's shape fonts (SHX): read from the font files AutoCAD keeps
//! (shapes, unifont and big font files), each character's shape bytes
//! drawn as the strokes AutoCAD draws. Text in a style whose font is an
//! SHX file found here is drawn and plotted in these strokes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use oxidraft_geom::{Point, curve::ArcSeg};

/// One character: its strokes, and how far it moves the pen, with capital
/// letters 1 high.
#[derive(Debug, Default, PartialEq)]
pub struct Glyph {
    pub strokes: Vec<Vec<Point>>,
    pub advance: f64,
}

pub struct ShxFont {
    /// How high capitals stand, in shape units: the scale of the font.
    above: f64,
    /// Subshapes are numbered with two bytes.
    unicode: bool,
    shapes: HashMap<u32, Vec<u8>>,
    glyphs: Mutex<HashMap<char, Arc<Glyph>>>,
    /// A big font's lead-byte ranges, and the code pages its two-byte
    /// codes may be in, likeliest first.
    ranges: Vec<(u16, u16)>,
    encodings: Vec<&'static encoding_rs::Encoding>,
}

impl std::fmt::Debug for ShxFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ShxFont({} shapes)", self.shapes.len())
    }
}

fn u16_le(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

fn u32_le(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*d.get(at)?, *d.get(at + 1)?, *d.get(at + 2)?, *d.get(at + 3)?]))
}

/// A shape's bytes after its name.
fn after_name(def: &[u8]) -> &[u8] {
    def.iter().position(|b| *b == 0).map_or(def, |i| &def[i + 1..])
}

/// The 16 directions of a vector byte, as steps along its longer axis.
const DIRECTIONS: [(f64, f64); 16] = [
    (1.0, 0.0),
    (1.0, 0.5),
    (1.0, 1.0),
    (0.5, 1.0),
    (0.0, 1.0),
    (-0.5, 1.0),
    (-1.0, 1.0),
    (-1.0, 0.5),
    (-1.0, 0.0),
    (-1.0, -0.5),
    (-1.0, -1.0),
    (-0.5, -1.0),
    (0.0, -1.0),
    (0.5, -1.0),
    (1.0, -1.0),
    (1.0, -0.5),
];

/// The pen a shape draws with.
struct Pen {
    at: Point,
    down: bool,
    scale: f64,
    stack: Vec<Point>,
    strokes: Vec<Vec<Point>>,
    current: Vec<Point>,
}

impl Pen {
    fn lift(&mut self) {
        if self.current.len() >= 2 {
            self.strokes.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    fn to(&mut self, p: Point) {
        if self.down {
            if self.current.is_empty() {
                self.current.push(self.at);
            }
            self.current.push(p);
        }
        self.at = p;
    }

    fn go(&mut self, dx: f64, dy: f64) {
        self.to(self.at + Point::new(dx, dy) * self.scale);
    }

    fn arc(&mut self, arc: &ArcSeg) {
        let n = ((arc.sweep.abs() / 10.0).ceil() as usize).max(2);
        for i in 1..=n {
            self.to(arc.point_at(i as f64 / n as f64));
        }
    }

    /// The arc from here round `radius`: `spec`'s high bit for clockwise,
    /// then the octant it starts at and how many it spans (0: all eight).
    /// A fractional arc (`offsets`, in 256ths of an octant) starts that far
    /// into its first octant and ends that far into its last (0: all of
    /// it), as ezdxf reads them.
    fn octant_arc(&mut self, radius: f64, spec: u8, offsets: Option<(f64, f64)>) {
        let ccw = spec & 0x80 == 0;
        let start_oct = f64::from((spec >> 4) & 7);
        let count = match spec & 7 {
            0 => 8.0,
            n => f64::from(n),
        };
        let sign = if ccw { 1.0 } else { -1.0 };
        let (a0, a1) = match offsets {
            None => (start_oct * 45.0, start_oct * 45.0 + sign * count * 45.0),
            Some((start_off, end_off)) => {
                let end_off = if end_off == 0.0 { 256.0 } else { end_off };
                let end_oct = start_oct + sign * (count - 1.0);
                (start_oct * 45.0 + sign * start_off * 45.0 / 256.0, end_oct * 45.0 + sign * end_off * 45.0 / 256.0)
            }
        };
        let r = radius * self.scale;
        let center = self.at - Point::new(r * a0.to_radians().cos(), r * a0.to_radians().sin());
        let mut sweep = a1 - a0;
        if ccw && sweep <= 0.0 {
            sweep += 360.0;
        } else if !ccw && sweep >= 0.0 {
            sweep -= 360.0;
        }
        self.arc(&ArcSeg { center, radius: r, start: a0, sweep });
    }

    fn bulge_to(&mut self, dx: i8, dy: i8, bulge: i8) {
        let to = self.at + Point::new(f64::from(dx), f64::from(dy)) * self.scale;
        match ArcSeg::from_bulge(self.at, to, f64::from(bulge) / 127.0) {
            Some(arc) => self.arc(&arc),
            None => self.to(to),
        }
    }
}

impl ShxFont {
    /// Reads an SHX file's bytes.
    pub fn parse(data: &[u8]) -> Option<ShxFont> {
        let head_end = data.iter().position(|b| *b == 0x1A)?;
        let head = String::from_utf8_lossy(&data[..head_end]).to_ascii_lowercase();
        let at = head_end + 1;
        let mut shapes = HashMap::new();
        let mut ranges = Vec::new();
        let unicode = head.contains("unifont");
        if unicode {
            let count = u32_le(data, at)? as usize;
            let info_len = u16_le(data, at + 4)? as usize;
            shapes.insert(0, data.get(at + 6..at + 6 + info_len)?.to_vec());
            let mut p = at + 6 + info_len;
            for _ in 1..count {
                let (Some(code), Some(len)) = (u16_le(data, p), u16_le(data, p + 2)) else { break };
                let Some(def) = data.get(p + 4..p + 4 + len as usize) else { break };
                shapes.insert(u32::from(code), def.to_vec());
                p += 4 + len as usize;
            }
        } else if head.contains("bigfont") {
            let count = u16_le(data, at + 2)? as usize;
            let range_count = u16_le(data, at + 4)? as usize;
            for r in 0..range_count {
                ranges.extend(u16_le(data, at + 6 + 4 * r).zip(u16_le(data, at + 8 + 4 * r)));
            }
            let mut p = at + 6 + 4 * range_count;
            for _ in 0..count {
                let (Some(code), Some(len), Some(offset)) = (u16_le(data, p), u16_le(data, p + 2), u32_le(data, p + 4))
                else {
                    break;
                };
                if let Some(def) = data.get(offset as usize..offset as usize + len as usize) {
                    shapes.insert(u32::from(code), def.to_vec());
                }
                p += 8;
            }
        } else if head.contains("shapes") {
            let count = u16_le(data, at + 4)? as usize;
            let mut p = at + 6 + 4 * count;
            for i in 0..count {
                let (Some(code), Some(len)) = (u16_le(data, at + 6 + 4 * i), u16_le(data, at + 8 + 4 * i)) else {
                    break;
                };
                let Some(def) = data.get(p..p + len as usize) else { break };
                shapes.insert(u32::from(code), def.to_vec());
                p += len as usize;
            }
        } else {
            return None;
        }
        // The font's own shape: its name, then how high capitals stand.
        let info = after_name(shapes.get(&0)?);
        let above = f64::from(*info.first()?);
        let encodings = if ranges.is_empty() { Vec::new() } else { big_encodings(&ranges, "") };
        (above > 0.0).then(|| ShxFont { above, unicode, shapes, glyphs: Mutex::default(), ranges, encodings })
    }

    /// Runs shape `number`'s bytes with `pen`.
    fn run(&self, number: u32, pen: &mut Pen, depth: usize) {
        let Some(def) = self.shapes.get(&number) else { return };
        if depth > 8 {
            return;
        }
        let bytes = after_name(def);
        let mut i = 0;
        let byte = |i: usize| bytes.get(i).copied().unwrap_or(0);
        let signed = |i: usize| byte(i) as i8;
        while i < bytes.len() {
            let b = bytes[i];
            i += 1;
            if b >= 0x10 {
                let (dx, dy) = DIRECTIONS[usize::from(b & 0x0F)];
                let len = f64::from(b >> 4);
                pen.go(dx * len, dy * len);
                continue;
            }
            match b {
                0 => break,
                1 => pen.down = true,
                2 => {
                    pen.lift();
                    pen.down = false;
                }
                3 => {
                    pen.scale /= f64::from(byte(i).max(1));
                    i += 1;
                }
                4 => {
                    pen.scale *= f64::from(byte(i).max(1));
                    i += 1;
                }
                5 => pen.stack.push(pen.at),
                6 => {
                    pen.lift();
                    if let Some(p) = pen.stack.pop() {
                        pen.at = p;
                    }
                }
                7 => {
                    let sub = if self.unicode {
                        i += 2;
                        u32::from(u16::from_be_bytes([byte(i - 2), byte(i - 1)]))
                    } else if byte(i) == 0 {
                        // A big font's: two bytes, then where and how big.
                        i += 7;
                        u32::from(u16::from_be_bytes([byte(i - 6), byte(i - 5)]))
                    } else {
                        i += 1;
                        u32::from(byte(i - 1))
                    };
                    self.run(sub, pen, depth + 1);
                }
                8 => {
                    pen.go(f64::from(signed(i)), f64::from(signed(i + 1)));
                    i += 2;
                }
                9 => {
                    while i + 1 < bytes.len() && (byte(i), byte(i + 1)) != (0, 0) {
                        pen.go(f64::from(signed(i)), f64::from(signed(i + 1)));
                        i += 2;
                    }
                    i += 2;
                }
                10 => {
                    pen.octant_arc(f64::from(byte(i)), byte(i + 1), None);
                    i += 2;
                }
                11 => {
                    let radius = f64::from(u16::from_be_bytes([byte(i + 2), byte(i + 3)]));
                    pen.octant_arc(radius, byte(i + 4), Some((f64::from(byte(i)), f64::from(byte(i + 1)))));
                    i += 5;
                }
                12 => {
                    pen.bulge_to(signed(i), signed(i + 1), signed(i + 2));
                    i += 3;
                }
                13 => {
                    while i + 1 < bytes.len() && (byte(i), byte(i + 1)) != (0, 0) {
                        pen.bulge_to(signed(i), signed(i + 1), signed(i + 2));
                        i += 3;
                    }
                    i += 2;
                }
                // Only for vertical text: the next command is skipped.
                14 => i += command_len(bytes, i, self.unicode),
                _ => {}
            }
        }
    }

    fn shape_for(&self, c: char) -> Option<u32> {
        // A big font's characters by their code in its code page.
        if !self.encodings.is_empty() {
            let mut buf = [0; 4];
            let text = c.encode_utf8(&mut buf);
            return self.encodings.iter().find_map(|enc| {
                let (bytes, _, unmappable) = enc.encode(text);
                let code = match *bytes {
                    [lead, trail] if !unmappable => u32::from(u16::from_be_bytes([lead, trail])),
                    [single] if !unmappable && single >= 0x80 => u32::from(single),
                    _ => return None,
                };
                self.shapes.contains_key(&code).then_some(code)
            });
        }
        let code = u32::from(c);
        if self.shapes.contains_key(&code) && code != 0 {
            return Some(code);
        }
        // AutoCAD's own numbers for %%d, %%p and %%c.
        let special = match c {
            '°' => 127,
            '±' => 128,
            'Ø' | 'ø' | '⌀' => 129,
            _ => return None,
        };
        self.shapes.contains_key(&special).then_some(special)
    }

    /// Character `c`'s strokes (a '?' for one the font lacks).
    pub fn glyph(&self, c: char) -> Arc<Glyph> {
        if let Some(g) = self.glyphs.lock().ok().and_then(|m| m.get(&c).cloned()) {
            return g;
        }
        let glyph = match self.shape_for(c).or_else(|| self.shape_for('?')) {
            Some(number) => {
                let mut pen = Pen {
                    at: Point::ORIGIN,
                    down: true,
                    scale: 1.0,
                    stack: Vec::new(),
                    strokes: Vec::new(),
                    current: Vec::new(),
                };
                self.run(number, &mut pen, 0);
                pen.lift();
                let k = 1.0 / self.above;
                Glyph {
                    strokes: pen.strokes.into_iter().map(|s| s.into_iter().map(|p| p * k).collect()).collect(),
                    advance: pen.at.x * k,
                }
            }
            None => Glyph { strokes: Vec::new(), advance: 0.7 },
        };
        let glyph = Arc::new(glyph);
        if let Ok(mut m) = self.glyphs.lock() {
            m.insert(c, glyph.clone());
        }
        glyph
    }

    /// A line of text's width, capitals 1 high.
    pub fn width(&self, text: &str) -> f64 {
        self.width_with(None, text)
    }

    /// Whether the font has a shape for `c`.
    pub fn has(&self, c: char) -> bool {
        self.shape_for(c).is_some()
    }

    /// Character `c` from this font, or from big font `big` when this one
    /// lacks it, as AutoCAD combines a style's two fonts.
    fn glyph_with(&self, big: Option<&ShxFont>, c: char) -> Arc<Glyph> {
        match big {
            Some(b) if !self.has(c) && b.has(c) => b.glyph(c),
            _ => self.glyph(c),
        }
    }

    /// [`ShxFont::width`] with a big font.
    pub fn width_with(&self, big: Option<&ShxFont>, text: &str) -> f64 {
        text.chars().map(|c| self.glyph_with(big, c).advance).sum()
    }

    /// A line of text's strokes from the start of its baseline, capitals 1
    /// high.
    pub fn line(&self, text: &str) -> Vec<Vec<Point>> {
        self.line_with(None, text)
    }

    /// [`ShxFont::line`] with a big font.
    pub fn line_with(&self, big: Option<&ShxFont>, text: &str) -> Vec<Vec<Point>> {
        let mut x = 0.0;
        let mut out = Vec::new();
        for c in text.chars() {
            let g = self.glyph_with(big, c);
            out.extend(g.strokes.iter().map(|s| s.iter().map(|p| *p + Point::new(x, 0.0)).collect()));
            x += g.advance;
        }
        out
    }
}

/// A text's strokes in the drawing, in SHX font `font`: capitals as high
/// as the text, lines spaced as MTEXT's, the block placed by its
/// attachment as the drawing area places text, stretched by its width
/// factor and slanted by its obliquing.
pub fn text_strokes(font: &ShxFont, big: Option<&ShxFont>, g: &oxidraft_model::Geometry) -> Vec<Vec<Point>> {
    let oxidraft_model::Geometry::Text { position, height, rotation, width, attach, content, look } = g else {
        return Vec::new();
    };
    let wf = if look.width_factor > 0.0 { look.width_factor } else { 1.0 };
    let wrap = if *width > 0.0 { width / (height * wf) } else { 0.0 };
    let lines = crate::font::wrap_lines(content, wrap, |s| font.width_with(big, s));
    let widest = lines.iter().map(|l| font.width_with(big, l)).fold(0.0, f64::max) * wf;
    let row = oxidraft_model::annotation::LINE_SPACING;
    let span = 1.0 + (lines.len() as f64 - 1.0) * row;
    let (fx, fy) = attach.fractions();
    let slant = look.oblique.to_radians().tan();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let origin = Point::new(-fx * widest, -1.0 + fy * span - i as f64 * row);
        for stroke in font.line_with(big, line) {
            let placed = stroke
                .iter()
                .map(|p| {
                    let local = (origin + Point::new(p.x * wf + p.y * slant, p.y)) * *height;
                    *position + local.rotated_about(Point::ORIGIN, *rotation)
                })
                .collect();
            out.push(placed);
        }
    }
    out
}

/// How many bytes the command at `i` takes (for code 14's skip).
fn command_len(bytes: &[u8], i: usize, unicode: bool) -> usize {
    let byte = |k: usize| bytes.get(k).copied().unwrap_or(0);
    let pairs_until_zero = |step: usize| {
        let mut k = i + 1;
        while k + 1 < bytes.len() && (byte(k), byte(k + 1)) != (0, 0) {
            k += step;
        }
        k + 2 - i
    };
    match byte(i) {
        b if b >= 0x10 => 1,
        3 | 4 => 2,
        7 if unicode => 3,
        7 if byte(i + 1) == 0 => 8,
        7 => 2,
        8 | 10 => 3,
        9 => pairs_until_zero(2),
        11 => 6,
        12 => 4,
        13 => pairs_until_zero(3),
        _ => 1,
    }
}

/// Fonts read so far, by name (lower case).
static FOUND: LazyLock<Mutex<HashMap<String, Option<&'static ShxFont>>>> = LazyLock::new(Default::default);

/// The SHX font a style names, if it's one and it's found; one that isn't
/// is drawn in simplex.shx when that is (as AutoCAD's FONTALT does).
pub fn shx_font(font: &str) -> Option<&'static ShxFont> {
    let font = &crate::font::mapped(font);
    let key = Path::new(font.trim()).file_name()?.to_string_lossy().to_ascii_lowercase();
    if !key.ends_with(".shx") {
        return None;
    }
    if let Some(found) = FOUND.lock().ok().and_then(|f| f.get(&key).copied()) {
        return found;
    }
    let read = |name: &str| {
        let data = std::fs::read(crate::font::find(name)?).ok()?;
        let mut font = ShxFont::parse(&data)?;
        if !font.ranges.is_empty() {
            font.encodings = big_encodings(&font.ranges, name);
        }
        Some(&*Box::leak(Box::new(font)))
    };
    let alt = font_alt();
    let loaded = read(&key).or_else(|| if key == alt { None } else { shx_font(&alt) });
    if let Ok(mut found) = FOUND.lock() {
        found.insert(key, loaded);
    }
    loaded
}

/// The code pages a big font's two-byte codes may be in, likeliest
/// first: from its file name when it's one of AutoCAD's (extfont and
/// bigfont Japanese, gbcbig Simplified Chinese, chineset Traditional,
/// whgtxt Korean), then from its lead bytes (Shift-JIS's stop at 0x9F).
fn big_encodings(ranges: &[(u16, u16)], name: &str) -> Vec<&'static encoding_rs::Encoding> {
    use encoding_rs::{BIG5, EUC_KR, GBK, SHIFT_JIS};
    let n = name.to_ascii_lowercase();
    let named = if n.contains("gb") || n.starts_with("hz") || n.contains("chs") {
        Some(GBK)
    } else if n.starts_with("whg") || n.contains("kor") {
        Some(EUC_KR)
    } else if n.contains("chineset") || n.contains("big5") || n.contains("cht") {
        Some(BIG5)
    } else if n.contains("extfont") || n.contains("bigfont") || n.contains("jis") || n.contains("jpn") {
        Some(SHIFT_JIS)
    } else {
        None
    };
    let shift_jis = ranges.iter().any(|(a, b)| *a <= 0x81 && (0x9F..0xA1).contains(b));
    let order = if shift_jis { [SHIFT_JIS, GBK, BIG5, EUC_KR] } else { [GBK, BIG5, EUC_KR, SHIFT_JIS] };
    let mut out: Vec<_> = named.into_iter().collect();
    out.extend(order.into_iter().filter(|e| Some(*e) != named));
    out
}

/// FONTALT: the SHX font a missing one is drawn in.
static FONT_ALT: Mutex<String> = Mutex::new(String::new());

pub fn font_alt() -> String {
    FONT_ALT.lock().ok().map(|a| a.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| "simplex.shx".into())
}

/// Sets FONTALT (empty: simplex.shx); fonts are looked for again.
pub fn set_font_alt(name: &str) {
    let mut name = name.trim().to_ascii_lowercase();
    if !name.is_empty() && !name.ends_with(".shx") {
        name.push_str(".shx");
    }
    if let Ok(mut a) = FONT_ALT.lock() {
        *a = name;
    }
    forget_all();
}

/// Forgets every font found (FONTMAP changed).
pub(crate) fn forget_all() {
    if let Ok(mut found) = FOUND.lock() {
        found.clear();
    }
}

/// Forgets fonts not found, to look for them again (a new folder to look in).
pub(crate) fn forget_missing() {
    if let Ok(mut found) = FOUND.lock() {
        found.retain(|_, f| f.is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shapes file: "A" as two strokes up and down with a bar, "B" a
    /// half circle by an octant arc, and a font shape of 10 above.
    /// A big font with Shift-JIS lead bytes and one character, あ (0x82A0):
    /// a stroke 10 to the right.
    fn big_sample() -> Vec<u8> {
        let shapes: [(u16, Vec<u8>); 2] =
            [(0, [b"BIG\0".as_slice(), &[10, 2, 2, 10, 0]].concat()), (0x82A0, vec![0, 0xA0, 0])];
        let ranges: [(u16, u16); 2] = [(0x81, 0x9F), (0xE0, 0xFC)];
        let mut out = b"AutoCAD-86 bigfont 1.0\r\n\x1A".to_vec();
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&(shapes.len() as u16).to_le_bytes());
        out.extend_from_slice(&(ranges.len() as u16).to_le_bytes());
        for (a, b) in ranges {
            out.extend_from_slice(&a.to_le_bytes());
            out.extend_from_slice(&b.to_le_bytes());
        }
        let mut offset = out.len() + 8 * shapes.len();
        for (n, def) in &shapes {
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&(def.len() as u16).to_le_bytes());
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += def.len();
        }
        for (_, def) in &shapes {
            out.extend_from_slice(def);
        }
        out
    }

    #[test]
    fn a_big_font_draws_what_the_main_font_lacks() {
        let main = ShxFont::parse(&sample()).unwrap();
        let big = ShxFont::parse(&big_sample()).unwrap();
        assert_eq!(big.encodings[0], encoding_rs::SHIFT_JIS, "from its lead bytes");
        assert!(big.has('あ') && !main.has('あ') && !big.has('A'));
        assert!((main.width_with(Some(&big), "Aあ") - 2.4).abs() < 1e-9);
        assert!(main.line("Aあ").len() == 2, "alone it has nothing for あ");
        let strokes = main.line_with(Some(&big), "Aあ");
        assert_eq!(strokes.len(), 3);
        assert!(strokes[2][0].approx_eq(Point::new(1.4, 0.0)) && strokes[2][1].approx_eq(Point::new(2.4, 0.0)));
        assert_eq!(big_encodings(&[(0xA1, 0xFE)], "gbcbig.shx")[0], encoding_rs::GBK);
        assert_eq!(big_encodings(&[(0xA1, 0xFE)], "whgtxt.shx")[0], encoding_rs::EUC_KR);
    }

    fn sample() -> Vec<u8> {
        let shapes: [(u16, Vec<u8>); 3] = [
            (0, [b"TEST\0".as_slice(), &[10, 2, 2, 0]].concat()),
            // Up 10 slanting, down 10, then the bar; pen up to the next.
            (
                65,
                [
                    b"A\0".as_slice(),
                    &[
                        0xA3, /* (5,10) */
                        0xAD, /* (5,-10) */
                        2,
                        8,
                        (-8i8) as u8,
                        4,
                        1,
                        0x60,
                        2,
                        8,
                        6,
                        (-4i8) as u8,
                        0,
                    ],
                ]
                .concat(),
            ),
            // A half circle of radius 5 counterclockwise from the bottom.
            (66, [b"B\0".as_slice(), &[10, 5, 0x64 /* from octant 6, 4 octants */, 2, 8, 6, 0, 0]].concat()),
        ];
        let mut out = b"AutoCAD-86 shapes 1.0\r\n\x1A".to_vec();
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&66u16.to_le_bytes());
        out.extend_from_slice(&(shapes.len() as u16).to_le_bytes());
        for (n, def) in &shapes {
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&(def.len() as u16).to_le_bytes());
        }
        for (_, def) in &shapes {
            out.extend_from_slice(def);
        }
        out
    }

    #[test]
    fn shapes_draw_their_strokes() {
        let font = ShxFont::parse(&sample()).unwrap();
        let a = font.glyph('A');
        assert_eq!(a.strokes.len(), 2, "{a:?}");
        let tip = a.strokes[0][1];
        assert!(tip.approx_eq(Point::new(0.5, 1.0)), "{tip:?}");
        assert!(a.strokes[1][0].approx_eq(Point::new(0.2, 0.4)) && a.strokes[1][1].approx_eq(Point::new(0.8, 0.4)));
        assert!((a.advance - 1.4).abs() < 1e-9, "{}", a.advance);
        // The half circle: from its bottom round to its top, rightwards.
        let b = font.glyph('B');
        let arc = &b.strokes[0];
        assert!(arc.last().unwrap().approx_eq(Point::new(0.0, 1.0)), "{arc:?}");
        assert!(arc.iter().any(|p| p.approx_eq(Point::new(0.5, 0.5))), "{arc:?}");
        // A character it lacks shows as nothing it has: no '?' here either.
        assert!(font.glyph('Z').strokes.is_empty());
        assert_eq!(font.line("AB").len(), 3);
        assert!((font.width("AA") - 2.8).abs() < 1e-9);
    }

    /// AutoCAD's own fonts, where AutoCAD is installed: text in them is
    /// drawn and plotted as their strokes.
    #[test]
    fn autocads_fonts_where_there_are_some() {
        let Some(txt) = shx_font("txt.shx") else { return };
        let a = txt.glyph('A');
        assert!(!a.strokes.is_empty() && a.advance > 0.5 && a.advance < 1.5, "{a:?}");
        let top = a.strokes.iter().flatten().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((top - 1.0).abs() < 0.05, "capitals stand 1 high: {top}");
        use oxidraft_model::{Attach, Document, Geometry, TextLook, TextStyle};
        let mut doc = Document::new();
        let mut style = TextStyle::new("Notes");
        style.font = "txt.shx".into();
        doc.set_text_style(style).unwrap();
        let text = Geometry::Text {
            position: Point::new(10.0, 10.0),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "A".into(),
            look: TextLook { style: "Notes".into(), ..Default::default() },
        };
        let strokes = text_strokes(txt, None, &text);
        let top = strokes.iter().flatten().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((top - 12.5).abs() < 0.1, "{top}");
        doc.add(text);
        let settings = crate::PlotSettings { area: crate::PlotArea::Extents, ..Default::default() };
        let sheet = crate::render(&doc, oxidraft_model::MODEL, &settings).unwrap();
        assert!(sheet.items.iter().all(|i| !matches!(i, crate::Item::Text { .. })));
        assert!(sheet.items.iter().any(|i| matches!(i, crate::Item::Stroke { .. })));
    }
}

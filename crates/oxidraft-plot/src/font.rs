//! Typefaces for text: the font a text style names (a TrueType or
//! OpenType file, found among the system's fonts or beside the drawing),
//! or else the one the drawing area shows text in by default (egui's
//! Ubuntu Light). The screen and plots use the same ones.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use ttf_parser::{Face, GlyphId};

pub struct Typeface {
    data: &'static [u8],
    face: Face<'static>,
    upem: f64,
    /// Its PostScript name, for PDF.
    name: String,
    /// The style's font it was found for (empty: the default typeface).
    key: String,
}

impl std::fmt::Debug for Typeface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Typeface({})", self.name)
    }
}

impl Typeface {
    fn parse(data: &'static [u8], key: &str) -> Option<Self> {
        let face = Face::parse(data, 0).ok()?;
        let name = face
            .names()
            .into_iter()
            .filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
            .find_map(|n| n.to_string())
            .unwrap_or_else(|| key.to_string());
        // A PDF name: no spaces or delimiters.
        let name: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        Some(Typeface { upem: f64::from(face.units_per_em()), face, data, name, key: key.into() })
    }

    /// The font file, to embed.
    pub fn data(&self) -> &'static [u8] {
        self.data
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its family name (as Windows names fonts).
    pub fn family(&self) -> String {
        self.face
            .names()
            .into_iter()
            .filter(|n| n.name_id == ttf_parser::name_id::FAMILY)
            .find_map(|n| n.to_string())
            .unwrap_or_else(|| "Arial".into())
    }

    /// The style font it stands for; empty for the default typeface.
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn is_default(&self) -> bool {
        self.key.is_empty()
    }

    /// Whether its outlines are CFF (an OpenType font PDF embeds as such)
    /// rather than TrueType.
    pub fn is_cff(&self) -> bool {
        self.face.tables().cff.is_some()
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

pub static FONT: LazyLock<Typeface> =
    LazyLock::new(|| Typeface::parse(epaint_default_fonts::UBUNTU_LIGHT, "").expect("the bundled typeface parses"));

/// Windows' font files and what other systems call them, for drawings
/// made there.
const ALSO: &[(&str, &[&str])] = &[
    ("arial.ttf", &["Arial.ttf", "LiberationSans-Regular.ttf", "DejaVuSans.ttf"]),
    ("arialbd.ttf", &["Arial Bold.ttf", "LiberationSans-Bold.ttf"]),
    ("arialn.ttf", &["Arial Narrow.ttf", "LiberationSansNarrow-Regular.ttf"]),
    ("times.ttf", &["Times New Roman.ttf", "LiberationSerif-Regular.ttf", "DejaVuSerif.ttf"]),
    ("cour.ttf", &["Courier New.ttf", "LiberationMono-Regular.ttf", "DejaVuSansMono.ttf"]),
    ("verdana.ttf", &["Verdana.ttf", "DejaVuSans.ttf"]),
    ("tahoma.ttf", &["Tahoma.ttf", "DejaVuSans.ttf"]),
    ("georgia.ttf", &["Georgia.ttf"]),
    ("calibri.ttf", &["Carlito-Regular.ttf"]),
    ("consola.ttf", &["Menlo.ttc", "DejaVuSansMono.ttf"]),
    ("segoeui.ttf", &["SF-Pro.ttf", "DejaVuSans.ttf"]),
];

/// Folders to look in besides the system's (the drawing's own).
static EXTRA: LazyLock<Mutex<Vec<PathBuf>>> = LazyLock::new(Default::default);

/// Fonts found so far, by the name asked for (lower case).
static FOUND: LazyLock<Mutex<HashMap<String, Option<&'static Typeface>>>> = LazyLock::new(Default::default);

/// Also looks for fonts in `dir` (a drawing's folder) from now on: fonts
/// not found before are looked for again.
pub fn add_search_dir(dir: &Path) {
    if let Ok(mut extra) = EXTRA.lock()
        && !extra.iter().any(|d| d == dir)
    {
        extra.push(dir.to_path_buf());
        if let Ok(mut found) = FOUND.lock() {
            found.retain(|_, f| f.is_some());
        }
        crate::shx::forget_missing();
    }
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = EXTRA.lock().map(|e| e.clone()).unwrap_or_default();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    // AutoCAD's own fonts, SHX ones too, where it's installed.
    let autocads = |root: &Path| -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
        entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("AutoCAD")).map(|e| e.path()).collect()
    };
    if cfg!(target_os = "macos") {
        dirs.extend(
            ["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"].map(PathBuf::from),
        );
        dirs.extend(home.iter().map(|h| h.join("Library/Fonts")));
        for acad in autocads(Path::new("/Applications/Autodesk")) {
            let app =
                acad.join(format!("{}.app/Contents/Resources", acad.file_name().unwrap_or_default().to_string_lossy()));
            dirs.extend([app.join("Fonts"), app.join("Support")]);
        }
    } else if cfg!(windows) {
        let windir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        dirs.push(windir.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
        let programs =
            std::env::var_os("ProgramFiles").map_or_else(|| PathBuf::from("C:\\Program Files"), PathBuf::from);
        dirs.extend(autocads(&programs.join("Autodesk")).into_iter().map(|a| a.join("Fonts")));
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        dirs.extend(home.iter().flat_map(|h| [h.join(".fonts"), h.join(".local/share/fonts")]));
    }
    dirs
}

/// The file named `name` (any case) in `dir` or the folders under it.
fn find_in(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    if let Some(e) = entries.iter().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)) {
        return Some(e.path());
    }
    if depth == 0 {
        return None;
    }
    entries.iter().filter(|e| e.path().is_dir()).find_map(|e| find_in(&e.path(), name, depth - 1))
}

/// The file for a style's font: its own name, else what other systems
/// call it; a family name may leave its extension off.
pub fn find(font: &str) -> Option<PathBuf> {
    let font = font.trim();
    let file = Path::new(font).file_name()?.to_string_lossy().into_owned();
    let lower = file.to_ascii_lowercase();
    let mut names = vec![file.clone()];
    if !lower.ends_with(".ttf") && !lower.ends_with(".otf") && !lower.ends_with(".ttc") {
        names.extend(["ttf", "otf", "ttc"].map(|ext| format!("{file}.{ext}")));
    }
    if let Some((_, others)) = ALSO.iter().find(|(n, _)| *n == lower) {
        names.extend(others.iter().map(|s| s.to_string()));
    }
    // A full path, as some drawings name it.
    if Path::new(font).is_absolute() && Path::new(font).is_file() {
        return Some(PathBuf::from(font));
    }
    let dirs = font_dirs();
    names.iter().find_map(|n| dirs.iter().find_map(|d| find_in(d, n, 2)))
}

/// One font of a TrueType collection as a font file of its own.
fn from_collection(data: &[u8], index: u32) -> Option<Vec<u8>> {
    let u32_at = |at: usize| data.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let u16_at = |at: usize| data.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    if data.get(0..4)? != b"ttcf" {
        return None;
    }
    let start = u32_at(12 + 4 * index as usize)? as usize;
    let count = u16_at(start + 4)? as usize;
    let header_len = 12 + 16 * count;
    let mut out = data.get(start..start + header_len)?.to_vec();
    for i in 0..count {
        let record = start + 12 + 16 * i;
        let (offset, length) = (u32_at(record + 8)? as usize, u32_at(record + 12)? as usize);
        let at = out.len() as u32;
        out[12 + 16 * i + 8..12 + 16 * i + 12].copy_from_slice(&at.to_be_bytes());
        out.extend_from_slice(data.get(offset..offset + length)?);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    Some(out)
}

/// The typeface for a style's font: the file it names when it's a
/// TrueType or OpenType font that can be found, else the default.
pub fn typeface(font: &str) -> &'static Typeface {
    let key = font.trim().to_ascii_lowercase();
    if key.is_empty() || key.ends_with(".shx") {
        return &FONT;
    }
    if let Some(found) = FOUND.lock().ok().and_then(|f| f.get(&key).copied()) {
        return found.unwrap_or(&FONT);
    }
    let loaded = find(font).and_then(|path| std::fs::read(path).ok()).and_then(|data| {
        let data = from_collection(&data, 0).unwrap_or(data);
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        Typeface::parse(data, &key).map(|t| &*Box::leak(Box::new(t)))
    });
    if let Ok(mut found) = FOUND.lock() {
        found.insert(key, loaded);
    }
    loaded.unwrap_or(&FONT)
}

/// Text broken into lines: at paragraph marks, and where a line would run
/// past `wrap` ems (0 for no wrapping), between words.
pub fn lines(content: &str, wrap: f64) -> Vec<String> {
    lines_in(&FONT, content, wrap)
}

/// [`lines`] in typeface `face`.
pub fn lines_in(face: &Typeface, content: &str, wrap: f64) -> Vec<String> {
    wrap_lines(content, wrap, |s| face.width(s))
}

/// Text broken into lines at paragraph marks, and where a line would be
/// wider than `wrap` (0 for no wrapping) as `width` measures it.
pub fn wrap_lines(content: &str, wrap: f64, width: impl Fn(&str) -> f64) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in oxidraft_model::annotation::plain_text(content).split('\n') {
        if wrap <= 0.0 {
            out.push(paragraph.to_string());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && width(&candidate) > wrap {
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

    #[test]
    fn fonts_found_beside_the_drawing_or_not_at_all() {
        // The bundled typeface, as a file beside a drawing.
        let dir = std::env::temp_dir().join(format!("oxidraft-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Plotter.ttf"), epaint_default_fonts::UBUNTU_LIGHT).unwrap();
        add_search_dir(&dir);
        let t = typeface("plotter.ttf");
        assert!(!t.is_default() && t.key() == "plotter.ttf" && t.name() == "Ubuntu-Light", "{t:?}");
        assert!(std::ptr::eq(typeface("PLOTTER.TTF"), t), "found once");
        assert!(typeface("nowhere-to-be-found.ttf").is_default());
        assert!(typeface("romans.shx").is_default());
        // A collection's first font, made a font file of its own.
        let mut ttc = b"ttcf\x00\x01\x00\x00\x00\x00\x00\x01\x00\x00\x00\x10".to_vec();
        let font = epaint_default_fonts::UBUNTU_LIGHT;
        let tables = u16::from_be_bytes([font[4], font[5]]) as usize;
        let mut header = font[..12 + 16 * tables].to_vec();
        for i in 0..tables {
            let at = 12 + 16 * i + 8;
            let offset = u32::from_be_bytes(header[at..at + 4].try_into().unwrap()) + 16;
            header[at..at + 4].copy_from_slice(&offset.to_be_bytes());
        }
        ttc.extend_from_slice(&header);
        ttc.extend_from_slice(&font[12 + 16 * tables..]);
        let single = from_collection(&ttc, 0).unwrap();
        assert!(Face::parse(&single, 0).is_ok_and(|f| f.glyph_index('A').is_some()));
        std::fs::remove_dir_all(dir).ok();
    }
}

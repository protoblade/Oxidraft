//! Color-dependent plot style tables (AutoCAD's .ctb files): for each of
//! the 255 colors, the color it plots in (or the object's own), in grays
//! or not, how much of it (screening), and its lineweight (or the
//! object's). A .ctb is a 60-byte header and a zlib-compressed text.

use std::cell::RefCell;
use std::io::{Read, Write};

/// The header's first part; three little-endian 32-bit numbers follow
/// (a checksum, the text's length, the compressed length).
const SIGNATURE: &[u8] = b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec";

/// A color "use the object's color" in a .ctb.
const OBJECT_COLOR: i64 = -1;
const OBJECT_COLOR2: i64 = -1006632961;
/// `color_policy`'s grayscale bit.
const GRAYSCALE: i64 = 2;

/// AutoCAD's lineweight table, in millimeters: a pen's lineweight is an
/// index into it (0: the object's own).
const LINEWEIGHTS: [f64; 27] = [
    0.0, 0.05, 0.09, 0.10, 0.13, 0.15, 0.18, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.53, 0.60, 0.65, 0.70, 0.80,
    0.90, 1.00, 1.06, 1.20, 1.40, 1.58, 2.00, 2.11,
];

/// How one color plots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pen {
    /// The color it plots in; `None` for the object's own.
    pub color: Option<[u8; 3]>,
    pub grayscale: bool,
    /// Percent of the color's intensity (100: full).
    pub screen: u8,
    /// In millimeters; `None` for the object's lineweight.
    pub lineweight: Option<f64>,
}

impl Default for Pen {
    fn default() -> Self {
        Pen { color: None, grayscale: false, screen: 100, lineweight: None }
    }
}

/// A plot style table: its name (the file's) and a pen for each color.
#[derive(Debug, Clone, PartialEq)]
pub struct PenTable {
    pub name: String,
    pub description: String,
    /// Colors 1 to 255, in order.
    pub pens: Vec<Pen>,
}

impl PenTable {
    /// A table that plots everything as it is.
    pub fn new(name: &str) -> PenTable {
        PenTable { name: name.into(), description: String::new(), pens: vec![Pen::default(); 255] }
    }

    /// The pen of AutoCAD Color Index `aci` (1–255).
    pub fn pen(&self, aci: u8) -> Pen {
        self.pens.get(usize::from(aci).wrapping_sub(1)).copied().unwrap_or_default()
    }

    /// Reads a .ctb file.
    pub fn read(name: &str, bytes: &[u8]) -> Result<PenTable, String> {
        if bytes.len() < 60 || !bytes.starts_with(b"PIAFILEVERSION") {
            return Err("not a plot style table".into());
        }
        let mut text = String::new();
        flate2::read::ZlibDecoder::new(&bytes[60..]).read_to_string(&mut text).map_err(|e| e.to_string())?;
        let text = text.trim_end_matches('\0');
        let root = parse(text);
        if root.get("aci_table_available").and_then(Value::text).is_some_and(|v| v.eq_ignore_ascii_case("FALSE")) {
            return Err("a named plot style table (.stb), not a color-dependent one".into());
        }
        let weights: Vec<f64> = match root.get("custom_lineweight_table").and_then(Value::list) {
            Some(list) => {
                let mut w: Vec<(usize, f64)> =
                    list.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.text()?.parse().ok()?))).collect();
                w.sort_by_key(|x| x.0);
                w.into_iter().map(|x| x.1).collect()
            }
            None => LINEWEIGHTS.to_vec(),
        };
        let mut table = PenTable::new(name);
        table.description = root.get("description").and_then(Value::text).unwrap_or_default().to_string();
        let styles = root.get("plot_style").and_then(Value::list).ok_or("no plot styles")?;
        for (key, style) in styles {
            let (Ok(index), Some(style)) = (key.parse::<usize>(), style.list()) else { continue };
            let Some(pen) = table.pens.get_mut(index) else { continue };
            let get = |name: &str| style.iter().find(|(k, _)| k == name).and_then(|(_, v)| v.text());
            let int = |name: &str| get(name).and_then(|v| v.trim().parse::<i64>().ok());
            let color = int("mode_color").or(int("color")).unwrap_or(OBJECT_COLOR);
            pen.color = (int("color").is_none_or(|c| c != OBJECT_COLOR && c != OBJECT_COLOR2)
                && color != OBJECT_COLOR
                && color != OBJECT_COLOR2)
                .then(|| {
                    let c = color as u32;
                    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
                });
            pen.grayscale = int("color_policy").is_some_and(|p| p & GRAYSCALE != 0);
            pen.screen = int("screen").unwrap_or(100).clamp(0, 100) as u8;
            let weight = int("lineweight").and_then(|i| weights.get(i as usize)).copied().unwrap_or(0.0);
            pen.lineweight = (weight > 0.0).then_some(weight);
        }
        Ok(table)
    }

    /// The table as a .ctb file.
    pub fn write(&self) -> Vec<u8> {
        let mut weights = LINEWEIGHTS.to_vec();
        let mut text = String::new();
        text.push_str(&format!("description=\"{}\n", self.description));
        text.push_str(
            "aci_table_available=TRUE\nscale_factor=1.0\napply_factor=FALSE\ncustom_lineweight_display_units=0\n",
        );
        text.push_str("aci_table{\n");
        for i in 0..255 {
            text.push_str(&format!(" {i}=\"Color_{}\n", i + 1));
        }
        text.push_str("}\nplot_style{\n");
        for (i, pen) in self.pens.iter().enumerate().take(255) {
            let weight = match pen.lineweight {
                Some(w) => weights.iter().position(|x| (x - w).abs() < 1e-6).unwrap_or_else(|| {
                    weights.push(w);
                    weights.len() - 1
                }),
                None => 0,
            };
            text.push_str(&format!(
                " {i}{{\n  name=\"Color_{n}\n  localized_name=\"Color_{n}\n  description=\"\n",
                n = i + 1
            ));
            match pen.color {
                Some([r, g, b]) => {
                    // A color as AutoCAD writes it: 0xC2 (RGB) above red, green and blue.
                    let c = (0xC2u32 << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)) as i32;
                    text.push_str(&format!("  color={c}\n  mode_color={c}\n"));
                }
                None => text.push_str("  color=-1\n"),
            }
            let policy = 1 + if pen.grayscale { GRAYSCALE } else { 0 };
            text.push_str(&format!(
                "  color_policy={policy}\n  physical_pen_number=0\n  virtual_pen_number=0\n  screen={}\n  linepattern_size=0.5\n  linetype=31\n  adaptive_linetype=TRUE\n  lineweight={weight}\n  fill_style=73\n  end_style=4\n  join_style=5\n }}\n",
                pen.screen
            ));
        }
        text.push_str("}\ncustom_lineweight_table{\n");
        for (i, w) in weights.iter().enumerate() {
            text.push_str(&format!(" {i}={w:?}\n"));
        }
        text.push_str("}\n");
        let mut body = text.into_bytes();
        body.push(0);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = z.write_all(&body);
        let compressed = z.finish().unwrap_or_default();
        let mut out = SIGNATURE.to_vec();
        out.extend_from_slice(&adler32(&compressed).to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
        out
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    b << 16 | a
}

/// A value in a .ctb's text: a line's value, or a `{ … }` list.
enum Value {
    Text(String),
    List(Vec<(String, Value)>),
}

impl Value {
    fn text(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            Value::List(_) => None,
        }
    }

    fn list(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::List(l) => Some(l),
            Value::Text(_) => None,
        }
    }
}

struct Root(Vec<(String, Value)>);

impl Root {
    fn get(&self, name: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

/// The text's `name=value` lines and `name{ … }` lists, nested.
fn parse(text: &str) -> Root {
    fn list(lines: &mut std::iter::Peekable<std::str::Lines>) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line == "}" {
                break;
            }
            if let Some(name) = line.strip_suffix('{') {
                out.push((name.trim().to_string(), Value::List(list(lines))));
            } else if let Some((name, value)) = line.split_once('=') {
                // Strings start with a quote and run to the line's end.
                let value = value.strip_prefix('"').unwrap_or(value);
                out.push((name.trim().to_string(), Value::Text(value.to_string())));
            }
        }
        out
    }
    Root(list(&mut text.lines().peekable()))
}

thread_local! {
    /// The plot style tables the app has found, for [`crate::PlotStyle::Table`].
    static TABLES: RefCell<Vec<PenTable>> = const { RefCell::new(Vec::new()) };
}

/// Makes `tables` the ones plots can use.
pub fn set_tables(tables: Vec<PenTable>) {
    TABLES.with(|t| *t.borrow_mut() = tables);
}

/// The tables plots can use.
pub fn tables() -> Vec<PenTable> {
    TABLES.with(|t| t.borrow().clone())
}

/// Runs `f` on table `i`, if there is one.
pub(crate) fn with_table<R>(i: usize, f: impl FnOnce(&PenTable) -> R) -> Option<R> {
    TABLES.with(|t| t.borrow().get(i).map(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back() {
        let mut t = PenTable::new("pens.ctb");
        t.description = "Office pens".into();
        t.pens[0] = Pen { color: Some([0, 0, 0]), grayscale: false, screen: 100, lineweight: Some(0.5) };
        t.pens[7] = Pen { color: None, grayscale: true, screen: 40, lineweight: Some(0.123) };
        let bytes = t.write();
        assert_eq!(&bytes[..48], SIGNATURE);
        assert_eq!(bytes.len(), 60 + u32::from_le_bytes(bytes[56..60].try_into().unwrap()) as usize);
        let back = PenTable::read("pens.ctb", &bytes).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn reads_what_ezdxf_and_autocad_write() {
        // A style as AutoCAD writes it: an RGB color in mode_color, an ACI
        // one in color, and lineweight 13 (0.50 mm).
        let text = "description=\"\naci_table_available=TRUE\nplot_style{\n 0{\n  name=\"Color_1\n  color=-1023409925\n  mode_color=-1023409925\n  color_policy=3\n  screen=75\n  lineweight=13\n }\n 1{\n  color=-1\n  lineweight=0\n }\n}\n";
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(text.as_bytes()).unwrap();
        let mut bytes = SIGNATURE.to_vec();
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend(z.finish().unwrap());
        let t = PenTable::read("x.ctb", &bytes).unwrap();
        let c = -1023409925i64 as u32;
        assert_eq!(t.pens[0].color, Some([(c >> 16) as u8, (c >> 8) as u8, c as u8]));
        assert!(t.pens[0].grayscale && t.pens[0].screen == 75 && t.pens[0].lineweight == Some(0.5));
        assert_eq!(t.pens[1], Pen::default());
    }
}

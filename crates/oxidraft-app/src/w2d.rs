//! DWF files read back: a DWF 6 package (`(DWF V06.00)` and a zip of a
//! manifest, ePlot sections and their W2D graphics) or a single-stream
//! DWF of the WHIP! days (`(DWF V00.55)`). The first ePlot sheet's W2D
//! stream is decoded — its readable and binary opcodes, colors (indexed
//! through the color map too), fills and fonts — as lines, polylines,
//! arcs, circles, solid fills, points and text in millimeters on the
//! sheet, through the transform its descriptor gives; WHIP!'s compressed
//! blocks are expanded in place ([`crate::w2d_compression`]). Pictures are
//! left out; an opcode the reader doesn't know ends the stream, keeping
//! what came before.

use oxidraft_geom::Point;
use oxidraft_model::{Attach, Geometry, HatchPattern, Vertex};

use crate::cuix::{Element, parse_xml};

/// What a stream drew: the geometry in millimeters, and its RGB.
pub type Drawn = Vec<(Geometry, [u8; 3])>;

/// The color map W2D starts with.
fn default_color_map() -> Vec<[u8; 3]> {
    const LEVELS: [u8; 6] = [0, 51, 102, 153, 204, 255];
    const GRAYS: [u8; 20] = [0, 13, 26, 40, 53, 67, 80, 93, 107, 120, 134, 147, 161, 174, 187, 201, 214, 228, 241, 255];
    const FIRST: [[u8; 3]; 10] = [
        [0, 0, 0],
        [0, 0, 128],
        [0, 128, 0],
        [0, 128, 128],
        [128, 0, 0],
        [128, 0, 128],
        [128, 128, 0],
        [192, 192, 192],
        [192, 220, 192],
        [240, 202, 166],
    ];
    const LAST: [[u8; 3]; 10] = [
        [240, 251, 255],
        [164, 160, 160],
        [128, 128, 128],
        [0, 0, 255],
        [0, 255, 0],
        [0, 255, 255],
        [255, 0, 0],
        [255, 0, 255],
        [255, 255, 0],
        [255, 255, 255],
    ];
    let mut map = FIRST.to_vec();
    for b in LEVELS {
        for g in LEVELS {
            for r in LEVELS {
                map.push([r, g, b]);
            }
        }
    }
    map.extend(GRAYS.iter().map(|g| [*g, *g, *g]));
    map.extend(LAST);
    map
}

/// An extended ASCII record's parts.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Atom(String),
    Text(String),
    List(Vec<Node>),
}

impl Node {
    fn text(&self) -> Option<&str> {
        match self {
            Node::Atom(s) | Node::Text(s) => Some(s),
            Node::List(_) => None,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Node::Atom(s) => s.parse().ok(),
            _ => None,
        }
    }

    fn list(&self) -> Option<&[Node]> {
        match self {
            Node::List(v) => Some(v),
            _ => None,
        }
    }
}

/// An extended ASCII record (`(Name ...)`) as nodes; commas and spaces
/// separate atoms, `'...'` is a string (`\` escaping), `"...."` hex
/// UTF-16 and `{` a count-prefixed UTF-16 string.
fn parse_record(raw: &[u8]) -> Option<Node> {
    fn list(raw: &[u8], at: &mut usize) -> Option<Vec<Node>> {
        let mut out = Vec::new();
        loop {
            let c = *raw.get(*at)?;
            match c {
                b' ' | b'\t' | b'\r' | b'\n' | b',' => *at += 1,
                b')' => {
                    *at += 1;
                    return Some(out);
                }
                b'(' => {
                    *at += 1;
                    out.push(Node::List(list(raw, at)?));
                }
                b'\'' => {
                    *at += 1;
                    let mut s = Vec::new();
                    loop {
                        let c = *raw.get(*at)?;
                        *at += 1;
                        match c {
                            b'\\' => {
                                s.push(*raw.get(*at)?);
                                *at += 1;
                            }
                            b'\'' => break,
                            c => s.push(c),
                        }
                    }
                    out.push(Node::Text(String::from_utf8_lossy(&s).into_owned()));
                }
                b'"' => {
                    *at += 1;
                    let end = raw[*at..].iter().position(|c| *c == b'"')? + *at;
                    let hex = std::str::from_utf8(&raw[*at..end]).ok()?;
                    let units: Vec<u16> = hex
                        .as_bytes()
                        .chunks(4)
                        .filter_map(|d| u16::from_str_radix(std::str::from_utf8(d).ok()?, 16).ok())
                        .collect();
                    out.push(Node::Text(String::from_utf16_lossy(&units)));
                    *at = end + 1;
                }
                b'{' => {
                    let n = i32::from_le_bytes(raw.get(*at + 1..*at + 5)?.try_into().ok()?).max(0) as usize;
                    let units: Vec<u16> = raw
                        .get(*at + 5..*at + 5 + 2 * n)?
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .collect();
                    out.push(Node::Text(String::from_utf16_lossy(&units)));
                    *at += 5 + 2 * n + 1;
                }
                _ => {
                    let start = *at;
                    while raw.get(*at).is_some_and(|c| !b" \t\r\n,()'\"{".contains(c)) {
                        *at += 1;
                    }
                    out.push(Node::Atom(String::from_utf8_lossy(&raw[start..*at]).into_owned()));
                }
            }
        }
    }
    if raw.first() != Some(&b'(') {
        return None;
    }
    let mut at = 1;
    list(raw, &mut at).map(Node::List)
}

/// Where the extended ASCII record starting at `start` ends.
fn record_end(data: &[u8], start: usize) -> Option<usize> {
    let (mut at, mut depth, mut quote, mut escaped) = (start, 0usize, None, false);
    while let Some(&c) = data.get(at) {
        at += 1;
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            b'\'' | b'"' => quote = Some(c),
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            // A binary UTF-16 string inside: its count, its characters.
            b'{' => {
                let n = i32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?).max(0) as usize;
                at += 4 + 2 * n + 1;
            }
            _ => {}
        }
    }
    None
}

/// The decoder's state as it reads a stream.
struct Decoder {
    data: Vec<u8>,
    /// Compressed blocks expanded so far.
    expanded: usize,
    at: usize,
    /// The stream's revision (00.55 is 55, 06.00 is 600).
    revision: u32,
    current: (i64, i64),
    color: [u8; 3],
    map: Vec<[u8; 3]>,
    fill: bool,
    visible: bool,
    font_height: f64,
    font_rotation: f64,
    /// Logical units to millimeters on the sheet.
    to_mm: [f64; 6],
    out: Drawn,
}

impl Decoder {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.data.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let b = self.data.get(self.at..self.at + N)?.try_into().ok()?;
        self.at += N;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        self.bytes::<2>().map(u16::from_le_bytes)
    }

    fn i16(&mut self) -> Option<i16> {
        self.bytes::<2>().map(i16::from_le_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.bytes::<4>().map(u32::from_le_bytes)
    }

    fn i32(&mut self) -> Option<i32> {
        self.bytes::<4>().map(i32::from_le_bytes)
    }

    /// A count: a byte, or (0) 256 more than the 2 bytes after it.
    fn count(&mut self) -> Option<usize> {
        match self.byte()? {
            0 => Some(usize::from(self.u16()?) + 256),
            n => Some(usize::from(n)),
        }
    }

    fn relative(&mut self, wide: bool) -> Option<(i64, i64)> {
        let (dx, dy) = if wide {
            (i64::from(self.i32()?), i64::from(self.i32()?))
        } else {
            (i64::from(self.i16()?), i64::from(self.i16()?))
        };
        self.current = (self.current.0 + dx, self.current.1 + dy);
        Some(self.current)
    }

    fn relatives(&mut self, n: usize, wide: bool) -> Option<Vec<(i64, i64)>> {
        (0..n).map(|_| self.relative(wide)).collect()
    }

    fn separators(&mut self) {
        while self.data.get(self.at).is_some_and(|c| b" \t\r\n,".contains(c)) {
            self.at += 1;
        }
    }

    fn ascii_int(&mut self) -> Option<i64> {
        self.separators();
        let start = self.at;
        if self.data.get(self.at).is_some_and(|c| matches!(c, b'+' | b'-')) {
            self.at += 1;
        }
        while self.data.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        std::str::from_utf8(&self.data[start..self.at]).ok()?.parse().ok()
    }

    fn ascii_point(&mut self) -> Option<(i64, i64)> {
        Some((self.ascii_int()?, self.ascii_int()?))
    }

    /// A string operand: quoted, hex UTF-16, binary UTF-16, or a bare word.
    fn string(&mut self) -> Option<String> {
        self.separators();
        match *self.data.get(self.at)? {
            b'\'' | b'"' => {
                let mut raw = vec![b'('];
                let start = self.at;
                // The string alone, as a record of one.
                let quote = self.data[start];
                let mut at = start + 1;
                let mut escaped = false;
                while let Some(&c) = self.data.get(at) {
                    at += 1;
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == quote {
                        break;
                    }
                }
                raw.extend_from_slice(&self.data[start..at]);
                raw.push(b')');
                self.at = at;
                parse_record(&raw)?.list()?.first()?.text().map(str::to_string)
            }
            b'{' => {
                self.at += 1;
                let n = self.i32()?.max(0) as usize;
                let units: Vec<u16> = (0..n).map(|_| self.u16()).collect::<Option<_>>()?;
                (self.byte()? == b'}').then(|| String::from_utf16_lossy(&units))
            }
            _ => {
                let start = self.at;
                while self.data.get(self.at).is_some_and(|c| !b" \t\r\n()".contains(c)) {
                    self.at += 1;
                }
                Some(String::from_utf8_lossy(&self.data[start..self.at]).into_owned())
            }
        }
    }

    fn mm(&self, (x, y): (i64, i64)) -> Point {
        let [a, b, c, d, e, f] = self.to_mm;
        let (x, y) = (x as f64, y as f64);
        Point::new(a * x + c * y + e, b * x + d * y + f)
    }

    /// How long a logical length is in millimeters.
    fn length(&self, l: f64) -> f64 {
        l * (self.to_mm[0] * self.to_mm[3] - self.to_mm[1] * self.to_mm[2]).abs().sqrt()
    }

    fn push(&mut self, g: Geometry) {
        if self.visible {
            self.out.push((g, self.color));
        }
    }

    fn lines(&mut self, points: &[(i64, i64)], filled: bool) {
        let pts: Vec<Point> = points.iter().map(|p| self.mm(*p)).collect();
        if filled && pts.len() >= 3 {
            self.solid(vec![pts]);
        } else if pts.len() == 2 {
            self.push(Geometry::Line { start: pts[0], end: pts[1] });
        } else if pts.len() > 2 {
            let closed = pts.first() == pts.last();
            let mut vertices: Vec<Vertex> = pts.into_iter().map(Vertex::at).collect();
            if closed {
                vertices.pop();
            }
            self.push(Geometry::Polyline { vertices, closed });
        }
    }

    fn solid(&mut self, loops: Vec<Vec<Point>>) {
        let loops: Vec<Vec<Vertex>> =
            loops.into_iter().filter(|l| l.len() >= 3).map(|l| l.into_iter().map(Vertex::at).collect()).collect();
        if !loops.is_empty() {
            self.push(Geometry::Hatch {
                loops,
                pattern: HatchPattern::Solid,
                scale: 1.0,
                angle: 0.0,
                origin: Point::ORIGIN,
            });
        }
    }

    /// A triangle strip, as solid triangles.
    fn triangles(&mut self, points: &[(i64, i64)]) {
        let tris = points.windows(3).map(|w| w.iter().map(|p| self.mm(*p)).collect()).collect();
        self.solid(tris);
    }

    fn circle(&mut self, center: (i64, i64), radius: f64, start: u32, end: u32) {
        let (center, radius) = (self.mm(center), self.length(radius));
        if radius <= 0.0 {
            return;
        }
        let deg = |a: u32| f64::from(a) * 360.0 / 65536.0;
        if end.wrapping_sub(start).is_multiple_of(65536) {
            self.push(Geometry::Circle { center, radius });
        } else {
            self.push(Geometry::Arc { center, radius, start_angle: deg(start), end_angle: deg(end) });
        }
    }

    fn ellipse(&mut self, center: (i64, i64), major: f64, minor: f64, start: u32, end: u32, tilt: u32) {
        let rad = |a: u32| f64::from(a) * std::f64::consts::TAU / 65536.0;
        let (a0, mut a1, t) = (rad(start), rad(end), rad(tilt));
        if a1 <= a0 {
            a1 += std::f64::consts::TAU;
        }
        let n = 64;
        let pts: Vec<(i64, i64)> = (0..=n)
            .map(|k| {
                let a = a0 + (a1 - a0) * f64::from(k) / f64::from(n);
                let (x, y) = (major * a.cos(), minor * a.sin());
                let (x, y) = (x * t.cos() - y * t.sin(), x * t.sin() + y * t.cos());
                (center.0 + x.round() as i64, center.1 + y.round() as i64)
            })
            .collect();
        self.lines(&pts, self.fill);
    }

    fn text(&mut self, at: (i64, i64), text: String) {
        if text.trim().is_empty() {
            return;
        }
        let height = self.length(self.font_height).max(0.1) * 0.7;
        let position = self.mm(at);
        let rotation = self.font_rotation;
        self.push(Geometry::Text {
            position,
            height,
            rotation,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: text,
            look: Default::default(),
        });
    }

    /// One extended ASCII record.
    fn extended(&mut self, raw: &[u8]) -> bool {
        let Some(node) = parse_record(raw) else { return true };
        let Some(v) = node.list() else { return true };
        let name = v.first().and_then(Node::text).unwrap_or("");
        let numbers =
            |from: usize| -> Vec<i64> { v.iter().skip(from).filter_map(Node::number).map(|n| n as i64).collect() };
        let pairs = |n: &[i64]| n.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect::<Vec<_>>();
        match name {
            "EndOfDWF" => return false,
            "Color" => {
                let n = numbers(1);
                if n.len() >= 3 {
                    self.color = [n[0] as u8, n[1] as u8, n[2] as u8];
                }
            }
            "ColorMap" => {
                let n = numbers(2);
                let map: Vec<[u8; 3]> =
                    n.as_chunks::<4>().0.iter().map(|c| [c[0] as u8, c[1] as u8, c[2] as u8]).collect();
                if !map.is_empty() {
                    self.map = map;
                }
            }
            "Font" => {
                for option in v.iter().skip(1).filter_map(Node::list) {
                    let value = option.get(1).and_then(Node::number);
                    match (option.first().and_then(Node::text), value) {
                        (Some("Height"), Some(h)) => self.font_height = h,
                        (Some("Rotation"), Some(r)) => self.font_rotation = r * 360.0 / 65536.0,
                        _ => {}
                    }
                }
            }
            "Line" => {
                let p = pairs(&numbers(1));
                if p.len() == 2 {
                    self.lines(&p, false);
                }
            }
            "Polyline" | "Polygon" => {
                let p = pairs(&numbers(2));
                self.lines(&p, name == "Polygon" || self.fill);
            }
            "Circle" => {
                let n = numbers(1);
                if n.len() >= 3 {
                    let (start, end) = if n.len() >= 5 { (n[3] as u32, n[4] as u32) } else { (0, 65536) };
                    self.circle((n[0], n[1]), n[2] as f64, start, end);
                }
            }
            "Contour" => {
                let n = numbers(1);
                let Some(&count) = n.first() else { return true };
                let count = count.max(0) as usize;
                let counts: Vec<usize> = n.iter().skip(1).take(count).map(|c| (*c).max(0) as usize).collect();
                let mut points = pairs(&n[(1 + count).min(n.len())..]).into_iter();
                let loops: Vec<Vec<Point>> =
                    counts.iter().map(|c| points.by_ref().take(*c).map(|p| self.mm(p)).collect()).collect();
                self.solid(loops);
            }
            "Text" => {
                let n: Vec<i64> = v.iter().skip(1).take(2).filter_map(Node::number).map(|n| n as i64).collect();
                if let (2, Some(text)) = (n.len(), v.get(3).and_then(Node::text)) {
                    self.text((n[0], n[1]), text.to_string());
                }
            }
            _ => {}
        }
        true
    }

    /// One single-byte opcode; `None` when the stream can't go on.
    fn single(&mut self, op: u8) -> Option<()> {
        match op {
            b'V' => self.visible = true,
            b'v' => self.visible = false,
            b'F' => self.fill = true,
            b'f' => self.fill = false,
            0x03 => {
                let c = self.bytes::<4>()?;
                self.color = [c[0], c[1], c[2]];
            }
            b'C' => {
                let i = self.ascii_int()?;
                self.color = self.map.get(i.max(0) as usize).copied().unwrap_or(self.color);
            }
            b'c' => {
                let i = self.byte()?;
                self.color = self.map.get(usize::from(i)).copied().unwrap_or(self.color);
            }
            0x06 => {
                let fields = self.u16()?;
                if fields & 0x0001 != 0 {
                    self.string()?;
                }
                for (bit, size) in [(0x0002, 1), (0x0004, 1), (0x0008, 1), (0x0010, 1)] {
                    if fields & bit != 0 {
                        self.at += size;
                    }
                }
                if fields & 0x0020 != 0 {
                    self.font_height = f64::from(self.i32()?);
                }
                if fields & 0x0040 != 0 {
                    self.font_rotation = f64::from(self.u16()?) * 360.0 / 65536.0;
                }
                for bit in [0x0080, 0x0100, 0x0200] {
                    if fields & bit != 0 {
                        self.u16()?;
                    }
                }
                if fields & 0x0400 != 0 {
                    self.u32()?;
                }
            }
            0x17 => {
                self.i32()?;
            }
            0xAC | 0xCC => {
                self.count()?;
            }
            b'O' => self.current = (i64::from(self.i32()?), i64::from(self.i32()?)),
            0x0C | b'l' => {
                let p = self.relatives(2, op == b'l')?;
                self.lines(&p, false);
            }
            0x8C => {
                let n = self.count()?;
                for _ in 0..n {
                    let p = self.relatives(2, false)?;
                    self.lines(&p, false);
                }
            }
            b'L' => {
                let p = [self.ascii_point()?, self.ascii_point()?];
                self.lines(&p, false);
            }
            0x10 | 0x14 | b'p' | b't' => {
                let n = self.count()?;
                let p = self.relatives(n, matches!(op, b'p' | b't'))?;
                if matches!(op, 0x14 | b't') {
                    self.triangles(&p);
                } else {
                    self.lines(&p, self.fill);
                }
            }
            b'P' | b'T' => {
                let n = self.ascii_int()?.max(0) as usize;
                let p: Option<Vec<_>> = (0..n).map(|_| self.ascii_point()).collect();
                let p = p?;
                if op == b'T' {
                    self.triangles(&p);
                } else {
                    self.lines(&p, self.fill);
                }
            }
            b'b' | 0x02 => {
                let curves = self.count()?;
                let p = self.relatives(curves * 3 + 1, op == b'b')?;
                // Each cubic as its control polygon's points, sampled.
                let mut pts = vec![p[0]];
                for c in p[1..].as_chunks::<3>().0 {
                    let start = *pts.last()?;
                    for k in 1..=8 {
                        let t = f64::from(k) / 8.0;
                        let u = 1.0 - t;
                        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                        let q = [start, c[0], c[1], c[2]];
                        let x: f64 = q.iter().zip(w).map(|(p, w)| p.0 as f64 * w).sum();
                        let y: f64 = q.iter().zip(w).map(|(p, w)| p.1 as f64 * w).sum();
                        pts.push((x.round() as i64, y.round() as i64));
                    }
                }
                self.lines(&pts, false);
            }
            0x12 => {
                let c = self.relative(false)?;
                let r = f64::from(self.u16()?);
                self.circle(c, r, 0, 65536);
            }
            b'r' => {
                let c = self.relative(true)?;
                let r = f64::from(self.u32()?);
                self.circle(c, r, 0, 65536);
            }
            0x92 => {
                let c = self.relative(true)?;
                let r = f64::from(self.u32()?);
                let (s, e) = (u32::from(self.u16()?), u32::from(self.u16()?));
                self.circle(c, r, s, e);
            }
            b'R' => {
                let c = self.ascii_point()?;
                let r = self.ascii_int()? as f64;
                self.circle(c, r, 0, 65536);
            }
            b'e' => {
                let c = self.relative(true)?;
                let (major, minor) = (f64::from(self.u32()?), f64::from(self.u32()?));
                let (s, e, t) = (u32::from(self.u16()?), u32::from(self.u16()?), u32::from(self.u16()?));
                self.ellipse(c, major, minor, s, e, t);
            }
            b'E' => {
                let c = self.ascii_point()?;
                let axes = self.ascii_point()?;
                self.ellipse(c, axes.0 as f64, axes.1 as f64, 0, 0, 0);
            }
            b'x' => {
                let at = self.relative(true)?;
                let text = self.string()?;
                self.text(at, text);
            }
            0x18 => {
                let at = self.relative(true)?;
                let text = self.string()?;
                // Overscores, underscores, the bounds and (from 00.55) the
                // reserved options.
                let vectors = |d: &mut Self| -> Option<()> {
                    let n = d.count()?.checked_sub(1)?;
                    for _ in 0..n {
                        d.count()?;
                    }
                    Some(())
                };
                vectors(self)?;
                vectors(self)?;
                let saved = self.current;
                self.relatives(4, true)?;
                self.current = saved;
                if self.revision >= 55 {
                    vectors(self)?;
                }
                self.text(at, text);
            }
            0x07 | b'g' | 0x11 | b'q' => {
                let n = self.count()?;
                let mut points = Vec::with_capacity(n);
                let mut first_color = None;
                for _ in 0..n {
                    points.push(self.relative(matches!(op, b'g' | b'q'))?);
                    let c = self.bytes::<4>()?;
                    first_color.get_or_insert([c[0], c[1], c[2]]);
                }
                let saved = self.color;
                self.color = first_color.unwrap_or(saved);
                if matches!(op, 0x07 | b'g') {
                    self.triangles(&points);
                } else {
                    self.lines(&points, false);
                }
                self.color = saved;
            }
            0x0B | b'k' => {
                let n = self.count()?;
                let counts: Option<Vec<usize>> = (0..n).map(|_| self.count()).collect();
                let counts = counts?;
                let mut loops = Vec::new();
                for c in counts {
                    let p = self.relatives(c, op == b'k')?;
                    loops.push(p.into_iter().map(|q| self.mm(q)).collect());
                }
                self.solid(loops);
            }
            b'M' => {
                let n = self.ascii_int()?.max(0) as usize;
                for _ in 0..n {
                    let p = self.ascii_point()?;
                    let position = self.mm(p);
                    self.push(Geometry::Point { position });
                }
            }
            b'm' | 0x8D => {
                let n = self.count()?;
                for _ in 0..n {
                    let p = self.relative(op == b'm')?;
                    let position = self.mm(p);
                    self.push(Geometry::Point { position });
                }
            }
            b'G' | b'S' => {
                self.ascii_int()?;
            }
            b's' | b'N' => {
                self.u32()?;
            }
            0x87 | b'n' => {
                self.u16()?;
            }
            0x0E => {}
            _ => return None,
        }
        Some(())
    }

    /// Expands the compressed block at `start` in its place.
    fn expand(&mut self, start: usize) -> bool {
        const LIMIT: usize = 256 << 20;
        let Some(op) = self.data.get(start + 5..start + 7).map(|b| u16::from_le_bytes([b[0], b[1]])) else {
            return false;
        };
        if !matches!(op, 0x0010 | 0x0011 | 0x0123) || self.expanded >= 4096 {
            return false;
        }
        let payload = &self.data[start + 7..];
        let expanded = if op == 0x0011 {
            crate::w2d_compression::expand_zlib(payload, LIMIT)
        } else {
            crate::w2d_compression::expand_lz(payload, self.revision, LIMIT)
        };
        let Ok(x) = expanded else { return false };
        let end = start + 7 + x.consumed;
        if self.data.get(end) != Some(&b'}') || self.data.len() - (end + 1 - start) + x.bytes.len() > LIMIT {
            return false;
        }
        self.data.splice(start..=end, x.bytes);
        self.expanded += 1;
        self.at = start;
        true
    }

    /// Reads the stream, from after its header.
    fn run(&mut self) {
        while self.at < self.data.len() {
            let start = self.at;
            let op = self.data[start];
            if b" \t\r\n".contains(&op) {
                self.at += 1;
                continue;
            }
            match op {
                b'(' => {
                    let Some(end) = record_end(&self.data, start) else { return };
                    self.at = end;
                    let raw = self.data[start..end].to_vec();
                    if !self.extended(&raw) {
                        return;
                    }
                }
                // Extended binary: skipped by its length; one without is a
                // compressed block, expanded in its place (or, if it isn't
                // one, ends what can be read).
                b'{' => {
                    let Some(len) = self
                        .data
                        .get(start + 1..start + 5)
                        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                    else {
                        return;
                    };
                    if len == 0 {
                        if !self.expand(start) {
                            return;
                        }
                        continue;
                    }
                    self.at = start + 5 + len;
                }
                _ => {
                    self.at += 1;
                    if self.single(op).is_none() {
                        return;
                    }
                }
            }
        }
    }
}

/// A W2D (or single-stream DWF) stream's drawing, logical units taken to
/// millimeters by `to_mm` (x' = a x + c y + e, y' = b x + d y + f).
pub fn decode(data: &[u8], to_mm: [f64; 6]) -> Result<Drawn, String> {
    let skip = data.iter().take_while(|c| c.is_ascii_whitespace()).count();
    let header_end = record_end(data, skip).ok_or("Not a W2D stream.")?;
    let header = String::from_utf8_lossy(&data[skip..header_end]);
    let version = header
        .strip_prefix("(W2D V")
        .or_else(|| header.strip_prefix("(DWF V"))
        .and_then(|v| v.strip_suffix(')'))
        .ok_or("Not a W2D stream.")?;
    let revision = version.replace('.', "").parse::<u32>().map_err(|_| "Not a W2D stream.")?;
    let mut d = Decoder {
        data: data.to_vec(),
        expanded: 0,
        at: header_end,
        revision,
        current: (0, 0),
        color: [0, 0, 0],
        map: default_color_map(),
        fill: false,
        visible: true,
        font_height: 0.0,
        font_rotation: 0.0,
        to_mm,
        out: Vec::new(),
    };
    d.run();
    Ok(d.out)
}

/// The named child's attribute, on an element whose name may carry a
/// namespace prefix.
fn local(e: &Element) -> &str {
    e.name.rsplit(':').next().unwrap_or(&e.name)
}

fn attr<'a>(e: &'a Element, name: &str) -> Option<&'a str> {
    e.attrs.iter().find(|(n, _)| n.rsplit(':').next() == Some(name)).map(|(_, v)| v.as_str())
}

fn every<'a>(e: &'a Element, name: &str, out: &mut Vec<&'a Element>) {
    if local(e) == name {
        out.push(e);
    }
    for c in &e.children {
        every(c, name, out);
    }
}

fn entry_name(href: &str) -> String {
    href.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

/// A DWF file's first sheet: a DWF 6 package's first ePlot section, or a
/// single-stream DWF.
pub fn read_dwf(data: &[u8]) -> Result<Drawn, String> {
    if !data.starts_with(b"(DWF V") || data.len() < 12 {
        return Err("Not a DWF file.".into());
    }
    let major: u32 = std::str::from_utf8(&data[6..8]).ok().and_then(|m| m.parse().ok()).unwrap_or(0);
    if major < 6 {
        // A single stream: its logical units as thousandths of a
        // millimeter (it says no more).
        return decode(data, [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
    }
    let files = crate::datalink::unzip(&data[12..]);
    let file = |name: &str| files.iter().find(|(n, _)| entry_name(n) == entry_name(name)).map(|(_, c)| c);
    let manifest = file("manifest.xml").ok_or("The DWF file has no manifest.")?;
    let manifest = parse_xml(&String::from_utf8_lossy(manifest));
    let mut sections = Vec::new();
    every(&manifest, "Section", &mut sections);
    let section = sections
        .iter()
        .find(|s| attr(s, "type").is_some_and(|t| t.eq_ignore_ascii_case("com.autodesk.dwf.ePlot")))
        .ok_or("The DWF file has no ePlot sheets.")?;
    let mut resources = Vec::new();
    every(section, "Resource", &mut resources);
    let href_of = |role: &str| {
        resources
            .iter()
            .find(|r| attr(r, "role").is_some_and(|v| v.eq_ignore_ascii_case(role)))
            .and_then(|r| attr(r, "href"))
    };
    // The descriptor: the paper's units, and each stream's transform.
    let mut to_mm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stream = href_of("2d streaming graphics").map(str::to_string);
    if let Some(descriptor) = href_of("descriptor").and_then(file) {
        let page = parse_xml(&String::from_utf8_lossy(descriptor));
        let mut papers = Vec::new();
        every(&page, "Paper", &mut papers);
        let unit = papers
            .first()
            .and_then(|p| attr(p, "units"))
            .map_or(1.0, |u| if u.to_ascii_lowercase().starts_with("in") { 25.4 } else { 1.0 });
        let mut graphics = Vec::new();
        every(&page, "GraphicResource", &mut graphics);
        if let Some(g) =
            graphics.iter().find(|g| attr(g, "mime").is_some_and(|m| m.eq_ignore_ascii_case("application/x-w2d")))
        {
            stream = attr(g, "href").map(str::to_string).or(stream);
            let t: Vec<f64> =
                attr(g, "transform").unwrap_or("").split_whitespace().filter_map(|v| v.parse().ok()).collect();
            if t.len() == 16 {
                to_mm = [t[0] * unit, t[1] * unit, t[4] * unit, t[5] * unit, t[12] * unit, t[13] * unit];
            }
        }
    }
    let stream = stream.and_then(|h| file(&h)).ok_or("The DWF sheet has no W2D graphics.")?;
    decode(stream, to_mm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_and_binary_opcodes() {
        let mut data = b"(W2D V06.00)(Color 255,0,0,255)L 0,0 1000,0\nP 3 0,0 0,1000 1000,1000\n".to_vec();
        // Binary: an indexed color (6: red-ish 128,0,128? index 4 is 128,0,0),
        // a 32-bit relative line from the origin set by O.
        data.push(b'c');
        data.push(4);
        data.push(b'O');
        data.extend(2000i32.to_le_bytes());
        data.extend(0i32.to_le_bytes());
        data.push(b'l');
        for v in [0i32, 0, 500, 500] {
            data.extend(v.to_le_bytes());
        }
        // A full circle (0x12): center relative, radius.
        data.push(0x12);
        data.extend(0i16.to_le_bytes());
        data.extend(0i16.to_le_bytes());
        data.extend(250u16.to_le_bytes());
        data.extend_from_slice(
            b"F(Contour 1 3 0,0 10,0 10,10)f(Font (Height 2500) (Rotation 16384))(Text 5000,5000 'It\\'s')",
        );
        // An unknown extended record is skipped; an extended binary one too.
        data.extend_from_slice(b"(Future (Nested ')'))");
        data.push(b'{');
        data.extend(4u32.to_le_bytes());
        data.extend([0x99, 0x09, 0xAB, b'}']);
        data.extend_from_slice(b"L 0,0 0,7000(EndOfDWF)L 0,0 1,1");
        let drawn = decode(&data, [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]).unwrap();
        let kinds: Vec<&str> = drawn.iter().map(|(g, _)| g.name()).collect();
        assert_eq!(kinds, ["Line", "Polyline", "Line", "Circle", "Hatch", "MText", "Line"], "{drawn:?}");
        assert_eq!(drawn[0].1, [255, 0, 0]);
        assert_eq!(drawn[2].1, [128, 0, 0], "index 4 of the color map");
        assert_eq!(drawn[2].0, Geometry::Line { start: Point::new(2.0, 0.0), end: Point::new(2.5, 0.5) });
        let Geometry::Circle { center, radius } = &drawn[3].0 else { unreachable!() };
        assert!((center.x - 2.5).abs() < 1e-9 && (radius - 0.25).abs() < 1e-9, "relative to the line's end");
        let Geometry::Text { content, rotation, height, .. } = &drawn[5].0 else { unreachable!() };
        assert_eq!((content.as_str(), *rotation), ("It's", 90.0));
        assert!((height - 1.75).abs() < 1e-9);
    }

    #[test]
    fn compressed_blocks_expand_in_place() {
        // "L 0,0 1000,0" in a zlib wrapper, as WHIP! writes them.
        let mut data = b"(W2D V06.00)".to_vec();
        data.push(b'{');
        data.extend(0u32.to_le_bytes());
        data.extend(0x0011u16.to_le_bytes());
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, b"L 0,0 1000,0\n").unwrap();
        data.extend(z.finish().unwrap());
        data.push(b'}');
        data.extend_from_slice(b"L 0,0 0,2000(EndOfDWF)");
        let drawn = decode(&data, [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]).unwrap();
        assert_eq!(drawn.len(), 2, "{drawn:?}");
        assert_eq!(drawn[0].0, Geometry::Line { start: Point::ORIGIN, end: Point::new(1.0, 0.0) });
    }

    #[test]
    fn oxidrafts_dwf_reads_back() {
        let mut doc = oxidraft_model::Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 50.0) });
        doc.add(Geometry::Circle { center: Point::new(50.0, 25.0), radius: 10.0 });
        doc.add(Geometry::Text {
            position: Point::new(10.0, 40.0),
            height: 5.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "PLAN".into(),
            look: Default::default(),
        });
        let settings = oxidraft_plot::PlotSettings {
            area: oxidraft_plot::PlotArea::Extents,
            ..oxidraft_plot::PlotSettings::default()
        };
        let sheet = oxidraft_plot::render(&doc, oxidraft_model::MODEL, &settings).unwrap();
        let drawn = read_dwf(&oxidraft_plot::dwf::dwf(&[sheet], "Plan")).unwrap();
        assert!(
            drawn.iter().any(|(g, _)| matches!(g, Geometry::Text { content, .. } if content == "PLAN")),
            "{drawn:?}"
        );
        assert!(
            drawn.iter().filter(|(g, _)| matches!(g, Geometry::Polyline { .. } | Geometry::Line { .. })).count() >= 2
        );
        assert!(read_dwf(b"PK").is_err());
    }
}

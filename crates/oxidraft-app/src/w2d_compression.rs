//! WHIP!'s compressed-data wrappers in a W2D stream, expanded: zlib with
//! W2D's preset dictionary (wrapper 0x0011), and WHIP!'s own LZ (0x0010
//! and 0x0123) over a history that starts as that dictionary and two ACI
//! palettes. The dictionary and history are generated as ezdwf (MIT,
//! neka-nat) generates them from the WHIP! format; their checksums are the
//! ones W2D streams name.

use std::collections::VecDeque;

use flate2::{Decompress, FlushDecompress, Status};

/// The preset dictionary's Adler-32, as zlib streams ask for it.
const DICTIONARY_ADLER32: u32 = 0x5CBC_15B9;

/// What an expansion gave, and how many bytes of the input it took.
pub struct Expanded {
    pub bytes: Vec<u8>,
    pub consumed: usize,
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

/// W2D's default color map as an extended binary record.
fn binary_color_map(out: &mut Vec<u8>) {
    out.extend_from_slice(&[b'{', 0x04, 0x04, 0, 0, 1, 0, 0]);
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
    for c in FIRST {
        out.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
    for b in LEVELS {
        for g in LEVELS {
            for r in LEVELS {
                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
    }
    for g in GRAYS {
        out.extend_from_slice(&[g, g, g, 255]);
    }
    for c in LAST {
        out.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
}

/// Every four-digit decimal string once (a de Bruijn sequence), and its
/// first three digits again.
fn decimal_de_bruijn(out: &mut Vec<u8>) {
    fn generate(t: usize, period: usize, state: &mut [u8], sequence: &mut Vec<u8>) {
        if t > 4 {
            if 4 % period == 0 {
                sequence.extend_from_slice(&state[1..=period]);
            }
            return;
        }
        state[t] = state[t - period];
        generate(t + 1, period, state, sequence);
        for digit in state[t - period] + 1..10 {
            state[t] = digit;
            generate(t + 1, t, state, sequence);
        }
    }
    let mut state = [0u8; 40];
    let mut sequence = Vec::with_capacity(10_000);
    generate(1, 1, &mut state, &mut sequence);
    out.extend(sequence.iter().chain(sequence.iter().take(3)).map(|d| b'0' + d));
}

/// W2D's zlib preset dictionary.
fn dictionary() -> Vec<u8> {
    let mut d = Vec::with_capacity(11_476);
    d.extend_from_slice(b"(DashPattern (LineStyle (Copyright (Keywords (Viewport (CodePage ");
    binary_color_map(&mut d);
    for text in [
        "(NamedView ",
        "(Author ",
        "(Background ",
        "(Bezier ",
        "(Bounds ",
        "(Clip ",
        "(Color ",
        "(ColorMap ",
        "(Comment ",
        "(Created ",
        "(Creator ",
        "(Description ",
        "(DrawingInfo ",
        ")(Embed 'image/vnd.dwg;' 'AutoCAD-r13' 'unknown.dwg' '')",
        "(View ",
        "(Gouraud ",
        "(Image ",
        "(Layer ",
        "(LineCap ",
        "(LineJoin ",
        "(LinePattern ",
        "(LineWeight ",
        "(Modified ",
        "(Projection ",
        "(Scale ",
        "(SourceCreated ",
        "(SourceFilename ",
        "(SourceModified ",
        "(Text ",
        "(URL 'http://www.com')",
        "'ftp://ftp.",
    ] {
        d.extend_from_slice(text.as_bytes());
    }
    decimal_de_bruijn(&mut d);
    d.extend_from_slice(&[b'{', 0x04, 0x04, 0, 0, 1, 0, 0, 0xFF]);
    d
}

/// AutoCAD R13's ACI palette, for a white background or a black one (BGRA).
fn aci_palette(white: bool) -> Vec<u8> {
    const WHITE_FIRST: [[u8; 3]; 10] = [
        [255, 255, 255],
        [0, 0, 255],
        [0, 255, 255],
        [0, 255, 0],
        [255, 255, 0],
        [255, 0, 0],
        [255, 0, 255],
        [0, 0, 0],
        [128, 128, 128],
        [192, 192, 192],
    ];
    const BLACK_FIRST: [[u8; 3]; 10] = [
        [0, 0, 0],
        [0, 0, 255],
        [0, 255, 255],
        [0, 255, 0],
        [255, 255, 0],
        [255, 0, 0],
        [255, 0, 255],
        [255, 255, 255],
        [128, 128, 128],
        [192, 192, 192],
    ];
    const WHITE_LEVELS: [[u8; 7]; 5] = [
        [0, 63, 127, 159, 191, 223, 255],
        [0, 41, 82, 103, 124, 145, 165],
        [0, 31, 63, 79, 95, 111, 127],
        [0, 19, 38, 47, 57, 66, 76],
        [0, 9, 19, 23, 28, 33, 38],
    ];
    const BLACK_LEVELS: [[u8; 7]; 5] = [
        [0, 63, 127, 159, 191, 223, 255],
        [0, 51, 102, 127, 153, 178, 204],
        [0, 38, 76, 95, 114, 133, 153],
        [0, 31, 63, 79, 95, 111, 127],
        [0, 19, 38, 47, 57, 66, 76],
    ];
    let (first, levels) = if white { (WHITE_FIRST, WHITE_LEVELS) } else { (BLACK_FIRST, BLACK_LEVELS) };
    let mut out = Vec::with_capacity(1024);
    for c in first {
        out.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
    for hue in 0..24 {
        let (sector, remainder) = (hue / 4, hue % 4);
        for row in levels {
            let low = row[[0, 1, 2, 4][remainder]];
            let high = row[6];
            let descending = [high, row[4], row[2], row[1]][remainder];
            let (r, g, b) = match sector {
                0 => (high, low, 0),
                1 => (descending, high, 0),
                2 => (0, high, low),
                3 => (0, descending, high),
                4 => (low, 0, high),
                _ => (high, 0, descending),
            };
            out.extend_from_slice(&[b, g, r, 255]);
            let pastel_low = row[[2, 3, 4, 5][remainder]];
            let pastel_descending = [high, row[5], row[4], row[3]][remainder];
            let (r, g, b) = match sector {
                0 => (high, pastel_low, row[2]),
                1 => (pastel_descending, high, row[2]),
                2 => (row[2], high, pastel_low),
                3 => (row[2], pastel_descending, high),
                4 => (pastel_low, row[2], high),
                _ => (high, row[2], pastel_descending),
            };
            out.extend_from_slice(&[b, g, r, 255]);
        }
    }
    let grays: [u8; 6] = if white { [0, 45, 91, 137, 183, 179] } else { [51, 91, 132, 173, 214, 255] };
    for g in grays {
        out.extend_from_slice(&[g, g, g, 255]);
    }
    out
}

/// The LZ history W2D streams from revision 0.23 start with.
fn lz_history() -> Vec<u8> {
    let mut h = dictionary();
    let white = aci_palette(true);
    h.extend_from_slice(&white[1..]);
    h.extend_from_slice(&aci_palette(false));
    h
}

/// A zlib stream that may ask for W2D's preset dictionary.
pub fn expand_zlib(input: &[u8], limit: usize) -> Result<Expanded, String> {
    let mut d = Decompress::new(true);
    let mut out = Vec::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut supplied = false;
    loop {
        let at = d.total_in() as usize;
        let before = d.total_out();
        let status = match d.decompress(input.get(at..).unwrap_or_default(), &mut buffer, FlushDecompress::None) {
            Ok(s) => s,
            Err(e) => match e.needs_dictionary() {
                Some(DICTIONARY_ADLER32) if !supplied => {
                    d.set_dictionary(&dictionary()).map_err(|e| format!("W2D dictionary refused: {e}"))?;
                    supplied = true;
                    continue;
                }
                _ => return Err(format!("invalid zlib data: {e}")),
            },
        };
        let produced = (d.total_out() - before) as usize;
        if out.len() + produced > limit {
            return Err("expanded data too large".into());
        }
        out.extend_from_slice(&buffer[..produced]);
        if status == Status::StreamEnd {
            return Ok(Expanded { bytes: out, consumed: d.total_in() as usize });
        }
        if produced == 0 && d.total_in() as usize == at {
            return Err("truncated zlib data".into());
        }
    }
}

/// WHIP!'s LZ: runs of literals and copies from the history, ended by 0.
pub fn expand_lz(input: &[u8], revision: u32, limit: usize) -> Result<Expanded, String> {
    const HISTORY: usize = 65_536;
    let mut history: VecDeque<u8> = if revision >= 23 { lz_history().into() } else { VecDeque::new() };
    let mut out = Vec::new();
    let mut at = 0usize;
    let short = || "truncated LZ data".to_string();
    loop {
        let code = *input.get(at).ok_or_else(short)?;
        at += 1;
        if code == 0 {
            return Ok(Expanded { bytes: out, consumed: at });
        }
        let mut literals = usize::from(code & 0x0F);
        if literals == 15 {
            literals += usize::from(*input.get(at).ok_or_else(short)?);
            at += 1;
        }
        let run = input.get(at..at + literals).ok_or_else(short)?;
        if out.len() + run.len() > limit {
            return Err("expanded data too large".into());
        }
        out.extend_from_slice(run);
        for b in run {
            history.push_back(*b);
            if history.len() > HISTORY {
                history.pop_front();
            }
        }
        at += literals;
        let mut copies = usize::from(code >> 4);
        if copies == 0 {
            continue;
        }
        copies += 3;
        if code >> 4 == 15 {
            copies += usize::from(*input.get(at).ok_or_else(short)?);
            at += 1;
        }
        let offset = u16::from_le_bytes(input.get(at..at + 2).ok_or_else(short)?.try_into().map_err(|_| short())?);
        at += 2;
        let mut from = if revision >= 23 {
            history.len().checked_sub(usize::from(offset) + 1).ok_or("LZ offset outside the history")?
        } else {
            usize::from(offset)
        };
        for _ in 0..copies {
            let b = *history.get(from).ok_or("LZ copy outside the history")?;
            if out.len() >= limit {
                return Err("expanded data too large".into());
            }
            out.push(b);
            history.push_back(b);
            from += 1;
            if history.len() > HISTORY {
                history.pop_front();
                from = from.saturating_sub(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compress, Compression, FlushCompress};

    #[test]
    fn the_dictionary_and_history_w2d_streams_name() {
        let d = dictionary();
        assert_eq!((d.len(), adler32(&d)), (11_476, DICTIONARY_ADLER32));
        let h = lz_history();
        assert_eq!((h.len(), adler32(&h)), (13_523, 0x70C6_4AB6));
    }

    #[test]
    fn zlib_with_the_dictionary_and_lz_from_the_history() {
        let source = b"(Line 0,0 10,10)(EndOfDWF)";
        let mut e = Compress::new(Compression::default(), true);
        e.set_dictionary(&dictionary()).unwrap();
        let mut encoded = Vec::with_capacity(256);
        e.compress_vec(source, &mut encoded, FlushCompress::Finish).unwrap();
        encoded.extend_from_slice(b"}tail");
        let x = expand_zlib(&encoded, 1024).unwrap();
        assert_eq!((x.bytes.as_slice(), &encoded[x.consumed..]), (&source[..], &b"}tail"[..]));
        // A copy of the history's first four bytes: "(Das".
        let offset = (13_523u16 - 1).to_le_bytes();
        let x = expand_lz(&[0x10, offset[0], offset[1], 0], 600, 16).unwrap();
        assert_eq!((x.bytes.as_slice(), x.consumed), (&b"(Das"[..], 4));
    }
}

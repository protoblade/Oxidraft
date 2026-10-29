//! AutoCAD slides (.sld): a snapshot of the view as colored vectors in
//! screen dots, from the lower left. MSLIDE writes them, VSLIDE shows them.

/// A slide: its size in dots, and its vectors (color, from, to).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slide {
    pub width: u16,
    pub height: u16,
    pub vectors: Vec<(u8, (i16, i16), (i16, i16))>,
}

const ID: &[u8; 17] = b"AutoCAD Slide\r\n\x1a\0";

/// The slide as a level 2 slide file.
pub fn write_sld(slide: &Slide) -> Vec<u8> {
    let mut out = ID.to_vec();
    out.extend_from_slice(&[0x56, 2]);
    let (w, h) = (slide.width.max(1), slide.height.max(1));
    out.extend_from_slice(&(w - 1).to_le_bytes());
    out.extend_from_slice(&(h - 1).to_le_bytes());
    let aspect = (f64::from(w) / f64::from(h) * 10_000_000.0).round() as u32;
    out.extend_from_slice(&aspect.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&0x1234u16.to_le_bytes());
    let mut color = None;
    for &(c, (fx, fy), (tx, ty)) in &slide.vectors {
        if color != Some(c) {
            out.extend_from_slice(&[c, 0xFF]);
            color = Some(c);
        }
        for v in [fx, fy, tx, ty] {
            out.extend_from_slice(&v.clamp(0, 0x7FFF).to_le_bytes());
        }
    }
    out.extend_from_slice(&[0x00, 0xFC]);
    out
}

/// Reads a slide file (level 2): vectors, offset and common-endpoint
/// vectors, colors; solid fills are skipped.
pub fn read_sld(data: &[u8]) -> Result<Slide, String> {
    if data.get(..17) != Some(ID.as_slice()) {
        return Err("Not an AutoCAD slide.".into());
    }
    if data.get(18) != Some(&2) {
        return Err("Only level 2 slides can be read.".into());
    }
    let big_endian = data.get(29..31) == Some(&[0x12, 0x34]);
    let word = |i: usize| -> Option<u16> {
        let b = data.get(i..i + 2)?;
        Some(if big_endian { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) })
    };
    let width = word(19).ok_or("Truncated slide.")?.saturating_add(1);
    let height = word(21).ok_or("Truncated slide.")?.saturating_add(1);
    let mut slide = Slide { width, height, vectors: Vec::new() };
    let (mut color, mut from, mut to) = (7u8, (0i16, 0i16), (0i16, 0i16));
    let mut i = 31;
    while let Some(w) = word(i) {
        let (hi, lo) = ((w >> 8) as u8, (w & 0xFF) as u8);
        let byte = |k: usize| data.get(k).map_or(0, |b| *b as i8);
        match hi {
            0x00..=0x7F => {
                let (Some(fy), Some(tx), Some(ty)) = (word(i + 2), word(i + 4), word(i + 6)) else { break };
                (from, to) = ((w as i16, fy as i16), (tx as i16, ty as i16));
                slide.vectors.push((color, from, to));
                i += 8;
            }
            0xFB => {
                let (dfx, dfy, dtx, dty) = (lo as i8, byte(i + 2), byte(i + 3), byte(i + 4));
                let f = (from.0 + i16::from(dfx), from.1 + i16::from(dfy));
                let t = (from.0 + i16::from(dtx), from.1 + i16::from(dty));
                (from, to) = (f, t);
                slide.vectors.push((color, from, to));
                i += 5;
            }
            0xFC => break,
            0xFD => i += 6,
            0xFE => {
                let (dx, dy) = (lo as i8, byte(i + 2));
                let t = (to.0 + i16::from(dx), to.1 + i16::from(dy));
                slide.vectors.push((color, to, t));
                (from, to) = (to, t);
                i += 3;
            }
            0xFF => {
                color = lo;
                i += 2;
            }
            _ => i += 2,
        }
    }
    Ok(slide)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_round_trip() {
        let slide = Slide { width: 800, height: 600, vectors: vec![(1, (0, 0), (799, 599)), (5, (10, 20), (30, 40))] };
        let data = write_sld(&slide);
        assert!(data.starts_with(b"AutoCAD Slide"));
        assert_eq!(read_sld(&data).unwrap(), slide);
        // A common-endpoint vector goes on from the last one's end.
        let mut more = data[..data.len() - 2].to_vec();
        more.extend_from_slice(&[5, 0xFE, (-10i8) as u8, 0x00, 0xFC]);
        let back = read_sld(&more).unwrap();
        assert_eq!(back.vectors.last(), Some(&(5, (30, 40), (35, 30))));
    }
}

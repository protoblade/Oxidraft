//! A plotted picture as PNG, for the plot files that carry pictures as
//! image parts (DWFx): its pixels over white, in color or gray.

use std::io::Write as _;

use crate::Picture;

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

/// A pixel over white: RGB, or its gray.
pub(crate) fn over_white(px: [u8; 4], gray: bool) -> [u8; 3] {
    let a = u32::from(px[3]);
    let c = |v: u8| ((u32::from(v) * a + 255 * (255 - a)) / 255) as u8;
    let (r, g, b) = (c(px[0]), c(px[1]), c(px[2]));
    if gray {
        let y = ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8;
        [y, y, y]
    } else {
        [r, g, b]
    }
}

/// The picture as a PNG file: 8-bit RGB, or gray.
pub fn png(p: &Picture, gray: bool) -> Vec<u8> {
    let channels = if gray { 1 } else { 3 };
    let mut raw = Vec::with_capacity((p.width as usize * channels + 1) * p.height as usize);
    for row in p.rgba.chunks(p.width as usize * 4) {
        raw.push(0);
        for px in row.as_chunks::<4>().0 {
            let rgb = over_white(*px, gray);
            raw.extend_from_slice(&rgb[..channels]);
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = z.write_all(&raw);
    let idat = z.finish().unwrap_or_default();
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&p.width.to_be_bytes());
    ihdr.extend_from_slice(&p.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, if gray { 0 } else { 2 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_with_its_header_and_chunks() {
        let p = Picture { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 0] };
        let data = png(&p, false);
        assert!(data.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(&data[12..16], b"IHDR");
        assert_eq!(&data[16..24], &[0, 0, 0, 2, 0, 0, 0, 1]);
        assert!(data.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
        // The second pixel is transparent: white.
        assert_eq!(over_white([0, 0, 255, 0], false), [255, 255, 255]);
    }
}

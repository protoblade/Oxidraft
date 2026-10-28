//! AutoCAD Color Index (ACI) colors as RGB.

/// AutoCAD's 256-color palette. 1-9 are the named colors; 10-249 step round
/// the hue circle in 15° steps, each hue in five brightness levels at full
/// (even index) and half (odd index) saturation; 250-255 are grays.
pub fn aci_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        1 => (255, 0, 0),
        2 => (255, 255, 0),
        3 => (0, 255, 0),
        4 => (0, 255, 255),
        5 => (0, 0, 255),
        6 => (255, 0, 255),
        8 => (128, 128, 128),
        9 => (192, 192, 192),
        10..=249 => {
            let (hue, step) = (f64::from((index - 10) / 10) * 15.0, (index - 10) % 10);
            let value = [255.0, 255.0, 204.0, 204.0, 153.0, 153.0, 127.0, 127.0, 76.0, 76.0][step as usize];
            let saturation = if step % 2 == 0 { 1.0 } else { 0.5 };
            hsv(hue, saturation, value)
        }
        250..=255 => {
            let v = [51, 80, 105, 130, 190, 255][(index - 250) as usize];
            (v, v, v)
        }
        _ => (255, 255, 255),
    }
}

/// HSV to RGB with `value` in 0..=255, truncating like AutoCAD's table.
fn hsv(hue: f64, saturation: f64, value: f64) -> (u8, u8, u8) {
    let h = hue / 60.0;
    let c = value * saturation;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    ((r + m) as u8, (g + m) as u8, (b + m) as u8)
}

#[cfg(test)]
mod tests {
    use super::aci_rgb;

    #[test]
    fn matches_autocads_palette() {
        assert_eq!(aci_rgb(1), (255, 0, 0));
        assert_eq!(aci_rgb(10), (255, 0, 0));
        assert_eq!(aci_rgb(11), (255, 127, 127));
        assert_eq!(aci_rgb(13), (204, 102, 102));
        assert_eq!(aci_rgb(20), (255, 63, 0));
        assert_eq!(aci_rgb(30), (255, 127, 0));
        assert_eq!(aci_rgb(90), (0, 255, 0));
        assert_eq!(aci_rgb(100), (0, 255, 63));
        assert_eq!(aci_rgb(110), (0, 255, 127));
        assert_eq!(aci_rgb(130), (0, 255, 255));
        assert_eq!(aci_rgb(150), (0, 127, 255), "the blue of the reported parcels");
        assert_eq!(aci_rgb(151), (127, 191, 255));
        assert_eq!(aci_rgb(170), (0, 0, 255));
        assert_eq!(aci_rgb(250), (51, 51, 51));
    }
}

//! Colors and egui styling modeled on AutoCAD's dark theme.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

// ---- application chrome ------------------------------------------------------

/// Title / quick-access bar and ribbon tab strip.
pub const TOP_BAR: Color32 = Color32::from_rgb(37, 43, 54);
/// Ribbon panel body.
pub const RIBBON: Color32 = Color32::from_rgb(59, 68, 83);
/// Ribbon panel title strip and separators.
pub const RIBBON_TITLE: Color32 = Color32::from_rgb(50, 58, 71);
pub const RIBBON_HOVER: Color32 = Color32::from_rgb(78, 90, 110);
/// File tab strip and status bar.
pub const STRIP: Color32 = Color32::from_rgb(44, 51, 63);
pub const TEXT: Color32 = Color32::from_rgb(226, 230, 236);
pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 160, 175);
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(105, 114, 128);
/// Active toggles, selected tab underline, focused input.
pub const ACCENT: Color32 = Color32::from_rgb(10, 152, 255);
/// Oxidraft's own app button (rust orange).
pub const BRAND: Color32 = Color32::from_rgb(196, 84, 30);

// ---- model space ---------------------------------------------------------------

pub const MODEL_BG: Color32 = Color32::from_rgb(33, 40, 48);
pub const GRID_MINOR: Color32 = Color32::from_rgb(41, 49, 58);
pub const GRID_MAJOR: Color32 = Color32::from_rgb(53, 62, 73);
pub const AXIS_X: Color32 = Color32::from_rgb(110, 54, 58);
pub const AXIS_Y: Color32 = Color32::from_rgb(52, 98, 60);
pub const CROSSHAIR: Color32 = Color32::from_rgb(235, 235, 235);
pub const AUTOSNAP: Color32 = Color32::from_rgb(84, 230, 84);
pub const TRACKING: Color32 = Color32::from_rgb(120, 200, 120);
pub const PREVIEW: Color32 = Color32::from_rgb(200, 205, 215);
pub const SELECTION_GLOW: Color32 = Color32::from_rgba_unmultiplied_const(30, 120, 255, 130);
pub const GRIP: Color32 = Color32::from_rgb(32, 112, 255);
pub const GRIP_HOVER: Color32 = Color32::from_rgb(255, 105, 140);
pub const WINDOW_FILL: Color32 = Color32::from_rgba_unmultiplied_const(60, 120, 255, 45);
pub const WINDOW_EDGE: Color32 = Color32::from_rgb(70, 130, 255);
pub const CROSSING_FILL: Color32 = Color32::from_rgba_unmultiplied_const(50, 210, 100, 40);
pub const CROSSING_EDGE: Color32 = Color32::from_rgb(70, 220, 110);
pub const TOOLTIP_BG: Color32 = Color32::from_rgb(52, 60, 74);
pub const TOOLTIP_EDGE: Color32 = Color32::from_rgb(90, 102, 122);
pub const INPUT_BG: Color32 = Color32::from_rgb(28, 33, 41);

/// Maps an AutoCAD Color Index to a screen color on the dark background
/// (color 7 shows white).
pub fn aci(index: u8) -> Color32 {
    let (r, g, b) = aci_rgb(index);
    Color32::from_rgb(r, g, b)
}

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

/// Applies the dark AutoCAD-like style to every egui widget.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut v = egui::Visuals::dark();
    v.panel_fill = RIBBON;
    v.window_fill = TOOLTIP_BG;
    v.window_stroke = Stroke::new(1.0, TOOLTIP_EDGE);
    v.extreme_bg_color = INPUT_BG;
    v.faint_bg_color = STRIP;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.6);
    v.selection.stroke = Stroke::new(1.0, TEXT);
    v.hyperlink_color = ACCENT;
    v.override_text_color = Some(TEXT);
    v.window_corner_radius = CornerRadius::same(2);
    v.menu_corner_radius = CornerRadius::same(2);
    for w in [&mut v.widgets.inactive, &mut v.widgets.noninteractive] {
        w.bg_fill = RIBBON;
        w.weak_bg_fill = RIBBON;
        w.corner_radius = CornerRadius::same(2);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, RIBBON_TITLE);
    v.widgets.hovered.weak_bg_fill = RIBBON_HOVER;
    v.widgets.hovered.bg_fill = RIBBON_HOVER;
    v.widgets.hovered.corner_radius = CornerRadius::same(2);
    v.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.5);
    v.widgets.active.corner_radius = CornerRadius::same(2);
    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::new(10.5, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(12.5, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(6.0, 3.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
        s.spacing.interact_size.y = 20.0;
    });
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

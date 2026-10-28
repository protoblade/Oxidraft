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
    match index {
        1 => Color32::from_rgb(255, 0, 0),
        2 => Color32::from_rgb(255, 255, 0),
        3 => Color32::from_rgb(0, 255, 0),
        4 => Color32::from_rgb(0, 255, 255),
        5 => Color32::from_rgb(0, 0, 255),
        6 => Color32::from_rgb(255, 0, 255),
        8 => Color32::from_rgb(128, 128, 128),
        9 => Color32::from_rgb(192, 192, 192),
        _ => Color32::WHITE,
    }
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

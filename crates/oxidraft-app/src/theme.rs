//! Colors and egui styling modeled on AutoCAD's: the dark or light color
//! theme (COLORTHEME) of the ribbon, palettes and command line, and a dark
//! or white drawing area, chosen apart as in AutoCAD's Options.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};
use serde::{Deserialize, Serialize};

/// AutoCAD's COLORTHEME: the ribbon, palettes and command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorTheme {
    #[default]
    Dark,
    Light,
}

/// The drawing area's background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Background {
    #[default]
    Dark,
    White,
}

static LIGHT: AtomicBool = AtomicBool::new(false);
static WHITE: AtomicBool = AtomicBool::new(false);

pub fn color_theme() -> ColorTheme {
    if LIGHT.load(Ordering::Relaxed) { ColorTheme::Light } else { ColorTheme::Dark }
}

pub fn background() -> Background {
    if WHITE.load(Ordering::Relaxed) { Background::White } else { Background::Dark }
}

/// Switches the theme and the drawing area's background, and restyles
/// egui's widgets to match.
pub fn set(ctx: &egui::Context, theme: ColorTheme, background: Background) {
    LIGHT.store(theme == ColorTheme::Light, Ordering::Relaxed);
    WHITE.store(background == Background::White, Ordering::Relaxed);
    apply(ctx);
}

/// The application's colors around the drawing.
struct Chrome {
    /// Title / quick-access bar and ribbon tab strip.
    top_bar: Color32,
    /// Ribbon panel body.
    ribbon: Color32,
    /// Ribbon panel title strip and separators.
    ribbon_title: Color32,
    ribbon_hover: Color32,
    /// File tab strip and status bar, and the open drawing's tab.
    strip: Color32,
    tab: Color32,
    text: Color32,
    text_dim: Color32,
    text_disabled: Color32,
    /// Text that warns of something lost.
    warning: Color32,
    /// Active toggles, selected tab underline, focused input.
    accent: Color32,
    tooltip_bg: Color32,
    tooltip_edge: Color32,
    input_bg: Color32,
    /// The edge of dropdowns, text fields, buttons and checkboxes.
    control_edge: Color32,
    /// The command line's input bar, its history over the drawing, and the
    /// clickable options in its prompt.
    command_bar: Color32,
    command_history: Color32,
    command_keyword: Color32,
}

const DARK_CHROME: Chrome = Chrome {
    top_bar: Color32::from_rgb(37, 43, 54),
    ribbon: Color32::from_rgb(59, 68, 83),
    ribbon_title: Color32::from_rgb(50, 58, 71),
    ribbon_hover: Color32::from_rgb(78, 90, 110),
    strip: Color32::from_rgb(44, 51, 63),
    tab: Color32::from_rgb(33, 40, 48),
    text: Color32::from_rgb(226, 230, 236),
    text_dim: Color32::from_rgb(150, 160, 175),
    text_disabled: Color32::from_rgb(105, 114, 128),
    warning: Color32::from_rgb(240, 180, 70),
    accent: Color32::from_rgb(10, 152, 255),
    tooltip_bg: Color32::from_rgb(52, 60, 74),
    tooltip_edge: Color32::from_rgb(90, 102, 122),
    input_bg: Color32::from_rgb(28, 33, 41),
    control_edge: Color32::from_rgb(84, 96, 116),
    command_bar: Color32::from_rgb(52, 60, 74),
    command_history: Color32::from_rgba_premultiplied(30, 35, 44, 200),
    command_keyword: Color32::from_rgb(70, 82, 102),
};

const LIGHT_CHROME: Chrome = Chrome {
    top_bar: Color32::from_rgb(214, 220, 228),
    ribbon: Color32::from_rgb(240, 243, 247),
    ribbon_title: Color32::from_rgb(222, 227, 234),
    ribbon_hover: Color32::from_rgb(203, 217, 236),
    strip: Color32::from_rgb(228, 232, 238),
    tab: Color32::from_rgb(252, 252, 253),
    text: Color32::from_rgb(34, 39, 46),
    text_dim: Color32::from_rgb(92, 101, 116),
    text_disabled: Color32::from_rgb(160, 167, 178),
    warning: Color32::from_rgb(176, 100, 0),
    accent: Color32::from_rgb(0, 112, 210),
    tooltip_bg: Color32::from_rgb(252, 252, 253),
    tooltip_edge: Color32::from_rgb(176, 184, 196),
    input_bg: Color32::from_rgb(255, 255, 255),
    control_edge: Color32::from_rgb(170, 178, 190),
    command_bar: Color32::from_rgb(250, 251, 253),
    command_history: Color32::from_rgba_premultiplied(225, 228, 233, 230),
    command_keyword: Color32::from_rgb(212, 223, 238),
};

/// The drawing area's colors.
struct Model {
    background: Color32,
    /// Labels on the drawing: the viewport controls, a picture's name.
    label: Color32,
    grid_minor: Color32,
    grid_major: Color32,
    axis_x: Color32,
    axis_y: Color32,
    crosshair: Color32,
    autosnap: Color32,
    tracking: Color32,
    preview: Color32,
    selection_glow: Color32,
    grip: Color32,
    grip_hover: Color32,
    window_fill: Color32,
    window_edge: Color32,
    crossing_fill: Color32,
    crossing_edge: Color32,
}

const DARK_MODEL: Model = Model {
    background: Color32::from_rgb(33, 40, 48),
    label: Color32::from_rgb(150, 160, 175),
    grid_minor: Color32::from_rgb(41, 49, 58),
    grid_major: Color32::from_rgb(53, 62, 73),
    axis_x: Color32::from_rgb(110, 54, 58),
    axis_y: Color32::from_rgb(52, 98, 60),
    crosshair: Color32::from_rgb(235, 235, 235),
    autosnap: Color32::from_rgb(84, 230, 84),
    tracking: Color32::from_rgb(120, 200, 120),
    preview: Color32::from_rgb(200, 205, 215),
    selection_glow: Color32::from_rgba_unmultiplied_const(30, 120, 255, 130),
    grip: Color32::from_rgb(32, 112, 255),
    grip_hover: Color32::from_rgb(255, 105, 140),
    window_fill: Color32::from_rgba_unmultiplied_const(60, 120, 255, 45),
    window_edge: Color32::from_rgb(70, 130, 255),
    crossing_fill: Color32::from_rgba_unmultiplied_const(50, 210, 100, 40),
    crossing_edge: Color32::from_rgb(70, 220, 110),
};

const WHITE_MODEL: Model = Model {
    background: Color32::from_rgb(255, 255, 255),
    label: Color32::from_rgb(92, 101, 116),
    grid_minor: Color32::from_rgb(237, 240, 244),
    grid_major: Color32::from_rgb(214, 220, 228),
    axis_x: Color32::from_rgb(228, 150, 150),
    axis_y: Color32::from_rgb(140, 198, 140),
    crosshair: Color32::from_rgb(30, 30, 30),
    autosnap: Color32::from_rgb(0, 150, 40),
    tracking: Color32::from_rgb(40, 140, 70),
    preview: Color32::from_rgb(78, 84, 96),
    selection_glow: Color32::from_rgba_unmultiplied_const(30, 120, 255, 100),
    grip: Color32::from_rgb(32, 112, 255),
    grip_hover: Color32::from_rgb(230, 60, 100),
    window_fill: Color32::from_rgba_unmultiplied_const(60, 120, 255, 40),
    window_edge: Color32::from_rgb(40, 100, 230),
    crossing_fill: Color32::from_rgba_unmultiplied_const(40, 170, 80, 40),
    crossing_edge: Color32::from_rgb(30, 150, 60),
};

fn chrome() -> &'static Chrome {
    if LIGHT.load(Ordering::Relaxed) { &LIGHT_CHROME } else { &DARK_CHROME }
}

fn model() -> &'static Model {
    if WHITE.load(Ordering::Relaxed) { &WHITE_MODEL } else { &DARK_MODEL }
}

macro_rules! colors {
    ($palette:ident: $($name:ident),*) => {
        $(pub fn $name() -> Color32 {
            $palette().$name
        })*
    };
}

colors!(chrome: top_bar, ribbon, ribbon_title, ribbon_hover, strip, tab, text, text_dim, text_disabled, warning, accent,
    tooltip_bg, tooltip_edge, input_bg, control_edge, command_bar, command_history, command_keyword);

colors!(model: label, grid_minor, grid_major, axis_x, axis_y, crosshair, autosnap, tracking, preview, selection_glow, grip,
    grip_hover, window_fill, window_edge, crossing_fill, crossing_edge);

/// The drawing area's background.
pub fn model_bg() -> Color32 {
    model().background
}

/// Oxidraft's own app button (rust orange), in either theme.
pub const BRAND: Color32 = Color32::from_rgb(196, 84, 30);

/// Maps an AutoCAD Color Index to a screen color in the drawing: color 7
/// shows white on a dark background and black on a white one, as in AutoCAD.
pub fn aci(index: u8) -> Color32 {
    if index == 7 && WHITE.load(Ordering::Relaxed) {
        return Color32::BLACK;
    }
    swatch(index)
}

/// An AutoCAD Color Index as its swatch shows it in palettes (7 is white).
pub fn swatch(index: u8) -> Color32 {
    let (r, g, b) = oxidraft_model::aci_rgb(index);
    Color32::from_rgb(r, g, b)
}

/// Applies the current AutoCAD-like style to every egui widget.
pub fn apply(ctx: &egui::Context) {
    let light = LIGHT.load(Ordering::Relaxed);
    let theme = if light { egui::Theme::Light } else { egui::Theme::Dark };
    ctx.set_theme(theme);
    let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.panel_fill = ribbon();
    v.window_fill = tooltip_bg();
    v.window_stroke = Stroke::new(1.0, tooltip_edge());
    v.extreme_bg_color = input_bg();
    v.faint_bg_color = strip();
    v.selection.bg_fill = accent().gamma_multiply(if light { 0.35 } else { 0.6 });
    v.selection.stroke = Stroke::new(1.0, text());
    v.hyperlink_color = accent();
    v.override_text_color = Some(text());
    v.window_corner_radius = CornerRadius::same(2);
    v.menu_corner_radius = CornerRadius::same(2);
    for w in [&mut v.widgets.inactive, &mut v.widgets.noninteractive] {
        w.bg_fill = ribbon();
        w.weak_bg_fill = ribbon();
        w.corner_radius = CornerRadius::same(2);
    }
    // Dropdowns, fields and buttons keep their edge, not just on hover.
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, control_edge());
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, ribbon_title());
    v.widgets.hovered.weak_bg_fill = ribbon_hover();
    v.widgets.hovered.bg_fill = ribbon_hover();
    v.widgets.hovered.corner_radius = CornerRadius::same(2);
    v.widgets.active.weak_bg_fill = accent().gamma_multiply(0.5);
    v.widgets.active.corner_radius = CornerRadius::same(2);
    ctx.set_visuals_of(theme, v);

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

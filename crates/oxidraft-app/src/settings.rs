//! What's kept between runs, as AutoCAD keeps its options in the user's
//! profile: the color theme and the drawing area's background, in a small
//! JSON file in the user's configuration folder. The Options dialog
//! (OPTIONS, OP) sets them.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout};
use serde::{Deserialize, Serialize};

use crate::app::OxidraftApp;
use crate::theme::{self, Background, ColorTheme};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub color_theme: ColorTheme,
    pub background: Background,
    /// Where the Properties palette sits.
    pub properties_dock: Dock,
    /// The Blocks palette's library folder.
    pub block_library: Option<PathBuf>,
    /// Command aliases of the user's own (ALIASEDIT), as (alias, command).
    pub aliases: Vec<(String, String)>,
    /// Words added to the dictionary (SPELL).
    pub words: Vec<String>,
    /// The tool palettes, once changed from the defaults (TOOLPALETTES).
    pub tool_palettes: Vec<crate::tool_palettes::ToolPalette>,
    /// Scales added to AutoCAD's (SCALELISTEDIT), as drawing units per
    /// paper unit.
    pub scales: Vec<f64>,
    /// Paper sizes of your own (PLOTTERMANAGER): name, width and height in
    /// millimeters.
    pub papers: Vec<(String, f64, f64)>,
    /// How fine PNG and JPEG plots are, in dots per inch (0: 150).
    pub raster_dpi: u32,
    /// Workspaces saved (WSSAVE), and the current one's name.
    pub workspaces: Vec<crate::customize::Workspace>,
    pub workspace: String,
    /// Keyboard shortcuts of your own (CUI): keys, command.
    pub shortcuts: Vec<(String, String)>,
    /// Commands added to the Quick Access Toolbar (CUI).
    pub quick_access: Vec<String>,
}

/// Where a palette sits: docked at the left or right of the drawing
/// area, or floating over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Dock {
    #[default]
    Left,
    Right,
    Float,
}

impl Settings {
    /// Where they're kept: Application Support on macOS, AppData on
    /// Windows, the XDG config folder elsewhere.
    pub fn default_path() -> Option<PathBuf> {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let dir = if cfg!(target_os = "macos") {
            env("HOME")?.join("Library/Application Support/Oxidraft")
        } else if cfg!(windows) {
            env("APPDATA")?.join("Oxidraft")
        } else {
            env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?.join("oxidraft")
        };
        Some(dir.join("settings.json"))
    }

    /// The settings in `path`, or the defaults if there are none (or they
    /// can't be read).
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

/// The Options dialog's choices, applied with OK or Apply.
pub(crate) struct OptionsDialog {
    settings: Settings,
}

impl OxidraftApp {
    /// Uses `settings` from now on: restyles the app and keeps them for
    /// next time, when they're kept at all.
    pub fn apply_settings(&mut self, ctx: &egui::Context, settings: Settings) {
        // Restyle only when the look itself changes.
        let look = |s: &Settings| (s.color_theme, s.background);
        if look(&settings) != look(&self.settings) {
            theme::set(ctx, settings.color_theme, settings.background);
        }
        self.keep_settings(settings);
    }

    /// Uses `settings` from now on, the look unchanged, and keeps them.
    pub(crate) fn keep_settings(&mut self, settings: Settings) {
        self.engine.vars.color_theme = u8::from(settings.color_theme == ColorTheme::Light);
        self.engine.aliases = settings.aliases.clone();
        if settings.papers != self.settings.papers {
            oxidraft_plot::set_custom_papers(&settings.papers);
        }
        if settings != self.settings
            && let Some(path) = &self.settings_path
            && let Err(e) = settings.save(path)
        {
            self.engine.log(format!("Could not save the options to {}: {e}", path.display()));
        }
        self.settings = settings;
    }

    pub(crate) fn open_options(&mut self) {
        self.options = Some(OptionsDialog { settings: self.settings.clone() });
    }

    /// OPTIONS: the color theme of the ribbon and palettes, and the
    /// drawing area's background, as on AutoCAD's Display tab.
    pub(crate) fn options_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.options.take() else { return };
        let (mut open, mut ok, mut apply, mut cancel) = (true, false, false, false);
        egui::Window::new("Options")
            .id(egui::Id::unique("options"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 180.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Display").strong());
                ui.add_space(4.0);
                egui::Grid::new("options_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Color theme:");
                    egui::ComboBox::from_id_salt("options_color_theme")
                        .width(140.0)
                        .selected_text(match d.settings.color_theme {
                            ColorTheme::Dark => "Dark",
                            ColorTheme::Light => "Light",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.settings.color_theme, ColorTheme::Dark, "Dark");
                            ui.selectable_value(&mut d.settings.color_theme, ColorTheme::Light, "Light");
                        });
                    ui.end_row();
                    ui.label("Drawing background:");
                    egui::ComboBox::from_id_salt("options_background")
                        .width(140.0)
                        .selected_text(match d.settings.background {
                            Background::Dark => "Dark",
                            Background::White => "White",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.settings.background, Background::Dark, "Dark");
                            ui.selectable_value(&mut d.settings.background, Background::White, "White");
                        });
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.separator();
                let row = egui::vec2(ui.available_width().max(260.0), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    apply = ui.button("Apply").clicked();
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok || apply {
            self.apply_settings(ctx, d.settings.clone());
        }
        if open && !ok && !cancel {
            self.options = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_default_when_missing() {
        let path = std::env::temp_dir().join(format!("oxidraft-settings-{}/settings.json", std::process::id()));
        assert_eq!(Settings::load(&path), Settings::default());
        let s = Settings {
            color_theme: ColorTheme::Light,
            background: Background::White,
            properties_dock: Dock::Right,
            block_library: Some("/blocks".into()),
            aliases: vec![("QQ".into(), "CIRCLE".into())],
            words: vec!["Oxidraft".into()],
            tool_palettes: crate::tool_palettes::default_palettes(),
            scales: vec![25.0],
            papers: vec![("Banner".into(), 1000.0, 300.0)],
            raster_dpi: 300,
            workspaces: vec![crate::customize::Workspace {
                name: "Layout".into(),
                properties: true,
                ..Default::default()
            }],
            workspace: "Layout".into(),
            shortcuts: vec![("Ctrl+Shift+L".into(), "LAYER".into())],
            quick_access: vec!["MATCHPROP".into()],
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        // Unknown or missing fields fall back to the defaults.
        std::fs::write(&path, r#"{"color_theme":"Light","later":1}"#).unwrap();
        assert_eq!(Settings::load(&path), Settings { color_theme: ColorTheme::Light, ..Settings::default() });
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}

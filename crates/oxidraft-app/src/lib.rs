//! The Oxidraft desktop app: an AutoCAD-style window around the command engine.

mod app;
mod attedit;
mod blocks;
mod canvas;
mod chrome;
mod clipboard;
mod cmdline;
mod compare;
mod drawing_units;
mod icons;
mod layer_walk;
mod layers;
mod layouts;
mod manage;
mod named_views;
mod object_props;
mod plot;
mod properties;
mod publish;
mod qselect;
mod quickcalc;
mod settings;
mod shortcut_menu;
mod styles;
mod tables;
pub mod theme;
mod tiles;
mod underlay;
pub mod update;
mod view;
mod viewport;
mod wblock;
mod xref;

pub use app::OxidraftApp;
pub use layers::LAYER_MANAGER;
pub use settings::Settings;
pub use wblock::{Afterwards, WblockSource};

/// Opens the main window.
pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Oxidraft")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png")).unwrap_or_default(),
            ),
        ..Default::default()
    };
    let file = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    eframe::run_native(
        "Oxidraft",
        options,
        Box::new(|cc| {
            let mut app = OxidraftApp::new(cc);
            // The options chosen last time, kept for next time.
            if let Some(path) = settings::Settings::default_path() {
                let kept = settings::Settings::load(&path);
                app.keep_settings_in(path);
                app.apply_settings(&cc.egui_ctx, kept);
            }
            if let Some(path) = file {
                app.open_path(path);
            }
            if std::env::var_os(update::DISABLE_ENV).is_none() {
                app.updater.check(&cc.egui_ctx, false);
            }
            Ok(Box::new(app))
        }),
    )
}

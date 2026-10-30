//! The Oxidraft desktop app: an AutoCAD-style window around the command engine.

mod annotation_styles;
mod app;
mod attedit;
mod autosave;
mod battman;
mod blocks;
mod canvas;
mod chrome;
mod classic;
mod clipboard;
mod cmdline;
mod compare;
mod customize;
mod datalink;
mod design_center;
mod dgn;
mod diminspect;
mod drafting;
mod drawing_units;
pub mod drawings;
mod dwf_in;
mod etransmit;
mod fields;
mod files;
mod fonts;
mod icons;
mod image_out;
mod last_cmds;
mod layer_states;
mod layer_walk;
mod layers;
mod layouts;
mod lisp;
mod lisp_ents;
mod lisp_host;
mod manage;
mod more_app;
mod named_views;
mod object_props;
mod paste_scales;
mod pdf_import;
mod plot;
mod plot_styles;
mod plotters;
mod properties;
mod publish;
mod qselect;
mod quickcalc;
mod settings;
mod share;
mod sheet_set;
mod shortcut_menu;
mod slides;
mod spell;
mod status;
mod styles;
mod sysvars;
mod tables;
mod text_styles;
pub mod theme;
mod tiles;
mod tolerance;
pub mod tool_palettes;
mod ucs_dialog;
mod ulayers;
mod underlay;
pub mod update;
mod view;
mod viewport;
mod wblock;
mod xref;

pub use app::OxidraftApp;
pub use drawings::Drawings;

/// A drawing's model space as a DGN (V7) file's bytes.
pub fn dgn_bytes(doc: &oxidraft_model::Document) -> Vec<u8> {
    dgn::write_dgn(doc)
}
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
            Ok(Box::new(Drawings::new(app)))
        }),
    )
}

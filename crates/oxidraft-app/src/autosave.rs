//! Automatic saves, as AutoCAD makes them: every SAVETIME minutes a
//! drawing with changes is written to an autosave file (.sv$) in the
//! system's temporary folder; one closed normally leaves none behind.
//! DRAWINGRECOVERY lists the autosaves left by a crash, to open.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::app::OxidraftApp;

/// Where autosaves go.
pub fn autosave_dir() -> PathBuf {
    std::env::temp_dir().join("Oxidraft autosave")
}

/// The autosaves there, newest first, with when each was written.
pub fn autosaves() -> Vec<(PathBuf, std::time::SystemTime)> {
    let Ok(entries) = std::fs::read_dir(autosave_dir()) else { return Vec::new() };
    let mut out: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("sv$")))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.1));
    out
}

impl OxidraftApp {
    /// This drawing's autosave file.
    pub(crate) fn autosave_path(&self) -> PathBuf {
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        autosave_dir().join(format!("{stem}_{}_{}.sv$", std::process::id(), self.untitled))
    }

    /// Writes the autosave when SAVETIME minutes have gone by with changes.
    pub(crate) fn autosave_tick(&mut self) {
        let minutes = self.save_time;
        if minutes == 0 || !self.is_dirty() {
            self.last_autosave = Instant::now();
            return;
        }
        if self.last_autosave.elapsed() < Duration::from_secs(u64::from(minutes) * 60) {
            return;
        }
        self.last_autosave = Instant::now();
        self.autosave_now();
    }

    /// Writes this drawing's autosave (DWG data, as AutoCAD's .sv$ are).
    pub fn autosave_now(&mut self) -> Option<PathBuf> {
        let path = self.autosave_path();
        std::fs::create_dir_all(autosave_dir()).ok()?;
        match oxidraft_io_dwg::save_file(&self.doc, &path, self.dwg_version.unwrap_or_default()) {
            Ok(()) => {
                self.engine.log(format!("Automatic save to {}", path.display()));
                Some(path)
            }
            Err(e) => {
                self.engine.log(format!("Automatic save failed: {e}"));
                None
            }
        }
    }

    /// The autosave goes once the drawing is saved or closed normally.
    pub(crate) fn drop_autosave(&self) {
        let _ = std::fs::remove_file(self.autosave_path());
    }

    /// DRAWINGRECOVERY: the Drawing Recovery Manager, listing autosaves.
    pub(crate) fn drawing_recovery(&mut self, ctx: &egui::Context) {
        if !self.recovery_open {
            return;
        }
        let mut open = true;
        let mut chosen = None;
        egui::Window::new("Drawing Recovery Manager")
            .id(egui::Id::unique("drawing_recovery"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 160.0])
            .show(ctx, |ui| {
                let list = autosaves();
                if list.is_empty() {
                    ui.label("No automatic saves to recover.");
                }
                for (path, when) in list {
                    let when = chrono::DateTime::<chrono::Local>::from(when).format("%Y-%m-%d %H:%M");
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(format!("{name}  ({when})"));
                        if ui.button("Open").clicked() {
                            chosen = Some(path.clone());
                        }
                    });
                }
            });
        self.recovery_open = open;
        if let Some(path) = chosen {
            if self.in_tabs() {
                self.tab_request = Some(crate::drawings::TabRequest::Open(path));
            } else {
                self.open_path(path);
            }
            self.recovery_open = false;
        }
    }
}

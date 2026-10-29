//! ADCENTER (Ctrl+2): DesignCenter. Folders and the drawings in them (or
//! this drawing), each drawing's blocks, layers, linetypes, text styles,
//! xrefs and layouts; blocks inserted, and layers, linetypes and text
//! styles brought into this drawing, with a double-click or Add.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::Document;

use crate::{app::OxidraftApp, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Blocks,
    Layers,
    Linetypes,
    TextStyles,
    Xrefs,
    Layouts,
}

impl Category {
    const ALL: [Category; 6] = [
        Category::Blocks,
        Category::Layers,
        Category::Linetypes,
        Category::TextStyles,
        Category::Xrefs,
        Category::Layouts,
    ];

    fn name(self) -> &'static str {
        match self {
            Category::Blocks => "Blocks",
            Category::Layers => "Layers",
            Category::Linetypes => "Linetypes",
            Category::TextStyles => "Textstyles",
            Category::Xrefs => "Xrefs",
            Category::Layouts => "Layouts",
        }
    }

    /// The names of `doc`'s items of this kind.
    fn items(self, doc: &Document) -> Vec<String> {
        let mut out: Vec<String> = match self {
            Category::Blocks => doc
                .blocks()
                .filter(|b| !b.name.starts_with('*') && doc.xref(&b.name).is_none())
                .map(|b| b.name.clone())
                .collect(),
            Category::Layers => doc.layers().iter().map(|l| l.name.clone()).collect(),
            Category::Linetypes => {
                oxidraft_commands::current_props::loaded_linetypes(doc).into_iter().map(|l| l.name).collect()
            }
            Category::TextStyles => doc.text_styles().iter().map(|s| s.name.clone()).collect(),
            Category::Xrefs => doc.xrefs().map(|x| x.name.clone()).collect(),
            Category::Layouts => doc.layouts().iter().map(|l| l.name.clone()).collect(),
        };
        out.sort_by_key(|n| n.to_ascii_lowercase());
        out
    }
}

/// Where DesignCenter is looking.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    /// This drawing (the Open Drawings tab).
    Here,
    File(PathBuf),
}

pub(crate) struct DesignCenter {
    folder: PathBuf,
    source: Option<Source>,
    category: Category,
    picked: Option<String>,
    status: String,
}

fn is_drawing(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"))
}

/// A folder's subfolders and drawings, each sorted.
fn folder_contents(folder: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut dirs = Vec::new();
    let mut drawings = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let p = entry.path();
        let hidden = p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if p.is_dir() {
            dirs.push(p);
        } else if is_drawing(&p) {
            drawings.push(p);
        }
    }
    let key = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    dirs.sort_by_key(key);
    drawings.sort_by_key(key);
    (dirs, drawings)
}

impl OxidraftApp {
    pub(crate) fn toggle_design_center(&mut self) {
        if self.design_center.take().is_some() {
            return;
        }
        let folder = self
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        self.design_center = Some(DesignCenter {
            folder,
            source: Some(Source::Here),
            category: Category::Blocks,
            picked: None,
            status: String::new(),
        });
    }

    /// Opens DesignCenter on `folder` (tests, and Browse).
    pub fn design_center_in(&mut self, folder: PathBuf) {
        self.design_center = None;
        self.toggle_design_center();
        if let Some(dc) = &mut self.design_center {
            dc.folder = folder;
            dc.source = None;
        }
    }

    /// Brings `name` of `category` from the source drawing into this one.
    fn add_from(&mut self, source: &Source, category: Category, name: &str) -> String {
        let Source::File(file) = source else {
            if category == Category::Blocks {
                self.engine.insert_block(name, &mut self.doc);
                return format!("Inserting {name}.");
            }
            return format!("{name} is already in this drawing.");
        };
        self.library_drawing(file);
        let Some(Ok(from)) = self.library_drawings.get(file).cloned() else {
            return "That drawing couldn't be read.".into();
        };
        if category == Category::Blocks {
            self.insert_from_library(file, Some(name));
            return format!("Inserting {name}.");
        }
        self.engine.cancel(&mut self.doc);
        self.doc.begin("ADCENTER");
        let added = match category {
            Category::Layers => self.doc.import_layer(&from, name),
            Category::Linetypes => self.doc.import_linetype(&from, name),
            Category::TextStyles => self.doc.import_text_style(&from, name),
            _ => {
                self.doc.rollback();
                return format!("{} can't be added from DesignCenter.", category.name());
            }
        };
        self.doc.commit();
        if added { format!("Added {name}.") } else { format!("This drawing already has {name}.") }
    }

    pub(crate) fn design_center(&mut self, ctx: &egui::Context) {
        let Some(mut dc) = self.design_center.take() else { return };
        let mut open = true;
        let (dirs, drawings) = folder_contents(&dc.folder);
        if let Some(Source::File(f)) = &dc.source {
            self.library_drawing(&f.clone());
        }
        let source_doc: Option<Document> = match &dc.source {
            Some(Source::Here) => Some(self.doc.clone()),
            Some(Source::File(f)) => self.library_drawings.get(f).and_then(|r| r.as_ref().ok()).cloned(),
            None => None,
        };
        let read_error = match &dc.source {
            Some(Source::File(f)) => self.library_drawings.get(f).and_then(|r| r.as_ref().err()).cloned(),
            _ => None,
        };
        let items = source_doc.as_ref().map(|d| dc.category.items(d)).unwrap_or_default();
        let (mut go, mut add, mut browse, mut up) = (None, None, false, false);
        egui::Window::new("DesignCenter")
            .id(egui::Id::unique("design_center"))
            .open(&mut open)
            .collapsible(true)
            .resizable(true)
            .default_size([720.0, 380.0])
            .default_pos([260.0, 200.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    up = ui.button("Up").on_hover_text("The folder above").clicked();
                    browse = ui.button("Browse...").clicked();
                    ui.label(RichText::new(dc.folder.display().to_string()).color(theme::text_dim()));
                });
                ui.separator();
                ui.horizontal_top(|ui| {
                    // Folders and their drawings; this drawing first.
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        egui::ScrollArea::vertical().id_salt("dc_tree").max_height(280.0).show(ui, |ui| {
                            let here = dc.source == Some(Source::Here);
                            if ui.selectable_label(here, format!("{} (this drawing)", self.file_name())).clicked() {
                                (dc.source, dc.picked) = (Some(Source::Here), None);
                            }
                            for d in &dirs {
                                let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                if ui.selectable_label(false, format!("{name}/")).double_clicked() {
                                    go = Some(d.clone());
                                }
                            }
                            for f in &drawings {
                                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                let picked = dc.source.as_ref() == Some(&Source::File(f.clone()));
                                if ui.selectable_label(picked, name.clone()).clicked() {
                                    (dc.source, dc.picked) = (Some(Source::File(f.clone())), None);
                                }
                            }
                        });
                    });
                    ui.separator();
                    // The drawing's kinds of named objects.
                    ui.vertical(|ui| {
                        ui.set_width(120.0);
                        for c in Category::ALL {
                            let n = source_doc.as_ref().map_or(0, |d| c.items(d).len());
                            if ui.selectable_label(dc.category == c, format!("{} ({n})", c.name())).clicked() {
                                (dc.category, dc.picked) = (c, None);
                            }
                        }
                    });
                    ui.separator();
                    // Its items: double-click (or Add) to bring one in.
                    ui.vertical(|ui| {
                        ui.set_min_width(240.0);
                        if let Some(e) = &read_error {
                            ui.colored_label(ui.visuals().error_fg_color, e);
                        } else if source_doc.is_none() {
                            ui.colored_label(theme::text_dim(), "Pick a drawing.");
                        }
                        egui::ScrollArea::vertical().id_salt("dc_items").max_height(250.0).show(ui, |ui| {
                            for name in &items {
                                let r = ui.selectable_label(dc.picked.as_deref() == Some(name.as_str()), name);
                                if r.clicked() {
                                    dc.picked = Some(name.clone());
                                }
                                if r.double_clicked() {
                                    add = Some(name.clone());
                                }
                            }
                        });
                        let can_add = matches!(
                            dc.category,
                            Category::Blocks | Category::Layers | Category::Linetypes | Category::TextStyles
                        );
                        let label = if dc.category == Category::Blocks { "Insert" } else { "Add to Drawing" };
                        let row = vec2(ui.available_width(), 24.0);
                        ui.allocate_ui_with_layout(row, Layout::left_to_right(Align::Center), |ui| {
                            if ui.add_enabled(can_add && dc.picked.is_some(), egui::Button::new(label)).clicked() {
                                add = dc.picked.clone();
                            }
                        });
                    });
                });
                if !dc.status.is_empty() {
                    ui.colored_label(theme::text_dim(), &dc.status);
                }
            });
        if up && let Some(parent) = dc.folder.parent() {
            dc.folder = parent.to_path_buf();
        }
        if let Some(d) = go {
            dc.folder = d;
        }
        if browse && let Some(d) = rfd::FileDialog::new().set_directory(&dc.folder).pick_folder() {
            dc.folder = d;
        }
        if let (Some(name), Some(source)) = (add, dc.source.clone()) {
            dc.status = self.add_from(&source, dc.category, &name);
        }
        if open {
            self.design_center = Some(dc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_and_drawings() {
        let dir = std::env::temp_dir().join(format!("oxidraft-dc-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        for f in ["b.DXF", "a.dwg", "notes.txt", ".hidden.dxf"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let (dirs, drawings) = folder_contents(&dir);
        let names =
            |v: &[PathBuf]| v.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(&dirs), ["Sub"]);
        assert_eq!(names(&drawings), ["a.dwg", "b.DXF"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}

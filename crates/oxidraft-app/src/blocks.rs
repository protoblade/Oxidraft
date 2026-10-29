//! Blocks: the Edit Block Definition dialog and the block editor (BEDIT,
//! BSAVE, BCLOSE), and the Blocks palette (BLOCKSPALETTE) that inserts the
//! drawing's own blocks or those of library drawings in a folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use oxidraft_geom::{Aabb, Point, curve::Seg};
use oxidraft_model::{Document, Geometry, Styled, is_dependent, sanitize_name};

use crate::{app::OxidraftApp, settings::Settings, theme};

/// A preview tile's size.
const TILE: Vec2 = vec2(96.0, 80.0);

/// What's drawn of `items` (curves, and text and fills as their outlines)
/// fitted into `rect`.
fn paint_preview(ui: &Ui, rect: Rect, items: &[Styled]) {
    let Some(b) = items.iter().map(|s| s.geometry.bounds()).reduce(Aabb::union) else { return };
    let inner = rect.shrink(6.0);
    let scale = (f64::from(inner.width()) / b.width().max(1e-9)).min(f64::from(inner.height()) / b.height().max(1e-9));
    let c = b.center();
    let to = |p: Point| {
        pos2(inner.center().x + ((p.x - c.x) * scale) as f32, inner.center().y - ((p.y - c.y) * scale) as f32)
    };
    let painter = ui.painter_at(rect);
    for s in items {
        let stroke = Stroke::new(1.0, theme::aci(s.color));
        let segs = match &s.geometry {
            Geometry::Text { .. } | Geometry::Hatch { .. } | Geometry::Image { .. } => {
                let r = s.geometry.bounds();
                painter.rect_stroke(Rect::from_two_pos(to(r.min), to(r.max)), 0.0, stroke, egui::StrokeKind::Middle);
                continue;
            }
            g => g.segments(),
        };
        for seg in segs {
            let points: Vec<Pos2> = match seg {
                Seg::Line(l) => vec![to(l.start), to(l.end)],
                Seg::Arc(a) => {
                    let n = (a.sweep.abs() / 10.0).ceil().max(2.0) as usize;
                    (0..=n).map(|i| to(seg.point_at(i as f64 / n as f64))).collect()
                }
            };
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}

/// A tile: a preview over a name. Returns its response.
fn tile(ui: &mut Ui, items: &[Styled], name: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(TILE + vec2(0.0, 18.0), Sense::click());
    let picture = Rect::from_min_size(rect.min, TILE);
    ui.painter().rect_filled(picture, 2.0, theme::model_bg());
    paint_preview(ui, picture, items);
    let edge = if selected {
        Stroke::new(2.0, theme::accent())
    } else if response.hovered() {
        Stroke::new(1.0, theme::accent())
    } else {
        Stroke::new(1.0, theme::control_edge())
    };
    ui.painter().rect_stroke(picture, 2.0, edge, egui::StrokeKind::Inside);
    let mut job =
        egui::text::LayoutJob::simple_singleline(name.into(), egui::FontId::proportional(11.5), theme::text());
    job.wrap = egui::text::TextWrapping::truncate_at_width(TILE.x);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(rect.center().x - galley.size().x / 2.0, picture.bottom() + 2.0), galley, theme::text());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    response.on_hover_text(name)
}

/// The blocks a drawing offers: not anonymous, not from an xref.
fn insertable(doc: &Document) -> Vec<String> {
    let mut names: Vec<String> = doc
        .blocks()
        .map(|b| b.name.clone())
        .filter(|n| !n.starts_with('*') && !is_dependent(n) && doc.xref(n).is_none())
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// The Blocks palette: which tab, and the library drawing opened in it.
#[derive(Default)]
pub(crate) struct BlocksPalette {
    library: bool,
    file: Option<PathBuf>,
}

impl OxidraftApp {
    // ---- block editor ---------------------------------------------------------

    /// BEDIT: the Edit Block Definition dialog, on `selected` if given (a
    /// block reference that was double-clicked or selected).
    pub(crate) fn open_block_edit_dialog(&mut self, selected: Option<String>) {
        let from_selection = || {
            self.engine.selection().iter().find_map(|id| match &self.doc.entity(*id)?.geometry {
                Geometry::Insert { block, .. } if self.doc.xref(block).is_none() => Some(block.clone()),
                _ => None,
            })
        };
        let name = selected.or_else(from_selection).or_else(|| insertable(&self.doc).into_iter().next());
        self.block_edit_dialog = Some(name.unwrap_or_default());
    }

    pub(crate) fn block_edit_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut selected) = self.block_edit_dialog.take() else { return };
        let names = insertable(&self.doc);
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Edit Block Definition")
            .id(egui::Id::unique("bedit_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 180.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label("Block to create or edit");
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            if names.is_empty() {
                                ui.colored_label(theme::text_dim(), "This drawing has no blocks.");
                            }
                            for n in &names {
                                let row = ui.selectable_label(n.eq_ignore_ascii_case(&selected), n);
                                if row.clicked() {
                                    selected = n.clone();
                                }
                                if row.double_clicked() {
                                    ok = true;
                                }
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Preview").color(theme::text_dim()));
                        let (rect, _) = ui.allocate_exact_size(vec2(200.0, 200.0), Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, theme::model_bg());
                        paint_preview(ui, rect, &self.doc.block_contents(&selected));
                    });
                });
                ui.separator();
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok |= ui.add_enabled(self.doc.block(&selected).is_some(), egui::Button::new("OK")).clicked();
                });
            });
        if ok {
            self.open_block_editor(&selected);
        } else if open && !cancel {
            self.block_edit_dialog = Some(selected);
        }
    }

    /// Opens block `name` in the block editor, zoomed to it.
    pub fn open_block_editor(&mut self, name: &str) {
        self.engine.cancel(&mut self.doc);
        self.engine.cancel(&mut self.doc);
        self.engine.log("Command: BEDIT");
        self.doc.begin("BEDIT");
        let opened = self.doc.begin_block_edit(name);
        self.doc.commit();
        match opened {
            Ok(_) => {
                self.block_editor_view = Some(self.view);
                self.block_editor_saved = self.doc.revision();
                let working: Option<Aabb> = self.doc.refedit().and_then(|r| {
                    self.doc
                        .entities()
                        .filter(|e| r.in_working_set(e.id))
                        .map(|e| self.doc.entity_bounds(e))
                        .reduce(Aabb::union)
                });
                // The base point at the origin is part of the view.
                let view = working.map_or(Aabb::from_points(Point::new(-10.0, -10.0), Point::new(10.0, 10.0)), |b| {
                    b.union(Aabb::from_points(Point::ORIGIN, Point::ORIGIN))
                });
                self.pending_views.push(oxidraft_commands::ViewRequest::Window(view));
                self.engine.log(format!("Editing block \"{name}\": its base point is at the origin. BCLOSE ends."));
            }
            Err(e) => self.engine.log(e),
        }
    }

    /// True while the block editor is open.
    pub fn in_block_editor(&self) -> bool {
        self.doc.refedit().is_some_and(|r| r.editor)
    }

    /// BSAVE: saves the block and goes on editing.
    pub(crate) fn save_block(&mut self) {
        if !self.in_block_editor() {
            self.engine.log("** Command only allowed in the Block Editor **".to_string());
            return;
        }
        self.engine.cancel(&mut self.doc);
        self.doc.begin("BSAVE");
        self.doc.save_block_edit();
        self.doc.commit();
        self.block_editor_saved = self.doc.revision();
        let name = self.doc.refedit().map(|r| r.block.clone()).unwrap_or_default();
        self.engine.log(format!("Block \"{name}\" saved."));
    }

    /// BCLOSE: closes the block editor, asking first if there are changes
    /// not saved; `save` answers that.
    pub(crate) fn close_block_editor(&mut self, save: Option<bool>) {
        if !self.in_block_editor() {
            self.engine.log("** Command only allowed in the Block Editor **".to_string());
            return;
        }
        self.engine.cancel(&mut self.doc);
        let changed = self.doc.revision() != self.block_editor_saved;
        let save = match (save, changed) {
            (Some(s), _) => s,
            (None, false) => false,
            (None, true) => {
                self.block_close_prompt = true;
                return;
            }
        };
        if save {
            self.save_block();
        }
        self.doc.begin("BCLOSE");
        self.doc.close_refedit(false);
        self.doc.commit();
        if let Some(view) = self.block_editor_view.take() {
            self.view = view;
        }
        self.block_close_prompt = false;
    }

    /// "Block - Changes Not Saved", as AutoCAD asks on BCLOSE.
    pub(crate) fn block_close_dialog(&mut self, ctx: &egui::Context) {
        if !self.block_close_prompt {
            return;
        }
        let name = self.doc.refedit().map(|r| r.block.clone()).unwrap_or_default();
        let mut answer = None;
        let mut open = true;
        egui::Window::new("Block - Changes Not Saved")
            .id(egui::Id::unique("bclose_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 220.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                if ui.button(format!("Save the changes to {name}")).clicked() {
                    answer = Some(true);
                }
                if ui.button("Discard the changes and close the Block Editor").clicked() {
                    answer = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.block_close_prompt = false;
                }
            });
        if !open {
            self.block_close_prompt = false;
        }
        if let Some(save) = answer {
            self.close_block_editor(Some(save));
        }
    }

    // ---- Blocks palette ---------------------------------------------------------

    pub(crate) fn open_blocks_palette(&mut self) {
        self.blocks_palette.get_or_insert_with(BlocksPalette::default);
    }

    /// Inserts block `name` (already in this drawing): INSERT with it chosen,
    /// placed at the cursor.
    fn insert_from_palette(&mut self, name: &str) {
        self.engine.insert_block(name, &mut self.doc);
    }

    /// Brings a library drawing's block (or the whole drawing, `name` being
    /// `None`) into this drawing, and inserts it.
    fn insert_from_library(&mut self, file: &Path, name: Option<&str>) {
        let Some(Ok(lib)) = self.library_drawings.get(file) else { return };
        let lib = lib.clone();
        self.engine.cancel(&mut self.doc);
        self.doc.begin("INSERT");
        let here = match name {
            Some(n) => self.doc.import_block(&lib, n),
            None => {
                let stem = file.file_stem().map_or("Drawing".into(), |s| s.to_string_lossy().into_owned());
                Some(self.doc.import_drawing(&lib, &sanitize_name(&stem)))
            }
        };
        self.doc.commit();
        if let Some(n) = here {
            self.insert_from_palette(&n);
        }
    }

    /// Reads a library drawing once, keeping it for later.
    fn library_drawing(&mut self, file: &Path) {
        if !self.library_drawings.contains_key(file) {
            let read = if oxidraft_io_dwg::is_dwg(file) {
                oxidraft_io_dwg::load_file(file).map(|o| o.imported.document)
            } else {
                oxidraft_io_dxf::load_file(file).map(|i| i.document).map_err(|e| e.to_string())
            };
            self.library_drawings.insert(file.to_path_buf(), read);
        }
    }

    pub(crate) fn blocks_palette(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.blocks_palette.take() else { return };
        let mut open = true;
        let mut insert: Option<String> = None;
        let mut from_library: Option<(PathBuf, Option<String>)> = None;
        let mut browse = false;
        let mut open_file: Option<Option<PathBuf>> = None;
        egui::Window::new("Blocks")
            .id(egui::Id::unique("blocks_palette"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([460.0, 420.0])
            .default_pos([300.0, 170.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut palette.library, false, "Current Drawing");
                    ui.selectable_value(&mut palette.library, true, "Libraries");
                });
                ui.separator();
                if !palette.library {
                    let names = insertable(&self.doc);
                    if names.is_empty() {
                        ui.colored_label(theme::text_dim(), "This drawing has no blocks. Create one with BLOCK.");
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for n in &names {
                                if tile(ui, &self.doc.block_contents(n), n, false).clicked() {
                                    insert = Some(n.clone());
                                }
                            }
                        });
                    });
                    return;
                }
                let folder = self.settings.block_library.clone();
                ui.horizontal(|ui| {
                    if ui.button("Browse...").on_hover_text("Pick a folder of DWG and DXF drawings").clicked() {
                        browse = true;
                    }
                    let shown = folder.as_ref().map_or("No library folder yet".into(), |f| f.display().to_string());
                    ui.add(egui::Label::new(RichText::new(shown).color(theme::text_dim())).truncate());
                });
                ui.separator();
                let Some(folder) = folder else { return };
                match palette.file.clone() {
                    None => {
                        let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
                            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
                            .unwrap_or_default();
                        files.retain(|f| {
                            f.extension()
                                .is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"))
                        });
                        files.sort();
                        if files.is_empty() {
                            ui.colored_label(theme::text_dim(), "No drawings in this folder.");
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for f in files {
                                let name = f.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                                if ui.selectable_label(false, name).clicked() {
                                    open_file = Some(Some(f));
                                }
                            }
                        });
                    }
                    Some(file) => {
                        ui.horizontal(|ui| {
                            if ui.button("< Back").clicked() {
                                open_file = Some(None);
                            }
                            let name = file.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                            ui.label(RichText::new(name).strong());
                        });
                        match self.library_drawings.get(&file) {
                            Some(Ok(lib)) => {
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        // The whole drawing, as a block of its own.
                                        let whole: Vec<Styled> =
                                            lib.entities().flat_map(|e| lib.expanded_styled(e)).collect();
                                        let stem = file
                                            .file_stem()
                                            .map_or(String::new(), |s| s.to_string_lossy().into_owned());
                                        if tile(ui, &whole, &format!("{stem} (whole drawing)"), false).clicked() {
                                            from_library = Some((file.clone(), None));
                                        }
                                        for n in insertable(lib) {
                                            if tile(ui, &lib.block_contents(&n), &n, false).clicked() {
                                                from_library = Some((file.clone(), Some(n)));
                                            }
                                        }
                                    });
                                });
                            }
                            Some(Err(e)) => {
                                ui.colored_label(theme::warning(), format!("Can't read this drawing: {e}"));
                            }
                            None => {}
                        }
                    }
                }
            });
        if browse && let Some(folder) = rfd::FileDialog::new().set_title("Block library folder").pick_folder() {
            palette.file = None;
            self.apply_settings(ctx, Settings { block_library: Some(folder), ..self.settings.clone() });
        }
        if let Some(file) = open_file {
            if let Some(f) = &file {
                self.library_drawing(f);
            }
            palette.file = file;
        }
        if open {
            self.blocks_palette = Some(palette);
        }
        if let Some(n) = insert {
            self.insert_from_palette(&n);
        }
        if let Some((file, name)) = from_library {
            self.insert_from_library(&file, name.as_deref());
        }
    }

    /// Opens `file` in the Blocks palette's Libraries tab (tests and the
    /// palette's file list).
    pub fn open_library_drawing(&mut self, folder: PathBuf, file: PathBuf, ctx: &egui::Context) {
        self.apply_settings(ctx, Settings { block_library: Some(folder), ..self.settings.clone() });
        self.library_drawing(&file);
        self.blocks_palette = Some(BlocksPalette { library: true, file: Some(file) });
    }
}

/// Library drawings read so far, by path.
pub(crate) type LibraryDrawings = HashMap<PathBuf, Result<Document, String>>;

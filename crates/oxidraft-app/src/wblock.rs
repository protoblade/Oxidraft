//! WBLOCK: AutoCAD's Write Block dialog. A block, the whole drawing, or
//! objects (kept, made a block, or deleted from the drawing) written to a
//! drawing file of their own, with what they need.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, ComboBox, Layout, RichText, vec2};
use oxidraft_commands::{block_drawing, clip_of};
use oxidraft_geom::Point;
use oxidraft_model::{Block, BlockEntity, Document, EntityId, Geometry, Placement, units};

use crate::{app::OxidraftApp, theme};

/// What WBLOCK writes.
#[derive(Debug, Clone, PartialEq)]
pub enum WblockSource {
    Block(String),
    Drawing,
    Objects(Vec<EntityId>),
}

/// What becomes of the objects written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Afterwards {
    Retain,
    ConvertToBlock,
    Delete,
}

pub(crate) struct WblockDialog {
    source: WblockSource,
    block: String,
    objects: Vec<EntityId>,
    base: Point,
    afterwards: Afterwards,
    file: String,
    insertion: i16,
    /// Hidden while objects are selected in the drawing (SELECT).
    picking: bool,
    error: Option<String>,
}

/// Writes `doc` as DWG (by its extension) or DXF.
fn write_drawing(doc: &Document, path: &Path) -> Result<(), String> {
    let dwg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg"));
    if dwg {
        oxidraft_io_dwg::save_file(doc, path, oxidraft_io_dwg::Version::default())
    } else {
        oxidraft_io_dwg::save_dxf(doc, path)
    }
}

impl OxidraftApp {
    /// WBLOCK: the objects selected beforehand are the source if there are
    /// any, else the first block.
    pub(crate) fn open_wblock(&mut self) {
        let objects = self.engine.selection().to_vec();
        let block = self.doc.blocks().find(|b| !b.name.starts_with('*')).map_or(String::new(), |b| b.name.clone());
        let source =
            if objects.is_empty() { WblockSource::Block(block.clone()) } else { WblockSource::Objects(Vec::new()) };
        let dir = self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_default();
        let file = dir.join("new block.dwg").to_string_lossy().into_owned();
        self.wblock = Some(WblockDialog {
            source,
            block,
            objects,
            base: Point::ORIGIN,
            afterwards: Afterwards::Retain,
            file,
            insertion: self.doc.units().insertion,
            picking: false,
            error: None,
        });
    }

    /// Writes `source` to `path` in `insertion` units, then does what
    /// `afterwards` says with objects written.
    pub fn write_block(
        &mut self,
        source: &WblockSource,
        base: Point,
        afterwards: Afterwards,
        insertion: i16,
        path: &Path,
    ) -> Result<(), String> {
        let mut out = match source {
            WblockSource::Block(name) => {
                block_drawing(&self.doc, name).ok_or(format!("Block \"{name}\" not found."))?
            }
            WblockSource::Drawing => self.doc.clone(),
            WblockSource::Objects(ids) if ids.is_empty() => return Err("No objects selected.".into()),
            WblockSource::Objects(ids) => clip_of(&self.doc, ids, base),
        };
        out.set_units(oxidraft_model::Units { insertion, ..out.units() });
        write_drawing(&out, path)?;
        self.engine.log(format!("Written to {}", path.display()));
        let WblockSource::Objects(ids) = source else { return Ok(()) };
        match afterwards {
            Afterwards::Retain => {}
            Afterwards::Delete => {
                self.doc.begin("WBLOCK");
                ids.iter().for_each(|id| {
                    self.doc.remove(*id);
                });
                self.doc.commit();
            }
            Afterwards::ConvertToBlock => {
                let name = path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                if self.doc.block(&name).is_some() {
                    return Err(format!("Written, but a block named \"{name}\" already exists."));
                }
                let entities: Vec<BlockEntity> = ids
                    .iter()
                    .filter_map(|id| self.doc.entity(*id))
                    .map(|e| BlockEntity {
                        layer: e.layer.clone(),
                        color: e.color,
                        linetype: e.linetype.clone(),
                        lineweight: e.lineweight,
                        geometry: e.geometry.clone(),
                    })
                    .collect();
                self.doc.begin("WBLOCK");
                self.doc.define_block(Block { name: name.clone(), base, entities });
                ids.iter().for_each(|id| {
                    self.doc.remove(*id);
                });
                let placement = Placement::at(base);
                self.doc.add(Geometry::Insert { block: name, placement, attributes: Vec::new(), clip: None });
                self.doc.commit();
            }
        }
        Ok(())
    }

    pub(crate) fn wblock_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.wblock.take() else { return };
        if d.picking {
            // Back once SELECT is done, with what it selected.
            if !self.engine.is_active() {
                d.picking = false;
                d.objects = self.engine.previous_selection().to_vec();
            }
            self.wblock = Some(d);
            return;
        }
        let (mut open, mut ok, mut cancel, mut browse, mut select) = (true, false, false, false, false);
        let mut fields = Vec::new();
        let blocks: Vec<String> =
            self.doc.blocks().filter(|b| !b.name.starts_with('*')).map(|b| b.name.clone()).collect();
        egui::Window::new("Write Block")
            .id(egui::Id::unique("wblock_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([400.0, 140.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Source").strong());
                ui.horizontal(|ui| {
                    let block = WblockSource::Block(d.block.clone());
                    if ui.radio(matches!(d.source, WblockSource::Block(_)), "Block").clicked() {
                        d.source = block;
                    }
                    if ui.radio(d.source == WblockSource::Drawing, "Entire drawing").clicked() {
                        d.source = WblockSource::Drawing;
                    }
                    if ui.radio(matches!(d.source, WblockSource::Objects(_)), "Objects").clicked() {
                        d.source = WblockSource::Objects(Vec::new());
                    }
                });
                match d.source {
                    WblockSource::Block(_) => {
                        let r = ComboBox::from_id_salt("wblock_block").width(220.0).selected_text(&d.block).show_ui(
                            ui,
                            |ui| {
                                for b in &blocks {
                                    ui.selectable_value(&mut d.block, b.clone(), b);
                                }
                            },
                        );
                        r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Block"));
                        d.source = WblockSource::Block(d.block.clone());
                    }
                    WblockSource::Objects(_) => {
                        ui.horizontal(|ui| {
                            ui.label("Base point  X:");
                            fields.push(ui.add(egui::DragValue::new(&mut d.base.x).speed(1.0)).id);
                            ui.label("Y:");
                            fields.push(ui.add(egui::DragValue::new(&mut d.base.y).speed(1.0)).id);
                        });
                        ui.horizontal(|ui| {
                            select = ui.button("Select objects").clicked();
                            ui.label(format!("{} objects selected", d.objects.len()));
                        });
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut d.afterwards, Afterwards::Retain, "Retain");
                            ui.radio_value(&mut d.afterwards, Afterwards::ConvertToBlock, "Convert to block");
                            ui.radio_value(&mut d.afterwards, Afterwards::Delete, "Delete from drawing");
                        });
                    }
                    WblockSource::Drawing => {
                        ui.colored_label(theme::text_dim(), "Everything in the drawing, as it is.");
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Destination").strong());
                ui.label("File name and path:");
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut d.file).desired_width(300.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "File name and path"));
                    fields.push(r.id);
                    browse = ui.button("...").on_hover_text("Choose where the drawing goes").clicked();
                });
                ui.horizontal(|ui| {
                    ui.label("Insert units:");
                    ComboBox::from_id_salt("wblock_units")
                        .width(160.0)
                        .selected_text(units::insertion_name(d.insertion))
                        .show_ui(ui, |ui| {
                            for (code, name, _) in units::INSERTION_UNITS {
                                ui.selectable_value(&mut d.insertion, *code, *name);
                            }
                        });
                });
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if browse {
            let current = PathBuf::from(&d.file);
            let mut dialog = rfd::FileDialog::new().add_filter("Drawing", &["dwg", "dxf"]);
            if let Some(dir) = current.parent().filter(|p| p.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = current.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
            if let Some(path) = dialog.save_file() {
                d.file = path.to_string_lossy().into_owned();
            }
        }
        if select {
            d.picking = true;
            self.run("SELECT");
            self.wblock = Some(d);
            return;
        }
        if ok {
            let source = match &d.source {
                WblockSource::Objects(_) => WblockSource::Objects(d.objects.clone()),
                s => s.clone(),
            };
            match self.write_block(&source, d.base, d.afterwards, d.insertion, Path::new(d.file.trim())) {
                Ok(()) => return,
                Err(e) => d.error = Some(e),
            }
        }
        if open && !cancel {
            self.wblock = Some(d);
        }
    }
}

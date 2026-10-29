//! BATTMAN: AutoCAD's Block Attribute Manager — a block's attribute
//! definitions, in order: their tag, prompt, default and modes edited,
//! moved up or down or removed, and the block's references brought in
//! line (Sync, or on Apply).

use eframe::egui::{self, Align, ComboBox, Layout, RichText, vec2};
use oxidraft_commands::sync_attributes;
use oxidraft_model::{Block, Geometry};

use crate::{app::OxidraftApp, theme};

/// One attribute definition as edited: (tag, prompt, default, invisible,
/// constant), and which of the block's entities it is.
#[derive(Clone, PartialEq)]
struct Definition {
    index: usize,
    tag: String,
    prompt: String,
    default: String,
    invisible: bool,
    constant: bool,
}

pub(crate) struct AttributeManager {
    block: String,
    definitions: Vec<Definition>,
    selected: Option<usize>,
    /// Apply changes to the references already placed.
    sync: bool,
    changed: bool,
}

fn definitions(block: &Block) -> Vec<Definition> {
    block
        .entities
        .iter()
        .enumerate()
        .filter_map(|(index, be)| match &be.geometry {
            Geometry::AttributeDefinition { tag, prompt, default, invisible, constant, .. } => Some(Definition {
                index,
                tag: tag.clone(),
                prompt: prompt.clone(),
                default: default.clone(),
                invisible: *invisible,
                constant: *constant,
            }),
            _ => None,
        })
        .collect()
}

impl OxidraftApp {
    fn blocks_with_attributes(&self) -> Vec<String> {
        self.doc
            .blocks()
            .filter(|b| !b.name.starts_with('*') && !definitions(b).is_empty())
            .map(|b| b.name.clone())
            .collect()
    }

    pub(crate) fn open_attribute_manager(&mut self) {
        let Some(block) = self.blocks_with_attributes().into_iter().next() else {
            self.engine.log("This drawing has no blocks with attributes.");
            return;
        };
        let defs = self.doc.block(&block).map(definitions).unwrap_or_default();
        self.attribute_manager =
            Some(AttributeManager { block, definitions: defs, selected: None, sync: true, changed: false });
    }

    /// The block's definition with the attributes as edited, in their new order.
    fn apply_attribute_changes(&mut self, m: &AttributeManager) {
        let Some(mut block) = self.doc.block(&m.block).cloned() else { return };
        let mut attdefs = Vec::new();
        for d in &m.definitions {
            let Some(be) = block.entities.get(d.index) else { continue };
            let mut be = be.clone();
            if let Geometry::AttributeDefinition { tag, prompt, default, invisible, constant, .. } = &mut be.geometry {
                (*tag, *prompt, *default) = (d.tag.trim().to_uppercase(), d.prompt.clone(), d.default.clone());
                (*invisible, *constant) = (d.invisible, d.constant);
            }
            attdefs.push(be);
        }
        // The other objects as they were, the attributes in their new order after them.
        block.entities.retain(|be| !matches!(be.geometry, Geometry::AttributeDefinition { .. }));
        block.entities.extend(attdefs);
        self.doc.begin("BATTMAN");
        self.doc.define_block(block);
        let synced = if m.sync { sync_attributes(&mut self.doc, &m.block) } else { 0 };
        self.doc.commit();
        if m.sync {
            self.engine.log(format!("{synced} reference(s) of \"{}\" updated.", m.block));
        }
    }

    pub(crate) fn attribute_manager_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut m) = self.attribute_manager.take() else { return };
        let (mut open, mut ok, mut apply, mut cancel, mut sync_now) = (true, false, false, false, false);
        let mut fields = Vec::new();
        let blocks = self.blocks_with_attributes();
        let mut switch_to = None;
        egui::Window::new("Block Attribute Manager")
            .id(egui::Id::unique("battman"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([360.0, 130.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Block:");
                    let r = ComboBox::from_id_salt("battman_block").width(180.0).selected_text(&m.block).show_ui(
                        ui,
                        |ui| {
                            for b in &blocks {
                                if ui.selectable_label(*b == m.block, b).clicked() {
                                    switch_to = Some(b.clone());
                                }
                            }
                        },
                    );
                    r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Block"));
                    sync_now = ui.button("Sync").on_hover_text("Brings the block's references in line now").clicked();
                });
                ui.horizontal_top(|ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_min_size(vec2(360.0, 160.0));
                        egui::Grid::new("battman_rows").num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
                            for h in ["Tag", "Prompt", "Default", "Modes"] {
                                ui.label(RichText::new(h).strong());
                            }
                            ui.end_row();
                            for (i, d) in m.definitions.iter().enumerate() {
                                if ui.selectable_label(m.selected == Some(i), &d.tag).clicked() {
                                    m.selected = Some(i);
                                }
                                ui.label(&d.prompt);
                                ui.label(&d.default);
                                let modes = [(d.invisible, "I"), (d.constant, "C")]
                                    .iter()
                                    .filter(|m| m.0)
                                    .map(|m| m.1)
                                    .collect::<Vec<_>>()
                                    .join("");
                                ui.label(modes);
                                ui.end_row();
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        let size = vec2(90.0, 22.0);
                        let n = m.definitions.len();
                        let picked = m.selected.filter(|i| *i < n);
                        if ui
                            .add_enabled(picked.is_some_and(|i| i > 0), egui::Button::new("Move Up").min_size(size))
                            .clicked()
                            && let Some(i) = picked
                        {
                            m.definitions.swap(i, i - 1);
                            m.selected = Some(i - 1);
                            m.changed = true;
                        }
                        if ui
                            .add_enabled(
                                picked.is_some_and(|i| i + 1 < n),
                                egui::Button::new("Move Down").min_size(size),
                            )
                            .clicked()
                            && let Some(i) = picked
                        {
                            m.definitions.swap(i, i + 1);
                            m.selected = Some(i + 1);
                            m.changed = true;
                        }
                        if ui.add_enabled(picked.is_some(), egui::Button::new("Remove").min_size(size)).clicked()
                            && let Some(i) = picked
                        {
                            m.definitions.remove(i);
                            m.selected = None;
                            m.changed = true;
                        }
                    });
                });
                // The one picked, to edit.
                if let Some(d) = m.selected.and_then(|i| m.definitions.get_mut(i)) {
                    ui.label(RichText::new("Edit Attribute").strong());
                    let before = d.clone();
                    egui::Grid::new("battman_edit").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                        for (label, value) in
                            [("Tag:", &mut d.tag), ("Prompt:", &mut d.prompt), ("Default:", &mut d.default)]
                        {
                            ui.label(label);
                            let r = ui.add(egui::TextEdit::singleline(value).desired_width(220.0));
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                            fields.push(r.id);
                            ui.end_row();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut d.invisible, "Invisible");
                        ui.checkbox(&mut d.constant, "Constant");
                    });
                    m.changed |= *d != before;
                } else {
                    ui.colored_label(theme::text_dim(), "Pick an attribute to edit it.");
                }
                ui.checkbox(&mut m.sync, "Apply changes to existing references");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    apply = ui.add_enabled(m.changed, egui::Button::new("Apply")).clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if (apply || ok) && m.changed {
            self.apply_attribute_changes(&m);
            m.changed = false;
            m.definitions = self.doc.block(&m.block).map(definitions).unwrap_or_default();
        }
        if sync_now {
            self.doc.begin("ATTSYNC");
            let n = sync_attributes(&mut self.doc, &m.block);
            self.doc.commit();
            self.engine.log(format!("ATTSYNC complete: {n} reference(s) of \"{}\".", m.block));
        }
        if let Some(b) = switch_to {
            m.definitions = self.doc.block(&b).map(definitions).unwrap_or_default();
            (m.block, m.selected, m.changed) = (b, None, false);
        }
        if open && !ok && !cancel {
            self.attribute_manager = Some(m);
        }
    }
}

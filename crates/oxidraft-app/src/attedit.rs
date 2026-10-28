//! The Enhanced Attribute Editor (EATTEDIT, or double-click a block with
//! attributes): edits a block reference's attribute values.

use eframe::egui::{self, Key, RichText};
use oxidraft_model::{EntityId, Geometry};

use crate::{app::OxidraftApp, theme};

/// The values being edited, applied on Apply or OK.
pub(crate) struct AttributeEditor {
    id: EntityId,
    values: Vec<String>,
}

impl OxidraftApp {
    /// Opens the editor on a block reference, if it has attributes.
    pub(crate) fn edit_attributes(&mut self, id: EntityId) -> bool {
        let Some(Geometry::Insert { attributes, .. }) = self.doc.entity(id).map(|e| &e.geometry) else {
            return false;
        };
        if attributes.is_empty() {
            return false;
        }
        let values = attributes.iter().map(|a| a.value.clone()).collect();
        self.attribute_editor = Some(AttributeEditor { id, values });
        true
    }

    pub(crate) fn attribute_editor(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.attribute_editor.take() else { return };
        // The block may have been erased since.
        let Some((block, tags)) = self.doc.entity(editor.id).and_then(|e| match &e.geometry {
            Geometry::Insert { block, attributes, .. } => {
                Some((block.clone(), attributes.iter().map(|a| a.tag.clone()).collect::<Vec<_>>()))
            }
            _ => None,
        }) else {
            return;
        };
        let (mut open, mut ok, mut apply, mut cancel) = (true, false, false, false);
        egui::Window::new("Enhanced Attribute Editor")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([360.0, 200.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("Block: {block}")).strong());
                ui.separator();
                egui::Grid::new("attedit").num_columns(2).striped(true).spacing([16.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new("Tag").color(theme::TEXT_DIM));
                    ui.label(RichText::new("Value").color(theme::TEXT_DIM));
                    ui.end_row();
                    for (i, (tag, value)) in tags.iter().zip(editor.values.iter_mut()).enumerate() {
                        ui.label(tag);
                        let id = egui::Id::unique(("attribute_value", i));
                        let field = ui.add(egui::TextEdit::singleline(value).id(id).desired_width(240.0));
                        field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, tag));
                        self.text_fields.push(id);
                        // Enter in a value is OK, as in AutoCAD's dialog.
                        ok |= field.lost_focus() && ui.input(|inp| inp.key_pressed(Key::Enter));
                        ui.end_row();
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ok |= ui.button("OK").clicked();
                    apply = ui.button("Apply").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if ok || apply {
            self.doc.begin("EATTEDIT");
            self.doc.update(editor.id, |e| {
                if let Geometry::Insert { attributes, .. } = &mut e.geometry {
                    for (a, v) in attributes.iter_mut().zip(&editor.values) {
                        a.value.clone_from(v);
                    }
                }
            });
            self.doc.commit();
        }
        if open && !ok && !cancel {
            self.attribute_editor = Some(editor);
        }
    }
}

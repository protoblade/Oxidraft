//! The Manage tab's dialogs: PURGE's list of what nothing uses,
//! ALIASEDIT's command aliases, and RENAME's named objects.

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_commands::{COMMANDS, Engine, Named};
use oxidraft_model::{EntityId, Geometry, Unused};

use crate::{app::OxidraftApp, theme};

/// PURGE: each unused item, ticked to go.
pub(crate) struct PurgeDialog {
    items: Vec<(Kind, String, bool)>,
    nested: bool,
    zero_length: bool,
    empty_text: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Block,
    Layer,
    Linetype,
    TextStyle,
}

impl Kind {
    fn heading(self) -> &'static str {
        match self {
            Kind::Block => "Blocks",
            Kind::Layer => "Layers",
            Kind::Linetype => "Linetypes",
            Kind::TextStyle => "Text styles",
        }
    }
}

/// ALIASEDIT: the user's aliases as they're being edited, and the one
/// being added.
pub(crate) struct AliasDialog {
    aliases: Vec<(String, String)>,
    alias: String,
    command: String,
}

/// RENAME: the kind of named object, the one picked, the new name, and
/// what went wrong last.
pub(crate) struct RenameDialog {
    kind: Named,
    old: Option<String>,
    new: String,
    error: Option<String>,
}

impl OxidraftApp {
    pub(crate) fn open_rename(&mut self) {
        self.rename = Some(RenameDialog { kind: Named::Block, old: None, new: String::new(), error: None });
    }

    pub(crate) fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.rename.take() else { return };
        let (mut open, mut ok, mut cancel, mut rename) = (true, false, false, false);
        let mut fields = Vec::new();
        let names = d.kind.names(&self.doc);
        egui::Window::new("Rename")
            .id(egui::Id::unique("rename_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Named Objects").strong());
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(vec2(130.0, 200.0));
                            for kind in Named::ALL {
                                if ui.selectable_label(d.kind == kind, kind.plural()).clicked() && d.kind != kind {
                                    (d.kind, d.old) = (kind, None);
                                    d.new.clear();
                                }
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Items").strong());
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(vec2(200.0, 200.0));
                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                if names.is_empty() {
                                    ui.colored_label(theme::text_dim(), "None to rename.");
                                }
                                for n in &names {
                                    let picked = d.old.as_ref().is_some_and(|o| o == n);
                                    if ui.selectable_label(picked, n).clicked() {
                                        d.old = Some(n.clone());
                                        d.new = n.clone();
                                    }
                                }
                            });
                        });
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Old Name:");
                    ui.label(d.old.as_deref().unwrap_or(""));
                });
                ui.horizontal(|ui| {
                    let ready = d.old.is_some() && !d.new.trim().is_empty();
                    rename = ui.add_enabled(ready, egui::Button::new("Rename To:")).clicked();
                    let r = ui.add(egui::TextEdit::singleline(&mut d.new).desired_width(220.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "New name"));
                    rename |= ready && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    fields.push(r.id);
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
        if rename && let Some(old) = d.old.clone() {
            self.doc.begin("RENAME");
            match d.kind.rename(&mut self.doc, &old, &d.new) {
                Ok(()) => {
                    self.doc.commit();
                    d.old = Some(d.new.trim().to_string());
                    d.error = None;
                }
                Err(e) => {
                    self.doc.rollback();
                    d.error = Some(e);
                }
            }
        }
        if open && !ok && !cancel {
            self.rename = Some(d);
        }
    }

    pub(crate) fn open_purge(&mut self) {
        let unused = self.doc.unused();
        let mut items: Vec<(Kind, String, bool)> = Vec::new();
        items.extend(unused.blocks.into_iter().map(|n| (Kind::Block, n, true)));
        items.extend(unused.layers.into_iter().map(|n| (Kind::Layer, n, true)));
        items.extend(unused.linetypes.into_iter().map(|n| (Kind::Linetype, n, true)));
        items.extend(unused.text_styles.into_iter().map(|n| (Kind::TextStyle, n, true)));
        self.purge = Some(PurgeDialog { items, nested: true, zero_length: false, empty_text: false });
    }

    pub(crate) fn purge_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.purge.take() else { return };
        let (mut open, mut purge, mut all, mut close) = (true, false, false, false);
        egui::Window::new("Purge")
            .id(egui::Id::unique("purge"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Unused named items").strong());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(300.0);
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        if d.items.is_empty() {
                            ui.colored_label(theme::text_dim(), "Nothing unused to purge.");
                        }
                        for kind in [Kind::Block, Kind::Layer, Kind::Linetype, Kind::TextStyle] {
                            if !d.items.iter().any(|i| i.0 == kind) {
                                continue;
                            }
                            ui.label(RichText::new(kind.heading()).color(theme::accent()));
                            for (_, name, ticked) in d.items.iter_mut().filter(|i| i.0 == kind) {
                                ui.checkbox(ticked, name.as_str());
                            }
                        }
                    });
                });
                ui.checkbox(&mut d.nested, "Purge nested items");
                ui.label(RichText::new("Unnamed objects").strong());
                ui.checkbox(&mut d.zero_length, "Purge zero-length geometry");
                ui.checkbox(&mut d.empty_text, "Purge empty text objects");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                    all = ui.button("Purge All").clicked();
                    purge = ui.button("Purge").clicked();
                });
            });
        if purge || all {
            let picked = |kind: Kind| -> Vec<String> {
                d.items.iter().filter(|i| i.0 == kind && (all || i.2)).map(|i| i.1.clone()).collect()
            };
            let what = Unused {
                blocks: picked(Kind::Block),
                layers: picked(Kind::Layer),
                linetypes: picked(Kind::Linetype),
                text_styles: picked(Kind::TextStyle),
            };
            self.doc.begin("PURGE");
            let mut count = self.doc.purge(&what, d.nested);
            let empty: Vec<EntityId> = self
                .doc
                .empty_objects()
                .into_iter()
                .filter(|id| {
                    let text = matches!(self.doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Text { .. }));
                    if text { d.empty_text } else { d.zero_length }
                })
                .collect();
            for id in &empty {
                count += usize::from(self.doc.remove(*id));
            }
            self.doc.commit();
            self.engine.log(format!("{count} item(s) purged."));
            // What's left to purge now.
            self.open_purge();
            if let Some(next) = &mut self.purge {
                (next.nested, next.zero_length, next.empty_text) = (d.nested, d.zero_length, d.empty_text);
            }
            return;
        }
        if open && !close {
            self.purge = Some(d);
        }
    }

    pub(crate) fn open_aliases(&mut self) {
        let aliases = self.settings.aliases.clone();
        self.alias_dialog = Some(AliasDialog { aliases, alias: String::new(), command: String::new() });
    }

    pub(crate) fn alias_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.alias_dialog.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Edit Aliases")
            .id(egui::Id::unique("alias_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.colored_label(theme::text_dim(), "Your own shortcuts: typing the alias runs the command.");
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(300.0);
                    let mut remove = None;
                    egui::Grid::new("aliases").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                        for (i, (alias, command)) in d.aliases.iter().enumerate() {
                            ui.label(RichText::new(alias).monospace());
                            ui.label(command);
                            if ui.small_button("Delete").clicked() {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                    if d.aliases.is_empty() {
                        ui.colored_label(theme::text_dim(), "None yet.");
                    }
                    if let Some(i) = remove {
                        d.aliases.remove(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Alias:");
                    let a = ui.add(egui::TextEdit::singleline(&mut d.alias).desired_width(60.0));
                    a.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Alias"));
                    ui.label("Command:");
                    let c = ui.add(egui::TextEdit::singleline(&mut d.command).desired_width(120.0));
                    c.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Command"));
                    fields.extend([a.id, c.id]);
                    let alias = d.alias.trim().to_uppercase();
                    let command = d.command.trim().to_uppercase();
                    let known = COMMANDS.iter().any(|c| c.name == command)
                        || Engine::suggest(&command).first().is_some_and(|c| c.name == command);
                    let ready = !alias.is_empty() && !alias.contains(' ') && known;
                    if ui.add_enabled(ready, egui::Button::new("Add")).clicked() {
                        d.aliases.retain(|(a, _)| *a != alias);
                        d.aliases.push((alias, command));
                        d.aliases.sort();
                        d.alias.clear();
                        d.command.clear();
                    }
                });
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            let settings = crate::settings::Settings { aliases: d.aliases.clone(), ..self.settings.clone() };
            self.apply_settings(ctx, settings);
        }
        if open && !ok && !cancel {
            self.alias_dialog = Some(d);
        }
    }
}

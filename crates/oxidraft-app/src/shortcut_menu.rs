//! The right-click shortcut menu in the drawing area, which changes with what
//! you're doing, like AutoCAD's Default, Edit and Command menus.

use eframe::egui::{self, Ui};
use oxidraft_commands::ViewRequest;
use oxidraft_model::{Geometry, snap::SnapKind};

use crate::app::OxidraftApp;

/// How many entries Recent Input lists.
const RECENT: usize = 8;

fn item(ui: &mut Ui, label: impl Into<String>, shortcut: &str) -> bool {
    item_enabled(ui, true, label, shortcut)
}

fn item_enabled(ui: &mut Ui, enabled: bool, label: impl Into<String>, shortcut: &str) -> bool {
    let mut button = egui::Button::new(label.into());
    if !shortcut.is_empty() {
        button = button.shortcut_text(shortcut);
    }
    ui.add_enabled(enabled, button).clicked()
}

impl OxidraftApp {
    pub(crate) fn shortcut_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(210.0);
        if self.engine.is_active() {
            self.command_menu(ui);
        } else if !self.engine.selection().is_empty() {
            self.edit_menu(ui);
        } else {
            self.default_menu(ui);
        }
        // Commands of your own (CUI), when no command runs.
        if !self.engine.is_active() && !self.settings.shortcut_menu.is_empty() {
            ui.separator();
            for c in self.settings.shortcut_menu.clone() {
                if ui.button(self.button_label(&c)).clicked() {
                    self.run(&c);
                    ui.close();
                }
            }
        }
    }

    /// Repeat <last command> and Recent Input, shared by the idle menus.
    fn repeat_items(&mut self, ui: &mut Ui) {
        let last = self.engine.last_command();
        if item_enabled(ui, last.is_some(), format!("Repeat {}", last.unwrap_or("")), "Enter")
            && let Some(name) = last
        {
            self.run(name);
        }
        let recent = self.engine.recent_commands(RECENT);
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Recent Input", |ui| {
                for name in recent {
                    if ui.button(name).clicked() {
                        self.run(name);
                    }
                }
            });
        });
    }

    /// Cut, copy and paste; cutting and copying need a selection.
    fn clipboard_items(&mut self, ui: &mut Ui, selected: bool) {
        ui.menu_button("Clipboard", |ui| {
            for (label, command, shortcut, needs_selection) in [
                ("Cut", "CUTCLIP", "Cmd+X", true),
                ("Copy", "COPYCLIP", "Cmd+C", true),
                ("Copy with Base Point", "COPYBASE", "Shift+Cmd+C", true),
                ("Paste", "PASTECLIP", "Cmd+V", false),
                ("Paste to Original Coordinates", "PASTEORIG", "", false),
            ] {
                if item_enabled(ui, selected || !needs_selection, label, shortcut) {
                    self.run(command);
                }
            }
        });
    }

    /// Nothing selected, no command running.
    fn default_menu(&mut self, ui: &mut Ui) {
        self.repeat_items(ui);
        self.clipboard_items(ui, false);
        ui.separator();
        let undo = self.doc.undo_name().map(str::to_string);
        if item_enabled(ui, undo.is_some(), format!("Undo {}", undo.unwrap_or_default()), "Cmd+Z") {
            self.run("UNDO");
        }
        let redo = self.doc.redo_name().map(str::to_string);
        if item_enabled(ui, redo.is_some(), format!("Redo {}", redo.unwrap_or_default()), "Shift+Cmd+Z") {
            self.run("REDO");
        }
        ui.separator();
        self.view_items(ui, false);
        if item(ui, "Select All", "Cmd+A") {
            self.engine.select_all(&self.doc);
        }
        if item(ui, "Quick Select...", "") {
            self.open_quick_select();
        }
        ui.separator();
        if item(ui, "Layer Properties...", "LA") {
            self.layer_manager_open = true;
        }
        if item(ui, "Properties", "Cmd+1") {
            self.properties_open = true;
        }
    }

    /// Objects selected, no command running.
    fn edit_menu(&mut self, ui: &mut Ui) {
        self.repeat_items(ui);
        self.clipboard_items(ui, true);
        ui.separator();
        for (label, command, alias) in [
            ("Erase", "ERASE", "Del"),
            ("Move", "MOVE", "M"),
            ("Copy Selection", "COPY", "CO"),
            ("Rotate", "ROTATE", "RO"),
            ("Scale", "SCALE", "SC"),
            ("Mirror", "MIRROR", "MI"),
        ] {
            if item(ui, label, alias) {
                self.run(command);
            }
        }
        ui.menu_button("Draw Order", |ui| {
            for (label, option) in [
                ("Bring to Front", "Front"),
                ("Send to Back", "Back"),
                ("Bring Above Objects", "Above objects"),
                ("Send Under Objects", "Under objects"),
            ] {
                if item(ui, label, "") {
                    self.engine.draw_order(option, &mut self.doc);
                }
            }
        });
        // A selected xref (or block reference) can be clipped from here.
        let reference = self.engine.selection().iter().find_map(|id| match &self.doc.entity(*id)?.geometry {
            Geometry::Insert { block, .. } => Some(self.doc.xref(block).is_some()),
            _ => None,
        });
        if let Some(xref) = reference {
            if item(ui, if xref { "Edit Xref In-place" } else { "Edit Block In-place" }, "") {
                self.engine.refedit_selected(&mut self.doc);
            }
            if item(ui, if xref { "Clip Xref" } else { "Clip Block Reference" }, "XC") {
                self.run("XCLIP");
            }
        }
        ui.separator();
        if item(ui, "Select Similar", "") {
            self.run("SELECTSIMILAR");
        }
        if item(ui, "Deselect All", "Esc") {
            self.engine.cancel(&mut self.doc);
        }
        if item(ui, "Quick Select...", "") {
            self.open_quick_select();
        }
        ui.separator();
        if item(ui, "Properties", "Cmd+1") {
            self.properties_open = true;
        }
    }

    /// A command is running: Enter, Cancel, its options and snap overrides.
    fn command_menu(&mut self, ui: &mut Ui) {
        if item(ui, "Enter", "") {
            self.command_text.clear();
            self.submit();
        }
        if item(ui, "Cancel", "Esc") {
            self.command_text.clear();
            self.snap_override = None;
            self.engine.cancel(&mut self.doc);
            return;
        }
        let recent = self.engine.recent_inputs(RECENT);
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Recent Input", |ui| {
                for input in recent {
                    if ui.button(&input).clicked() {
                        self.type_command(&input);
                    }
                }
            });
        });
        let keywords = self.engine.current_prompt().map(|p| p.keywords).unwrap_or_default();
        if !keywords.is_empty() {
            ui.separator();
            for k in keywords {
                if ui.button(k).clicked() {
                    self.type_command(k);
                }
            }
        }
        ui.separator();
        // Snap overrides apply to points, not to picking objects.
        let points = !self.engine.is_selecting() && !self.engine.is_picking();
        ui.add_enabled_ui(points, |ui| {
            ui.menu_button("Snap Overrides", |ui| {
                for kind in SnapKind::ALL {
                    if ui.button(kind.label()).clicked() {
                        self.snap_override = Some(Some(kind));
                        self.engine.log(format!("{} of", kind.label()));
                    }
                }
                ui.separator();
                if ui.button("None").clicked() {
                    self.snap_override = Some(None);
                    self.engine.log("None of");
                }
            });
        });
        self.view_items(ui, true);
    }

    /// Pan and the Zoom submenu. While a command runs, only the zooms that
    /// need no picking are offered, so the command carries on afterwards.
    pub(crate) fn view_items(&mut self, ui: &mut Ui, during_command: bool) {
        if item(ui, "Pan", "P") {
            self.pending_views.push(ViewRequest::RealtimePan);
        }
        ui.menu_button("Zoom", |ui| self.zoom_items(ui, during_command));
    }

    /// The zoom options, for the Zoom submenu and the navigation bar.
    pub(crate) fn zoom_items(&mut self, ui: &mut Ui, during_command: bool) {
        ui.set_min_width(170.0);
        let mut zoom = |ui: &mut Ui, label: &str, shortcut: &str, request: ViewRequest| {
            if item(ui, label, shortcut) {
                self.pending_views.push(request);
            }
        };
        zoom(ui, "Realtime", "", ViewRequest::RealtimeZoom);
        zoom(ui, "Previous", "Z P", ViewRequest::Previous);
        zoom(ui, "Extents", "ZE", ViewRequest::Extents);
        zoom(ui, "In", "Z 2X", ViewRequest::Magnify(2.0));
        zoom(ui, "Out", "Z .5X", ViewRequest::Magnify(0.5));
        if !during_command {
            ui.separator();
            for (label, option) in [("Window", "W"), ("Object", "O"), ("Center", "C")] {
                if item(ui, label, &format!("Z {option}")) {
                    self.run("ZOOM");
                    self.type_command(option);
                }
            }
        }
    }
}

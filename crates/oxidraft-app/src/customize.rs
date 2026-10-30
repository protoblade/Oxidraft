//! Workspaces and CUI, as AutoCAD has them. A workspace is a named
//! arrangement of the palettes and the ribbon (WORKSPACE, WSSAVE,
//! WSCURRENT, the status bar's Workspace menu); "Drafting & Annotation" is
//! always there. CUI (Customize User Interface) adds keyboard shortcuts of
//! your own and commands on the Quick Access Toolbar. Both are kept in the
//! settings.

use eframe::egui::{self, Align, Key, Layout, Modifiers, RichText, vec2};
use serde::{Deserialize, Serialize};

use crate::app::OxidraftApp;
use crate::settings::Settings;
use crate::theme;

/// AutoCAD's own workspace, which can't be deleted.
pub const DRAFTING: &str = "Drafting & Annotation";

/// A named arrangement: which palettes are open, the ribbon's tab, and
/// whether the screen is clean.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub name: String,
    pub properties: bool,
    pub layers: bool,
    pub tool_palettes: bool,
    pub design_center: bool,
    pub quickcalc: bool,
    pub xrefs: bool,
    pub clean_screen: bool,
    pub ribbon_tab: usize,
}

impl Workspace {
    fn drafting() -> Self {
        Workspace { name: DRAFTING.into(), ..Default::default() }
    }
}

/// The built-in keyboard shortcuts, as the CUI lists them.
const BUILT_IN: &[(&str, &str)] = &[
    ("Cmd+N", "NEW"),
    ("Cmd+O", "OPEN"),
    ("Cmd+S", "SAVE"),
    ("Cmd+P", "PLOT"),
    ("Cmd+Z", "UNDO"),
    ("Shift+Cmd+Z", "REDO"),
    ("Cmd+A", "SELECTALL"),
    ("Cmd+C", "COPYCLIP"),
    ("Cmd+X", "CUTCLIP"),
    ("Cmd+V", "PASTECLIP"),
    ("Cmd+1", "PROPERTIES"),
    ("Cmd+2", "ADCENTER"),
    ("Cmd+8", "QUICKCALC"),
    ("Cmd+K", "HYPERLINK"),
    ("Ctrl+Tab", "SWITCHWINDOWS"),
    ("Ctrl+R", "next viewport"),
    ("Ctrl+W", "selection cycling"),
    ("Ctrl+Shift+A", "group selection"),
    ("F3", "OSNAP on/off"),
    ("F7", "GRID on/off"),
    ("F8", "ORTHO on/off"),
    ("F9", "SNAP on/off"),
    ("F10", "POLAR on/off"),
    ("F12", "Dynamic Input on/off"),
];

/// A shortcut's keys ("Ctrl+Shift+L", "Cmd+F5", "F2"): its modifiers and
/// key, or `None` when it doesn't name one.
pub fn parse_shortcut(text: &str) -> Option<(Modifiers, Key)> {
    let mut mods = Modifiers::NONE;
    let parts: Vec<&str> = text.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let (key, rest) = parts.split_last()?;
    for m in rest {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= Modifiers::CTRL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" | "opt" => mods |= Modifiers::ALT,
            "cmd" | "command" => mods |= Modifiers::COMMAND,
            _ => return None,
        }
    }
    let key = Key::from_name(key).or_else(|| Key::from_name(&key.to_ascii_uppercase()))?;
    Some((mods, key))
}

/// CUI's dialog: the shortcuts and toolbar commands being edited.
pub(crate) struct CuiDialog {
    shortcuts: Vec<(String, String)>,
    quick_access: Vec<String>,
    keys: String,
    command: String,
    toolbar_command: String,
    panels: Vec<crate::settings::RibbonPanel>,
    /// A panel being added: its tab, its title, its commands.
    panel: (String, String, String),
    shortcut_menu: Vec<String>,
    menu_command: String,
    commands: Vec<crate::settings::CustomCommand>,
    /// A command being made: its name, macro and image file.
    new_command: (String, String, String),
}

/// What names a command made in CUI: its name, upper case, spaces as `_`.
pub fn command_id(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("_").to_ascii_uppercase()
}

impl OxidraftApp {
    /// Every workspace: AutoCAD's, then those saved.
    pub fn workspaces(&self) -> Vec<Workspace> {
        let mut all = vec![Workspace::drafting()];
        all.extend(self.settings.workspaces.iter().filter(|w| w.name != DRAFTING).cloned());
        all
    }

    /// The current workspace's name (WSCURRENT).
    pub fn current_workspace(&self) -> String {
        if self.settings.workspace.is_empty() { DRAFTING.into() } else { self.settings.workspace.clone() }
    }

    /// The palettes and ribbon as they are now, as a workspace named `name`.
    fn arrangement(&self, name: &str) -> Workspace {
        Workspace {
            name: name.into(),
            properties: self.properties_open,
            layers: self.layer_manager_open,
            tool_palettes: self.tool_palettes.is_some(),
            design_center: self.design_center.is_some(),
            quickcalc: self.quickcalc.is_some(),
            xrefs: self.xref_palette_open,
            clean_screen: self.clean_screen,
            ribbon_tab: self.ribbon_tab,
        }
    }

    /// Makes workspace `name` current: its palettes open, the others
    /// closed, its ribbon tab.
    pub fn set_workspace(&mut self, name: &str) -> Result<(), String> {
        let w = self
            .workspaces()
            .into_iter()
            .find(|w| w.name.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| format!("Workspace \"{}\" not found.", name.trim()))?;
        self.properties_open = w.properties;
        self.layer_manager_open = w.layers;
        self.xref_palette_open = w.xrefs;
        self.clean_screen = w.clean_screen;
        self.ribbon_tab = w.ribbon_tab;
        if w.tool_palettes != self.tool_palettes.is_some() {
            self.toggle_tool_palettes();
        }
        if w.design_center != self.design_center.is_some() {
            self.toggle_design_center();
        }
        if w.quickcalc != self.quickcalc.is_some() {
            self.toggle_quickcalc();
        }
        let settings = Settings { workspace: w.name.clone(), ..self.settings.clone() };
        self.keep_settings(settings);
        Ok(())
    }

    /// WSSAVE: the arrangement as it is now, saved as workspace `name`
    /// (replacing one of that name), and current.
    pub fn save_workspace(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A workspace needs a name.".into());
        }
        let w = self.arrangement(name);
        let mut settings = self.settings.clone();
        settings.workspaces.retain(|x| !x.name.eq_ignore_ascii_case(name));
        settings.workspaces.push(w);
        settings.workspace = name.into();
        self.keep_settings(settings);
        Ok(())
    }

    /// WORKSPACE's options: setCurrent, SAveas, Rename, Delete, or ? to list.
    pub fn workspace_option(&mut self, option: &str, name: &str, new_name: &str) {
        let done = match option {
            "SAveas" => self.save_workspace(name).map(|()| format!("Workspace \"{}\" saved.", name.trim())),
            "Rename" => self.rename_workspace(name, new_name),
            "Delete" => self.delete_workspace(name),
            "?" => {
                let current = self.current_workspace();
                let names: Vec<String> = self
                    .workspaces()
                    .iter()
                    .map(|w| if w.name == current { format!("{} (current)", w.name) } else { w.name.clone() })
                    .collect();
                Ok(format!("Workspaces: {}", names.join(", ")))
            }
            _ => self.set_workspace(name).map(|()| format!("Workspace \"{}\" is current.", name.trim())),
        };
        match done {
            Ok(m) | Err(m) => self.engine.log(m),
        }
    }

    fn rename_workspace(&mut self, old: &str, new: &str) -> Result<String, String> {
        let (old, new) = (old.trim(), new.trim());
        if old.eq_ignore_ascii_case(DRAFTING) {
            return Err(format!("\"{DRAFTING}\" can't be renamed."));
        }
        if new.is_empty() || self.workspaces().iter().any(|w| w.name.eq_ignore_ascii_case(new)) {
            return Err(format!("\"{new}\" can't be used: empty or taken."));
        }
        let mut settings = self.settings.clone();
        let w = settings.workspaces.iter_mut().find(|w| w.name.eq_ignore_ascii_case(old)).ok_or("Not found.")?;
        w.name = new.into();
        if settings.workspace.eq_ignore_ascii_case(old) {
            settings.workspace = new.into();
        }
        self.keep_settings(settings);
        Ok(format!("Workspace \"{old}\" renamed \"{new}\"."))
    }

    fn delete_workspace(&mut self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.eq_ignore_ascii_case(DRAFTING) || name.eq_ignore_ascii_case(&self.current_workspace()) {
            return Err(format!("\"{name}\" can't be deleted: it's AutoCAD's or current."));
        }
        let mut settings = self.settings.clone();
        let before = settings.workspaces.len();
        settings.workspaces.retain(|w| !w.name.eq_ignore_ascii_case(name));
        if settings.workspaces.len() == before {
            return Err(format!("Workspace \"{name}\" not found."));
        }
        self.keep_settings(settings);
        Ok(format!("Workspace \"{name}\" deleted."))
    }

    /// The status bar's Workspace menu: the workspaces, and Save Current As.
    pub(crate) fn workspace_menu(&mut self, ui: &mut egui::Ui) {
        let current = self.current_workspace();
        let menu = ui.menu_button(RichText::new("Workspace").color(theme::text_dim()), |ui| {
            for w in self.workspaces() {
                if ui.selectable_label(w.name == current, &w.name).clicked() {
                    if let Err(e) = self.set_workspace(&w.name) {
                        self.engine.log(e);
                    }
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Save Current As...").clicked() {
                self.run("WSSAVE");
                ui.close();
            }
            if ui.button("Customize...").clicked() {
                self.run("CUI");
                ui.close();
            }
        });
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Workspace Switching"));
        menu.response.on_hover_text(format!("Workspace Switching: {current}"));
    }

    /// Runs the commands of the shortcuts of your own pressed (CUI).
    pub(crate) fn custom_shortcuts(&mut self, ctx: &egui::Context) {
        let pressed: Vec<String> = self
            .settings
            .shortcuts
            .iter()
            .filter_map(|(keys, command)| {
                let (mods, key) = parse_shortcut(keys)?;
                ctx.input_mut(|i| i.consume_key(mods, key)).then(|| command.clone())
            })
            .collect();
        for command in pressed {
            self.run(&command);
        }
    }

    pub(crate) fn open_cui(&mut self) {
        self.cui = Some(CuiDialog {
            shortcuts: self.settings.shortcuts.clone(),
            quick_access: self.settings.quick_access.clone(),
            keys: String::new(),
            command: String::new(),
            toolbar_command: String::new(),
            panels: self.settings.ribbon_panels.clone(),
            panel: (String::new(), String::new(), String::new()),
            shortcut_menu: self.settings.shortcut_menu.clone(),
            menu_command: String::new(),
            commands: self.settings.commands.clone(),
            new_command: Default::default(),
        });
    }

    /// CUI: the Customize User Interface dialog — commands of your own
    /// (a macro and an image for the button), keyboard shortcuts of your
    /// own beside the built-in ones, the Quick Access Toolbar's commands,
    /// ribbon panels of your own (on AutoCAD's tabs or tabs of their own)
    /// and commands added to the shortcut menu.
    pub(crate) fn cui_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.cui.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        // Below its title, above OK and Cancel.
        let max_height = (self.dialog_area(ctx).bottom() - 60.0 - 110.0).max(200.0);
        egui::Window::new("Customize User Interface")
            .id(egui::Id::unique("cui"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([340.0, 60.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                // The sections scroll when the window is short; OK and
                // Cancel stay in sight.
                egui::ScrollArea::vertical().id_salt("cui_sections").max_height(max_height).show(ui, |ui| {
                    egui::CollapsingHeader::new(RichText::new("Commands").strong())
                        .id_salt("cui_commands_header")
                        .default_open(false)
                        .show(ui, |ui| {
                            let mut remove = None;
                            egui::ScrollArea::vertical().id_salt("cui_commands").max_height(110.0).show(ui, |ui| {
                                for (i, c) in d.commands.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        let group =
                                            if c.group.is_empty() { String::new() } else { format!(" [{}]", c.group) };
                                        ui.label(format!("{}: {}{group}", c.name, c.macro_text))
                                            .on_hover_text(format!("Panels, shortcuts and menus name it {}", c.id));
                                        if ui.small_button("Remove").clicked() {
                                            remove = Some(i);
                                        }
                                    });
                                }
                            });
                            if let Some(i) = remove {
                                d.commands.remove(i);
                            }
                            ui.horizontal(|ui| {
                                ui.label("Name:");
                                let r = ui.add(egui::TextEdit::singleline(&mut d.new_command.0).desired_width(90.0));
                                r.widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Command name")
                                });
                                fields.push(r.id);
                                ui.label("Macro:");
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut d.new_command.1)
                                        .desired_width(130.0)
                                        .hint_text("^C^C_CIRCLE;\\2;"),
                                );
                                r.widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Command macro")
                                });
                                fields.push(r.id);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Image:");
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut d.new_command.2)
                                        .desired_width(200.0)
                                        .hint_text("PNG, BMP or JPEG"),
                                );
                                r.widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Command image")
                                });
                                fields.push(r.id);
                                if ui.button("Browse...").clicked()
                                    && let Some(file) = rfd::FileDialog::new()
                                        .add_filter("Images", &["png", "bmp", "jpg", "jpeg"])
                                        .pick_file()
                                {
                                    d.new_command.2 = file.to_string_lossy().into_owned();
                                }
                                let id = command_id(&d.new_command.0);
                                let valid = !id.is_empty() && !d.new_command.1.trim().is_empty();
                                if ui.add_enabled(valid, egui::Button::new("Add Command")).clicked() {
                                    d.commands.retain(|c| !c.id.eq_ignore_ascii_case(&id));
                                    let (name, macro_text, image) = std::mem::take(&mut d.new_command);
                                    d.commands.push(crate::settings::CustomCommand {
                                        id,
                                        name: name.trim().to_string(),
                                        macro_text: macro_text.trim().to_string(),
                                        image: image.trim().to_string(),
                                        group: String::new(),
                                    });
                                }
                            });
                            ui.colored_label(
                                theme::text_dim(),
                                "Panels, shortcuts and menus name a command of your own by its name, spaces as _.",
                            );
                        });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Keyboard Shortcuts").strong());
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        egui::Grid::new("cui_keys").num_columns(3).spacing([16.0, 2.0]).show(ui, |ui| {
                            let mut remove = None;
                            for (i, (keys, command)) in d.shortcuts.iter().enumerate() {
                                ui.label(keys);
                                ui.label(command);
                                if ui.small_button("Remove").clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                            if let Some(i) = remove {
                                d.shortcuts.remove(i);
                            }
                            for (keys, command) in BUILT_IN {
                                ui.colored_label(theme::text_dim(), *keys);
                                ui.colored_label(theme::text_dim(), *command);
                                ui.label("");
                                ui.end_row();
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Keys:");
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut d.keys).desired_width(110.0).hint_text("Ctrl+Shift+L"),
                        );
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Shortcut keys"));
                        fields.push(r.id);
                        ui.label("Command:");
                        let r = ui.add(egui::TextEdit::singleline(&mut d.command).desired_width(110.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Shortcut command"));
                        fields.push(r.id);
                        let command = d.command.trim().to_ascii_uppercase();
                        let valid = parse_shortcut(&d.keys).is_some() && !command.is_empty();
                        if ui.add_enabled(valid, egui::Button::new("Add Shortcut")).clicked() {
                            let keys = d.keys.trim().to_string();
                            d.shortcuts.retain(|(k, _)| !k.eq_ignore_ascii_case(&keys));
                            d.shortcuts.push((keys, command));
                            d.keys.clear();
                            d.command.clear();
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Quick Access Toolbar").strong());
                    ui.colored_label(theme::text_dim(), "New, Open, Save, Undo and Redo, then these:");
                    let mut remove = None;
                    ui.horizontal_wrapped(|ui| {
                        for (i, c) in d.quick_access.iter().enumerate() {
                            if ui.button(format!("{c}  x")).on_hover_text("Remove").clicked() {
                                remove = Some(i);
                            }
                        }
                    });
                    if let Some(i) = remove {
                        d.quick_access.remove(i);
                    }
                    ui.horizontal(|ui| {
                        ui.label("Command:");
                        let r = ui.add(egui::TextEdit::singleline(&mut d.toolbar_command).desired_width(140.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Toolbar command"));
                        fields.push(r.id);
                        let command = d.toolbar_command.trim().to_ascii_uppercase();
                        if ui.add_enabled(!command.is_empty(), egui::Button::new("Add to Toolbar")).clicked() {
                            if !d.quick_access.contains(&command) {
                                d.quick_access.push(command);
                            }
                            d.toolbar_command.clear();
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Ribbon Panels").strong());
                    let mut remove = None;
                    for (i, p) in d.panels.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{} › {}: {}", p.tab, p.name, p.commands.join(", ")));
                            if ui.small_button("Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        d.panels.remove(i);
                    }
                    ui.horizontal(|ui| {
                        ui.label("Tab:");
                        let r =
                            ui.add(egui::TextEdit::singleline(&mut d.panel.0).desired_width(80.0).hint_text("Home"));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Panel tab"));
                        fields.push(r.id);
                        ui.label("Panel:");
                        let r = ui.add(egui::TextEdit::singleline(&mut d.panel.1).desired_width(80.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Panel name"));
                        fields.push(r.id);
                        ui.label("Commands:");
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut d.panel.2).desired_width(140.0).hint_text("LINE, CIRCLE"),
                        );
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Panel commands"));
                        fields.push(r.id);
                        // `A|B|C`: a split button.
                        let commands: Vec<String> = d
                            .panel
                            .2
                            .split([',', ' '])
                            .map(|c| c.trim().to_ascii_uppercase())
                            .filter(|c| !c.is_empty())
                            .collect();
                        let valid =
                            !d.panel.0.trim().is_empty() && !d.panel.1.trim().is_empty() && !commands.is_empty();
                        if ui.add_enabled(valid, egui::Button::new("Add Panel")).clicked() {
                            d.panels.push(crate::settings::RibbonPanel {
                                tab: d.panel.0.trim().to_string(),
                                name: d.panel.1.trim().to_string(),
                                commands,
                                group: String::new(),
                            });
                            d.panel.1.clear();
                            d.panel.2.clear();
                        }
                    });
                    ui.colored_label(
                        theme::text_dim(),
                        "A tab that isn't the ribbon's is made for its panels; CIRCLE|ARC is a split button.",
                    );
                    ui.add_space(8.0);
                    ui.label(RichText::new("Shortcut Menu").strong());
                    let mut remove = None;
                    ui.horizontal_wrapped(|ui| {
                        for (i, c) in d.shortcut_menu.iter().enumerate() {
                            if ui.button(format!("{c}  x")).on_hover_text("Remove").clicked() {
                                remove = Some(i);
                            }
                        }
                    });
                    if let Some(i) = remove {
                        d.shortcut_menu.remove(i);
                    }
                    ui.horizontal(|ui| {
                        ui.label("Command:");
                        let r = ui.add(egui::TextEdit::singleline(&mut d.menu_command).desired_width(140.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Menu command"));
                        fields.push(r.id);
                        let command = d.menu_command.trim().to_ascii_uppercase();
                        if ui.add_enabled(!command.is_empty(), egui::Button::new("Add to Menu")).clicked() {
                            if !d.shortcut_menu.contains(&command) {
                                d.shortcut_menu.push(command);
                            }
                            d.menu_command.clear();
                        }
                    });
                });
                let row = vec2(ui.available_width().max(440.0), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            self.button_images.clear();
            let settings = Settings {
                shortcuts: d.shortcuts,
                quick_access: d.quick_access,
                ribbon_panels: d.panels,
                shortcut_menu: d.shortcut_menu,
                commands: d.commands,
                ..self.settings.clone()
            };
            self.keep_settings(settings);
            return;
        }
        if open && !cancel {
            self.cui = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_parse() {
        assert_eq!(parse_shortcut("Ctrl+Shift+L"), Some((Modifiers::CTRL | Modifiers::SHIFT, Key::L)));
        assert_eq!(parse_shortcut("cmd+f5"), Some((Modifiers::COMMAND, Key::F5)));
        assert_eq!(parse_shortcut("F2"), Some((Modifiers::NONE, Key::F2)));
        assert_eq!(parse_shortcut("Hyper+L"), None);
        assert_eq!(parse_shortcut(""), None);
    }
}

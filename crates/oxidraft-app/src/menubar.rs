//! AutoCAD's classic menu bar and toolbars. MENUBAR 1 shows the menus
//! AutoCAD LT has — File, Edit, View, Insert, Format, Tools, Draw,
//! Dimension, Modify, Window and Help — each item a command; TOOLBAR lists
//! the toolbars (Standard, Draw, Modify, Dimension, Layers, Text, Inquiry,
//! Zoom) to show, docked under the ribbon (or at the bottom, left or right)
//! or floating, and -TOOLBAR shows, hides, docks or floats one by name. A
//! customization file's menus and toolbars (CUILOAD) join them.

use eframe::egui::{self, Ui};
use serde::{Deserialize, Serialize};

use crate::app::OxidraftApp;
use crate::theme;

/// A menu's item: a command, a line between items, or a submenu.
pub(crate) enum Item {
    Command(&'static str, &'static str),
    Separator,
    Sub(&'static str, &'static [Item]),
}

use Item::{Command as C, Separator as S, Sub};

const ZOOM: &[Item] = &[
    C("Realtime", "ZOOM"),
    C("Previous", "ZOOM P"),
    C("Window", "ZOOM W"),
    C("Dynamic", "ZOOM D"),
    C("Scale", "ZOOM S"),
    C("Center", "ZOOM C"),
    C("Object", "ZOOM O"),
    S,
    C("All", "ZOOM A"),
    C("Extents", "ZOOM E"),
];

const ARC: &[Item] = &[C("3 Points", "ARC"), C("Start, Center, End", "ARC C"), C("Center, Start, End", "ARC C")];

const CIRCLE: &[Item] = &[
    C("Center, Radius", "CIRCLE"),
    C("2 Points", "CIRCLE 2P"),
    C("3 Points", "CIRCLE 3P"),
    C("Tan, Tan, Radius", "CIRCLE T"),
];

const POINT: &[Item] = &[
    C("Single Point", "POINT"),
    C("Multiple Point", "MULTIPLE POINT"),
    C("Divide", "DIVIDE"),
    C("Measure", "MEASURE"),
];

const BLOCK: &[Item] = &[C("Make...", "BLOCK"), C("Write Block...", "WBLOCK"), C("Define Attributes...", "ATTDEF")];

const TEXT: &[Item] = &[C("Multiline Text...", "MTEXT"), C("Single Line Text", "TEXT")];

const INQUIRY: &[Item] = &[
    C("Distance", "DIST"),
    C("Area", "AREA"),
    C("List", "LIST"),
    C("ID Point", "ID"),
    S,
    C("Time", "TIME"),
    C("Status", "STATUS"),
    C("Set Variable", "SETVAR"),
];

const DRAW_ORDER: &[Item] = &[
    C("Bring to Front", "DRAWORDER F"),
    C("Send to Back", "DRAWORDER B"),
    C("Bring Above Objects", "DRAWORDER A"),
    C("Send Under Objects", "DRAWORDER U"),
    S,
    C("Bring Text to Front", "TEXTTOFRONT"),
];

const UTILITIES: &[Item] = &[
    C("Audit", "AUDIT"),
    C("Recover...", "RECOVER"),
    C("Drawing Recovery Manager...", "DRAWINGRECOVERY"),
    S,
    C("Purge...", "PURGE"),
];

const PALETTES: &[Item] = &[
    C("Properties", "PROPERTIES"),
    C("Tool Palettes", "TOOLPALETTES"),
    C("DesignCenter", "ADCENTER"),
    C("Sheet Set Manager", "SHEETSET"),
    C("QuickCalc", "QUICKCALC"),
    C("External References", "EXTERNALREFERENCES"),
];

const DIM_TEXT: &[Item] = &[
    C("Angle", "DIMTEDIT A"),
    C("Home", "DIMTEDIT H"),
    C("Left", "DIMTEDIT L"),
    C("Center", "DIMTEDIT C"),
    C("Right", "DIMTEDIT R"),
];

/// The menus, as AutoCAD LT's.
pub(crate) const MENUS: &[(&str, &[Item])] = &[
    (
        "File",
        &[
            C("New...", "NEW"),
            C("Open...", "OPEN"),
            C("Close", "CLOSE"),
            S,
            C("Save", "QSAVE"),
            C("Save As...", "SAVEAS"),
            C("eTransmit...", "ETRANSMIT"),
            C("Export...", "EXPORT"),
            S,
            C("Page Setup Manager...", "PAGESETUP"),
            C("Plotter Manager...", "PLOTTERMANAGER"),
            C("Plot Preview", "PREVIEW"),
            C("Plot...", "PLOT"),
            C("Publish...", "PUBLISH"),
            S,
            Sub("Drawing Utilities", UTILITIES),
            C("Drawing Properties...", "DWGPROPS"),
            S,
            C("Exit", "QUIT"),
        ],
    ),
    (
        "Edit",
        &[
            C("Undo", "U"),
            C("Redo", "REDO"),
            S,
            C("Cut", "CUTCLIP"),
            C("Copy", "COPYCLIP"),
            C("Copy with Base Point", "COPYBASE"),
            C("Paste", "PASTECLIP"),
            C("Paste as Block", "PASTEBLOCK"),
            C("Paste as Hyperlink", "PASTEASHYPERLINK"),
            C("Paste to Original Coordinates", "PASTEORIG"),
            S,
            C("Clear", "ERASE"),
            C("Select All", "SELECTALL"),
            S,
            C("Find...", "FIND"),
        ],
    ),
    (
        "View",
        &[
            C("Redraw", "REDRAW"),
            C("Regen", "REGEN"),
            C("Regen All", "REGENALL"),
            S,
            Sub("Zoom", ZOOM),
            C("Pan", "PAN"),
            S,
            C("Viewports...", "VPORTS"),
            C("Named Views...", "VIEW"),
            S,
            C("Clean Screen", "CLEANSCREENON"),
            C("Toolbars...", "TOOLBAR"),
        ],
    ),
    (
        "Insert",
        &[
            C("Block...", "INSERT"),
            C("DWG Reference...", "XATTACH"),
            C("Raster Image Reference...", "IMAGEATTACH"),
            C("PDF Underlay...", "PDFATTACH"),
            C("DWF Underlay...", "DWFATTACH"),
            C("DGN Underlay...", "DGNATTACH"),
            S,
            C("Field...", "FIELD"),
            S,
            C("Layout Wizard", "LAYOUTWIZARD"),
            C("OLE Object...", "INSERTOBJ"),
            S,
            C("External References", "EXTERNALREFERENCES"),
            C("Hyperlink...", "HYPERLINK"),
        ],
    ),
    (
        "Format",
        &[
            C("Layer...", "LAYER"),
            C("Layer States Manager...", "LAYERSTATE"),
            S,
            C("Color...", "COLOR"),
            C("Linetype...", "LINETYPE"),
            C("Lineweight...", "LWEIGHT"),
            C("Scale List...", "SCALELISTEDIT"),
            S,
            C("Text Style...", "STYLE"),
            C("Dimension Style...", "DIMSTYLE"),
            C("Table Style...", "TABLESTYLE"),
            C("Multileader Style...", "MLEADERSTYLE"),
            C("Point Style...", "PTYPE"),
            S,
            C("Units...", "UNITS"),
            C("Drawing Limits", "LIMITS"),
            S,
            C("Rename...", "RENAME"),
        ],
    ),
    (
        "Tools",
        &[
            Sub("Palettes", PALETTES),
            S,
            C("Spelling", "SPELL"),
            C("Quick Select...", "QSELECT"),
            Sub("Draw Order", DRAW_ORDER),
            Sub("Inquiry", INQUIRY),
            S,
            C("Load Application...", "APPLOAD"),
            C("Run Script...", "SCRIPT"),
            S,
            C("Drafting Settings...", "DSETTINGS"),
            C("Customize Interface...", "CUI"),
            C("Load Customizations...", "CUILOAD"),
            C("Options...", "OPTIONS"),
        ],
    ),
    (
        "Draw",
        &[
            C("Line", "LINE"),
            C("Ray", "RAY"),
            C("Construction Line", "XLINE"),
            S,
            C("Polyline", "PLINE"),
            C("Polygon", "POLYGON"),
            C("Rectangle", "RECTANG"),
            S,
            Sub("Arc", ARC),
            Sub("Circle", CIRCLE),
            C("Donut", "DONUT"),
            C("Spline", "SPLINE"),
            C("Ellipse", "ELLIPSE"),
            Sub("Block", BLOCK),
            C("Table...", "TABLE"),
            Sub("Point", POINT),
            S,
            C("Hatch...", "HATCH"),
            C("Gradient...", "GRADIENT"),
            C("Boundary...", "BOUNDARY"),
            C("Wipeout", "WIPEOUT"),
            C("Revision Cloud", "REVCLOUD"),
            S,
            Sub("Text", TEXT),
        ],
    ),
    (
        "Dimension",
        &[
            C("Quick Dimension", "QDIM"),
            S,
            C("Linear", "DIMLINEAR"),
            C("Aligned", "DIMALIGNED"),
            C("Arc Length", "DIMARC"),
            C("Ordinate", "DIMORDINATE"),
            C("Radius", "DIMRADIUS"),
            C("Jogged", "DIMJOGGED"),
            C("Diameter", "DIMDIAMETER"),
            C("Angular", "DIMANGULAR"),
            S,
            C("Baseline", "DIMBASELINE"),
            C("Continue", "DIMCONTINUE"),
            C("Dimension Space", "DIMSPACE"),
            C("Dimension Break", "DIMBREAK"),
            S,
            C("Multileader", "MLEADER"),
            C("Tolerance...", "TOLERANCE"),
            C("Center Mark", "CENTERMARK"),
            C("Inspection...", "DIMINSPECT"),
            C("Jogged Linear", "DIMJOGLINE"),
            S,
            C("Oblique", "DIMEDIT O"),
            Sub("Align Text", DIM_TEXT),
            S,
            C("Dimension Style...", "DIMSTYLE"),
            C("Override", "DIMOVERRIDE"),
            C("Update", "DIMUPDATE"),
            C("Reassociate Dimensions", "DIMREASSOCIATE"),
        ],
    ),
    (
        "Modify",
        &[
            C("Properties", "PROPERTIES"),
            C("Match Properties", "MATCHPROP"),
            C("Change to ByLayer", "SETBYLAYER"),
            S,
            C("Erase", "ERASE"),
            C("Copy", "COPY"),
            C("Mirror", "MIRROR"),
            C("Offset", "OFFSET"),
            C("Array", "ARRAY"),
            C("Move", "MOVE"),
            C("Rotate", "ROTATE"),
            C("Scale", "SCALE"),
            C("Stretch", "STRETCH"),
            C("Lengthen", "LENGTHEN"),
            C("Trim", "TRIM"),
            C("Extend", "EXTEND"),
            C("Break", "BREAK"),
            C("Join", "JOIN"),
            C("Chamfer", "CHAMFER"),
            C("Fillet", "FILLET"),
            S,
            C("Explode", "EXPLODE"),
        ],
    ),
    (
        "Window",
        &[
            C("Close", "CLOSE"),
            C("Close All", "CLOSEALL"),
            S,
            C("Cascade", "CASCADE"),
            C("Tile", "TILE"),
            C("Switch Windows", "SWITCHWINDOWS"),
        ],
    ),
    ("Help", &[C("Help", "HELP"), C("About Oxidraft", "ABOUT")]),
];

/// The toolbars: each a name and its commands, `""` a line between them.
pub(crate) const TOOLBARS: &[(&str, &[&str])] = &[
    (
        "Standard",
        &[
            "QNEW",
            "OPEN",
            "QSAVE",
            "PLOT",
            "PREVIEW",
            "",
            "CUTCLIP",
            "COPYCLIP",
            "PASTECLIP",
            "MATCHPROP",
            "",
            "U",
            "REDO",
            "",
            "PAN",
            "ZOOM",
            "ZOOM W",
            "ZOOM P",
            "",
            "PROPERTIES",
            "ADCENTER",
            "TOOLPALETTES",
            "SHEETSET",
            "QUICKCALC",
        ],
    ),
    (
        "Draw",
        &[
            "LINE", "XLINE", "PLINE", "POLYGON", "RECTANG", "ARC", "CIRCLE", "REVCLOUD", "SPLINE", "ELLIPSE", "",
            "INSERT", "BLOCK", "POINT", "HATCH", "GRADIENT", "TABLE", "MTEXT",
        ],
    ),
    (
        "Modify",
        &[
            "ERASE", "COPY", "MIRROR", "OFFSET", "ARRAY", "MOVE", "ROTATE", "SCALE", "STRETCH", "TRIM", "EXTEND",
            "BREAK", "JOIN", "CHAMFER", "FILLET", "EXPLODE",
        ],
    ),
    (
        "Dimension",
        &[
            "DIMLINEAR",
            "DIMALIGNED",
            "DIMARC",
            "DIMORDINATE",
            "DIMRADIUS",
            "DIMJOGGED",
            "DIMDIAMETER",
            "DIMANGULAR",
            "",
            "QDIM",
            "DIMBASELINE",
            "DIMCONTINUE",
            "DIMSPACE",
            "DIMBREAK",
            "",
            "TOLERANCE",
            "CENTERMARK",
            "DIMINSPECT",
            "DIMJOGLINE",
            "",
            "DIMEDIT",
            "DIMTEDIT",
            "DIMUPDATE",
            "DIMSTYLE",
        ],
    ),
    ("Layers", &["LAYER", "LAYERP", "LAYMCUR", "LAYISO", "LAYUNISO"]),
    ("Text", &["MTEXT", "TEXT", "TEXTEDIT", "FIND", "SPELL", "STYLE", "SCALETEXT", "JUSTIFYTEXT"]),
    ("Inquiry", &["DIST", "AREA", "LIST", "ID"]),
    ("Zoom", &["ZOOM W", "ZOOM D", "ZOOM S", "ZOOM C", "ZOOM O", "", "ZOOM A", "ZOOM E"]),
];

/// A menu of a customization file's: its name and items.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomMenu {
    pub name: String,
    pub items: Vec<MenuEntry>,
    pub group: String,
}

/// A custom menu's item: its label and the id of the command it runs;
/// with no command, a line (or, with items of its own, a submenu).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MenuEntry {
    pub label: String,
    pub command: String,
    pub items: Vec<MenuEntry>,
}

/// A toolbar of a customization file's: its name and command ids, `""` a
/// line between them.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomToolbar {
    pub name: String,
    pub commands: Vec<String>,
    pub group: String,
}

/// Where a toolbar is: docked at a side of the drawing area, or floating
/// with its window's corner here.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum ToolbarPlace {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
    Float(f32, f32),
}

/// The Toolbars dialog (TOOLBAR).
#[derive(Default)]
pub(crate) struct ToolbarsDialog;

impl OxidraftApp {
    /// Every toolbar's name: AutoCAD's, then those loaded.
    pub fn toolbar_names(&self) -> Vec<String> {
        let own = TOOLBARS.iter().map(|(n, _)| n.to_string());
        own.chain(self.settings.custom_toolbars.iter().map(|t| t.name.clone())).collect()
    }

    /// The toolbars shown, by name.
    pub fn toolbars_shown(&self) -> &[String] {
        &self.settings.toolbars
    }

    /// A toolbar's commands, by its name.
    fn toolbar_commands(&self, name: &str) -> Vec<String> {
        match TOOLBARS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some((_, commands)) => commands.iter().map(|c| c.to_string()).collect(),
            None => self
                .settings
                .custom_toolbars
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
                .map(|t| t.commands.clone())
                .unwrap_or_default(),
        }
    }

    /// -TOOLBAR: shows or hides toolbar `name` (or ALL of them).
    pub fn show_toolbar(&mut self, name: &str, show: bool) -> Result<(), String> {
        let names = self.toolbar_names();
        let picked: Vec<String> = if name.eq_ignore_ascii_case("ALL") {
            names
        } else {
            let n = names.into_iter().find(|n| n.eq_ignore_ascii_case(name.trim()));
            vec![n.ok_or_else(|| format!("No toolbar named \"{}\".", name.trim()))?]
        };
        let mut settings = self.settings.clone();
        for n in picked {
            settings.toolbars.retain(|t| !t.eq_ignore_ascii_case(&n));
            if show {
                settings.toolbars.push(n);
            }
        }
        self.keep_settings(settings);
        Ok(())
    }

    /// -TOOLBAR's options: Show, Hide, a side to dock at, or Float.
    pub fn toolbar_option(&mut self, name: &str, option: &str) -> Result<(), String> {
        let show = option != "Hide";
        self.show_toolbar(name, show)?;
        let place = match option {
            "Top" => ToolbarPlace::Top,
            "Bottom" => ToolbarPlace::Bottom,
            "Left" => ToolbarPlace::Left,
            "Right" => ToolbarPlace::Right,
            "Float" => ToolbarPlace::Float(240.0, 200.0),
            _ => return Ok(()),
        };
        let names: Vec<String> = self
            .toolbar_names()
            .into_iter()
            .filter(|n| name.eq_ignore_ascii_case("ALL") || n.eq_ignore_ascii_case(name.trim()))
            .collect();
        for n in names {
            self.place_toolbar(&n, place);
        }
        Ok(())
    }

    /// MENUBAR.
    pub fn set_menu_bar(&mut self, on: bool) {
        let settings = crate::settings::Settings { menubar: on, ..self.settings.clone() };
        self.keep_settings(settings);
    }

    /// The classic menu bar, when MENUBAR is 1: AutoCAD's menus, then
    /// those loaded.
    pub(crate) fn menu_bar(&mut self, ui: &mut Ui) {
        if !self.settings.menubar {
            return;
        }
        let frame = egui::Frame::NONE.fill(theme::strip()).inner_margin(egui::Margin::symmetric(6, 0));
        egui::Panel::top("menu_bar").frame(frame).exact_size(22.0).show_separator_line(false).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for (title, items) in MENUS {
                    ui.menu_button(*title, |ui| self.menu_items(ui, items));
                }
                for menu in self.settings.menus.clone() {
                    ui.menu_button(&menu.name, |ui| self.custom_menu_items(ui, &menu.items));
                }
            });
        });
    }

    fn menu_items(&mut self, ui: &mut Ui, items: &[Item]) {
        for item in items {
            match item {
                Item::Command(label, command) => {
                    if ui.button(*label).clicked() {
                        self.run(command);
                        ui.close();
                    }
                }
                Item::Separator => {
                    ui.separator();
                }
                Item::Sub(label, items) => {
                    ui.menu_button(*label, |ui| self.menu_items(ui, items));
                }
            }
        }
    }

    fn custom_menu_items(&mut self, ui: &mut Ui, items: &[MenuEntry]) {
        for item in items {
            if !item.items.is_empty() {
                ui.menu_button(&item.label, |ui| self.custom_menu_items(ui, &item.items));
            } else if item.command.is_empty() {
                ui.separator();
            } else if ui.button(&item.label).on_hover_text(self.button_tip(&item.command)).clicked() {
                self.run(&item.command);
                ui.close();
            }
        }
    }

    /// Where toolbar `name` is: docked at a side, or floating.
    pub fn toolbar_place(&self, name: &str) -> ToolbarPlace {
        self.settings
            .toolbar_places
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map_or(ToolbarPlace::Top, |(_, p)| *p)
    }

    /// Docks toolbar `name` at a side, or floats it.
    pub fn place_toolbar(&mut self, name: &str, place: ToolbarPlace) {
        let mut settings = self.settings.clone();
        settings.toolbar_places.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        if place != ToolbarPlace::Top {
            settings.toolbar_places.push((name.to_string(), place));
        }
        self.keep_settings(settings);
    }

    fn shown_at(&self, side: impl Fn(ToolbarPlace) -> bool) -> Vec<String> {
        self.toolbar_names()
            .into_iter()
            .filter(|n| self.settings.toolbars.iter().any(|s| s == n) && side(self.toolbar_place(n)))
            .collect()
    }

    /// A toolbar: its grip (its name as a tip; right-click to dock it at
    /// a side, float or hide it) and its buttons, in a row or a column.
    fn toolbar_strip(&mut self, ui: &mut Ui, name: &str, vertical: bool) -> egui::Response {
        let commands = self.toolbar_commands(name);
        let mut placed = None;
        let group = ui.group(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(1.0, 1.0);
            let strip = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
                if vertical {
                    ui.vertical(|ui| add(ui));
                } else {
                    ui.horizontal(|ui| add(ui));
                }
            };
            strip(ui, &mut |ui: &mut Ui| {
                let size = if vertical { egui::vec2(20.0, 6.0) } else { egui::vec2(6.0, 20.0) };
                let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
                let stroke = egui::Stroke::new(1.0, theme::text_dim());
                for k in [1.5, 4.5] {
                    let line = if vertical {
                        [egui::pos2(rect.left() + 3.0, rect.top() + k), egui::pos2(rect.right() - 3.0, rect.top() + k)]
                    } else {
                        [
                            egui::pos2(rect.left() + k, rect.top() + 3.0),
                            egui::pos2(rect.left() + k, rect.bottom() - 3.0),
                        ]
                    };
                    ui.painter().line_segment(line, stroke);
                }
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name} toolbar")));
                let r = r.on_hover_text(name);
                r.context_menu(|ui| {
                    for (label, place) in [
                        ("Dock at Top", ToolbarPlace::Top),
                        ("Dock at Bottom", ToolbarPlace::Bottom),
                        ("Dock at Left", ToolbarPlace::Left),
                        ("Dock at Right", ToolbarPlace::Right),
                        ("Float", ToolbarPlace::Float(240.0, 200.0)),
                    ] {
                        if ui.button(label).clicked() {
                            placed = Some(Some(place));
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Hide").clicked() {
                        placed = Some(None);
                        ui.close();
                    }
                });
                for c in &commands {
                    if c.is_empty() {
                        ui.add_space(6.0);
                    } else {
                        self.toolbar_button(ui, c);
                    }
                }
            });
        });
        match placed {
            Some(Some(place)) => self.place_toolbar(name, place),
            Some(None) => {
                let _ = self.show_toolbar(name, false);
            }
            None => {}
        }
        group.response
    }

    /// The toolbars docked at the top (under the ribbon) or the bottom, in
    /// rows as wide as the window, by how wide each was last time.
    fn toolbar_rows(&mut self, ui: &mut Ui, shown: Vec<String>) {
        let width_id = |n: &str| egui::Id::unique(("toolbar_width", n.to_string()));
        let available = ui.available_width();
        let mut rows: Vec<Vec<String>> = vec![Vec::new()];
        let mut x = 0.0;
        for name in shown {
            let w = ui.ctx().data(|d| d.get_temp::<f32>(width_id(&name))).unwrap_or(0.0);
            if x > 0.0 && x + w > available {
                rows.push(Vec::new());
                x = 0.0;
            }
            x += w + 4.0;
            rows.last_mut().into_iter().for_each(|r| r.push(name.clone()));
        }
        for row in rows {
            ui.horizontal(|ui| {
                for name in row {
                    let w = self.toolbar_strip(ui, &name, false).rect.width();
                    ui.ctx().data_mut(|d| d.insert_temp(width_id(&name), w));
                }
            });
        }
    }

    /// The toolbars shown that are docked: at the top under the ribbon,
    /// at the bottom, or in columns at the left or right.
    pub(crate) fn toolbars(&mut self, ui: &mut Ui) {
        let frame = egui::Frame::NONE.fill(theme::ribbon()).inner_margin(egui::Margin::symmetric(6, 2));
        let top = self.shown_at(|p| p == ToolbarPlace::Top);
        if !top.is_empty() {
            egui::Panel::top("toolbars")
                .frame(frame)
                .show_separator_line(true)
                .show(ui, |ui| self.toolbar_rows(ui, top));
        }
        let bottom = self.shown_at(|p| p == ToolbarPlace::Bottom);
        if !bottom.is_empty() {
            egui::Panel::bottom("toolbars_bottom")
                .frame(frame)
                .show_separator_line(true)
                .show(ui, |ui| self.toolbar_rows(ui, bottom));
        }
        let side = egui::Frame::NONE.fill(theme::ribbon()).inner_margin(egui::Margin::symmetric(2, 6));
        for (place, id) in [(ToolbarPlace::Left, "toolbars_left"), (ToolbarPlace::Right, "toolbars_right")] {
            let shown = self.shown_at(|p| p == place);
            if shown.is_empty() {
                continue;
            }
            let panel = if place == ToolbarPlace::Left { egui::Panel::left(id) } else { egui::Panel::right(id) };
            panel.frame(side).resizable(false).show_separator_line(true).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for name in shown {
                        self.toolbar_strip(ui, &name, true);
                    }
                });
            });
        }
    }

    /// The floating toolbars, each in a window of its own; closing one
    /// hides it.
    pub(crate) fn floating_toolbars(&mut self, ctx: &egui::Context) {
        for name in self.shown_at(|p| matches!(p, ToolbarPlace::Float(..))) {
            let ToolbarPlace::Float(x, y) = self.toolbar_place(&name) else { continue };
            let mut open = true;
            let shown = egui::Window::new(&name)
                .id(egui::Id::unique(("floating_toolbar", name.clone())))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .auto_sized()
                .default_pos([x, y])
                .constrain_to(self.dialog_area(ctx))
                .show(ctx, |ui| {
                    self.toolbar_strip(ui, &name, false);
                });
            if !open {
                let _ = self.show_toolbar(&name, false);
            } else if let Some(r) = shown
                && r.response.drag_stopped()
            {
                let at = r.response.rect.min;
                if self.toolbar_place(&name) != ToolbarPlace::Float(at.x, at.y) {
                    self.place_toolbar(&name, ToolbarPlace::Float(at.x, at.y));
                }
            }
        }
    }

    /// TOOLBAR: the toolbars, each to show or hide.
    pub(crate) fn toolbars_dialog(&mut self, ctx: &egui::Context) {
        if self.toolbars_dialog.is_none() {
            return;
        }
        let mut open = true;
        let mut toggled = None;
        let names = self.toolbar_names();
        egui::Window::new("Toolbars")
            .id(egui::Id::unique("toolbars_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                let mut menubar = self.settings.menubar;
                if ui.checkbox(&mut menubar, "Menu Bar").changed() {
                    toggled = Some((String::new(), menubar));
                }
                ui.separator();
                for n in &names {
                    let mut on = self.settings.toolbars.iter().any(|s| s == n);
                    if ui.checkbox(&mut on, n.as_str()).changed() {
                        toggled = Some((n.clone(), on));
                    }
                }
            });
        match toggled {
            Some((n, on)) if n.is_empty() => self.set_menu_bar(on),
            Some((n, on)) => {
                let _ = self.show_toolbar(&n, on);
            }
            None => {}
        }
        if !open {
            self.toolbars_dialog = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `command` (with its options) starts a command Oxidraft has.
    fn known(command: &str) -> bool {
        let name = command.split_whitespace().next().unwrap_or("");
        oxidraft_commands::is_command(name)
    }

    #[test]
    fn every_menu_item_and_toolbar_button_runs_a_command() {
        fn check(items: &[Item], path: &str, unknown: &mut Vec<String>) {
            for item in items {
                match item {
                    Item::Command(label, command) if !known(command) => {
                        unknown.push(format!("{path} › {label}: {command}"))
                    }
                    Item::Sub(label, items) => check(items, &format!("{path} › {label}"), unknown),
                    _ => {}
                }
            }
        }
        let mut unknown = Vec::new();
        for (title, items) in MENUS {
            check(items, title, &mut unknown);
        }
        for (name, commands) in TOOLBARS {
            for c in commands.iter().filter(|c| !c.is_empty() && !known(c)) {
                unknown.push(format!("{name} toolbar: {c}"));
            }
        }
        assert!(unknown.is_empty(), "{unknown:#?}");
    }
}

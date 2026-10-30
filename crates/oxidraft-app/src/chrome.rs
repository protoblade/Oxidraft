//! Window chrome around the drawing: quick-access bar, ribbon, file tabs and
//! status bar, laid out like AutoCAD LT's.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect, Response, Sense, Shape, Stroke,
    Ui, pos2, vec2,
};

use crate::{
    app::OxidraftApp,
    icons::{self, Icon},
    layers::layer_state_icons,
    layouts::TabAction,
    properties::common,
    theme,
};

const RIBBON_TABS: &[&str] = &["Home", "Insert", "Annotate", "View", "Manage", "Output", "Collaborate"];
/// On a layout's tab the ribbon has a Layout tab too, after the others.
const LAYOUT_TAB: usize = RIBBON_TABS.len();
/// Tabs of your own (CUI) are numbered from here.
const CUSTOM_TAB: usize = 100;

const LAYOUT_TOOLS: &[Tool] = &[
    tool(Icon::NewLayout, "New", Some("LAYOUT N"), "Creates a layout (LAYOUT New)"),
    tool(Icon::PageSetup, "Page Setup", Some("PAGESETUP"), "Sets the layout's paper and orientation (PAGESETUP)"),
];
const VIEWPORT_TOOLS: &[Tool] = &[
    tool(Icon::ViewportLock, "Lock", Some("MVIEW L ON"), "Locks viewports' views: zooming in them moves the sheet"),
    tool(Icon::ViewportUnlock, "Unlock", Some("MVIEW L OFF"), "Unlocks viewports' views"),
    tool(Icon::Maximize, "Maximize", Some("VPMAX"), "Fills the drawing area with a viewport (VPMAX; VPMIN goes back)"),
];

/// A ribbon button: its icon, label and the command it runs (`None` = not implemented yet).
struct Tool {
    icon: Icon,
    label: &'static str,
    command: Option<&'static str>,
    tip: &'static str,
}

const fn tool(icon: Icon, label: &'static str, command: Option<&'static str>, tip: &'static str) -> Tool {
    Tool { icon, label, command, tip }
}

const DRAW_LARGE: &[Tool] = &[
    tool(Icon::Line, "Line", Some("LINE"), "Creates straight line segments (L)"),
    tool(Icon::Polyline, "Polyline", Some("PLINE"), "Creates a 2D polyline of lines and arcs (PL)"),
    tool(Icon::Circle, "Circle", Some("CIRCLE"), "Creates a circle by center and radius (C)"),
    tool(Icon::Arc, "Arc", Some("ARC"), "Creates an arc through three points or from its center (A)"),
];
/// Beside the large Draw tools.
const DRAW_SMALL: &[Tool] = &[
    tool(Icon::Rectangle, "Rectangle", Some("RECTANG"), "Creates a rectangular polyline (REC)"),
    tool(Icon::Hatch, "Hatch", Some("HATCH"), "Fills an enclosed area with a pattern (H)"),
];
/// The Draw panel's slide-out, under its title.
const DRAW_MORE: &[Tool] = &[
    tool(Icon::Ellipse, "Ellipse", Some("ELLIPSE"), "Creates an ellipse or elliptical arc (EL)"),
    tool(Icon::Polygon, "Polygon", Some("POLYGON"), "Creates a regular polygon (POL)"),
    tool(
        Icon::Spline,
        "Spline",
        Some("SPLINE"),
        "Creates a smooth curve through fit points or by control vertices (SPL)",
    ),
    tool(
        Icon::Point,
        "Point",
        Some("POINT"),
        "Places a point object (PO); DIVIDE and MEASURE place them along objects",
    ),
    tool(Icon::Xline, "Construction Line", Some("XLINE"), "Creates a line infinite both ways (XL)"),
    tool(Icon::Ray, "Ray", Some("RAY"), "Creates a line infinite one way"),
    tool(Icon::RevCloud, "Revision Cloud", Some("REVCLOUD"), "Draws a cloud of arcs around an area or object"),
    tool(Icon::Donut, "Donut", Some("DONUT"), "Creates filled rings and dots (DO)"),
    tool(Icon::Hatch, "Boundary", Some("BOUNDARY"), "Draws closed polylines around an area picked inside (BO)"),
];
const MODIFY_GRID: &[[Tool; 3]] = &[
    [
        tool(Icon::Move, "Move", Some("MOVE"), "Moves objects a specified distance (M)"),
        tool(Icon::Rotate, "Rotate", Some("ROTATE"), "Rotates objects around a base point (RO)"),
        tool(Icon::Trim, "Trim", Some("TRIM"), "Trims objects at other objects; Shift-click extends instead (TR, EX)"),
    ],
    [
        tool(Icon::Copy, "Copy", Some("COPY"), "Copies objects a specified distance (CO)"),
        tool(Icon::Mirror, "Mirror", Some("MIRROR"), "Creates a mirrored copy of objects (MI)"),
        tool(
            Icon::Fillet,
            "Fillet",
            Some("FILLET"),
            "Rounds or squares the corner between lines, or a polyline's corners (F)",
        ),
    ],
    [
        tool(Icon::Stretch, "Stretch", Some("STRETCH"), "Stretches objects crossed by a window (S)"),
        tool(Icon::Scale, "Scale", Some("SCALE"), "Enlarges or reduces objects (SC)"),
        tool(Icon::Array, "Array", Some("ARRAY"), "Copies objects in rows and columns or around a center (AR)"),
    ],
];
/// The Modify panel's slide-out, under its title.
const MODIFY_MORE: &[Tool] = &[
    tool(Icon::Erase, "Erase", Some("ERASE"), "Removes objects from the drawing (E, or Delete)"),
    tool(Icon::Offset, "Offset", Some("OFFSET"), "Creates parallel copies (O)"),
    tool(Icon::Explode, "Explode", Some("EXPLODE"), "Breaks polylines and blocks into their parts (X)"),
    tool(Icon::Chamfer, "Chamfer", Some("CHAMFER"), "Bevels the corner between lines, or a polyline's corners (CHA)"),
    tool(Icon::Extend, "Extend", Some("EXTEND"), "Extends objects to meet others (EX)"),
    tool(Icon::Break, "Break", Some("BREAK"), "Breaks an object between two points (BR)"),
    tool(Icon::Join, "Join", Some("JOIN"), "Joins lines, arcs and polylines end to end (J)"),
    tool(Icon::Lengthen, "Lengthen", Some("LENGTHEN"), "Changes the length of an object (LEN)"),
    tool(Icon::EditPolyline, "Edit Polyline", Some("PEDIT"), "Closes, opens, joins or reverses polylines (PE)"),
    tool(Icon::Spline, "Edit Spline", Some("SPLINEDIT"), "Edits a spline's fit points, vertices and order (SPE)"),
    tool(Icon::Spline, "Blend Curves", Some("BLEND"), "Joins two objects' ends with a smooth spline"),
    tool(Icon::Hatch, "Edit Hatch", Some("HATCHEDIT"), "Changes a hatch's pattern, draw order or boundaries (HE)"),
    tool(Icon::Move, "Align", Some("ALIGN"), "Moves, turns and scales objects so two points land on two others (AL)"),
    tool(Icon::Lengthen, "Reverse", Some("REVERSE"), "Reverses lines, polylines and splines"),
    tool(Icon::Array, "Path Array", Some("ARRAYPATH"), "Copies objects evenly along a path curve"),
    tool(Icon::Array, "Edit Array", Some("ARRAYEDIT"), "Changes an associative array's rows, columns or items"),
    tool(Icon::Viewport, "Change Space", Some("CHSPACE"), "Moves objects between a layout's sheet and model space"),
];
const BLOCK_INSERT: Tool = tool(
    Icon::InsertBlock,
    "Insert",
    Some("BLOCKSPALETTE"),
    "Inserts a block from this drawing or a library (the Blocks palette); INSERT (I) asks for its name",
);
const BLOCK_SMALL: &[Tool] = &[
    tool(Icon::CreateBlock, "Create", Some("BLOCK"), "Creates a block from objects (B)"),
    tool(Icon::EditReference, "Edit", Some("BEDIT"), "Edits a block definition in the Block Editor (BE)"),
];
/// The Insert tab's Block Definition panel.
const BLOCK_DEFINITION: &[Tool] = &[
    tool(
        Icon::Text,
        "Define Attributes",
        Some("ATTDEF"),
        "Defines an attribute for a block's references to ask for (ATT)",
    ),
    tool(Icon::EditReference, "Edit Attribute", Some("EATTEDIT"), "Edits a reference's attribute values"),
    tool(Icon::Properties, "Synchronize", Some("ATTSYNC"), "Gives a block's references the attributes it now defines"),
    tool(Icon::Properties, "Manage Attributes", Some("BATTMAN"), "Edits, reorders and removes a block's attributes"),
    tool(Icon::Save, "Write Block", Some("WBLOCK"), "Writes a block, objects or the drawing to a drawing file (W)"),
];
const LAYER_TOOLS: &[Tool] = &[
    tool(Icon::LayerIsolate, "", Some("LAYISO"), "Isolate: locks every layer but those of the selected objects"),
    tool(Icon::LayerUnisolate, "", Some("LAYUNISO"), "Unisolate: puts back the layers Isolate changed"),
    tool(Icon::LayerOff, "", Some("LAYOFF"), "Off: turns off the layer of the object you pick"),
    tool(Icon::LayerFreeze, "", Some("LAYFRZ"), "Freeze: freezes the layer of the object you pick"),
    tool(Icon::LayerLock, "", Some("LAYLCK"), "Lock: locks the layer of the object you pick"),
    tool(Icon::LayerUnlock, "", Some("LAYULK"), "Unlock: unlocks the layer of the object you pick"),
];
/// The Layers panel's slide-out.
const LAYERS_MORE: &[Tool] = &[
    tool(Icon::LayerOff, "Turn All Layers On", Some("LAYON"), "Turns every layer on (LAYON)"),
    tool(Icon::LayerFreeze, "Thaw All Layers", Some("LAYTHW"), "Thaws every layer (LAYTHW)"),
    tool(Icon::LayerProperties, "Layer Walk", Some("LAYWALK"), "Shows the layers you pick, one or a few at a time"),
    tool(Icon::MatchProperties, "Match Layer", Some("LAYMCH"), "Puts objects on the layer of one you pick (LAYMCH)"),
    tool(Icon::MakeCurrent, "Change to Current Layer", Some("LAYCUR"), "Puts objects on the current layer (LAYCUR)"),
    tool(Icon::Copy, "Copy Objects to New Layer", Some("COPYTOLAYER"), "Copies objects onto another layer"),
    tool(Icon::Join, "Merge", Some("LAYMRG"), "Merges layers into another; their objects move (LAYMRG)"),
    tool(Icon::Erase, "Delete", Some("LAYDEL"), "Deletes a layer and everything on it (LAYDEL)"),
    tool(Icon::Undo, "Previous", Some("LAYERP"), "Puts back the layer settings before the last change (LAYP)"),
    tool(Icon::LayerProperties, "Layer States...", Some("LAYERSTATE"), "Saves and restores named layer settings (LAS)"),
    tool(Icon::Viewport, "Isolate to Current Viewport", Some("LAYVPI"), "Freezes layers in every other viewport"),
];
const ANNOTATION_LARGE: &[Tool] = &[
    tool(Icon::Text, "Text", Some("MTEXT"), "Creates multiline text (T); single-line text is DT"),
    tool(Icon::Dimension, "Dimension", Some("DIMLINEAR"), "Creates a horizontal or vertical dimension (DLI)"),
];
const ANNOTATION_SMALL: &[Tool] = &[
    tool(Icon::Dimension, "Aligned", Some("DIMALIGNED"), "Creates a dimension aligned to two points (DAL)"),
    tool(Icon::Leader, "Leader", Some("MLEADER"), "Creates a leader with text (MLD)"),
    tool(Icon::Table, "Table", Some("TABLE"), "Inserts a table of rows and columns (TB)"),
];
/// The Annotate tab's panels.
const ANNOTATE_TEXT: &[Tool] = &[
    tool(Icon::Text, "Multiline\nText", Some("MTEXT"), "Creates multiline text (T, MT)"),
    tool(Icon::SingleLineText, "Single\nLine", Some("TEXT"), "Creates single-line text (DT)"),
];
const ANNOTATE_DIMENSIONS: &[Tool] = &[
    tool(Icon::Dimension, "Aligned", Some("DIMALIGNED"), "Creates a dimension aligned to two points (DAL)"),
    tool(Icon::DimAngular, "Angular", Some("DIMANGULAR"), "Dimensions the angle between two lines, or of an arc (DAN)"),
    tool(Icon::DimRadius, "Radius", Some("DIMRADIUS"), "Dimensions a circle's or arc's radius (DRA)"),
    tool(Icon::DimDiameter, "Diameter", Some("DIMDIAMETER"), "Dimensions a circle's or arc's diameter (DDI)"),
    tool(
        Icon::DimContinue,
        "Continue",
        Some("DIMCONTINUE"),
        "Continues a dimension from the last one's second line (DCO)",
    ),
    tool(Icon::DimBaseline, "Baseline", Some("DIMBASELINE"), "Dimensions from the last one's first line (DBA)"),
    tool(Icon::DimAngular, "Arc Length", Some("DIMARC"), "Dimensions the length of an arc (DAR)"),
    tool(Icon::DimRadius, "Jogged", Some("DIMJOGGED"), "Dimensions a large arc's radius with a jog (DJO)"),
    tool(Icon::Dimension, "Ordinate", Some("DIMORDINATE"), "Dimensions a feature's X or Y (DOR)"),
    tool(Icon::DimContinue, "Quick", Some("QDIM"), "Dimensions the points of the selected objects at once"),
];
/// The Dimensions panel's slide-out: editing dimensions.
const ANNOTATE_DIMENSIONS_MORE: &[Tool] = &[
    tool(Icon::Break, "Break", Some("DIMBREAK"), "Cuts gaps where objects cross a dimension"),
    tool(Icon::DimBaseline, "Adjust Space", Some("DIMSPACE"), "Spaces parallel dimensions evenly"),
    tool(Icon::Dimension, "Linear", Some("DIMLINEAR"), "Creates a horizontal or vertical dimension (DLI)"),
    tool(Icon::Text, "Edit Dimension", Some("DIMEDIT"), "New text, text angle, or oblique extension lines (DED)"),
    tool(Icon::Move, "Move Dimension Text", Some("DIMTEDIT"), "Moves or turns a dimension's text"),
    tool(Icon::Dimension, "Jogged Linear", Some("DIMJOGLINE"), "Adds a jog to a linear dimension's line (DJL)"),
    tool(Icon::Properties, "Override", Some("DIMOVERRIDE"), "Sets dimension variables on selected dimensions (DOV)"),
    tool(Icon::Undo, "Update", Some("DIMUPDATE"), "Puts dimensions back to the current style"),
    tool(Icon::Table, "Tolerance", Some("TOLERANCE"), "Places a geometric tolerance frame (TOL)"),
    tool(Icon::Text, "Inspect", Some("DIMINSPECT"), "Adds an inspection label and rate around dimension text"),
];
const UTILITIES_LARGE: &[Tool] = &[tool(
    Icon::Measure,
    "Measure",
    Some("MEASUREGEOM"),
    "Measures distances, radii, angles, areas and volumes (MEA)",
)];
const UTILITIES_SMALL: &[Tool] = &[
    tool(Icon::Properties, "Select All", Some("SELECTALL"), "Selects every object (Cmd+A)"),
    tool(Icon::Point, "Point Style...", Some("PTYPE"), "How points look, and their size"),
    tool(Icon::Measure, "Drafting Settings...", Some("DSETTINGS"), "Snap, grid, polar tracking, object snaps (DS)"),
];

const NOT_YET: &str = "Not available in Oxidraft yet";

impl OxidraftApp {
    /// Title row: app button, quick-access toolbar, drawing name.
    pub(crate) fn top_bar(&mut self, ui: &mut Ui) {
        let frame = Frame::NONE.fill(theme::top_bar()).inner_margin(Margin::symmetric(0, 0));
        egui::Panel::top("top_bar").frame(frame).exact_size(30.0).show_separator_line(false).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                self.app_button(ui);
                ui.add_space(8.0);
                for (icon, tip, cmd) in [
                    (Icon::New, "New (Cmd+N)", "NEW"),
                    (Icon::Open, "Open (Cmd+O)", "OPEN"),
                    (Icon::Save, "Save (Cmd+S)", "SAVE"),
                ] {
                    if icon_button(ui, icon, 18.0, true, tip).clicked() {
                        self.run(cmd);
                    }
                }
                ui.add_space(6.0);
                if icon_button(ui, Icon::Undo, 18.0, self.doc.can_undo(), "Undo (Cmd+Z)").clicked() {
                    self.run("UNDO");
                }
                if icon_button(ui, Icon::Redo, 18.0, self.doc.can_redo(), "Redo (Shift+Cmd+Z)").clicked() {
                    self.run("REDO");
                }
                // Commands added to the Quick Access Toolbar (CUI).
                for command in self.settings.quick_access.clone() {
                    let r = match self.custom_command(&command).cloned() {
                        // One of your own: its image, else its name.
                        Some(c) => match self.button_image(ui.ctx(), &c.image) {
                            Some(t) => ui.add(
                                egui::Button::image(egui::Image::new(&t).fit_to_exact_size(egui::vec2(16.0, 16.0)))
                                    .frame(false),
                            ),
                            None => ui.add(
                                egui::Button::new(egui::RichText::new(&c.name).size(11.5).color(theme::text()))
                                    .frame(false),
                            ),
                        },
                        None => ui.add(
                            egui::Button::new(
                                egui::RichText::new(command.to_ascii_lowercase()).size(11.5).color(theme::text()),
                            )
                            .frame(false),
                        ),
                    };
                    if r.on_hover_text(self.button_tip(&command)).clicked() {
                        self.run(&command);
                    }
                }
                let dirty = if self.is_dirty() { "*" } else { "" };
                let title = format!("Oxidraft    {}{dirty}", self.file_name());
                let rect = ui.max_rect();
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    title,
                    FontId::proportional(12.5),
                    theme::text(),
                );
            });
        });
    }

    fn app_button(&mut self, ui: &mut Ui) {
        let (rect, response) = ui.allocate_exact_size(vec2(44.0, 30.0), Sense::click());
        let fill = if response.hovered() { theme::BRAND.gamma_multiply(1.2) } else { theme::BRAND };
        ui.painter().rect_filled(rect, 0.0, fill);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, "Ox", FontId::proportional(16.0), Color32::WHITE);
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(180.0);
            for (label, cmd) in [("New", "NEW"), ("Open...", "OPEN"), ("Save", "SAVE"), ("Save As...", "SAVEAS")] {
                if ui.button(label).clicked() {
                    self.run(cmd);
                }
            }
            ui.separator();
            for (label, cmd) in [("Export to PDF...", "EXPORTPDF"), ("Publish...", "PUBLISH"), ("Plot...", "PLOT")] {
                if ui.button(label).clicked() {
                    self.run(cmd);
                }
            }
            ui.menu_button("Drawing Utilities", |ui| {
                for (label, cmd) in
                    [("Units...", "UNITS"), ("Rename...", "RENAME"), ("Audit", "AUDIT"), ("Purge...", "PURGE")]
                {
                    if ui.button(label).clicked() {
                        self.run(cmd);
                    }
                }
            });
            ui.separator();
            if ui.button("Options...").clicked() {
                self.run("OPTIONS");
            }
            if ui.button("Check for Updates...").clicked() {
                self.check_for_updates(ui.ctx());
            }
            ui.label(
                egui::RichText::new(format!("Oxidraft {}", env!("CARGO_PKG_VERSION"))).small().color(theme::text_dim()),
            );
        });
    }

    /// Ribbon: tab strip plus the panels of the selected tab.
    pub(crate) fn ribbon(&mut self, ui: &mut Ui) {
        let tabs = Frame::NONE.fill(theme::top_bar()).inner_margin(Margin { left: 8, right: 8, top: 0, bottom: 0 });
        egui::Panel::top("ribbon_tabs").frame(tabs).exact_size(26.0).show_separator_line(false).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // A layout brings its Layout tab up, as in AutoCAD; model space puts it away.
                let shown = self.shown_space();
                let on_layout = self.doc.layout(shown).is_some();
                if shown != self.ribbon_space {
                    self.ribbon_space = shown;
                    if on_layout {
                        self.ribbon_tab = LAYOUT_TAB;
                    }
                }
                if !on_layout && self.ribbon_tab == LAYOUT_TAB {
                    self.ribbon_tab = 0;
                }
                let names = RIBBON_TABS.iter().chain(on_layout.then_some(&"Layout"));
                for (i, name) in names.enumerate() {
                    if ribbon_tab(ui, name, self.ribbon_tab == i).clicked() {
                        self.ribbon_tab = i;
                    }
                }
                // Tabs of your own, after AutoCAD's.
                for (i, name) in self.custom_tabs().iter().enumerate() {
                    if ribbon_tab(ui, name, self.ribbon_tab == CUSTOM_TAB + i).clicked() {
                        self.ribbon_tab = CUSTOM_TAB + i;
                    }
                }
                if self.ribbon_tab >= CUSTOM_TAB && self.ribbon_tab - CUSTOM_TAB >= self.custom_tabs().len() {
                    self.ribbon_tab = 0;
                }
            });
        });

        let body = Frame::NONE.fill(theme::ribbon()).inner_margin(Margin::symmetric(4, 0));
        egui::Panel::top("ribbon").frame(body).exact_size(98.0).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                // While a reference is edited in place (or a block in the
                // Block Editor), its panel leads every tab.
                if self.in_block_editor() {
                    self.block_editor_panel(ui);
                } else if self.doc.refedit().is_some() {
                    self.edit_reference_panel(ui);
                }
                if self.ribbon_tab >= CUSTOM_TAB {
                    let tab = self.custom_tabs().get(self.ribbon_tab - CUSTOM_TAB).cloned().unwrap_or_default();
                    self.custom_panels(ui, &tab);
                    return;
                }
                if self.ribbon_tab == 0 {
                    self.home_tab(ui);
                } else if self.ribbon_tab == LAYOUT_TAB {
                    self.layout_ribbon(ui);
                } else if RIBBON_TABS[self.ribbon_tab] == "Collaborate" {
                    self.collaborate_tab(ui);
                } else if RIBBON_TABS[self.ribbon_tab] == "Manage" {
                    self.manage_tab(ui);
                } else if RIBBON_TABS[self.ribbon_tab] == "View" {
                    self.view_tab(ui);
                } else if RIBBON_TABS[self.ribbon_tab] == "Annotate" {
                    self.annotate_tab(ui);
                } else if RIBBON_TABS[self.ribbon_tab] == "Insert" {
                    self.insert_tab(ui);
                } else if RIBBON_TABS[self.ribbon_tab] == "Output" {
                    self.output_tab(ui);
                } else {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.add_space(36.0);
                        ui.colored_label(
                            theme::text_dim(),
                            format!("The {} tab is {}.", RIBBON_TABS[self.ribbon_tab], NOT_YET.to_lowercase()),
                        );
                    });
                }
                // Panels of your own on this tab, after its own.
                let tab = if self.ribbon_tab == LAYOUT_TAB { "Layout" } else { RIBBON_TABS[self.ribbon_tab] };
                self.custom_panels(ui, tab);
            });
        });
    }

    /// The tabs CUI's panels make: those not AutoCAD's, in order.
    fn custom_tabs(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in &self.settings.ribbon_panels {
            let builtin = RIBBON_TABS.iter().chain(&["Layout"]).any(|t| t.eq_ignore_ascii_case(&p.tab));
            if !builtin && !p.tab.trim().is_empty() && !out.iter().any(|t| t.eq_ignore_ascii_case(&p.tab)) {
                out.push(p.tab.clone());
            }
        }
        out
    }

    /// CUI's panels on tab `tab`: a button a command, with the ribbon's
    /// icon where it has one; `A|B|C` is a split button, A with B and C
    /// in its drop-down.
    fn custom_panels(&mut self, ui: &mut Ui, tab: &str) {
        let panels: Vec<crate::settings::RibbonPanel> =
            self.settings.ribbon_panels.iter().filter(|p| p.tab.eq_ignore_ascii_case(tab)).cloned().collect();
        for p in panels {
            ribbon_panel(ui, &p.name, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(240.0);
                    for entry in &p.commands {
                        let mut split = entry.split('|').map(str::trim).filter(|c| !c.is_empty());
                        let Some(main) = split.next() else { continue };
                        let others: Vec<&str> = split.collect();
                        self.custom_button(ui, main);
                        if !others.is_empty() {
                            let more = format!("More like {}", self.button_label(main));
                            let r = ui.menu_button("+", |ui| {
                                for c in others {
                                    let label = self.button_label(c);
                                    if ui.button(label).on_hover_text(self.button_tip(c)).clicked() {
                                        self.run(c);
                                        ui.close();
                                    }
                                }
                            });
                            r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &more));
                            r.response.on_hover_text(more.clone());
                        }
                    }
                });
            });
        }
    }

    /// A toolbar's button: a command of your own; else the ribbon's icon
    /// for the command where it has one, else its name.
    pub(crate) fn toolbar_button(&mut self, ui: &mut Ui, command: &str) {
        if self.custom_command(command).is_some() {
            return self.custom_button(ui, command);
        }
        // The ribbon's icon, or one of the other commands with an icon of
        // their own.
        let icon = known_tool(command).map(|t| (t.icon, t.label.replace('\n', " "), t.tip.to_string())).or_else(|| {
            let icon = match command {
                "QNEW" | "NEW" => Icon::New,
                "OPEN" => Icon::Open,
                "QSAVE" | "SAVE" => Icon::Save,
                "U" | "UNDO" => Icon::Undo,
                "REDO" => Icon::Redo,
                "PLOT" => Icon::Plot,
                "PREVIEW" => Icon::PlotPreview,
                "PAN" => Icon::Pan,
                "ZOOM" => Icon::Zoom,
                "MATCHPROP" => Icon::MatchProperties,
                "PROPERTIES" => Icon::Properties,
                "QUICKCALC" => Icon::Calculator,
                "LAYER" => Icon::LayerProperties,
                "TEXT" => Icon::SingleLineText,
                _ => return None,
            };
            let name = command[..1].to_string() + &command[1..].to_ascii_lowercase();
            Some((icon, name, command_tip(command)))
        });
        let r = match icon {
            Some((icon, label, tip)) => {
                let r = icon_button(ui, icon, 16.0, true, &tip);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                r
            }
            None => {
                let label: Vec<String> =
                    command.split_whitespace().map(|w| w[..1].to_string() + &w[1..].to_ascii_lowercase()).collect();
                ui.add_space(3.0);
                let r = ui
                    .add(egui::Button::new(egui::RichText::new(label.join(" ")).size(11.0)).frame(false))
                    .on_hover_text(command_tip(command));
                ui.add_space(3.0);
                r
            }
        };
        if r.clicked() {
            self.run(command);
        }
    }

    /// A button's name: a command of your own's, else the command's.
    pub(crate) fn button_label(&self, command: &str) -> String {
        self.custom_command(command).map_or_else(|| command.to_string(), |c| c.name.clone())
    }

    pub(crate) fn button_tip(&self, command: &str) -> String {
        match self.custom_command(command) {
            Some(c) => format!("{} ({})", c.name, c.macro_text),
            None => command_tip(command),
        }
    }

    /// A CUI panel's command: one of your own with its image (if it has
    /// one; else the ribbon's icon for the command its macro runs, as
    /// AutoCAD's own commands show theirs) and name; else the ribbon's own
    /// tool (its icon) when it has one, else a button with its name.
    fn custom_button(&mut self, ui: &mut Ui, command: &str) {
        if let Some(c) = self.custom_command(command).cloned() {
            let image = self.button_image(ui.ctx(), &c.image);
            let tool = crate::cuix::macro_command(&c.macro_text).and_then(|m| known_tool(&m));
            let r = match (image, tool) {
                (Some(t), _) => ui.add(egui::Button::image_and_text(
                    egui::Image::new(&t).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                    c.name.as_str(),
                )),
                (None, Some(t)) => paint_small_labeled(ui, t.icon, &c.name, true),
                (None, None) => ui.button(c.name.as_str()),
            };
            if r.on_hover_text(self.button_tip(command)).clicked() {
                self.run_macro(&c.macro_text);
            }
            return;
        }
        match known_tool(command) {
            Some(t) => self.small_tool(ui, t),
            None => {
                if ui.button(command).on_hover_text(command_tip(command)).clicked() {
                    self.run(command);
                }
            }
        }
    }

    fn insert_tab(&mut self, ui: &mut Ui) {
        ribbon_panel(ui, "Block", |ui| {
            self.large_tool(ui, &BLOCK_INSERT);
            ui.vertical(|ui| {
                for t in BLOCK_SMALL {
                    self.small_tool(ui, t);
                }
            });
        });
        ribbon_panel(ui, "Block Definition", |ui| {
            ui.vertical(|ui| {
                for t in BLOCK_DEFINITION {
                    self.small_tool(ui, t);
                }
            });
        });
        ribbon_panel(ui, "Reference", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::Attach,
                    "Attach",
                    Some("XATTACH"),
                    "Attaches a DWG or DXF drawing as an external reference (XA)",
                ),
            );
            self.large_tool(
                ui,
                &tool(
                    Icon::ExternalReferences,
                    "External\nReferences",
                    Some("EXTERNALREFERENCES"),
                    "Opens the External References palette (XR)",
                ),
            );
            ui.vertical(|ui| {
                for t in [
                    tool(
                        Icon::Image,
                        "Image",
                        Some("IMAGEATTACH"),
                        "Attaches an image, placed by its georeferencing if it has some; pick many GeoTIFFs to attach them unloaded (IAT)",
                    ),
                    tool(Icon::Pdf, "PDF", Some("PDFATTACH"), "Attaches a page of a PDF as an underlay"),
                    tool(
                        Icon::ImageClip,
                        "Clip",
                        Some("CLIP"),
                        "Clips an xref, block reference, image or PDF underlay to a boundary",
                    ),
                ] {
                    self.small_tool(ui, &t);
                }
            });
            ui.vertical(|ui| {
                self.small_tool(
                    ui,
                    &tool(
                        Icon::EditReference,
                        "Edit Reference",
                        Some("REFEDIT"),
                        "Edits a block reference or xref in place (REFEDIT)",
                    ),
                );
                self.small_tool(
                    ui,
                    &tool(
                        Icon::Image,
                        "Adjust",
                        Some("ADJUST"),
                        "Adjusts an image's brightness, contrast and fade, or a PDF's fade and monochrome",
                    ),
                );
                self.small_tool(
                    ui,
                    &tool(
                        Icon::ImageClip,
                        "Frames",
                        Some("FRAME"),
                        "Frames of images, PDFs and wipeouts: hidden, shown and plotted, or shown only (FRAME)",
                    ),
                );
            });
        });
        ribbon_panel(ui, "Data", |ui| {
            ui.vertical(|ui| {
                for t in [
                    tool(
                        Icon::Text,
                        "Field",
                        Some("FIELD"),
                        "Inserts a field: text that shows the date, the file name, a drawing property or an object's area",
                    ),
                    tool(Icon::Text, "Update Fields", Some("UPDATEFIELD"), "Updates the fields in the selected objects"),
                    tool(Icon::Text, "Hyperlink", Some("HYPERLINK"), "Links objects to a web page or file (Ctrl+K)"),
                ] {
                    self.small_tool(ui, &t);
                }
            });
        });
    }

    /// A large button that shows a setting on (highlighted) or off; returns
    /// true when clicked.
    fn toggle_tool(&mut self, ui: &mut Ui, icon: Icon, label: &'static str, on: bool, tip: &str) -> bool {
        let response = paint_large(ui, &tool(icon, label, None, ""), true);
        if on {
            ui.painter().rect_stroke(
                response.rect.shrink(2.0),
                2.0,
                Stroke::new(1.0, theme::accent()),
                egui::StrokeKind::Inside,
            );
        }
        response.on_hover_text(format!("{tip} ({})", if on { "on" } else { "off" })).clicked()
    }

    /// The View tab: the UCS icon and navigation bar, model viewports,
    /// named views, the palettes, and the file and layout tabs.
    fn view_tab(&mut self, ui: &mut Ui) {
        ribbon_panel(ui, "Viewport Tools", |ui| {
            let vars = &self.engine.vars;
            let (ucs, nav) = (vars.ucs_icon, vars.nav_bar);
            if self.toggle_tool(ui, Icon::Ucs, "UCS\nIcon", ucs, "Shows the UCS icon (UCSICON)") {
                self.engine.vars.ucs_icon = !ucs;
            }
            if self.toggle_tool(ui, Icon::NavigationBar, "Navigation\nBar", nav, "Shows the navigation bar (NAVBAR)") {
                self.engine.vars.nav_bar = !nav;
            }
        });
        ribbon_panel(ui, "Coordinates", |ui| {
            for column in [
                [
                    tool(Icon::Ucs, "World", Some("UCS W"), "Back to the world coordinate system (UCS World)"),
                    tool(Icon::Ucs, "UCS", Some("UCS"), "A new origin, and a point on its X axis (UCS)"),
                    tool(Icon::Ucs, "Object", Some("UCS OB"), "The UCS along an object (UCS Object)"),
                ],
                [
                    tool(Icon::Rotate, "Z", Some("UCS Z"), "The UCS turned about Z (UCS Z)"),
                    tool(Icon::Ucs, "View", Some("UCS V"), "The UCS lined up with the screen (UCS View)"),
                    tool(Icon::Undo, "Previous", Some("UCS P"), "The UCS before (UCS Previous)"),
                ],
                [
                    tool(Icon::Ucs, "Named", Some("UCSMAN"), "Named UCSs to make current, rename or delete (UCSMAN)"),
                    tool(Icon::Ucs, "Plan View", Some("PLAN"), "Turns the view so the UCS's X axis runs across (PLAN)"),
                    tool(Icon::Ucs, "World Plan", Some("PLAN W"), "The view as the world lies (PLAN World)"),
                ],
            ] {
                ui.vertical(|ui| {
                    for t in &column {
                        self.small_tool(ui, t);
                    }
                });
            }
        });
        let viewports = ribbon_panel_more(ui, "Model Viewports", |ui| {
            let config = tool(
                Icon::ViewportConfig,
                "Viewport\nConfiguration",
                Some("VPORTS"),
                "Splits model space into viewports (VPORTS); Ctrl+R makes the next one current",
            );
            let response = paint_large(ui, &config, true).on_hover_text(config.tip);
            egui::Popup::menu(&response).show(|ui| self.tiling_items(ui));
        });
        egui::Popup::menu(&viewports).show(|ui| self.tiling_items(ui));
        ribbon_panel(ui, "Named Views", |ui| {
            self.large_tool(
                ui,
                &tool(Icon::ViewManager, "View\nManager", Some("VIEW"), "Saves, restores and deletes named views (V)"),
            );
            ui.vertical(|ui| {
                let views: Vec<oxidraft_model::NamedView> = self.doc.views().to_vec();
                egui::ComboBox::from_id_salt("named_views").width(130.0).selected_text("Restore view").show_ui(
                    ui,
                    |ui| {
                        if views.is_empty() {
                            ui.colored_label(theme::text_dim(), "No named views");
                        }
                        for v in &views {
                            if ui.selectable_label(false, &v.name).clicked() {
                                self.restore_view(v);
                            }
                        }
                    },
                );
            });
        });
        ribbon_panel(ui, "Palettes", |ui| {
            for t in [
                tool(Icon::Properties, "Properties", Some("PROPERTIES"), "Shows the Properties palette (Cmd+1)"),
                tool(
                    Icon::LayerProperties,
                    "Layer\nProperties",
                    Some("LAYER"),
                    "Opens the Layer Properties Manager (LA)",
                ),
                tool(
                    Icon::ExternalReferences,
                    "External\nReferences",
                    Some("XR"),
                    "Opens the External References palette",
                ),
                tool(Icon::InsertBlock, "Blocks", Some("BLOCKSPALETTE"), "Opens the Blocks palette"),
                tool(Icon::Calculator, "QuickCalc", Some("QUICKCALC"), "Opens the QuickCalc calculator (QC, Cmd+8)"),
                tool(Icon::InsertBlock, "Tool\nPalettes", Some("TOOLPALETTES"), "Opens the tool palettes (TP, Cmd+3)"),
                tool(
                    Icon::ExternalReferences,
                    "Design\nCenter",
                    Some("ADCENTER"),
                    "Opens DesignCenter: other drawings' blocks, layers and styles (ADC, Cmd+2)",
                ),
                tool(
                    Icon::PageSetup,
                    "Sheet Set\nManager",
                    Some("SHEETSET"),
                    "Sheets from drawings' layouts, published and archived together (SSM)",
                ),
            ] {
                self.large_tool(ui, &t);
            }
        });
        ribbon_panel(ui, "Interface", |ui| {
            let (files, layouts) = (self.show_file_tabs, self.show_layout_tabs);
            if self.toggle_tool(ui, Icon::FileTabs, "File\nTabs", files, "Shows the file tabs (FILETAB, FILETABCLOSE)")
            {
                self.show_file_tabs = !files;
            }
            if self.toggle_tool(ui, Icon::LayoutTabs, "Layout\nTabs", layouts, "Shows the Model and layout tabs") {
                self.show_layout_tabs = !layouts;
            }
            ui.vertical(|ui| {
                for t in [
                    tool(Icon::FileTabs, "Switch Windows", Some("SWITCHWINDOWS"), "The next open drawing (Ctrl+Tab)"),
                    tool(Icon::FileTabs, "Tile Vertically", Some("SYSWINDOWS V"), "The open drawings side by side"),
                    tool(Icon::FileTabs, "Cascade", Some("SYSWINDOWS C"), "The open drawings cascaded"),
                ] {
                    self.small_tool(ui, &t);
                }
            });
        });
    }

    /// The Collaborate tab: comparing with another drawing, and counting
    /// blocks.
    fn collaborate_tab(&mut self, ui: &mut Ui) {
        ribbon_panel(ui, "Compare", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::Compare,
                    "DWG\nCompare",
                    Some("COMPARE"),
                    "Shows what's only here, only in another drawing, or in both",
                ),
            );
            let exit = tool(Icon::CompareExit, "Exit\nCompare", Some("COMPAREEXIT"), "Stops comparing (COMPAREEXIT)");
            self.large_tool(ui, &exit);
        });
        ribbon_panel(ui, "Share", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::Pdf,
                    "Share",
                    Some("SHARE"),
                    "Packs the drawing, a PDF of its sheets and its files to send",
                ),
            );
        });
        ribbon_panel(ui, "Markup", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::Pdf,
                    "Markup\nImport",
                    Some("MARKUP"),
                    "Lays a marked-up plot (PDF or image) over its sheet, on layer MARKUP",
                ),
            );
        });
        ribbon_panel(ui, "Count", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::Count,
                    "Count",
                    Some("COUNT"),
                    "Counts a block's references and selects them; Enter lists every block",
                ),
            );
        });
    }

    /// The Manage tab: cleaning the drawing up, and the user's aliases.
    fn manage_tab(&mut self, ui: &mut Ui) {
        ribbon_panel(ui, "Cleanup", |ui| {
            for t in [
                tool(Icon::Purge, "Purge", Some("PURGE"), "Removes layers, blocks and linetypes nothing uses (PU)"),
                tool(
                    Icon::Overkill,
                    "Overkill",
                    Some("OVERKILL"),
                    "Deletes duplicate objects and joins lines along each other (OV)",
                ),
                tool(Icon::Audit, "Audit", Some("AUDIT"), "Checks the drawing for broken objects, and mends them"),
            ] {
                self.large_tool(ui, &t);
            }
        });
        ribbon_panel(ui, "Customization", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::Properties,
                    "User\nInterface",
                    Some("CUI"),
                    "Keyboard shortcuts and the Quick Access Toolbar",
                ),
            );
            self.large_tool(ui, &tool(Icon::Aliases, "Edit\nAliases", Some("ALIASEDIT"), "Your own command aliases"));
            self.large_tool(
                ui,
                &tool(Icon::Properties, "Workspace", Some("WORKSPACE"), "Make a workspace current, save or delete one"),
            );
        });
    }

    /// The model viewport configurations, as VPORTS makes them.
    fn tiling_items(&mut self, ui: &mut Ui) {
        use oxidraft_commands::Tiling;
        for (tiling, label) in [
            (Tiling::Single, "Single"),
            (Tiling::TwoVertical, "Two: Vertical"),
            (Tiling::TwoHorizontal, "Two: Horizontal"),
            (Tiling::ThreeRight, "Three: Right"),
            (Tiling::ThreeLeft, "Three: Left"),
            (Tiling::FourEqual, "Four: Equal"),
        ] {
            if ui.selectable_label(self.tiles.tiling == tiling, label).clicked() {
                if self.doc.space() != oxidraft_model::MODEL || self.active_viewport.is_some() {
                    self.engine.log("Model viewports are for model space: switch to the Model tab.");
                } else {
                    self.set_tiling(tiling);
                }
            }
        }
    }

    /// The Annotate tab: text, dimensions, leaders, tables and markup.
    fn annotate_tab(&mut self, ui: &mut Ui) {
        let text_panel = ribbon_panel_more(ui, "Text", |ui| {
            for t in ANNOTATE_TEXT {
                self.large_tool(ui, t);
            }
            ui.vertical(|ui| {
                ui.add_space(2.0);
                self.text_style_combo(ui);
                for t in [
                    tool(Icon::Zoom, "Find Text", Some("FIND"), "Finds and replaces text"),
                    tool(Icon::Scale, "Scale", Some("SCALETEXT"), "Gives text a new height, each about its own point"),
                    tool(Icon::Text, "Justify", Some("JUSTIFYTEXT"), "Hangs text from another of its points"),
                ] {
                    self.small_tool(ui, &t);
                }
            });
        });
        // The panel's title opens STYLE, as its arrow does in AutoCAD.
        if text_panel.clicked() {
            self.run("STYLE");
        }
        let dimensions = ribbon_panel_more(ui, "Dimensions", |ui| {
            self.large_tool(
                ui,
                &tool(Icon::Dimension, "Dimension", Some("DIM"), "Dimensions what you pick, or two points (DIM)"),
            );
            for column in ANNOTATE_DIMENSIONS.chunks(3) {
                ui.vertical(|ui| {
                    for t in column {
                        self.small_tool(ui, t);
                    }
                });
            }
        });
        egui::Popup::menu(&dimensions).show(|ui| {
            for t in ANNOTATE_DIMENSIONS_MORE {
                self.small_tool(ui, t);
            }
        });
        ribbon_panel(ui, "Dimension Style", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::Dimension,
                    "Dimension\nStyle",
                    Some("DIMSTYLE"),
                    "Sets text height, arrows, extension lines and precision (D)",
                ),
            );
        });
        ribbon_panel(ui, "Centerlines", |ui| {
            ui.vertical(|ui| {
                self.small_tool(
                    ui,
                    &tool(Icon::Circle, "Center Mark", Some("CENTERMARK"), "Marks circles' and arcs' centers (CM)"),
                );
                self.small_tool(
                    ui,
                    &tool(
                        Icon::Xline,
                        "Centerline",
                        Some("CENTERLINE"),
                        "Draws the center line between two lines (CL)",
                    ),
                );
            });
        });
        ribbon_panel(ui, "Leaders", |ui| {
            self.large_tool(
                ui,
                &tool(Icon::Leader, "Multileader", Some("MLEADER"), "Creates a leader with text (MLD)"),
            );
            ui.vertical(|ui| {
                for t in [
                    tool(
                        Icon::Leader,
                        "Add/Remove Leader",
                        Some("MLEADEREDIT"),
                        "Adds arrowheads to a multileader (MLE)",
                    ),
                    tool(Icon::Leader, "Align", Some("MLEADERALIGN"), "Lines up multileaders' text (MLA)"),
                    tool(
                        Icon::Leader,
                        "Collect",
                        Some("MLEADERCOLLECT"),
                        "Gathers multileaders' blocks onto one (MLC)",
                    ),
                ] {
                    self.small_tool(ui, &t);
                }
            });
            ui.vertical(|ui| {
                let style = tool(
                    Icon::Properties,
                    "Multileader Style",
                    Some("MLEADERSTYLE"),
                    "Arrowhead, content (text or a tag block), landing (MLS)",
                );
                self.small_tool(ui, &style);
            });
        });
        ribbon_panel(ui, "Tables", |ui| {
            self.large_tool(ui, &tool(Icon::Table, "Table", Some("TABLE"), "Inserts a table of rows and columns (TB)"));
            ui.vertical(|ui| {
                let style = tool(
                    Icon::Properties,
                    "Table Style",
                    Some("TABLESTYLE"),
                    "Text height and margins of new tables (TS)",
                );
                self.small_tool(ui, &style);
            });
        });
        ribbon_panel(ui, "Markup", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::RevCloud,
                    "Revision\nCloud",
                    Some("REVCLOUD"),
                    "Draws a cloud of arcs around an area or object",
                ),
            );
            self.large_tool(
                ui,
                &tool(
                    Icon::Hatch,
                    "Wipeout",
                    Some("WIPEOUT"),
                    "Hides what's under a polygon; Frames shows or hides frames",
                ),
            );
        });
    }

    /// The Layout tab, on a layout's tab: new layouts and page setup, and
    /// its viewports.
    fn layout_ribbon(&mut self, ui: &mut Ui) {
        ribbon_panel(ui, "Layout", |ui| {
            for t in LAYOUT_TOOLS {
                self.large_tool(ui, t);
            }
        });
        ribbon_panel(ui, "Layout Viewports", |ui| {
            self.large_tool(
                ui,
                &tool(Icon::Viewport, "Rectangular", Some("MVIEW"), "Creates a rectangular viewport (MV)"),
            );
            ui.vertical(|ui| {
                for t in [
                    tool(Icon::Polygon, "Polygonal", Some("MVIEW P"), "Creates a viewport bounded by points you pick"),
                    tool(
                        Icon::Circle,
                        "Object",
                        Some("MVIEW O"),
                        "Makes a closed polyline, circle or ellipse a viewport",
                    ),
                    tool(
                        Icon::ImageClip,
                        "Clip",
                        Some("VPCLIP"),
                        "Clips a viewport to a shape or points, or Deletes its clipping",
                    ),
                ] {
                    self.small_tool(ui, &t);
                }
            });
            ui.vertical(|ui| {
                for t in VIEWPORT_TOOLS {
                    self.small_tool(ui, t);
                }
            });
        });
    }

    fn output_tab(&mut self, ui: &mut Ui) {
        ribbon_panel(ui, "Plot", |ui| {
            self.large_tool(ui, &tool(Icon::Plot, "Plot", Some("PLOT"), "Plots the drawing to a PDF file (Cmd+P)"));
            self.large_tool(
                ui,
                &tool(Icon::Plot, "Batch\nPlot", Some("PUBLISH"), "Plots several sheets to one PDF (PUBLISH)"),
            );
            ui.vertical(|ui| {
                self.small_tool(
                    ui,
                    &tool(Icon::PlotPreview, "Preview", Some("PREVIEW"), "Shows the drawing as it will plot"),
                );
                self.small_tool(
                    ui,
                    &tool(Icon::PageSetup, "Page Setup Manager", Some("PAGESETUP"), "Sets how a layout plots"),
                );
                self.small_tool(
                    ui,
                    &tool(
                        Icon::PageSetup,
                        "Plot Style Manager",
                        Some("STYLESMANAGER"),
                        "Plot style tables (.ctb): each color's pen, screening and lineweight",
                    ),
                );
            });
            ui.vertical(|ui| {
                self.small_tool(
                    ui,
                    &tool(
                        Icon::Plot,
                        "Plotter Manager",
                        Some("PLOTTERMANAGER"),
                        "Plotter configurations, paper sizes of your own, image plot resolution",
                    ),
                );
            });
        });
        ribbon_panel(ui, "Export to DWF/PDF", |ui| {
            self.large_tool(
                ui,
                &tool(Icon::Pdf, "Export", Some("EXPORTPDF"), "Saves the current layout, or all of them, as a PDF"),
            );
            ui.vertical(|ui| {
                self.small_tool(
                    ui,
                    &tool(Icon::Pdf, "DWFx", Some("EXPORTDWFX"), "Saves the current layout, or all of them, as DWFx"),
                );
                self.small_tool(
                    ui,
                    &tool(Icon::Pdf, "Metafile", Some("WMFOUT"), "Saves objects as a Windows Metafile (WMF)"),
                );
            });
        });
    }

    fn home_tab(&mut self, ui: &mut Ui) {
        // The most used tools show; the rest are in each panel's slide-out,
        // opened from its title, as in AutoCAD.
        let draw = ribbon_panel_more(ui, "Draw", |ui| {
            for t in DRAW_LARGE {
                self.large_tool(ui, t);
            }
            ui.vertical(|ui| {
                for t in DRAW_SMALL {
                    self.small_tool(ui, t);
                }
            });
        });
        egui::Popup::menu(&draw).show(|ui| {
            for t in DRAW_MORE {
                self.small_tool(ui, t);
            }
        });
        let modify = ribbon_panel_more(ui, "Modify", |ui| {
            ui.vertical(|ui| {
                for row in MODIFY_GRID {
                    ui.horizontal(|ui| {
                        for t in row {
                            self.small_tool(ui, t);
                        }
                    });
                }
            });
        });
        egui::Popup::menu(&modify).show(|ui| {
            for t in MODIFY_MORE {
                self.small_tool(ui, t);
            }
            // Draw order, as the panel's split button offers it.
            for (icon, label, option, tip) in [
                (
                    Icon::BringToFront,
                    "Bring to Front",
                    "Front",
                    "Draws the selected objects over everything (DRAWORDER)",
                ),
                (Icon::SendToBack, "Send to Back", "Back", "Draws the selected objects under everything (DRAWORDER)"),
            ] {
                let response = paint_small(ui, &tool(icon, label, None, tip), true);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
                if response.on_hover_text(tip).clicked() {
                    self.engine.draw_order(option, &mut self.doc);
                }
            }
        });
        ribbon_panel(ui, "Annotation", |ui| {
            for t in ANNOTATION_LARGE {
                self.large_tool(ui, t);
            }
            ui.vertical(|ui| {
                for t in ANNOTATION_SMALL {
                    self.small_tool(ui, t);
                }
            });
        });
        let layers = ribbon_panel_more(ui, "Layers", |ui| {
            self.large_tool(
                ui,
                &tool(Icon::LayerProperties, "Layer\nProperties", Some("LAYER"), "Manages layers (LA)"),
            );
            ui.vertical(|ui| {
                ui.add_space(6.0);
                self.layer_combo(ui);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let make_current = tool(
                        Icon::MakeCurrent,
                        "Make Current",
                        Some("LAYMCUR"),
                        "Makes a selected object's layer current",
                    );
                    self.small_tool(ui, &make_current);
                    for t in LAYER_TOOLS {
                        self.small_tool(ui, t);
                    }
                });
            });
        });
        egui::Popup::menu(&layers).show(|ui| {
            for t in LAYERS_MORE {
                self.small_tool(ui, t);
            }
        });
        ribbon_panel(ui, "Block", |ui| {
            self.large_tool(ui, &BLOCK_INSERT);
            ui.vertical(|ui| {
                for t in BLOCK_SMALL {
                    self.small_tool(ui, t);
                }
            });
        });
        ribbon_panel(ui, "Properties", |ui| {
            self.large_tool(
                ui,
                &tool(
                    Icon::MatchProperties,
                    "Match\nProperties",
                    Some("MATCHPROP"),
                    "Copies layer and color to other objects (MA)",
                ),
            );
            self.property_combos(ui);
        });
        ribbon_panel(ui, "Groups", |ui| {
            self.large_tool(ui, &tool(Icon::Group, "Group", Some("GROUP"), "Makes the selected objects a group (G)"));
            ui.vertical(|ui| {
                self.small_tool(ui, &tool(Icon::Ungroup, "Ungroup", Some("UNGROUP"), "Takes a group apart"));
                self.small_tool(
                    ui,
                    &tool(
                        Icon::Properties,
                        "Group Edit",
                        Some("GROUPEDIT"),
                        "Adds objects to a group, removes them, or renames it",
                    ),
                );
                let on = self.engine.vars.group_select;
                let label = if on { "Selection On" } else { "Selection Off" };
                let response = paint_small(ui, &tool(Icon::Group, label, None, ""), true);
                if response
                    .on_hover_text("Group Selection: picking one object picks its group (Ctrl+Shift+A)")
                    .clicked()
                {
                    self.toggle_group_selection();
                }
            });
        });
        ribbon_panel(ui, "Utilities", |ui| {
            for t in UTILITIES_LARGE {
                self.large_tool(ui, t);
            }
            ui.vertical(|ui| {
                for t in UTILITIES_SMALL {
                    self.small_tool(ui, t);
                }
            });
        });
    }

    /// Ctrl+Shift+A (PICKSTYLE): picking an object of a group picks it all, or not.
    pub(crate) fn toggle_group_selection(&mut self) {
        self.engine.vars.group_select = !self.engine.vars.group_select;
        let on = self.engine.vars.group_select;
        self.engine.log(format!("<Group {}>", if on { "on" } else { "off" }));
    }

    /// The Block Editor's panel: save the block, close the editor.
    fn block_editor_panel(&mut self, ui: &mut Ui) {
        let name = self.doc.refedit().map(|r| r.block.clone()).unwrap_or_default();
        ribbon_panel(ui, &format!("Block Editor - {name}"), |ui| {
            let save = tool(Icon::SaveReference, "Save\nBlock", None, "Saves the block definition (BSAVE)");
            let close = tool(Icon::DiscardReference, "Close Block\nEditor", None, "Closes the Block Editor (BCLOSE)");
            for (t, command) in [(save, "BSAVE"), (close, "BCLOSE")] {
                let response = paint_large(ui, &t, true);
                let label = t.label.replace('\n', " ");
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                if response.on_hover_text(t.tip).clicked() {
                    self.run(command);
                }
            }
        });
        // Making it dynamic: parameters, their actions, visibility states.
        ribbon_panel(ui, "Dynamic", |ui| {
            ui.vertical(|ui| {
                for t in [
                    tool(Icon::Dimension, "Parameter", Some("BPARAMETER"), "A Linear, Flip or Rotation parameter"),
                    tool(Icon::Stretch, "Stretch Action", Some("BACTION"), "What a linear parameter stretches"),
                    tool(Icon::Properties, "Visibility States", Some("BVSTATE"), "New, current or deleted states"),
                ] {
                    self.small_tool(ui, &t);
                }
            });
            ui.vertical(|ui| {
                for t in [
                    tool(Icon::Erase, "Make Invisible", Some("BVHIDE"), "Hides objects in the current state"),
                    tool(Icon::Properties, "Make Visible", Some("BVSHOW"), "Shows objects in the current state"),
                ] {
                    self.small_tool(ui, &t);
                }
            });
        });
    }

    /// The Edit Reference panel, shown while a reference is edited in place.
    fn edit_reference_panel(&mut self, ui: &mut Ui) {
        ribbon_panel(ui, "Edit Reference", |ui| {
            let save = tool(Icon::SaveReference, "Save\nChanges", None, "Saves the edits to the reference (REFCLOSE)");
            let discard = tool(
                Icon::DiscardReference,
                "Discard\nChanges",
                None,
                "Discards the edits to the reference (REFCLOSE)",
            );
            for (t, action) in [(save, "Save"), (discard, "Discard")] {
                let response = paint_large(ui, &t, true);
                let label = t.label.replace('\n', " ");
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                if response.on_hover_text(t.tip).clicked() {
                    self.command_text.clear();
                    self.engine.edit_reference(action, &mut self.doc);
                }
            }
            ui.vertical(|ui| {
                let add =
                    tool(Icon::AddToWorkingSet, "Add to Working Set", None, "Adds objects to the working set (REFSET)");
                let remove = tool(
                    Icon::RemoveFromWorkingSet,
                    "Remove from Working Set",
                    None,
                    "Takes objects out of the working set (REFSET)",
                );
                for (t, action) in [(add, "Add"), (remove, "Remove")] {
                    let response = paint_small(ui, &t, true);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, t.label));
                    if response.on_hover_text(t.tip).clicked() {
                        self.command_text.clear();
                        self.engine.edit_reference(action, &mut self.doc);
                    }
                }
            });
        });
    }

    fn large_tool(&mut self, ui: &mut Ui, t: &Tool) {
        let response = paint_large(ui, t, t.command.is_some());
        self.tool_response(&response, t);
    }

    fn small_tool(&mut self, ui: &mut Ui, t: &Tool) {
        let response = paint_small(ui, t, t.command.is_some());
        self.tool_response(&response, t);
    }

    fn tool_response(&mut self, response: &Response, t: &Tool) {
        let response = match t.command {
            Some(_) => response.clone().on_hover_text(t.tip),
            None => response.clone().on_hover_text(format!("{} — {NOT_YET}", t.tip)),
        };
        if response.clicked()
            && let Some(cmd) = t.command
        {
            self.run(cmd);
        }
    }

    /// The layer dropdown: each row has LT's on/freeze/lock toggles and color.
    /// With nothing selected it shows the current layer, and picking a row
    /// makes it current. With objects selected it shows their layer (blank if
    /// they differ), and picking a row moves them to it, as in AutoCAD.
    fn layer_combo(&mut self, ui: &mut Ui) {
        let selected = if self.engine.is_active() { Vec::new() } else { self.engine.selection().to_vec() };
        let shown_name = if selected.is_empty() {
            Some(self.doc.current_layer().to_string())
        } else {
            common(selected.iter().filter_map(|&id| self.doc.entity(id)).map(|e| e.layer.clone()))
        };
        let shown_layer = shown_name.as_deref().and_then(|n| self.doc.layer(n)).cloned();
        let layers = self.doc.layers().to_vec();
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 1.0;
            // Objects on different layers: keep the room, show nothing.
            let blank = oxidraft_model::Layer::new("0", 7);
            ui.scope(|ui| {
                if shown_layer.is_none() {
                    ui.set_invisible();
                }
                let layer = shown_layer.as_ref().unwrap_or(&blank);
                if let Some(changed) = layer_state_icons(ui, layer) {
                    self.doc.update_layer(changed);
                }
                ui.add_space(3.0);
                color_swatch(ui, layer.color);
            });
            ui.add_space(3.0);
            let combo = egui::ComboBox::from_id_salt("layer_combo")
                .width(150.0)
                .selected_text(shown_name.clone().unwrap_or_default());
            let shown = combo.show_ui(ui, |ui| {
                ui.set_min_width(230.0);
                for layer in layers {
                    ui.horizontal(|ui| {
                        if let Some(changed) = layer_state_icons(ui, &layer) {
                            self.doc.update_layer(changed);
                        }
                        color_swatch(ui, layer.color);
                        if ui.selectable_label(shown_name.as_ref() == Some(&layer.name), &layer.name).clicked() {
                            chosen = Some(layer.name.clone());
                        }
                    });
                }
            });
            shown.response.widget_info(|| egui::WidgetInfo {
                current_text_value: shown_name.clone(),
                ..egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Layer control")
            });
        });
        match chosen {
            // An xref's layers stay its own: nothing new goes on them.
            Some(name) if oxidraft_model::is_dependent(&name) => {
                self.engine.log(format!("\"{name}\" comes from an external reference; objects can't be put on it."));
            }
            Some(name) if selected.is_empty() => {
                self.doc.set_current_layer(&name);
            }
            Some(name) => self.edit(selected, |e| e.layer = name.clone()),
            None => {}
        }
    }

    /// Drawing tabs under the ribbon.
    pub(crate) fn file_tabs(&mut self, ui: &mut Ui) {
        let frame = Frame::NONE.fill(theme::strip()).inner_margin(Margin { left: 6, right: 6, top: 3, bottom: 0 });
        // Every drawing open in the window, or this one alone.
        let tabs = if self.tabs.is_empty() { vec![(self.file_name(), self.is_dirty())] } else { self.tabs.clone() };
        let current = if self.tabs.is_empty() { 0 } else { self.tab };
        egui::Panel::top("file_tabs").frame(frame).exact_size(28.0).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, (name, dirty)) in tabs.iter().enumerate() {
                    let label = format!("{name}{}", if *dirty { "*" } else { "" });
                    let active = i == current;
                    let color = if active { theme::text() } else { theme::text_dim() };
                    let galley = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(12.0), color);
                    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 44.0, 25.0), Sense::click());
                    let close = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(16.0, 16.0));
                    let over_close = response.hover_pos().is_some_and(|p| close.contains(p));
                    let fill = if active || response.hovered() { theme::tab() } else { theme::strip() };
                    ui.painter().rect_filled(rect, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, fill);
                    if active {
                        ui.painter()
                            .line_segment([rect.left_top(), rect.right_top()], Stroke::new(2.0, theme::accent()));
                    }
                    ui.painter().galley(
                        pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
                        galley,
                        color,
                    );
                    let x = if over_close { theme::text() } else { theme::text_dim() };
                    ui.painter().text(close.center(), Align2::CENTER_CENTER, "×", FontId::proportional(13.0), x);
                    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, active, &label));
                    if response.clicked() {
                        use crate::drawings::TabRequest;
                        self.tab_request = Some(match (over_close, active) {
                            (true, true) => {
                                self.run("CLOSE");
                                continue;
                            }
                            (true, false) => TabRequest::CloseAsking(i),
                            (false, _) => TabRequest::Switch(i),
                        });
                    }
                }
                ui.add_space(4.0);
                if ui.add(egui::Button::new("+").frame(false)).on_hover_text("New drawing").clicked() {
                    self.run("NEW");
                }
            });
        });
    }

    /// Status bar: layout tabs, coordinates and drafting toggles.
    pub(crate) fn status_bar(&mut self, ui: &mut Ui) {
        let frame = Frame::NONE.fill(theme::strip()).inner_margin(Margin::symmetric(6, 0));
        egui::Panel::bottom("status").frame(frame).exact_size(28.0).show_separator_line(false).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // Model, then the layouts; a drawing without any shows Layout1,
                // made when it's first opened.
                let current = self.shown_space();
                let mut tabs: Vec<(Option<oxidraft_model::Space>, String)> =
                    vec![(Some(oxidraft_model::MODEL), "Model".into())];
                tabs.extend(self.doc.layouts().iter().map(|l| (Some(l.id), l.name.clone())));
                if tabs.len() == 1 {
                    tabs.push((None, "Layout1".into()));
                }
                let (mut switch, mut action) = (None, None);
                // The View tab's Layout Tabs can hide them.
                let tabs = if self.show_layout_tabs { tabs } else { Vec::new() };
                for (space, name) in tabs {
                    // Double-clicking a layout's tab (or its menu's Rename) renames it in place.
                    if space.is_some() && self.renaming_layout.as_ref().map(|r| r.space) == space {
                        self.rename_field(ui);
                        continue;
                    }
                    let tab = layout_tab(ui, &name, space == Some(current));
                    let model = space == Some(oxidraft_model::MODEL);
                    let tab = tab.on_hover_text(if model { "Model space" } else { "A layout: a sheet to plot" });
                    if tab.clicked() {
                        switch = Some(space);
                    }
                    if let Some(s) = space.filter(|_| !model && tab.double_clicked()) {
                        action = Some(TabAction::Rename(s));
                    }
                    tab.context_menu(|ui| {
                        let mut item = |label: &str, a: TabAction| {
                            if ui.button(label).clicked() {
                                action = Some(a);
                            }
                        };
                        item("New Layout", TabAction::New);
                        item("From Template...", TabAction::FromTemplate);
                        if let Some(s) = space.filter(|_| !model) {
                            item("Delete", TabAction::Delete(s));
                            item("Rename", TabAction::Rename(s));
                            item("Move or Copy...", TabAction::MoveOrCopy(s));
                            item("Copy", TabAction::Copy(s));
                            item("Activate Model Tab", TabAction::Model);
                            item("Page Setup...", TabAction::PageSetup(s));
                            item("Plot...", TabAction::Plot(s));
                        }
                        if model {
                            item("Page Setup...", TabAction::PageSetup(oxidraft_model::MODEL));
                            item("Plot...", TabAction::Plot(oxidraft_model::MODEL));
                        }
                    });
                }
                if let Some(space) = switch.filter(|s| *s != Some(current)) {
                    let space = space.unwrap_or_else(|| self.first_layout());
                    self.switch_space(space);
                }
                if let Some(a) = action {
                    self.tab_action(a);
                }
                ui.add_space(12.0);
                let l = oxidraft_model::units::length;
                // In the current UCS.
                let ucs = self.doc.ucs();
                let coords = self
                    .cursor
                    .map(|p| ucs.to_ucs(p))
                    .map_or(String::new(), |p| format!("{}, {}, {}", l(p.x), l(p.y), l(0.0)));
                ui.add_sized(
                    [210.0, 20.0],
                    egui::Label::new(egui::RichText::new(coords).monospace().color(theme::text_dim())),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.update_badge(ui);
                    self.workspace_menu(ui);
                    ui.add_space(6.0);
                    // The annotation scale (CANNOSCALE), and its two toggles.
                    let name = oxidraft_model::annoscale::scale_name(self.doc.anno_scale());
                    let menu = ui.menu_button(egui::RichText::new(name).color(theme::text_dim()), |ui| {
                        for (name, s) in self.scale_list() {
                            if ui.selectable_label(self.doc.anno_scale() == s, name).clicked() {
                                self.set_anno_scale(s);
                                ui.close();
                            }
                        }
                        if ui.button("Custom...").clicked() {
                            self.open_scale_list();
                            ui.close();
                        }
                        ui.separator();
                        let mut all = self.doc.anno_all_visible();
                        if ui.checkbox(&mut all, "Show annotative objects at every scale").changed() {
                            self.doc.set_anno_all_visible(all);
                        }
                        ui.checkbox(
                            &mut self.anno_autoscale,
                            "Add scales to annotative objects when the scale changes",
                        );
                    });
                    menu.response
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Annotation Scale"));
                    menu.response.on_hover_text("Annotation scale (CANNOSCALE): annotative objects are sized for it");
                    ui.add_space(6.0);
                    let mut cycling = self.selection_cycling;
                    if status_toggle(
                        ui,
                        "SELCYCLE",
                        &mut cycling,
                        "Selection Cycling: choose among objects on top of each other (Ctrl+W)",
                    ) {
                        self.selection_cycling = cycling;
                        self.log_toggle("Selection cycling", cycling);
                    }
                    let mut iso = self.engine.vars.isoplane.is_some();
                    if status_toggle(ui, "ISO", &mut iso, "Isometric Drafting (ISODRAFT); F5 changes the plane") {
                        self.engine.vars.isoplane = iso.then_some(oxidraft_commands::IsoPlane::Top);
                        self.log_toggle("Isometric drafting", iso);
                    }
                    let mut lwt = self.show_lineweights;
                    if status_toggle(ui, "LWT", &mut lwt, "Show/Hide Lineweight") {
                        self.show_lineweights = lwt;
                        self.log_toggle("Lineweight display", lwt);
                    }
                    let mut dyn_on = self.engine.dynamic_input;
                    if status_toggle(ui, "DYN", &mut dyn_on, "Dynamic Input (F12)") {
                        self.engine.dynamic_input = dyn_on;
                        self.log_toggle("Dynamic Input", dyn_on);
                    }
                    // AutoCAD's OSNAP dropdown: which running snaps are on.
                    let (rect, arrow) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::click());
                    if arrow.hovered() {
                        ui.painter().rect_filled(rect, 2.0, theme::ribbon_hover());
                    }
                    let c = rect.center();
                    let tip = vec![c + vec2(-3.5, -1.5), c + vec2(3.5, -1.5), c + vec2(0.0, 2.5)];
                    ui.painter().add(Shape::convex_polygon(tip, theme::text_dim(), Stroke::NONE));
                    let arrow = arrow.on_hover_text("Running object snaps");
                    egui::Popup::menu(&arrow).show(|ui| {
                        for kind in oxidraft_model::snap::SnapKind::ALL {
                            let mut on = self.osnap_modes.contains(&kind);
                            if ui.checkbox(&mut on, kind.label()).changed() {
                                self.osnap_modes.retain(|k| *k != kind);
                                if on {
                                    self.osnap_modes.push(kind);
                                    self.osnap = true;
                                }
                            }
                        }
                        ui.separator();
                        if ui.button("Object Snap Settings...").clicked() {
                            self.open_drafting_settings(2);
                        }
                    });
                    let mut osnap = self.osnap;
                    if status_toggle(ui, "OSNAP", &mut osnap, "Object Snap (F3)") {
                        self.osnap = osnap;
                        self.log_toggle("Osnap", osnap);
                    }
                    let mut polar = self.engine.polar();
                    if status_toggle(ui, "POLAR", &mut polar, "Polar Tracking (F10)") {
                        self.engine.set_polar(polar);
                        self.log_toggle("Polar", polar);
                    }
                    let mut ortho = self.engine.ortho();
                    if status_toggle(ui, "ORTHO", &mut ortho, "Ortho Mode (F8)") {
                        self.engine.set_ortho(ortho);
                        self.log_toggle("Ortho", ortho);
                    }
                    let mut snap = self.engine.vars.snap;
                    let tip = format!("Snap Mode (F9): every {:.4} (SNAP)", self.engine.vars.snap_spacing);
                    if status_toggle(ui, "SNAP", &mut snap, &tip) {
                        self.engine.vars.snap = snap;
                        self.log_toggle("Snap", snap);
                    }
                    let mut grid = self.grid;
                    if status_toggle(ui, "GRID", &mut grid, "Grid Display (F7)") {
                        self.grid = grid;
                        self.log_toggle("Grid", grid);
                    }
                    // In a layout, switches between its sheet and a viewport (MSPACE, PSPACE).
                    let space = if self.doc.layout(self.doc.space()).is_some() { "PAPER" } else { "MODEL" };
                    let label = ui.add(
                        egui::Label::new(egui::RichText::new(space).strong().color(theme::text()))
                            .sense(egui::Sense::click()),
                    );
                    if self.doc.layout(self.shown_space()).is_some()
                        && label.on_hover_text("Paper or model space (MSPACE, PSPACE)").clicked()
                    {
                        self.run(if self.active_viewport.is_some() { "PSPACE" } else { "MSPACE" });
                    }
                });
            });
        });
    }
}

impl OxidraftApp {
    /// "Update available" / "Restart to update" on the status bar.
    fn update_badge(&mut self, ui: &mut Ui) {
        use crate::update::UpdateState;
        let text = match self.updater.state() {
            UpdateState::Available(r) => format!("Update {} available", r.tag_name),
            UpdateState::Installing(_) => "Updating...".into(),
            UpdateState::Installed(v) => format!("Restart to use {v}"),
            _ => return,
        };
        let button = egui::Button::new(egui::RichText::new(text).color(Color32::WHITE).size(11.5))
            .fill(theme::accent().gamma_multiply(0.8))
            .corner_radius(3.0);
        if ui.add(button).on_hover_text("Oxidraft update").clicked() {
            self.update_dialog = true;
        }
        ui.add_space(8.0);
    }
}

/// A large ribbon button: its icon above its label.
fn paint_large(ui: &mut Ui, t: &Tool, enabled: bool) -> Response {
    // As wide as its longest line of label needs.
    let font = FontId::proportional(11.5);
    let widest = t
        .label
        .lines()
        .map(|l| ui.painter().layout_no_wrap(l.to_string(), font.clone(), theme::text()).size().x)
        .fold(0.0, f32::max);
    let (rect, response) = ui.allocate_exact_size(vec2((widest + 10.0).max(58.0), 76.0), Sense::click());
    paint_hover(ui, rect, &response, enabled);
    let color = if enabled { theme::text() } else { theme::text_disabled() };
    let icon = Rect::from_center_size(pos2(rect.center().x, rect.top() + 22.0), vec2(30.0, 30.0));
    icons::paint(ui.painter(), icon, t.icon, color, enabled);
    ui.painter().text(
        pos2(rect.center().x, rect.top() + 42.0),
        Align2::CENTER_TOP,
        t.label,
        FontId::proportional(11.5),
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, t.label.replace('\n', " ")));
    response
}

/// A small ribbon button: its icon, then its label if it has one.
fn paint_small(ui: &mut Ui, t: &Tool, enabled: bool) -> Response {
    paint_small_labeled(ui, t.icon, t.label, enabled)
}

/// A small button: an icon, and a label beside it.
fn paint_small_labeled(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let font = FontId::proportional(11.5);
    let text_w = if label.is_empty() {
        0.0
    } else {
        ui.painter().layout_no_wrap(label.into(), font.clone(), theme::text()).size().x + 6.0
    };
    let (rect, response) = ui
        .allocate_exact_size(vec2(24.0 + text_w.max(if label.is_empty() { 0.0 } else { 46.0 }), 24.0), Sense::click());
    paint_hover(ui, rect, &response, enabled);
    let color = if enabled { theme::text() } else { theme::text_disabled() };
    let at = Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(ui.painter(), at, icon, color, enabled);
    if !label.is_empty() {
        ui.painter().text(pos2(rect.left() + 24.0, rect.center().y), Align2::LEFT_CENTER, label, font, color);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    }
    response
}

/// The ribbon's tool that runs `command`, if it has one.
fn known_tool(command: &str) -> Option<&'static Tool> {
    let lists: [&[Tool]; 17] = [
        LAYOUT_TOOLS,
        VIEWPORT_TOOLS,
        DRAW_LARGE,
        DRAW_SMALL,
        DRAW_MORE,
        MODIFY_MORE,
        BLOCK_SMALL,
        BLOCK_DEFINITION,
        LAYER_TOOLS,
        LAYERS_MORE,
        ANNOTATION_LARGE,
        ANNOTATION_SMALL,
        ANNOTATE_TEXT,
        ANNOTATE_DIMENSIONS,
        ANNOTATE_DIMENSIONS_MORE,
        UTILITIES_LARGE,
        UTILITIES_SMALL,
    ];
    let same = |t: &&Tool| t.command.is_some_and(|c| c.eq_ignore_ascii_case(command));
    lists.into_iter().flatten().chain(MODIFY_GRID.iter().flatten()).find(same)
}

/// What a command does, for its button's tip.
fn command_tip(command: &str) -> String {
    oxidraft_commands::COMMANDS
        .iter()
        .find(|i| i.name.eq_ignore_ascii_case(command))
        .map_or_else(|| command.to_string(), |i| format!("{} ({})", i.description, i.name))
}

fn ribbon_panel(ui: &mut Ui, title: &str, contents: impl FnOnce(&mut Ui)) {
    let _ = ribbon_panel_more(ui, title, contents);
}

/// A ribbon panel; returns its title strip, which opens the panel's
/// slide-out where it has one.
fn ribbon_panel_more(ui: &mut Ui, title: &str, contents: impl FnOnce(&mut Ui)) -> Response {
    let mut title_response = None;
    let response = ui
        .vertical(|ui| {
            let r = ui.horizontal(|ui| {
                ui.set_height(78.0);
                ui.add_space(4.0);
                contents(ui);
                ui.add_space(4.0);
            });
            let width = r.response.rect.width();
            let (title_rect, strip) = ui.allocate_exact_size(vec2(width, 18.0), Sense::click());
            let fill = if strip.hovered() { theme::ribbon_hover() } else { theme::ribbon_title() };
            ui.painter().rect_filled(title_rect, 0.0, fill);
            strip.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{title} panel")));
            title_response = Some(strip);
            let text = ui.painter().text(
                title_rect.center() - vec2(5.0, 0.0),
                Align2::CENTER_CENTER,
                title,
                FontId::proportional(11.0),
                theme::text_dim(),
            );
            // Expander arrow, as on AutoCAD's panel titles.
            let c = pos2(text.right() + 9.0, title_rect.center().y + 1.0);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![c + vec2(-3.5, -2.0), c + vec2(3.5, -2.0), c + vec2(0.0, 2.5)],
                theme::text_dim(),
                Stroke::NONE,
            ));
        })
        .response;
    let r = response.rect;
    ui.painter().line_segment(
        [pos2(r.right(), r.top() + 4.0), pos2(r.right(), r.bottom())],
        Stroke::new(1.0, theme::ribbon_title()),
    );
    ui.add_space(1.0);
    title_response.unwrap_or(response)
}

fn paint_hover(ui: &Ui, rect: Rect, response: &Response, enabled: bool) {
    if enabled && response.hovered() {
        let fill = if response.is_pointer_button_down_on() {
            theme::accent().gamma_multiply(0.45)
        } else {
            theme::ribbon_hover()
        };
        ui.painter().rect(
            rect.shrink(1.0),
            2.0,
            fill,
            Stroke::new(1.0, theme::ribbon_hover().gamma_multiply(1.3)),
            egui::StrokeKind::Inside,
        );
    }
}

fn icon_button(ui: &mut Ui, icon: Icon, size: f32, enabled: bool, tip: &str) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(size + 10.0, size + 8.0), sense);
    paint_hover(ui, rect, &response, enabled);
    let color = if enabled { theme::text() } else { theme::text_disabled() };
    icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(size, size)), icon, color, enabled);
    response.on_hover_text(tip)
}

fn ribbon_tab(ui: &mut Ui, name: &str, selected: bool) -> Response {
    let font = FontId::proportional(12.5);
    let w = ui.painter().layout_no_wrap(name.into(), font.clone(), theme::text()).size().x + 24.0;
    let (rect, response) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::ribbon());
    } else if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::strip());
    }
    let color = if selected { theme::text() } else { theme::text_dim() };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, name, font, color);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, name));
    response
}

fn layout_tab(ui: &mut Ui, name: &str, selected: bool) -> Response {
    let font = FontId::proportional(12.0);
    let w = ui.painter().layout_no_wrap(name.into(), font.clone(), theme::text()).size().x + 18.0;
    let (rect, response) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 2.0, theme::ribbon());
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 2.0, theme::ribbon_hover());
    }
    let color = if selected { theme::text() } else { theme::text_dim() };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, name, font, color);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, name));
    response
}

/// A text toggle on the status bar, blue when on. Returns true when clicked.
fn status_toggle(ui: &mut Ui, label: &str, on: &mut bool, tip: &str) -> bool {
    let font = FontId::proportional(11.5);
    let w = ui.painter().layout_no_wrap(label.into(), font.clone(), theme::text()).size().x + 14.0;
    let (rect, response) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
    let enabled = ui.is_enabled();
    if response.hovered() && enabled {
        ui.painter().rect_filled(rect, 2.0, theme::ribbon_hover());
    } else if *on {
        ui.painter().rect_filled(rect, 2.0, theme::accent().gamma_multiply(0.25));
    }
    let color = match (enabled, *on) {
        (false, _) => theme::text_disabled(),
        (true, true) => theme::accent(),
        (true, false) => theme::text_dim(),
    };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, color);
    let state = *on;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, state, label));
    let clicked = response.on_hover_text(tip).clicked();
    if clicked {
        *on = !*on;
    }
    clicked
}

fn color_swatch(ui: &mut Ui, aci: u8) {
    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    ui.painter().rect(rect, 1.0, theme::swatch(aci), Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
}

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
const LAYER_TOOLS: &[Tool] = &[
    tool(Icon::LayerIsolate, "", Some("LAYISO"), "Isolate: locks every layer but those of the selected objects"),
    tool(Icon::LayerUnisolate, "", Some("LAYUNISO"), "Unisolate: puts back the layers Isolate changed"),
    tool(Icon::LayerOff, "", Some("LAYOFF"), "Off: turns off the layer of the object you pick"),
    tool(Icon::LayerFreeze, "", Some("LAYFRZ"), "Freeze: freezes the layer of the object you pick"),
    tool(Icon::LayerLock, "", Some("LAYLCK"), "Lock: locks the layer of the object you pick"),
    tool(Icon::LayerUnlock, "", Some("LAYULK"), "Unlock: unlocks the layer of the object you pick"),
];
const ANNOTATION_LARGE: &[Tool] = &[
    tool(Icon::Text, "Text", Some("MTEXT"), "Creates multiline text (T); single-line text is DT"),
    tool(Icon::Dimension, "Dimension", Some("DIMLINEAR"), "Creates a horizontal or vertical dimension (DLI)"),
];
const ANNOTATION_SMALL: &[Tool] = &[
    tool(Icon::Dimension, "Aligned", Some("DIMALIGNED"), "Creates a dimension aligned to two points (DAL)"),
    tool(Icon::Leader, "Leader", Some("MLEADER"), "Creates a leader with text (MLD)"),
    tool(Icon::Table, "Table", None, "Table"),
];
const UTILITIES_LARGE: &[Tool] = &[tool(
    Icon::Measure,
    "Measure",
    Some("DIST"),
    "Measures the distance between two points (DI); also AREA (AA) and ID",
)];

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
            if ui.button("Plot...").clicked() {
                self.run("PLOT");
            }
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
                if self.ribbon_tab == 0 {
                    self.home_tab(ui);
                } else if self.ribbon_tab == LAYOUT_TAB {
                    self.layout_ribbon(ui);
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
            });
        });
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
            });
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
                &tool(Icon::PlotPreview, "Preview", Some("PREVIEW"), "Shows the drawing as it will plot"),
            );
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
        ribbon_panel(ui, "Layers", |ui| {
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
            ui.vertical(|ui| {
                self.small_tool(
                    ui,
                    &tool(Icon::Properties, "Properties", Some("PROPERTIES"), "Shows the Properties palette (Cmd+1)"),
                );
            });
        });
        ribbon_panel(ui, "Utilities", |ui| {
            for t in UTILITIES_LARGE {
                self.large_tool(ui, t);
            }
        });
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
        egui::Panel::top("file_tabs").frame(frame).exact_size(28.0).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let dirty = if self.is_dirty() { "*" } else { "" };
                let label = format!("{}{dirty}", self.file_name());
                let galley = ui.painter().layout_no_wrap(label, FontId::proportional(12.0), theme::text());
                let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 44.0, 25.0), Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::tab());
                ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(2.0, theme::accent()));
                ui.painter().galley(
                    pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    theme::text(),
                );
                ui.painter().text(
                    pos2(rect.right() - 14.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    "×",
                    FontId::proportional(13.0),
                    theme::text_dim(),
                );
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
                        if let Some(s) = space.filter(|_| !model) {
                            item("Delete", TabAction::Delete(s));
                            item("Rename", TabAction::Rename(s));
                            item("Copy", TabAction::Copy(s));
                            item("Activate Model Tab", TabAction::Model);
                            item("Page Setup...", TabAction::PageSetup(s));
                            item("Plot...", TabAction::Plot(s));
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
                let coords = self.cursor.map_or(String::new(), |p| format!("{:.4}, {:.4}, 0.0000", p.x, p.y));
                ui.add_sized(
                    [210.0, 20.0],
                    egui::Label::new(egui::RichText::new(coords).monospace().color(theme::text_dim())),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.update_badge(ui);
                    ui.label(egui::RichText::new("1:1").color(theme::text_dim()));
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
    let (rect, response) = ui.allocate_exact_size(vec2(58.0, 76.0), Sense::click());
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
    let font = FontId::proportional(11.5);
    let text_w = if t.label.is_empty() {
        0.0
    } else {
        ui.painter().layout_no_wrap(t.label.into(), font.clone(), theme::text()).size().x + 6.0
    };
    let (rect, response) = ui.allocate_exact_size(
        vec2(24.0 + text_w.max(if t.label.is_empty() { 0.0 } else { 46.0 }), 24.0),
        Sense::click(),
    );
    paint_hover(ui, rect, &response, enabled);
    let color = if enabled { theme::text() } else { theme::text_disabled() };
    let icon = Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(ui.painter(), icon, t.icon, color, enabled);
    if !t.label.is_empty() {
        ui.painter().text(pos2(rect.left() + 24.0, rect.center().y), Align2::LEFT_CENTER, t.label, font, color);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, t.label));
    }
    response
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

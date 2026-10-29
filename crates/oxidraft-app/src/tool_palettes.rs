//! TOOLPALETTES (Ctrl+3): palettes of tools. A tool runs a command (on a
//! layer, in a color, if it says), hatches with a pattern, or inserts a
//! block. Tools are added from the selected objects (each object's kind,
//! layer and color, its pattern, its block) or as typed command macros;
//! palettes are added, renamed and deleted. They're kept with the
//! settings.

use eframe::egui::{self, RichText};
use oxidraft_model::{Color, Geometry, HatchPattern};
use serde::{Deserialize, Serialize};

use crate::{app::OxidraftApp, theme};

/// What a tool does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolKind {
    /// A command or macro ("CIRCLE", "MVIEW L ON"), on a layer and in a
    /// color (an AutoCAD Color Index) when given.
    Command { command: String, layer: Option<String>, color: Option<u8> },
    /// HATCH with this pattern and scale.
    Hatch { pattern: String, scale: f64 },
    /// INSERT this drawing's block.
    Block { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaletteTool {
    pub label: String,
    pub kind: ToolKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolPalette {
    pub name: String,
    pub tools: Vec<PaletteTool>,
}

fn command(label: &str, command: &str) -> PaletteTool {
    PaletteTool { label: label.into(), kind: ToolKind::Command { command: command.into(), layer: None, color: None } }
}

fn hatch(pattern: &str) -> PaletteTool {
    PaletteTool { label: pattern.into(), kind: ToolKind::Hatch { pattern: pattern.into(), scale: 1.0 } }
}

/// The palettes a new user starts with.
pub fn default_palettes() -> Vec<ToolPalette> {
    vec![
        ToolPalette {
            name: "Draw".into(),
            tools: vec![
                command("Line", "LINE"),
                command("Polyline", "PLINE"),
                command("Circle", "CIRCLE"),
                command("Arc", "ARC"),
                command("Rectangle", "RECTANG"),
                command("Polygon", "POLYGON"),
                command("Ellipse", "ELLIPSE"),
                command("Spline", "SPLINE"),
                command("Revision Cloud", "REVCLOUD"),
            ],
        },
        ToolPalette {
            name: "Modify".into(),
            tools: vec![
                command("Move", "MOVE"),
                command("Copy", "COPY"),
                command("Rotate", "ROTATE"),
                command("Mirror", "MIRROR"),
                command("Scale", "SCALE"),
                command("Offset", "OFFSET"),
                command("Trim", "TRIM"),
                command("Fillet", "FILLET"),
                command("Array", "ARRAYRECT"),
            ],
        },
        ToolPalette {
            name: "Annotation".into(),
            tools: vec![
                command("Text", "MTEXT"),
                command("Dimension", "DIM"),
                command("Leader", "MLEADER"),
                command("Table", "TABLE"),
                command("Field", "FIELD"),
            ],
        },
        ToolPalette {
            name: "Hatches".into(),
            tools: ["SOLID", "ANSI31", "ANSI37", "NET", "LINE"].iter().map(|p| hatch(p)).collect(),
        },
    ]
}

/// The palettes' window: which one shows, and the macro being typed.
#[derive(Default)]
pub(crate) struct ToolPalettesWindow {
    shown: usize,
    new_command: String,
    renaming: Option<String>,
}

/// A tool made from an object: its kind's command on its layer in its
/// color, its hatch pattern, or its block.
fn tool_from(e: &oxidraft_model::Entity) -> Option<PaletteTool> {
    let color = match e.color {
        Color::Index(i) => Some(i),
        _ => None,
    };
    let layer = Some(e.layer.clone());
    let cmd = |label: &str, c: &str| PaletteTool {
        label: label.into(),
        kind: ToolKind::Command { command: c.into(), layer: layer.clone(), color },
    };
    Some(match &e.geometry {
        Geometry::Line { .. } => cmd("Line", "LINE"),
        Geometry::Polyline { .. } => cmd("Polyline", "PLINE"),
        Geometry::Circle { .. } => cmd("Circle", "CIRCLE"),
        Geometry::Arc { .. } => cmd("Arc", "ARC"),
        Geometry::Ellipse(_) => cmd("Ellipse", "ELLIPSE"),
        Geometry::Spline(_) => cmd("Spline", "SPLINE"),
        Geometry::Text { .. } => cmd("Text", "MTEXT"),
        Geometry::Point { .. } => cmd("Point", "POINT"),
        Geometry::Dimension { .. } => cmd("Linear Dimension", "DIMLINEAR"),
        Geometry::Hatch { pattern, scale, .. } if !matches!(pattern, HatchPattern::Gradient { .. }) => PaletteTool {
            label: pattern.name().into(),
            kind: ToolKind::Hatch { pattern: pattern.name().into(), scale: *scale },
        },
        Geometry::Insert { block, .. } => {
            PaletteTool { label: block.clone(), kind: ToolKind::Block { name: block.clone() } }
        }
        _ => return None,
    })
}

impl OxidraftApp {
    /// The palettes: the user's, or the defaults.
    pub fn palettes(&self) -> Vec<ToolPalette> {
        if self.settings.tool_palettes.is_empty() { default_palettes() } else { self.settings.tool_palettes.clone() }
    }

    fn set_palettes(&mut self, ctx: &egui::Context, palettes: Vec<ToolPalette>) {
        let settings = crate::settings::Settings { tool_palettes: palettes, ..self.settings.clone() };
        self.apply_settings(ctx, settings);
    }

    pub(crate) fn toggle_tool_palettes(&mut self) {
        self.tool_palettes = match self.tool_palettes.take() {
            Some(_) => None,
            None => Some(ToolPalettesWindow::default()),
        };
    }

    /// Uses a tool.
    pub fn use_tool(&mut self, tool: &PaletteTool) {
        match &tool.kind {
            ToolKind::Command { command, layer, color } => {
                if let Some(layer) = layer {
                    if self.doc.layer(layer).is_none() {
                        self.doc.add_layer(oxidraft_model::Layer::new(layer.as_str(), 7));
                    }
                    self.doc.set_current_layer(layer);
                }
                if let Some(c) = color {
                    let mut p = self.doc.current_props().clone();
                    p.color = Color::Index(*c);
                    self.doc.set_current_props(p);
                }
                self.run(command);
            }
            ToolKind::Hatch { pattern, scale } => {
                if let Some(p) = HatchPattern::from_name(pattern) {
                    self.engine.vars.hatch_pattern = p;
                }
                self.engine.vars.hatch_scale = *scale;
                self.run("HATCH");
            }
            ToolKind::Block { name } => {
                if self.doc.block(name).is_some() {
                    self.engine.insert_block(name, &mut self.doc);
                } else {
                    self.engine.log(format!("This drawing has no block \"{name}\"."));
                }
            }
        }
    }

    pub(crate) fn tool_palettes_window(&mut self, ctx: &egui::Context) {
        let Some(mut w) = self.tool_palettes.take() else { return };
        let mut palettes = self.palettes();
        w.shown = w.shown.min(palettes.len().saturating_sub(1));
        let mut open = true;
        let (mut used, mut changed, mut add_selected, mut add_command) = (None, false, false, false);
        let (mut new_palette, mut delete_palette) = (false, false);
        let mut fields = Vec::new();
        egui::Window::new("Tool Palettes")
            .id(egui::Id::unique("tool_palettes"))
            .open(&mut open)
            .collapsible(true)
            .resizable(true)
            .default_size([230.0, 420.0])
            .default_pos([40.0, 180.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                // The palettes' tabs.
                ui.horizontal_wrapped(|ui| {
                    for (i, p) in palettes.iter().enumerate() {
                        let r = ui.selectable_label(w.shown == i, &p.name);
                        if r.clicked() {
                            w.shown = i;
                        }
                        r.context_menu(|ui| {
                            if ui.button("Rename Palette").clicked() {
                                w.shown = i;
                                w.renaming = Some(p.name.clone());
                                ui.close();
                            }
                            if ui.button("Delete Palette").clicked() {
                                w.shown = i;
                                delete_palette = true;
                                ui.close();
                            }
                        });
                    }
                    new_palette = ui.small_button("+").on_hover_text("New palette").clicked();
                });
                if let Some(name) = &mut w.renaming {
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(180.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Palette name"));
                    fields.push(r.id);
                    if r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let name = name.trim().to_string();
                        if !name.is_empty()
                            && let Some(p) = palettes.get_mut(w.shown)
                        {
                            p.name = name;
                            changed = true;
                        }
                        w.renaming = None;
                    }
                }
                ui.separator();
                let mut delete_tool = None;
                if let Some(p) = palettes.get(w.shown) {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (i, tool) in p.tools.iter().enumerate() {
                            let hint = match &tool.kind {
                                ToolKind::Command { command, layer, .. } => match layer {
                                    Some(l) => format!("{command} on layer {l}"),
                                    None => command.clone(),
                                },
                                ToolKind::Hatch { pattern, scale } => format!("HATCH {pattern}, scale {scale}"),
                                ToolKind::Block { name } => format!("INSERT {name}"),
                            };
                            let r = ui.add(egui::Button::new(&tool.label).min_size(egui::vec2(190.0, 22.0)));
                            if r.clicked() {
                                used = Some(tool.clone());
                            }
                            r.on_hover_text(hint).context_menu(|ui| {
                                if ui.button("Delete Tool").clicked() {
                                    delete_tool = Some(i);
                                    ui.close();
                                }
                            });
                        }
                        if p.tools.is_empty() {
                            ui.colored_label(theme::text_dim(), "No tools: add the selected objects, or a command.");
                        }
                    });
                }
                if let (Some(i), Some(p)) = (delete_tool, palettes.get_mut(w.shown)) {
                    p.tools.remove(i);
                    changed = true;
                }
                ui.separator();
                add_selected = ui
                    .button("Add Selected Objects")
                    .on_hover_text("A tool for each selected object: its command on its layer, its pattern, its block")
                    .clicked();
                ui.horizontal(|ui| {
                    let r = ui
                        .add(egui::TextEdit::singleline(&mut w.new_command).desired_width(120.0).hint_text("command"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Tool command"));
                    fields.push(r.id);
                    add_command = ui.add_enabled(!w.new_command.trim().is_empty(), egui::Button::new("Add")).clicked();
                });
                ui.label(RichText::new("Right-click a tool or palette for more.").small().color(theme::text_dim()));
            });
        self.text_fields.extend(fields);
        if new_palette {
            palettes.push(ToolPalette { name: format!("New Palette {}", palettes.len() + 1), tools: Vec::new() });
            w.shown = palettes.len() - 1;
            changed = true;
        }
        if delete_palette && palettes.len() > 1 {
            palettes.remove(w.shown);
            w.shown = 0;
            changed = true;
        }
        if add_selected && let Some(p) = palettes.get_mut(w.shown) {
            let tools: Vec<PaletteTool> =
                self.engine.selection().iter().filter_map(|id| self.doc.entity(*id)).filter_map(tool_from).collect();
            if tools.is_empty() {
                self.engine.log("Select objects to make tools of first.".to_string());
            }
            for t in tools {
                if !p.tools.contains(&t) {
                    p.tools.push(t);
                    changed = true;
                }
            }
        }
        if add_command && let Some(p) = palettes.get_mut(w.shown) {
            let command = w.new_command.trim().to_uppercase();
            let label = command.split_whitespace().next().unwrap_or_default().to_string();
            p.tools.push(PaletteTool { label, kind: ToolKind::Command { command, layer: None, color: None } });
            w.new_command.clear();
            changed = true;
        }
        if changed {
            self.set_palettes(ctx, palettes);
        }
        if let Some(tool) = used {
            self.use_tool(&tool);
        }
        if open {
            self.tool_palettes = Some(w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_geom::Point;

    #[test]
    fn tools_from_objects() {
        let mut doc = oxidraft_model::Document::new();
        doc.add_layer(oxidraft_model::Layer::new("Walls", 1));
        let id = doc.add_on_layer(Geometry::Circle { center: Point::ORIGIN, radius: 1.0 }, "Walls", Color::Index(3));
        let t = tool_from(doc.entity(id).unwrap()).unwrap();
        assert_eq!(t.kind, ToolKind::Command { command: "CIRCLE".into(), layer: Some("Walls".into()), color: Some(3) });
        let json = serde_json::to_string(&default_palettes()).unwrap();
        let back: Vec<ToolPalette> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, default_palettes());
    }
}

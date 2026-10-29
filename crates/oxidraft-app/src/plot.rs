//! PLOT and PREVIEW: AutoCAD's Plot dialog, the full-screen plot preview,
//! and writing the plot as a PDF file.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Color32, FontId, Key, Layout, Painter, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
};
use eframe::egui::{pos2, vec2};
use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Document, MODEL, Space, hatch};
use std::sync::Arc;

use oxidraft_plot::{Item, PAPERS, Picture, PlotArea, PlotScale, PlotSettings, PlotStyle, SCALES, Sheet, scale_label};

use crate::{app::OxidraftApp, theme};

/// The Plot dialog while it's open.
pub(crate) struct PlotDialog {
    pub(crate) settings: PlotSettings,
    /// Picking a window in the drawing: the dialog steps aside meanwhile.
    pub(crate) picking: bool,
    /// Showing the full-size preview.
    pub(crate) preview: bool,
    /// Opened for PREVIEW: closing the preview closes everything.
    preview_only: bool,
    /// PAGESETUP on the Model tab: OK keeps the settings for the next
    /// plot instead of plotting.
    setup_only: bool,
    /// The sheet as last laid out, with the settings and drawing revision
    /// it was laid out for.
    sheet: Option<(PlotSettings, u64, Option<Sheet>)>,
    /// Model space or the layout plotted.
    space: Space,
}

impl PlotDialog {
    /// Lays the sheet out again when the settings or the drawing changed.
    /// Lays the sheet out again when the settings, the drawing or the
    /// loaded pictures (`revision` counts both) changed.
    fn refresh(&mut self, doc: &Document, revision: u64, pictures: &dyn Fn(&str) -> Option<Arc<Picture>>) {
        let current = matches!(&self.sheet, Some((s, rev, _)) if *s == self.settings && *rev == revision);
        if !current {
            let sheet = oxidraft_plot::render_with_pictures(doc, self.space, &self.settings, pictures);
            self.sheet = Some((self.settings, revision, sheet));
        }
    }

    fn sheet(&self) -> Option<&Sheet> {
        self.sheet.as_ref().and_then(|(_, _, s)| s.as_ref())
    }
}

/// What the preview's buttons and keys asked for.
enum PreviewAction {
    Stay,
    Close,
    Plot,
}

fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Paints a sheet on its paper, filling `rect` (which has the paper's
/// proportions): the same strokes, fills and text the PDF gets.
pub(crate) fn paint_sheet(
    painter: &Painter,
    rect: Rect,
    sheet: &Sheet,
    printable: bool,
    textures: &dyn Fn(&str) -> Option<egui::TextureId>,
) {
    let l = &sheet.layout;
    let k = rect.width() / l.width as f32;
    let to = |p: Point| pos2(rect.left() + p.x as f32 * k, rect.bottom() - p.y as f32 * k);
    painter.rect_filled(rect.translate(vec2(3.0, 3.0)), 0.0, Color32::from_black_alpha(90));
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    if printable {
        let p = l.printable;
        let edge = Rect::from_two_pos(to(p.min), to(p.max));
        painter.rect_stroke(edge, 0.0, Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Middle);
    }
    let clip = Rect::from_two_pos(to(l.clip.min), to(l.clip.max));
    let painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
    paint_items(&painter, &sheet.items, &to, k, textures);
}

/// Paints plotted items; `to` puts a point of the sheet on screen, `k`
/// screen points to the millimeter.
fn paint_items(
    painter: &Painter,
    items: &[Item],
    to: &dyn Fn(Point) -> Pos2,
    k: f32,
    textures: &dyn Fn(&str) -> Option<egui::TextureId>,
) {
    for item in items {
        match item {
            Item::Stroke { paths, width, color, .. } => {
                let stroke = Stroke::new((*width as f32 * k).max(0.6), rgb(*color));
                for path in paths {
                    let points: Vec<Pos2> = path.points.iter().map(|p| to(*p)).collect();
                    if points.len() == 2 && points[0] == points[1] {
                        painter.circle_filled(points[0], stroke.width / 2.0, stroke.color);
                    } else if path.closed {
                        painter.add(Shape::closed_line(points, stroke));
                    } else {
                        painter.add(Shape::line(points, stroke));
                    }
                }
            }
            Item::Fill { polygons, color } => {
                let mut mesh = egui::Mesh::default();
                for t in hatch::triangles(polygons) {
                    let i = mesh.vertices.len() as u32;
                    for p in t {
                        mesh.colored_vertex(to(p), rgb(*color));
                    }
                    mesh.add_triangle(i, i + 1, i + 2);
                }
                painter.add(Shape::mesh(mesh));
            }
            Item::Image { name, origin, u, v, clip, .. } => {
                let Some(texture) = textures(name) else { continue };
                let ring: Vec<Point> = if clip.len() >= 3 {
                    clip.clone()
                } else {
                    oxidraft_model::image_corners(*origin, *u, *v).to_vec()
                };
                let det = u.cross(*v);
                let uv = |p: Point| {
                    let d = p - *origin;
                    let (a, b) = if det.abs() > 0.0 { (d.cross(*v) / det, u.cross(d) / det) } else { (0.0, 0.0) };
                    pos2(a as f32, (1.0 - b) as f32)
                };
                let mut mesh = egui::Mesh::with_texture(texture);
                for t in hatch::triangles(std::slice::from_ref(&ring)) {
                    let i = mesh.vertices.len() as u32;
                    for p in t {
                        mesh.vertices.push(egui::epaint::Vertex { pos: to(p), uv: uv(p), color: Color32::WHITE });
                    }
                    mesh.add_triangle(i, i + 1, i + 2);
                }
                painter.add(Shape::mesh(mesh));
            }
            Item::Text { at, rotation, size, text, color } => {
                let px = *size as f32 * k;
                if px < 2.0 {
                    continue;
                }
                // Few distinct sizes keep egui's glyph cache small.
                let px = 2f32.powf((px.min(400.0).log2() * 8.0).round() / 8.0);
                let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(px), rgb(*color));
                let baseline = galley.rows.first().map_or(px * 0.8, |row| {
                    row.pos.y + row.row.glyphs.first().map_or(row.row.size.y * 0.8, |g| g.pos.y)
                });
                let angle = -(*rotation as f32).to_radians();
                let pos = to(*at) - egui::emath::Rot2::from_angle(angle) * vec2(0.0, baseline);
                painter.add(egui::epaint::TextShape::new(pos, galley, rgb(*color)).with_angle(angle));
            }
            Item::Clipped { clip: frame, shape, items } => {
                let clip = Rect::from_two_pos(to(frame.min), to(frame.max)).intersect(painter.clip_rect());
                let inside = painter.with_clip_rect(clip);
                paint_items(&inside, items, to, k, textures);
                // A non-rectangular viewport: the paper's white outside its boundary.
                if shape.len() >= 3 {
                    let mut mesh = egui::Mesh::default();
                    for t in hatch::triangles(&[frame.corners().to_vec(), shape.clone()]) {
                        let i = mesh.vertices.len() as u32;
                        for p in t {
                            mesh.colored_vertex(to(p), Color32::WHITE);
                        }
                        mesh.add_triangle(i, i + 1, i + 2);
                    }
                    inside.add(Shape::mesh(mesh));
                }
            }
        }
    }
}

/// The largest rect with the sheet's proportions inside `space`, centered.
fn paper_rect(space: Rect, sheet_w: f64, sheet_h: f64) -> Rect {
    let k = (space.width() / sheet_w as f32).min(space.height() / sheet_h as f32);
    Rect::from_center_size(space.center(), vec2(sheet_w as f32 * k, sheet_h as f32 * k))
}

/// A titled box of settings, like the dialog's group boxes in AutoCAD.
fn section(ui: &mut Ui, title: &str, contents: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(title).strong());
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        contents(ui);
    });
    ui.add_space(6.0);
}

impl OxidraftApp {
    /// The part of the drawing the drawing area shows (the sheet's part,
    /// working in a viewport).
    pub(crate) fn displayed(&self) -> Aabb {
        let r = self.canvas_rect;
        let view = self.active_viewport.filter(|a| !a.maximized).map_or(self.view, |a| a.page);
        Aabb::from_points(view.to_world(r, r.left_bottom()), view.to_world(r, r.right_top()))
    }

    /// PLOT (or PREVIEW, straight to the preview), with the last plot's
    /// settings; Display plots what's on screen now. On a layout's tab it
    /// plots the layout, as its page setup has it.
    pub(crate) fn open_plot(&mut self, preview: bool) {
        self.engine.cancel(&mut self.doc);
        let mut settings = self.plot_settings;
        let space = self.shown_space();
        match settings.area {
            PlotArea::Display(_) => settings.area = PlotArea::Display(self.displayed()),
            PlotArea::Layout if space == MODEL => settings.area = PlotArea::Extents,
            _ => {}
        }
        if space != MODEL {
            settings.area = PlotArea::Layout;
        }
        let setup_only = false;
        self.plot = Some(PlotDialog {
            settings,
            picking: false,
            preview,
            preview_only: preview,
            setup_only,
            sheet: None,
            space,
        });
    }

    /// The Plot dialog's settings, while it's open.
    pub fn plot_settings_mut(&mut self) -> Option<&mut PlotSettings> {
        self.plot.as_mut().map(|d| &mut d.settings)
    }

    /// Whether the Plot dialog, and its preview, show (not while a window
    /// is being picked).
    pub fn plot_open(&self) -> (bool, bool) {
        self.plot.as_ref().filter(|d| !d.picking).map_or((false, false), |d| (!d.preview, d.preview))
    }

    /// Plots with the dialog's settings (or the last plot's) to a PDF at
    /// `path`, and closes the dialog.
    pub fn plot_to(&mut self, path: &Path) -> Result<(), String> {
        let (settings, space) =
            self.plot.as_ref().map_or((self.plot_settings, self.shown_space()), |d| (d.settings, d.space));
        let sheet = {
            let pictures = self.plot_pictures(true);
            oxidraft_plot::render_with_pictures(&self.doc, space, &settings, &pictures)
        };
        let sheet = sheet.ok_or("There's nothing to plot.")?;
        let bytes = oxidraft_plot::pdf(&sheet, &self.file_name());
        std::fs::write(path, bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        let area = sheet.layout.clip;
        self.engine.log(format!("Effective plotting area: {:.2} wide by {:.2} high", area.width(), area.height()));
        self.engine.log(format!("Plot written to {}", path.display()));
        self.plot_settings = settings;
        self.plot = None;
        Ok(())
    }

    /// OK: asks where to save the PDF, then plots.
    fn plot_with_dialog(&mut self) {
        let name = Path::new(&self.file_name()).with_extension("pdf");
        let mut dialog = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(name.to_string_lossy());
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(path): Option<PathBuf> = dialog.save_file() else { return };
        if let Err(e) = self.plot_to(&path) {
            self.engine.log(e);
        }
    }

    /// Esc closes the dialog or the preview, Enter the preview; nothing
    /// else reaches the drawing meanwhile. Returns true while they're open.
    pub(crate) fn plot_keys(&mut self, ctx: &egui::Context) -> bool {
        let Some(dialog) = &mut self.plot else { return false };
        if dialog.picking {
            return false;
        }
        let (esc, enter) = ctx.input_mut(|i| {
            (i.consume_key(egui::Modifiers::NONE, Key::Escape), i.consume_key(egui::Modifiers::NONE, Key::Enter))
        });
        if dialog.preview && (esc || enter) {
            dialog.preview = false;
            if dialog.preview_only {
                self.plot = None;
            }
        } else if esc {
            self.plot = None;
        }
        true
    }

    pub(crate) fn plot_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.plot.take() else { return };
        // Picking the window: wait, unless the pick was cancelled.
        if dialog.picking {
            dialog.picking = self.engine.is_active();
            if dialog.picking {
                self.plot = Some(dialog);
                return;
            }
        }
        let revision = self.doc.revision() + self.raster_revision;
        dialog.refresh(&self.doc, revision, &self.plot_pictures(false));
        if dialog.preview {
            match self.plot_preview(ctx, &dialog) {
                PreviewAction::Stay => {}
                PreviewAction::Close if dialog.preview_only => return,
                PreviewAction::Close => dialog.preview = false,
                PreviewAction::Plot => {
                    self.plot = Some(dialog);
                    self.plot_with_dialog();
                    return;
                }
            }
            self.plot = Some(dialog);
            return;
        }
        let (mut open, mut ok, mut cancel, mut preview, mut pick) = (true, false, false, false, false);
        let before = dialog.settings;
        let displayed = self.displayed();
        let (space, layout) = (dialog.space, dialog.settings.area == PlotArea::Layout);
        let title = self.doc.layout(space).map_or("Model".into(), |l| l.name.clone());
        let paper_name = dialog.settings.paper_name(&self.doc, space);
        egui::Window::new(format!("{} - {title}", if dialog.setup_only { "Page Setup" } else { "Plot" }))
            .id(egui::Id::unique("plot_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([260.0, 130.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                let s = &mut dialog.settings;
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(340.0);
                        section(ui, "Printer/plotter", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Name:");
                                egui::ComboBox::from_id_salt("plot_printer")
                                    .selected_text("Oxidraft PDF")
                                    .width(ui.available_width())
                                    .show_ui(ui, |ui| {
                                        let _ = ui.selectable_label(true, "Oxidraft PDF");
                                    });
                            });
                            ui.colored_label(
                                theme::text_dim(),
                                "Writes a PDF file: lines as vectors, text you can search.",
                            );
                        });
                        // A layout plots on its page setup's paper.
                        section(ui, "Paper size", |ui| {
                            ui.add_enabled_ui(!layout, |ui| {
                                egui::ComboBox::from_id_salt("plot_paper")
                                    .selected_text(&paper_name)
                                    .width(ui.available_width())
                                    .show_ui(ui, |ui| {
                                        for p in PAPERS {
                                            ui.selectable_value(&mut s.paper, *p, p.name);
                                        }
                                    });
                            });
                        });
                        section(ui, "Plot area", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("What to plot:");
                                let label = match s.area {
                                    PlotArea::Display(_) => "Display",
                                    PlotArea::Extents => "Extents",
                                    PlotArea::Window(_) => "Window",
                                    PlotArea::Layout => "Layout",
                                };
                                let area = egui::ComboBox::from_id_salt("plot_area").width(ui.available_width());
                                area.selected_text(label).show_ui(ui, |ui| {
                                    if space != MODEL && ui.selectable_label(label == "Layout", "Layout").clicked() {
                                        s.area = PlotArea::Layout;
                                    }
                                    if ui.selectable_label(label == "Display", "Display").clicked() {
                                        s.area = PlotArea::Display(displayed);
                                    }
                                    if ui.selectable_label(label == "Extents", "Extents").clicked() {
                                        s.area = PlotArea::Extents;
                                    }
                                    // Choosing Window goes straight to picking it, as in AutoCAD.
                                    if ui.selectable_label(label == "Window", "Window").clicked() {
                                        pick = true;
                                    }
                                });
                                if matches!(s.area, PlotArea::Window(_)) {
                                    pick |= ui.button("Window<").clicked();
                                }
                            });
                        });
                        section(ui, "Plot offset (origin set to printable area)", |ui| {
                            ui.add_enabled_ui(!layout, |ui| {
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(!s.center, |ui| {
                                        ui.label("X:");
                                        ui.add(egui::DragValue::new(&mut s.offset.x).speed(0.5).suffix(" mm"));
                                        ui.label("Y:");
                                        ui.add(egui::DragValue::new(&mut s.offset.y).speed(0.5).suffix(" mm"));
                                    });
                                    ui.checkbox(&mut s.center, "Center the plot");
                                })
                            });
                        });
                        section(ui, "Plot scale", |ui| {
                            if layout {
                                ui.colored_label(theme::text_dim(), "1:1, as the layout's page setup has it.");
                                return;
                            }
                            let mut fit = s.scale == PlotScale::Fit;
                            if ui.checkbox(&mut fit, "Fit to paper").changed() {
                                s.scale = if fit {
                                    PlotScale::Fit
                                } else {
                                    let mm_per_unit = dialog
                                        .sheet
                                        .as_ref()
                                        .and_then(|(_, _, sh)| sh.as_ref())
                                        .map_or(1.0, |sh| sh.layout.scale);
                                    PlotScale::Custom { mm: 1.0, units: (1.0 / mm_per_unit * 1e4).round() / 1e4 }
                                };
                            }
                            let (mut mm, mut units) = match s.scale {
                                PlotScale::Custom { mm, units } => (mm, units),
                                PlotScale::Fit => {
                                    let k = dialog
                                        .sheet
                                        .as_ref()
                                        .and_then(|(_, _, sh)| sh.as_ref())
                                        .map_or(1.0, |sh| sh.layout.scale);
                                    (1.0, 1.0 / k)
                                }
                            };
                            ui.add_enabled_ui(!fit, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Scale:");
                                    let current = if fit { "Custom".to_string() } else { scale_label(mm, units) };
                                    egui::ComboBox::from_id_salt("plot_scale")
                                        .selected_text(&current)
                                        .width(ui.available_width())
                                        .show_ui(ui, |ui| {
                                            for &(m, u) in SCALES {
                                                if ui
                                                    .selectable_label(current == scale_label(m, u), scale_label(m, u))
                                                    .clicked()
                                                {
                                                    (mm, units) = (m, u);
                                                }
                                            }
                                        });
                                });
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut mm)
                                            .speed(0.1)
                                            .range(0.0001..=f64::MAX)
                                            .max_decimals(4),
                                    );
                                    ui.label("mm =");
                                    ui.add(
                                        egui::DragValue::new(&mut units)
                                            .speed(0.1)
                                            .range(0.0001..=f64::MAX)
                                            .max_decimals(4),
                                    );
                                    ui.label("units");
                                });
                            });
                            if !fit {
                                s.scale = PlotScale::Custom { mm, units };
                            }
                        });
                    });
                    // A vertical separator here would stretch the row to the
                    // window's full height.
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.set_width(250.0);
                        section(ui, "Plot style table (pen assignments)", |ui| {
                            egui::ComboBox::from_id_salt("plot_style")
                                .selected_text(s.style.name())
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for style in PlotStyle::ALL {
                                        ui.selectable_value(&mut s.style, style, style.name());
                                    }
                                });
                        });
                        section(ui, "Plot options", |ui| {
                            ui.checkbox(&mut s.lineweights, "Plot object lineweights");
                        });
                        section(ui, "Drawing orientation", |ui| {
                            ui.add_enabled_ui(!layout, |ui| {
                                ui.radio_value(&mut s.landscape, false, "Portrait");
                                ui.radio_value(&mut s.landscape, true, "Landscape");
                            });
                        });
                        // A small preview of the sheet, as in AutoCAD's dialog.
                        let laid_out = dialog.sheet.as_ref().and_then(|(_, _, sh)| sh.as_ref());
                        let (w, h) = match laid_out {
                            Some(sheet) if layout => (sheet.layout.width, sheet.layout.height),
                            _ => s.sheet(),
                        };
                        let (space, _) = ui.allocate_exact_size(vec2(250.0, 170.0), Sense::hover());
                        ui.painter().rect_filled(space, 4.0, theme::input_bg());
                        let paper = paper_rect(space.shrink(10.0), w, h);
                        match dialog.sheet.as_ref().and_then(|(_, _, sh)| sh.as_ref()) {
                            Some(sheet) if sheet.layout.width == w => {
                                paint_sheet(ui.painter(), paper, sheet, true, &|n| self.picture_texture(n));
                            }
                            _ => {
                                ui.painter().rect_filled(paper, 0.0, Color32::WHITE);
                            }
                        }
                        let note = match dialog.sheet.as_ref().and_then(|(_, _, sh)| sh.as_ref()) {
                            Some(sheet) => {
                                let c = sheet.layout.clip;
                                format!("Plotted area: {:.1} x {:.1} mm", c.width(), c.height())
                            }
                            None => "There's nothing to plot.".into(),
                        };
                        ui.colored_label(theme::text_dim(), note);
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    preview = ui.button("Preview...").clicked();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        cancel = ui.button("Cancel").clicked();
                        ok = ui.add_enabled(dialog.sheet().is_some(), egui::Button::new("OK")).clicked();
                    });
                });
            });
        if dialog.settings != before {
            ctx.request_repaint();
        }
        if !open || cancel {
            return;
        }
        if pick {
            dialog.picking = true;
            self.engine.pick_plot_window(&mut self.doc);
        }
        dialog.preview |= preview && dialog.sheet().is_some();
        if ok && dialog.setup_only {
            self.plot_settings = dialog.settings;
            self.engine.log("Page setup kept for the model's plots.");
            return;
        }
        self.plot = Some(dialog);
        if ok {
            self.plot_with_dialog();
        }
    }

    /// PAGESETUP on the Model tab: the Plot dialog's settings, kept for
    /// plotting model space.
    pub(crate) fn open_model_page_setup(&mut self) {
        self.open_plot(false);
        if let Some(d) = &mut self.plot {
            d.setup_only = true;
        }
    }

    /// The preview fills the window: the sheet on its paper, with Plot and
    /// Close buttons.
    fn plot_preview(&mut self, ctx: &egui::Context, dialog: &PlotDialog) -> PreviewAction {
        let screen = ctx.content_rect();
        let mut action = PreviewAction::Stay;
        egui::Area::new(egui::Id::unique("plot_preview")).order(egui::Order::Foreground).fixed_pos(screen.min).show(
            ctx,
            |ui| {
                let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
                ui.painter().rect_filled(rect, 0.0, Color32::from_gray(96));
                let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 36.0));
                ui.painter().rect_filled(bar, 0.0, theme::top_bar());
                let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 6.0))));
                bar_ui.horizontal_centered(|ui| {
                    if ui.button("Plot").clicked() {
                        action = PreviewAction::Plot;
                    }
                    if ui.button("Close Preview").clicked() {
                        action = PreviewAction::Close;
                    }
                    ui.colored_label(theme::text_dim(), "Press Esc or Enter to exit.");
                });
                if let Some(sheet) = dialog.sheet() {
                    let space = Rect::from_min_max(pos2(rect.left(), bar.bottom()), rect.max).shrink(24.0);
                    let paper = paper_rect(space, sheet.layout.width, sheet.layout.height);
                    paint_sheet(ui.painter(), paper, sheet, false, &|n| self.picture_texture(n));
                }
            },
        );
        action
    }
}

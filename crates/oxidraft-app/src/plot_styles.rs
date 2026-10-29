//! Plot style tables: the .ctb files in the Plot Styles folder (beside
//! the settings), which PLOT can plot with; STYLESMANAGER lists them to
//! add, import, edit or delete, and the Plot Style Table Editor sets each
//! color's plotted color, grayscale, screening and lineweight.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Color32, Layout, RichText, vec2};
use oxidraft_model::{Lineweight, aci_rgb};
use oxidraft_plot::ctb::{PenTable, set_tables, tables};

use crate::{app::OxidraftApp, theme};

/// The tables in `folder`, by name.
pub(crate) fn read_folder(folder: &Path) -> Vec<PenTable> {
    let mut out: Vec<PenTable> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ctb")))
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            PenTable::read(&name, &std::fs::read(&p).ok()?).ok()
        })
        .collect();
    out.sort_by_key(|t| t.name.to_ascii_lowercase());
    out
}

pub(crate) struct StylesManager {
    picked: Option<String>,
    new_name: String,
    error: Option<String>,
}

pub(crate) struct PenTableEditor {
    table: PenTable,
    /// The color being set (1–255).
    color: u8,
    /// Just opened: brought in front of the manager.
    raise: bool,
}

impl OxidraftApp {
    /// The Plot Styles folder: beside the settings.
    pub(crate) fn plot_styles_folder(&self) -> Option<PathBuf> {
        self.plot_styles_dir.clone().or_else(|| self.settings_path.as_ref()?.parent().map(|d| d.join("Plot Styles")))
    }

    /// Keeps plot style tables in `folder` (as tests do).
    pub fn keep_plot_styles_in(&mut self, folder: PathBuf) {
        self.plot_styles_dir = Some(folder);
        self.load_plot_styles();
    }

    /// Reads the folder's tables for PLOT, keeping the one chosen if it's
    /// still there.
    pub(crate) fn load_plot_styles(&mut self) {
        let chosen = self.plot_settings.style.name();
        let found = self.plot_styles_folder().map(|f| read_folder(&f)).unwrap_or_default();
        set_tables(found);
        if matches!(self.plot_settings.style, oxidraft_plot::PlotStyle::Table(_)) {
            self.plot_settings.style = oxidraft_plot::PlotStyle::all()
                .into_iter()
                .find(|s| s.name() == chosen)
                .unwrap_or(oxidraft_plot::PlotStyle::None);
        }
    }

    pub(crate) fn open_styles_manager(&mut self) {
        self.load_plot_styles();
        self.styles_manager = Some(StylesManager { picked: None, new_name: String::new(), error: None });
    }

    fn save_table(&mut self, table: &PenTable) -> Result<(), String> {
        let folder = self.plot_styles_folder().ok_or("There's no Plot Styles folder.")?;
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        std::fs::write(folder.join(&table.name), table.write()).map_err(|e| e.to_string())?;
        self.load_plot_styles();
        Ok(())
    }

    pub(crate) fn styles_manager(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.styles_manager.take() else { return };
        let (mut open, mut close, mut add, mut edit, mut import, mut delete, mut reveal) =
            (true, false, false, false, false, false, false);
        let mut fields = Vec::new();
        let found = tables();
        egui::Window::new("Plot Style Manager")
            .id(egui::Id::unique("styles_manager"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.colored_label(
                    theme::text_dim(),
                    "Color-dependent plot style tables (.ctb), for PLOT's Plot style table.",
                );
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_size(vec2(320.0, 160.0));
                    if found.is_empty() {
                        ui.colored_label(theme::text_dim(), "None yet: add or import one.");
                    }
                    for t in &found {
                        let picked = d.picked.as_deref() == Some(t.name.as_str());
                        let label = if t.description.is_empty() {
                            t.name.clone()
                        } else {
                            format!("{}  —  {}", t.name, t.description)
                        };
                        let r = ui.selectable_label(picked, label);
                        if r.clicked() {
                            d.picked = Some(t.name.clone());
                        }
                        if r.double_clicked() {
                            (d.picked, edit) = (Some(t.name.clone()), true);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("New table:");
                    let r =
                        ui.add(egui::TextEdit::singleline(&mut d.new_name).desired_width(140.0).hint_text("name.ctb"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "New table name"));
                    fields.push(r.id);
                    add = ui.add_enabled(!d.new_name.trim().is_empty(), egui::Button::new("Add")).clicked();
                });
                ui.horizontal(|ui| {
                    let picked = d.picked.is_some();
                    edit |= ui.add_enabled(picked, egui::Button::new("Edit...")).clicked();
                    delete = ui.add_enabled(picked, egui::Button::new("Delete")).clicked();
                    import = ui.button("Import...").clicked();
                    reveal = ui.button("Open Folder").clicked();
                });
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
        self.text_fields.extend(fields);
        if add {
            let mut name = d.new_name.trim().to_string();
            if !name.to_ascii_lowercase().ends_with(".ctb") {
                name.push_str(".ctb");
            }
            if found.iter().any(|t| t.name.eq_ignore_ascii_case(&name)) {
                d.error = Some(format!("{name} already exists."));
            } else {
                match self.save_table(&PenTable::new(&name)) {
                    Ok(()) => {
                        d.new_name.clear();
                        d.error = None;
                        self.pen_table_editor =
                            Some(PenTableEditor { table: PenTable::new(&name), color: 1, raise: true });
                        d.picked = Some(name);
                    }
                    Err(e) => d.error = Some(e),
                }
            }
        }
        if edit && let Some(t) = d.picked.as_ref().and_then(|n| found.iter().find(|t| &t.name == n)) {
            self.pen_table_editor = Some(PenTableEditor { table: t.clone(), color: 1, raise: true });
        }
        if delete && let (Some(name), Some(folder)) = (d.picked.take(), self.plot_styles_folder()) {
            if let Err(e) = std::fs::remove_file(folder.join(&name)) {
                d.error = Some(e.to_string());
            }
            self.load_plot_styles();
        }
        if import && let Some(file) = rfd::FileDialog::new().add_filter("Plot style table", &["ctb"]).pick_file() {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match std::fs::read(&file).map_err(|e| e.to_string()).and_then(|b| PenTable::read(&name, &b)) {
                Ok(t) => match self.save_table(&t) {
                    Ok(()) => d.picked = Some(name),
                    Err(e) => d.error = Some(e),
                },
                Err(e) => d.error = Some(format!("Can't read {name}: {e}")),
            }
        }
        if reveal && let Some(folder) = self.plot_styles_folder() {
            let _ = std::fs::create_dir_all(&folder);
            ctx.open_url(egui::OpenUrl::new_tab(format!("file://{}", folder.display())));
        }
        if open && !close {
            self.styles_manager = Some(d);
        }
    }

    pub(crate) fn pen_table_editor(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.pen_table_editor.take() else { return };
        let (mut open, mut save, mut cancel, mut same_for_all) = (true, false, false, false);
        let mut fields = Vec::new();
        let shown = egui::Window::new(format!("Plot Style Table Editor - {}", ed.table.name))
            .id(egui::Id::unique("table_editor"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 120.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Description:");
                    let r = ui.add(egui::TextEdit::singleline(&mut ed.table.description).desired_width(280.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Description"));
                    fields.push(r.id);
                });
                ui.horizontal_top(|ui| {
                    // The colors, each with its swatch.
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(130.0);
                            ui.label(RichText::new("Plot styles").strong());
                            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                for aci in 1..=255u8 {
                                    ui.horizontal(|ui| {
                                        let (r, g, b) = aci_rgb(aci);
                                        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
                                        ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
                                        if ui.selectable_label(ed.color == aci, format!("Color {aci}")).clicked() {
                                            ed.color = aci;
                                        }
                                    });
                                }
                            });
                        });
                    });
                    let pen = &mut ed.table.pens[usize::from(ed.color) - 1];
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("Properties of Color {}", ed.color)).strong());
                        let mut own = pen.color.is_none();
                        if ui.checkbox(&mut own, "Use object color").changed() {
                            pen.color = if own { None } else { Some([0, 0, 0]) };
                        }
                        if let Some(c) = &mut pen.color {
                            ui.horizontal(|ui| {
                                ui.label("Color:");
                                ui.color_edit_button_srgb(c);
                                if ui.small_button("Black").clicked() {
                                    *c = [0, 0, 0];
                                }
                                if ui.small_button("This color").clicked() {
                                    let (r, g, b) = aci_rgb(ed.color);
                                    *c = [r, g, b];
                                }
                            });
                        }
                        ui.checkbox(&mut pen.grayscale, "Grayscale");
                        ui.horizontal(|ui| {
                            ui.label("Screening:");
                            ui.add(egui::DragValue::new(&mut pen.screen).range(0..=100).suffix(" %"));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Lineweight:");
                            let shown =
                                pen.lineweight.map_or("Use object lineweight".to_string(), |w| format!("{w:.2} mm"));
                            egui::ComboBox::from_id_salt("pen_lineweight").width(170.0).selected_text(shown).show_ui(
                                ui,
                                |ui| {
                                    ui.selectable_value(&mut pen.lineweight, None, "Use object lineweight");
                                    for h in Lineweight::STANDARD.iter().filter(|h| **h > 0) {
                                        let mm = f64::from(*h) / 100.0;
                                        ui.selectable_value(&mut pen.lineweight, Some(mm), format!("{mm:.2} mm"));
                                    }
                                },
                            );
                        });
                        same_for_all = ui
                            .button("Same for all colors")
                            .on_hover_text("Gives every color these properties")
                            .clicked();
                    });
                });
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    save = ui.button("Save & Close").clicked();
                });
            });
        if let Some(shown) = shown.filter(|_| ed.raise) {
            ctx.move_to_top(shown.response.layer_id);
            ed.raise = false;
        }
        self.text_fields.extend(fields);
        if same_for_all {
            let pen = ed.table.pen(ed.color);
            ed.table.pens.iter_mut().for_each(|p| *p = pen);
        }
        if save {
            if let Err(e) = self.save_table(&ed.table) {
                self.engine.log(format!("Could not save {}: {e}", ed.table.name));
            }
            return;
        }
        if open && !cancel {
            self.pen_table_editor = Some(ed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_plot::ctb::Pen;

    #[test]
    fn reads_only_ctb_files() {
        let dir = std::env::temp_dir().join(format!("oxidraft-ctb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut t = PenTable::new("b.ctb");
        t.pens[0] = Pen { lineweight: Some(0.7), ..Pen::default() };
        std::fs::write(dir.join("b.ctb"), t.write()).unwrap();
        std::fs::write(dir.join("a.ctb"), PenTable::new("a.ctb").write()).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join("broken.ctb"), "x").unwrap();
        let names: Vec<String> = read_folder(&dir).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["a.ctb", "b.ctb"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}

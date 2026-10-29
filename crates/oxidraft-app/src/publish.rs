//! EXPORTPDF and PUBLISH: the drawing's sheets in one PDF, a page each.
//! EXPORTPDF exports the current layout (or what PLOT would plot on the
//! Model tab) or every layout; PUBLISH (Batch Plot) plots a list of sheets,
//! from this drawing and others added to it.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_model::{Document, MODEL, Space};
use oxidraft_plot::{PlotArea, PlotSettings, Sheet};

use crate::{app::OxidraftApp, theme};

/// EXPORTPDF's dialog: which layouts.
pub(crate) struct ExportPdf {
    all: bool,
}

/// A sheet in PUBLISH's list: model space or a layout of this drawing
/// (`drawing` 0) or of one added (`drawing` i, the i-th added).
#[derive(Clone)]
struct SheetEntry {
    drawing: usize,
    space: Space,
    name: String,
    include: bool,
}

pub(crate) struct PublishDialog {
    sheets: Vec<SheetEntry>,
    /// Drawings sheets were added from: file name and contents.
    others: Vec<(String, Document)>,
    selected: Option<usize>,
}

/// A drawing's sheets: its Model tab, then its layouts in tab order.
fn sheets_of(doc: &Document, drawing: usize, file: &str) -> Vec<SheetEntry> {
    let prefix = |name: &str| if drawing == 0 { name.to_string() } else { format!("{file}-{name}") };
    let model = SheetEntry { drawing, space: MODEL, name: prefix("Model"), include: false };
    std::iter::once(model)
        .chain(doc.layouts().iter().map(|l| SheetEntry { drawing, space: l.id, name: prefix(&l.name), include: true }))
        .collect()
}

impl OxidraftApp {
    /// What PLOT would use for `space`: a layout's own page setup, or the
    /// last plot's settings for model space; Display is what's on screen
    /// in this drawing (`here`), the extents in another.
    fn sheet_settings(&self, space: Space, here: bool) -> PlotSettings {
        let mut settings = self.plot_settings;
        settings.area = match settings.area {
            _ if space != MODEL => PlotArea::Layout,
            PlotArea::Display(_) if here => PlotArea::Display(self.displayed()),
            PlotArea::Layout | PlotArea::Display(_) => PlotArea::Extents,
            area => area,
        };
        settings
    }

    fn write_pdf(&mut self, path: &Path, sheets: &[Sheet]) -> Result<usize, String> {
        if sheets.is_empty() {
            return Err("There's nothing to plot.".into());
        }
        let bytes = oxidraft_plot::pdf_pages(sheets, &self.file_name());
        std::fs::write(path, bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        self.engine.log(format!("{} sheet(s) written to {}", sheets.len(), path.display()));
        Ok(sheets.len())
    }

    /// EXPORTPDF to `path`: the layout shown (on the Model tab, what PLOT
    /// would plot), or every layout. Returns how many pages.
    pub fn export_pdf_to(&mut self, path: &Path, all_layouts: bool) -> Result<usize, String> {
        let spaces: Vec<Space> =
            if all_layouts { self.doc.layouts().iter().map(|l| l.id).collect() } else { vec![self.shown_space()] };
        let sheets: Vec<Sheet> = {
            let pictures = self.plot_pictures(true);
            spaces
                .iter()
                .filter_map(|&s| {
                    oxidraft_plot::render_with_pictures(&self.doc, s, &self.sheet_settings(s, true), &pictures)
                })
                .collect()
        };
        self.write_pdf(path, &sheets)
    }

    /// PUBLISH to `path`: the sheets ticked in its list. Returns how many
    /// pages.
    pub fn publish_to(&mut self, path: &Path) -> Result<usize, String> {
        let Some(d) = &self.publish else { return Err("Publish isn't open.".into()) };
        let sheets: Vec<Sheet> = {
            let pictures = self.plot_pictures(true);
            d.sheets
                .iter()
                .filter(|s| s.include)
                .filter_map(|s| {
                    let settings = self.sheet_settings(s.space, s.drawing == 0);
                    match s.drawing {
                        0 => oxidraft_plot::render_with_pictures(&self.doc, s.space, &settings, &pictures),
                        i => oxidraft_plot::render(&d.others.get(i - 1)?.1, s.space, &settings),
                    }
                })
                .collect()
        };
        self.write_pdf(path, &sheets)
    }

    /// Asks where the PDF goes, named after the drawing.
    fn pdf_path(&self) -> Option<PathBuf> {
        let name = Path::new(&self.file_name()).with_extension("pdf");
        let mut dialog = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(name.to_string_lossy());
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.save_file()
    }

    pub(crate) fn open_export_pdf(&mut self) {
        self.export_pdf = Some(ExportPdf { all: false });
    }

    pub(crate) fn open_publish(&mut self) {
        let sheets = sheets_of(&self.doc, 0, "");
        self.publish = Some(PublishDialog { sheets, others: Vec::new(), selected: None });
    }

    /// Adds sheets from drawings picked to PUBLISH's list.
    pub fn publish_add_drawing(&mut self, path: &Path) -> Result<(), String> {
        let doc = crate::xref::read_drawing(path)?;
        let file = path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let d = self.publish.as_mut().ok_or("Publish isn't open.")?;
        d.others.push((file.clone(), doc));
        let i = d.others.len();
        d.sheets.extend(sheets_of(&d.others[i - 1].1, i, &file));
        Ok(())
    }

    pub(crate) fn export_pdf_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.export_pdf.take() else { return };
        let (mut open, mut save, mut cancel) = (true, false, false);
        let model = self.shown_space() == MODEL;
        egui::Window::new("Save As PDF")
            .id(egui::Id::unique("export_pdf"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Export").strong());
                let current = if model { "What PLOT plots (Model)" } else { "Current Layout" };
                ui.radio_value(&mut d.all, false, current);
                ui.radio_value(&mut d.all, true, format!("All Layouts ({})", self.doc.layouts().len()));
                ui.colored_label(theme::text_dim(), "Page setup: each layout's own; Model uses the last plot's.");
                let row = vec2(ui.available_width().max(260.0), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    save = ui.button("Save").clicked();
                });
            });
        if save {
            if let Some(path) = self.pdf_path()
                && let Err(e) = self.export_pdf_to(&path, d.all)
            {
                self.engine.log(e);
            }
            return;
        }
        if open && !cancel {
            self.export_pdf = Some(d);
        }
    }

    pub(crate) fn publish_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.publish.take() else { return };
        let (mut open, mut publish, mut add, mut cancel) = (true, false, false, false);
        egui::Window::new("Batch Plot")
            .id(egui::Id::unique("publish"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 140.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Sheets to publish").strong());
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(340.0);
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for (i, s) in d.sheets.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut s.include, "").widget_info(|| {
                                    egui::WidgetInfo::selected(egui::Role::CheckBox, true, s.include, &s.name)
                                });
                                if ui.selectable_label(d.selected == Some(i), &s.name).clicked() {
                                    d.selected = Some(i);
                                }
                            });
                        }
                    });
                });
                ui.horizontal(|ui| {
                    add = ui.button("Add Sheets…").on_hover_text("Adds the sheets of another drawing").clicked();
                    let selected = d.selected.filter(|&i| i < d.sheets.len());
                    if ui.add_enabled(selected.is_some(), egui::Button::new("Remove")).clicked()
                        && let Some(i) = selected
                    {
                        d.sheets.remove(i);
                        d.selected = None;
                    }
                    if ui.add_enabled(selected.is_some_and(|i| i > 0), egui::Button::new("Move Up")).clicked()
                        && let Some(i) = selected
                    {
                        d.sheets.swap(i, i - 1);
                        d.selected = Some(i - 1);
                    }
                    let last = d.sheets.len().saturating_sub(1);
                    if ui.add_enabled(selected.is_some_and(|i| i < last), egui::Button::new("Move Down")).clicked()
                        && let Some(i) = selected
                    {
                        d.sheets.swap(i, i + 1);
                        d.selected = Some(i + 1);
                    }
                });
                let count = d.sheets.iter().filter(|s| s.include).count();
                ui.colored_label(theme::text_dim(), format!("{count} sheet(s) to one PDF, each with its page setup."));
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    publish = ui.add_enabled(count > 0, egui::Button::new("Publish")).clicked();
                });
            });
        let keep = open && !cancel;
        self.publish = keep.then_some(d);
        if add {
            let picked = rfd::FileDialog::new().add_filter("Drawing", &["dwg", "dxf"]).pick_files().unwrap_or_default();
            for path in picked {
                if let Err(e) = self.publish_add_drawing(&path) {
                    self.engine.log(e);
                }
            }
        }
        if publish && let Some(path) = self.pdf_path() {
            match self.publish_to(&path) {
                Ok(_) => self.publish = None,
                Err(e) => self.engine.log(e),
            }
        }
    }
}

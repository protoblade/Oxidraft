//! TABLE's Insert Table dialog (columns, rows and their sizes), and the
//! table editor, where a table's cells are filled in and rows and columns
//! added or taken away.

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_geom::Point;
use oxidraft_model::{EntityId, Geometry, table};

use crate::{app::OxidraftApp, theme};

/// The Insert Table dialog's settings, kept for the next table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InsertTable {
    pub columns: usize,
    pub column_width: f64,
    pub data_rows: usize,
    /// Rows this many lines of text tall.
    pub row_lines: usize,
    pub text_height: f64,
}

impl Default for InsertTable {
    fn default() -> Self {
        Self { columns: 5, column_width: 63.5, data_rows: 1, row_lines: 1, text_height: 4.5 }
    }
}

impl InsertTable {
    /// A row's height: its lines of text and a margin above and below.
    fn row_height(&self) -> f64 {
        self.text_height * (1.6 * self.row_lines as f64 + 0.8)
    }

    pub fn table(&self) -> Geometry {
        let (columns, rows) = (self.columns, self.data_rows);
        table::new_table(Point::ORIGIN, columns, rows, self.column_width, self.row_height(), self.text_height)
    }
}

/// A table being edited: its cells as typed so far, which apply on OK.
pub(crate) struct TableEditor {
    id: EntityId,
    cells: Vec<Vec<String>>,
    widths: Vec<f64>,
    heights: Vec<f64>,
}

impl OxidraftApp {
    /// TABLE: the Insert Table dialog.
    pub(crate) fn open_insert_table(&mut self) {
        self.engine.cancel(&mut self.doc);
        self.insert_table = Some(self.table_settings);
    }

    pub(crate) fn insert_table_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.insert_table.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Insert Table")
            .id(egui::Id::unique("insert_table"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 200.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Column & row settings").strong());
                egui::Grid::new("insert_table_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Columns:");
                    fields.push(ui.add(egui::DragValue::new(&mut d.columns).range(1..=100)).id);
                    ui.end_row();
                    ui.label("Column width:");
                    fields.push(
                        ui.add(
                            egui::DragValue::new(&mut d.column_width).range(0.01..=f64::MAX).speed(0.5).max_decimals(4),
                        )
                        .id,
                    );
                    ui.end_row();
                    ui.label("Data rows:");
                    fields.push(ui.add(egui::DragValue::new(&mut d.data_rows).range(1..=1000)).id);
                    ui.end_row();
                    ui.label("Row height (lines):");
                    fields.push(ui.add(egui::DragValue::new(&mut d.row_lines).range(1..=20)).id);
                    ui.end_row();
                    ui.label("Text height:");
                    fields.push(
                        ui.add(
                            egui::DragValue::new(&mut d.text_height).range(0.01..=f64::MAX).speed(0.1).max_decimals(4),
                        )
                        .id,
                    );
                    ui.end_row();
                });
                ui.colored_label(
                    theme::text_dim(),
                    format!("A title row, a header row and {} data row(s); you fill them in next.", d.data_rows),
                );
                ui.separator();
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        // The command line leaves the keyboard to them.
        self.text_fields.extend(fields);
        if ok {
            self.table_settings = d;
            self.engine.insert_table(d.table(), &mut self.doc);
        } else if open && !cancel {
            self.insert_table = Some(d);
        }
    }

    /// The table editor for table `id`.
    pub(crate) fn open_table_editor(&mut self, id: EntityId) {
        let Some(Geometry::Table { cells, widths, heights, .. }) = self.doc.entity(id).map(|e| &e.geometry) else {
            return;
        };
        self.table_editor =
            Some(TableEditor { id, cells: cells.clone(), widths: widths.clone(), heights: heights.clone() });
    }

    pub(crate) fn table_editor_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.table_editor.take() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Edit Table")
            .id(egui::Id::unique("table_editor"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_pos([300.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                let columns = d.widths.len();
                egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("table_cells").spacing([4.0, 4.0]).show(ui, |ui| {
                        for (r, row) in d.cells.iter_mut().enumerate() {
                            if r == 0 {
                                ui.label("Title");
                                let field =
                                    egui::TextEdit::singleline(&mut row[0]).desired_width(110.0 * columns as f32);
                                let response = ui.add(field);
                                response
                                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Title"));
                                fields.push(response.id);
                                ui.end_row();
                                continue;
                            }
                            ui.label(if r == 1 { "Header".to_string() } else { format!("Row {}", r - 1) });
                            for (c, cell) in row.iter_mut().enumerate() {
                                let label = format!("{}{}", (b'A' + (c % 26) as u8) as char, r + 1);
                                let response = ui.add(egui::TextEdit::singleline(cell).desired_width(106.0));
                                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, &label));
                                fields.push(response.id);
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.horizontal(|ui| {
                    let row_height = d.heights.last().copied().unwrap_or(1.0);
                    if ui.button("Add Row").clicked() {
                        d.cells.push(vec![String::new(); columns]);
                        d.heights.push(row_height);
                    }
                    if ui.add_enabled(d.heights.len() > 3, egui::Button::new("Delete Row")).clicked() {
                        d.cells.pop();
                        d.heights.pop();
                    }
                    let width = d.widths.last().copied().unwrap_or(1.0);
                    if ui.button("Add Column").clicked() {
                        d.widths.push(width);
                        d.cells.iter_mut().for_each(|row| row.push(String::new()));
                    }
                    if ui.add_enabled(columns > 1, egui::Button::new("Delete Column")).clicked() {
                        d.widths.pop();
                        d.cells.iter_mut().for_each(|row| {
                            row.truncate(d.widths.len().max(1));
                        });
                    }
                });
                ui.separator();
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        if ok {
            let (cells, widths, heights) = (d.cells.clone(), d.widths.clone(), d.heights.clone());
            self.doc.begin("TABLEDIT");
            self.doc.update(d.id, |e| {
                if let Geometry::Table { cells: c, widths: w, heights: h, .. } = &mut e.geometry {
                    (*c, *w, *h) = (cells, widths, heights);
                }
            });
            self.doc.commit();
        } else if open && !cancel {
            self.table_editor = Some(d);
        }
    }
}

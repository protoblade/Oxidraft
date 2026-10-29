//! AutoCAD's classic dialogs: ARRAYCLASSIC (the Array dialog: rectangular
//! or polar copies), CLASSICGROUP (Object Grouping: groups made, renamed,
//! described, selected and exploded) and LAYOUTWIZARD (a new layout's
//! name, paper and orientation, and a viewport onto the drawing).

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_geom::Point;
use oxidraft_model::EntityId;
use oxidraft_plot::PAPERS;

use crate::{app::OxidraftApp, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrayType {
    Rectangular,
    Polar,
}

pub(crate) struct ArrayDialog {
    kind: ArrayType,
    objects: Vec<EntityId>,
    rows: usize,
    columns: usize,
    row_offset: f64,
    column_offset: f64,
    angle: f64,
    center: Point,
    items: usize,
    fill: f64,
    rotate: bool,
    /// Hidden while objects are selected (SELECT).
    picking: bool,
}

/// Where each copy goes: its displacement and turn (about `center`), the
/// original left out.
fn placements(d: &ArrayDialog) -> Vec<(Point, f64)> {
    match d.kind {
        ArrayType::Rectangular => (0..d.rows.max(1))
            .flat_map(|r| (0..d.columns.max(1)).map(move |c| (r, c)))
            .filter(|rc| *rc != (0, 0))
            .map(|(r, c)| {
                let v = Point::new(c as f64 * d.column_offset, r as f64 * d.row_offset);
                (v.rotated_about(Point::ORIGIN, d.angle), 0.0)
            })
            .collect(),
        ArrayType::Polar => {
            let n = d.items.max(1);
            // A full circle spaces them around it; less spaces them across it.
            let step =
                if (d.fill.abs() - 360.0).abs() < 1e-9 { d.fill / n as f64 } else { d.fill / (n.max(2) - 1) as f64 };
            (1..n).map(|i| (Point::ORIGIN, step * i as f64)).collect()
        }
    }
}

pub(crate) struct GroupDialog {
    picked: Option<String>,
    name: String,
    description: String,
    unnamed: bool,
    error: Option<String>,
}

pub(crate) struct LayoutWizard {
    name: String,
    paper: usize,
    landscape: bool,
    viewport: bool,
}

impl OxidraftApp {
    pub(crate) fn open_array_dialog(&mut self) {
        self.array_dialog = Some(ArrayDialog {
            kind: ArrayType::Rectangular,
            objects: self.engine.selection().to_vec(),
            rows: 4,
            columns: 4,
            row_offset: 10.0,
            column_offset: 10.0,
            angle: 0.0,
            center: Point::ORIGIN,
            items: 4,
            fill: 360.0,
            rotate: true,
            picking: false,
        });
    }

    /// The Array dialog's copies, as one undoable step.
    fn make_array(&mut self, d: &ArrayDialog) -> usize {
        let originals: Vec<oxidraft_model::Entity> =
            d.objects.iter().filter_map(|id| self.doc.entity(*id).cloned()).collect();
        self.engine.cancel(&mut self.doc);
        self.doc.begin("ARRAY");
        let mut n = 0;
        for (shift, turn) in placements(d) {
            for e in &originals {
                let g = match d.kind {
                    ArrayType::Rectangular => e.geometry.translated(shift),
                    ArrayType::Polar if d.rotate => e.geometry.rotated(d.center, turn),
                    // Not turned: moved round with its middle.
                    ArrayType::Polar => {
                        let mid = self.doc.entity_bounds(e).center();
                        e.geometry.translated(mid.rotated_about(d.center, turn) - mid)
                    }
                };
                self.doc.add_like(g, e);
                n += 1;
            }
        }
        self.doc.commit();
        n
    }

    pub(crate) fn array_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.array_dialog.take() else { return };
        if d.picking {
            if !self.engine.is_active() {
                d.picking = false;
                d.objects = self.engine.previous_selection().to_vec();
            }
            self.array_dialog = Some(d);
            return;
        }
        let (mut open, mut ok, mut cancel, mut select) = (true, false, false, false);
        egui::Window::new("Array")
            .id(egui::Id::unique("array_classic"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.kind, ArrayType::Rectangular, "Rectangular Array");
                    ui.radio_value(&mut d.kind, ArrayType::Polar, "Polar Array");
                });
                ui.horizontal(|ui| {
                    select = ui.button("Select objects").clicked();
                    ui.label(format!("{} objects selected", d.objects.len()));
                });
                ui.separator();
                let number = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
                    ui.label(label);
                    let r = ui.add(egui::DragValue::new(v).speed(0.5));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, label));
                    ui.end_row();
                };
                egui::Grid::new("array_fields").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| match d.kind {
                    ArrayType::Rectangular => {
                        ui.label("Rows:");
                        ui.add(egui::DragValue::new(&mut d.rows).range(1..=1000));
                        ui.end_row();
                        ui.label("Columns:");
                        ui.add(egui::DragValue::new(&mut d.columns).range(1..=1000));
                        ui.end_row();
                        number(ui, "Row offset:", &mut d.row_offset);
                        number(ui, "Column offset:", &mut d.column_offset);
                        number(ui, "Angle of array:", &mut d.angle);
                    }
                    ArrayType::Polar => {
                        number(ui, "Center point X:", &mut d.center.x);
                        number(ui, "Center point Y:", &mut d.center.y);
                        ui.label("Total number of items:");
                        ui.add(egui::DragValue::new(&mut d.items).range(2..=1000));
                        ui.end_row();
                        number(ui, "Angle to fill:", &mut d.fill);
                        ui.label("");
                        ui.checkbox(&mut d.rotate, "Rotate items as copied");
                        ui.end_row();
                    }
                });
                let copies = placements(&d).len() * d.objects.len();
                ui.colored_label(theme::text_dim(), format!("{copies} copies"));
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.add_enabled(!d.objects.is_empty(), egui::Button::new("OK")).clicked();
                });
            });
        if select {
            d.picking = true;
            self.run("SELECT");
            self.array_dialog = Some(d);
            return;
        }
        if ok {
            let n = self.make_array(&d);
            self.engine.log(format!("{n} copies made."));
            return;
        }
        if open && !cancel {
            self.array_dialog = Some(d);
        }
    }

    pub(crate) fn open_group_dialog(&mut self) {
        self.group_dialog = Some(GroupDialog {
            picked: None,
            name: String::new(),
            description: String::new(),
            unnamed: false,
            error: None,
        });
    }

    pub(crate) fn group_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.group_dialog.take() else { return };
        let (mut open, mut ok) = (true, false);
        let (mut new, mut explode, mut rename, mut describe, mut highlight) = (false, false, false, false, false);
        let mut fields = Vec::new();
        let groups = self.doc.groups();
        egui::Window::new("Object Grouping")
            .id(egui::Id::unique("classic_group"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                egui::Grid::new("group_list").num_columns(3).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new("Group Name").strong());
                    ui.label(RichText::new("Objects").strong());
                    ui.label(RichText::new("Description").strong());
                    ui.end_row();
                    for g in groups.iter().filter(|g| d.unnamed || !g.is_unnamed()) {
                        if ui.selectable_label(d.picked.as_deref() == Some(g.name.as_str()), &g.name).clicked() {
                            d.picked = Some(g.name.clone());
                            (d.name, d.description) = (g.name.clone(), g.description.clone());
                        }
                        ui.label(g.members.len().to_string());
                        ui.label(&g.description);
                        ui.end_row();
                    }
                });
                ui.checkbox(&mut d.unnamed, "Include Unnamed");
                ui.separator();
                for (label, value) in [("Group Name:", &mut d.name), ("Description:", &mut d.description)] {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let r = ui.add(egui::TextEdit::singleline(value).desired_width(220.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
                        fields.push(r.id);
                    });
                }
                ui.horizontal(|ui| {
                    new = ui.button("New").on_hover_text("A group of the selected objects").clicked();
                    let picked = d.picked.is_some();
                    rename = ui.add_enabled(picked, egui::Button::new("Rename")).clicked();
                    describe = ui.add_enabled(picked, egui::Button::new("Description")).clicked();
                    highlight = ui.add_enabled(picked, egui::Button::new("Highlight")).clicked();
                    explode = ui.add_enabled(picked, egui::Button::new("Explode")).clicked();
                });
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    ok = ui.button("OK").clicked();
                });
            });
        self.text_fields.extend(fields);
        let picked = d.picked.clone().and_then(|n| groups.iter().find(|g| g.name == n).cloned());
        if new {
            let members = self.engine.selection().to_vec();
            if members.is_empty() {
                d.error = Some("Select the objects first.".into());
            } else {
                self.doc.begin("GROUP");
                match self.doc.add_group(&d.name, &d.description, members) {
                    Ok(name) => {
                        self.doc.commit();
                        d.picked = Some(name);
                        d.error = None;
                    }
                    Err(e) => {
                        self.doc.rollback();
                        d.error = Some(e);
                    }
                }
            }
        }
        if let Some(g) = picked {
            let others: Vec<oxidraft_model::Group> =
                self.doc.all_groups().iter().filter(|x| x.name != g.name).cloned().collect();
            if explode {
                let rest = others.clone();
                self.doc.begin("GROUP");
                self.doc.set_groups(rest);
                self.doc.commit();
                d.picked = None;
            }
            if rename || describe {
                let name = if rename { d.name.trim().to_string() } else { g.name.clone() };
                let clash = rename && others.iter().any(|x| x.name.eq_ignore_ascii_case(&name));
                if clash || name.is_empty() {
                    d.error = Some(format!("A group named \"{name}\" already exists."));
                } else {
                    let edited =
                        oxidraft_model::Group { name: name.clone(), description: d.description.clone(), ..g.clone() };
                    let all: Vec<_> = self
                        .doc
                        .all_groups()
                        .iter()
                        .map(|x| if x.name == g.name { edited.clone() } else { x.clone() })
                        .collect();
                    self.doc.begin("GROUP");
                    self.doc.set_groups(all);
                    self.doc.commit();
                    d.picked = Some(name);
                    d.error = None;
                }
            }
            if highlight {
                self.engine.set_selection(g.members.clone());
            }
        }
        if open && !ok {
            self.group_dialog = Some(d);
        }
    }

    pub(crate) fn open_layout_wizard(&mut self) {
        let n = self.doc.layouts().len() + 1;
        self.layout_wizard =
            Some(LayoutWizard { name: format!("Layout{n}"), paper: 1, landscape: true, viewport: true });
    }

    /// LAYOUTWIZARD's Finish: the layout, its paper, and a viewport onto
    /// the drawing filling its printable area.
    pub fn finish_layout_wizard(
        &mut self,
        name: &str,
        paper: usize,
        landscape: bool,
        viewport: bool,
    ) -> Result<(), String> {
        let p = PAPERS.get(paper).ok_or("No such paper.")?;
        self.engine.cancel(&mut self.doc);
        self.doc.begin("LAYOUTWIZARD");
        let space = match self.doc.new_layout(name) {
            Ok(s) => s,
            Err(e) => {
                self.doc.rollback();
                return Err(e);
            }
        };
        let (w, h) = crate::layouts::turned(p, landscape);
        let current =
            self.doc.layout(space).map(|l| l.paper.clone()).unwrap_or_else(oxidraft_model::Paper::a4_landscape);
        let margin = oxidraft_plot::MARGIN;
        let paper = current.resized(&crate::layouts::media_name(p, (w, h)), w, h, [margin; 4], 0);
        let printable = paper.sheet_bounds().1;
        self.doc.set_paper(space, paper);
        if viewport {
            let g = oxidraft_commands::viewport_onto_model(&self.doc, printable);
            let here = self.doc.space();
            self.doc.set_space(space);
            self.doc.add(g);
            self.doc.set_space(here);
        }
        self.doc.commit();
        self.switch_space(space);
        Ok(())
    }

    pub(crate) fn layout_wizard(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.layout_wizard.take() else { return };
        let (mut open, mut finish, mut cancel) = (true, false, false);
        let mut fields = Vec::new();
        egui::Window::new("Create Layout")
            .id(egui::Id::unique("layout_wizard"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Layout name:");
                    let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(180.0));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Layout name"));
                    fields.push(r.id);
                });
                ui.horizontal(|ui| {
                    ui.label("Paper size:");
                    egui::ComboBox::from_id_salt("wizard_paper")
                        .width(240.0)
                        .selected_text(PAPERS[d.paper].name)
                        .show_ui(ui, |ui| {
                            for (i, p) in PAPERS.iter().enumerate() {
                                ui.selectable_value(&mut d.paper, i, p.name);
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Orientation:");
                    ui.radio_value(&mut d.landscape, false, "Portrait");
                    ui.radio_value(&mut d.landscape, true, "Landscape");
                });
                ui.checkbox(&mut d.viewport, "A viewport onto the drawing, filling the printable area");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    finish = ui.button("Finish").clicked();
                });
            });
        self.text_fields.extend(fields);
        if finish {
            match self.finish_layout_wizard(&d.name.clone(), d.paper, d.landscape, d.viewport) {
                Ok(()) => return,
                Err(e) => self.engine.log(e),
            }
        }
        if open && !cancel {
            self.layout_wizard = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(kind: ArrayType) -> ArrayDialog {
        ArrayDialog {
            kind,
            objects: Vec::new(),
            rows: 2,
            columns: 3,
            row_offset: 10.0,
            column_offset: 5.0,
            angle: 0.0,
            center: Point::ORIGIN,
            items: 4,
            fill: 360.0,
            rotate: true,
            picking: false,
        }
    }

    #[test]
    fn copies_where_the_array_puts_them() {
        let rect = placements(&dialog(ArrayType::Rectangular));
        assert_eq!(rect.len(), 5, "2 x 3, less the original");
        assert!(rect.contains(&(Point::new(10.0, 10.0), 0.0)));
        let polar = placements(&dialog(ArrayType::Polar));
        let turns: Vec<f64> = polar.iter().map(|p| p.1).collect();
        assert_eq!(turns, [90.0, 180.0, 270.0]);
        let half = placements(&ArrayDialog { fill: 180.0, items: 3, ..dialog(ArrayType::Polar) });
        assert_eq!(half.iter().map(|p| p.1).collect::<Vec<_>>(), [90.0, 180.0]);
    }
}

//! Layouts from the tabs beside Model, as in AutoCAD: their shortcut menu
//! (New Layout, Delete, Rename, Copy, Page Setup, Plot), renaming a tab in
//! place by double-clicking it, and the Page Setup dialog for a layout's
//! paper.

use eframe::egui::{self, Align, Color32, Key, Layout, Modifiers, Rect, RichText, Sense, Stroke, Ui, vec2};
use oxidraft_model::{MODEL, Paper, Space};
use oxidraft_plot::{MARGIN, PAPERS};

use crate::{app::OxidraftApp, theme, view::View};

/// What a layout tab's shortcut menu asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TabAction {
    New,
    Delete(Space),
    Rename(Space),
    Copy(Space),
    MoveOrCopy(Space),
    FromTemplate,
    PageSetup(Space),
    Plot(Space),
    Model,
}

/// A layout tab being renamed in place: its space and the name typed.
pub(crate) struct Renaming {
    pub space: Space,
    pub text: String,
    /// The field has had the keyboard (it's given it on its first frame).
    focused: bool,
}

/// The Move or Copy dialog: where a layout's tab goes (before another,
/// or `None` for the end), and whether a copy goes there instead.
pub(crate) struct MoveOrCopy {
    space: Space,
    before: Option<Space>,
    copy: bool,
}

/// Insert Layout(s) from a template drawing: its layouts, the ones picked.
pub(crate) struct FromTemplate {
    source: oxidraft_model::Document,
    file: String,
    picked: Vec<bool>,
}

/// The Page Setup dialog's choices for a layout.
pub(crate) struct PageSetup {
    space: Space,
    /// One of [`PAPERS`], or `None` for the layout's own paper.
    paper: Option<usize>,
    landscape: bool,
}

/// The paper size's name as AutoCAD writes it, turned for the
/// orientation: "ISO_A4_(297.00_x_210.00_MM)".
fn media_name(p: &oxidraft_plot::Paper, (w, h): (f64, f64)) -> String {
    let base = p.name.split(" (").next().unwrap_or(p.name).replace(' ', "_");
    if p.name.contains("Inches") {
        format!("{base}_({:.2}_x_{:.2}_Inches)", w / 25.4, h / 25.4)
    } else {
        format!("{base}_({w:.2}_x_{h:.2}_MM)")
    }
}

/// The sheet `paper` makes, in millimeters, turned for the orientation.
fn turned(p: &oxidraft_plot::Paper, landscape: bool) -> (f64, f64) {
    let (short, long) = (p.width.min(p.height), p.width.max(p.height));
    if landscape { (long, short) } else { (short, long) }
}

impl OxidraftApp {
    /// The drawing's current space changed under the view (LAYOUT Set or
    /// Delete, undo): the view the space had, and the one left is kept.
    pub(crate) fn follow_space(&mut self) {
        match self.active_viewport.take() {
            Some(a) => {
                self.space_views.insert(a.layout, a.page);
            }
            None if self.view_space == MODEL || self.doc.layout(self.view_space).is_some() => {
                self.space_views.insert(self.view_space, self.view);
            }
            None => {}
        }
        self.engine.cancel(&mut self.doc);
        self.show_space_view();
    }

    /// The view the current space had when left; a layout seen first shows
    /// its whole sheet, model space its extents.
    pub(crate) fn show_space_view(&mut self) {
        let space = self.doc.space();
        match (self.space_views.get(&space), self.doc.layout(space)) {
            (Some(view), _) => self.view = *view,
            (None, Some(layout)) => {
                let (sheet, _) = layout.paper.sheet_bounds();
                self.pending_views.push(oxidraft_commands::ViewRequest::Window(sheet));
            }
            (None, None) => self.zoom_extents_pending = true,
        }
        self.view_space = space;
    }

    pub(crate) fn tab_action(&mut self, action: TabAction) {
        // The shortcut menu's work is on the sheets: out of a viewport first.
        self.leave_viewport();
        let name = |app: &Self, space: Space| app.doc.layout(space).map(|l| l.name.clone()).unwrap_or_default();
        match action {
            TabAction::New => {
                let name = self.doc.next_layout_name();
                self.doc.begin("LAYOUT");
                let made = self.doc.new_layout(&name);
                self.doc.commit();
                if let Err(e) = made {
                    self.engine.log(e);
                }
            }
            TabAction::Delete(space) => self.delete_layout_prompt = Some(space),
            TabAction::Rename(space) => {
                self.renaming_layout = Some(Renaming { space, text: name(self, space), focused: false });
            }
            TabAction::Copy(space) => {
                let source = name(self, space);
                let copy = (2..)
                    .map(|n| format!("{source} ({n})"))
                    .find(|n| self.doc.layout_named(n).is_none())
                    .unwrap_or_default();
                self.doc.begin("LAYOUT");
                let made = self.doc.copy_layout(space, &copy);
                self.doc.commit();
                if let Err(e) = made {
                    self.engine.log(e);
                }
            }
            TabAction::MoveOrCopy(space) => {
                self.move_or_copy = Some(MoveOrCopy { space, before: None, copy: false });
            }
            TabAction::FromTemplate => self.layout_from_template(),
            TabAction::PageSetup(MODEL) => {
                self.switch_space(MODEL);
                self.open_model_page_setup();
            }
            TabAction::PageSetup(space) => self.open_page_setup(space),
            TabAction::Plot(space) => {
                self.switch_space(space);
                self.open_plot(false);
            }
            TabAction::Model => self.switch_space(MODEL),
        }
    }

    /// A layout tab being renamed: a field in its place. Enter or a click
    /// elsewhere renames it, Esc leaves it as it was.
    pub(crate) fn rename_field(&mut self, ui: &mut Ui) {
        let Some(r) = &mut self.renaming_layout else { return };
        let id = egui::Id::unique(("rename_layout", r.space));
        let width =
            ui.painter().layout_no_wrap(r.text.clone(), egui::FontId::proportional(12.0), theme::text()).size().x;
        let field = egui::TextEdit::singleline(&mut r.text).id(id).desired_width(width.max(60.0) + 12.0);
        let response = ui.add(field);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Layout name"));
        self.text_fields.push(id);
        if !r.focused {
            response.request_focus();
            r.focused = true;
            return;
        }
        if !response.lost_focus() && response.has_focus() {
            return;
        }
        let escaped = ui.input(|i| i.key_pressed(Key::Escape));
        let Some(r) = self.renaming_layout.take() else { return };
        let unchanged = self.doc.layout(r.space).is_some_and(|l| l.name == r.text.trim());
        if escaped || unchanged {
            return;
        }
        self.doc.begin("RENAME");
        let renamed = self.doc.rename_layout(r.space, &r.text);
        self.doc.commit();
        if let Err(e) = renamed {
            self.engine.log(e);
        }
    }

    /// Move or Copy: the layout's tab before another, or at the end, or a
    /// copy of it there.
    pub(crate) fn move_or_copy_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.move_or_copy.take() else { return };
        let Some(name) = self.doc.layout(d.space).map(|l| l.name.clone()) else { return };
        let others: Vec<(Space, String)> =
            self.doc.layouts().iter().filter(|l| l.id != d.space).map(|l| (l.id, l.name.clone())).collect();
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new(format!("Move or Copy - {name}"))
            .id(egui::Id::unique("move_or_copy"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([480.0, 240.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label("Before layout:");
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(240.0);
                    for (id, other) in &others {
                        ui.selectable_value(&mut d.before, Some(*id), other);
                    }
                    ui.selectable_value(&mut d.before, None, "(move to end)");
                });
                ui.checkbox(&mut d.copy, "Create a copy");
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok {
            self.doc.begin("LAYOUT");
            let moved = if d.copy {
                let copy = (2..)
                    .map(|n| format!("{name} ({n})"))
                    .find(|n| self.doc.layout_named(n).is_none())
                    .unwrap_or_default();
                self.doc.copy_layout(d.space, &copy)
            } else {
                Ok(d.space)
            };
            match moved {
                Ok(space) => self.doc.move_layout(space, d.before),
                Err(e) => self.engine.log(e),
            }
            self.doc.commit();
        }
        if open && !ok && !cancel {
            self.move_or_copy = Some(d);
        }
    }

    /// From Template: layouts from another drawing (a DWG, DXF or DWT),
    /// picked in Insert Layout(s).
    fn layout_from_template(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Drawing template or drawing", &["dwt", "dwg", "dxf"]);
        let Some(path) = dialog.pick_file() else { return };
        self.open_template(&path);
    }

    /// Insert Layout(s) for the drawing at `path`.
    pub fn open_template(&mut self, path: &std::path::Path) {
        match crate::xref::read_drawing(path) {
            Ok(source) if source.layouts().is_empty() => {
                self.engine.log(format!("{} has no layouts.", path.display()));
            }
            Ok(source) => {
                let picked = vec![true; source.layouts().len()];
                let file = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                self.from_template = Some(FromTemplate { source, file, picked });
            }
            Err(e) => self.engine.log(format!("Could not read {}: {e}", path.display())),
        }
    }

    pub(crate) fn insert_layouts_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.from_template.take() else { return };
        let names: Vec<String> = d.source.layouts().iter().map(|l| l.name.clone()).collect();
        let (mut open, mut ok, mut cancel) = (true, false, false);
        egui::Window::new("Insert Layout(s)")
            .id(egui::Id::unique("from_template"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([480.0, 240.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(format!("Layout name(s) in {}:", d.file));
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(240.0);
                    for (name, picked) in names.iter().zip(&mut d.picked) {
                        ui.checkbox(picked, name);
                    }
                });
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.add_enabled(d.picked.contains(&true), egui::Button::new("OK")).clicked();
                });
            });
        if ok {
            self.insert_layouts(&d);
        }
        if open && !ok && !cancel {
            self.from_template = Some(d);
        }
    }

    /// The picked layouts after this drawing's: each with its page setup,
    /// what's on its sheet, and the layers, linetypes and blocks those use.
    fn insert_layouts(&mut self, d: &FromTemplate) {
        self.doc.begin("LAYOUT");
        let current = self.doc.space();
        for (layout, _) in d.source.layouts().into_iter().zip(&d.picked).filter(|(_, p)| **p) {
            let name = match self.doc.layout_named(&layout.name) {
                None => layout.name.clone(),
                Some(_) => (2..)
                    .map(|n| format!("{} ({n})", layout.name))
                    .find(|n| self.doc.layout_named(n).is_none())
                    .unwrap_or_default(),
            };
            let tab_order = self.doc.layouts().iter().map(|l| l.tab_order).max().unwrap_or(0) + 1;
            let copy = oxidraft_model::layout::Layout { id: MODEL, name, tab_order, paper: layout.paper.clone() };
            let space = self.doc.add_layout(copy);
            let ids: Vec<_> = d.source.space_in_draw_order(layout.id).iter().map(|e| e.id).collect();
            let clip = oxidraft_commands::clip_of(&d.source, &ids, oxidraft_geom::Point::ORIGIN);
            self.doc.set_space(space);
            oxidraft_commands::paste(&mut self.doc, &clip, oxidraft_geom::Point::ORIGIN);
        }
        self.doc.set_space(current);
        self.doc.commit();
    }

    /// Deleting a layout asks first, as AutoCAD does.
    pub(crate) fn delete_layout_dialog(&mut self, ctx: &egui::Context) {
        let Some(space) = self.delete_layout_prompt else { return };
        let Some(name) = self.doc.layout(space).map(|l| l.name.clone()) else {
            self.delete_layout_prompt = None;
            return;
        };
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new("Oxidraft - Delete Layout")
            .id(egui::Id::unique("delete_layout"))
            .collapsible(false)
            .resizable(false)
            .default_pos([480.0, 300.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(format!("The layout \"{name}\" and everything on it will be permanently deleted."));
                ui.add_space(8.0);
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok {
            self.doc.begin("LAYOUT");
            self.doc.delete_layout(space);
            self.doc.commit();
            self.engine.log(format!("Layout \"{name}\" deleted."));
        }
        if ok || cancel {
            self.delete_layout_prompt = None;
        }
    }

    /// PAGESETUP: the current layout's paper.
    pub(crate) fn page_setup_command(&mut self) {
        let space = self.shown_space();
        if space == MODEL {
            self.open_model_page_setup();
            return;
        }
        self.open_page_setup(space);
    }

    pub(crate) fn open_page_setup(&mut self, space: Space) {
        let Some(layout) = self.doc.layout(space) else { return };
        let sheet = layout.paper.sheet_bounds().0;
        let (w, h) = (sheet.width(), sheet.height());
        let mm = if layout.paper.millimeters { 1.0 } else { 25.4 };
        let (w, h) = (w * mm, h * mm);
        // Its paper among the plotter's, either way round.
        let paper = PAPERS.iter().position(|p| {
            let (a, b) = turned(p, w > h);
            (a - w).abs() < 0.5 && (b - h).abs() < 0.5
        });
        self.page_setup = Some(PageSetup { space, paper, landscape: w >= h });
    }

    pub(crate) fn page_setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.page_setup.take() else { return };
        let Some(layout) = self.doc.layout(d.space).cloned() else { return };
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let own = layout.paper.size_name.replace('_', " ");
        egui::Window::new(format!("Page Setup - {}", layout.name))
            .id(egui::Id::unique("page_setup"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([380.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Paper size").strong());
                let shown = match d.paper {
                    Some(i) => PAPERS[i].name.to_string(),
                    None if own.trim().is_empty() => "As the layout has it".into(),
                    None => own.clone(),
                };
                egui::ComboBox::from_id_salt("page_setup_paper").width(320.0).selected_text(shown).show_ui(ui, |ui| {
                    if !own.trim().is_empty() {
                        ui.selectable_value(&mut d.paper, None, &own);
                    }
                    for (i, p) in PAPERS.iter().enumerate() {
                        ui.selectable_value(&mut d.paper, Some(i), p.name);
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Drawing orientation").strong());
                ui.add_enabled_ui(d.paper.is_some(), |ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut d.landscape, false, "Portrait");
                        ui.radio_value(&mut d.landscape, true, "Landscape");
                    });
                });
                // The sheet and its printable area, as they'll be.
                let paper = self.chosen_paper(&layout.paper, &d);
                let (sheet, printable) = paper.sheet_bounds();
                ui.add_space(6.0);
                let (space, _) = ui.allocate_exact_size(vec2(320.0, 150.0), Sense::hover());
                ui.painter().rect_filled(space, 4.0, theme::input_bg());
                let k = ((space.width() - 20.0) as f64 / sheet.width())
                    .min((space.height() - 20.0) as f64 / sheet.height());
                let view = View::new(sheet.center(), k);
                let to = |p| view.to_screen(space, p);
                let on_screen = Rect::from_two_pos(to(sheet.min), to(sheet.max));
                ui.painter().rect_filled(on_screen, 0.0, Color32::WHITE);
                let inner = Rect::from_two_pos(to(printable.min), to(printable.max));
                ui.painter().rect_stroke(
                    inner,
                    0.0,
                    Stroke::new(1.0, Color32::from_gray(150)),
                    egui::StrokeKind::Middle,
                );
                let units = if paper.millimeters { "mm" } else { "inches" };
                ui.colored_label(
                    theme::text_dim(),
                    format!(
                        "Printable area: {:.2} x {:.2} {units}. Layouts plot at 1:1.",
                        printable.width(),
                        printable.height()
                    ),
                );
                ui.separator();
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if ok {
            let paper = self.chosen_paper(&layout.paper, &d);
            if paper != layout.paper {
                self.doc.begin("PAGESETUP");
                self.doc.set_paper(d.space, paper);
                self.doc.commit();
            }
        }
        let escaped = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        if open && !ok && !cancel && !escaped {
            self.page_setup = Some(d);
        }
    }

    /// The layout's paper with the dialog's choices: one of the plotter's
    /// papers, turned for the orientation and with its margins.
    fn chosen_paper(&self, current: &Paper, d: &PageSetup) -> Paper {
        let Some(p) = d.paper.map(|i| &PAPERS[i]) else { return current.clone() };
        let (w, h) = turned(p, d.landscape);
        let mm = if current.millimeters { 1.0 } else { 25.4 };
        let sheet = current.sheet_bounds().0;
        let (cw, ch) = (sheet.width(), sheet.height());
        // The same sheet keeps its margins.
        let same = current.rotation.is_multiple_of(2) && (cw * mm - w).abs() < 0.5 && (ch * mm - h).abs() < 0.5;
        let margins = if same { current.margins } else { [MARGIN; 4] };
        current.resized(&media_name(p, (w, h)), w, h, margins, 0)
    }
}

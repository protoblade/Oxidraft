//! DCL dialogs on screen: what AutoLISP's `new_dialog` opens, drawn as
//! its tiles say, and what's done in it sent back to the program
//! (`action_tile`), until `done_dialog` closes it.

use std::collections::{HashMap, HashSet, VecDeque};

use eframe::egui::{self, Ui};

use crate::app::OxidraftApp;
use crate::dcl::{Dcl, Tile, keyed};
use crate::lisp::{DclAnswer, DclCall, ImageOp, Reply};

/// An image tile's size in pixels: its width in characters, its height in
/// lines.
fn image_size(tile: &Tile) -> egui::Vec2 {
    let n = |a: &str, d: f32| tile.attr(a).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
    egui::vec2(n("width", 10.0) * 8.0, n("height", 3.0) * 16.0)
}

/// A dialog open: its tiles, their values and lists, those disabled, and
/// what's been done in it not yet sent.
pub(crate) struct OpenDialog {
    tile: Tile,
    values: HashMap<String, String>,
    lists: HashMap<String, Vec<String>>,
    disabled: HashSet<String>,
    focus: Option<String>,
    /// What programs drew in image tiles, by key.
    images: HashMap<String, Vec<ImageOp>>,
    /// start_dialog has shown it.
    pub(crate) running: bool,
    /// done_dialog's status, once it's called.
    pub(crate) done: Option<i64>,
    pub(crate) events: VecDeque<(String, String, i64)>,
}

impl OpenDialog {
    fn new(tile: Tile) -> Self {
        let mut values = HashMap::new();
        let mut lists = HashMap::new();
        for t in keyed(&tile) {
            let key = t.key().unwrap_or_default().to_string();
            if let Some(v) = t.attr("value") {
                values.insert(key.clone(), v.to_string());
            }
            if let Some(l) = t.attr("list") {
                lists.insert(key, l.split('\n').map(str::to_string).collect());
            }
        }
        Self {
            tile,
            values,
            lists,
            disabled: HashSet::new(),
            focus: None,
            images: HashMap::new(),
            running: false,
            done: None,
            events: VecDeque::new(),
        }
    }

    fn value(&self, key: &str) -> String {
        self.values.get(key).cloned().unwrap_or_default()
    }

    fn event(&mut self, key: &str, value: String, reason: i64) {
        self.events.push_back((key.to_string(), value, reason));
    }

    /// Draws `tile` and what's in it.
    fn show(&mut self, ui: &mut Ui, tile: &Tile) {
        let key = tile.key().unwrap_or("").to_string();
        let enabled = !self.disabled.contains(&key) && !tile.is("is_enabled_false");
        let label = tile.label();
        match tile.kind.as_str() {
            "row" | "radio_row" => {
                ui.horizontal(|ui| self.children(ui, tile));
            }
            "boxed_row" | "boxed_radio_row" => {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        if !label.is_empty() {
                            ui.label(&label);
                        }
                        ui.horizontal(|ui| self.children(ui, tile));
                    });
                });
            }
            "boxed_column" | "boxed_radio_column" => {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        if !label.is_empty() {
                            ui.label(&label);
                        }
                        self.children(ui, tile);
                    });
                });
            }
            // Drawn as the program drew it; an image button is picked where it's clicked.
            "image" | "image_button" => {
                let size = image_size(tile);
                let sense = if tile.kind == "image_button" { egui::Sense::click() } else { egui::Sense::hover() };
                let (rect, r) = ui.allocate_exact_size(size, sense);
                let painter = ui.painter_at(rect);
                let at = |x: f64, y: f64| rect.min + egui::vec2(x as f32, y as f32);
                let color = |c: i64| crate::theme::aci(u8::try_from(c.clamp(0, 255)).unwrap_or(7));
                for op in self.images.get(&key).into_iter().flatten() {
                    match *op {
                        ImageOp::Fill { x, y, w, h, color: c } => {
                            painter.rect_filled(
                                egui::Rect::from_min_size(at(x, y), egui::vec2(w as f32, h as f32)),
                                0.0,
                                color(c),
                            );
                        }
                        ImageOp::Vector { x1, y1, x2, y2, color: c } => {
                            painter.line_segment([at(x1, y1), at(x2, y2)], egui::Stroke::new(1.0, color(c)));
                        }
                        ImageOp::Slide { .. } => {}
                    }
                }
                if r.clicked() {
                    self.event(&key, String::new(), 1);
                }
            }
            "button" => {
                let r = ui.add_enabled(enabled, egui::Button::new(if label.is_empty() { key.clone() } else { label }));
                if self.focus.as_deref() == Some(&key) {
                    r.request_focus();
                    self.focus = None;
                }
                if r.clicked() {
                    self.event(&key, String::new(), 1);
                }
            }
            "edit_box" => {
                ui.horizontal(|ui| {
                    if !label.is_empty() {
                        ui.label(&label);
                    }
                    let width = tile.attr("edit_width").and_then(|w| w.parse::<f32>().ok()).unwrap_or(16.0) * 8.0;
                    let mut v = self.value(&key);
                    let r = ui.add_enabled(enabled, egui::TextEdit::singleline(&mut v).desired_width(width));
                    let name = if tile.label().is_empty() { key.clone() } else { tile.label() };
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, enabled, &name));
                    if self.focus.as_deref() == Some(&key) {
                        r.request_focus();
                        self.focus = None;
                    }
                    if r.changed() {
                        self.values.insert(key.clone(), v.clone());
                    }
                    if r.lost_focus() {
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        self.event(&key, v, if enter { 1 } else { 2 });
                    }
                });
            }
            "toggle" => {
                let mut on = self.value(&key) == "1";
                if ui.add_enabled(enabled, egui::Checkbox::new(&mut on, &label)).changed() {
                    let v = if on { "1" } else { "0" }.to_string();
                    self.values.insert(key.clone(), v.clone());
                    self.event(&key, v, 1);
                }
            }
            "radio_column" | "column" | "dialog" | "concatenation" | "paragraph" => {
                ui.vertical(|ui| self.children(ui, tile));
            }
            "radio_button" => {
                let on = self.value(&key) == "1";
                if ui.add_enabled(enabled, egui::RadioButton::new(on, &label)).clicked() && !on {
                    self.values.insert(key.clone(), "1".into());
                    self.event(&key, "1".into(), 1);
                }
            }
            "list_box" => {
                if !label.is_empty() {
                    ui.label(&label);
                }
                let items = self.lists.get(&key).cloned().unwrap_or_default();
                let chosen = self.value(&key);
                let height = tile.attr("height").and_then(|h| h.parse::<f32>().ok()).unwrap_or(8.0) * 16.0;
                egui::ScrollArea::vertical().id_salt(("dcl_list", key.clone())).max_height(height).show(ui, |ui| {
                    for (i, item) in items.iter().enumerate() {
                        let r = ui.add_enabled(enabled, egui::Button::selectable(chosen == i.to_string(), item));
                        if r.clicked() || r.double_clicked() {
                            self.values.insert(key.clone(), i.to_string());
                            self.event(&key, i.to_string(), if r.double_clicked() { 4 } else { 1 });
                        }
                    }
                });
            }
            "popup_list" => {
                let items = self.lists.get(&key).cloned().unwrap_or_default();
                let chosen = self.value(&key).parse::<usize>().ok();
                let shown = chosen.and_then(|i| items.get(i)).cloned().unwrap_or_default();
                ui.horizontal(|ui| {
                    if !label.is_empty() {
                        ui.label(&label);
                    }
                    ui.add_enabled_ui(enabled, |ui| {
                        let r = egui::ComboBox::from_id_salt(("dcl_popup", key.clone())).selected_text(shown).show_ui(
                            ui,
                            |ui| {
                                for (i, item) in items.iter().enumerate() {
                                    if ui.selectable_label(chosen == Some(i), item).clicked() {
                                        self.values.insert(key.clone(), i.to_string());
                                        self.event(&key, i.to_string(), 1);
                                    }
                                }
                            },
                        );
                        let name = if tile.label().is_empty() { key.clone() } else { tile.label() };
                        r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, &name));
                    });
                });
            }
            "slider" => {
                let min = tile.attr("min_value").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                let max = tile.attr("max_value").and_then(|v| v.parse::<f64>().ok()).unwrap_or(10000.0);
                let mut v = self.value(&key).parse::<f64>().unwrap_or(min);
                let r = ui.add_enabled(enabled, egui::Slider::new(&mut v, min..=max).integer());
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, enabled, &key));
                if r.changed() {
                    let v = format!("{}", v.round());
                    self.values.insert(key.clone(), v.clone());
                    self.event(&key, v, 3);
                }
            }
            // text, errtile and the spacers: their value, else their label.
            _ => {
                let shown = self.values.get(&key).cloned().unwrap_or(label);
                if tile.children.is_empty() {
                    ui.label(shown);
                } else {
                    ui.vertical(|ui| self.children(ui, tile));
                }
            }
        }
    }

    fn children(&mut self, ui: &mut Ui, tile: &Tile) {
        let radio = tile.kind.contains("radio");
        for c in &tile.children {
            let before = c.key().map(|k| self.value(k));
            self.show(ui, c);
            // A radio button picked: the others off, the cluster's value its key.
            if radio
                && let (Some(k), Some(b)) = (c.key(), before)
                && b != "1"
                && self.value(k) == "1"
            {
                for o in tile.children.iter().filter_map(Tile::key).filter(|o| *o != k) {
                    self.values.insert(o.to_string(), "0".into());
                }
                if let Some(cluster) = tile.key() {
                    self.values.insert(cluster.to_string(), k.to_string());
                    self.event(cluster, k.to_string(), 1);
                }
            }
        }
    }
}

impl OxidraftApp {
    /// What a program asks of its dialog. `None` while the reply waits for
    /// what's done in it.
    pub(crate) fn dcl_request(&mut self, call: DclCall) -> Option<Reply> {
        let text = |t: String| Some(Reply::Dcl(DclAnswer::Text(t)));
        match call {
            DclCall::Load(name) => {
                let mut path = std::path::PathBuf::from(&name);
                if path.extension().is_none() {
                    path.set_extension("dcl");
                }
                if path.is_relative()
                    && let Some(dir) = self.path.as_ref().and_then(|p| p.parent())
                    && dir.join(&path).exists()
                {
                    path = dir.join(&path);
                }
                let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| Dcl::parse(&s));
                match parsed {
                    Ok(dcl) => {
                        self.lisp.dcl_files.push(Some(dcl));
                        Some(Reply::Dcl(DclAnswer::Id(self.lisp.dcl_files.len() as i64)))
                    }
                    Err(e) => {
                        self.engine.log(format!("load_dialog: {}: {e}", path.display()));
                        Some(Reply::Dcl(DclAnswer::Failed))
                    }
                }
            }
            DclCall::Unload(id) => {
                if let Some(f) = self.lisp.dcl_files.get_mut((id - 1).max(0) as usize) {
                    *f = None;
                }
                Some(Reply::Unit)
            }
            DclCall::New(name, id) => {
                let tile = self
                    .lisp
                    .dcl_files
                    .get((id - 1).max(0) as usize)
                    .and_then(Option::as_ref)
                    .and_then(|d| d.dialog(&name))
                    .cloned();
                match tile {
                    Some(t) => {
                        self.lisp.dialog = Some(OpenDialog::new(t));
                        text("T".into())
                    }
                    None => Some(Reply::Dcl(DclAnswer::Failed)),
                }
            }
            DclCall::Start => {
                let d = self.lisp.dialog.as_mut()?;
                d.running = true;
                self.lisp.wait_for_dialog();
                None
            }
            DclCall::Resume => match self.lisp.dialog.as_ref().and_then(|d| d.done) {
                Some(status) => {
                    self.lisp.dialog = None;
                    Some(Reply::Dcl(DclAnswer::Done(status)))
                }
                None if self.lisp.dialog.is_none() => Some(Reply::Dcl(DclAnswer::Done(0))),
                None => {
                    self.lisp.wait_for_dialog();
                    None
                }
            },
            DclCall::Done(status) => {
                if let Some(d) = &mut self.lisp.dialog {
                    d.done = Some(status);
                }
                Some(Reply::Unit)
            }
            DclCall::Term => {
                if let Some(d) = &mut self.lisp.dialog {
                    d.done = Some(0);
                }
                Some(Reply::Unit)
            }
            DclCall::SetTile(key, value) => {
                if let Some(d) = &mut self.lisp.dialog {
                    d.values.insert(key, value);
                }
                Some(Reply::Unit)
            }
            DclCall::GetTile(key) => text(self.lisp.dialog.as_ref().map(|d| d.value(&key)).unwrap_or_default()),
            DclCall::GetAttr(key, attr) => {
                let d = self.lisp.dialog.as_ref();
                let value = d
                    .and_then(|d| keyed(&d.tile).into_iter().find(|t| t.key() == Some(key.as_str())))
                    .and_then(|t| t.attr(&attr))
                    .unwrap_or_default();
                text(value.to_string())
            }
            DclCall::Mode(key, mode) => {
                if let Some(d) = &mut self.lisp.dialog {
                    match mode {
                        0 => {
                            d.disabled.remove(&key);
                        }
                        1 => {
                            d.disabled.insert(key);
                        }
                        2 | 3 => d.focus = Some(key),
                        _ => {}
                    }
                }
                Some(Reply::Unit)
            }
            DclCall::Image(key, ops) => {
                // A slide's vectors, fitted into its rectangle (slides run up).
                let mut drawn = Vec::new();
                for op in ops {
                    let ImageOp::Slide { x, y, w, h, name } = &op else {
                        drawn.push(op);
                        continue;
                    };
                    let mut file = std::path::PathBuf::from(name);
                    if file.extension().is_none() {
                        file.set_extension("sld");
                    }
                    if file.is_relative()
                        && let Some(dir) = self.path.as_ref().and_then(|p| p.parent())
                    {
                        file = dir.join(file);
                    }
                    let Ok(slide) = std::fs::read(&file)
                        .map_err(|e| e.to_string())
                        .and_then(|b| oxidraft_plot::slide::read_sld(&b))
                    else {
                        continue;
                    };
                    let k = (w / f64::from(slide.width.max(1))).min(h / f64::from(slide.height.max(1)));
                    let at = |p: (i16, i16)| (x + f64::from(p.0) * k, y + h - f64::from(p.1) * k);
                    for (c, a, b) in &slide.vectors {
                        let ((x1, y1), (x2, y2)) = (at(*a), at(*b));
                        drawn.push(ImageOp::Vector { x1, y1, x2, y2, color: i64::from(*c) });
                    }
                }
                if let Some(d) = &mut self.lisp.dialog {
                    d.images.insert(key, drawn);
                }
                Some(Reply::Unit)
            }
            DclCall::Size(key) => {
                let d = self.lisp.dialog.as_ref();
                let size = d
                    .and_then(|d| keyed(&d.tile).into_iter().find(|t| t.key() == Some(key.as_str())).map(image_size))
                    .unwrap_or_default();
                text(format!("{},{}", size.x as i64, size.y as i64))
            }
            DclCall::List(key, op, index, items) => {
                if let Some(d) = &mut self.lisp.dialog {
                    let list = d.lists.entry(key).or_default();
                    match (op, index) {
                        (1, Some(i)) => {
                            if let (Some(slot), Some(item)) = (list.get_mut(i), items.into_iter().next()) {
                                *slot = item;
                            }
                        }
                        (2, _) => list.extend(items),
                        _ => *list = items,
                    }
                }
                Some(Reply::Unit)
            }
        }
    }

    /// The dialog a program shows, while it waits on it.
    pub(crate) fn dcl_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.lisp.dialog.take() else { return };
        if !d.running || d.done.is_some() {
            self.lisp.dialog = Some(d);
            return;
        }
        let tile = d.tile.clone();
        let mut open = true;
        egui::Window::new(tile.label())
            .id(egui::Id::unique("dcl_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([300.0, 200.0])
            .show(ctx, |ui| d.show(ui, &tile));
        // Closed, or Esc: Cancel.
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            d.event("cancel", String::new(), 1);
        }
        if !d.events.is_empty() {
            ctx.request_repaint();
        }
        self.lisp.dialog = Some(d);
    }
}

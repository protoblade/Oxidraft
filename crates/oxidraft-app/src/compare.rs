//! COMPARE (DWG Compare): this drawing's model space against another
//! drawing's. What's only here draws green, what's only in the other
//! drawing red, and what both have gray, with a bar to say how many of
//! each and to stop comparing (COMPAREEXIT).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use eframe::egui::{self, Color32, RichText};
use oxidraft_commands::object_key;
use oxidraft_model::{Document, EntityId, Geometry, MODEL};

use crate::app::OxidraftApp;

/// Only here.
pub(crate) const HERE: Color32 = Color32::from_rgb(60, 200, 90);
/// Only in the other drawing.
pub(crate) const THERE: Color32 = Color32::from_rgb(235, 70, 60);
/// In both.
pub(crate) const BOTH: Color32 = Color32::from_rgb(120, 120, 120);

/// A comparison under way, worked out again when this drawing changes.
pub(crate) struct Comparison {
    file: String,
    other: Document,
    revision: u64,
    pub only_here: HashSet<EntityId>,
    /// The other drawing's objects this one hasn't (how many, and their
    /// parts to draw).
    there: usize,
    pub only_there: Vec<Geometry>,
    same: usize,
}

impl Comparison {
    fn new(file: String, other: Document, doc: &Document) -> Comparison {
        let mut c = Comparison {
            file,
            other,
            revision: u64::MAX,
            only_here: HashSet::new(),
            there: 0,
            only_there: Vec::new(),
            same: 0,
        };
        c.update(doc);
        c
    }

    /// Matches objects one for one by layer, properties and shape.
    fn update(&mut self, doc: &Document) {
        if self.revision == doc.revision() {
            return;
        }
        self.revision = doc.revision();
        let mut theirs: HashMap<String, Vec<EntityId>> = HashMap::new();
        for e in self.other.space_in_draw_order(MODEL) {
            theirs.entry(object_key(e)).or_default().push(e.id);
        }
        self.only_here.clear();
        self.same = 0;
        for e in doc.space_in_draw_order(MODEL) {
            match theirs.get_mut(&object_key(e)).and_then(Vec::pop) {
                Some(_) => self.same += 1,
                None => {
                    self.only_here.insert(e.id);
                }
            }
        }
        let other = &self.other;
        let remaining: Vec<EntityId> = theirs.into_values().flatten().collect();
        self.there = remaining.len();
        self.only_there = remaining.iter().filter_map(|id| other.entity(*id)).flat_map(|e| other.expanded(e)).collect();
    }
}

impl OxidraftApp {
    /// COMPARE: picks the drawing to compare with.
    pub(crate) fn compare_command(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Drawing", &["dwg", "dxf"]);
        if let Some(path) = dialog.pick_file() {
            self.compare_with(&path);
        }
    }

    /// Compares model space with the drawing at `path`'s.
    pub fn compare_with(&mut self, path: &Path) {
        match crate::xref::read_drawing(path) {
            Ok(other) => {
                let file = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                self.leave_viewport();
                self.switch_space(MODEL);
                let c = Comparison::new(file, other, &self.doc);
                self.engine.log(format!(
                    "Compared with {}: {} only here, {} only there, {} the same.",
                    c.file,
                    c.only_here.len(),
                    c.there,
                    c.same
                ));
                self.comparison = Some(c);
            }
            Err(e) => self.engine.log(format!("Could not read {}: {e}", path.display())),
        }
    }

    /// The bar over the drawing while comparing: the counts, and Exit.
    pub(crate) fn compare_bar(&mut self, ctx: &egui::Context) {
        let Some(c) = &mut self.comparison else { return };
        c.update(&self.doc);
        let mut exit = false;
        let r = self.drawing_rect;
        egui::Area::new(egui::Id::unique("compare_bar"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(r.center().x, r.top() + 30.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Compare with {}", c.file)).strong());
                        ui.colored_label(HERE, format!("{} only here", c.only_here.len()));
                        ui.colored_label(THERE, format!("{} only there", c.there));
                        ui.colored_label(BOTH, format!("{} the same", c.same));
                        exit = ui.button("Exit Compare").clicked();
                    });
                });
            });
        if exit {
            self.comparison = None;
        }
    }
}

//! STATUS and TIME, the drawing's facts in the command history; and the
//! file commands that lead to others: ATTACH (by the file's type),
//! EXPORT and DXFOUT (a copy in another format), DXFIN, QNEW and SAVEALL.

use oxidraft_model::{MODEL, snap::SnapKind};
use std::path::Path;

use crate::app::OxidraftApp;

fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

impl OxidraftApp {
    /// STATUS: counts, limits, extents, the current settings and modes.
    pub(crate) fn status(&mut self) {
        let len = |v: f64| oxidraft_model::units::length(v);
        let pt = |p: oxidraft_geom::Point| format!("X: {:>12}  Y: {:>12}", len(p.x), len(p.y));
        let space = if self.doc.space() == MODEL { "Model space" } else { "Paper space" };
        let mut lines = vec![format!("{} objects in {}", self.doc.len(), self.file_name())];
        let (min, max) = self.doc.limits();
        lines.push(format!("{space} limits are   {}", pt(min)));
        lines.push(format!("                     {}", pt(max)));
        match self.doc.extents() {
            Some(b) => {
                lines.push(format!("{space} uses     {}", pt(b.min)));
                lines.push(format!("                     {}", pt(b.max)));
            }
            None => lines.push(format!("{space} uses     *Nothing*")),
        }
        let view = |x: f64| len(x);
        let w = f64::from(self.drawing_rect.width()) / self.view.scale;
        let h = f64::from(self.drawing_rect.height()) / self.view.scale;
        let c = self.view.center;
        lines.push(format!("Display shows        {}", pt(oxidraft_geom::Point::new(c.x - w / 2.0, c.y - h / 2.0))));
        lines.push(format!("                     {}", pt(oxidraft_geom::Point::new(c.x + w / 2.0, c.y + h / 2.0))));
        lines.push(format!("Insertion base is    {}", pt(self.doc.insbase())));
        let snap = self.engine.vars.snap_spacing;
        lines.push(format!("Snap resolution is   X: {:>12}  Y: {:>12}", view(snap), view(snap)));
        let grid = self.grid_spacing.map_or("adaptive".to_string(), view);
        lines.push(format!("Grid spacing is      {grid}"));
        let p = self.doc.current_props();
        lines.push(format!("Current space:       {space}"));
        lines.push(format!("Current layer:       {}", self.doc.current_layer()));
        lines.push(format!("Current color:       {}", oxidraft_commands::current_props::color_text(p.color)));
        lines.push(format!("Current linetype:    {}", p.linetype));
        lines.push(format!("Current lineweight:  {}", p.lineweight.label()));
        lines.push(format!("Current text style:  {}", self.doc.current_text_style().name));
        lines.push(format!(
            "Fill {}  Grid {}  Ortho {}  Qtext {}  Snap {}  Limits check {}",
            on_off(self.doc.fill()),
            on_off(self.grid),
            on_off(self.engine.ortho()),
            on_off(self.doc.quick_text()),
            on_off(self.engine.vars.snap),
            on_off(self.doc.limits_check()),
        ));
        let snaps: Vec<&str> =
            SnapKind::ALL.iter().filter(|k| self.osnap_modes.contains(k)).map(|k| k.label()).collect();
        let snaps = if !self.osnap || snaps.is_empty() { "None".to_string() } else { snaps.join(", ") };
        lines.push(format!("Object snap modes:   {snaps}"));
        for l in lines {
            self.engine.log(l);
        }
    }

    /// TIME: when the drawing was made and last saved, and how long it's
    /// been open.
    pub(crate) fn time(&mut self) {
        let env = self.field_env();
        let when = |m: Option<oxidraft_model::field::Moment>| {
            m.map_or("(not saved yet)".to_string(), |m| {
                oxidraft_model::field::format_date(m, "dddd, MMMM d, yyyy  h:mm:ss tt")
            })
        };
        let elapsed = self.opened_at.elapsed().as_secs();
        let lines = [
            format!("Current time:             {}", when(Some(env.now))),
            format!("Created:                  {}", when(env.created)),
            format!("Last updated:             {}", when(env.saved)),
            format!(
                "Elapsed timer (on):       {} days {:02}:{:02}:{:02}",
                elapsed / 86400,
                elapsed / 3600 % 24,
                elapsed / 60 % 60,
                elapsed % 60
            ),
        ];
        for l in lines {
            self.engine.log(l);
        }
    }

    /// ATTACH: a drawing as an xref, or an image or PDF as an underlay,
    /// by the file picked.
    pub(crate) fn attach(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("All attachable files", &["dwg", "dxf", "pdf", "tif", "tiff", "png", "jpg", "jpeg", "bmp"])
            .add_filter("Drawing (DWG, DXF)", &["dwg", "dxf"])
            .add_filter("PDF", &["pdf"])
            .add_filter("Images", &["tif", "tiff", "png", "jpg", "jpeg", "bmp"]);
        if let Some(path) = dialog.pick_file() {
            self.attach_by_type(path);
        }
    }

    pub fn attach_by_type(&mut self, path: std::path::PathBuf) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext == "dwg" || ext == "dxf" {
            self.attach_file(path);
        } else {
            self.open_image_dialog(path);
        }
    }

    /// EXPORT and DXFOUT: a copy of the drawing in another format (or a
    /// PDF), leaving the drawing's own file as it is.
    pub(crate) fn export(&mut self, dxf_only: bool) {
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut dialog =
            rfd::FileDialog::new().add_filter("DXF drawing", &["dxf"]).set_file_name(format!("{stem}.dxf"));
        if !dxf_only {
            dialog = dialog.add_filter("AutoCAD drawing", &["dwg"]).add_filter("PDF", &["pdf"]);
        }
        if let Some(path) = dialog.save_file() {
            self.export_to(&path);
        }
    }

    pub fn export_to(&mut self, path: &Path) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let written = match ext.as_str() {
            "pdf" => {
                self.open_export_pdf();
                return;
            }
            "dwg" => oxidraft_io_dwg::save_file(&self.doc, path, self.dwg_version.unwrap_or_default()),
            _ => oxidraft_io_dwg::save_dxf(&self.doc, path),
        };
        match written {
            Ok(()) => self.engine.log(format!("Exported {}", path.display())),
            Err(e) => self.engine.log(format!("Could not export {}: {e}", path.display())),
        }
    }
}

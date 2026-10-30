//! STATUS and TIME, the drawing's facts in the command history; and the
//! file commands that lead to others: ATTACH and IMPORT (by the file's
//! type), INSERTOBJ, EXPORT and DXFOUT (a copy in another format), DXFIN,
//! QNEW and SAVEALL.

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

    /// IMPORT: a drawing comes in as a block, a PDF's drawing as objects
    /// (PDFIMPORT), an image attached.
    pub(crate) fn import(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("All importable files", &["dwg", "dxf", "pdf", "tif", "tiff", "png", "jpg", "jpeg", "bmp"])
            .add_filter("Drawing (DWG, DXF)", &["dwg", "dxf"])
            .add_filter("PDF", &["pdf"])
            .add_filter("Images", &["tif", "tiff", "png", "jpg", "jpeg", "bmp"]);
        if let Some(path) = dialog.pick_file() {
            self.import_file(path);
        }
    }

    pub fn import_file(&mut self, path: std::path::PathBuf) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "dwg" | "dxf" => {
                self.library_drawing(&path);
                self.insert_from_library(&path, None);
            }
            "pdf" => self.open_pdf_import(path),
            _ => self.open_image_dialog(path),
        }
    }

    /// INSERTOBJ: an image or PDF page shown as it is; any other file as
    /// a framed object naming it, linked to it (Cmd/Ctrl-click opens it),
    /// as an OLE object shown as an icon.
    pub(crate) fn insert_object(&mut self) {
        if let Some(path) = rfd::FileDialog::new().set_title("Insert Object: create from file").pick_file() {
            self.insert_object_file(&path);
        }
    }

    pub fn insert_object_file(&mut self, path: &Path) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ["pdf", "tif", "tiff", "png", "jpg", "jpeg", "bmp"].contains(&ext.as_str()) {
            return self.open_image_dialog(path.to_path_buf());
        }
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Embedded, as AutoCAD's INSERTOBJ embeds a file: its contents kept
        // in the drawing (a link to it if it can't be read).
        let (url, embedded) = match std::fs::read(path) {
            Ok(bytes) => {
                self.engine.cancel(&mut self.doc);
                let name = self.doc.embed_file(&file, bytes);
                (format!("{}{name}", oxidraft_io_dxf::EMBEDDED_PREFIX), true)
            }
            Err(_) => (path.display().to_string(), false),
        };
        let at = self.view.center;
        // A frame a twelfth of the view across, the file's name in it.
        let w = f64::from(self.drawing_rect.width()) / self.view.scale / 12.0;
        let h = w * 0.6;
        let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
            .map(|(x, y)| oxidraft_model::Vertex::at(at + oxidraft_geom::Point::new(x, y)));
        let text = oxidraft_model::Geometry::Text {
            look: Default::default(),
            position: at + oxidraft_geom::Point::new(w / 2.0, h / 2.0),
            height: h / 6.0,
            rotation: 0.0,
            width: w * 0.9,
            attach: oxidraft_model::Attach::MiddleCenter,
            content: file.clone(),
        };
        let link = oxidraft_model::Hyperlink { url, description: file.clone() };
        self.engine.cancel(&mut self.doc);
        self.doc.begin("INSERTOBJ");
        for g in [oxidraft_model::Geometry::Polyline { vertices: corners.to_vec(), closed: true }, text] {
            let id = self.doc.add(g);
            let link = link.clone();
            self.doc.update(id, |e| e.hyperlink = Some(Box::new(link)));
        }
        self.doc.commit();
        let how = if embedded { "embedded in the drawing" } else { "linked" };
        self.engine.log(format!("{file} inserted, {how}: Cmd/Ctrl-click it to open it."));
    }

    /// A copy of embedded file `name` to open in its own program (in a
    /// folder of the drawing's embedded files), watched: what's saved in
    /// it comes back into the drawing, as AutoCAD's OLE editing does.
    pub fn open_embedded(&mut self, name: &str) -> Option<std::path::PathBuf> {
        let bytes = self.doc.embedded_file(name)?;
        let dir = std::env::temp_dir().join("oxidraft-embedded");
        let file = dir.join(name.replace(['/', '\\'], "_"));
        std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, bytes)).ok()?;
        let written = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        self.embedded_open.retain(|(n, _, _)| n != name);
        self.embedded_open.push((name.to_string(), file.clone(), written));
        Some(file)
    }

    /// Embedded files saved in their programs since they were opened: their
    /// new contents into the drawing (looked at every second or so).
    pub(crate) fn take_embedded_edits(&mut self, ctx: &eframe::egui::Context) {
        if self.embedded_open.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        if self.embedded_checked.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(900)) {
            return;
        }
        self.embedded_checked = Some(std::time::Instant::now());
        let mut changed = Vec::new();
        for (name, file, written) in &mut self.embedded_open {
            let now = std::fs::metadata(&*file).and_then(|m| m.modified()).ok();
            if now.is_some() && now != *written {
                *written = now;
                if let Ok(bytes) = std::fs::read(&*file) {
                    changed.push((name.clone(), bytes));
                }
            }
        }
        for (name, bytes) in changed {
            if self.doc.embedded_file(&name).is_some_and(|b| b != bytes.as_slice())
                && self.doc.update_embedded_file(&name, bytes)
            {
                self.engine.log(format!("{name} was changed: what's in the drawing now is what was saved."));
            }
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

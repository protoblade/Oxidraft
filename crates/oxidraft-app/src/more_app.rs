//! The app's side of smaller AutoCAD LT commands: showing and hiding the
//! ribbon, command line and palettes, the command history (TEXTSCR,
//! COPYHIST, LOGFILEON), ABOUT, TABLEEXPORT's CSV, web addresses,
//! EXPORTLAYOUT, DWGCONVERT, PLOTSTAMP and WHOHAS.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use oxidraft_geom::Point;
use oxidraft_model::{Attach, Color, Document, Geometry, HatchPattern, MODEL, TextLook, Vertex};
use oxidraft_plot::{Item, PlotArea, Sheet};

use crate::app::OxidraftApp;

/// The palettes HIDEPALETTES hid: Properties, Layers, Tool Palettes,
/// DesignCenter, QuickCalc, External References.
#[derive(Clone, Copy, Default)]
pub(crate) struct HiddenPalettes {
    properties: bool,
    layers: bool,
    tool_palettes: bool,
    design_center: bool,
    quickcalc: bool,
    xrefs: bool,
}

/// A table's cells as CSV: each cell quoted when it needs to be.
pub fn table_csv(cells: &[Vec<String>]) -> String {
    let field = |s: &str| {
        let plain = oxidraft_model::annotation::plain_text(s).replace('\n', " ");
        if plain.contains([',', '"', '\n']) { format!("\"{}\"", plain.replace('"', "\"\"")) } else { plain }
    };
    cells.iter().map(|row| row.iter().map(|c| field(c)).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join("\n")
        + "\n"
}

/// A plotted sheet as drawing objects in millimeters: its strokes as
/// polylines, fills as solid hatches, text as text (EXPORTLAYOUT).
pub fn sheet_objects(sheet: &Sheet) -> Vec<(Geometry, Color)> {
    fn walk(items: &[Item], out: &mut Vec<(Geometry, Color)>) {
        let color = |rgb: [u8; 3]| Color::Index(crate::pdf_import::nearest_aci(rgb));
        for item in items {
            match item {
                Item::Stroke { paths, color: c, .. } => {
                    for p in paths.iter().filter(|p| p.points.len() >= 2) {
                        let g = if p.points.len() == 2 && !p.closed {
                            Geometry::Line { start: p.points[0], end: p.points[1] }
                        } else {
                            Geometry::Polyline {
                                vertices: p.points.iter().map(|q| Vertex::at(*q)).collect(),
                                closed: p.closed,
                            }
                        };
                        out.push((g, color(*c)));
                    }
                }
                Item::Fill { polygons, color: c } => {
                    let loops = polygons.iter().map(|p| p.iter().map(|q| Vertex::at(*q)).collect()).collect();
                    let g = Geometry::Hatch {
                        loops,
                        pattern: HatchPattern::Solid,
                        scale: 1.0,
                        angle: 0.0,
                        origin: Point::ORIGIN,
                    };
                    out.push((g, color(*c)));
                }
                Item::Text { at, rotation, size, text, color: c, width_factor, oblique, .. } => {
                    let look = TextLook { width_factor: *width_factor, oblique: *oblique, ..Default::default() };
                    let g = Geometry::Text {
                        position: *at,
                        height: size * 0.7,
                        rotation: *rotation,
                        width: 0.0,
                        attach: Attach::BottomLeft,
                        content: text.clone(),
                        look,
                    };
                    out.push((g, color(*c)));
                }
                Item::Clipped { items, .. } => walk(items, out),
                Item::Image { .. } => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&sheet.items, &mut out);
    out
}

/// Opens `url` in the system's browser (or file in its program).
pub(crate) fn open_url(url: &str) -> std::io::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(url).spawn().map(|_| ())
}

impl OxidraftApp {
    /// HIDEPALETTES and SHOWPALETTES.
    pub(crate) fn hide_palettes(&mut self, hide: bool) {
        if hide {
            self.hidden_palettes = Some(HiddenPalettes {
                properties: std::mem::take(&mut self.properties_open),
                layers: std::mem::take(&mut self.layer_manager_open),
                tool_palettes: self.tool_palettes.take().is_some(),
                design_center: self.design_center.take().is_some(),
                quickcalc: self.quickcalc.take().is_some(),
                xrefs: std::mem::take(&mut self.xref_palette_open),
            });
            return;
        }
        let Some(h) = self.hidden_palettes.take() else { return };
        self.properties_open |= h.properties;
        self.layer_manager_open |= h.layers;
        self.xref_palette_open |= h.xrefs;
        if h.tool_palettes && self.tool_palettes.is_none() {
            self.toggle_tool_palettes();
        }
        if h.design_center && self.design_center.is_none() {
            self.toggle_design_center();
        }
        if h.quickcalc && self.quickcalc.is_none() {
            self.toggle_quickcalc();
        }
    }

    /// LOGFILEON: the history from now on, appended to Oxidraft.log beside
    /// the drawing (or in the home folder).
    pub(crate) fn log_file_on(&mut self) {
        let dir = self
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let path = dir.join("Oxidraft.log");
        self.engine.log(format!("Command history is written to {}", path.display()));
        self.log_file = Some((path, self.engine.history().len()));
    }

    /// Appends the history written since last time to the log file.
    pub(crate) fn write_log(&mut self) {
        let Some((path, written)) = &mut self.log_file else { return };
        let history = self.engine.history();
        if history.len() <= *written {
            return;
        }
        let lines = history[*written..].join("\n");
        *written = history.len();
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&*path);
        if let Err(e) = file.and_then(|mut f| writeln!(f, "{lines}")) {
            self.log_file = None;
            self.engine.log(format!("Could not write the log file: {e}"));
        }
    }

    /// ABOUT.
    pub(crate) fn about(&mut self) {
        self.engine.log(format!(
            "Oxidraft {}: a 2D drafting program that works like AutoCAD LT. Free and open source (MIT or Apache-2.0): https://github.com/protoblade/Oxidraft",
            env!("CARGO_PKG_VERSION")
        ));
    }

    /// TABLEEXPORT: the table's cells to a CSV file picked.
    pub(crate) fn table_export(&mut self, id: oxidraft_model::EntityId) {
        let Some(Geometry::Table { cells, .. }) = self.doc.entity(id).map(|e| e.geometry.clone()) else { return };
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("Table.csv").save_file()
        else {
            return;
        };
        match std::fs::write(&path, table_csv(&cells)) {
            Ok(()) => self.engine.log(format!("Table written to {}", path.display())),
            Err(e) => self.engine.log(format!("Could not write {}: {e}", path.display())),
        }
    }

    /// EXPORTLAYOUT: asks where the drawing of the layout shown goes.
    pub(crate) fn export_layout(&mut self) {
        let space = self.shown_space();
        let Some(layout) = self.doc.layout(space).map(|l| l.name.clone()) else {
            self.engine.log("EXPORTLAYOUT works on a layout tab.");
            return;
        };
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AutoCAD drawing", &["dwg"])
            .add_filter("DXF drawing", &["dxf"])
            .set_file_name(format!("{stem}_{layout}.dwg"))
            .save_file()
        else {
            return;
        };
        match self.export_layout_to(&path) {
            Ok(n) => self.engine.log(format!("Layout \"{layout}\" saved as {} ({n} objects)", path.display())),
            Err(e) => self.engine.log(e),
        }
    }

    /// The layout shown as a drawing at `path`: what its sheet plots, in
    /// model space, in millimeters; how many objects.
    pub fn export_layout_to(&mut self, path: &Path) -> Result<usize, String> {
        let space = self.shown_space();
        if space == MODEL {
            return Err("EXPORTLAYOUT works on a layout tab.".into());
        }
        let settings = oxidraft_plot::PlotSettings { area: PlotArea::Layout, ..self.plot_settings };
        let sheet = oxidraft_plot::render(&self.doc, space, &settings).ok_or("The layout has nothing to export.")?;
        let mut doc = Document::new();
        let objects = sheet_objects(&sheet);
        let n = objects.len();
        for (g, color) in objects {
            doc.add_on_layer(g, "0", color);
        }
        let written = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dxf")) {
            oxidraft_io_dwg::save_dxf(&doc, path)
        } else {
            oxidraft_io_dwg::save_file(&doc, path, self.dwg_version.unwrap_or_default())
        };
        written.map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(n)
    }

    /// DWGCONVERT: drawings picked, saved again in the version chosen
    /// (as typed: R2000 to R2018), beside themselves.
    pub(crate) fn dwg_convert(&mut self) {
        let files = rfd::FileDialog::new().add_filter("Drawing", &["dwg", "dxf"]).pick_files().unwrap_or_default();
        if files.is_empty() {
            return;
        }
        let version = self.dwg_version.unwrap_or_default();
        let (mut done, mut failed) = (0, Vec::new());
        for file in files {
            match self.convert_drawing(&file, version) {
                Ok(_) => done += 1,
                Err(e) => failed.push(e),
            }
        }
        self.engine.log(format!("{done} drawing(s) converted to {version:?}."));
        for e in failed {
            self.engine.log(e);
        }
    }

    /// `file` read and written as a DWG of `version`, next to it ("_converted"
    /// when it was a DWG already); where.
    pub fn convert_drawing(&mut self, file: &Path, version: oxidraft_io_dwg::Version) -> Result<PathBuf, String> {
        let doc = crate::xref::read_drawing(file)?;
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let is_dwg = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg"));
        let to = file.with_file_name(if is_dwg { format!("{stem}_converted.dwg") } else { format!("{stem}.dwg") });
        oxidraft_io_dwg::save_file(&doc, &to, version).map_err(|e| format!("Could not write {}: {e}", to.display()))?;
        Ok(to)
    }

    /// PLOTSTAMP: plots stamped with the drawing's name, the layout and
    /// the date, or not.
    pub(crate) fn toggle_plot_stamp(&mut self) {
        self.plot_stamp = !self.plot_stamp;
        self.log_toggle("Plot stamp", self.plot_stamp);
    }

    /// The stamp a plot gets in its lower left corner, when PLOTSTAMP is on.
    pub(crate) fn stamp(&self, sheet: &mut Sheet, space: oxidraft_model::Space) {
        if !self.plot_stamp {
            return;
        }
        let layout = self.doc.layout(space).map_or("Model".into(), |l| l.name.clone());
        let date = chrono::Local::now().format("%Y-%m-%d %H:%M");
        let text =
            format!("{}, {layout}, {date}", self.path.as_ref().map_or(self.file_name(), |p| p.display().to_string()));
        let p = sheet.layout.printable.min + Point::new(1.0, 1.0);
        sheet.items.push(Item::Text {
            at: p,
            rotation: 0.0,
            size: 2.5,
            text,
            color: [0, 0, 0],
            width_factor: 1.0,
            oblique: 0.0,
            font: String::new(),
        });
    }

    /// WHOHAS: asks for a drawing, and says when it was saved and whether
    /// this window has it open.
    pub(crate) fn who_has(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("Drawing", &["dwg", "dxf"]).pick_file() else { return };
        let meta = match std::fs::metadata(&file) {
            Ok(m) => m,
            Err(e) => return self.engine.log(format!("{}: {e}", file.display())),
        };
        let saved = meta
            .modified()
            .ok()
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let open_here = self.path.as_deref() == Some(file.as_path());
        self.engine.log(format!(
            "{}: last saved {saved}, {} bytes; {}",
            file.display(),
            meta.len(),
            if open_here { "open here" } else { "not open in this window" }
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_what_it_must() {
        let cells = vec![vec!["Door".to_string(), "2,5 m".to_string()], vec!["A \"big\" one".into(), "x".into()]];
        assert_eq!(table_csv(&cells), "Door,\"2,5 m\"\n\"A \"\"big\"\" one\",x\n");
    }
}

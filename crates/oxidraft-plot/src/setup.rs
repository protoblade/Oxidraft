//! What to plot and how: the paper, the part of the drawing, the scale,
//! the colors and lineweights (AutoCAD's Plot dialog), and where that puts
//! the drawing on the paper.

use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Document, Space};

/// A sheet size, in millimeters, portrait.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paper {
    pub name: &'static str,
    pub width: f64,
    pub height: f64,
}

const fn paper(name: &'static str, width: f64, height: f64) -> Paper {
    Paper { name, width, height }
}

/// The sheets the PDF plotter offers.
pub const PAPERS: &[Paper] = &[
    paper("ISO A4 (210.00 x 297.00 MM)", 210.0, 297.0),
    paper("ISO A3 (297.00 x 420.00 MM)", 297.0, 420.0),
    paper("ISO A2 (420.00 x 594.00 MM)", 420.0, 594.0),
    paper("ISO A1 (594.00 x 841.00 MM)", 594.0, 841.0),
    paper("ISO A0 (841.00 x 1189.00 MM)", 841.0, 1189.0),
    paper("ANSI A (8.50 x 11.00 Inches)", 215.9, 279.4),
    paper("ANSI B (11.00 x 17.00 Inches)", 279.4, 431.8),
    paper("ANSI C (17.00 x 22.00 Inches)", 431.8, 558.8),
    paper("ANSI D (22.00 x 34.00 Inches)", 558.8, 863.6),
    paper("ANSI E (34.00 x 44.00 Inches)", 863.6, 1117.6),
];

/// Paper sizes of your own (PLOTTERMANAGER), after the standard ones.
static CUSTOM: std::sync::Mutex<Vec<Paper>> = std::sync::Mutex::new(Vec::new());

/// Sets the paper sizes of your own: (name, width, height in millimeters).
pub fn set_custom_papers(sizes: &[(String, f64, f64)]) {
    let papers = sizes
        .iter()
        .filter(|(_, w, h)| *w > 0.0 && *h > 0.0)
        .map(|(name, w, h)| {
            // Names live as long as the papers do: kept once each.
            let known = CUSTOM.lock().ok().and_then(|c| c.iter().find(|p| p.name == name).map(|p| p.name));
            let name: &'static str = known.unwrap_or_else(|| Box::leak(name.clone().into_boxed_str()));
            Paper { name, width: *w, height: *h }
        })
        .collect();
    if let Ok(mut c) = CUSTOM.lock() {
        *c = papers;
    }
}

/// Every paper size the plotters offer: the standard ones, then yours.
pub fn papers() -> Vec<Paper> {
    let custom = CUSTOM.lock().map(|c| c.clone()).unwrap_or_default();
    PAPERS.iter().copied().chain(custom).collect()
}

/// The computer's printers, each name kept for the rest of the run so a
/// [`Plotter::Printer`] stays a number.
static PRINTERS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

/// The computer's printers, as its print queue names them.
pub fn printers() -> Vec<&'static str> {
    PRINTERS.lock().map(|p| p.clone()).unwrap_or_default()
}

/// Adds the printers the system lists (ones already known keep their number).
pub fn set_printers(names: &[String]) {
    if let Ok(mut p) = PRINTERS.lock() {
        for n in names {
            if !p.iter().any(|k| k == n) {
                p.push(Box::leak(n.clone().into_boxed_str()));
            }
        }
    }
}

/// Where a plot goes: AutoCAD's plotter configurations for files, or one
/// of the computer's printers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Plotter {
    #[default]
    Pdf,
    Dwfx,
    Dwf6,
    Png,
    Jpg,
    /// A printer, by its number in [`printers`].
    Printer(u16),
}

impl Plotter {
    pub const ALL: [Plotter; 5] = [Plotter::Pdf, Plotter::Dwfx, Plotter::Dwf6, Plotter::Png, Plotter::Jpg];

    /// Its configuration's name, as AutoCAD's.
    pub fn name(self) -> &'static str {
        match self {
            Plotter::Pdf => "DWG To PDF.pc3",
            Plotter::Dwfx => "DWFx ePlot (XPS Compatible).pc3",
            Plotter::Dwf6 => "DWF6 ePlot.pc3",
            Plotter::Png => "PublishToWeb PNG.pc3",
            Plotter::Jpg => "PublishToWeb JPG.pc3",
            Plotter::Printer(i) => printers().get(i as usize).copied().unwrap_or("Printer"),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Plotter::Pdf | Plotter::Printer(_) => "pdf",
            Plotter::Dwfx => "dwfx",
            Plotter::Dwf6 => "dwf",
            Plotter::Png => "png",
            Plotter::Jpg => "jpg",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Plotter::Pdf => "Writes a PDF file: lines as vectors, text you can search.",
            Plotter::Dwfx => "Writes a DWFx file (XPS compatible): vectors and text, for DWF and XPS viewers.",
            Plotter::Dwf6 => "Writes a DWF 6 file: its sheets' vectors, fills, text and pictures, for DWF viewers.",
            Plotter::Png => "Writes a PNG image of the sheet.",
            Plotter::Jpg => "Writes a JPEG image of the sheet.",
            Plotter::Printer(_) => "Prints on this printer, through the computer's print queue.",
        }
    }

    /// File plotters, then the computer's printers.
    pub fn all() -> Vec<Plotter> {
        Self::ALL.into_iter().chain((0..printers().len()).map(|i| Plotter::Printer(i as u16))).collect()
    }

    /// Whether it writes pixels rather than vectors.
    pub fn is_raster(self) -> bool {
        matches!(self, Plotter::Png | Plotter::Jpg)
    }
}

/// The border round the sheet that isn't plotted on, in millimeters.
pub const MARGIN: f64 = 5.0;

/// What to plot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlotArea {
    /// What the drawing area shows.
    Display(Aabb),
    /// Everything in the drawing.
    Extents,
    /// A window picked in the drawing.
    Window(Aabb),
    /// A layout's sheet as its page setup has it: on its own paper, at 1:1.
    Layout,
}

/// How drawing units map to paper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlotScale {
    /// As large as fits the printable area.
    Fit,
    /// `mm` millimeters on paper show `units` drawing units.
    Custom { mm: f64, units: f64 },
}

/// The standard scales the dialog lists, as (mm, units).
pub const SCALES: &[(f64, f64)] = &[
    (1.0, 1.0),
    (1.0, 2.0),
    (1.0, 5.0),
    (1.0, 10.0),
    (1.0, 20.0),
    (1.0, 25.0),
    (1.0, 50.0),
    (1.0, 100.0),
    (1.0, 200.0),
    (1.0, 250.0),
    (1.0, 500.0),
    (1.0, 1000.0),
    (1.0, 2000.0),
    (1.0, 5000.0),
    (2.0, 1.0),
    (5.0, 1.0),
    (10.0, 1.0),
];

/// Pen assignments: plot in the objects' colors, in black, in grays, or
/// as a plot style table (.ctb) says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlotStyle {
    None,
    Monochrome,
    Grayscale,
    /// A plot style table the app found, by its place among them
    /// ([`crate::ctb::tables`]).
    Table(u16),
}

fn gray([r, g, b]: [u8; 3]) -> u8 {
    (0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)).round() as u8
}

impl PlotStyle {
    pub const ALL: [PlotStyle; 3] = [PlotStyle::None, PlotStyle::Monochrome, PlotStyle::Grayscale];

    /// The built-in ones and the tables found.
    pub fn all() -> Vec<PlotStyle> {
        let tables = crate::ctb::tables().len().min(usize::from(u16::MAX)) as u16;
        PlotStyle::ALL.into_iter().chain((0..tables).map(PlotStyle::Table)).collect()
    }

    /// The name AutoCAD gives the plot style table.
    pub fn name(self) -> String {
        match self {
            PlotStyle::None => "None".into(),
            PlotStyle::Monochrome => "monochrome.ctb".into(),
            PlotStyle::Grayscale => "grayscale.ctb".into(),
            PlotStyle::Table(i) => crate::ctb::with_table(usize::from(i), |t| t.name.clone()).unwrap_or_default(),
        }
    }

    /// The color an AutoCAD Color Index plots in. White (7) plots black,
    /// as on paper it would vanish.
    pub fn rgb(self, aci: u8) -> [u8; 3] {
        let (r, g, b) = if aci == 7 { (0, 0, 0) } else { oxidraft_model::aci_rgb(aci) };
        match self {
            PlotStyle::None => [r, g, b],
            PlotStyle::Monochrome => [0, 0, 0],
            PlotStyle::Grayscale => {
                // Light colors would wash out: keep grays at most 80% light.
                let y = gray([r, g, b]).min(204);
                [y, y, y]
            }
            PlotStyle::Table(i) => crate::ctb::with_table(usize::from(i), |t| {
                let pen = t.pen(aci);
                let mut c = pen.color.unwrap_or([r, g, b]);
                if pen.grayscale {
                    c = [gray(c); 3];
                }
                // Screening: that much of the color, the rest white.
                let k = f64::from(pen.screen.min(100)) / 100.0;
                c.map(|v| (f64::from(v) * k + 255.0 * (1.0 - k)).round() as u8)
            })
            .unwrap_or([r, g, b]),
        }
    }

    /// The lineweight a table gives color `aci`, in millimeters.
    pub fn lineweight(self, aci: u8) -> Option<f64> {
        match self {
            PlotStyle::Table(i) => crate::ctb::with_table(usize::from(i), |t| t.pen(aci).lineweight).flatten(),
            _ => None,
        }
    }
}

/// Everything the Plot dialog sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotSettings {
    pub paper: Paper,
    pub landscape: bool,
    pub area: PlotArea,
    pub scale: PlotScale,
    /// Center the plot in the printable area, else place it `offset` from
    /// the printable area's lower-left corner (millimeters).
    pub center: bool,
    pub offset: Point,
    pub style: PlotStyle,
    /// Plot object lineweights; off, every line plots as thin as it can.
    pub lineweights: bool,
    pub plotter: Plotter,
}

impl Default for PlotSettings {
    fn default() -> Self {
        Self {
            plotter: Plotter::Pdf,
            paper: PAPERS[0],
            landscape: true,
            area: PlotArea::Extents,
            scale: PlotScale::Fit,
            center: true,
            offset: Point::ORIGIN,
            style: PlotStyle::None,
            lineweights: true,
        }
    }
}

/// Where the plotted part of the drawing lands on the sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The sheet, millimeters, as turned for the orientation.
    pub width: f64,
    pub height: f64,
    /// The part of the drawing plotted.
    pub window: Aabb,
    /// Millimeters per drawing unit.
    pub scale: f64,
    /// Where the window's lower-left corner lands on the sheet.
    pub origin: Point,
    /// What's plotted: the window on the sheet, cut to the printable area.
    pub clip: Aabb,
    /// The sheet's printable area.
    pub printable: Aabb,
}

impl Layout {
    pub fn to_paper(&self, p: Point) -> Point {
        self.origin + (p - self.window.min) * self.scale
    }
}

impl PlotSettings {
    /// The sheet's width and height in millimeters, turned for the orientation.
    pub fn sheet(&self) -> (f64, f64) {
        let (w, h) = (self.paper.width, self.paper.height);
        if self.landscape { (w.max(h), w.min(h)) } else { (w.min(h), w.max(h)) }
    }

    /// The part of `space` (model space or a layout) to plot, or `None`
    /// when there's nothing to.
    pub fn window(&self, doc: &Document, space: Space) -> Option<Aabb> {
        let w = match self.area {
            PlotArea::Display(b) | PlotArea::Window(b) => b,
            PlotArea::Extents => doc.space_extents(space)?,
            PlotArea::Layout => doc.layout(space)?.paper.sheet_bounds().0,
        };
        (w.width() > 0.0 || w.height() > 0.0).then_some(w)
    }

    /// Where the plotted part of `space` lands on the sheet, or `None` when
    /// there's nothing to plot.
    pub fn layout(&self, doc: &Document, space: Space) -> Option<Layout> {
        if self.area == PlotArea::Layout {
            return Self::sheet_layout(doc, space);
        }
        let window = self.window(doc, space)?;
        let (width, height) = self.sheet();
        let printable = Aabb::from_points(Point::new(MARGIN, MARGIN), Point::new(width - MARGIN, height - MARGIN));
        let scale = match self.scale {
            PlotScale::Fit => {
                let sx = if window.width() > 0.0 { printable.width() / window.width() } else { f64::INFINITY };
                let sy = if window.height() > 0.0 { printable.height() / window.height() } else { f64::INFINITY };
                sx.min(sy)
            }
            PlotScale::Custom { mm, units } if mm > 0.0 && units > 0.0 => mm / units,
            PlotScale::Custom { .. } => return None,
        };
        let size = Point::new(window.width(), window.height()) * scale;
        let origin = if self.center { printable.center() - size * 0.5 } else { printable.min + self.offset };
        let placed = Aabb::from_points(origin, origin + size);
        let clip = Aabb::from_points(
            Point::new(placed.min.x.max(printable.min.x), placed.min.y.max(printable.min.y)),
            Point::new(placed.max.x.min(printable.max.x), placed.max.y.min(printable.max.y)),
        );
        Some(Layout { width, height, window, scale, origin, clip, printable })
    }

    /// A layout's sheet at 1:1 on its own paper, cut to its printable area.
    fn sheet_layout(doc: &Document, space: Space) -> Option<Layout> {
        let paper = &doc.layout(space)?.paper;
        let (sheet, printable) = paper.sheet_bounds();
        // Millimeters per paper unit.
        let scale = if paper.millimeters { 1.0 } else { 25.4 };
        let on_paper = |p: Point| (p - sheet.min) * scale;
        let printable = Aabb::from_points(on_paper(printable.min), on_paper(printable.max));
        Some(Layout {
            width: sheet.width() * scale,
            height: sheet.height() * scale,
            window: sheet,
            scale,
            origin: Point::ORIGIN,
            clip: printable,
            printable,
        })
    }

    /// The paper's name for the dialog: the layout's own for a layout plot.
    pub fn paper_name(&self, doc: &Document, space: Space) -> String {
        match (self.area, doc.layout(space)) {
            (PlotArea::Layout, Some(l)) => {
                let sheet = l.paper.sheet_bounds().0;
                let (w, h) = (sheet.width(), sheet.height());
                let units = if l.paper.millimeters { "MM" } else { "Inches" };
                let name = l.paper.size_name.replace('_', " ");
                if name.trim().is_empty() { format!("{w:.2} x {h:.2} {units}") } else { name }
            }
            _ => self.paper.name.to_string(),
        }
    }
}

/// A scale as AutoCAD shows it: "1:100", "2:1", or "1 mm = 0.3378 units".
pub fn scale_label(mm: f64, units: f64) -> String {
    let whole = |v: f64| (v - v.round()).abs() < 1e-9 && v >= 1.0;
    if whole(mm) && whole(units) && (mm == 1.0 || units == 1.0) {
        format!("{}:{}", mm.round(), units.round())
    } else {
        format!("1 mm = {:.4} units", units / mm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_model::Geometry;

    #[test]
    fn printers_follow_the_file_plotters() {
        set_printers(&["Test Office Printer".to_string()]);
        set_printers(&["Test Office Printer".to_string()]);
        let all = Plotter::all();
        let printer = all.iter().find(|p| p.name() == "Test Office Printer").copied().unwrap();
        assert!(matches!(printer, Plotter::Printer(_)));
        assert_eq!(all.iter().filter(|p| p.name() == "Test Office Printer").count(), 1, "listed once");
        assert_eq!(&all[..Plotter::ALL.len()], &Plotter::ALL);
        assert_eq!(printer.extension(), "pdf");
        assert!(!printer.is_raster());
    }

    fn doc() -> Document {
        let mut d = Document::new();
        d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 50.0) });
        d
    }

    #[test]
    fn a_layout_plots_on_its_own_paper_at_one_to_one() {
        let mut d = doc();
        let paper = oxidraft_model::Paper {
            width: 420.0,
            height: 297.0,
            margins: [5.0, 10.0, 5.0, 10.0],
            millimeters: true,
            ..Default::default()
        };
        let space = d.add_layout(oxidraft_model::layout::Layout { id: 0, name: "A3".into(), tab_order: 1, paper });
        let s = PlotSettings { area: PlotArea::Layout, ..PlotSettings::default() };
        let l = s.layout(&d, space).unwrap();
        assert_eq!((l.width, l.height, l.scale), (420.0, 297.0, 1.0));
        // The printable area's corner is the layout's origin.
        assert!(l.to_paper(Point::ORIGIN).approx_eq(Point::new(5.0, 10.0)));
        assert_eq!(l.clip, Aabb::from_points(Point::new(5.0, 10.0), Point::new(415.0, 287.0)));
        assert!(s.layout(&d, oxidraft_model::MODEL).is_none(), "model space has no sheet");
    }

    #[test]
    fn fit_to_paper_centers_the_drawing() {
        let s = PlotSettings::default();
        let l = s.layout(&doc(), oxidraft_model::MODEL).unwrap();
        // A4 landscape: 287 x 200 printable; 100 x 50 fits at 2.87 mm per unit.
        assert_eq!((l.width, l.height), (297.0, 210.0));
        assert!((l.scale - 2.87).abs() < 1e-12);
        let c = l.to_paper(Point::new(50.0, 25.0));
        assert!(c.approx_eq(Point::new(148.5, 105.0)));
        assert!((l.clip.width() - 287.0).abs() < 1e-9 && (l.clip.height() - 143.5).abs() < 1e-9);
    }

    #[test]
    fn a_set_scale_from_the_corner_is_cut_to_the_paper() {
        let s = PlotSettings {
            scale: PlotScale::Custom { mm: 1.0, units: 0.25 },
            center: false,
            offset: Point::new(10.0, 0.0),
            landscape: false,
            ..PlotSettings::default()
        };
        let l = s.layout(&doc(), oxidraft_model::MODEL).unwrap();
        assert_eq!(l.scale, 4.0);
        assert!(l.to_paper(Point::ORIGIN).approx_eq(Point::new(15.0, 5.0)));
        // 400 mm wide doesn't fit on portrait A4: plotted up to the margin.
        assert!((l.clip.max.x - 205.0).abs() < 1e-9);
        assert!(
            PlotSettings { area: PlotArea::Extents, ..s }.layout(&Document::new(), oxidraft_model::MODEL).is_none()
        );
    }

    #[test]
    fn scales_and_colors_read_like_autocad() {
        assert_eq!(scale_label(1.0, 100.0), "1:100");
        assert_eq!(scale_label(2.0, 1.0), "2:1");
        assert_eq!(scale_label(1.0, 0.3378), "1 mm = 0.3378 units");
        assert_eq!(PlotStyle::None.rgb(7), [0, 0, 0]);
        assert_eq!(PlotStyle::None.rgb(1), [255, 0, 0]);
        assert_eq!(PlotStyle::Monochrome.rgb(3), [0, 0, 0]);
        assert_eq!(PlotStyle::Grayscale.rgb(1), [76, 76, 76]);
    }
}

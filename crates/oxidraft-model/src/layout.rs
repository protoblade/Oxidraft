//! Layouts (paper space): sheets with their plot settings, drawn on in
//! paper units, with viewports onto model space. A drawing's objects each
//! belong to one space: model space, or one layout.

/// Which space an object is in: [`MODEL`], or a layout's [`Layout::id`].
pub type Space = u32;

/// Model space.
pub const MODEL: Space = 0;

/// A layout: a sheet in paper space.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// What its objects' space is; never [`MODEL`].
    pub id: Space,
    pub name: String,
    /// Its place among the layout tabs.
    pub tab_order: i32,
    pub paper: Paper,
}

/// A layout's sheet, as its page setup describes it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Paper {
    /// AutoCAD's name for the paper size ("ISO_A3_(420.00_x_297.00_MM)").
    pub size_name: String,
    /// The sheet's size in millimeters as the paper size gives it (before
    /// `rotation`), and the printable margins (left, bottom, right, top).
    pub width: f64,
    pub height: f64,
    pub margins: [f64; 4],
    /// Quarter turns counterclockwise the sheet is plotted at.
    pub rotation: u8,
    /// True if the layout is drawn in millimeters, false for inches.
    pub millimeters: bool,
    /// The page setup as the file had it (AcDbPlotSettings' group codes),
    /// written back unchanged.
    pub raw: Vec<(i32, String)>,
}

impl Paper {
    /// The sheet and its printable area in paper space. As in AutoCAD, the
    /// origin is the printable area's lower-left corner. An unknown paper
    /// size is A4 landscape.
    pub fn sheet_bounds(&self) -> (oxidraft_geom::Aabb, oxidraft_geom::Aabb) {
        use oxidraft_geom::{Aabb, Point};
        let (w, h) = match self.sheet() {
            (w, h) if w > 0.0 && h > 0.0 => (w, h),
            _ => (297.0, 210.0),
        };
        let scale = if self.millimeters { 1.0 } else { 1.0 / 25.4 };
        let m = self.margins.map(|v| v * scale);
        // Margins (left, bottom, right, top) turn with the sheet.
        let [l, b, r, t] = match self.rotation % 4 {
            1 => [m[1], m[2], m[3], m[0]],
            2 => [m[2], m[3], m[0], m[1]],
            3 => [m[3], m[0], m[1], m[2]],
            _ => m,
        };
        let sheet = Aabb::from_points(Point::new(-l, -b), Point::new(w - l, h - b));
        let printable = Aabb::from_points(Point::ORIGIN, Point::new((w - l - r).max(0.0), (h - b - t).max(0.0)));
        (sheet, printable)
    }

    /// The sheet as it lies on screen: width and height in paper units,
    /// the rotation applied.
    pub fn sheet(&self) -> (f64, f64) {
        let scale = if self.millimeters { 1.0 } else { 1.0 / 25.4 };
        let (w, h) = (self.width * scale, self.height * scale);
        if self.rotation % 2 == 1 { (h, w) } else { (w, h) }
    }
}

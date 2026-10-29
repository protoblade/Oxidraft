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
    /// The sheet as it lies on screen: width and height in paper units,
    /// the rotation applied.
    pub fn sheet(&self) -> (f64, f64) {
        let scale = if self.millimeters { 1.0 } else { 1.0 / 25.4 };
        let (w, h) = (self.width * scale, self.height * scale);
        if self.rotation % 2 == 1 { (h, w) } else { (w, h) }
    }
}

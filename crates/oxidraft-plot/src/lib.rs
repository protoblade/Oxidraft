//! Plotting: the page setup AutoCAD's Plot dialog makes, the drawing laid
//! out on the sheet as strokes, fills and text, and that sheet as PDF.

pub mod ctb;
pub mod dwfx;
pub mod font;
pub mod pdf;
pub mod setup;
pub mod sheet;
pub mod shx;
pub mod slide;
pub mod wmf;
pub mod zip;

pub use pdf::{pdf, pdf_pages};
pub use setup::{
    Layout, MARGIN, PAPERS, Paper, PlotArea, PlotScale, PlotSettings, PlotStyle, Plotter, SCALES, papers, scale_label,
    set_custom_papers,
};
pub use sheet::{Item, Path, Picture, Rgb, Sheet, render, render_with_pictures};

//! Plotting: the page setup AutoCAD's Plot dialog makes, the drawing laid
//! out on the sheet as strokes, fills and text, and that sheet as PDF.

pub mod font;
pub mod pdf;
pub mod setup;
pub mod sheet;

pub use pdf::pdf;
pub use setup::{Layout, MARGIN, PAPERS, Paper, PlotArea, PlotScale, PlotSettings, PlotStyle, SCALES, scale_label};
pub use sheet::{Item, Path, Rgb, Sheet, render};

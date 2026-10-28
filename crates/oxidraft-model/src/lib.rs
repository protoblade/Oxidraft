//! The drawing document: entities, layers, and an undo history built from
//! transactions. Every edit goes through [`Document`] so UNDO works for every
//! command without the command knowing about it.

pub mod annotation;
pub mod boundary;
mod document;
mod entity;
pub mod hatch;
pub mod linetype;
mod palette;
pub mod snap;

pub use annotation::Attach;
pub use document::{AttributeDisplay, Block, BlockEntity, Document, Layer, Styled, check_name};
pub use entity::{Attribute, Color, Entity, EntityId, FAR, Geometry, Placement, Vertex, constant_width};
pub use hatch::HatchPattern;
pub use linetype::{Linetype, Lineweight};
pub use oxidraft_geom::{ellipse::Ellipse, spline::Spline};
pub use palette::aci_rgb;

//! The drawing document: entities, layers, and an undo history built from
//! transactions. Every edit goes through [`Document`] so UNDO works for every
//! command without the command knowing about it.

pub mod annotation;
pub mod boundary;
mod document;
mod entity;
pub mod hatch;
pub mod snap;

pub use annotation::Attach;
pub use document::{Block, BlockEntity, Document, Layer, check_name};
pub use entity::{Attribute, Color, Entity, EntityId, Geometry, Placement, Vertex};
pub use hatch::HatchPattern;

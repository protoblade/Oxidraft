//! The drawing document: entities, layers, and an undo history built from
//! transactions. Every edit goes through [`Document`] so UNDO works for every
//! command without the command knowing about it.

mod document;
mod entity;
pub mod snap;

pub use document::{Block, BlockEntity, Document, Layer, check_name};
pub use entity::{Color, Entity, EntityId, Geometry, Placement, Vertex};

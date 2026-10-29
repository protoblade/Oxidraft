//! The drawing document: entities, layers, and an undo history built from
//! transactions. Every edit goes through [`Document`] so UNDO works for every
//! command without the command knowing about it.

pub mod annotation;
pub mod boundary;
mod clip;
mod document;
mod entity;
pub mod hatch;
pub mod linetype;
mod palette;
mod refedit;
pub mod snap;
mod underlay;
mod xref;

pub use annotation::Attach;
pub use clip::clip_to_convex;
pub use document::{AttributeDisplay, Block, BlockEntity, Document, Layer, Styled, check_name, sanitize_name};
pub use entity::{Attribute, Color, Entity, EntityId, FAR, Geometry, Placement, Vertex, XClip, constant_width};
pub use hatch::HatchPattern;
pub use linetype::{Linetype, Lineweight};
pub use oxidraft_geom::{ellipse::Ellipse, spline::Spline};
pub use palette::aci_rgb;
pub use refedit::RefEdit;
pub use underlay::{ImageDef, image_corners};
pub use xref::{Xref, XrefStatus, dependent_name, is_dependent};

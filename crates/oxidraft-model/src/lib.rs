//! The drawing document: entities, layers, and an undo history built from
//! transactions. Every edit goes through [`Document`] so UNDO works for every
//! command without the command knowing about it.

pub mod annoscale;
pub mod annotation;
pub mod boundary;
mod clip;
mod document;
mod entity;
pub mod field;
pub mod hatch;
pub mod layout;
mod library;
pub mod linetype;
mod palette;
mod purge;
mod refedit;
pub mod snap;
pub mod table;
pub mod ucs;
mod underlay;
pub mod units;
mod xref;

pub use annotation::{Attach, TextLook, TextStyle};
pub use clip::clip_to_convex;
pub use document::{
    AttributeDisplay, Block, BlockEntity, CurrentProps, Document, DrawingProperties, Group, Layer, LayerState,
    NamedView, Styled, check_name, sanitize_name,
};
pub use entity::{
    Annotative, Attribute, Color, Entity, EntityId, FAR, Geometry, Hyperlink, Placement, Vertex, XClip, constant_width,
    viewport_corners,
};
pub use hatch::HatchPattern;
pub use layout::{MODEL, Paper, Space};
pub use linetype::{Linetype, Lineweight, Transparency};
pub use oxidraft_geom::{ellipse::Ellipse, spline::Spline};
pub use palette::aci_rgb;
pub use purge::Unused;
pub use refedit::RefEdit;
pub use underlay::{ImageDef, PictureAdjust, image_corners};
pub use units::{AngleType, LengthType, Units};
pub use xref::{Xref, XrefStatus, dependent_name, is_dependent};

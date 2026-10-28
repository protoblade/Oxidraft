//! Helpers shared by the modify commands.

use oxidraft_model::{Entity, Geometry};

use crate::{Ctx, Selection};

/// The entities a selection refers to, as they are now.
pub(crate) fn selected_entities(selection: &Selection, ctx: &Ctx) -> Vec<Entity> {
    selection.ids.iter().filter_map(|&id| ctx.doc.entity(id).cloned()).collect()
}

/// Replaces each entity's geometry with `f(geometry)`, or with `copy` adds the
/// result as a new entity on the same layer and color.
pub(crate) fn apply(selected: &[Entity], copy: bool, ctx: &mut Ctx, f: impl Fn(&Geometry) -> Geometry) {
    for e in selected {
        let g = f(&e.geometry);
        if copy {
            ctx.doc.add_like(g, e);
        } else {
            ctx.doc.modify(e.id, g);
        }
    }
}

pub(crate) const SELECT: &str = "Select objects, then press Enter.";

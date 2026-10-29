use oxidraft_model::Geometry;

use crate::{
    Command, Ctx, Input, Prompt, Step,
    common::{SELECT, selected_entities},
};

/// EXPLODE: breaks polylines into their line and arc segments, and block
/// references into the objects they contain.
#[derive(Default)]
pub struct Explode;

impl Command for Explode {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(selection) = input else { return Err(SELECT.into()) };
        let mut not_exploded = 0;
        for e in selected_entities(&selection, ctx) {
            match &e.geometry {
                Geometry::Polyline { .. } => {
                    ctx.doc.remove(e.id);
                    for seg in e.geometry.segments() {
                        ctx.doc.add_like(Geometry::from_seg(&seg), &e);
                    }
                }
                // An array's items become plain objects.
                Geometry::Insert { .. } if e.array.is_some() => {
                    ctx.doc.remove(e.id);
                    for (be, g) in ctx.doc.array_objects(&e) {
                        let layer = if be.layer == "0" { &e.layer } else { &be.layer };
                        ctx.doc.add_styled(g, layer, be.color, &be.linetype, be.lineweight);
                    }
                    if let (Geometry::Insert { block, .. }, Some(a)) = (&e.geometry, e.array.as_deref()) {
                        ctx.doc.remove_unused_block(block);
                        ctx.doc.remove_unused_block(&a.item);
                    }
                }
                // One level, as in AutoCAD: nested references stay references.
                // Attribute values become plain text (like AutoCAD's BURST).
                // AutoCAD won't explode an attached drawing: bind it first.
                Geometry::Insert { block, .. } if ctx.doc.xref(block).is_some() => {
                    ctx.messages.push(format!("\"{block}\" is an external reference; bind it to explode it."));
                    not_exploded += 1;
                }
                Geometry::Insert { block, placement, attributes, .. } => {
                    let Some(def) = ctx.doc.block(block).cloned() else {
                        not_exploded += 1;
                        continue;
                    };
                    ctx.doc.remove(e.id);
                    for be in &def.entities {
                        let layer = if be.layer == "0" { &e.layer } else { &be.layer };
                        let placed = placement.place(def.base, &be.geometry);
                        ctx.doc.add_styled(placed, layer, be.color, &be.linetype, be.lineweight);
                    }
                    for a in attributes.iter().filter(|a| !a.invisible && !a.value.is_empty()) {
                        // ByBlock only means something inside a reference.
                        let color = if a.color == oxidraft_model::Color::ByBlock { e.color } else { a.color };
                        ctx.doc.add_on_layer(a.text(), &a.layer, color);
                    }
                }
                _ => not_exploded += 1,
            }
        }
        if not_exploded > 0 {
            let verb = if not_exploded == 1 { "was" } else { "were" };
            ctx.messages.push(format!("{not_exploded} {verb} not able to be exploded."));
        }
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

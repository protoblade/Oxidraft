//! REFEDIT, REFSET and REFCLOSE: editing a block reference or xref in place.

use oxidraft_model::{EntityId, Geometry, snap};

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest};

const NOT_EDITING: &str = "** Command not allowed unless a reference is checked out with REFEDIT command **";

/// Opens reference `id` for editing in place. Returns the lines to print.
pub(crate) fn open(doc: &mut oxidraft_model::Document, id: EntityId) -> Result<Vec<String>, String> {
    if doc.refedit().is_some() {
        return Err("** A reference is already being edited; end it with REFCLOSE **".into());
    }
    let Some(Geometry::Insert { block, .. }) = doc.entity(id).map(|e| e.geometry.clone()) else {
        return Err("Select a block reference or xref.".into());
    };
    let n = doc.begin_refedit(id)?;
    Ok(vec![
        format!("Reference name: {block}"),
        format!("{n} object(s) added to the working set."),
        "Use REFCLOSE or the Edit Reference panel to end reference editing session.".into(),
    ])
}

/// REFEDIT: opens the picked reference for editing in place.
#[derive(Default)]
pub struct RefEdit;

impl Command for RefEdit {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select reference")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } => {
                if ctx.doc.refedit().is_some() {
                    ctx.messages.push("** A reference is already being edited; end it with REFCLOSE **".into());
                    return Ok(Step::Done);
                }
                let id = ctx.pick(at).ok_or("No object found.")?;
                ctx.messages.extend(open(ctx.doc, id)?);
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select a block reference or xref.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

/// REFCLOSE: saves the working set back to the block (or asks the app to
/// write the xref's file), or discards it.
pub struct RefClose {
    /// Started as the panel's Save Changes or Discard Changes.
    preset: Option<bool>,
    idle: bool,
    /// ARRAYCLOSE: asks Yes or No.
    array: bool,
}

impl RefClose {
    pub fn new(save: Option<bool>) -> Self {
        Self { preset: save, idle: false, array: false }
    }

    /// ARRAYCLOSE: ends editing an array's source objects.
    pub fn array() -> Self {
        Self { array: true, ..Self::new(None) }
    }

    fn close(save: bool, ctx: &mut Ctx) -> Step {
        let Some(r) = ctx.doc.refedit() else {
            ctx.messages.push(NOT_EDITING.into());
            return Step::Done;
        };
        if save && ctx.doc.xref(&r.block).is_some() {
            // Writing the xref's file is the app's.
            ctx.ui = Some(UiRequest::SaveXrefEdit);
            return Step::Done;
        }
        let (block, array) = (r.block.clone(), r.array.is_some());
        ctx.doc.close_refedit(save);
        if array {
            let done =
                if save { "Changes to the array's source saved." } else { "Changes to the array's source discarded." };
            ctx.messages.push(done.into());
        } else if save {
            let n = ctx
                .doc
                .entities()
                .filter(|e| matches!(&e.geometry, Geometry::Insert { block: b, .. } if b.eq_ignore_ascii_case(&block)))
                .count();
            ctx.messages.push(format!("Block \"{block}\" redefined; {n} reference(s) updated."));
        } else {
            ctx.messages.push("Reference edits discarded.".into());
        }
        Step::Done
    }
}

impl Command for RefClose {
    fn prepare(&mut self, doc: &oxidraft_model::Document) -> Option<String> {
        self.idle = doc.refedit().is_none();
        self.idle.then(|| NOT_EDITING.into())
    }

    fn is_finished(&self) -> bool {
        self.idle
    }

    fn prompt(&self) -> Prompt {
        if self.array {
            return Prompt::new("Save changes to array").with_keywords(&["Yes", "No"]).with_default("Yes");
        }
        Prompt::new("Enter option").with_keywords(&["Save", "Discard reference changes"]).with_default("Save")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if let Some(save) = self.preset {
            return Ok(Self::close(save, ctx));
        }
        match input {
            Input::Keyword("Discard reference changes" | "No") => Ok(Self::close(false, ctx)),
            Input::Keyword(_) | Input::Enter => Ok(Self::close(true, ctx)),
            _ => Err("Enter Save or Discard.".into()),
        }
    }
}

/// REFSET: moves objects between the working set and the drawing. Objects
/// outside the working set can't be selected as usual, so they're picked
/// one at a time.
pub struct RefSet {
    add: Option<bool>,
    picked: Vec<EntityId>,
    idle: bool,
}

impl RefSet {
    pub fn new(add: Option<bool>) -> Self {
        Self { add, picked: Vec::new(), idle: false }
    }

    fn finish(&mut self, add: bool, ctx: &mut Ctx) -> Step {
        let n = ctx.doc.refset(&self.picked, add);
        let place = if add { "added to" } else { "removed from" };
        ctx.messages.push(format!("{n} object(s) {place} the working set."));
        Step::Done
    }
}

impl Command for RefSet {
    fn prepare(&mut self, doc: &oxidraft_model::Document) -> Option<String> {
        self.idle = doc.refedit().is_none();
        self.idle.then(|| NOT_EDITING.into())
    }

    fn is_finished(&self) -> bool {
        self.idle
    }

    fn prompt(&self) -> Prompt {
        match self.add {
            None => Prompt::new("Enter an option").with_keywords(&["Add", "Remove"]).with_default("Add"),
            Some(_) => Prompt::new("Select objects"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Some(r) = ctx.doc.refedit().cloned() else {
            ctx.messages.push(NOT_EDITING.into());
            return Ok(Step::Done);
        };
        match (self.add, input) {
            (None, Input::Keyword("Remove")) => self.add = Some(false),
            (None, Input::Keyword(_) | Input::Enter) => self.add = Some(true),
            (Some(add), Input::Pick { at, .. }) => {
                // Adding picks from the drawing; removing, from the working set.
                let wanted = |e: &oxidraft_model::Entity| {
                    ctx.doc.is_editable(&e.layer) && e.id != r.reference && r.in_working_set(e.id) != add
                };
                let id = snap::pick_where(ctx.doc, at, ctx.pick_tolerance, wanted).ok_or("No object found.")?;
                if !self.picked.contains(&id) {
                    self.picked.push(id);
                }
                ctx.messages.push(format!("{} found", self.picked.len()));
            }
            (Some(add), Input::Enter) => return Ok(self.finish(add, ctx)),
            (None, _) => return Err("Enter Add or Remove.".into()),
            _ => return Err("Select objects, then press Enter.".into()),
        }
        Ok(Step::Continue)
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.picked.clone()
    }

    fn wants_pick(&self) -> bool {
        self.add.is_some()
    }
}

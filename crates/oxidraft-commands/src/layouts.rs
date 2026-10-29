//! LAYOUT (LO): makes, copies, renames, deletes and switches between
//! layouts from the command line, as the layout tabs' shortcut menu does.

use oxidraft_model::{Document, MODEL, Space};

use crate::{Command, Ctx, Input, Prompt, Step};

const OPTIONS: &[&str] = &["Copy", "Delete", "New", "Rename", "Set", "?"];

#[derive(Clone, PartialEq)]
enum Stage {
    Option,
    NewName,
    CopyWhich,
    CopyName { source: Space, default: String },
    DeleteWhich,
    RenameWhich,
    RenameName { layout: Space, old: String },
    SetWhich,
}

/// LAYOUT: its options, each asking for the layout names it needs; Enter
/// takes the one offered (the current layout, or a new name).
pub struct LayoutCommand {
    stage: Stage,
    /// The layout offered by default: the current one, or the first.
    current: Option<(Space, String)>,
    /// What New offers: Layout2, Layout3, …
    next_name: String,
}

impl Default for LayoutCommand {
    fn default() -> Self {
        Self { stage: Stage::Option, current: None, next_name: String::new() }
    }
}

/// "Layout1 (2)", "Layout1 (3)", …: the first copy name not taken.
fn copy_name(doc: &Document, of: &str) -> String {
    (2..).map(|n| format!("{of} ({n})")).find(|n| doc.layout_named(n).is_none()).unwrap_or_default()
}

impl LayoutCommand {
    fn current_name(&self) -> String {
        self.current.as_ref().map_or_else(String::new, |c| c.1.clone())
    }

    /// The layout `text` names, or the one offered when it's empty.
    fn which(&self, doc: &Document, text: &str) -> Result<(Space, String), String> {
        if text.trim().is_empty() {
            return self.current.clone().ok_or_else(|| "There are no layouts.".to_string());
        }
        doc.layout_named(text)
            .map(|l| (l.id, l.name.clone()))
            .ok_or_else(|| format!("Layout \"{}\" not found.", text.trim()))
    }
}

impl Command for LayoutCommand {
    fn prepare(&mut self, doc: &Document) -> Option<String> {
        let current = doc.layout(doc.space()).or_else(|| doc.layouts().first().copied());
        self.current = current.map(|l| (l.id, l.name.clone()));
        self.next_name = doc.next_layout_name();
        None
    }

    fn prompt(&self) -> Prompt {
        let current = self.current_name();
        match &self.stage {
            Stage::Option => Prompt::new("Enter layout option").with_keywords(OPTIONS).with_default("set"),
            Stage::NewName => Prompt::new("Enter name of new layout").with_default(&self.next_name),
            Stage::CopyWhich => Prompt::new("Enter name of layout to copy").with_default(current),
            Stage::CopyName { default, .. } => Prompt::new("Enter layout name for copy").with_default(default),
            Stage::DeleteWhich => Prompt::new("Enter name of layout to delete").with_default(current),
            Stage::RenameWhich => Prompt::new("Enter layout to rename").with_default(current),
            Stage::RenameName { old, .. } => Prompt::new("Enter new layout name").with_default(old),
            Stage::SetWhich => Prompt::new("Enter layout to make current").with_default(current),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let text = match input {
            Input::Text(t) => t,
            Input::Enter => String::new(),
            Input::Keyword(k) if self.stage == Stage::Option => {
                self.stage = match k {
                    "Copy" => Stage::CopyWhich,
                    "Delete" => Stage::DeleteWhich,
                    "New" => Stage::NewName,
                    "Rename" => Stage::RenameWhich,
                    "Set" => Stage::SetWhich,
                    _ => {
                        let mut list = String::from("Active Layouts:\n");
                        for l in ctx.doc.layouts() {
                            list.push('\n');
                            list.push_str(&l.name);
                        }
                        ctx.messages.push(list);
                        return Ok(Step::Done);
                    }
                };
                return Ok(Step::Continue);
            }
            _ => return Err("Enter a layout name.".into()),
        };
        let given = |default: &str| if text.trim().is_empty() { default.to_string() } else { text.trim().to_string() };
        match self.stage.clone() {
            // Enter at the options: Set.
            Stage::Option => {
                self.stage = Stage::SetWhich;
                return Ok(Step::Continue);
            }
            Stage::NewName => {
                ctx.doc.new_layout(&given(&self.next_name))?;
            }
            Stage::CopyWhich => {
                let (source, name) = self.which(ctx.doc, &text)?;
                self.stage = Stage::CopyName { source, default: copy_name(ctx.doc, &name) };
                return Ok(Step::Continue);
            }
            Stage::CopyName { source, default } => {
                ctx.doc.copy_layout(source, &given(&default))?;
            }
            Stage::DeleteWhich => {
                let (layout, name) = self.which(ctx.doc, &text)?;
                ctx.doc.delete_layout(layout);
                ctx.messages.push(format!("Layout \"{name}\" deleted."));
            }
            Stage::RenameWhich => {
                let (layout, old) = self.which(ctx.doc, &text)?;
                self.stage = Stage::RenameName { layout, old };
                return Ok(Step::Continue);
            }
            Stage::RenameName { layout, old } => {
                ctx.doc.rename_layout(layout, &given(&old))?;
            }
            Stage::SetWhich => {
                let space =
                    if text.trim().eq_ignore_ascii_case("Model") { MODEL } else { self.which(ctx.doc, &text)?.0 };
                ctx.doc.set_space(space);
            }
        }
        Ok(Step::Done)
    }

    fn wants_text(&self) -> bool {
        self.stage != Stage::Option
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use oxidraft_geom::Point;

    fn run(engine: &mut Engine, doc: &mut Document, inputs: &[&str]) {
        for i in inputs {
            engine.submit(i, doc, Point::ORIGIN, 1.0);
        }
    }

    fn names(doc: &Document) -> Vec<String> {
        doc.layouts().iter().map(|l| l.name.clone()).collect()
    }

    #[test]
    fn layout_options_from_the_command_line() {
        let mut doc = Document::new();
        let mut e = Engine::new();
        // New, offering Layout1, then one named.
        run(&mut e, &mut doc, &["LAYOUT", "N", "", "LO", "n", "Site plan"]);
        assert_eq!(names(&doc), ["Layout1", "Site plan"]);
        // Set makes one current; Copy offers it and "(2)".
        run(&mut e, &mut doc, &["LAYOUT", "S", "site plan", "LAYOUT", "C", "", ""]);
        assert_eq!(doc.layout(doc.space()).unwrap().name, "Site plan");
        assert_eq!(names(&doc), ["Layout1", "Site plan", "Site plan (2)"]);
        // Rename, and a name that's taken is refused.
        run(&mut e, &mut doc, &["LAYOUT", "R", "Layout1", "Site plan"]);
        assert!(e.history().iter().any(|l| l.contains("already exists")));
        run(&mut e, &mut doc, &["Cover"]);
        assert_eq!(names(&doc), ["Cover", "Site plan", "Site plan (2)"]);
        // Delete the current one: back in model space.
        run(&mut e, &mut doc, &["LAYOUT", "D", ""]);
        assert_eq!(names(&doc), ["Cover", "Site plan (2)"]);
        assert_eq!(doc.space(), MODEL);
        assert!(e.history().iter().any(|l| l == "Layout \"Site plan\" deleted."));
        run(&mut e, &mut doc, &["LAYOUT", "?"]);
        assert!(e.history().iter().any(|l| l.contains("Active Layouts:")));
        // Each was one undo step.
        doc.undo();
        assert_eq!(names(&doc), ["Cover", "Site plan", "Site plan (2)"]);
    }
}

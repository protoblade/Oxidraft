//! GROUP (G), UNGROUP and GROUPEDIT: objects picked together.

use oxidraft_model::{EntityId, Group};

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Clone, PartialEq)]
enum Stage {
    Select,
    Name,
    Description,
}

/// GROUP: the objects selected become a group, named (Name) or not,
/// with a Description.
pub struct GroupCommand {
    stage: Stage,
    name: String,
    description: String,
}

impl Default for GroupCommand {
    fn default() -> Self {
        Self { stage: Stage::Select, name: String::new(), description: String::new() }
    }
}

impl Command for GroupCommand {
    fn prompt(&self) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select objects").with_keywords(&["Name", "Description"]),
            Stage::Name => Prompt::new("Enter a group name or [?]"),
            Stage::Description => Prompt::new("Enter group description"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.stage, input) {
            (Stage::Select, Input::Keyword("Name")) => self.stage = Stage::Name,
            (Stage::Select, Input::Keyword(_)) => self.stage = Stage::Description,
            (Stage::Name, Input::Text(t)) if t.trim() == "?" => {
                let names: Vec<String> =
                    ctx.doc.groups().iter().filter(|g| !g.is_unnamed()).map(|g| g.name.clone()).collect();
                ctx.messages.push(if names.is_empty() { "No named groups.".into() } else { names.join("\n") });
            }
            (Stage::Name, Input::Text(t)) => {
                if ctx.doc.groups().iter().any(|g| g.name.eq_ignore_ascii_case(t.trim())) {
                    return Err(format!("A group named \"{}\" already exists.", t.trim()));
                }
                oxidraft_model::check_name(&t)?;
                self.name = t.trim().to_string();
                self.stage = Stage::Select;
            }
            (Stage::Description, Input::Text(t)) => {
                self.description = t;
                self.stage = Stage::Select;
            }
            (Stage::Name | Stage::Description, Input::Enter) => self.stage = Stage::Select,
            (Stage::Select, Input::Selection(s)) => {
                if s.ids.is_empty() {
                    return Ok(Step::Done);
                }
                let name = ctx.doc.add_group(&self.name, &self.description, s.ids)?;
                ctx.messages.push(if name.starts_with('*') {
                    "Unnamed group has been created.".into()
                } else {
                    format!("Group \"{name}\" has been created.")
                });
                return Ok(Step::Done);
            }
            _ => return Err("Select objects, or choose an option.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        self.stage == Stage::Select
    }

    fn wants_text(&self) -> bool {
        self.stage != Stage::Select
    }

    fn text_with_spaces(&self) -> bool {
        self.stage == Stage::Description
    }
}

/// The group a click at `at` picks one object of, or the group named.
fn group_at(ctx: &Ctx, at: oxidraft_geom::Point) -> Option<Group> {
    ctx.pick(at).and_then(|id| ctx.doc.group_of(id))
}

/// UNGROUP: the group picked (or named) comes apart; its objects stay.
#[derive(Default)]
pub struct Ungroup {
    naming: bool,
}

impl Ungroup {
    fn dissolve(group: &Group, ctx: &mut Ctx) -> Step {
        let rest: Vec<Group> = ctx.doc.all_groups().iter().filter(|g| g.name != group.name).cloned().collect();
        ctx.doc.set_groups(rest);
        ctx.messages.push(if group.is_unnamed() {
            "Group has been exploded.".into()
        } else {
            format!("Group \"{}\" has been exploded.", group.name)
        });
        Step::Done
    }
}

impl Command for Ungroup {
    fn prompt(&self) -> Prompt {
        if self.naming {
            Prompt::new("Enter a group name")
        } else {
            Prompt::new("Select group").with_keywords(&["Name"])
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Keyword(_) => {
                self.naming = true;
                Ok(Step::Continue)
            }
            Input::Text(name) => {
                let group = ctx.doc.groups().into_iter().find(|g| g.name.eq_ignore_ascii_case(name.trim()));
                let group = group.ok_or_else(|| format!("No group named \"{}\".", name.trim()))?;
                Ok(Self::dissolve(&group, ctx))
            }
            Input::Pick { at, .. } => {
                let group = group_at(ctx, at).ok_or("That object isn't in a group.")?;
                Ok(Self::dissolve(&group, ctx))
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select a group.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        !self.naming
    }

    fn wants_text(&self) -> bool {
        self.naming
    }
}

#[derive(Clone, PartialEq)]
enum EditStage {
    Group,
    Option(String),
    Add(String),
    Remove(String),
    Rename(String),
}

/// GROUPEDIT: Add objects to a group, Remove objects from it, or
/// REName it.
pub struct GroupEdit {
    stage: EditStage,
}

impl Default for GroupEdit {
    fn default() -> Self {
        Self { stage: EditStage::Group }
    }
}

impl GroupEdit {
    fn change(ctx: &mut Ctx, name: &str, f: impl Fn(&mut Group)) {
        let mut groups = ctx.doc.all_groups().to_vec();
        if let Some(g) = groups.iter_mut().find(|g| g.name == name) {
            f(g);
        }
        groups.retain(|g| !g.members.is_empty());
        ctx.doc.set_groups(groups);
    }
}

impl Command for GroupEdit {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            EditStage::Group => Prompt::new("Select group"),
            EditStage::Option(_) => {
                Prompt::new("Enter an option").with_keywords(&["Add objects", "Remove objects", "REName"])
            }
            EditStage::Add(_) => Prompt::new("Select objects to add to group"),
            EditStage::Remove(_) => Prompt::new("Select objects to remove from group"),
            EditStage::Rename(_) => Prompt::new("Enter new name for group"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (EditStage::Group, Input::Pick { at, .. }) => {
                let group = group_at(ctx, at).ok_or("That object isn't in a group.")?;
                self.stage = EditStage::Option(group.name);
            }
            (EditStage::Option(name), Input::Keyword(k)) => {
                self.stage = match k {
                    "Add objects" => EditStage::Add(name),
                    "Remove objects" => EditStage::Remove(name),
                    _ => EditStage::Rename(name),
                };
            }
            (EditStage::Add(name), Input::Selection(s)) => {
                Self::change(ctx, &name, |g| {
                    for id in &s.ids {
                        if !g.members.contains(id) {
                            g.members.push(*id);
                        }
                    }
                });
                return Ok(Step::Done);
            }
            (EditStage::Remove(name), Input::Selection(s)) => {
                let out: Vec<EntityId> = s.ids;
                Self::change(ctx, &name, |g| g.members.retain(|m| !out.contains(m)));
                return Ok(Step::Done);
            }
            (EditStage::Rename(name), Input::Text(new)) => {
                oxidraft_model::check_name(&new)?;
                let taken = ctx.doc.groups().iter().any(|g| g.name != name && g.name.eq_ignore_ascii_case(new.trim()));
                if taken {
                    return Err(format!("A group named \"{}\" already exists.", new.trim()));
                }
                Self::change(ctx, &name, |g| g.name = new.trim().to_string());
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Choose a group, then an option.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        self.stage == EditStage::Group
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, EditStage::Add(_) | EditStage::Remove(_))
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, EditStage::Rename(_))
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::Document;

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn groups_are_picked_together_until_ungrouped() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "0,5", "10,5", "", "c", "50,50", "1"]);
        // A named group of the two lines.
        run(&mut e, &mut d, &["group", "n", "Lines"]);
        let (a, b) = (Point::new(5.0, 0.0), Point::new(5.0, 5.0));
        e.click(a, a, false, &mut d, 0.5);
        e.click(b, b, false, &mut d, 0.5);
        run(&mut e, &mut d, &[""]);
        assert_eq!(d.groups().len(), 1);
        assert!(e.history().iter().any(|l| l == "Group \"Lines\" has been created."));
        // Picking one picks both.
        e.click(a, a, false, &mut d, 0.5);
        assert_eq!(e.selection().len(), 2);
        e.cancel(&mut d);
        // Ctrl+Shift+A's setting off: one at a time.
        e.vars.group_select = false;
        e.click(a, a, false, &mut d, 0.5);
        assert_eq!(e.selection().len(), 1);
        e.cancel(&mut d);
        e.vars.group_select = true;
        run(&mut e, &mut d, &["groupedit"]);
        e.click(a, a, false, &mut d, 0.5);
        run(&mut e, &mut d, &["ren", "Edges"]);
        assert_eq!(d.groups()[0].name, "Edges");
        run(&mut e, &mut d, &["ungroup"]);
        e.click(a, a, false, &mut d, 0.5);
        assert!(d.groups().is_empty());
        d.undo();
        assert_eq!(d.groups().len(), 1);
    }
}

//! FIELD's text placed in the drawing, UPDATEFIELD, and TEXTEDIT (`ED`),
//! which opens the text picked in the text editor.

use oxidraft_geom::Point;
use oxidraft_model::{Attach, EntityId, Geometry, TextLook, TextStyle, field};

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest, Vars};

/// FIELD (after its dialog): a field's code, then where its text goes, as
/// multiline text in the current style.
pub struct FieldText {
    code: Option<String>,
    height: f64,
    style: TextStyle,
}

impl Default for FieldText {
    fn default() -> Self {
        Self { code: None, height: 2.5, style: TextStyle::standard() }
    }
}

impl FieldText {
    /// Any text, placed the same way (PASTESPEC's Text).
    pub fn with_text(content: String) -> Self {
        Self { code: Some(content), ..Self::default() }
    }

    fn text(&self, code: &str, at: Point) -> Geometry {
        Geometry::Text {
            look: TextLook::of(&self.style),
            position: at,
            height: self.height,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::TopLeft,
            content: code.to_string(),
        }
    }
}

impl Command for FieldText {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        self.height = vars.text_height;
        None
    }

    fn prepare(&mut self, doc: &oxidraft_model::Document) -> Option<String> {
        self.style = doc.current_text_style();
        if self.style.height > 0.0 {
            self.height = self.style.height;
        }
        None
    }

    fn prompt(&self) -> Prompt {
        match self.code {
            None => Prompt::new("Enter field expression"),
            Some(_) => Prompt::new("Specify start point"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (&self.code, input) {
            (None, Input::Text(t)) if field::has_fields(&t) => self.code = Some(t),
            (None, Input::Text(_)) => return Err("That isn't a field expression, like %<\\AcVar Date>%.".into()),
            (Some(code), Input::Point(p)) => {
                ctx.doc.add(self.text(code, p));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Specify the text's start point.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.code.as_deref().map(|c| vec![self.text(c, cursor)]).unwrap_or_default()
    }

    fn wants_text(&self) -> bool {
        self.code.is_none()
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// How many fields `g` has: in text, attribute values and table cells.
fn fields_in(g: &Geometry) -> usize {
    let count = |s: &str| field::fields(s).len();
    match g {
        Geometry::Text { content, .. } => count(content),
        Geometry::Insert { attributes, .. } => attributes.iter().map(|a| count(&a.value)).sum(),
        _ => 0,
    }
}

/// UPDATEFIELD: brings the selected objects' fields up to date. Fields
/// show their values as they're drawn, so it finds and redraws them.
#[derive(Default)]
pub struct UpdateField;

impl Command for UpdateField {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Selection(s) => {
                let found: usize =
                    s.ids.iter().filter_map(|id| ctx.doc.entity(*id)).map(|e| fields_in(&e.geometry)).sum();
                ctx.messages.push(format!("{found} field(s) found."));
                ctx.messages.push(format!("{found} field(s) updated."));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select objects.".into()),
        }
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

/// TEXTEDIT (`ED`, DDEDIT): the text picked opens in the text editor;
/// a block's attributes in the attribute editor, a table to edit its cells.
#[derive(Default)]
pub struct TextEdit {
    preselected: Option<EntityId>,
}

impl TextEdit {
    fn open(id: EntityId, ctx: &mut Ctx) -> Result<Step, String> {
        let request = match ctx.doc.entity(id).map(|e| &e.geometry) {
            Some(Geometry::Text { .. }) => UiRequest::EditText(id),
            Some(Geometry::Insert { attributes, .. }) if !attributes.is_empty() => UiRequest::EditAttributes(id),
            Some(Geometry::Table { .. }) => UiRequest::EditTable(id),
            _ => return Err("That isn't text, a block with attributes or a table.".into()),
        };
        ctx.ui = Some(request);
        Ok(Step::Done)
    }
}

impl Command for TextEdit {
    fn preselected(&mut self, ids: &[EntityId]) {
        if let [id] = ids {
            self.preselected = Some(*id);
        }
    }

    fn prompt(&self) -> Prompt {
        Prompt::new("Select an annotation object")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } => {
                let id = ctx.pick(at).ok_or("Nothing there.")?;
                Self::open(id, ctx)
            }
            Input::Enter => match self.preselected.take() {
                Some(id) => Self::open(id, ctx),
                None => Ok(Step::Done),
            },
            _ => Err("Pick text to edit.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

/// XOPEN: the attached drawing picked opens in its own window.
#[derive(Default)]
pub struct XOpen;

impl Command for XOpen {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select xref")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Pick { at, .. } => {
                let id = ctx.pick(at).ok_or("Nothing there.")?;
                let name = match ctx.doc.entity(id).map(|e| &e.geometry) {
                    Some(Geometry::Insert { block, .. }) if ctx.doc.xref(block).is_some() => block.clone(),
                    _ => return Err("That isn't an attached drawing (xref).".into()),
                };
                ctx.ui = Some(UiRequest::OpenXref(name));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Pick an xref.".into()),
        }
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;

    #[test]
    fn field_places_its_text() {
        let mut doc = oxidraft_model::Document::new();
        let mut engine = Engine::default();
        engine.run("FIELD", &mut doc);
        engine.submit(r#"%<\AcVar Date \f "yyyy">%"#, &mut doc, Point::ORIGIN, 0.1);
        engine.submit("5,5", &mut doc, Point::ORIGIN, 0.1);
        assert!(!engine.is_active());
        let e = doc.entities().next().unwrap();
        assert!(matches!(&e.geometry, Geometry::Text { content, position, .. }
            if field::has_fields(content) && *position == Point::new(5.0, 5.0)));
        engine.run("UPDATEFIELD", &mut doc);
        engine.submit("all", &mut doc, Point::ORIGIN, 0.1);
        engine.submit("", &mut doc, Point::ORIGIN, 0.1);
        assert!(engine.history().iter().any(|l| l.contains("1 field(s) updated")));
    }

    #[test]
    fn textedit_asks_for_the_text_editor() {
        let mut doc = oxidraft_model::Document::new();
        let id = doc.add(Geometry::Text {
            look: TextLook::default(),
            position: Point::ORIGIN,
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "Room".into(),
        });
        let mut engine = Engine::default();
        engine.run("ED", &mut doc);
        engine.click(Point::new(1.0, 1.0), Point::new(1.0, 1.0), false, &mut doc, 0.5);
        assert_eq!(engine.take_ui_requests(), vec![UiRequest::EditText(id)]);
    }
}

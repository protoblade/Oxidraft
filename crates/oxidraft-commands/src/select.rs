//! SELECTSIMILAR, and the filtering behind Quick Select (QSELECT).

use oxidraft_model::{Color, Document, Entity, EntityId, Geometry};

use crate::{Command, Ctx, Input, Prompt, Step, Vars};

/// SELECTSIMILARMODE: what besides the object type must match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimilarMode {
    pub color: bool,
    pub layer: bool,
    pub linetype: bool,
    pub lineweight: bool,
    /// Block name for block references.
    pub name: bool,
}

impl Default for SimilarMode {
    /// AutoCAD's default (130): layer and name.
    fn default() -> Self {
        Self { color: false, layer: true, linetype: false, lineweight: false, name: true }
    }
}

/// The object type as SELECTSIMILAR and Quick Select compare it: the
/// AutoCAD name, so rotated and aligned dimensions differ.
pub fn object_type(g: &Geometry) -> &'static str {
    g.name()
}

fn block_name(g: &Geometry) -> Option<&str> {
    match g {
        Geometry::Insert { block, .. } => Some(block),
        _ => None,
    }
}

/// True if `e` is like `seed` in the ways `mode` asks for.
pub fn similar(e: &Entity, seed: &Entity, mode: SimilarMode) -> bool {
    object_type(&e.geometry) == object_type(&seed.geometry)
        && (!mode.layer || e.layer.eq_ignore_ascii_case(&seed.layer))
        && (!mode.color || e.color == seed.color)
        && (!mode.linetype || e.linetype.eq_ignore_ascii_case(&seed.linetype))
        && (!mode.lineweight || e.lineweight == seed.lineweight)
        && (!mode.name
            || block_name(&e.geometry).map(str::to_ascii_lowercase)
                == block_name(&seed.geometry).map(str::to_ascii_lowercase))
}

/// Every selectable object similar to one of `seeds`, in drawing order.
pub fn select_similar(doc: &Document, seeds: &[EntityId], mode: SimilarMode) -> Vec<EntityId> {
    let seeds: Vec<&Entity> = seeds.iter().filter_map(|&id| doc.entity(id)).collect();
    doc.entities()
        .filter(|e| doc.is_editable(&e.layer))
        .filter(|e| seeds.iter().any(|s| similar(e, s, mode)))
        .map(|e| e.id)
        .collect()
}

/// The text Quick Select shows and compares for a color.
pub fn color_label(c: Color) -> String {
    match c {
        Color::ByLayer => "ByLayer".into(),
        Color::ByBlock => "ByBlock".into(),
        Color::Index(i) => format!("{i}"),
    }
}

/// A property Quick Select filters on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickProperty {
    Color,
    Layer,
    Linetype,
    Lineweight,
}

impl QuickProperty {
    pub const ALL: [QuickProperty; 4] =
        [QuickProperty::Color, QuickProperty::Layer, QuickProperty::Linetype, QuickProperty::Lineweight];

    pub fn label(self) -> &'static str {
        match self {
            QuickProperty::Color => "Color",
            QuickProperty::Layer => "Layer",
            QuickProperty::Linetype => "Linetype",
            QuickProperty::Lineweight => "Lineweight",
        }
    }

    /// The object's value, as Quick Select lists and compares it.
    pub fn value(self, e: &Entity) -> String {
        match self {
            QuickProperty::Color => color_label(e.color),
            QuickProperty::Layer => e.layer.clone(),
            QuickProperty::Linetype => e.linetype.clone(),
            QuickProperty::Lineweight => e.lineweight.label(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickOperator {
    Equals,
    NotEqual,
    /// Every object of the type, whatever the property.
    SelectAll,
}

impl QuickOperator {
    pub const ALL: [QuickOperator; 3] = [QuickOperator::Equals, QuickOperator::NotEqual, QuickOperator::SelectAll];

    pub fn label(self) -> &'static str {
        match self {
            QuickOperator::Equals => "= Equals",
            QuickOperator::NotEqual => "<> Not Equal",
            QuickOperator::SelectAll => "* Select All",
        }
    }
}

/// The choices in the Quick Select dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickSelect {
    /// Filter the current selection instead of the whole drawing.
    pub in_selection: bool,
    /// An object type name ([`object_type`]), or `None` for all types.
    pub object_type: Option<String>,
    pub property: QuickProperty,
    pub operator: QuickOperator,
    pub value: String,
    /// Select the objects that don't match instead.
    pub exclude: bool,
    /// Add to the current selection instead of replacing it.
    pub append: bool,
}

impl QuickSelect {
    /// The objects the filter looks at: selectable, of the chosen type, and
    /// in `current` if it filters the selection.
    pub fn candidates<'a>(&'a self, doc: &'a Document, current: &'a [EntityId]) -> impl Iterator<Item = &'a Entity> {
        doc.entities()
            .filter(|e| doc.is_editable(&e.layer))
            .filter(move |e| !self.in_selection || current.contains(&e.id))
            .filter(|e| self.object_type.as_deref().is_none_or(|t| object_type(&e.geometry) == t))
    }

    /// The new selection.
    pub fn apply(&self, doc: &Document, current: &[EntityId]) -> Vec<EntityId> {
        let matches = |e: &Entity| match self.operator {
            QuickOperator::SelectAll => true,
            QuickOperator::Equals => self.property.value(e).eq_ignore_ascii_case(&self.value),
            QuickOperator::NotEqual => !self.property.value(e).eq_ignore_ascii_case(&self.value),
        };
        let chosen = self.candidates(doc, current).filter(|e| matches(e) != self.exclude).map(|e| e.id);
        let mut out: Vec<EntityId> = if self.append { current.to_vec() } else { Vec::new() };
        for id in chosen {
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }
}

const SETTINGS: &[&str] = &["Color", "LAyer", "LType", "LWeight", "Name", "eXit"];

/// SELECTSIMILAR: selects every object of the same type and layer (and
/// block name) as the selected ones; SEttings chooses what must match.
#[derive(Default)]
pub struct SelectSimilar {
    settings: bool,
}

impl Command for SelectSimilar {
    fn start(&mut self, vars: &Vars) -> Option<String> {
        Some(describe(vars.select_similar))
    }

    fn prompt(&self) -> Prompt {
        if self.settings {
            Prompt::new("Also match").with_keywords(SETTINGS).with_default("eXit")
        } else {
            Prompt::new("Select objects").with_keywords(&["SEttings"])
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Selection(s) if !self.settings => {
                if s.ids.is_empty() {
                    return Ok(Step::Done);
                }
                let ids = select_similar(ctx.doc, &s.ids, ctx.vars.select_similar);
                ctx.messages.push(format!("{} found", ids.len()));
                ctx.select = Some(ids);
                Ok(Step::Done)
            }
            Input::Keyword("SEttings") => {
                self.settings = true;
                Ok(Step::Continue)
            }
            Input::Keyword("eXit") | Input::Enter if self.settings => {
                self.settings = false;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if self.settings => {
                let m = &mut ctx.vars.select_similar;
                let flag = match k {
                    "Color" => &mut m.color,
                    "LAyer" => &mut m.layer,
                    "LType" => &mut m.linetype,
                    "LWeight" => &mut m.lineweight,
                    _ => &mut m.name,
                };
                *flag = !*flag;
                ctx.messages.push(describe(*m));
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Select objects, then press Enter.".into()),
        }
    }

    fn wants_selection(&self) -> bool {
        !self.settings
    }
}

fn describe(m: SimilarMode) -> String {
    let on: Vec<&str> = [
        (m.color, "Color"),
        (m.layer, "Layer"),
        (m.linetype, "Linetype"),
        (m.lineweight, "Lineweight"),
        (m.name, "Name"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    let also = if on.is_empty() { "nothing else".to_string() } else { on.join(", ") };
    format!("Matching: object type, {also}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_geom::Point;
    use oxidraft_model::{Layer, Placement};

    #[test]
    fn similar_objects_share_type_layer_and_block_name() {
        let mut d = Document::new();
        d.add_layer(Layer::new("A", 1));
        let line = |y: f64| Geometry::Line { start: Point::new(0.0, y), end: Point::new(1.0, y) };
        let seed = d.add_on_layer(line(0.0), "A", Color::ByLayer);
        let same = d.add_on_layer(line(1.0), "A", Color::Index(3));
        d.add_on_layer(line(2.0), "0", Color::ByLayer);
        d.add_on_layer(Geometry::Circle { center: Point::ORIGIN, radius: 1.0 }, "A", Color::ByLayer);
        let insert = |name: &str| Geometry::Insert {
            block: name.into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: Vec::new(),
            clip: None,
        };
        let door = d.add_on_layer(insert("DOOR"), "A", Color::ByLayer);
        let door2 = d.add_on_layer(insert("door"), "A", Color::ByLayer);
        d.add_on_layer(insert("WINDOW"), "A", Color::ByLayer);

        let mode = SimilarMode::default();
        assert_eq!(select_similar(&d, &[seed], mode), vec![seed, same]);
        assert_eq!(select_similar(&d, &[seed], SimilarMode { color: true, ..mode }), vec![seed]);
        assert_eq!(select_similar(&d, &[door], mode), vec![door, door2]);
        assert_eq!(select_similar(&d, &[door], SimilarMode { name: false, ..mode }).len(), 3);

        // Quick Select: lines on layer A, then those not on A, then appended.
        let q = QuickSelect {
            in_selection: false,
            object_type: Some("Line".into()),
            property: QuickProperty::Layer,
            operator: QuickOperator::Equals,
            value: "a".into(),
            exclude: false,
            append: false,
        };
        assert_eq!(q.apply(&d, &[]), vec![seed, same]);
        let not_a = QuickSelect { exclude: true, ..q.clone() }.apply(&d, &[]);
        assert_eq!(not_a.len(), 1);
        assert_eq!(
            QuickSelect { exclude: true, append: true, ..q.clone() }.apply(&d, &[seed]),
            [vec![seed], not_a].concat()
        );
        // Within the selection, by color.
        let red = QuickSelect {
            in_selection: true,
            object_type: None,
            property: QuickProperty::Color,
            value: "3".into(),
            ..q
        };
        assert_eq!(red.apply(&d, &[seed, same, door]), vec![same]);
    }
}

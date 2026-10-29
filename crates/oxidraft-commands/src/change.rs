//! CHPROP and CHANGE: objects' properties changed at the command line
//! (CHANGE also moves line ends, resizes circles and edits text by a
//! change point); DBLIST, LIST of everything; SOLID, filled triangles and
//! quadrilaterals; TXT2MTXT, single-line texts made one multiline text.

use oxidraft_geom::Point;
use oxidraft_model::{Attach, EntityId, Geometry, HatchPattern, Transparency, Vertex, linetype};

use crate::current_props::{parse_color, parse_lineweight};
use crate::{Command, Ctx, Input, Prompt, Step};

/// The properties CHPROP and CHANGE's Properties change.
const PROPERTIES: [&str; 5] = ["Color", "LAyer", "LType", "LWeight", "TRansparency"];

/// A transparency typed: ByLayer, ByBlock or 0–90.
fn parse_transparency(text: &str) -> Option<Transparency> {
    match text.trim().to_ascii_lowercase().as_str() {
        "bylayer" => Some(Transparency::ByLayer),
        "byblock" => Some(Transparency::ByBlock),
        t => t.parse::<u8>().ok().filter(|v| *v <= 90).map(Transparency::Percent),
    }
}

/// Sets `property` to `value` on `ids`; the message on what went wrong.
fn set_property(ctx: &mut Ctx, ids: &[EntityId], property: &str, value: &str) -> Result<(), String> {
    match property {
        "Color" => {
            let c = parse_color(value).ok_or("Enter a color: a name, a number from 1 to 255, ByLayer or ByBlock.")?;
            for id in ids {
                ctx.doc.update(*id, |e| e.color = c);
            }
        }
        "LAyer" => {
            let layer = ctx.doc.layer(value.trim()).map(|l| l.name.clone()).ok_or("No such layer.")?;
            for id in ids {
                ctx.doc.update(*id, |e| e.layer.clone_from(&layer));
            }
        }
        "LType" => {
            let t = value.trim();
            let name = if t.eq_ignore_ascii_case(linetype::BYLAYER) || t.eq_ignore_ascii_case("byblock") {
                t.to_ascii_uppercase()
            } else {
                ctx.doc.linetype(t).map(|l| l.name).ok_or("That linetype isn't loaded.")?
            };
            for id in ids {
                ctx.doc.update(*id, |e| e.linetype.clone_from(&name));
            }
        }
        "LWeight" => {
            let w = parse_lineweight(value).ok_or("Enter a lineweight in millimeters, ByLayer, ByBlock or Default.")?;
            for id in ids {
                ctx.doc.update(*id, |e| e.lineweight = w);
            }
        }
        _ => {
            let t = parse_transparency(value).ok_or("Enter a transparency from 0 to 90, ByLayer or ByBlock.")?;
            for id in ids {
                ctx.doc.update(*id, |e| e.transparency = t);
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum PropStage {
    Select,
    Property,
    Value(&'static str),
}

/// CHPROP: the selected objects' color, layer, linetype, lineweight or
/// transparency, one after another until Enter.
pub struct ChProp {
    stage: PropStage,
    ids: Vec<EntityId>,
}

impl Default for ChProp {
    fn default() -> Self {
        Self { stage: PropStage::Select, ids: Vec::new() }
    }
}

fn property_prompt(stage: PropStage) -> Prompt {
    match stage {
        PropStage::Select => Prompt::new("Select objects"),
        PropStage::Property => Prompt::new("Enter property to change").with_keywords(&PROPERTIES),
        PropStage::Value(p) => Prompt::new(match p {
            "Color" => "Enter new color",
            "LAyer" => "Enter new layer name",
            "LType" => "Enter new linetype name",
            "LWeight" => "Enter new lineweight",
            _ => "Enter transparency value (0-90)",
        }),
    }
}

/// The Property and Value stages, shared by CHPROP and CHANGE.
fn property_input(stage: &mut PropStage, ids: &[EntityId], input: Input, ctx: &mut Ctx) -> Result<Step, String> {
    match (*stage, input) {
        (PropStage::Property, Input::Keyword(k)) => *stage = PropStage::Value(k),
        (PropStage::Value(p), Input::Text(t)) => {
            set_property(ctx, ids, p, &t)?;
            *stage = PropStage::Property;
        }
        (PropStage::Value(p), Input::Value(v)) => {
            set_property(ctx, ids, p, &v.to_string())?;
            *stage = PropStage::Property;
        }
        (PropStage::Value(_), Input::Enter) => *stage = PropStage::Property,
        (_, Input::Enter) => return Ok(Step::Done),
        _ => return Err("Invalid input.".into()),
    }
    Ok(Step::Continue)
}

impl Command for ChProp {
    fn prompt(&self) -> Prompt {
        property_prompt(self.stage)
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if let (PropStage::Select, Input::Selection(s)) = (self.stage, &input) {
            if s.ids.is_empty() {
                return Ok(Step::Done);
            }
            self.ids = s.ids.clone();
            self.stage = PropStage::Property;
            return Ok(Step::Continue);
        }
        property_input(&mut self.stage, &self.ids, input, ctx)
    }

    fn wants_selection(&self) -> bool {
        self.stage == PropStage::Select
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, PropStage::Value(_))
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.ids.clone()
    }
}

#[derive(Clone, PartialEq)]
enum ChangeStage {
    Select,
    Point,
    Props(PropStage),
    /// A text's new insertion point (or Enter for the same), then its text.
    TextPoint(usize),
    TextValue(usize),
}

/// CHANGE (`-CH`): with a change point, lines' nearer ends move to it and
/// circles pass through it, and texts take a new place and text, one by
/// one; or the objects' Properties, as CHPROP.
pub struct Change {
    stage: ChangeStage,
    ids: Vec<EntityId>,
    texts: Vec<EntityId>,
}

impl Default for Change {
    fn default() -> Self {
        Self { stage: ChangeStage::Select, ids: Vec::new(), texts: Vec::new() }
    }
}

impl Change {
    /// On to the next text, or done.
    fn next_text(&mut self, i: usize) -> Step {
        if i + 1 < self.texts.len() {
            self.stage = ChangeStage::TextPoint(i + 1);
            Step::Continue
        } else {
            Step::Done
        }
    }
}

impl Command for Change {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            ChangeStage::Select => Prompt::new("Select objects"),
            ChangeStage::Point => Prompt::new("Specify change point or").with_keywords(&["Properties"]),
            ChangeStage::Props(s) => property_prompt(*s),
            ChangeStage::TextPoint(_) => Prompt::new("Specify new text insertion point <no change>"),
            ChangeStage::TextValue(_) => Prompt::new("Enter new text <no change>"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (ChangeStage::Select, Input::Selection(s)) => {
                if s.ids.is_empty() {
                    return Ok(Step::Done);
                }
                self.ids = s.ids;
                self.stage = ChangeStage::Point;
            }
            (ChangeStage::Point, Input::Keyword(_)) => self.stage = ChangeStage::Props(PropStage::Property),
            (ChangeStage::Point, Input::Point(to)) => {
                for id in &self.ids {
                    ctx.doc.update(*id, |e| match &mut e.geometry {
                        Geometry::Line { start, end } => {
                            if to.distance(*start) < to.distance(*end) {
                                *start = to;
                            } else {
                                *end = to;
                            }
                        }
                        Geometry::Circle { center, radius } if to.distance(*center) > 1e-9 => {
                            *radius = to.distance(*center);
                        }
                        _ => {}
                    });
                }
                self.texts = self
                    .ids
                    .iter()
                    .copied()
                    .filter(|id| matches!(ctx.doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Text { .. })))
                    .collect();
                if self.texts.is_empty() {
                    return Ok(Step::Done);
                }
                self.stage = ChangeStage::TextPoint(0);
            }
            (ChangeStage::Props(mut s), input) => {
                let step = property_input(&mut s, &self.ids, input, ctx)?;
                self.stage = ChangeStage::Props(s);
                return Ok(step);
            }
            (ChangeStage::TextPoint(i), input @ (Input::Point(_) | Input::Enter)) => {
                if let Input::Point(p) = input {
                    ctx.doc.update(self.texts[i], |e| {
                        if let Geometry::Text { position, .. } = &mut e.geometry {
                            *position = p;
                        }
                    });
                }
                self.stage = ChangeStage::TextValue(i);
            }
            (ChangeStage::TextValue(i), input @ (Input::Text(_) | Input::Enter)) => {
                if let Input::Text(t) = input {
                    ctx.doc.update(self.texts[i], |e| {
                        if let Geometry::Text { content, .. } = &mut e.geometry {
                            *content = t;
                        }
                    });
                }
                return Ok(self.next_text(i));
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        self.stage == ChangeStage::Select
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, ChangeStage::Props(PropStage::Value(_)) | ChangeStage::TextValue(_))
    }

    fn text_with_spaces(&self) -> bool {
        matches!(self.stage, ChangeStage::TextValue(_))
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.ids.clone()
    }
}

/// DBLIST: LIST for every object in the drawing.
#[derive(Default)]
pub struct DbList;

impl DbList {
    pub fn lines(doc: &oxidraft_model::Document) -> Vec<String> {
        doc.all_entities().flat_map(|e| crate::list::describe(e).into_iter().chain([String::new()])).collect()
    }
}

/// SOLID (`SO`): filled triangles and quadrilaterals, corner by corner
/// in AutoCAD's order (the third and fourth across from the first two);
/// each one's last edge starts the next.
#[derive(Default)]
pub struct Solid {
    points: Vec<Point>,
}

impl Solid {
    fn fill(corners: &[Point]) -> Geometry {
        // AutoCAD's order: 1, 2, 4, 3 around.
        let ring: Vec<Point> = match corners {
            [a, b, c, d] => vec![*a, *b, *d, *c],
            other => other.to_vec(),
        };
        Geometry::Hatch {
            loops: vec![ring.into_iter().map(Vertex::at).collect()],
            pattern: HatchPattern::Solid,
            scale: 1.0,
            angle: 0.0,
        }
    }
}

impl Command for Solid {
    fn prompt(&self) -> Prompt {
        Prompt::new(match self.points.len() {
            0 => "Specify first point",
            1 => "Specify second point",
            2 => "Specify third point",
            _ => "Specify fourth point or <exit>",
        })
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Point(p) => {
                self.points.push(p);
                if self.points.len() == 4 {
                    ctx.doc.add(Solid::fill(&self.points));
                    // The third and fourth points begin the next one.
                    self.points = self.points.split_off(2);
                }
                Ok(Step::Continue)
            }
            Input::Enter if self.points.len() == 3 => {
                ctx.doc.add(Solid::fill(&self.points));
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err("Specify a point.".into()),
        }
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let mut corners = self.points.clone();
        corners.push(cursor);
        match corners.len() {
            3 | 4 => vec![Solid::fill(&corners)],
            2 => vec![Geometry::Line { start: corners[0], end: corners[1] }],
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        self.points.last().copied()
    }
}

/// TXT2MTXT: the selected single-line texts as one multiline text, top
/// line first, at the top one's place and with its height.
#[derive(Default)]
pub struct Txt2Mtxt;

impl Command for Txt2Mtxt {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Ok(Step::Done) };
        let mut texts: Vec<(EntityId, Point, f64, f64, String)> = s
            .ids
            .iter()
            .filter_map(|id| match ctx.doc.entity(*id).map(|e| &e.geometry) {
                Some(Geometry::Text { position, height, rotation, content, .. }) => {
                    Some((*id, *position, *height, *rotation, content.clone()))
                }
                _ => None,
            })
            .collect();
        if texts.len() < 2 {
            return Err("Select two or more texts to combine.".into());
        }
        // Top to bottom, then left to right.
        texts.sort_by(|a, b| b.1.y.total_cmp(&a.1.y).then(a.1.x.total_cmp(&b.1.x)));
        let (first, position, height, rotation, _) = texts[0].clone();
        let content = texts.iter().map(|t| t.4.as_str()).collect::<Vec<_>>().join("\\P");
        let like = ctx.doc.entity(first).cloned().ok_or("Lost the text.")?;
        let look = match &like.geometry {
            Geometry::Text { look, .. } => look.clone(),
            _ => Default::default(),
        };
        // The top line's baseline is the multiline text's first line.
        let top = position + Point::ORIGIN.polar(height, rotation + 90.0);
        let merged =
            Geometry::Text { look, position: top, height, rotation, width: 0.0, attach: Attach::TopLeft, content };
        for t in &texts {
            ctx.doc.remove(t.0);
        }
        ctx.doc.add_like(merged, &like);
        ctx.messages.push(format!("{} texts combined.", texts.len()));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use oxidraft_model::{Color, Document};

    use super::*;
    use crate::Engine;

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for (i, s) in inputs.iter().enumerate() {
            if i == 0 {
                e.run(s, d);
            } else {
                e.submit(s, d, Point::ORIGIN, 0.5);
            }
        }
    }

    fn text(d: &mut Document, y: f64, s: &str) -> EntityId {
        d.add(Geometry::Text {
            look: Default::default(),
            position: Point::new(0.0, y),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: s.into(),
        })
    }

    #[test]
    fn chprop_and_change() {
        let mut d = Document::new();
        d.add_layer(oxidraft_model::Layer::new("Walls", 1));
        let line = d.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let circle = d.add(Geometry::Circle { center: Point::new(50.0, 0.0), radius: 2.0 });
        let mut e = Engine::new();
        run(&mut e, &mut d, &["chprop", "all", "", "c", "red", "la", "walls", "tr", "30", ""]);
        for id in [line, circle] {
            let x = d.entity(id).unwrap();
            assert_eq!(
                (x.color, x.layer.as_str(), x.transparency),
                (Color::Index(1), "Walls", Transparency::Percent(30))
            );
        }
        assert!(!e.is_active());
        run(&mut e, &mut d, &["change", "all", "", "12,3"]);
        assert!(matches!(d.entity(line).unwrap().geometry, Geometry::Line { end, .. } if end == Point::new(12.0, 3.0)));
        let r = match d.entity(circle).unwrap().geometry {
            Geometry::Circle { radius, .. } => radius,
            _ => 0.0,
        };
        assert!((r - Point::new(12.0, 3.0).distance(Point::new(50.0, 0.0))).abs() < 1e-9);
    }

    #[test]
    fn change_edits_text_and_txt2mtxt_combines() {
        let mut d = Document::new();
        let a = text(&mut d, 10.0, "FIRST");
        text(&mut d, 5.0, "SECOND");
        let mut e = Engine::new();
        run(&mut e, &mut d, &["change", "all", "", "1,1", "", "TOP LINE", "", "LOWER"]);
        assert!(matches!(&d.entity(a).unwrap().geometry, Geometry::Text { content, .. } if content == "TOP LINE"));
        run(&mut e, &mut d, &["txt2mtxt", "all", ""]);
        let texts: Vec<String> = d
            .entities()
            .filter_map(|x| match &x.geometry {
                Geometry::Text { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["TOP LINE\\PLOWER"]);
    }

    #[test]
    fn solid_fills_in_autocads_order() {
        let mut d = Document::new();
        let mut e = Engine::new();
        run(&mut e, &mut d, &["solid", "0,0", "10,0", "0,10", "10,10", "0,20", ""]);
        let fills: Vec<Vec<Point>> = d
            .entities()
            .filter_map(|x| match &x.geometry {
                Geometry::Hatch { loops, .. } => Some(loops[0].iter().map(|v| v.point).collect()),
                _ => None,
            })
            .collect();
        assert_eq!(fills.len(), 2, "a square, then a triangle off its far edge");
        assert_eq!(
            fills[0],
            [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)]
        );
        assert_eq!(fills[1].len(), 3);
    }
}

//! SKETCH: freehand lines, recorded as the cursor moves; TEXTALIGN: texts
//! lined up with another; ATTREDEF: a block redefined from new objects,
//! its references' attributes kept.

use std::cell::RefCell;

use oxidraft_geom::Point;
use oxidraft_model::{Block, BlockEntity, EntityId, Geometry, Spline, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Clone, Copy, PartialEq)]
enum SketchType {
    Line,
    Polyline,
    Spline,
}

#[derive(Clone, Copy, PartialEq)]
enum SketchStage {
    Sketch,
    Type,
    Increment,
}

/// SKETCH: click to put the pen down, move to draw, click to lift it;
/// each stroke a polyline (or lines, or a spline) of points a record
/// increment apart. Enter ends it.
pub struct Sketch {
    stage: SketchStage,
    kind: SketchType,
    increment: f64,
    /// The stroke being drawn while the pen is down, added to as the
    /// cursor moves (which the preview sees).
    stroke: RefCell<Option<Vec<Point>>>,
}

impl Default for Sketch {
    fn default() -> Self {
        Self { stage: SketchStage::Sketch, kind: SketchType::Polyline, increment: 1.0, stroke: RefCell::new(None) }
    }
}

impl Sketch {
    /// The pen lifted: the stroke drawn, if it has a length.
    fn lift(&self, ctx: &mut Ctx, last: Option<Point>) -> usize {
        let Some(mut points) = self.stroke.borrow_mut().take() else { return 0 };
        if let Some(p) = last.filter(|p| points.last().is_none_or(|q| q.distance(*p) > 1e-9)) {
            points.push(p);
        }
        if points.len() < 2 {
            return 0;
        }
        match self.kind {
            SketchType::Line => {
                for w in points.windows(2) {
                    ctx.doc.add(Geometry::Line { start: w[0], end: w[1] });
                }
            }
            SketchType::Polyline => {
                ctx.doc.add(Geometry::Polyline {
                    vertices: points.iter().map(|p| Vertex::at(*p)).collect(),
                    closed: false,
                });
            }
            SketchType::Spline => {
                let Some(spline) = Spline::through(&points, false) else { return 0 };
                ctx.doc.add(Geometry::Spline(spline));
            }
        }
        1
    }
}

impl Command for Sketch {
    fn start(&mut self, _vars: &crate::Vars) -> Option<String> {
        Some(format!("Type = Polyline   Increment = {:.4}", self.increment))
    }

    fn prompt(&self) -> Prompt {
        match self.stage {
            SketchStage::Sketch => Prompt::new("Specify sketch or").with_keywords(&["Type", "Increment"]),
            SketchStage::Type => {
                Prompt::new("Enter sketch type").with_keywords(&["Line", "Polyline", "Spline"]).with_default("Polyline")
            }
            SketchStage::Increment => {
                Prompt::new("Specify sketch increment").with_default(format!("{:.4}", self.increment))
            }
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage, input) {
            (SketchStage::Sketch, Input::Point(p)) => {
                let down = self.stroke.borrow().is_some();
                if down {
                    self.lift(ctx, Some(p));
                } else {
                    *self.stroke.borrow_mut() = Some(vec![p]);
                }
            }
            (SketchStage::Sketch, Input::Keyword("Type")) => self.stage = SketchStage::Type,
            (SketchStage::Sketch, Input::Keyword(_)) => self.stage = SketchStage::Increment,
            (SketchStage::Sketch, Input::Enter) => {
                self.lift(ctx, None);
                return Ok(Step::Done);
            }
            (SketchStage::Type, input @ (Input::Keyword(_) | Input::Enter)) => {
                self.kind = match input {
                    Input::Keyword("Line") => SketchType::Line,
                    Input::Keyword("Spline") => SketchType::Spline,
                    _ => SketchType::Polyline,
                };
                self.stage = SketchStage::Sketch;
            }
            (SketchStage::Increment, Input::Value(v)) if v > 0.0 => {
                self.increment = v;
                self.stage = SketchStage::Sketch;
            }
            (SketchStage::Increment, Input::Enter) => self.stage = SketchStage::Sketch,
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    /// While the pen is down, the cursor's path is recorded, a point each
    /// increment.
    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        let mut stroke = self.stroke.borrow_mut();
        let Some(points) = stroke.as_mut() else { return Vec::new() };
        if points.last().is_some_and(|q| q.distance(cursor) >= self.increment) {
            points.push(cursor);
        }
        let mut shown: Vec<Vertex> = points.iter().map(|p| Vertex::at(*p)).collect();
        shown.push(Vertex::at(cursor));
        vec![Geometry::Polyline { vertices: shown, closed: false }]
    }
}

/// TEXTALIGN: the selected texts moved onto the line through another
/// text's insertion point, along a direction picked, each keeping its
/// place along it.
#[derive(Default)]
pub struct TextAlign {
    texts: Vec<EntityId>,
    to: Option<Point>,
}

fn text_position(ctx: &Ctx, id: EntityId) -> Option<Point> {
    match ctx.doc.entity(id).map(|e| &e.geometry) {
        Some(Geometry::Text { position, .. }) => Some(*position),
        _ => None,
    }
}

impl Command for TextAlign {
    fn prompt(&self) -> Prompt {
        match (self.texts.is_empty(), self.to) {
            (true, _) => Prompt::new("Select text objects to align"),
            (false, None) => Prompt::new("Select text object to align to"),
            (false, Some(_)) => Prompt::new("Pick second point"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.texts.is_empty(), self.to, input) {
            (true, _, Input::Selection(s)) => {
                self.texts = s.ids.iter().copied().filter(|id| text_position(ctx, *id).is_some()).collect();
                if self.texts.is_empty() {
                    return Ok(Step::Done);
                }
            }
            (false, None, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("Nothing there.")?;
                self.to = Some(text_position(ctx, id).ok_or("That isn't text.")?);
            }
            (false, Some(base), Input::Point(p)) => {
                let d = p - base;
                let len = d.length();
                if len < 1e-9 {
                    return Err("Pick a point away from the first.".into());
                }
                let d = d * (1.0 / len);
                for id in &self.texts {
                    ctx.doc.update(*id, |e| {
                        if let Geometry::Text { position, .. } = &mut e.geometry {
                            *position = base + d * (*position - base).dot(d);
                        }
                    });
                }
                ctx.messages.push(format!("{} text(s) aligned.", self.texts.len()));
                return Ok(Step::Done);
            }
            (_, _, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        self.texts.is_empty()
    }

    fn wants_pick(&self) -> bool {
        !self.texts.is_empty() && self.to.is_none()
    }

    fn base_point(&self) -> Option<Point> {
        self.to
    }

    fn highlighted(&self) -> Vec<EntityId> {
        self.texts.clone()
    }
}

#[derive(Clone, PartialEq)]
enum RedefStage {
    Name,
    Select(String),
    Base(String, Vec<EntityId>),
}

/// ATTREDEF: a block redefined from the objects selected (attribute
/// definitions among them), its references keeping the values of the
/// attributes still defined, and new ones at their defaults.
pub struct AttRedef {
    stage: RedefStage,
}

impl Default for AttRedef {
    fn default() -> Self {
        Self { stage: RedefStage::Name }
    }
}

impl Command for AttRedef {
    fn prompt(&self) -> Prompt {
        match self.stage {
            RedefStage::Name => Prompt::new("Enter name of the block you wish to redefine"),
            RedefStage::Select(_) => Prompt::new("Select objects for new Block"),
            RedefStage::Base(..) => Prompt::new("Specify insertion base point of new Block"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (RedefStage::Name, Input::Text(t)) => {
                let name = ctx.doc.block(t.trim()).map(|b| b.name.clone()).ok_or("No such block.")?;
                self.stage = RedefStage::Select(name);
            }
            (RedefStage::Select(name), Input::Selection(s)) => {
                if s.ids.is_empty() {
                    return Ok(Step::Done);
                }
                self.stage = RedefStage::Base(name, s.ids);
            }
            (RedefStage::Base(name, ids), Input::Point(base)) => {
                let entities: Vec<BlockEntity> = ids
                    .iter()
                    .filter_map(|id| ctx.doc.entity(*id))
                    .map(|e| BlockEntity {
                        layer: e.layer.clone(),
                        color: e.color,
                        linetype: e.linetype.clone(),
                        lineweight: e.lineweight,
                        geometry: e.geometry.clone(),
                    })
                    .collect();
                ctx.doc.define_block(Block { name: name.clone(), base, entities });
                for id in &ids {
                    ctx.doc.remove(*id);
                }
                let n = crate::attdef::sync_attributes(ctx.doc, &name);
                ctx.messages.push(format!("Block \"{name}\" redefined; {n} reference(s) updated."));
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        self.stage == RedefStage::Name
    }

    fn wants_selection(&self) -> bool {
        matches!(self.stage, RedefStage::Select(_))
    }
}

#[cfg(test)]
mod tests {
    use oxidraft_model::{Attach, Document, Placement};

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

    #[test]
    fn sketch_records_strokes() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        run(&mut e, &mut d, &["sketch", "0,0"]);
        for x in 1..=10 {
            e.preview(Point::new(f64::from(x) * 0.5, 0.0));
        }
        e.submit("6,0", &mut d, Point::ORIGIN, 0.5);
        e.submit("", &mut d, Point::ORIGIN, 0.5);
        let strokes: Vec<usize> = d
            .entities()
            .filter_map(|x| match &x.geometry {
                Geometry::Polyline { vertices, .. } => Some(vertices.len()),
                _ => None,
            })
            .collect();
        // A point each unit, and where the pen came up.
        assert_eq!(strokes, [7]);
    }

    #[test]
    fn textalign_lines_texts_up() {
        let mut d = Document::new();
        let text = |d: &mut Document, x: f64, y: f64| {
            d.add(Geometry::Text {
                look: Default::default(),
                position: Point::new(x, y),
                height: 2.5,
                rotation: 0.0,
                width: 0.0,
                attach: Attach::BottomLeft,
                content: "NOTE".into(),
            })
        };
        let a = text(&mut d, 0.0, 0.0);
        let b = text(&mut d, 3.0, -10.0);
        let mut e = Engine::new();
        run(&mut e, &mut d, &["textalign", "all", ""]);
        let p = Point::new(1.0, 1.0);
        e.click(p, p, false, &mut d, 0.5);
        e.submit("0,-20", &mut d, Point::ORIGIN, 0.5);
        assert!(
            matches!(d.entity(b).unwrap().geometry, Geometry::Text { position, .. } if position.approx_eq(Point::new(0.0, -10.0)))
        );
        assert!(
            matches!(d.entity(a).unwrap().geometry, Geometry::Text { position, .. } if position.approx_eq(Point::ORIGIN))
        );
    }

    #[test]
    fn attredef_keeps_values() {
        let mut d = Document::new();
        let attdef = |tag: &str, default: &str| Geometry::AttributeDefinition {
            tag: tag.into(),
            prompt: tag.into(),
            default: default.into(),
            position: Point::ORIGIN,
            height: 2.0,
            rotation: 0.0,
            attach: Attach::BottomLeft,
            invisible: false,
            constant: false,
        };
        let line = |x: f64| BlockEntity {
            layer: "0".into(),
            color: Default::default(),
            linetype: "ByLayer".into(),
            lineweight: Default::default(),
            geometry: Geometry::Line { start: Point::ORIGIN, end: Point::new(x, 0.0) },
        };
        let tag = |g: Geometry| BlockEntity { geometry: g, ..line(0.0) };
        d.define_block(Block {
            name: "TAG".into(),
            base: Point::ORIGIN,
            entities: vec![line(5.0), tag(attdef("NO", "1"))],
        });
        let placement = Placement::at(Point::new(100.0, 0.0));
        let def = d.block("TAG").unwrap().clone();
        let mut attributes = def.attributes_for(&placement, |_| None);
        attributes[0].value = "42".into();
        let reference = d.add(Geometry::Insert { block: "TAG".into(), placement, attributes, clip: None });
        // The new definition: a longer line, the same tag and a new one.
        d.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(9.0, 0.0) });
        d.add(attdef("NO", "1"));
        d.add(attdef("ROOM", "A"));
        let mut e = Engine::new();
        run(&mut e, &mut d, &["attredef", "tag", "w", "-1,-5", "20,5", "", "0,0"]);
        match &d.entity(reference).unwrap().geometry {
            Geometry::Insert { attributes, .. } => {
                let values: Vec<(&str, &str)> = attributes.iter().map(|a| (a.tag.as_str(), a.value.as_str())).collect();
                assert_eq!(values, [("NO", "42"), ("ROOM", "A")]);
            }
            g => panic!("{g:?}"),
        }
    }
}

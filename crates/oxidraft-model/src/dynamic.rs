//! Dynamic blocks: parameters authored in the block editor (BPARAMETER,
//! BACTION) and visibility states (BVSTATE, BVHIDE, BVSHOW), and the
//! values each reference has for them. A reference whose values differ
//! from the defaults shows an anonymous block made from the definition
//! with them applied, as AutoCAD's dynamic references do; the definition
//! stays as it was.

use oxidraft_geom::{Aabb, Point};

use crate::document::Change;
use crate::{Block, BlockEntity, Document, EntityId, Geometry};

/// A parameter and what it does to the block.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    /// A distance from `base` toward `end`; what's inside `stretch`
    /// moves along with its end (a stretch action).
    Linear { name: String, base: Point, end: Point, stretch: Option<Aabb> },
    /// 1 flips the block over the line from `base` to `end`.
    Flip { name: String, base: Point, end: Point },
    /// An angle (degrees) the block turns about `base`.
    Rotation { name: String, base: Point, radius: f64 },
}

impl Param {
    pub fn name(&self) -> &str {
        match self {
            Param::Linear { name, .. } | Param::Flip { name, .. } | Param::Rotation { name, .. } => name,
        }
    }

    /// The value it has in the definition.
    pub fn default_value(&self) -> f64 {
        match self {
            Param::Linear { base, end, .. } => base.distance(*end),
            Param::Flip { .. } | Param::Rotation { .. } => 0.0,
        }
    }
}

/// A visibility state: its name, and the objects (by place in the
/// block) it hides.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VisibilityState {
    pub name: String,
    pub hidden: Vec<usize>,
}

/// What makes a block dynamic.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DynamicBlock {
    pub params: Vec<Param>,
    pub states: Vec<VisibilityState>,
    /// The state being authored in the block editor.
    pub current_state: usize,
}

/// A reference's values: the dynamic block it's of, its parameters'
/// values and its visibility state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DynamicValues {
    pub block: String,
    pub values: Vec<(String, f64)>,
    pub visibility: Option<String>,
}

impl DynamicValues {
    pub fn value(&self, name: &str) -> Option<f64> {
        self.values.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| *v)
    }
}

/// Names and states written in text: `|` and line breaks kept out.
fn clean(s: &str) -> String {
    s.replace(['|', '\n', '\r'], " ")
}

impl DynamicBlock {
    /// The definition as lines of text, for files: a line a parameter
    /// (L, F or R and its numbers) or visibility state (V, its name and the
    /// objects it hides), then the state being authored.
    pub fn to_text(&self) -> String {
        let mut out = Vec::new();
        for p in &self.params {
            out.push(match p {
                Param::Linear { name, base, end, stretch } => {
                    let frame =
                        stretch.map_or(String::new(), |f| format!("|{}|{}|{}|{}", f.min.x, f.min.y, f.max.x, f.max.y));
                    format!("L|{}|{}|{}|{}|{}{frame}", clean(name), base.x, base.y, end.x, end.y)
                }
                Param::Flip { name, base, end } => {
                    format!("F|{}|{}|{}|{}|{}", clean(name), base.x, base.y, end.x, end.y)
                }
                Param::Rotation { name, base, radius } => format!("R|{}|{}|{}|{}", clean(name), base.x, base.y, radius),
            });
        }
        for s in &self.states {
            let hidden: Vec<String> = s.hidden.iter().map(usize::to_string).collect();
            out.push(format!("V|{}|{}", clean(&s.name), hidden.join(",")));
        }
        out.push(format!("C|{}", self.current_state));
        out.join("\n")
    }

    /// Reads [`DynamicBlock::to_text`]'s text back.
    pub fn from_text(text: &str) -> Option<DynamicBlock> {
        let mut d = DynamicBlock::default();
        for line in text.lines() {
            let f: Vec<&str> = line.split('|').collect();
            let n = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok());
            match f.first().copied() {
                Some("L") => {
                    let stretch = match (n(6), n(7), n(8), n(9)) {
                        (Some(a), Some(b), Some(c), Some(e)) => {
                            Some(Aabb::from_points(Point::new(a, b), Point::new(c, e)))
                        }
                        _ => None,
                    };
                    d.params.push(Param::Linear {
                        name: f.get(1)?.to_string(),
                        base: Point::new(n(2)?, n(3)?),
                        end: Point::new(n(4)?, n(5)?),
                        stretch,
                    });
                }
                Some("F") => d.params.push(Param::Flip {
                    name: f.get(1)?.to_string(),
                    base: Point::new(n(2)?, n(3)?),
                    end: Point::new(n(4)?, n(5)?),
                }),
                Some("R") => d.params.push(Param::Rotation {
                    name: f.get(1)?.to_string(),
                    base: Point::new(n(2)?, n(3)?),
                    radius: n(4)?,
                }),
                Some("V") => d.states.push(VisibilityState {
                    name: f.get(1)?.to_string(),
                    hidden: f.get(2).map_or_else(Vec::new, |h| h.split(',').filter_map(|v| v.parse().ok()).collect()),
                }),
                Some("C") => d.current_state = n(1).map_or(0, |v| v as usize),
                _ => {}
            }
        }
        Some(d)
    }

    /// The block's objects with `values` applied.
    pub fn apply(&self, entities: &[BlockEntity], values: &DynamicValues) -> Vec<BlockEntity> {
        let hidden: &[usize] = values
            .visibility
            .as_deref()
            .and_then(|v| self.states.iter().find(|s| s.name.eq_ignore_ascii_case(v)))
            .or_else(|| self.states.first())
            .map_or(&[], |s| &s.hidden);
        let mut out: Vec<BlockEntity> =
            entities.iter().enumerate().filter(|(i, _)| !hidden.contains(i)).map(|(_, e)| e.clone()).collect();
        for p in &self.params {
            let v = values.value(p.name()).unwrap_or_else(|| p.default_value());
            match p {
                Param::Linear { base, end, stretch: Some(frame), .. } => {
                    let d = *end - *base;
                    let len = d.length();
                    if len <= 0.0 {
                        continue;
                    }
                    let shift = d * ((v - len) / len);
                    for e in &mut out {
                        e.geometry = e.geometry.stretched(std::slice::from_ref(frame), shift);
                    }
                }
                Param::Linear { .. } => {}
                Param::Flip { base, end, .. } if v != 0.0 => {
                    for e in &mut out {
                        e.geometry = e.geometry.mirrored(*base, *end);
                    }
                }
                Param::Flip { .. } => {}
                Param::Rotation { base, .. } if v != 0.0 => {
                    for e in &mut out {
                        e.geometry = e.geometry.rotated(*base, v);
                    }
                }
                Param::Rotation { .. } => {}
            }
        }
        out
    }
}

impl Document {
    /// Block `name`'s dynamic definition, if it has one.
    pub fn dynamic_block(&self, name: &str) -> Option<&DynamicBlock> {
        self.dynamic_blocks.get(&name.to_ascii_uppercase())
    }

    pub fn all_dynamic_blocks(&self) -> impl Iterator<Item = (&String, &DynamicBlock)> {
        self.dynamic_blocks.iter()
    }

    /// Gives block `name` a dynamic definition, or takes it away.
    pub fn set_dynamic_block(&mut self, name: &str, def: Option<DynamicBlock>) {
        let key = name.to_ascii_uppercase();
        let before = match &def {
            Some(d) => self.dynamic_blocks.insert(key.clone(), d.clone()),
            None => self.dynamic_blocks.remove(&key),
        };
        if before != def {
            self.record(Change::Dynamic { key, before, after: def });
        }
    }

    /// The dynamic block a reference is of, and its values.
    pub fn dynamic_values(&self, id: EntityId) -> Option<DynamicValues> {
        let e = self.entity(id)?;
        if let Some(v) = e.dynamic.as_deref() {
            return Some(v.clone());
        }
        let Geometry::Insert { block, .. } = &e.geometry else { return None };
        self.dynamic_block(block).map(|_| DynamicValues { block: block.clone(), ..Default::default() })
    }

    /// Gives reference `id` these values: it shows the dynamic block with
    /// them applied (an anonymous block), or the block itself at the
    /// defaults.
    pub fn set_dynamic_values(&mut self, id: EntityId, values: DynamicValues) -> bool {
        let (Some(def), Some(block)) = (self.dynamic_block(&values.block).cloned(), self.block(&values.block).cloned())
        else {
            return false;
        };
        let Some(Geometry::Insert { block: shown, .. }) = self.entity(id).map(|e| e.geometry.clone()) else {
            return false;
        };
        let at_defaults =
            def.params.iter().all(|p| values.value(p.name()).is_none_or(|v| (v - p.default_value()).abs() < 1e-9))
                && values
                    .visibility
                    .as_deref()
                    .is_none_or(|v| def.states.first().is_some_and(|s| s.name.eq_ignore_ascii_case(v)));
        let target = if at_defaults {
            block.name.clone()
        } else {
            let anon = Block {
                name: self.unused_anonymous_name(),
                base: block.base,
                entities: def.apply(&block.entities, &values),
            };
            let name = anon.name.clone();
            self.define_block(anon);
            name
        };
        self.update(id, |e| {
            if let Geometry::Insert { block, .. } = &mut e.geometry {
                *block = target;
            }
            e.dynamic = (!at_defaults).then(|| Box::new(values));
        });
        if shown.starts_with('*') && !shown.eq_ignore_ascii_case(&block.name) {
            self.remove_unused_block(&shown);
        }
        true
    }

    /// The objects of the block being edited in the block editor, in the
    /// order it'll be saved in: (id, place).
    pub fn block_editor_order(&self) -> Vec<EntityId> {
        let Some(r) = self.refedit() else { return Vec::new() };
        self.entities_in_draw_order().into_iter().filter(|e| r.in_working_set(e.id)).map(|e| e.id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Placement};

    fn door() -> Document {
        let mut d = Document::new();
        let leaf = |a: (f64, f64), b: (f64, f64)| {
            BlockEntity::new(
                "0",
                Color::ByBlock,
                Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) },
            )
        };
        d.define_block(Block {
            name: "DOOR".into(),
            base: Point::ORIGIN,
            entities: vec![leaf((0.0, 0.0), (900.0, 0.0)), leaf((0.0, 0.0), (0.0, 900.0))],
        });
        let def = DynamicBlock {
            params: vec![
                Param::Linear {
                    name: "Width".into(),
                    base: Point::ORIGIN,
                    end: Point::new(900.0, 0.0),
                    stretch: Some(Aabb::from_points(Point::new(800.0, -10.0), Point::new(1000.0, 10.0))),
                },
                Param::Flip { name: "Flip".into(), base: Point::ORIGIN, end: Point::new(0.0, 100.0) },
            ],
            states: vec![
                VisibilityState { name: "Open".into(), hidden: vec![] },
                VisibilityState { name: "Closed".into(), hidden: vec![1] },
            ],
            current_state: 0,
        };
        assert_eq!(DynamicBlock::from_text(&def.to_text()), Some(def.clone()));
        d.set_dynamic_block("DOOR", Some(def));
        d
    }

    fn lines(d: &Document, id: EntityId) -> Vec<(Point, Point)> {
        d.expanded(d.entity(id).unwrap())
            .into_iter()
            .filter_map(|g| match g {
                Geometry::Line { start, end } => Some((start, end)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn references_take_their_own_values() {
        let mut d = door();
        let id = d.add(Geometry::Insert {
            block: "DOOR".into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: vec![],
            clip: None,
        });
        let mut v = d.dynamic_values(id).unwrap();
        v.values = vec![("Width".into(), 1000.0)];
        v.visibility = Some("Closed".into());
        d.begin("width");
        assert!(d.set_dynamic_values(id, v.clone()));
        d.commit();
        let l = lines(&d, id);
        assert_eq!(l.len(), 1, "the closed state hides the swing");
        assert!(l[0].1.approx_eq(Point::new(1000.0, 0.0)), "stretched: {l:?}");
        assert!(d.block("DOOR").unwrap().entities.len() == 2, "the definition stays");
        // Flipped, then back to the defaults.
        v.values.push(("Flip".into(), 1.0));
        d.set_dynamic_values(id, v.clone());
        assert!(lines(&d, id)[0].1.approx_eq(Point::new(-1000.0, 0.0)));
        d.set_dynamic_values(id, DynamicValues { block: "DOOR".into(), ..Default::default() });
        assert!(matches!(&d.entity(id).unwrap().geometry, Geometry::Insert { block, .. } if block == "DOOR"));
        assert_eq!(lines(&d, id).len(), 2);
    }
}

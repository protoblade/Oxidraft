//! Dynamic blocks: parameters authored in the block editor (BPARAMETER,
//! BACTION, BLOOKUPTABLE) and visibility states (BVSTATE, BVHIDE,
//! BVSHOW), and the values each reference has for them. A reference whose
//! values differ from the defaults shows an anonymous block made from the
//! definition with them applied, as AutoCAD's dynamic references do; the
//! definition stays as it was. Each parameter has properties (a point's X
//! and Y, a polar one's Distance and Angle), set in the Properties palette,
//! by -DYNPROP or by dragging the parameter's grip.

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
    /// A position (its X and Y); the objects of the move action (by place
    /// in the block) move from `base` with it.
    Point { name: String, base: Point, objects: Vec<usize> },
    /// A distance and angle from `base`; what's inside `stretch` moves
    /// with its end.
    Polar { name: String, base: Point, end: Point, stretch: Option<Aabb> },
    /// A horizontal and vertical distance from `base`; what's inside
    /// `stretch` moves with its corner.
    Xy { name: String, base: Point, end: Point, stretch: Option<Aabb> },
    /// An angle (degrees) the block lines up at, turning about `base`:
    /// its grip dropped on an object lines the block up with it.
    Alignment { name: String, base: Point, end: Point },
    /// A row of the lookup table, by its label: the values it gives other
    /// parameters' properties.
    Lookup { name: String, base: Point, rows: Vec<(String, Vec<(String, f64)>)> },
    /// An array action on a linear or XY parameter (`follows`): the objects
    /// repeat every `spacing` along its distance.
    Array { name: String, follows: String, objects: Vec<usize>, spacing: f64 },
}

impl Param {
    pub fn name(&self) -> &str {
        match self {
            Param::Linear { name, .. }
            | Param::Flip { name, .. }
            | Param::Rotation { name, .. }
            | Param::Point { name, .. }
            | Param::Polar { name, .. }
            | Param::Xy { name, .. }
            | Param::Alignment { name, .. }
            | Param::Lookup { name, .. }
            | Param::Array { name, .. } => name,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Param::Linear { .. } => "Linear",
            Param::Flip { .. } => "Flip",
            Param::Rotation { .. } => "Rotation",
            Param::Point { .. } => "Point",
            Param::Polar { .. } => "Polar",
            Param::Xy { .. } => "XY",
            Param::Alignment { .. } => "Alignment",
            Param::Lookup { .. } => "Lookup",
            Param::Array { .. } => "Array",
        }
    }

    /// Its properties and the values they have in the definition: one for
    /// most, a point's X and Y, a polar one's Distance and Angle, an XY
    /// one's Horizontal and Vertical; an array action none of its own.
    pub fn properties(&self) -> Vec<(String, f64)> {
        let two = |a: &str, b: &str, va: f64, vb: f64| {
            vec![(format!("{} {a}", self.name()), va), (format!("{} {b}", self.name()), vb)]
        };
        match self {
            Param::Linear { base, end, .. } => vec![(self.name().to_string(), base.distance(*end))],
            Param::Flip { .. } | Param::Rotation { .. } | Param::Alignment { .. } | Param::Lookup { .. } => {
                vec![(self.name().to_string(), 0.0)]
            }
            Param::Point { base, .. } => two("X", "Y", base.x, base.y),
            Param::Polar { base, end, .. } => two("Distance", "Angle", base.distance(*end), base.angle_to(*end)),
            Param::Xy { base, end, .. } => two("Horizontal", "Vertical", end.x - base.x, end.y - base.y),
            Param::Array { .. } => Vec::new(),
        }
    }

    /// The value it has in the definition (its first property's).
    pub fn default_value(&self) -> f64 {
        self.properties().first().map_or(0.0, |p| p.1)
    }

    /// Whether a property is an angle (shown in degrees).
    pub fn is_angle(&self, property: &str) -> bool {
        matches!(self, Param::Rotation { .. } | Param::Alignment { .. })
            || matches!(self, Param::Polar { .. }) && property.ends_with(" Angle")
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

/// `d` one long, the same way.
fn unit(d: Point) -> Point {
    let len = d.length();
    if len > 0.0 { d * (1.0 / len) } else { Point::new(1.0, 0.0) }
}

/// Objects by place, written `0,2,3`.
fn list(objects: &[usize]) -> String {
    objects.iter().map(usize::to_string).collect::<Vec<_>>().join(",")
}

fn places(s: Option<&&str>) -> Vec<usize> {
    s.map_or_else(Vec::new, |h| h.split(',').filter_map(|v| v.parse().ok()).collect())
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
                Param::Point { name, base, objects } => {
                    format!("P|{}|{}|{}|{}", clean(name), base.x, base.y, list(objects))
                }
                Param::Polar { name, base, end, stretch } | Param::Xy { name, base, end, stretch } => {
                    let frame =
                        stretch.map_or(String::new(), |f| format!("|{}|{}|{}|{}", f.min.x, f.min.y, f.max.x, f.max.y));
                    let letter = if matches!(p, Param::Polar { .. }) { "O" } else { "X" };
                    format!("{letter}|{}|{}|{}|{}|{}{frame}", clean(name), base.x, base.y, end.x, end.y)
                }
                Param::Alignment { name, base, end } => {
                    format!("A|{}|{}|{}|{}|{}", clean(name), base.x, base.y, end.x, end.y)
                }
                // Rows: label, then property=value pairs, `;` between.
                Param::Lookup { name, base, rows } => {
                    let rows: Vec<String> = rows
                        .iter()
                        .map(|(label, values)| {
                            std::iter::once(clean(label).replace([';', '='], " "))
                                .chain(values.iter().map(|(k, v)| format!("{}={v}", clean(k).replace([';', '='], " "))))
                                .collect::<Vec<_>>()
                                .join(";")
                        })
                        .collect();
                    format!("K|{}|{}|{}|{}", clean(name), base.x, base.y, rows.join("|"))
                }
                Param::Array { name, follows, objects, spacing } => {
                    format!("Y|{}|{}|{}|{}", clean(name), clean(follows), spacing, list(objects))
                }
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
                Some("P") => d.params.push(Param::Point {
                    name: f.get(1)?.to_string(),
                    base: Point::new(n(2)?, n(3)?),
                    objects: places(f.get(4)),
                }),
                Some(letter @ ("O" | "X")) => {
                    let stretch = match (n(6), n(7), n(8), n(9)) {
                        (Some(a), Some(b), Some(c), Some(e)) => {
                            Some(Aabb::from_points(Point::new(a, b), Point::new(c, e)))
                        }
                        _ => None,
                    };
                    let (name, base, end) = (f.get(1)?.to_string(), Point::new(n(2)?, n(3)?), Point::new(n(4)?, n(5)?));
                    d.params.push(if letter == "O" {
                        Param::Polar { name, base, end, stretch }
                    } else {
                        Param::Xy { name, base, end, stretch }
                    });
                }
                Some("A") => d.params.push(Param::Alignment {
                    name: f.get(1)?.to_string(),
                    base: Point::new(n(2)?, n(3)?),
                    end: Point::new(n(4)?, n(5)?),
                }),
                Some("K") => {
                    let rows = f[4.min(f.len())..]
                        .iter()
                        .filter(|r| !r.is_empty())
                        .map(|r| {
                            let mut parts = r.split(';');
                            let label = parts.next().unwrap_or("").to_string();
                            let values = parts
                                .filter_map(|kv| kv.split_once('='))
                                .filter_map(|(k, v)| Some((k.to_string(), v.parse().ok()?)))
                                .collect();
                            (label, values)
                        })
                        .collect();
                    d.params.push(Param::Lookup { name: f.get(1)?.to_string(), base: Point::new(n(2)?, n(3)?), rows });
                }
                Some("Y") => d.params.push(Param::Array {
                    name: f.get(1)?.to_string(),
                    follows: f.get(2)?.to_string(),
                    spacing: n(3)?,
                    objects: places(f.get(4)),
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
        // A lookup's row sets other properties first.
        let mut values = values.clone();
        for p in &self.params {
            if let Param::Lookup { name, rows, .. } = p
                && let Some(row) = values.value(name).and_then(|i| rows.get(i.max(0.0) as usize))
            {
                for (k, v) in &row.1 {
                    values.values.retain(|(n, _)| !n.eq_ignore_ascii_case(k));
                    values.values.push((k.clone(), *v));
                }
            }
        }
        let values = &values;
        let get = |p: &Param, i: usize| {
            let props = p.properties();
            props.get(i).map_or(0.0, |(k, d)| values.value(k).unwrap_or(*d))
        };
        // Array actions first, on the objects as they are: copies along
        // their parameter's direction, as many as its distance holds.
        let mut out: Vec<BlockEntity> = Vec::new();
        let mut copies: Vec<BlockEntity> = Vec::new();
        for (i, e) in entities.iter().enumerate() {
            if hidden.contains(&i) {
                continue;
            }
            out.push(e.clone());
            for p in &self.params {
                let Param::Array { follows, objects, spacing, .. } = p else { continue };
                if !objects.contains(&i) || *spacing <= 0.0 {
                    continue;
                }
                let Some(f) = self.params.iter().find(|q| q.name().eq_ignore_ascii_case(follows)) else { continue };
                let (dir, length) = match f {
                    Param::Linear { base, end, .. } => (unit(*end - *base), get(f, 0)),
                    Param::Xy { .. } => (Point::new(1.0, 0.0), get(f, 0).abs()),
                    _ => continue,
                };
                let n = (length / spacing + 1e-9).floor() as usize;
                for k in 1..=n {
                    copies
                        .push(BlockEntity { geometry: e.geometry.translated(dir * (spacing * k as f64)), ..e.clone() });
                }
            }
        }
        out.extend(copies);
        for p in &self.params {
            let v = get(p, 0);
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
                // The move action: its objects (by place) move from the base.
                Param::Point { base, objects, .. } => {
                    let shift = Point::new(get(p, 0), get(p, 1)) - *base;
                    if shift.length() > 0.0 {
                        let shown: Vec<usize> = (0..entities.len()).filter(|i| !hidden.contains(i)).collect();
                        for (slot, i) in shown.iter().enumerate() {
                            if objects.contains(i) {
                                out[slot].geometry = out[slot].geometry.translated(shift);
                            }
                        }
                    }
                }
                Param::Polar { base, end, stretch: Some(frame), .. } => {
                    let to = base.polar(get(p, 0), get(p, 1));
                    let shift = to - *end;
                    for e in &mut out {
                        e.geometry = e.geometry.stretched(std::slice::from_ref(frame), shift);
                    }
                }
                Param::Xy { base, end, stretch: Some(frame), .. } => {
                    let to = *base + Point::new(get(p, 0), get(p, 1));
                    let shift = to - *end;
                    for e in &mut out {
                        e.geometry = e.geometry.stretched(std::slice::from_ref(frame), shift);
                    }
                }
                Param::Alignment { base, .. } if v != 0.0 => {
                    for e in &mut out {
                        e.geometry = e.geometry.rotated(*base, v);
                    }
                }
                Param::Polar { .. }
                | Param::Xy { .. }
                | Param::Alignment { .. }
                | Param::Lookup { .. }
                | Param::Array { .. } => {}
            }
        }
        out
    }

    /// The properties of every parameter, and what they are in the
    /// definition.
    pub fn properties(&self) -> Vec<(String, f64)> {
        self.params.iter().flat_map(Param::properties).collect()
    }

    /// The parameter a property belongs to.
    pub fn param_of(&self, property: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.properties().iter().any(|(k, _)| k.eq_ignore_ascii_case(property)))
    }
}

/// What a dynamic reference's grip does when it's dragged or clicked.
#[derive(Debug, Clone, PartialEq)]
pub enum DynGrip {
    /// Sets parameter `param`'s properties from where it's dropped.
    Param(usize),
    /// The next visibility state.
    Visibility,
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
        let at_defaults = def.properties().iter().all(|(k, d)| values.value(k).is_none_or(|v| (v - d).abs() < 1e-9))
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

    /// A dynamic reference's grips, in the drawing: each parameter's (a
    /// linear one's end, a point's position, a rotation's handle, a flip's
    /// arrow, a lookup's and the visibility states' menu).
    pub fn dynamic_grips(&self, id: EntityId) -> Vec<(Point, DynGrip)> {
        let Some(values) = self.dynamic_values(id) else { return Vec::new() };
        let (Some(def), Some(block), Some(e)) =
            (self.dynamic_block(&values.block), self.block(&values.block), self.entity(id))
        else {
            return Vec::new();
        };
        let Geometry::Insert { placement, .. } = &e.geometry else { return Vec::new() };
        let world = |p: Point| placement.apply(block.base, p);
        let get = |p: &Param, i: usize| {
            let props = p.properties();
            props.get(i).map_or(0.0, |(k, d)| values.value(k).unwrap_or(*d))
        };
        let mut out = Vec::new();
        for (i, p) in def.params.iter().enumerate() {
            let at = match p {
                Param::Linear { base, end, .. } => {
                    let d = unit(*end - *base);
                    Some(*base + d * get(p, 0))
                }
                Param::Flip { base, end, .. } => Some(base.midpoint(*end)),
                Param::Rotation { base, radius, .. } => Some(base.polar(*radius, get(p, 0))),
                Param::Point { .. } => Some(Point::new(get(p, 0), get(p, 1))),
                Param::Polar { base, .. } => Some(base.polar(get(p, 0), get(p, 1))),
                Param::Xy { base, .. } => Some(*base + Point::new(get(p, 0), get(p, 1))),
                Param::Alignment { base, .. } | Param::Lookup { base, .. } => Some(*base),
                Param::Array { .. } => None,
            };
            out.extend(at.map(|a| (world(a), DynGrip::Param(i))));
        }
        if def.states.len() > 1 {
            let corner = block.entities.iter().map(|be| be.geometry.bounds()).reduce(Aabb::union);
            let at = corner.map_or(block.base, |b| Point::new(b.max.x, b.max.y));
            out.push((world(at), DynGrip::Visibility));
        }
        out
    }

    /// Reference `id`'s values with grip `grip` dropped at `at` (in the
    /// drawing); `along` is the direction there of an object it's dropped
    /// on (an alignment grip's).
    pub fn dynamic_grip_values(
        &self,
        id: EntityId,
        grip: &DynGrip,
        at: Point,
        along: Option<f64>,
    ) -> Option<DynamicValues> {
        let mut values = self.dynamic_values(id)?;
        let def = self.dynamic_block(&values.block)?.clone();
        let block = self.block(&values.block)?;
        let Geometry::Insert { placement, .. } = &self.entity(id)?.geometry else { return None };
        let local = placement.local(block.base, at);
        let mut changes: Vec<(String, f64)> = Vec::new();
        let mut set = |k: String, v: f64| changes.push((k, v));
        match grip {
            DynGrip::Visibility => {
                let i = values
                    .visibility
                    .as_deref()
                    .and_then(|v| def.states.iter().position(|s| s.name.eq_ignore_ascii_case(v)))
                    .unwrap_or(0);
                values.visibility = def.states.get((i + 1) % def.states.len().max(1)).map(|s| s.name.clone());
            }
            DynGrip::Param(i) => {
                let p = def.params.get(*i)?;
                let props: Vec<String> = p.properties().into_iter().map(|(k, _)| k).collect();
                let current = |k: &str| values.value(k);
                match p {
                    Param::Linear { base, end, .. } => {
                        let d = unit(*end - *base);
                        set(props[0].clone(), (local - *base).dot(d).max(0.0));
                    }
                    Param::Flip { .. } => {
                        let now = current(&props[0]).unwrap_or(0.0);
                        set(props[0].clone(), if now == 0.0 { 1.0 } else { 0.0 });
                    }
                    Param::Rotation { base, .. } => set(props[0].clone(), base.angle_to(local)),
                    Param::Point { .. } => {
                        set(props[0].clone(), local.x);
                        set(props[1].clone(), local.y);
                    }
                    Param::Polar { base, .. } => {
                        set(props[0].clone(), base.distance(local));
                        set(props[1].clone(), base.angle_to(local));
                    }
                    Param::Xy { base, .. } => {
                        set(props[0].clone(), local.x - base.x);
                        set(props[1].clone(), local.y - base.y);
                    }
                    // Lined up with what it's dropped on, in the block's terms.
                    Param::Alignment { .. } => {
                        let a = along.unwrap_or(0.0) - placement.rotation;
                        set(props[0].clone(), a.rem_euclid(360.0));
                    }
                    // The next row; the first when none is chosen yet.
                    Param::Lookup { rows, .. } => {
                        let next = current(&props[0]).map_or(0, |n| (n as usize + 1) % rows.len().max(1));
                        set(props[0].clone(), next as f64);
                    }
                    Param::Array { .. } => return None,
                }
            }
        }
        for (k, v) in changes {
            values.values.retain(|(n, _)| !n.eq_ignore_ascii_case(&k));
            values.values.push((k, v));
        }
        Some(values)
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

    #[test]
    fn points_polar_xy_lookup_and_arrays() {
        let mut d = Document::new();
        let line = |a: (f64, f64), b: (f64, f64)| {
            BlockEntity::new(
                "0",
                Color::ByBlock,
                Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) },
            )
        };
        // A table top (0) and a chair (1).
        d.define_block(Block {
            name: "TABLE".into(),
            base: Point::ORIGIN,
            entities: vec![line((0.0, 0.0), (100.0, 0.0)), line((10.0, -20.0), (20.0, -20.0))],
        });
        let frame = Aabb::from_points(Point::new(90.0, -5.0), Point::new(110.0, 5.0));
        let def = DynamicBlock {
            params: vec![
                Param::Point { name: "Chair".into(), base: Point::new(10.0, -20.0), objects: vec![1] },
                Param::Xy {
                    name: "Size".into(),
                    base: Point::ORIGIN,
                    end: Point::new(100.0, 0.0),
                    stretch: Some(frame),
                },
                Param::Linear {
                    name: "Length".into(),
                    base: Point::ORIGIN,
                    end: Point::new(100.0, 0.0),
                    stretch: None,
                },
                Param::Array { name: "Chairs".into(), follows: "Length".into(), objects: vec![1], spacing: 30.0 },
                Param::Lookup {
                    name: "Model".into(),
                    base: Point::new(0.0, 10.0),
                    rows: vec![
                        ("Short".into(), vec![("Length".into(), 0.0)]),
                        ("Long".into(), vec![("Length".into(), 60.0)]),
                    ],
                },
                Param::Polar { name: "Leg".into(), base: Point::ORIGIN, end: Point::new(0.0, 50.0), stretch: None },
                Param::Alignment { name: "Align".into(), base: Point::ORIGIN, end: Point::new(10.0, 0.0) },
            ],
            states: vec![],
            current_state: 0,
        };
        assert_eq!(DynamicBlock::from_text(&def.to_text()), Some(def.clone()));
        let props: Vec<String> = def.properties().into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            props,
            [
                "Chair X",
                "Chair Y",
                "Size Horizontal",
                "Size Vertical",
                "Length",
                "Model",
                "Leg Distance",
                "Leg Angle",
                "Align"
            ]
        );
        d.set_dynamic_block("TABLE", Some(def));
        let id = d.add(Geometry::Insert {
            block: "TABLE".into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: vec![],
            clip: None,
        });
        let set = |d: &mut Document, values: Vec<(&str, f64)>| {
            let v = DynamicValues {
                block: "TABLE".into(),
                values: values.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                visibility: None,
            };
            d.set_dynamic_values(id, v);
        };
        // The chair moved, the top stretched to 150.
        set(&mut d, vec![("Chair X", 40.0), ("Chair Y", -30.0), ("Size Horizontal", 150.0)]);
        let l = lines(&d, id);
        assert!(
            l.iter().any(|(a, b)| a.approx_eq(Point::new(0.0, 0.0)) && b.approx_eq(Point::new(150.0, 0.0))),
            "{l:?}"
        );
        assert!(l.iter().any(|(a, _)| a.approx_eq(Point::new(40.0, -30.0))), "{l:?}");
        // The lookup's Long row: Length 60, so two more chairs 30 apart.
        set(&mut d, vec![("Model", 1.0)]);
        let chairs: Vec<f64> = lines(&d, id).iter().filter(|(a, _)| a.y == -20.0).map(|(a, _)| a.x).collect();
        assert_eq!(chairs, [10.0, 40.0, 70.0]);
        // Grips: the point's grip dragged moves the chair; the lookup's cycles its rows.
        let grips = d.dynamic_grips(id);
        let chair = grips.iter().find(|(_, g)| *g == DynGrip::Param(0)).unwrap();
        assert!(chair.0.approx_eq(Point::new(10.0, -20.0)));
        let moved = d.dynamic_grip_values(id, &chair.1, Point::new(25.0, -25.0), None).unwrap();
        assert_eq!((moved.value("Chair X"), moved.value("Chair Y")), (Some(25.0), Some(-25.0)));
        let model = grips.iter().find(|(_, g)| *g == DynGrip::Param(4)).unwrap();
        let next = d.dynamic_grip_values(id, &model.1, model.0, None).unwrap();
        assert_eq!(next.value("Model"), Some(0.0), "Long goes round to Short");
        d.set_dynamic_values(id, DynamicValues { block: "TABLE".into(), ..Default::default() });
        let first = d.dynamic_grip_values(id, &model.1, model.0, None).unwrap();
        assert_eq!(first.value("Model"), Some(0.0), "none chosen: the first row");
        let align = grips.iter().find(|(_, g)| *g == DynGrip::Param(6)).unwrap();
        let along = d.dynamic_grip_values(id, &align.1, align.0, Some(90.0)).unwrap();
        assert_eq!(along.value("Align"), Some(90.0));
    }
}

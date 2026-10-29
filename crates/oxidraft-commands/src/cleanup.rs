//! The Manage tab's cleanup: OVERKILL (duplicate and overlapping
//! objects), -PURGE (what nothing uses, empty objects) and AUDIT (broken
//! objects).

use std::collections::HashMap;

use oxidraft_geom::Point;
use oxidraft_model::{Document, EntityId, Geometry, Unused};

use crate::{Command, Ctx, Input, Prompt, Step};

/// A shape's numbers rounded, so copies that differ by rounding match; a
/// line the same either way round.
fn shape_key(g: &Geometry) -> String {
    let g = match g {
        Geometry::Line { start, end } if (end.x, end.y) < (start.x, start.y) => {
            Geometry::Line { start: *end, end: *start }
        }
        g => g.clone(),
    };
    let text = format!("{g:?}");
    let mut out = String::with_capacity(text.len());
    let mut number = String::new();
    let flush = |number: &mut String, out: &mut String| {
        if number.is_empty() {
            return;
        }
        match number.parse::<f64>() {
            Ok(v) => {
                let r = format!("{:.6}", (v * 1e6).round() / 1e6);
                out.push_str(if r == "-0.000000" { "0.000000" } else { &r });
            }
            Err(_) => out.push_str(number),
        }
        number.clear();
    };
    for c in text.chars() {
        let part_of_number = c.is_ascii_digit()
            || c == '.'
            || ((c == '-' || c == 'e') && !number.is_empty())
            || (c == '-' && number.is_empty());
        if part_of_number {
            number.push(c);
        } else {
            flush(&mut number, &mut out);
            out.push(c);
        }
    }
    flush(&mut number, &mut out);
    out
}

/// OVERKILL on `ids`: copies of an object (same shape and properties)
/// go, and lines lying along each other become one. Returns (copies
/// deleted, overlapping lines deleted).
pub fn overkill(doc: &mut Document, ids: &[EntityId]) -> (usize, usize) {
    let style = |doc: &Document, id: EntityId| {
        doc.entity(id).map(|e| {
            format!("{}|{:?}|{}|{:?}", e.layer.to_lowercase(), e.color, e.linetype.to_lowercase(), e.lineweight)
        })
    };
    // Copies: the first of each kept.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut copies = Vec::new();
    for &id in ids {
        let (Some(s), Some(e)) = (style(doc, id), doc.entity(id)) else { continue };
        if !seen.insert(format!("{s}#{}", shape_key(&e.geometry))) {
            copies.push(id);
        }
    }
    for id in &copies {
        doc.remove(*id);
    }
    // Lines along one line: grouped by style, direction and offset, then
    // merged where they overlap or touch.
    let mut runs: HashMap<String, Vec<(EntityId, f64, f64, Point)>> = HashMap::new();
    for &id in ids {
        let Some(Geometry::Line { start, end }) = doc.entity(id).map(|e| e.geometry.clone()) else { continue };
        let Some(s) = style(doc, id) else { continue };
        let length = start.distance(end);
        if length < 1e-12 {
            continue;
        }
        let mut d = (end - start) * (1.0 / length);
        if d.x < -1e-12 || (d.x.abs() <= 1e-12 && d.y < 0.0) {
            d = d * -1.0;
        }
        let offset = start.cross(d);
        let key = format!("{s}#{:.6},{:.6},{:.6}", d.x, d.y, offset);
        let (a, b) = (start.dot(d), end.dot(d));
        runs.entry(key).or_default().push((id, a.min(b), a.max(b), d));
    }
    let mut overlapping = 0;
    for mut lines in runs.into_values().filter(|l| l.len() > 1) {
        lines.sort_by(|x, y| x.1.total_cmp(&y.1));
        let d = lines[0].3;
        // A point on the line: the first line's start, less its own run.
        let base = match doc.entity(lines[0].0).map(|e| &e.geometry) {
            Some(Geometry::Line { start, .. }) => *start - d * start.dot(d),
            _ => continue,
        };
        let mut group: Vec<(EntityId, f64, f64, Point)> = vec![lines[0]];
        let flush = |group: &[(EntityId, f64, f64, Point)], doc: &mut Document| {
            if group.len() < 2 {
                return 0;
            }
            let (from, to) = (group[0].1, group.iter().map(|g| g.2).fold(f64::MIN, f64::max));
            doc.modify(group[0].0, Geometry::Line { start: base + d * from, end: base + d * to });
            for g in &group[1..] {
                doc.remove(g.0);
            }
            group.len() - 1
        };
        for line in lines.into_iter().skip(1) {
            let end = group.iter().map(|g| g.2).fold(f64::MIN, f64::max);
            if line.1 <= end + 1e-9 {
                group.push(line);
            } else {
                overlapping += flush(&group, doc);
                group = vec![line];
            }
        }
        overlapping += flush(&group, doc);
    }
    (copies.len(), overlapping)
}

/// OVERKILL: the objects selected, cleaned of copies and overlaps.
#[derive(Default)]
pub struct Overkill;

impl Command for Overkill {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Err("Select objects, then press Enter.".into()) };
        let (copies, overlapping) = overkill(ctx.doc, &s.ids);
        ctx.messages.push(format!("{copies} duplicate(s) deleted"));
        ctx.messages.push(format!("{overlapping} overlapping object(s) or segment(s) deleted"));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

const PURGE_OPTIONS: &[&str] = &["Blocks", "LAyers", "LTypes", "Zero-length geometry", "Empty text objects", "All"];

/// -PURGE: blocks, layers, linetypes (or All) nothing uses, nested ones
/// too, or zero-length geometry and empty text.
#[derive(Default)]
pub struct Purge;

impl Command for Purge {
    fn prompt(&self) -> Prompt {
        Prompt::new("Enter type of unused objects to purge").with_keywords(PURGE_OPTIONS).with_default("All")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let kind = match input {
            Input::Keyword(k) => k,
            Input::Enter => "All",
            _ => return Err("Choose what to purge.".into()),
        };
        if kind.starts_with("Zero") || kind.starts_with("Empty") {
            let empty: Vec<EntityId> = ctx.doc.empty_objects();
            let wanted = |doc: &Document, id: &EntityId| {
                let text = matches!(doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Text { .. }));
                text == kind.starts_with("Empty")
            };
            let ids: Vec<EntityId> = empty.into_iter().filter(|id| wanted(ctx.doc, id)).collect();
            for id in &ids {
                ctx.doc.remove(*id);
            }
            ctx.messages.push(format!("{} object(s) deleted.", ids.len()));
            return Ok(Step::Done);
        }
        let all = ctx.doc.unused();
        let what = Unused {
            blocks: if matches!(kind, "Blocks" | "All") { all.blocks } else { Vec::new() },
            layers: if matches!(kind, "LAyers" | "All") { all.layers } else { Vec::new() },
            linetypes: if matches!(kind, "LTypes" | "All") { all.linetypes } else { Vec::new() },
        };
        let count = ctx.doc.purge(&what, true);
        ctx.messages.push(if count == 0 {
            "No unreferenced objects found.".into()
        } else {
            format!("{count} item(s) purged.")
        });
        Ok(Step::Done)
    }
}

/// AUDIT: checks the drawing, mending what it finds if asked to.
#[derive(Default)]
pub struct Audit;

impl Command for Audit {
    fn prompt(&self) -> Prompt {
        Prompt::new("Fix any errors detected?").with_keywords(&["Yes", "No"]).with_default("N")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let fix = match input {
            Input::Keyword(k) => k == "Yes",
            Input::Enter => false,
            _ => return Err("Yes or No.".into()),
        };
        let found = ctx.doc.audit(fix);
        ctx.messages.push("Auditing Header\nAuditing Tables\nAuditing Entities".into());
        ctx.messages.extend(found.iter().cloned());
        let fixed = if fix { found.len() } else { 0 };
        ctx.messages.push(format!("Total errors found {} fixed {fixed}", found.len()));
        Ok(Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overkill_deletes_copies_and_joins_overlapping_lines() {
        let mut d = Document::new();
        let line = |d: &mut Document, a: (f64, f64), b: (f64, f64)| {
            d.add(Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) })
        };
        let a = line(&mut d, (0.0, 0.0), (10.0, 0.0));
        line(&mut d, (10.0, 0.0), (0.0, 0.0)); // the same, reversed
        line(&mut d, (5.0, 0.0), (15.0, 0.0)); // overlapping
        line(&mut d, (15.0, 0.0), (20.0, 0.0)); // touching
        line(&mut d, (30.0, 0.0), (40.0, 0.0)); // apart
        line(&mut d, (0.0, 1.0), (10.0, 1.0)); // parallel, not along
        d.add(Geometry::Circle { center: Point::ORIGIN, radius: 1.0 });
        d.add(Geometry::Circle { center: Point::new(0.0, 0.000_000_01), radius: 1.0 });
        let ids: Vec<EntityId> = d.entities().map(|e| e.id).collect();
        let (copies, overlapping) = overkill(&mut d, &ids);
        assert_eq!((copies, overlapping), (2, 2));
        assert_eq!(d.len(), 4, "the joined line, the one apart, the parallel one and a circle");
        let Some(Geometry::Line { start, end }) = d.entity(a).map(|e| e.geometry.clone()) else { panic!() };
        assert!(start.approx_eq(Point::new(0.0, 0.0)) && end.approx_eq(Point::new(20.0, 0.0)));
    }
}

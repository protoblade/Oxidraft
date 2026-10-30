//! Multileaders as AutoCAD keeps them: MULTILEADER entities (AutoCAD 2007
//! and later). Oxidraft's DXF writer gives a multileader as a LEADER with
//! its XDATA, plus parts: its text as MTEXT, added arrowheads as LEADERs,
//! blocks as INSERTs. On the way out they become one MULTILEADER that keeps
//! the XDATA; on the way in a MULTILEADER, AutoCAD's too, becomes that
//! LEADER again (XDATA made for it when it has none), which the DXF reader
//! makes one multileader of. Blocks gathered by MLEADERCOLLECT go out as
//! one anonymous block of their references, AutoCAD's MULTILEADER having
//! room for one block; it's dropped on the way in, the XDATA having them.

use std::collections::HashMap;
use std::sync::Arc;

use acadrust::entities::{
    BlockAttribute, LeaderContentType, MultiLeader, ProxyGraphicRecord, ProxyGraphics, ProxyUnicodeText,
    TextAttachmentPointType,
};
use acadrust::objects::{Dictionary, MultiLeaderStyle, ObjectType};
use acadrust::types::{Handle, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType};
use oxidraft_geom::Point;
use oxidraft_model::annotation::{leader_primitives, with_leader_style};
use oxidraft_model::{Attach, Document, EntityId, Geometry};

const APP: &str = "OXIDRAFT_MLEADER";

fn v3(p: Point) -> Vector3 {
    Vector3::new(p.x, p.y, 0.0)
}

fn point(v: &Vector3) -> Point {
    Point::new(v.x, v.y)
}

/// The MULTILEADER style a new multileader names: the drawing's Standard,
/// made (in an ACAD_MLEADERSTYLE dictionary) when there's none.
pub(crate) fn standard_style(cad: &mut CadDocument, text_style: Option<Handle>) -> Handle {
    let found = cad.objects.iter().find_map(|(h, o)| match o {
        ObjectType::MultiLeaderStyle(s) if s.name.eq_ignore_ascii_case("Standard") => Some(*h),
        _ => None,
    });
    if let Some(h) = found {
        return h;
    }
    let root = cad.header.named_objects_dict_handle;
    let existing = match cad.objects.get(&root) {
        Some(ObjectType::Dictionary(d)) => d.get("ACAD_MLEADERSTYLE"),
        _ => None,
    };
    let dict = existing.unwrap_or_else(|| {
        let h = cad.allocate_handle();
        let mut d = Dictionary::new();
        d.handle = h;
        d.owner = root;
        cad.objects.insert(h, ObjectType::Dictionary(d));
        if let Some(ObjectType::Dictionary(r)) = cad.objects.get_mut(&root) {
            r.add_entry("ACAD_MLEADERSTYLE", h);
        }
        h
    });
    let handle = cad.allocate_handle();
    let mut style = MultiLeaderStyle::standard();
    style.handle = handle;
    style.owner_handle = dict;
    style.text_style_handle = text_style;
    cad.objects.insert(handle, ObjectType::MultiLeaderStyle(style));
    if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&dict) {
        d.add_entry("Standard", handle);
    }
    handle
}

/// A new anonymous block (`*U`) in `cad`, empty.
fn anonymous_block(cad: &mut CadDocument) -> Handle {
    let mut index = 1;
    while cad.block_records.get(&format!("*U{index}")).is_some() {
        index += 1;
    }
    let mut record = acadrust::tables::BlockRecord::new(format!("*U{index}"));
    record.handle = cad.allocate_handle();
    record.block_entity_handle = cad.allocate_handle();
    record.block_end_handle = cad.allocate_handle();
    record.flags.anonymous = true;
    let handle = record.handle;
    let _ = cad.block_records.add(record);
    handle
}

/// Moves the object `h` into the block `record`.
pub(crate) fn move_into(cad: &mut CadDocument, h: Handle, record: Handle) {
    for r in cad.block_records.iter_mut() {
        if r.handle == record {
            r.entity_handles.push(h);
        } else {
            r.entity_handles.retain(|x| *x != h);
        }
    }
    if let Some(e) = cad.get_entity_mut(h) {
        e.common_mut().owner_handle = record;
    }
}

/// Proxy graphics of `parts` (a multileader's lines, arrowhead and text),
/// for a program that hasn't multileaders (AutoCAD 2004 and older) to
/// draw it by: polylines (type 6), filled polygons (7) and lines of text
/// (36).
pub(crate) fn proxy_graphics(parts: &[Geometry]) -> ProxyGraphics {
    let mut records = Vec::new();
    // A polyline (6), or a filled polygon (7).
    let mut points_record = |record_type: u32, points: &[Point]| {
        let mut data = (points.len() as u32).to_le_bytes().to_vec();
        for p in points {
            for v in [p.x, p.y, 0.0] {
                data.extend(v.to_le_bytes());
            }
        }
        records.push(ProxyGraphicRecord::Unknown { record_type, data });
    };
    let mut texts = Vec::new();
    for g in parts {
        match g {
            Geometry::Line { start, end } => points_record(6, &[*start, *end]),
            // Circles and arcs, as 32-sided polylines.
            Geometry::Circle { center, radius } => points_record(
                6,
                &(0..=32).map(|k| center.polar(*radius, f64::from(k) * 360.0 / 32.0)).collect::<Vec<_>>(),
            ),
            Geometry::Arc { center, radius, start_angle, end_angle } => {
                let sweep = (end_angle - start_angle).rem_euclid(360.0);
                let points: Vec<Point> =
                    (0..=32).map(|k| center.polar(*radius, start_angle + sweep * f64::from(k) / 32.0)).collect();
                points_record(6, &points);
            }
            // A filled arrowhead.
            Geometry::Hatch { loops, .. } => {
                for l in loops {
                    points_record(7, &l.iter().map(|v| v.point).collect::<Vec<_>>());
                }
            }
            Geometry::Polyline { vertices, closed } => {
                let mut points: Vec<Point> = vertices.iter().map(|v| v.point).collect();
                if *closed && let Some(&first) = points.first() {
                    points.push(first);
                }
                // A dot (a ring of arcs): a circle of its width's radius.
                if vertices.iter().any(|v| v.bulge != 0.0) && points.len() >= 2 {
                    let center = Point::new((points[0].x + points[1].x) / 2.0, (points[0].y + points[1].y) / 2.0);
                    let r = points[0].distance(points[1]) / 2.0 + vertices[0].start_width / 2.0;
                    points = (0..=32).map(|k| center.polar(r, f64::from(k) * 360.0 / 32.0)).collect();
                }
                points_record(6, &points);
            }
            Geometry::Text { position, height, attach, content, .. } => {
                // Its lines, stacked as MTEXT stacks them, from the middle.
                let lines: Vec<String> =
                    content.replace("\\P", "\n").lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
                let n = lines.len().max(1) as f64;
                let block = height * (n + (n - 1.0) * 0.667);
                for (k, line) in lines.into_iter().enumerate() {
                    let wide = height * 0.6 * line.chars().count() as f64;
                    let x = match attach {
                        Attach::MiddleRight | Attach::TopRight | Attach::BottomRight => position.x - wide,
                        Attach::MiddleCenter | Attach::TopCenter | Attach::BottomCenter => position.x - wide / 2.0,
                        _ => position.x,
                    };
                    let y = position.y + block / 2.0 - height - k as f64 * height * 1.667;
                    texts.push(ProxyGraphicRecord::UnicodeText(ProxyUnicodeText {
                        position: Vector3::new(x, y, 0.0),
                        normal: Vector3::new(0.0, 0.0, 1.0),
                        direction: Vector3::new(1.0, 0.0, 0.0),
                        height: *height,
                        width_factor: 1.0,
                        oblique_angle: 0.0,
                        text: line,
                    }));
                }
            }
            _ => {}
        }
    }
    records.extend(texts);
    ProxyGraphics { records }
}

/// Makes each of `doc`'s multileaders one MULTILEADER in `cad`, in its
/// LEADER's place; `handles` are what each object was written as. With
/// `proxies`, each carries proxy graphics too, for a version of AutoCAD
/// that hasn't multileaders to draw it.
pub(crate) fn finish(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>, proxies: bool) {
    if !doc.all_entities().any(|e| matches!(e.geometry, Geometry::Leader { .. })) {
        return;
    }
    let text_style = cad.text_styles.get("Standard").map(|s| s.handle);
    let style = Some(standard_style(cad, text_style));
    let ls = doc.leader_style();
    let arrowhead = crate::styles::arrow_block(cad, ls.arrowhead);
    for e in doc.all_entities() {
        let Geometry::Leader { points, content, scale, more, blocks } = &e.geometry else { continue };
        let Some(written) = handles.get(&e.id) else { continue };
        let Some((&first, parts)) = written.split_first() else { continue };
        let (first, [.., prev, last]) = (Handle::new(first), points.as_slice()) else { continue };
        let Some(EntityType::Leader(old)) = cad.get_entity(first) else { continue };
        let mut m = MultiLeader::new();
        m.common = old.common.clone();
        m.style_handle = style;
        m.text_style_handle = text_style;
        m.context.text_style_handle = text_style;
        let side = if last.x >= prev.x { 1.0 } else { -1.0 };
        let landing = if ls.landing_on { ls.landing * scale } else { 0.0 };
        m.arrowhead_handle = arrowhead;
        m.arrowhead_size = ls.arrow_size * scale;
        m.text_height = ls.text_height * scale;
        m.enable_landing = ls.landing_on;
        m.enable_dogleg = ls.landing_on;
        m.dogleg_length = landing;
        m.context.scale_factor = 1.0;
        m.context.arrowhead_size = ls.arrow_size * scale;
        m.context.landing_gap = 0.0;
        let root = m.add_leader_root();
        root.connection_point = v3(*last + Point::new(side * landing, 0.0));
        root.direction = Vector3::new(side, 0.0, 0.0);
        root.landing_distance = landing;
        root.create_line(points.iter().map(|p| v3(*p)).collect());
        for tip in more {
            root.create_line(vec![v3(*tip), v3(*last)]);
        }
        let inserts = doc.leader_inserts(&e.geometry);
        let mut kept = Vec::new();
        match (blocks, inserts.first()) {
            // Gathered blocks: their references, as placed, in a block of
            // their own at the drawing's origin.
            (Some(b), Some(_)) if b.items.len() > 1 => {
                let refs: Vec<Handle> = parts
                    .iter()
                    .map(|p| Handle::new(*p))
                    .filter(|h| matches!(cad.get_entity(*h), Some(EntityType::Insert(_))))
                    .collect();
                let record = anonymous_block(cad);
                for h in &refs {
                    move_into(cad, *h, record);
                }
                kept = refs;
                m.set_block_content(record, Vector3::new(0.0, 0.0, 0.0));
                m.context.block_content_scale = Vector3::new(1.0, 1.0, 1.0);
            }
            (Some(b), Some(Geometry::Insert { block, placement, .. })) => {
                let Some(record) = cad.block_records.get(block).map(|r| r.handle) else { continue };
                m.set_block_content(record, v3(placement.position));
                m.context.block_content_scale = Vector3::new(placement.scale, placement.scale, placement.scale);
                // The block's attribute gets its value.
                let attdef = cad
                    .entities_in_block(block)
                    .find(|x| matches!(x, EntityType::AttributeDefinition(_)))
                    .map(|x| x.common().handle);
                if let Some(value) = b.items.first().map(|i| i.value.clone()) {
                    m.block_attributes
                        .push(BlockAttribute { attribute_definition_handle: attdef, ..BlockAttribute::new(&value) });
                }
            }
            _ if !content.trim().is_empty() => {
                let text = with_leader_style(ls, || leader_primitives(points, content, *scale, false))
                    .into_iter()
                    .find_map(|g| match g {
                        Geometry::Text { position, height, .. } => Some((position, height)),
                        _ => None,
                    });
                let (at, height) = text.unwrap_or((*last, ls.text_height * scale));
                // Kept from its top: AutoCAD's MTEXT in a multileader hangs
                // from the top line.
                m.set_text_content(content, v3(at + Point::new(0.0, height / 2.0)));
                m.context.text_height = height;
                m.context.text_attachment_point =
                    if side > 0.0 { TextAttachmentPointType::Left } else { TextAttachmentPointType::Right };
            }
            _ => m.content_type = LeaderContentType::None,
        }
        if proxies {
            let mut parts = with_leader_style(ls, || leader_primitives(points, content, *scale, blocks.is_some()));
            // Its blocks, placed, and their attributes' values.
            for insert in &inserts {
                let Geometry::Insert { block, placement, attributes, .. } = insert else { continue };
                let Some(b) = doc.block(block) else { continue };
                parts.extend(b.entities.iter().map(|x| placement.place(b.base, &x.geometry)));
                parts.extend(attributes.iter().filter(|a| !a.invisible).map(|a| Geometry::Text {
                    look: Default::default(),
                    position: a.position,
                    height: a.height,
                    rotation: a.rotation,
                    width: 0.0,
                    attach: a.attach,
                    content: a.value.clone(),
                }));
            }
            m.common.set_proxy_graphics(&proxy_graphics(&parts));
        }
        if let Some(slot) = cad.get_entity_mut(first) {
            *slot = EntityType::MultiLeader(Box::new(m));
        }
        for &part in parts {
            let part = Handle::new(part);
            if kept.contains(&part) {
                continue;
            }
            cad.remove_entity(part);
            for r in cad.block_records.iter_mut() {
                r.entity_handles.retain(|h| *h != part);
            }
        }
    }
}

/// Makes each MULTILEADER in `cad` a LEADER along its first leader line,
/// with the multileader as Oxidraft's XDATA: kept, or made from its text,
/// size, other leader lines and block.
pub(crate) fn unpack(cad: &mut CadDocument) {
    let ls = oxidraft_model::annotation::DEFAULT_LEADER_STYLE;
    let found: Vec<Handle> =
        cad.entities().filter(|e| matches!(e, EntityType::MultiLeader(_))).map(|e| e.common().handle).collect();
    for h in found {
        let Some(EntityType::MultiLeader(m)) = cad.get_entity(h).cloned() else { continue };
        let lines: Vec<Vec<Point>> = m
            .context
            .leader_roots
            .iter()
            .flat_map(|r| &r.lines)
            .map(|l| l.points.iter().map(point).collect())
            .collect();
        let Some(first) = lines.first() else { continue };
        let mut vertices = first.clone();
        if vertices.len() < 2 {
            // Only the arrow's tip: the line goes to where the landing starts.
            let root = &m.context.leader_roots[0];
            let start = point(&root.connection_point) - point(&root.direction) * root.landing_distance;
            vertices.push(start);
        }
        // Gathered blocks' anonymous block: the XDATA has them.
        let content = m.block_content_handle.and_then(|b| cad.block_records.iter().find(|r| r.handle == b));
        let content = content.map(|r| r.name.clone());
        if m.common.extended_data.get_record(APP).is_some()
            && let Some(name) = content
            && name.starts_with("*U")
        {
            let inside = cad.block_records.get(&name).map(|r| r.entity_handles.clone()).unwrap_or_default();
            for h in inside {
                cad.remove_entity(h);
            }
            cad.block_records.remove(&name);
        }
        let mut leader = acadrust::entities::Leader::new();
        leader.common = m.common.clone();
        leader.vertices = vertices.iter().map(|p| v3(*p)).collect();
        leader.arrow_enabled = true;
        if leader.common.extended_data.get_record(APP).is_none() {
            let text_height = if m.context.text_height > 0.0 { m.context.text_height } else { m.text_height };
            let scale = if text_height > 0.0 { text_height / ls.text_height } else { 1.0 };
            let mut x = ExtendedDataRecord::new(APP);
            let block = m
                .block_content_handle
                .and_then(|b| cad.block_records.iter().find(|r| r.handle == b))
                .map(|r| r.name.clone());
            x.add_value(XDataValue::String(if block.is_some() {
                String::new()
            } else {
                m.text().unwrap_or("").into()
            }));
            x.add_value(XDataValue::Real(scale));
            x.add_value(XDataValue::Integer32(i32::from(block.is_some())));
            for tip in lines.iter().skip(1).filter_map(|l| l.first()) {
                x.add_value(XDataValue::Real(tip.x));
                x.add_value(XDataValue::Real(tip.y));
            }
            if let Some(name) = block {
                let value = m.block_attributes.first().map_or_else(String::new, |a| a.text.clone());
                x.add_value(XDataValue::String(name));
                x.add_value(XDataValue::String(value));
            }
            leader.common.extended_data.add_record(x);
        }
        cad.replace_entity_arc(h, Arc::new(EntityType::Leader(leader)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nativesave_keeps_multileaders_in_older_versions_with_their_graphics() {
        let mut doc = Document::new();
        let leader = Geometry::Leader {
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
            content: "SEE DETAIL\\PSHEET 4".into(),
            scale: 1.0,
            more: Vec::new(),
            blocks: None,
        };
        doc.add(leader);
        // And one with a tag block (a circle around its number).
        let tag = doc.tag_block(oxidraft_model::annotation::TagShape::Circle);
        doc.add(Geometry::Leader {
            points: vec![Point::new(50.0, 0.0), Point::new(60.0, 10.0)],
            content: String::new(),
            scale: 1.0,
            more: Vec::new(),
            blocks: Some(Box::new(oxidraft_model::annotation::LeaderBlocks::one(tag, "7"))),
        });
        for version in [crate::Version::R2004, crate::Version::R2000] {
            crate::set_native_objects(true);
            let dwg = crate::write(&doc, version).unwrap();
            crate::set_native_objects(false);
            // As AutoCAD 2004 reads it: a multileader it draws by its graphics.
            let cad = acadrust::DwgReader::from_stream(std::io::Cursor::new(dwg.clone())).read().unwrap();
            let leaders: Vec<&EntityType> =
                cad.entities().filter(|e| matches!(e, EntityType::MultiLeader(_))).collect();
            assert_eq!(leaders.len(), 2, "{version:?}");
            let drawn = |e: &EntityType| {
                let graphics = e.common().proxy_graphics().expect("its proxy graphics");
                let lines = graphics
                    .records
                    .iter()
                    .filter(|r| matches!(r, ProxyGraphicRecord::Unknown { record_type: 6, .. }))
                    .count();
                let texts: Vec<String> = graphics
                    .records
                    .iter()
                    .filter_map(|r| match r {
                        ProxyGraphicRecord::UnicodeText(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect();
                (lines, texts)
            };
            let all: Vec<(usize, Vec<String>)> = leaders.iter().map(|e| drawn(e)).collect();
            assert!(all.iter().any(|(lines, texts)| *lines >= 1 && texts == &["SEE DETAIL", "SHEET 4"]), "{all:?}");
            // The tag: its leader, its circle and its number.
            assert!(all.iter().any(|(lines, texts)| *lines >= 2 && texts == &["7"]), "{all:?}");
            // Oxidraft (and AutoCAD 2007 on) reads it back a multileader.
            let back = crate::read(&dwg).unwrap().imported.document;
            let got: Vec<&Geometry> = back.entities().map(|e| &e.geometry).collect();
            assert!(
                got.iter().any(|g| matches!(g, Geometry::Leader { content, .. } if content == "SEE DETAIL\\PSHEET 4")),
                "{version:?}: {got:?}"
            );
            assert!(got.iter().any(|g| matches!(g, Geometry::Leader { blocks: Some(_), .. })), "{version:?}: {got:?}");
        }
        // Without NATIVESAVE, AutoCAD 2004 gets LEADERs it can edit.
        let cad =
            acadrust::DwgReader::from_stream(std::io::Cursor::new(crate::write(&doc, crate::Version::R2004).unwrap()))
                .read()
                .unwrap();
        assert!(cad.entities().any(|e| matches!(e, EntityType::Leader(_))));
    }
}

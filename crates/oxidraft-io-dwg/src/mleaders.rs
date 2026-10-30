//! Multileaders as AutoCAD keeps them: MULTILEADER entities (AutoCAD 2007
//! and later). Oxidraft's DXF writer gives a multileader as a LEADER with
//! its XDATA, plus parts: its text as MTEXT, added arrowheads as LEADERs,
//! blocks as INSERTs. On the way out they become one MULTILEADER that keeps
//! the XDATA; on the way in a MULTILEADER, AutoCAD's too, becomes that
//! LEADER again (XDATA made for it when it has none), which the DXF reader
//! makes one multileader of.

use std::collections::HashMap;
use std::sync::Arc;

use acadrust::entities::{BlockAttribute, LeaderContentType, MultiLeader, TextAttachmentPointType};
use acadrust::objects::{Dictionary, MultiLeaderStyle, ObjectType};
use acadrust::types::{Handle, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType};
use oxidraft_geom::Point;
use oxidraft_model::annotation::{leader_primitives, with_leader_style};
use oxidraft_model::{Document, EntityId, Geometry};

const APP: &str = "OXIDRAFT_MLEADER";

fn v3(p: Point) -> Vector3 {
    Vector3::new(p.x, p.y, 0.0)
}

fn point(v: &Vector3) -> Point {
    Point::new(v.x, v.y)
}

/// The MULTILEADER style a new multileader names: the drawing's Standard,
/// made (in an ACAD_MLEADERSTYLE dictionary) when there's none.
fn standard_style(cad: &mut CadDocument, text_style: Option<Handle>) -> Handle {
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

/// Makes each of `doc`'s multileaders one MULTILEADER in `cad`, in its
/// LEADER's place; `handles` are what each object was written as.
pub(crate) fn finish(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    if !doc.all_entities().any(|e| matches!(e.geometry, Geometry::Leader { .. })) {
        return;
    }
    let text_style = cad.text_styles.get("Standard").map(|s| s.handle);
    let style = Some(standard_style(cad, text_style));
    let ls = doc.leader_style();
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
        match (blocks, inserts.first()) {
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
        if let Some(slot) = cad.get_entity_mut(first) {
            *slot = EntityType::MultiLeader(Box::new(m));
        }
        for &part in parts {
            let part = Handle::new(part);
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

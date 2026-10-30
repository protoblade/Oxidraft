//! AutoCAD's annotative objects coming in annotative. AutoCAD keeps an
//! object's representation for each annotation scale as a context leaf
//! (`AcDb*ObjectContextData`) under its extension dictionary
//! (`AcDbContextDataManager` → `ACDB_ANNOTATIONSCALES` → `*A1`...), each
//! naming an `AcDbScale`: those scales become the object's, the one its own
//! data is at (the default context) the one it shows. The drawing's
//! annotation scale is AutoCAD's CANNOSCALE when Oxidraft's settings don't
//! give one. With NATIVESAVE on, annotative MTEXT, TEXT and block
//! references go out in that form too: AcadAnnotative XDATA, a context leaf
//! for each scale (its representation, moved where it was on its own), and
//! the scales in ACAD_SCALELIST.

use std::collections::HashMap;

use acadrust::objects::{Dictionary, MTextContext, ObjectContextData, ObjectContextKind, ObjectType, Scale};
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};
use oxidraft_io_dxf::Imported;
use oxidraft_model::{Annotative, Document, EntityId};

/// What owns object `h` (a dictionary or a context leaf).
fn owner(cad: &CadDocument, h: Handle) -> Option<Handle> {
    match cad.objects.get(&h)? {
        ObjectType::Dictionary(d) => Some(d.owner),
        ObjectType::ObjectContextData(c) => Some(c.owner_handle),
        _ => None,
    }
}

/// Drawing units per paper unit of scale object `h`.
fn ratio(cad: &CadDocument, h: Handle) -> Option<f64> {
    match cad.objects.get(&h)? {
        ObjectType::Scale(s) if s.paper_units > 0.0 && s.drawing_units > 0.0 => Some(s.drawing_units / s.paper_units),
        _ => None,
    }
}

pub(crate) fn restore(cad: &CadDocument, imported: &mut Imported) {
    // Each object's scales, by its handle: its leaves climb to it through
    // the dictionaries above them.
    let mut scales: HashMap<u64, (Vec<f64>, Option<f64>)> = HashMap::new();
    for o in cad.objects.values() {
        let ObjectType::ObjectContextData(c) = o else { continue };
        let Some(k) = ratio(cad, c.scale) else { continue };
        let mut at = c.owner_handle;
        for _ in 0..4 {
            if cad.get_entity(at).is_some() {
                let entry = scales.entry(at.value()).or_default();
                if !entry.0.iter().any(|s| (s - k).abs() < 1e-9 * k) {
                    entry.0.push(k);
                }
                if c.is_default {
                    entry.1 = Some(k);
                }
                break;
            }
            let Some(next) = owner(cad, at).filter(|n| !n.is_null() && *n != at) else { break };
            at = next;
        }
    }
    for (handle, (mut list, shown)) in scales {
        let Some(&id) = imported.handles.get(&handle) else { continue };
        if imported.document.entity(id).is_none_or(|e| e.annotative.is_some()) {
            continue;
        }
        list.sort_by(f64::total_cmp);
        let shown = shown.unwrap_or(list[0]);
        imported
            .document
            .update(id, |e| e.annotative = Some(Box::new(Annotative { scales: list, shown, offsets: Vec::new() })));
    }
    // The drawing's annotation scale, unless Oxidraft's own settings gave
    // one: CANNOSCALE's scale (by name in the scale list, or as `1:50`
    // says), else CANNOSCALEVALUE (paper per drawing unit).
    let name = cad.header.current_annotation_scale.trim();
    let by_name = cad
        .objects
        .values()
        .find_map(|o| match o {
            ObjectType::Scale(s) if s.name.eq_ignore_ascii_case(name) && s.paper_units > 0.0 => {
                Some(s.drawing_units / s.paper_units)
            }
            _ => None,
        })
        .or_else(|| {
            let (p, d) = name.split_once(':')?;
            let (p, d) = (p.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?);
            (p > 0.0 && d > 0.0).then_some(d / p)
        });
    let value = cad.header.annotation_scale_value;
    let scale = by_name.or((value > 0.0).then(|| 1.0 / value)).unwrap_or(1.0);
    if (imported.document.anno_scale() - 1.0).abs() < 1e-12 && (scale - 1.0).abs() > 1e-12 {
        imported.document.load_anno_scale(scale);
    }
}

/// A dictionary in `cad`, owned by `owner`, with `entries`: its handle.
fn add_dictionary(cad: &mut CadDocument, owner: Handle, entries: Vec<(String, Handle)>) -> Handle {
    let h = cad.allocate_handle();
    let mut d = Dictionary::new();
    d.handle = h;
    d.owner = owner;
    d.entries = entries;
    cad.objects.insert(h, ObjectType::Dictionary(d));
    h
}

fn add_entry(cad: &mut CadDocument, dict: Handle, name: &str, h: Handle) {
    if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&dict) {
        d.entries.push((name.to_string(), h));
    }
}

/// The scale `ratio` (drawing units per paper unit) in ACAD_SCALELIST,
/// made there if it has none such.
fn scale_handle(cad: &mut CadDocument, ratio: f64) -> Handle {
    let found = cad.objects.iter().find_map(|(h, o)| match o {
        ObjectType::Scale(s)
            if s.paper_units > 0.0 && (s.drawing_units / s.paper_units - ratio).abs() < 1e-9 * ratio =>
        {
            Some(*h)
        }
        _ => None,
    });
    if let Some(h) = found {
        return h;
    }
    let root = cad.header.named_objects_dict_handle;
    let list = match cad.objects.get(&root) {
        Some(ObjectType::Dictionary(d)) => d.entries.iter().find(|(n, _)| n == "ACAD_SCALELIST").map(|(_, h)| *h),
        _ => None,
    };
    let list = list.unwrap_or_else(|| {
        let h = add_dictionary(cad, root, Vec::new());
        add_entry(cad, root, "ACAD_SCALELIST", h);
        h
    });
    let count = match cad.objects.get(&list) {
        Some(ObjectType::Dictionary(d)) => d.entries.len(),
        _ => 0,
    };
    let name = if ratio >= 1.0 { format!("1:{}", fmt(ratio)) } else { format!("{}:1", fmt(1.0 / ratio)) };
    let (paper, drawing) = if ratio >= 1.0 { (1.0, ratio) } else { (1.0 / ratio, 1.0) };
    let mut s = Scale::new(&name, paper, drawing);
    let h = cad.allocate_handle();
    s.handle = h;
    s.owner_handle = list;
    s.is_unit_scale = (ratio - 1.0).abs() < 1e-12;
    cad.objects.insert(h, ObjectType::Scale(s));
    add_entry(cad, list, &format!("A{count}"), h);
    h
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A representation of `entity` at `factor` times its size, moved by
/// `shift`: its context leaf's payload (MTEXT, TEXT and block references).
fn context(entity: &EntityType, factor: f64, shift: Vector3) -> Option<ObjectContextKind> {
    let add = |a: Vector3| Vector3::new(a.x + shift.x, a.y + shift.y, a.z);
    Some(match entity {
        EntityType::MText(m) => ObjectContextKind::MText(MTextContext {
            attachment: m.attachment_point as i32,
            x_axis_dir: Vector3::new(m.rotation.cos(), m.rotation.sin(), 0.0),
            insertion: add(m.insertion_point),
            rect_width: m.rectangle_width * factor,
            rect_height: m.rectangle_height.unwrap_or(0.0) * factor,
            extents_width: m.rectangle_width * factor,
            extents_height: m.rectangle_height.unwrap_or(m.height) * factor,
            column_type: 0,
            columns: None,
        }),
        EntityType::Text(t) => {
            let at = add(t.insertion_point);
            let align = t.alignment_point.map_or(at, add);
            ObjectContextKind::Text {
                horizontal_mode: t.horizontal_alignment as i16,
                rotation: t.rotation,
                insertion: Vector2::new(at.x, at.y),
                alignment: Vector2::new(align.x, align.y),
            }
        }
        EntityType::Insert(i) => ObjectContextKind::BlkRef {
            rotation: i.rotation,
            insertion: add(i.insert_point),
            scale_factor: Vector3::new(i.x_scale() * factor, i.y_scale() * factor, i.z_scale() * factor),
        },
        _ => return None,
    })
}

/// NATIVESAVE: `doc`'s annotative MTEXT, TEXT and block references (each
/// written as `handles` says) made annotative as AutoCAD keeps them.
pub(crate) fn write(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    let objects: Vec<(Handle, Annotative)> = doc
        .all_entities()
        .filter_map(|e| Some((Handle::new(*handles.get(&e.id)?.first()?), e.annotative.as_deref()?.clone())))
        .collect();
    for (h, a) in objects {
        let Some(entity) = cad.get_entity(h).cloned() else { continue };
        if context(&entity, 1.0, Vector3::ZERO).is_none() || a.scales.is_empty() {
            continue;
        }
        if cad.app_ids.get("AcadAnnotative").is_none() {
            let mut app = acadrust::tables::AppId::new("AcadAnnotative");
            app.handle = cad.allocate_handle();
            let _ = cad.app_ids.add(app);
        }
        // The chain from the object to its leaves.
        let xdict = match entity.common().xdictionary_handle.filter(|x| cad.objects.contains_key(x)) {
            Some(x) => x,
            None => add_dictionary(cad, h, Vec::new()),
        };
        let manager = add_dictionary(cad, xdict, Vec::new());
        add_entry(cad, xdict, "AcDbContextDataManager", manager);
        let scales = add_dictionary(cad, manager, Vec::new());
        add_entry(cad, manager, "ACDB_ANNOTATIONSCALES", scales);
        for (k, s) in a.scales.iter().enumerate() {
            let scale = scale_handle(cad, *s);
            let off = a.offset(*s);
            let Some(kind) = context(&entity, s / a.shown, Vector3::new(off.x, off.y, 0.0)) else { continue };
            let leaf = cad.allocate_handle();
            cad.objects.insert(
                leaf,
                ObjectType::ObjectContextData(ObjectContextData {
                    handle: leaf,
                    owner_handle: scales,
                    reactors: vec![scales],
                    xdictionary_handle: None,
                    class_version: 4,
                    has_file_to_extension_dictionary: false,
                    uses_legacy_mleader_context_base: false,
                    is_default: (s - a.shown).abs() < 1e-9 * s,
                    scale,
                    kind,
                }),
            );
            add_entry(cad, scales, &format!("*A{}", k + 1), leaf);
        }
        if let Some(e) = cad.get_entity_mut(h) {
            let common = e.common_mut();
            common.xdictionary_handle = Some(xdict);
            let mut x = ExtendedDataRecord::new("AcadAnnotative");
            for v in [
                XDataValue::String("AnnotativeData".into()),
                XDataValue::ControlString("{".into()),
                XDataValue::Integer16(1),
                XDataValue::Integer16(1),
                XDataValue::ControlString("}".into()),
            ] {
                x.add_value(v);
            }
            common.extended_data.add_record(x);
            if let EntityType::MText(m) = e {
                m.is_annotative = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::objects::{Dictionary, ObjectContextData, ObjectContextKind, Scale};
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry};

    #[test]
    fn annotative_objects_go_out_as_autocads() {
        let mut doc = Document::new();
        let id = doc.add(Geometry::Text {
            position: Point::new(10.0, 20.0),
            height: 2.5,
            rotation: 0.0,
            width: 40.0,
            attach: oxidraft_model::Attach::TopLeft,
            content: "NOTE".into(),
            look: Default::default(),
        });
        let mut a = Annotative::at(50.0);
        a.scales.push(100.0);
        a.offsets.push((100.0, Point::new(5.0, 0.0)));
        doc.update(id, |e| e.annotative = Some(Box::new(a)));
        crate::set_native_objects(true);
        let dwg = crate::write(&doc, crate::Version::R2018);
        let dxf = crate::write_dxf(&doc);
        crate::set_native_objects(false);
        let (dwg, dxf) = (dwg.unwrap(), dxf.unwrap());
        for (how, mut cad) in [
            ("DWG", acadrust::DwgReader::from_stream(std::io::Cursor::new(dwg)).read().unwrap()),
            ("DXF", acadrust::DxfReader::from_reader(std::io::Cursor::new(dxf)).and_then(|r| r.read()).unwrap()),
        ] {
            let text = cad.entities().find(|e| matches!(e, EntityType::MText(_))).unwrap();
            let apps: Vec<String> =
                text.common().extended_data.records().iter().map(|r| r.application_name.clone()).collect();
            let annotative = matches!(text, EntityType::MText(m) if m.is_annotative);
            assert!(apps.iter().any(|a| a == "AcadAnnotative") || annotative, "{how}: {apps:?}");
            let leaves: Vec<(bool, f64, Vector3)> = cad
                .objects
                .values()
                .filter_map(|o| match o {
                    ObjectType::ObjectContextData(c) => {
                        let ObjectContextKind::MText(m) = &c.kind else { return None };
                        Some((c.is_default, ratio(&cad, c.scale)?, m.insertion))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(leaves.len(), 2, "{how}: {leaves:?}");
            assert!(leaves.iter().any(|(d, r, at)| *d && *r == 50.0 && at.x == 10.0), "{how}: {leaves:?}");
            assert!(leaves.iter().any(|(d, r, at)| !*d && *r == 100.0 && at.x == 15.0), "{how}: moved on its own");
            // Read as AutoCAD's alone (Oxidraft's own data taken off), the
            // scales come back.
            for e in cad.entities_mut() {
                let data = &mut e.common_mut().extended_data;
                let kept: Vec<_> =
                    data.records().iter().filter(|r| r.application_name != "OXIDRAFT_ANNO").cloned().collect();
                *data = Default::default();
                for r in kept {
                    data.add_record(r);
                }
            }
            let back = crate::imported_from(&mut cad).unwrap().document;
            let a = back.entities().find_map(|e| e.annotative.clone()).expect(how);
            assert_eq!((a.scales, a.shown), (vec![50.0, 100.0], 50.0), "{how}");
        }
    }

    #[test]
    fn autocads_annotative_scales_come_in() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let dxf = crate::write_dxf(&doc).unwrap();
        let mut cad = acadrust::DxfReader::from_reader(std::io::Cursor::new(dxf)).and_then(|r| r.read()).unwrap();
        let line = cad.entities().next().unwrap().common().handle;
        // The chain AutoCAD hangs off an annotative object, with its 1:50
        // (the default) and 1:100 representations.
        let mut next = || cad.allocate_handle();
        let (xdict, manager, scales, s50, s100, a1, a2) = (next(), next(), next(), next(), next(), next(), next());
        let dictionary = |handle: Handle, owner: Handle, entries: Vec<(String, Handle)>| {
            let mut d = Dictionary::new();
            d.handle = handle;
            d.owner = owner;
            d.entries = entries;
            ObjectType::Dictionary(d)
        };
        cad.objects.insert(xdict, dictionary(xdict, line, vec![("AcDbContextDataManager".into(), manager)]));
        cad.objects.insert(manager, dictionary(manager, xdict, vec![("ACDB_ANNOTATIONSCALES".into(), scales)]));
        cad.objects.insert(scales, dictionary(scales, manager, vec![("*A1".into(), a1), ("*A2".into(), a2)]));
        for (h, name, units) in [(s50, "1:50", 50.0), (s100, "1:100", 100.0)] {
            let mut s = Scale::new(name, 1.0, units);
            s.handle = h;
            cad.objects.insert(h, ObjectType::Scale(s));
        }
        for (h, scale, default) in [(a1, s50, true), (a2, s100, false)] {
            cad.objects.insert(
                h,
                ObjectType::ObjectContextData(ObjectContextData {
                    handle: h,
                    owner_handle: scales,
                    reactors: vec![scales],
                    xdictionary_handle: None,
                    class_version: 4,
                    has_file_to_extension_dictionary: false,
                    uses_legacy_mleader_context_base: false,
                    is_default: default,
                    scale,
                    kind: ObjectContextKind::AnnotScale,
                }),
            );
        }
        if let Some(e) = cad.get_entity_mut(line) {
            e.common_mut().xdictionary_handle = Some(xdict);
        }
        cad.header.annotation_scale_value = 0.01;
        cad.header.current_annotation_scale = "1:100".into();
        let check = |doc: &Document, how: &str| {
            let a = doc.entities().find_map(|e| e.annotative.clone()).expect(how);
            assert_eq!((a.scales, a.shown), (vec![50.0, 100.0], 50.0), "{how}");
            assert_eq!(doc.anno_scale(), 100.0, "{how}: CANNOSCALE 1:100");
        };
        // AutoCAD's DXF names the annotation scale in its header.
        let dxf = String::from_utf8(acadrust::DxfWriter::new(&cad).write_to_vec().unwrap()).unwrap();
        let dxf = dxf.replacen("$ACADVER", "$CANNOSCALE\n  2\n1:100\n  9\n$ACADVER", 1).into_bytes();
        check(&crate::imported_from(&mut cad).unwrap().document, "as from a DWG");
        check(&crate::read_dxf(&dxf).unwrap().document, "from a DXF");
    }
}

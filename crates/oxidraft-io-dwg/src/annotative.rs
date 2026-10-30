//! AutoCAD's annotative objects coming in annotative. AutoCAD keeps an
//! object's representation for each annotation scale as a context leaf
//! (`AcDb*ObjectContextData`) under its extension dictionary
//! (`AcDbContextDataManager` → `ACDB_ANNOTATIONSCALES` → `*A1`...), each
//! naming an `AcDbScale`: those scales become the object's, the one its own
//! data is at (the default context) the one it shows. The drawing's
//! annotation scale is AutoCAD's CANNOSCALE when Oxidraft's settings don't
//! give one.

use std::collections::HashMap;

use acadrust::objects::ObjectType;
use acadrust::{CadDocument, Handle};
use oxidraft_io_dxf::Imported;
use oxidraft_model::Annotative;

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

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::objects::{Dictionary, ObjectContextData, ObjectContextKind, Scale};
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry};

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

//! AutoCAD's dimension breaks (DIMBREAK) coming in: a BREAKDATA object
//! names its dimension and holds its break point references — a gap
//! between two points (a manual break), or around one (a break where an
//! object crosses, as wide as DIMBREAK's ISO size at the dimension's
//! scale) — which become the dimension's breaks. With NATIVESAVE on, a
//! dimension's breaks go out that way too: a BREAKDATA naming it, a
//! two-point reference for each gap, kept in its extension dictionary as
//! ACAD_BREAKDATA (as AutoCAD keeps DIMASSOC's; untested in AutoCAD).

use std::collections::HashMap;

use acadrust::objects::{BreakData, BreakPointReference, DataObject, DataObjectData, Dictionary, ObjectType};
use acadrust::types::Vector3;
use acadrust::{CadDocument, Handle};
use oxidraft_geom::Point;
use oxidraft_io_dxf::Imported;
use oxidraft_model::{Document, EntityId, Geometry};

/// DIMBREAK's gap at scale 1, as Oxidraft's DIMBREAK makes it.
const BREAK_SIZE: f64 = 3.75;

pub(crate) fn restore(cad: &CadDocument, imported: &mut Imported) {
    for o in cad.objects.values() {
        let ObjectType::DataObject(d) = o else { continue };
        let DataObjectData::BreakData(b) = &d.data else { continue };
        let Some(&id) = imported.handles.get(&b.dimension_reference.value()) else { continue };
        let Some(Geometry::Dimension { rotation, scale, .. }) =
            imported.document.entity(id).map(|e| e.geometry.clone())
        else {
            continue;
        };
        // A one-point break's gap runs along the dimension line.
        let along = Point::new(rotation.to_radians().cos(), rotation.to_radians().sin());
        let mut gaps = Vec::new();
        for r in &b.point_references {
            let a = Point::new(r.first_point.x, r.first_point.y);
            let c = Point::new(r.second_point.x, r.second_point.y);
            if a.distance(c) > 1e-9 && c != Point::ORIGIN {
                gaps.push((a, c));
            } else if a != Point::ORIGIN {
                let half = along * (BREAK_SIZE * scale / 2.0);
                gaps.push((a - half, a + half));
            }
        }
        if gaps.is_empty() {
            continue;
        }
        imported.document.update(id, |e| {
            if let Geometry::Dimension { extra, .. } = &mut e.geometry {
                for g in gaps {
                    if !extra.breaks.contains(&g) {
                        extra.breaks.push(g);
                    }
                }
            }
        });
    }
}

/// NATIVESAVE: each of `doc`'s dimensions with breaks given a BREAKDATA.
pub(crate) fn write(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    for e in doc.all_entities() {
        let Geometry::Dimension { extra, .. } = &e.geometry else { continue };
        if extra.breaks.is_empty() {
            continue;
        }
        let Some(dim) = handles.get(&e.id).and_then(|h| h.first()).map(|h| Handle::new(*h)) else { continue };
        let Some(entity) = cad.get_entity(dim) else { continue };
        let existing = entity.common().xdictionary_handle.filter(|x| cad.objects.contains_key(x));
        let xdict = existing.unwrap_or_else(|| {
            let h = cad.allocate_handle();
            let mut d = Dictionary::new();
            d.handle = h;
            d.owner = dim;
            cad.objects.insert(h, ObjectType::Dictionary(d));
            h
        });
        let point = |p: Point| Vector3::new(p.x, p.y, 0.0);
        let mut data = DataObject::new(DataObjectData::BreakData(BreakData {
            version: 1,
            dimension_reference: dim,
            reserved_reference: Handle::default(),
            point_references: extra
                .breaks
                .iter()
                .enumerate()
                .map(|(k, (a, b))| BreakPointReference {
                    reference_type: 2,
                    identifier: k as i32,
                    first_point: point(*a),
                    second_point: point(*b),
                    ..Default::default()
                })
                .collect(),
        }));
        data.handle = cad.allocate_handle();
        data.owner = xdict;
        data.reactors = vec![xdict];
        let h = data.handle;
        cad.objects.insert(h, ObjectType::DataObject(data));
        if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&xdict) {
            d.entries.push(("ACAD_BREAKDATA".into(), h));
        }
        if let Some(e) = cad.get_entity_mut(dim) {
            e.common_mut().xdictionary_handle = Some(xdict);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::objects::{BreakData, BreakPointReference, DataObject};
    use acadrust::types::Vector3;
    use oxidraft_model::Document;

    #[test]
    fn autocads_dimension_breaks_come_in() {
        let mut doc = Document::new();
        doc.add(Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(40.0, 0.0),
            line: Point::new(0.0, 10.0),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Default::default(),
        });
        let dxf = crate::write_dxf(&doc).unwrap();
        let mut cad = acadrust::DxfReader::from_reader(std::io::Cursor::new(dxf)).and_then(|r| r.read()).unwrap();
        let dim = cad.entities().find(|e| matches!(e, acadrust::EntityType::Dimension(_))).unwrap().common().handle;
        let point = |x: f64, y: f64| Vector3::new(x, y, 0.0);
        let reference =
            |a: Vector3, b: Vector3| BreakPointReference { first_point: a, second_point: b, ..Default::default() };
        let mut data = DataObject::new(DataObjectData::BreakData(BreakData {
            version: 1,
            dimension_reference: dim,
            reserved_reference: Default::default(),
            // A manual gap, and a break where an object crosses at 30,10.
            point_references: vec![
                reference(point(10.0, 10.0), point(14.0, 10.0)),
                reference(point(30.0, 10.0), point(0.0, 0.0)),
            ],
        }));
        data.handle = cad.allocate_handle();
        cad.objects.insert(data.handle, ObjectType::DataObject(data));
        let back = crate::imported_from(&mut cad).unwrap().document;
        // NATIVESAVE: saved again, AutoCAD's form alone gives them back.
        crate::set_native_objects(true);
        let saved = crate::write(&back, crate::Version::R2018);
        crate::set_native_objects(false);
        let mut cad = acadrust::DwgReader::from_stream(std::io::Cursor::new(saved.unwrap())).read().unwrap();
        for e in cad.entities_mut() {
            let data = &mut e.common_mut().extended_data;
            let kept: Vec<_> =
                data.records().iter().filter(|r| r.application_name != "OXIDRAFT_DIM").cloned().collect();
            *data = Default::default();
            for r in kept {
                data.add_record(r);
            }
        }
        let back = crate::imported_from(&mut cad).unwrap().document;
        let breaks = back.entities().find_map(|e| match &e.geometry {
            Geometry::Dimension { extra, .. } => Some(extra.breaks.clone()),
            _ => None,
        });
        assert_eq!(
            breaks.unwrap(),
            [(Point::new(10.0, 10.0), Point::new(14.0, 10.0)), (Point::new(28.125, 10.0), Point::new(31.875, 10.0))]
        );
    }
}

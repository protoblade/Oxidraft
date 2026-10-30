//! DIMINSPECT's data in AutoCAD's own form, with NATIVESAVE on: the
//! dimension's ACAD_DSTYLE_DIMINSPECT XDATA, (1070 code, value) pairs as
//! AutoCAD keeps its other dimension overrides — 396 the rate, 395 the
//! label, 394 the shape and which of them are shown, 393 shown. The codes
//! are as OpenCADStudio reads AutoCAD's, the flags as ODA documents them;
//! untested in AutoCAD. Read back in oxidraft-io-dxf.

use std::collections::HashMap;

use acadrust::CadDocument;
use acadrust::types::Handle;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use oxidraft_model::annotation::{Inspection, InspectionShape};
use oxidraft_model::{Document, EntityId, Geometry};

pub(crate) const APP: &str = "ACAD_DSTYLE_DIMINSPECT";

/// `i` as AutoCAD's record.
fn record(i: &Inspection) -> ExtendedDataRecord {
    let mut flags = match i.shape {
        InspectionShape::Round => 1,
        InspectionShape::Angular => 2,
        InspectionShape::None => 4,
    };
    let mut values = Vec::new();
    if !i.rate.is_empty() {
        flags |= 32;
        values.push((396, XDataValue::String(i.rate.clone())));
    }
    if !i.label.is_empty() {
        flags |= 16;
        values.push((395, XDataValue::String(i.label.clone())));
    }
    values.push((394, XDataValue::Integer16(flags)));
    values.push((393, XDataValue::Integer16(1)));
    let mut r = ExtendedDataRecord::new(APP);
    for (code, v) in values {
        r.add_value(XDataValue::Integer16(code));
        r.add_value(v);
    }
    r
}

/// NATIVESAVE: each of `doc`'s dimensions with inspection data given
/// AutoCAD's record of it.
pub(crate) fn write(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    let mut any = false;
    for e in doc.all_entities() {
        let Geometry::Dimension { extra, .. } = &e.geometry else { continue };
        let Some(i) = &extra.inspection else { continue };
        let Some(&h) = handles.get(&e.id).and_then(|h| h.first()) else { continue };
        let Some(entity) = cad.get_entity_mut(Handle::new(h)) else { continue };
        entity.common_mut().extended_data.add_record(record(i));
        any = true;
    }
    if any && cad.app_ids.get(APP).is_none() {
        let mut app = acadrust::tables::AppId::new(APP);
        app.handle = cad.allocate_handle();
        let _ = cad.app_ids.add(app);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use acadrust::{DwgWriter, DxfReader, DxfWriter};
    use oxidraft_geom::Point;

    use super::*;
    use crate::{Version, read, read_dxf, set_native_objects, write, write_dxf};

    fn inspected(i: &Inspection) -> Document {
        let mut doc = Document::new();
        doc.add(Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(40.0, 0.0),
            line: Point::new(0.0, 10.0),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Box::new(oxidraft_model::annotation::DimExtra { inspection: Some(i.clone()), ..Default::default() }),
        });
        doc
    }

    fn inspection_of(doc: &Document) -> Option<Inspection> {
        doc.entities().find_map(|e| match &e.geometry {
            Geometry::Dimension { extra, .. } => extra.inspection.clone(),
            _ => None,
        })
    }

    /// The drawing's dimensions left with AutoCAD's record alone, as
    /// AutoCAD writes them, its value 393 set to `shown`.
    fn autocads_only(cad: &mut CadDocument, shown: i16) {
        for e in cad.entities_mut() {
            let data = &mut e.common_mut().extended_data;
            let Some(mut r) = data.get_record(APP).cloned() else { continue };
            if let Some(v) = r.values.iter().position(|v| *v == XDataValue::Integer16(393)) {
                r.values[v + 1] = XDataValue::Integer16(shown);
            }
            *data = Default::default();
            data.add_record(r);
        }
    }

    #[test]
    fn inspection_in_autocads_form() {
        let i = Inspection { shape: InspectionShape::Round, label: "A".into(), rate: String::new() };
        let doc = inspected(&i);
        let records = |dxf: Vec<u8>| {
            let cad = DxfReader::from_reader(Cursor::new(dxf)).and_then(|r| r.read()).unwrap();
            cad.entities().filter_map(|e| e.common().extended_data.get_record(APP).cloned()).collect::<Vec<_>>()
        };
        assert!(records(write_dxf(&doc).unwrap()).is_empty(), "only with NATIVESAVE");

        set_native_objects(true);
        let dxf = write_dxf(&doc).unwrap();
        let dwg = write(&doc, Version::R2018).unwrap();
        set_native_objects(false);
        let got = records(dxf.clone());
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].values,
            [
                XDataValue::Integer16(395),
                XDataValue::String("A".into()),
                XDataValue::Integer16(394),
                XDataValue::Integer16(1 | 16),
                XDataValue::Integer16(393),
                XDataValue::Integer16(1),
            ]
        );
        assert_eq!(inspection_of(&read(&dwg).unwrap().imported.document).as_ref(), Some(&i), "DWG");

        // With only AutoCAD's record, as AutoCAD writes it: in DXF and DWG;
        // taken off in AutoCAD (393 0), it's gone.
        for shown in [1, 0] {
            let mut cad = DxfReader::from_reader(Cursor::new(dxf.clone())).and_then(|r| r.read()).unwrap();
            autocads_only(&mut cad, shown);
            let want = (shown == 1).then(|| i.clone());
            let autocad = DxfWriter::new(&cad).write_to_vec().unwrap();
            assert_eq!(inspection_of(&read_dxf(&autocad).unwrap().document), want, "DXF, shown {shown}");
            let mut out = Cursor::new(Vec::new());
            DwgWriter::write_to_writer(&mut out, &cad).unwrap();
            assert_eq!(inspection_of(&read(&out.into_inner()).unwrap().imported.document), want, "DWG, shown {shown}");
        }
    }
}

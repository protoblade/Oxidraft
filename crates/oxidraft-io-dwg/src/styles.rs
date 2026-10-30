//! The drawing's settings AutoCAD keeps in objects of its own, written
//! there too (oxidraft_io_dxf reads AutoCAD's): the multileader style as
//! the Standard MLEADERSTYLE (with AutoCAD's arrowhead blocks), the table
//! style as the Standard TABLESTYLE, IMAGEFRAME and WIPEOUTFRAME in
//! RASTERVARIABLES and WIPEOUTVARIABLES, and LIMCHECK.

use acadrust::entities::{Line, LwPolyline};
use acadrust::objects::{Dictionary, LeaderContentType, ObjectType, RasterVariables, TableStyle, WipeoutVariables};
use acadrust::types::{Color, DxfVersion, Handle, Vector2, Vector3};
use acadrust::{CadDocument, EntityType};
use oxidraft_model::annotation::{DEFAULT_LEADER_STYLE, LeaderArrow};
use oxidraft_model::table::DEFAULT_TABLE_STYLE;
use oxidraft_model::{Document, Geometry};

/// AutoCAD's arrowhead block for `arrow` (the closed filled one needs
/// none), made as AutoCAD draws it where the drawing hasn't it: pointing
/// along x at the origin, a unit long, ByBlock.
pub(crate) fn arrow_block(cad: &mut CadDocument, arrow: LeaderArrow) -> Option<Handle> {
    let name = match arrow {
        LeaderArrow::ClosedFilled => return None,
        LeaderArrow::Open => "_Open",
        LeaderArrow::Dot => "_Dot",
        LeaderArrow::None => "_None",
    };
    if let Some(r) = cad.block_records.iter().find(|r| r.name.eq_ignore_ascii_case(name)) {
        return Some(r.handle);
    }
    let mut record = acadrust::tables::BlockRecord::new(name);
    record.handle = cad.allocate_handle();
    record.block_entity_handle = cad.allocate_handle();
    record.block_end_handle = cad.allocate_handle();
    let handle = record.handle;
    cad.block_records.add(record).ok()?;
    let line = |a: (f64, f64), b: (f64, f64)| {
        EntityType::Line(Line::from_points(Vector3::new(a.0, a.1, 0.0), Vector3::new(b.0, b.1, 0.0)))
    };
    let parts = match arrow {
        LeaderArrow::Open => {
            let h = (18.924644f64 / 2.0).to_radians().sin();
            let v = vec![Vector2::new(-1.0, h), Vector2::new(0.0, 0.0), Vector2::new(-1.0, -h)];
            vec![EntityType::LwPolyline(LwPolyline::from_points(v)), line((0.0, 0.0), (-1.0, 0.0))]
        }
        LeaderArrow::Dot => {
            let mut dot = LwPolyline::from_points(vec![Vector2::new(-0.25, 0.0), Vector2::new(0.25, 0.0)]);
            dot.vertices.iter_mut().for_each(|v| v.bulge = 1.0);
            dot.is_closed = true;
            dot.constant_width = 0.5;
            vec![EntityType::LwPolyline(dot), line((-0.5, 0.0), (-1.0, 0.0))]
        }
        _ => Vec::new(),
    };
    for mut e in parts {
        e.common_mut().color = Color::ByBlock;
        e.common_mut().layer = "0".into();
        if let Ok(h) = cad.add_entity(e) {
            crate::mleaders::move_into(cad, h, handle);
        }
    }
    Some(handle)
}

/// The object the root dictionary names `key`, made by `make` (owned by
/// it) when there's none.
fn named_object(cad: &mut CadDocument, key: &str, make: impl FnOnce(Handle, Handle) -> ObjectType) -> Handle {
    let root = cad.header.named_objects_dict_handle;
    let found = match cad.objects.get(&root) {
        Some(ObjectType::Dictionary(d)) => d.get(key).filter(|h| cad.objects.contains_key(h)),
        _ => None,
    };
    found.unwrap_or_else(|| {
        let h = cad.allocate_handle();
        cad.objects.insert(h, make(h, root));
        if let Some(ObjectType::Dictionary(r)) = cad.objects.get_mut(&root) {
            r.add_entry(key, h);
        }
        h
    })
}

/// The Standard TABLESTYLE, made (in an ACAD_TABLESTYLE dictionary) when
/// there's none.
fn standard_table_style(cad: &mut CadDocument) -> Handle {
    let found = cad.objects.iter().find_map(|(h, o)| match o {
        ObjectType::TableStyle(s) if s.name.eq_ignore_ascii_case(TableStyle::STANDARD) => Some(*h),
        _ => None,
    });
    if let Some(h) = found {
        return h;
    }
    let dict = named_object(cad, "ACAD_TABLESTYLE", |h, owner| {
        let mut d = Dictionary::new();
        d.handle = h;
        d.owner = owner;
        ObjectType::Dictionary(d)
    });
    let h = cad.allocate_handle();
    let mut style = TableStyle::new(TableStyle::STANDARD);
    style.handle = h;
    style.owner_handle = dict;
    cad.objects.insert(h, ObjectType::TableStyle(style));
    if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&dict) {
        d.add_entry(TableStyle::STANDARD, h);
    }
    h
}

pub(crate) fn finish(cad: &mut CadDocument, doc: &Document, version: DxfVersion) {
    cad.header.limit_check = doc.limits_check();
    // The multileader style: AutoCAD 2007's MLEADERSTYLE.
    let l = doc.leader_style();
    let leaders = doc.all_entities().any(|e| matches!(e.geometry, Geometry::Leader { .. }));
    if version >= DxfVersion::AC1021 && (leaders || l != DEFAULT_LEADER_STYLE) {
        let text_style = cad.text_styles.get("Standard").map(|s| s.handle);
        let h = crate::mleaders::standard_style(cad, text_style);
        let arrow = arrow_block(cad, l.arrowhead);
        let tag = l.block.and_then(|t| cad.block_records.iter().find(|r| r.name == t.block_name()).map(|r| r.handle));
        if let Some(ObjectType::MultiLeaderStyle(s)) = cad.objects.get_mut(&h) {
            s.arrowhead_handle = arrow;
            s.arrowhead_size = l.arrow_size;
            s.text_height = l.text_height;
            s.enable_landing = l.landing_on;
            s.enable_dogleg = l.landing_on;
            s.landing_distance = l.landing;
            s.landing_gap = 0.0;
            s.scale_factor = 1.0;
            s.is_annotative = l.annotative;
            if let Some(tag) = tag {
                s.content_type = LeaderContentType::Block;
                s.block_content_handle = Some(tag);
            }
        }
    }
    // The table style: AutoCAD 2005's TABLESTYLE, its title a size up.
    let t = doc.table_style();
    if version >= DxfVersion::AC1018 && t != DEFAULT_TABLE_STYLE {
        let h = standard_table_style(cad);
        if let Some(ObjectType::TableStyle(s)) = cad.objects.get_mut(&h) {
            s.horizontal_margin = t.margin;
            s.vertical_margin = t.margin;
            s.data_row_style.text_height = t.text_height;
            s.header_row_style.text_height = t.text_height;
            s.title_row_style.text_height = t.text_height * 1.4;
        }
    }
    // Image and wipeout frames (AutoCAD's default, 1, needs nothing).
    let image = i16::from(doc.picture_frame(false));
    if image != 1 {
        let h = named_object(cad, "ACAD_IMAGE_VARS", |h, owner| {
            ObjectType::RasterVariables(RasterVariables { handle: h, owner, ..RasterVariables::new() })
        });
        if let Some(ObjectType::RasterVariables(v)) = cad.objects.get_mut(&h) {
            v.display_image_frame = image;
        }
    }
    let wipeout = i16::from(doc.wipeout_frame());
    if wipeout != 1 {
        let h = named_object(cad, "ACAD_WIPEOUT_VARS", |h, owner| {
            ObjectType::WipeoutVariables(WipeoutVariables { handle: h, owner, ..WipeoutVariables::new() })
        });
        if let Some(ObjectType::WipeoutVariables(v)) = cad.objects.get_mut(&h) {
            v.display_frame = wipeout;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_geom::Point;
    use oxidraft_model::annotation::LeaderStyle;
    use std::io::Cursor;

    #[test]
    fn settings_in_autocads_objects_and_back() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) });
        let leader = LeaderStyle {
            arrowhead: LeaderArrow::Open,
            arrow_size: 4.0,
            text_height: 3.5,
            landing_on: true,
            landing: 6.0,
            block: None,
            annotative: false,
        };
        let table = oxidraft_model::table::TableStyle { text_height: 3.0, margin: 1.0 };
        doc.set_annotation_styles(leader, table);
        doc.set_picture_frame(false, 0);
        doc.set_wipeout_frame(2);
        doc.set_limits_check(true);
        let dwg = crate::write(&doc, crate::Version::R2018).unwrap();
        let mut cad = acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().unwrap();
        // As AutoCAD reads them.
        // (A DWG keeps a multileader style's name in its dictionary only.)
        let root = cad.header.named_objects_dict_handle;
        let entry = |dict: &str, name: &str| match cad.objects.get(&root) {
            Some(ObjectType::Dictionary(r)) => match cad.objects.get(&r.get(dict)?) {
                Some(ObjectType::Dictionary(d)) => d.get(name),
                _ => None,
            },
            _ => None,
        };
        let standard = entry("ACAD_MLEADERSTYLE", "Standard").unwrap();
        let mut seen = 0;
        for (h, o) in &cad.objects {
            match o {
                ObjectType::MultiLeaderStyle(s) if *h == standard => {
                    assert_eq!((s.arrowhead_size, s.text_height, s.landing_distance), (4.0, 3.5, 6.0));
                    let arrow = cad.block_records.iter().find(|r| Some(r.handle) == s.arrowhead_handle).unwrap();
                    assert_eq!((arrow.name.as_str(), arrow.entity_handles.len()), ("_Open", 2));
                    seen += 1;
                }
                ObjectType::TableStyle(t) if t.name == "Standard" => {
                    assert_eq!((t.horizontal_margin, t.data_row_style.text_height), (1.0, 3.0));
                    assert!((t.title_row_style.text_height - 4.2).abs() < 1e-9);
                    seen += 1;
                }
                ObjectType::RasterVariables(v) => {
                    assert_eq!(v.display_image_frame, 0);
                    seen += 1;
                }
                ObjectType::WipeoutVariables(v) => {
                    assert_eq!(v.display_frame, 2);
                    seen += 1;
                }
                _ => {}
            }
        }
        assert_eq!(seen, 4);
        assert!(cad.header.limit_check);
        // Without Oxidraft's record, as AutoCAD would save them: read from
        // its objects, in DWG and in DXF.
        if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&root) {
            d.entries.retain(|(n, _)| !n.starts_with("OXIDRAFT"));
        }
        let ours: Vec<String> =
            cad.app_ids.iter().map(|a| a.name.clone()).filter(|n| n.starts_with("OXIDRAFT")).collect();
        for name in ours {
            cad.app_ids.remove(&name);
        }
        let check = |back: &Document, how: &str| {
            assert_eq!((back.leader_style(), back.table_style()), (leader, table), "{how}");
            assert_eq!((back.picture_frame(false), back.wipeout_frame()), (0, 2), "{how}");
            assert!(back.limits_check(), "{how}");
        };
        let dwg = acadrust::DwgWriter::write_to_vec(&cad).unwrap();
        check(&crate::read(&dwg).unwrap().imported.document, "DWG");
        let dxf = acadrust::DxfWriter::new(&cad).write_to_vec().unwrap();
        assert!(!String::from_utf8_lossy(&dxf).contains("OXIDRAFT"));
        let mut back = crate::read_dxf(&dxf).unwrap().document;
        // acadrust's DXF leaves $LIMCHECK out (Oxidraft's puts it in).
        back.set_limits_check(true);
        check(&back, "DXF");
        let ours = crate::write_dxf(&doc).unwrap();
        assert!(String::from_utf8_lossy(&ours).contains("$LIMCHECK\r\n 70\r\n     1\r\n"));
        assert!(crate::read_dxf(&ours).unwrap().document.limits_check());
    }
}

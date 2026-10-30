//! The drawing's settings as AutoCAD keeps them in objects of its own:
//! the current multileader style (MLEADERSTYLE) and table style
//! (TABLESTYLE), and IMAGEFRAME and WIPEOUTFRAME (RASTERVARIABLES,
//! WIPEOUTVARIABLES). Read from drawings Oxidraft didn't write; for those
//! it did, its own record says (oxidraft_io_dwg writes both).

use oxidraft_model::annotation::{LeaderArrow, TagShape};
use oxidraft_model::{Document, MODEL};

use crate::layout_dxf::RawLayouts;

pub(crate) fn read(doc: &mut Document, raw: &RawLayouts) {
    // Named views on the layout AutoCAD's record of them names; paper space
    // ones without it on the first layout (Oxidraft's own record says
    // which, for its own).
    let first = doc.layouts().iter().min_by_key(|l| l.tab_order).map(|l| l.id);
    for (name, flags, xdict) in &raw.views {
        let space = match view_layout(raw, *xdict) {
            Some(l) if l == "MODEL" => Some(MODEL),
            Some(l) => doc.layouts().iter().find(|x| x.name.eq_ignore_ascii_case(l)).map(|x| x.id),
            None => first.filter(|_| flags & 1 != 0),
        };
        if let Some(space) = space
            && let Some(mut v) = doc.views().iter().find(|v| &v.name == name && v.space != space).cloned()
        {
            v.space = space;
            let _ = doc.save_view(v);
        }
    }
    let ours = raw.apps.iter().any(|a| a.to_ascii_uppercase().starts_with("OXIDRAFT"))
        || raw.dictionaries.values().flatten().any(|(n, _)| n == crate::SETTINGS_RECORD);
    if ours {
        return;
    }
    // A variable AutoCAD keeps in its AcDbVariableDictionary (as it does
    // CMLEADERSTYLE and CTABLESTYLE), or in the header.
    let variable = |name: &str| {
        let dict = raw.dictionaries.values().flatten().find(|(n, _)| n == "AcDbVariableDictionary").map(|(_, h)| *h);
        let entry = dict.and_then(|d| raw.dictionaries.get(&d)).and_then(|e| e.iter().find(|(n, _)| n == name));
        let object = entry.and_then(|(_, h)| raw.setting_objects.iter().find(|o| o.0 == "DICTIONARYVAR" && o.1 == *h));
        object
            .and_then(|o| o.2.iter().find(|(c, _)| *c == 1).map(|(_, v)| v.as_str()))
            .or_else(|| raw.header.get(&format!("${name}")).map(String::as_str))
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    // The current style of a kind (Standard, if none is named or the one
    // named isn't there): its group codes.
    let style = |kind: &str, dictionary: &str, current: &str| {
        let name = variable(current).unwrap_or("Standard");
        let dict = raw.dictionaries.values().flatten().find(|(n, _)| n == dictionary).map(|(_, h)| *h);
        let entries = dict.and_then(|d| raw.dictionaries.get(&d));
        let named = |n: &str| entries.and_then(|e| e.iter().find(|(k, _)| k.eq_ignore_ascii_case(n))).map(|(_, h)| *h);
        let handle = named(name).or_else(|| named("Standard"))?;
        raw.setting_objects.iter().find(|o| o.0 == kind && o.1 == handle).map(|o| &o.2)
    };
    let first = |pairs: &[(i32, String)], code: i32| {
        pairs.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.trim().parse::<f64>().ok())
    };
    let block = |pairs: &[(i32, String)], code: i32| {
        let h = pairs.iter().find(|(c, _)| *c == code).and_then(|(_, v)| u64::from_str_radix(v.trim(), 16).ok())?;
        raw.records.get(&h).cloned()
    };
    let (mut l, mut t) = (doc.leader_style(), doc.table_style());
    if let Some(p) = style("MLEADERSTYLE", "ACAD_MLEADERSTYLE", "CMLEADERSTYLE") {
        let num = |code| first(p, code);
        // Its sizes are on paper when annotative, else times its scale.
        l.annotative = num(296).is_some_and(|v| v != 0.0);
        let scale = if l.annotative { 1.0 } else { num(142).filter(|s| *s > 0.0).unwrap_or(1.0) };
        if let Some(v) = num(44).filter(|v| *v > 0.0) {
            l.arrow_size = v * scale;
        }
        if let Some(v) = num(45).filter(|v| *v > 0.0) {
            l.text_height = v * scale;
        }
        if let Some(on) = num(291).or_else(|| num(290)) {
            l.landing_on = on != 0.0;
        }
        if let Some(v) = num(43).filter(|v| *v >= 0.0) {
            l.landing = v * scale;
        }
        l.arrowhead = match block(p, 341).as_deref() {
            Some("_OPEN" | "_OPEN30" | "_OPEN90") => LeaderArrow::Open,
            Some("_DOT" | "_DOTSMALL" | "_DOTBLANK" | "_SMALL") => LeaderArrow::Dot,
            Some("_NONE") => LeaderArrow::None,
            _ => LeaderArrow::ClosedFilled,
        };
        l.block = match num(170) {
            Some(1.0) => {
                block(p, 343).and_then(|n| TagShape::ALL.into_iter().find(|t| t.block_name().eq_ignore_ascii_case(&n)))
            }
            _ => None,
        };
    }
    // A table style's margin, and its data rows' text height (its first).
    if let Some(p) = style("TABLESTYLE", "ACAD_TABLESTYLE", "CTABLESTYLE") {
        if let Some(m) = first(p, 40).filter(|m| *m >= 0.0) {
            t.margin = m;
        }
        if let Some(h) = first(p, 140).filter(|h| *h > 0.0) {
            t.text_height = h;
        }
    }
    doc.set_annotation_styles(l, t);
    // PDFFRAME and ANNOALLVISIBLE, where the variable dictionary has them.
    if let Some(f) = variable("PDFFRAME").and_then(|v| v.parse::<u8>().ok()) {
        doc.set_picture_frame(true, f);
    }
    if let Some(on) = variable("ANNOALLVISIBLE").and_then(|v| v.parse::<i32>().ok()) {
        doc.set_anno_all_visible(on != 0);
    }
    for (kind, _, p) in &raw.setting_objects {
        let frame = p.iter().find(|(c, _)| *c == 70).and_then(|(_, v)| v.trim().parse::<u8>().ok());
        match (kind.as_str(), frame) {
            ("RASTERVARIABLES", Some(f)) => doc.set_picture_frame(false, f),
            ("WIPEOUTVARIABLES", Some(f)) => doc.set_wipeout_frame(f),
            _ => {}
        }
    }
}

/// The upper-case name of the layout a view's ADSK_XREC_VTRVIEWINFO (in
/// its extension dictionary `xdict`) names: the record's own 330, after
/// its owner's.
fn view_layout(raw: &RawLayouts, xdict: u64) -> Option<&String> {
    let (_, record) = raw.dictionaries.get(&xdict)?.iter().find(|(n, _)| n == "ADSK_XREC_VTRVIEWINFO")?;
    let codes = raw.xrecords.get(record)?;
    let body = codes.iter().position(|(c, v)| *c == 100 && v == "AcDbXrecord")?;
    let (_, layout) = codes[body..].iter().find(|(c, _)| *c == 330)?;
    raw.layout_names.get(&u64::from_str_radix(layout, 16).ok()?)
}

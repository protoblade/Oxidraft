//! Oxidraft's own settings on the way out: an XRECORD in the root
//! dictionary (see `oxidraft_io_dxf::SETTINGS_RECORD`).

use acadrust::CadDocument;
use acadrust::objects::{ObjectType, XRecord, XRecordEntry};
use oxidraft_model::Document;

pub(crate) fn finish(cad: &mut CadDocument, doc: &Document) {
    let fresh = Document::new();
    let settings = oxidraft_io_dxf::settings_of(doc);
    // Nothing set: nothing written.
    if settings != oxidraft_io_dxf::settings_of(&fresh) {
        let mut entries = Vec::new();
        for (key, value) in settings {
            entries.push(XRecordEntry::string(1, key));
            entries.push(XRecordEntry::double(40, value));
        }
        add_record(cad, oxidraft_io_dxf::SETTINGS_RECORD, entries);
    }
    // DWGPROPS, as AutoCAD keeps it: a cookie, then title (2), subject
    // (3), author (4), comments (6) and keywords (7).
    let p = doc.properties();
    if *p != oxidraft_model::DrawingProperties::default() {
        let mut entries = vec![XRecordEntry::string(1, "DWGPROPS COOKIE")];
        for (code, value) in [(2, &p.title), (3, &p.subject), (4, &p.author), (6, &p.comments), (7, &p.keywords)] {
            entries.push(XRecordEntry::string(code, value.clone()));
        }
        entries.extend([XRecordEntry::string(8, ""), XRecordEntry::string(9, "")]);
        add_record(cad, "DWGPROPS", entries);
    }
    // Layer states: each its name (1), description (3) and current layer
    // (2), then its layers: name (8), on 1 / frozen 2 / locked 4 (90),
    // color (62), linetype (6), lineweight (370) and transparency (440).
    if !doc.layer_states().is_empty() {
        let mut entries = Vec::new();
        for s in doc.layer_states() {
            entries.extend([
                XRecordEntry::string(1, s.name.clone()),
                XRecordEntry::string(3, s.description.clone()),
                XRecordEntry::string(2, s.current.clone()),
            ]);
            for l in &s.layers {
                let flags = i32::from(l.on) | i32::from(l.frozen) << 1 | i32::from(l.locked) << 2;
                entries.extend([
                    XRecordEntry::string(8, l.name.clone()),
                    XRecordEntry::int32(90, flags),
                    XRecordEntry::int16(62, i16::from(l.color)),
                    XRecordEntry::string(6, l.linetype.clone()),
                    XRecordEntry::int16(370, l.lineweight.to_dxf()),
                    XRecordEntry::int32(440, i32::from(l.transparency)),
                ]);
            }
        }
        add_record(cad, oxidraft_io_dxf::LAYER_STATES_RECORD, entries);
    }
    // ULAYERS: each PDF underlay's name (1), then its layers turned off (8).
    let off: Vec<_> = doc.all_pdf_layers_off().collect();
    if !off.is_empty() {
        let mut entries = Vec::new();
        for (name, layers) in off {
            entries.push(XRecordEntry::string(1, name.clone()));
            entries.extend(layers.iter().map(|l| XRecordEntry::string(8, l.clone())));
        }
        add_record(cad, oxidraft_io_dxf::PDF_LAYERS_RECORD, entries);
    }
    // Files embedded (INSERTOBJ) that objects link to: each one's name (1),
    // then its contents in chunks (310).
    let linked = |name: &str| {
        let url = format!("{}{name}", oxidraft_io_dxf::EMBEDDED_PREFIX);
        doc.all_entities().any(|e| e.hyperlink.as_ref().is_some_and(|l| l.url == url))
    };
    let embedded: Vec<_> = doc.embedded_files().filter(|(n, _)| linked(n)).collect();
    if !embedded.is_empty() {
        let mut entries = Vec::new();
        for (name, bytes) in embedded {
            entries.push(XRecordEntry::string(1, name.clone()));
            entries.extend(
                bytes.chunks(127).map(|c| XRecordEntry::new(310, acadrust::objects::XRecordValue::Chunk(c.to_vec()))),
            );
        }
        add_record(cad, oxidraft_io_dxf::EMBEDDED_RECORD, entries);
    }
    // Dynamic blocks: each block's name (1), then its definition's lines (3).
    let dynamic: Vec<_> = doc.all_dynamic_blocks().collect();
    if !dynamic.is_empty() {
        let mut entries = Vec::new();
        for (name, def) in dynamic {
            let name = doc.block(name).map_or_else(|| name.clone(), |b| b.name.clone());
            entries.push(XRecordEntry::string(1, name));
            entries.extend(def.to_text().lines().map(|l| XRecordEntry::string(3, l.to_string())));
        }
        add_record(cad, oxidraft_io_dxf::DYNAMIC_BLOCKS_RECORD, entries);
    }
}

/// An XRECORD of `entries` in the root dictionary under `name`.
fn add_record(cad: &mut CadDocument, name: &str, entries: Vec<XRecordEntry>) {
    let root = cad.header.named_objects_dict_handle;
    let handle = cad.allocate_handle();
    let mut record = XRecord::new();
    record.handle = handle;
    record.owner = root;
    for e in entries {
        record.add_entry(e);
    }
    cad.objects.insert(handle, ObjectType::XRecord(record));
    if let Some(ObjectType::Dictionary(r)) = cad.objects.get_mut(&root) {
        r.add_entry(name, handle);
    }
}

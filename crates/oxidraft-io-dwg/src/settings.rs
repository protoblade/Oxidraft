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

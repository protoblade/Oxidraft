//! Oxidraft's own settings on the way out: an XRECORD in the root
//! dictionary (see `oxidraft_io_dxf::SETTINGS_RECORD`).

use acadrust::CadDocument;
use acadrust::objects::{ObjectType, XRecord, XRecordEntry};
use oxidraft_model::Document;

pub(crate) fn finish(cad: &mut CadDocument, doc: &Document) {
    let fresh = Document::new();
    let settings = oxidraft_io_dxf::settings_of(doc);
    // Nothing set: nothing written.
    if settings == oxidraft_io_dxf::settings_of(&fresh) {
        return;
    }
    let root = cad.header.named_objects_dict_handle;
    let handle = cad.allocate_handle();
    let mut record = XRecord::new();
    record.handle = handle;
    record.owner = root;
    for (key, value) in settings {
        record.add_entry(XRecordEntry::string(1, key));
        record.add_entry(XRecordEntry::double(40, value));
    }
    cad.objects.insert(handle, ObjectType::XRecord(record));
    if let Some(ObjectType::Dictionary(r)) = cad.objects.get_mut(&root) {
        r.add_entry(oxidraft_io_dxf::SETTINGS_RECORD, handle);
    }
}

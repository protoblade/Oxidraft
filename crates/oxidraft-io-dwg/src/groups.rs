//! Groups on the way out: each a GROUP object in the ACAD_GROUP
//! dictionary, pointing at the handles its objects were written with.

use std::collections::HashMap;

use acadrust::CadDocument;
use acadrust::objects::{Dictionary, Group, ObjectType};
use acadrust::types::Handle;
use oxidraft_model::{Document, EntityId};

/// Adds `doc`'s groups to `cad`; `handles` are its objects' handles there.
pub(crate) fn finish(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    let groups = doc.groups();
    if groups.is_empty() {
        return;
    }
    let mut dict = cad.header.acad_group_dict_handle;
    if !matches!(cad.objects.get(&dict), Some(ObjectType::Dictionary(_))) {
        // No ACAD_GROUP yet: one in the root dictionary.
        dict = cad.allocate_handle();
        let root = cad.header.named_objects_dict_handle;
        let mut d = Dictionary::new();
        d.handle = dict;
        d.owner = root;
        cad.objects.insert(dict, ObjectType::Dictionary(d));
        if let Some(ObjectType::Dictionary(r)) = cad.objects.get_mut(&root) {
            r.add_entry("ACAD_GROUP", dict);
        }
        cad.header.acad_group_dict_handle = dict;
    }
    for g in groups {
        let handle = cad.allocate_handle();
        let mut group = if g.is_unnamed() { Group::unnamed() } else { Group::new(g.name.clone()) };
        group.name = g.name.clone();
        group.handle = handle;
        group.owner = dict;
        group.description = g.description.clone();
        group.entities =
            g.members.iter().flat_map(|id| handles.get(id).into_iter().flatten()).map(|h| Handle::new(*h)).collect();
        cad.objects.insert(handle, ObjectType::Group(group));
        if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&dict) {
            d.add_entry(g.name.clone(), handle);
        }
    }
}

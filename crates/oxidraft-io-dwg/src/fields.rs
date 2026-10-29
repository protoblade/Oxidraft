//! Fields: Oxidraft keeps them as codes in the text, AutoCAD as FIELD
//! objects in the text's dictionary with the text holding their values.

use std::collections::HashMap;

use acadrust::fields::NewField;
use acadrust::objects::{Field, ObjectType};
use acadrust::types::Handle;
use acadrust::{CadDocument, EntityType};
use oxidraft_io_dxf::Imported;
use oxidraft_model::{Document, EntityId, Geometry, field};

/// Each text's fields as AutoCAD's FIELD objects.
pub(crate) fn finish(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    write(cad, doc, handles, true);
}

/// Each text with its fields' values, for versions before fields.
pub(crate) fn flatten(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    write(cad, doc, handles, false);
}

fn write(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>, native_fields: bool) {
    for e in doc.all_entities() {
        let Geometry::Text { content, .. } = &e.geometry else { continue };
        if !field::has_fields(content) {
            continue;
        }
        let Some(host) = handles.get(&e.id).and_then(|h| h.first()).map(|h| Handle::new(*h)) else { continue };
        let text = match cad.get_entity(host) {
            Some(EntityType::MText(m)) => m.value.clone(),
            Some(EntityType::Text(t)) => t.value.clone(),
            _ => continue,
        };
        let (mut template, mut children, mut last) = (String::new(), Vec::new(), 0);
        for (range, code) in field::fields(&text) {
            template.push_str(&text[last..range.start]);
            match native(code, doc, handles).filter(|_| native_fields) {
                Some(child) => {
                    template.push_str(&format!("%<\\_FldIdx {}>%", children.len()));
                    children.push(child);
                }
                None => template.push_str(&field::value(code, doc).unwrap_or_else(|| field::UNKNOWN.into())),
            }
            last = range.end;
        }
        template.push_str(&text[last..]);
        if children.is_empty() {
            set_text(cad, host, template);
        } else {
            cad.set_text_field(host, &template, children);
        }
    }
}

fn set_text(cad: &mut CadDocument, host: Handle, text: String) {
    match cad.get_entity_mut(host) {
        Some(EntityType::MText(m)) => m.value = text,
        Some(EntityType::Text(t)) => t.value = text,
        _ => {}
    }
}

/// A field as AutoCAD's, with its value now; an object field names its
/// object by handle.
fn native(code: &str, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) -> Option<NewField> {
    let shown = field::value(code, doc).unwrap_or_else(|| field::UNKNOWN.into());
    let Some(id) = field::object_id(code) else {
        return Some(NewField::new(code, shown));
    };
    let handle = *handles.get(&id)?.first()?;
    let code = code.replace(&format!("%<\\_ObjId {}>%", id.0), "%<\\_ObjIdx 0>%");
    let mut f = NewField::new(code, shown);
    f.objects = vec![Handle::new(handle)];
    Some(f)
}

/// Brings back the fields of an AutoCAD drawing's texts: each field's code
/// goes where its value is in the text as read.
pub(crate) fn restore(cad: &CadDocument, imported: &mut Imported) {
    let field_of = |h: &Handle| match cad.objects.get(h) {
        Some(ObjectType::Field(f)) => Some(f),
        _ => None,
    };
    let owner = |h: Handle| match cad.objects.get(&h) {
        Some(ObjectType::Dictionary(d)) => Some(d.owner),
        _ => None,
    };
    let mut restored = false;
    for container in cad.objects.values().filter_map(|o| match o {
        ObjectType::Field(f) if f.evaluator_id == "_text" => Some(f),
        _ => None,
    }) {
        // The container is in the ACAD_FIELD dictionary of the text's own.
        let Some(host) = owner(container.owner).and_then(owner) else { continue };
        let Some(&id) = imported.handles.get(&host.value()) else { continue };
        let doc = &mut imported.document;
        let Some(Geometry::Text { content, .. }) = doc.entity(id).map(|e| &e.geometry) else { continue };
        let mut out = String::new();
        let mut rest = content.as_str();
        let mut whole = true;
        for child in container.child_fields.iter().filter_map(field_of) {
            let (Some(code), Some(p)) = (oxidraft_code(child, &imported.handles), rest.find(&cached(child))) else {
                whole = false;
                break;
            };
            out.push_str(&rest[..p]);
            out.push_str(&format!("%<{code}>%"));
            rest = &rest[p + cached(child).len()..];
        }
        if !whole || out.is_empty() {
            continue;
        }
        out.push_str(rest);
        doc.update(id, |e| {
            if let Geometry::Text { content, .. } = &mut e.geometry {
                *content = out;
            }
        });
        restored = true;
    }
    if restored {
        imported.document.forget_history();
    }
}

/// The value AutoCAD last showed for a field.
fn cached(f: &Field) -> String {
    match f.value.display() {
        "" => f.value_string.clone(),
        shown => shown.to_string(),
    }
}

/// A field's code as Oxidraft keeps it: an object field names its object
/// by id.
fn oxidraft_code(f: &Field, handles: &HashMap<u64, EntityId>) -> Option<String> {
    let mut code = f.code.trim().to_string();
    for (i, h) in f.referenced_objects.iter().enumerate() {
        let id = handles.get(&h.value())?;
        code = code.replace(&format!("%<\\_ObjIdx {i}>%"), &format!("%<\\_ObjId {}>%", id.0));
    }
    (!code.contains("\\_ObjIdx")).then_some(code)
}

//! Associative dimensions as AutoCAD keeps them: a DIMASSOC object in
//! each dimension's extension dictionary, an osnap point reference to an
//! object for each associated definition point.

use std::collections::HashMap;

use acadrust::objects::{
    AssocDimensionAssociation, AssocDimensionReference, AssociativeData, AssociativeObject, Dictionary, ObjectType,
};
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use oxidraft_io_dxf::Imported;
use oxidraft_model::dimassoc::{AssocKind, assoc_point, reference_of_kind};
use oxidraft_model::{Document, EntityId};

/// AutoCAD's osnap type for each kind of point.
fn osnap_type(kind: AssocKind) -> u8 {
    match kind {
        AssocKind::Endpoint => 1,
        AssocKind::Midpoint => 2,
        AssocKind::Center => 3,
        AssocKind::Node => 4,
        AssocKind::Quadrant => 5,
        AssocKind::Insertion => 7,
        AssocKind::OnCircle => 10,
    }
}

fn kind_of(osnap: u8) -> Option<AssocKind> {
    Some(match osnap {
        1 => AssocKind::Endpoint,
        2 => AssocKind::Midpoint,
        3 => AssocKind::Center,
        4 => AssocKind::Node,
        5 => AssocKind::Quadrant,
        7 => AssocKind::Insertion,
        10 => AssocKind::OnCircle,
        _ => return None,
    })
}

/// Each associative dimension's DIMASSOC.
pub(crate) fn finish(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    let handle_of = |id: EntityId| handles.get(&id).and_then(|h| h.first()).map(|h| Handle::new(*h));
    for (dim, refs) in doc.all_dim_assoc() {
        let Some(dim_handle) = handle_of(*dim) else { continue };
        if !matches!(cad.get_entity(dim_handle), Some(EntityType::Dimension(_))) {
            continue;
        }
        let mut assoc = AssocDimensionAssociation { dimension: dim_handle, ..Default::default() };
        let mut targets = Vec::new();
        for (slot, r) in refs.iter().enumerate().take(4) {
            let Some(r) = r else { continue };
            let (Some(target), Some(at)) =
                (handle_of(r.target), doc.entity(r.target).and_then(|t| assoc_point(&t.geometry, r)))
            else {
                continue;
            };
            assoc.associativity |= 1 << slot;
            assoc.references[slot] = vec![AssocDimensionReference {
                class_name: "AcDbOsnapPointRef".into(),
                osnap_type: osnap_type(r.kind),
                xrefs: vec![target],
                main_subent_type: 1,
                main_gs_marker: r.index as i32 + 1,
                osnap_point: Vector3::new(at.x, at.y, 0.0),
                ..Default::default()
            }];
            targets.push(target);
        }
        if assoc.associativity == 0 {
            continue;
        }
        // The dimension's extension dictionary, holding it as ACAD_DIMASSOC.
        let (xdict, assoc_handle) = (cad.allocate_handle(), cad.allocate_handle());
        let mut d = Dictionary::new();
        d.handle = xdict;
        d.owner = dim_handle;
        d.hard_owner = true;
        d.add_entry("ACAD_DIMASSOC", assoc_handle);
        cad.objects.insert(xdict, ObjectType::Dictionary(d));
        let mut object = AssociativeObject::new("DIMASSOC", "AcDbDimAssoc");
        object.handle = assoc_handle;
        object.owner = xdict;
        object.reactors = vec![xdict];
        object.data = AssociativeData::DimensionAssociation(assoc);
        cad.objects.insert(assoc_handle, ObjectType::Associative(object));
        if let Some(e) = cad.get_entity_mut(dim_handle) {
            e.common_mut().xdictionary_handle = Some(xdict);
        }
        // The objects it follows tell it when they change.
        for t in targets {
            if let Some(e) = cad.get_entity_mut(t)
                && !e.common().reactors.contains(&assoc_handle)
            {
                e.common_mut().reactors.push(assoc_handle);
            }
        }
    }
}

/// The dimensions' associations, from their DIMASSOCs.
pub(crate) fn restore(cad: &CadDocument, imported: &mut Imported) {
    let mut any = false;
    for object in cad.objects.values() {
        let ObjectType::Associative(AssociativeObject { data: AssociativeData::DimensionAssociation(a), .. }) = object
        else {
            continue;
        };
        let Some(&dim) = imported.handles.get(&a.dimension.value()) else { continue };
        let doc = &mut imported.document;
        let Some(points) = doc.entity(dim).map(|e| e.geometry.dim_def_points().len()) else { continue };
        let mut refs = vec![None; points];
        for (slot, slot_refs) in a.references.iter().enumerate().take(points) {
            if a.associativity & (1 << slot) == 0 {
                continue;
            }
            let Some(r) = slot_refs.first() else { continue };
            let (Some(kind), Some(target)) =
                (kind_of(r.osnap_type), r.xrefs.first().and_then(|h| imported.handles.get(&h.value())))
            else {
                continue;
            };
            let at = oxidraft_geom::Point::new(r.osnap_point.x, r.osnap_point.y);
            refs[slot] = doc.entity(*target).and_then(|e| reference_of_kind(e, kind, at));
        }
        doc.set_dim_assoc(dim, Some(refs));
        any = true;
    }
    if any {
        imported.document.forget_history();
    }
}

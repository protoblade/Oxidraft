//! Associative arrays as AutoCAD keeps them. AutoCAD's come in as arrays:
//! an array action body's items (each one's place in the rows and columns
//! and the reference that shows it) give a rectangular or polar array's
//! layout, as in AutoCAD's drawings (a path array's path is what its other
//! dependency is on). With NATIVESAVE, Oxidraft's arrays go out that way
//! too: the association network AutoCAD
//! keeps in the space's extension dictionary (and the drawing's), an
//! action with a dependency on the array's block reference, its array
//! body with an item for each reference, and the layout as value
//! parameters (Items, ItemSpacing, Rows, RowSpacing, …, as AutoCAD's
//! properties name them; how AutoCAD stores them isn't published).

use std::collections::HashMap;

use acadrust::objects::{
    AssocAction, AssocActionBody, AssocActionDependency, AssocArrayActionBody, AssocArrayItem, AssocDependency,
    AssocEvalValue, AssocEvalVariant, AssocGeomDependency, AssocNetwork, AssocPersistentSubentId, AssocValueParam,
    AssocValueParamVariable, AssociativeData, AssociativeObject, Dictionary, ObjectType,
};
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use oxidraft_geom::Point;
use oxidraft_io_dxf::Imported;
use oxidraft_model::assoc_array::{ArrayLayout, AssocArray};
use oxidraft_model::{Document, EntityId, Geometry};

const RECTANGULAR: &str = "AcDbAssocArrayRectangularParameters";
const POLAR: &str = "AcDbAssocArrayPolarParameters";
const PATH: &str = "AcDbAssocArrayPathParameters";

/// A row-major 4×4 matrix: turned `degrees` about the origin, then moved
/// by `by`.
fn matrix(degrees: f64, by: Point) -> [f64; 16] {
    let (s, c) = degrees.to_radians().sin_cos();
    [c, -s, 0.0, by.x, s, c, 0.0, by.y, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
}

fn point(v: Vector3) -> Point {
    Point::new(v.x, v.y)
}

fn associative(cad: &mut CadDocument, dxf: &str, class: &str, owner: Handle, data: AssociativeData) -> Handle {
    let h = cad.allocate_handle();
    let mut o = AssociativeObject::new(dxf, class);
    (o.handle, o.owner, o.data) = (h, owner, data);
    cad.objects.insert(h, ObjectType::Associative(o));
    h
}

/// A dictionary owned by `owner`, named `key` in dictionary `parent`.
fn dictionary_in(cad: &mut CadDocument, parent: Handle, key: &str) -> Handle {
    if let Some(ObjectType::Dictionary(d)) = cad.objects.get(&parent)
        && let Some(h) = d.get(key).filter(|h| matches!(cad.objects.get(h), Some(ObjectType::Dictionary(_))))
    {
        return h;
    }
    let h = cad.allocate_handle();
    let mut d = Dictionary::new();
    (d.handle, d.owner, d.reactors) = (h, parent, vec![parent]);
    cad.objects.insert(h, ObjectType::Dictionary(d));
    if let Some(ObjectType::Dictionary(p)) = cad.objects.get_mut(&parent) {
        p.add_entry(key, h);
    }
    h
}

/// The network named ACAD_ASSOCNETWORK in `dict` (made, empty, owned by
/// `owning` when there's none).
fn network_in(cad: &mut CadDocument, dict: Handle, owning: Handle) -> Handle {
    if let Some(ObjectType::Dictionary(d)) = cad.objects.get(&dict)
        && let Some(h) = d.get("ACAD_ASSOCNETWORK")
        && matches!(cad.objects.get(&h), Some(ObjectType::Associative(o)) if matches!(o.data, AssociativeData::Network(_)))
    {
        return h;
    }
    let action = AssocAction { class_version: 2, owning_network: owning, ..Default::default() };
    let data = AssociativeData::Network(AssocNetwork { action, network_action_index: 0, ..Default::default() });
    let h = associative(cad, "ACDBASSOCNETWORK", "AcDbAssocNetwork", dict, data);
    if let Some(ObjectType::Associative(o)) = cad.objects.get_mut(&h) {
        o.reactors = vec![dict];
    }
    if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&dict) {
        d.add_entry("ACAD_ASSOCNETWORK", h);
    }
    h
}

/// The association network of the space `record` (its extension
/// dictionary's), in the drawing's own (the root dictionary's).
fn space_network(cad: &mut CadDocument, record: Handle) -> Handle {
    let root = cad.header.named_objects_dict_handle;
    let top_dict = dictionary_in(cad, root, "ACAD_ASSOCNETWORK");
    let top = network_in(cad, top_dict, Handle::NULL);
    let xdict = cad.ensure_extension_dictionary(record);
    let dict = dictionary_in(cad, xdict, "ACAD_ASSOCNETWORK");
    let network = network_in(cad, dict, top);
    let mut index = None;
    if let Some(ObjectType::Associative(o)) = cad.objects.get_mut(&top)
        && let AssociativeData::Network(n) = &mut o.data
        && !n.actions.iter().any(|a| a.dependency == network)
    {
        n.actions.push(AssocActionDependency { is_owned: false, dependency: network });
        n.network_action_index = n.actions.len() as i32;
        index = Some(n.network_action_index);
    }
    // It's an action of the drawing's network, numbered in it.
    if let (Some(index), Some(ObjectType::Associative(o))) = (index, cad.objects.get_mut(&network))
        && let AssociativeData::Network(n) = &mut o.data
    {
        n.action.action_index = index;
    }
    network
}

/// A value parameter: a whole number or a real (a distance or an angle).
fn value(name: &str, v: AssocEvalValue, unit_type: i32) -> AssocValueParam {
    let code = if matches!(v, AssocEvalValue::Long(_)) { 90 } else { 40 };
    AssocValueParam {
        class_version: 0,
        name: name.to_string(),
        unit_type,
        variables: vec![AssocValueParamVariable { value: AssocEvalVariant { code, value: v }, handle: Handle::NULL }],
        controlled_object_dependency: Handle::NULL,
    }
}

/// NATIVESAVE: `doc`'s rectangular and polar arrays as AutoCAD's.
pub(crate) fn write(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    for e in doc.all_entities() {
        let (Some(a), Geometry::Insert { block, placement, .. }) = (e.array.as_deref(), &e.geometry) else { continue };
        let Some(&h) = handles.get(&e.id).and_then(|h| h.first()) else { continue };
        let insert = Handle::new(h);
        let Some(EntityType::Insert(reference)) = cad.get_entity(insert) else { continue };
        let space = reference.common.owner_handle;
        // The references in the array's block, in the layout's order.
        let Some(record) = cad.block_records.iter().find(|r| r.name.eq_ignore_ascii_case(&reference.block_name)) else {
            continue;
        };
        let refs: Vec<(Handle, Point, f64)> = record
            .entity_handles
            .iter()
            .filter_map(|h| match cad.get_entity(*h) {
                Some(EntityType::Insert(i)) => Some((*h, point(i.insert_point), i.rotation.to_degrees())),
                _ => None,
            })
            .collect();
        let Some(outer) = doc.block(block) else { continue };
        let Some(&(_, first, _)) = refs.first() else { continue };
        // A path array's path: the object it's laid along, in the world.
        let mut path_on = None;
        let (class, values, rect) = match &a.layout {
            ArrayLayout::Rect { rows, columns, row_spacing, column_spacing } => {
                let n = |v: usize| AssocEvalValue::Long(v as i32);
                let values = vec![
                    value("Items", n(*columns), 0),
                    value("ItemSpacing", AssocEvalValue::Real(*column_spacing), 1),
                    value("Rows", n(*rows), 0),
                    value("RowSpacing", AssocEvalValue::Real(*row_spacing), 1),
                    value("Levels", n(1), 0),
                    value("LevelSpacing", AssocEvalValue::Real(1.0), 1),
                    value("AxesAngle", AssocEvalValue::Real(90f64.to_radians()), 2),
                ];
                (RECTANGULAR, values, Some(*columns))
            }
            ArrayLayout::Polar { items, fill, rotate, .. } => {
                let values = vec![
                    value("Items", AssocEvalValue::Long(*items as i32), 0),
                    value("FillAngle", AssocEvalValue::Real(fill.to_radians()), 2),
                    value("Rows", AssocEvalValue::Long(1), 0),
                    value("Levels", AssocEvalValue::Long(1), 0),
                    value("RotateItems", AssocEvalValue::Long(i32::from(*rotate)), 0),
                ];
                (POLAR, values, None)
            }
            ArrayLayout::Path { path, items, align, spacing, rows } => {
                let world = placement.place(outer.base, path);
                let same = |g: &Geometry| {
                    let (a, b) = (g.segments(), world.segments());
                    a.len() == b.len()
                        && g.is_closed() == world.is_closed()
                        && a.iter().zip(&b).all(|(x, y)| x.start().approx_eq(y.start()) && x.end().approx_eq(y.end()))
                };
                path_on = doc
                    .all_entities()
                    .find(|p| p.id != e.id && p.space == e.space && p.array.is_none() && same(&p.geometry))
                    .and_then(|p| handles.get(&p.id).and_then(|h| h.first()))
                    .map(|h| Handle::new(*h));
                let length: f64 = path.segments().iter().map(|s| s.length()).sum();
                let gaps = if path.is_closed() { *items } else { items.saturating_sub(1) };
                let step = spacing.unwrap_or(length / gaps.max(1) as f64);
                let values = vec![
                    value("Items", AssocEvalValue::Long(*items as i32), 0),
                    value("ItemSpacing", AssocEvalValue::Real(step), 1),
                    value("Rows", AssocEvalValue::Long(rows.0.max(1) as i32), 0),
                    value("RowSpacing", AssocEvalValue::Real(rows.1), 1),
                    value("Levels", AssocEvalValue::Long(1), 0),
                    value("AlignItems", AssocEvalValue::Long(i32::from(*align)), 0),
                    value("Method", AssocEvalValue::Long(i32::from(spacing.is_some())), 0),
                ];
                (PATH, values, Some(*items))
            }
        };
        let turned = !matches!(a.layout, ArrayLayout::Rect { .. });
        // Each item: its place in the rows and columns, and where it is from
        // the first (moved, or for a polar array turned too).
        let items = refs
            .iter()
            .enumerate()
            .map(|(k, &(h, at, turn))| {
                let location = match rect {
                    Some(columns) => [(k % columns.max(1)) as i32, (k / columns.max(1)) as i32, 0],
                    None => [k as i32, 0, 0],
                };
                let offset = at - first;
                let mut item = AssocArrayItem { location, flags: 8, first_handle: Some(h), ..Default::default() };
                if !turned {
                    item.uses_default_transform = true;
                    item.x_direction = Vector3::new(offset.x, offset.y, 0.0);
                } else {
                    // Turned about the first item's base, then moved there
                    // (flag 4: a transform of its own).
                    item.flags |= 4;
                    item.transform = matrix(turn, at - first.rotated_about(Point::ORIGIN, turn));
                }
                item
            })
            .collect();
        // The first item in the world.
        let world = placement.position + (first - outer.base).rotated_about(Point::ORIGIN, placement.rotation);
        let network = space_network(cad, space);
        let action = cad.allocate_handle();
        let body = associative(
            cad,
            "ACDBASSOCARRAYACTIONBODY",
            "AcDbAssocArrayActionBody",
            action,
            AssociativeData::ArrayActionBody(AssocArrayActionBody {
                action_body: AssocActionBody { version: 2 },
                version: 1,
                parameter_block: class.to_string(),
                item_class: "AcDbAssocArrayItem".to_string(),
                items,
                transform: matrix(placement.rotation, world),
                ..Default::default()
            }),
        );
        let dependency = associative(
            cad,
            "ACDBASSOCDEPENDENCY",
            "AcDbAssocDependency",
            action,
            AssociativeData::Dependency(AssocDependency {
                class_version: 2,
                is_read_dependency: true,
                is_write_dependency: true,
                is_attached_to_object: true,
                is_delegating_to_owning_action: true,
                dependent_on: insert,
                dependency_body_id: 1,
                ..Default::default()
            }),
        );
        // A path array reads its path, as AutoCAD's do: a dependency on its
        // one edge.
        let mut dependencies = vec![AssocActionDependency { is_owned: true, dependency }];
        if let Some(path) = path_on {
            let geom = associative(
                cad,
                "ACDBASSOCGEOMDEPENDENCY",
                "AcDbAssocGeomDependency",
                action,
                AssociativeData::GeomDependency(AssocGeomDependency {
                    dependency: AssocDependency {
                        class_version: 2,
                        is_read_dependency: true,
                        is_attached_to_object: true,
                        is_delegating_to_owning_action: true,
                        order: i32::MAX,
                        dependent_on: path,
                        dependency_body_id: 2,
                        ..Default::default()
                    },
                    enabled: true,
                    persistent_subent: AssocPersistentSubentId {
                        class_name: "AcDbAssocSingleEdgePersSubentId".to_string(),
                        class_code: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            );
            if let Some(e) = cad.get_entity_mut(path) {
                e.common_mut().reactors.push(geom);
            }
            dependencies.push(AssocActionDependency { is_owned: true, dependency: geom });
        }
        let index = match cad.objects.get_mut(&network) {
            Some(ObjectType::Associative(o)) => match &mut o.data {
                AssociativeData::Network(n) => {
                    n.actions.push(AssocActionDependency { is_owned: true, dependency: action });
                    n.network_action_index = n.actions.len() as i32;
                    n.network_action_index
                }
                _ => 1,
            },
            _ => 1,
        };
        let mut o = AssociativeObject::new("ACDBASSOCACTION", "AcDbAssocAction");
        (o.handle, o.owner) = (action, network);
        o.data = AssociativeData::Action(AssocAction {
            class_version: 2,
            geometry_status: 0,
            owning_network: network,
            action_body: body,
            action_index: index,
            max_dependency_index: dependencies.len() as i32,
            dependencies,
            owned_parameters: Vec::new(),
            values,
        });
        cad.objects.insert(action, ObjectType::Associative(o));
        if let Some(e) = cad.get_entity_mut(insert) {
            e.common_mut().reactors.push(dependency);
        }
    }
}

/// The center a turn of `degrees` turns `from` to `to` about.
fn center_of_turn(from: Point, to: Point, degrees: f64) -> Option<Point> {
    let half = (degrees / 2.0).to_radians();
    if half.tan().abs() < 1e-12 {
        return None;
    }
    let d = to - from;
    let perp = Point::new(-d.y, d.x);
    Some(from + d * 0.5 + perp * (0.5 / half.tan()))
}

/// The circle's center through three points.
fn circumcenter(a: Point, b: Point, c: Point) -> Option<Point> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.x * a.x + a.y * a.y, b.x * b.x + b.y * b.y, c.x * c.x + c.y * c.y);
    Some(Point::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    ))
}

/// The object an action's dependency `h` is on.
fn dependent_on(cad: &CadDocument, h: Handle) -> Option<Handle> {
    match cad.objects.get(&h) {
        Some(ObjectType::Associative(o)) => match &o.data {
            AssociativeData::Dependency(d) => Some(d.dependent_on),
            AssociativeData::GeomDependency(g) => Some(g.dependency.dependent_on),
            _ => None,
        },
        _ => None,
    }
}

/// The angle (degrees) `a` is from `b`, from -180 to 180.
fn turn_between(a: f64, b: f64) -> f64 {
    (a - b + 540.0).rem_euclid(360.0) - 180.0
}

/// AutoCAD's arrays (those Oxidraft's own data doesn't already make one):
/// their layout from their items (and a path array's path, what its other
/// dependency is on), their block made again as Oxidraft's — when that
/// puts every item where AutoCAD has it; otherwise they stay references.
pub(crate) fn restore(cad: &CadDocument, imported: &mut Imported) {
    for o in cad.objects.values() {
        let ObjectType::Associative(o) = o else { continue };
        let AssociativeData::ArrayActionBody(body) = &o.data else { continue };
        let Some(ObjectType::Associative(action)) = cad.objects.get(&o.owner) else { continue };
        let AssociativeData::Action(action) = &action.data else { continue };
        let on: Vec<Handle> = action.dependencies.iter().filter_map(|d| dependent_on(cad, d.dependency)).collect();
        let Some((insert, reference)) = on.iter().find_map(|h| match cad.get_entity(*h) {
            Some(EntityType::Insert(r)) => Some((*h, r)),
            _ => None,
        }) else {
            continue;
        };
        let Some(&id) = imported.handles.get(&insert.value()) else { continue };
        let path_id = on.iter().filter(|h| **h != insert).find_map(|h| imported.handles.get(&h.value()).copied());
        let doc = &mut imported.document;
        let Some(e) = doc.entity(id).filter(|e| e.array.is_none()) else { continue };
        let Geometry::Insert { placement, .. } = e.geometry.clone() else { continue };
        // Its items in the layout's order: their places in the rows and
        // columns, where they are in the array's block, how far turned.
        let mut items: Vec<([i32; 3], Point, f64, String)> = body
            .items
            .iter()
            .filter_map(|i| match cad.get_entity(i.first_handle?) {
                Some(EntityType::Insert(r)) => {
                    Some((i.location, point(r.insert_point), r.rotation.to_degrees(), r.block_name.clone()))
                }
                _ => None,
            })
            .collect();
        items.sort_by_key(|i| (i.0[2], i.0[1], i.0[0]));
        let Some((_, first, first_turn, item_block)) = items.first().cloned() else { continue };
        let Some(item_base) = doc.block(&item_block).map(|b| b.base) else { continue };
        let Some(array_base) = doc.block(&reference.block_name).map(|b| b.base) else { continue };
        // Each item in the world: where, how far turned.
        let world: Vec<(Point, f64)> =
            items.iter().map(|i| (placement.apply(array_base, i.1), placement.rotation + i.2)).collect();
        let at = |l: [i32; 3]| items.iter().find(|i| i.0 == l).map(|i| i.1 - first);
        let columns = items.iter().map(|i| i.0[0]).max().unwrap_or(0) as usize + 1;
        let rows = items.iter().map(|i| i.0[1]).max().unwrap_or(0) as usize + 1;
        let class = body.parameter_block.as_str();
        // The layout, and how far the array's reference turns past its own.
        let (layout, turn) = if class.eq_ignore_ascii_case(RECTANGULAR) {
            let (across, up) = (at([1, 0, 0]), at([0, 1, 0]));
            let turn = match (across, up) {
                (Some(d), _) if d.length() > 0.0 => d.y.atan2(d.x).to_degrees(),
                (_, Some(d)) if d.length() > 0.0 => d.y.atan2(d.x).to_degrees() - 90.0,
                _ => 0.0,
            };
            let column_spacing = across.map_or(1.0, |d| d.rotated_about(Point::ORIGIN, -turn).x);
            let row_spacing = up.map_or(1.0, |d| d.rotated_about(Point::ORIGIN, -turn).y);
            (ArrayLayout::Rect { rows, columns, row_spacing, column_spacing }, turn)
        } else if class.eq_ignore_ascii_case(POLAR) && items.len() >= 2 {
            let (second, second_turn) = (items[1].1, items[1].2);
            let rotate = turn_between(second_turn, first_turn).abs() > 1e-9;
            let center = if rotate {
                center_of_turn(first, second, turn_between(second_turn, first_turn))
            } else {
                items.get(2).and_then(|third| circumcenter(first, second, third.1))
            };
            let Some(center) = center else { continue };
            let angle = |p: Point| (p - center).y.atan2((p - center).x).to_degrees();
            let step = turn_between(angle(second), angle(first));
            let n = items.len();
            let fill =
                if (step * n as f64).abs() >= 360.0 - 1e-6 { 360.0 * step.signum() } else { step * (n - 1) as f64 };
            // The center in the block Oxidraft makes, its first item at the item's base.
            (ArrayLayout::Polar { center: item_base + (center - first), items: n, fill, rotate }, 0.0)
        } else if class.eq_ignore_ascii_case(PATH) {
            let Some(path) = path_id.and_then(|p| doc.entity(p)).map(|e| e.geometry.clone()) else { continue };
            let length: f64 = path.segments().iter().map(|s| s.length()).sum();
            let gaps = if path.is_closed() { columns } else { columns.saturating_sub(1) };
            let from = |k: usize| world.get(k).map(|w| oxidraft_model::assoc_array::length_at(&path, w.0));
            let step = match (from(0), from(1)) {
                (Some(a), Some(b)) if columns > 1 => b - a,
                _ => length,
            };
            let spacing = ((step * gaps as f64 - length).abs() > 1e-6 * length.max(1.0)).then_some(step);
            let align = items.iter().filter(|i| i.0[1] == 0).any(|i| turn_between(i.2, first_turn).abs() > 1e-9);
            // Rows to the path's left at its start.
            let row = match (world.first(), items.iter().position(|i| i.0 == [0, 1, 0]).and_then(|k| world.get(k))) {
                (Some(a), Some(b)) => {
                    let start = oxidraft_model::assoc_array::at_length(&path.segments(), 0.0).map_or(0.0, |s| s.1);
                    (b.0 - a.0).rotated_about(Point::ORIGIN, -start).y
                }
                _ => 0.0,
            };
            // The path in the block Oxidraft makes, turned with its first item.
            let new = oxidraft_model::Placement { position: world[0].0, rotation: world[0].1, ..placement };
            let back = oxidraft_model::Placement {
                position: item_base,
                scale: 1.0 / new.scale,
                rotation: -new.rotation,
                mirror: new.mirror,
            };
            let local = back.place(new.position, &path);
            let layout = ArrayLayout::Path { path: Box::new(local), items: columns, align, spacing, rows: (rows, row) };
            (layout, first_turn)
        } else {
            continue;
        };
        // Where Oxidraft's block puts each item: AutoCAD's places, or not at all.
        let new = oxidraft_model::Placement { position: world[0].0, rotation: placement.rotation + turn, ..placement };
        let made = layout.items(item_base);
        let close = |a: Point, b: Point| a.distance(b) <= 1e-6 * (1.0 + a.length().max(b.length()));
        let same = made.len() == world.len()
            && made.iter().zip(&world).all(|((q, t), (w, wt))| {
                close(new.apply(item_base, *q), *w) && turn_between(new.rotation + t, *wt).abs() < 1e-6
            });
        if !same {
            continue;
        }
        let array = AssocArray { layout: layout.clone(), item: item_block, replaced: Vec::new() };
        doc.update(id, |e| {
            e.array = Some(Box::new(array));
            e.geometry =
                match std::mem::replace(&mut e.geometry, Geometry::Line { start: Point::ORIGIN, end: Point::ORIGIN }) {
                    Geometry::Insert { block, attributes, clip, .. } => {
                        Geometry::Insert { block, placement: new, attributes, clip }
                    }
                    g => g,
                };
        });
        doc.set_array_layout(id, layout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// `doc` with an array of `layout` made of one circle at (10, 10).
    fn arrayed(layout: ArrayLayout) -> (Document, EntityId) {
        let mut doc = Document::new();
        let c = doc.add(Geometry::Circle { center: Point::new(10.0, 10.0), radius: 2.0 });
        doc.add(Geometry::Line { start: Point::new(100.0, 0.0), end: Point::new(110.0, 0.0) });
        if let ArrayLayout::Path { path, .. } = &layout {
            doc.add(path.as_ref().clone());
        }
        let id = doc.make_array(&[c], layout).unwrap();
        (doc, id)
    }

    /// A path array along an L of lines from the circle's center (the
    /// path, in the drawing too), aligned or not, divided or measured, in
    /// two rows.
    fn path_layout(align: bool, spacing: Option<f64>) -> ArrayLayout {
        let vertices =
            [(10.0, 10.0), (50.0, 10.0), (50.0, 40.0)].map(|(x, y)| oxidraft_model::Vertex::at(Point::new(x, y)));
        let path = Geometry::Polyline { vertices: vertices.to_vec(), closed: false };
        ArrayLayout::Path { path: Box::new(path), items: 5, align, spacing, rows: (2, 6.0) }
    }

    /// Where each of the array's items is, in the world.
    fn items(doc: &Document, id: EntityId) -> Vec<Point> {
        let n = doc.array(id).unwrap().layout.count();
        (0..n).map(|i| doc.array_item(id, i).unwrap().0.position).collect()
    }

    /// The drawing's DWG as AutoCAD would save it, without Oxidraft's data:
    /// as DWG and as DXF.
    fn as_autocads(dwg: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
        let mut cad = acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().unwrap();
        for e in cad.entities_mut() {
            let data = &mut e.common_mut().extended_data;
            let kept: Vec<_> =
                data.records().iter().filter(|r| !r.application_name.starts_with("OXIDRAFT")).cloned().collect();
            *data = Default::default();
            for r in kept {
                data.add_record(r);
            }
        }
        let root = cad.header.named_objects_dict_handle;
        if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&root) {
            d.entries.retain(|(n, _)| !n.starts_with("OXIDRAFT"));
        }
        let ours: Vec<String> =
            cad.app_ids.iter().map(|a| a.name.clone()).filter(|n| n.starts_with("OXIDRAFT")).collect();
        for name in ours {
            cad.app_ids.remove(&name);
        }
        (acadrust::DwgWriter::write_to_vec(&cad).unwrap(), acadrust::DxfWriter::new(&cad).write_to_vec().unwrap())
    }

    #[test]
    fn arrays_in_autocads_form_and_back() {
        for layout in [
            ArrayLayout::Rect { rows: 2, columns: 3, row_spacing: 15.0, column_spacing: 20.0 },
            ArrayLayout::Polar { center: Point::new(30.0, 10.0), items: 6, fill: 360.0, rotate: true },
            ArrayLayout::Polar { center: Point::new(30.0, 10.0), items: 4, fill: 90.0, rotate: false },
            path_layout(true, None),
            path_layout(false, Some(12.0)),
        ] {
            let (doc, id) = arrayed(layout.clone());
            let before = items(&doc, id);
            crate::set_native_objects(true);
            let dwg = crate::write(&doc, crate::Version::R2018).unwrap();
            crate::set_native_objects(false);
            // As AutoCAD reads it: a network in model space's extension
            // dictionary, an action, its dependency on the reference, its
            // array body with an item for each reference.
            let cad = acadrust::DwgReader::from_stream(Cursor::new(dwg.clone())).read().unwrap();
            let bodies: Vec<&AssocArrayActionBody> = cad
                .objects
                .values()
                .filter_map(|o| match o {
                    ObjectType::Associative(o) => match &o.data {
                        AssociativeData::ArrayActionBody(b) => Some(b),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            assert_eq!(bodies.len(), 1, "{layout:?}");
            assert_eq!(bodies[0].items.len(), layout.count());
            assert!(cad.objects.values().any(|o| matches!(o, ObjectType::Associative(o) if matches!(&o.data, AssociativeData::Action(a) if !a.values.is_empty()))));
            // Without Oxidraft's data, from AutoCAD's form alone: the same array.
            let (dwg, dxf) = as_autocads(dwg);
            if let Ok(dir) = std::env::var("ARRAY_DUMP") {
                std::fs::write(format!("{dir}/array.dxf"), &dxf).unwrap();
            }
            for back in [crate::read(&dwg).unwrap().imported.document, crate::read_dxf(&dxf).unwrap().document] {
                let id = back.all_entities().find(|e| e.array.is_some()).expect("an array again").id;
                let got = back.array(id).unwrap().layout.clone();
                match (&layout, &got) {
                    (ArrayLayout::Rect { .. }, ArrayLayout::Rect { .. }) => assert_eq!(got, layout),
                    (
                        ArrayLayout::Path { items: n, align, spacing, rows, .. },
                        ArrayLayout::Path { items: m, align: a, spacing: s, rows: r, .. },
                    ) => {
                        assert!(n == m && align == a && rows.0 == r.0 && (rows.1 - r.1).abs() < 1e-6, "{got:?}");
                        assert_eq!(spacing.is_some(), s.is_some(), "{got:?}");
                    }
                    (
                        ArrayLayout::Polar { items: n, fill, rotate, .. },
                        ArrayLayout::Polar { items: m, fill: f, rotate: r, .. },
                    ) => assert!(n == m && rotate == r && (fill - f).abs() < 1e-6, "{got:?}"),
                    _ => panic!("{got:?}"),
                }
                let after = items(&back, id);
                assert_eq!(before.len(), after.len());
                for (a, b) in before.iter().zip(&after) {
                    assert!(a.distance(*b) < 1e-6, "{layout:?}: {before:?} {after:?}");
                }
            }
        }
    }
}

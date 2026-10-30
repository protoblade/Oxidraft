//! AutoCAD's dynamic blocks coming in dynamic: a definition's parameters
//! and actions (its evaluation graph, as acadrust reads it) made
//! Oxidraft's, and each reference to one of its representations given the
//! values AutoCAD saved for it, so its grips and the Properties palette
//! work on it. A reference keeps showing AutoCAD's representation until
//! its values change. What Oxidraft's dynamic blocks have no room for (a
//! stretch frame's shape beyond its extents, an XY array's rows,
//! constraints) is left out.

use std::collections::HashMap;

use acadrust::objects::{
    BlockAction, BlockEvalValue, BlockEvaluationGraph, BlockOnePointParameter, BlockTwoPointParameter,
    DynamicBlockData, ObjectType,
};
use acadrust::types::Vector3;
use acadrust::{CadDocument, EntityType, Handle};
use oxidraft_geom::{Aabb, Point};
use oxidraft_io_dxf::Imported;
use oxidraft_model::Geometry;
use oxidraft_model::dynamic::{DynamicBlock, DynamicValues, Param, VisibilityState};

fn point(v: &Vector3) -> Point {
    Point::new(v.x, v.y)
}

fn data(cad: &CadDocument, h: Handle) -> Option<&DynamicBlockData> {
    match cad.objects.get(&h)? {
        ObjectType::DynamicBlock(o) => Some(&o.data),
        _ => None,
    }
}

/// A definition's evaluation graph.
fn graph(cad: &CadDocument, block: Handle) -> Option<&BlockEvaluationGraph> {
    let dict = cad.extension_dictionary_handle(block)?;
    let ObjectType::Dictionary(d) = cad.objects.get(&dict)? else { return None };
    let (_, h) = d.entries.iter().find(|(n, _)| n == "ACAD_ENHANCEDBLOCK")?;
    match data(cad, *h)? {
        DynamicBlockData::EvaluationGraph(g) => Some(g),
        _ => None,
    }
}

/// Where a value AutoCAD gives a property goes: Oxidraft's property, and
/// how it's turned (radians to degrees, a distance's sign, a lookup's
/// label to its row).
#[derive(Debug, Clone)]
enum Target {
    Value { name: String, factor: f64 },
    Row { name: String, labels: Vec<String> },
    Visibility,
}

/// A converted definition and where its properties' values go, by
/// AutoCAD's property names.
struct Converted {
    def: DynamicBlock,
    props: HashMap<String, Target>,
}

fn two_points(p: &BlockTwoPointParameter) -> (Point, Point) {
    (point(&p.definition_base_point), point(&p.definition_end_point))
}

fn one_point(p: &BlockOnePointParameter) -> Point {
    point(&p.definition_point)
}

fn convert(cad: &CadDocument, g: &BlockEvaluationGraph, index: &HashMap<u64, usize>) -> Converted {
    let at = |id: i32| g.nodes.iter().find(|n| n.id == id).map(|n| n.expression);
    // The actions each parameter's node leads to.
    let actions_of = |node: i32| -> Vec<&DynamicBlockData> {
        g.edges
            .iter()
            .filter(|e| e.source_node == node)
            .filter_map(|e| at(e.destination_node).and_then(|h| data(cad, h)))
            .collect()
    };
    let objects = |a: &BlockAction| -> Vec<usize> {
        let mut out: Vec<usize> = a.dependencies.iter().filter_map(|h| index.get(&h.value()).copied()).collect();
        out.sort_unstable();
        out.dedup();
        out
    };
    // A stretch's frame: its corners' extents.
    let frame = |points: &[acadrust::types::Vector2]| {
        let mut it = points.iter().map(|p| Point::new(p.x, p.y));
        let first = it.next()?;
        Some(it.fold(Aabb::from_points(first, first), |b, p| b.union(Aabb::from_points(p, p))))
    };
    let mut def = DynamicBlock::default();
    let mut props: HashMap<String, Target> = HashMap::new();
    let value = |name: &str| Target::Value { name: name.to_string(), factor: 1.0 };
    // Lookup tables, by the parameters they serve.
    let mut lookups: Vec<(&acadrust::objects::BlockLookupAction, Vec<(i32, String)>)> = Vec::new();
    let mut names_by_element: HashMap<i32, Vec<(String, String)>> = HashMap::new();
    for node in &g.nodes {
        let Some(d) = data(cad, node.expression) else { continue };
        let actions = actions_of(node.id);
        match d {
            DynamicBlockData::PointParameter(p) => {
                let name = p.position_name.clone();
                let base = one_point(&p.parameter);
                let mut moved = Vec::new();
                for a in &actions {
                    if let DynamicBlockData::MoveAction(m) = a {
                        moved.extend(objects(&m.action));
                    }
                }
                for axis in ["X", "Y"] {
                    props.insert(format!("{name} {axis}"), value(&format!("{name} {axis}")));
                }
                names_by_element
                    .entry(p.parameter.parameter.element.eval.node_id)
                    .or_default()
                    .extend([("UpdatedX".into(), format!("{name} X")), ("UpdatedY".into(), format!("{name} Y"))]);
                def.params.push(Param::Point { name, base, objects: moved });
            }
            DynamicBlockData::LinearParameter(p) => {
                let name = if p.distance_name.is_empty() {
                    p.parameter.parameter.element.name.clone()
                } else {
                    p.distance_name.clone()
                };
                let (base, end) = two_points(&p.parameter);
                props.insert(name.clone(), value(&name));
                names_by_element
                    .entry(p.parameter.parameter.element.eval.node_id)
                    .or_default()
                    .push(("UpdatedDistance".into(), name.clone()));
                let mut stretch = None;
                for a in &actions {
                    match a {
                        DynamicBlockData::StretchAction(s) => stretch = stretch.or(frame(&s.points)),
                        DynamicBlockData::ArrayAction(arr) => def.params.push(Param::Array {
                            name: arr.action.element.name.clone(),
                            follows: name.clone(),
                            objects: objects(&arr.action),
                            spacing: arr.column_offset,
                        }),
                        DynamicBlockData::ScaleAction(s) => def.params.push(Param::Scale {
                            name: s.action.action.element.name.clone(),
                            follows: name.clone(),
                            base: point(&s.action.base_point),
                            objects: objects(&s.action.action),
                        }),
                        _ => {}
                    }
                }
                def.params.push(Param::Linear { name, base, end, stretch });
            }
            DynamicBlockData::PolarParameter(p) => {
                let name = p.parameter.parameter.element.name.clone();
                let (base, end) = two_points(&p.parameter);
                props.insert(p.distance_name.clone(), value(&format!("{name} Distance")));
                props.insert(
                    p.angle_name.clone(),
                    Target::Value { name: format!("{name} Angle"), factor: 180.0 / std::f64::consts::PI },
                );
                let mut stretch = None;
                for a in &actions {
                    match a {
                        DynamicBlockData::PolarStretchAction(s) => stretch = stretch.or(frame(&s.points)),
                        DynamicBlockData::StretchAction(s) => stretch = stretch.or(frame(&s.points)),
                        _ => {}
                    }
                }
                def.params.push(Param::Polar { name, base, end, stretch });
            }
            DynamicBlockData::XYParameter(p) => {
                let name = p.parameter.parameter.element.name.clone();
                let (base, end) = two_points(&p.parameter);
                // AutoCAD's distances are unsigned; Oxidraft's run from the
                // base toward the end.
                let sign = |d: f64| if d < 0.0 { -1.0 } else { 1.0 };
                props.insert(
                    p.x_label.clone(),
                    Target::Value { name: format!("{name} Horizontal"), factor: sign(end.x - base.x) },
                );
                props.insert(
                    p.y_label.clone(),
                    Target::Value { name: format!("{name} Vertical"), factor: sign(end.y - base.y) },
                );
                let mut stretch = None;
                for a in &actions {
                    match a {
                        DynamicBlockData::StretchAction(s) => stretch = stretch.or(frame(&s.points)),
                        DynamicBlockData::ArrayAction(arr) => def.params.push(Param::Array {
                            name: arr.action.element.name.clone(),
                            follows: name.clone(),
                            objects: objects(&arr.action),
                            spacing: arr.column_offset,
                        }),
                        _ => {}
                    }
                }
                def.params.push(Param::Xy { name, base, end, stretch });
            }
            DynamicBlockData::RotationParameter(p) => {
                let name = p.angle_name.clone();
                let (base, end) = two_points(&p.parameter);
                props.insert(name.clone(), Target::Value { name: name.clone(), factor: 180.0 / std::f64::consts::PI });
                def.params.push(Param::Rotation { name, base, radius: base.distance(end) });
            }
            DynamicBlockData::FlipParameter(p) => {
                let name = p.flip_label.clone();
                let (base, end) = two_points(&p.parameter);
                props.insert(name.clone(), value(&name));
                def.params.push(Param::Flip { name, base, end });
            }
            DynamicBlockData::AlignmentParameter(p) => {
                let (base, end) = two_points(&p.parameter);
                let name = p.parameter.parameter.element.name.clone();
                def.params.push(Param::Alignment { name, base, end });
            }
            DynamicBlockData::BasePointParameter(p) => {
                let name = p.parameter.parameter.element.name.clone();
                def.params.push(Param::BasePoint { name, base: one_point(&p.parameter) });
            }
            DynamicBlockData::LookupParameter(p) => {
                // Its table: the lookup action its node leads to (or that
                // leads to it).
                let eval = p.parameter.parameter.element.eval.node_id;
                let table = g
                    .edges
                    .iter()
                    .filter(|e| e.destination_node == node.id || e.source_node == node.id)
                    .flat_map(|e| [e.source_node, e.destination_node])
                    .filter_map(|n| at(n).and_then(|h| data(cad, h)))
                    .find_map(|d| match d {
                        DynamicBlockData::LookupAction(a) => Some(a),
                        _ => None,
                    });
                if let Some(t) = table {
                    match lookups.iter_mut().find(|(a, _)| std::ptr::eq(*a, t)) {
                        Some((_, served)) => served.push((eval, p.lookup_name.clone())),
                        None => lookups.push((t, vec![(eval, p.lookup_name.clone())])),
                    }
                }
                def.params.push(Param::Lookup {
                    name: p.lookup_name.clone(),
                    base: one_point(&p.parameter),
                    rows: Vec::new(),
                });
            }
            DynamicBlockData::VisibilityParameter(v) => {
                // Each state hides what the parameter governs that it
                // doesn't show.
                for s in &v.states {
                    let hidden = v
                        .all_blocks
                        .iter()
                        .filter(|h| !s.visible_blocks.contains(h))
                        .filter_map(|h| index.get(&h.value()).copied())
                        .collect();
                    def.states.push(VisibilityState { name: s.name.clone(), hidden });
                }
                props.insert(v.name.clone(), Target::Visibility);
            }
            _ => {}
        }
    }
    // Lookup tables' rows: each row's label for each lookup parameter,
    // and the values it gives the other parameters' properties.
    for (table, served) in &lookups {
        let columns = table.columns.len().max(1);
        let rows: Vec<&[String]> = table.expressions.chunks(columns).collect();
        for (eval, lookup) in served {
            let Some(label_column) = table.columns.iter().position(|c| c.lookup_property && c.node_id == *eval) else {
                continue;
            };
            let mut out = Vec::new();
            for row in &rows {
                let label = row.get(label_column).map_or(String::new(), |s| s.trim().to_string());
                let mut values = Vec::new();
                for (c, column) in table.columns.iter().enumerate().filter(|(_, c)| !c.lookup_property) {
                    let target = names_by_element
                        .get(&column.node_id)
                        .and_then(|n| n.iter().find(|(k, _)| *k == column.connection_name))
                        .map(|(_, v)| v.clone());
                    if let (Some(target), Some(v)) = (target, row.get(c).and_then(|s| s.trim().parse::<f64>().ok())) {
                        values.push((target, v));
                    }
                }
                out.push((label, values));
            }
            let labels: Vec<String> = out.iter().map(|(l, _)| l.clone()).collect();
            props.insert(lookup.clone(), Target::Row { name: lookup.clone(), labels });
            for p in &mut def.params {
                if let Param::Lookup { name, rows, .. } = p
                    && name == lookup
                {
                    *rows = out.clone();
                }
            }
        }
    }
    Converted { def, props }
}

fn number(v: &BlockEvalValue) -> Option<f64> {
    match v {
        BlockEvalValue::Real(r) => Some(*r),
        BlockEvalValue::Long(n) => Some(f64::from(*n)),
        BlockEvalValue::Short(n) => Some(f64::from(*n)),
        _ => None,
    }
}

/// Each of block `r`'s objects' place in the block as imported, by its
/// handle in `cad`: by the handles the DXF reader saw when they're
/// `cad`'s, else in order (acadrust's DXF numbers block objects afresh),
/// hatches coming last, when the counts agree.
fn block_index(cad: &CadDocument, r: &acadrust::tables::BlockRecord, imported: &Imported) -> HashMap<u64, usize> {
    let by_handle: HashMap<u64, usize> = r
        .entity_handles
        .iter()
        .filter_map(|h| {
            let (b, i) = imported.block_handles.get(&h.value())?;
            b.eq_ignore_ascii_case(&r.name).then_some((h.value(), *i))
        })
        .collect();
    if !by_handle.is_empty() {
        return by_handle;
    }
    let Some(block) = imported.document.block(&r.name) else { return HashMap::new() };
    let hatches_here = block.entities.iter().filter(|e| matches!(e.geometry, Geometry::Hatch { .. })).count();
    let (hatches, others): (Vec<Handle>, Vec<Handle>) = r
        .entity_handles
        .iter()
        .filter(|h| {
            cad.get_entity(**h).is_some_and(|e| !matches!(e, EntityType::Seqend(_) | EntityType::AttributeEntity(_)))
        })
        .partition(|h| matches!(cad.get_entity(**h), Some(EntityType::Hatch(_))));
    if hatches.len() != hatches_here || others.len() + hatches.len() != block.entities.len() {
        return HashMap::new();
    }
    others.iter().chain(&hatches).enumerate().map(|(i, h)| (h.value(), i)).collect()
}

/// Makes `imported`'s blocks that are AutoCAD dynamic blocks in `cad`
/// Oxidraft's, and gives their references their values.
pub(crate) fn import(cad: &CadDocument, imported: &mut Imported) {
    let mut converted: HashMap<Handle, (String, HashMap<String, Target>)> = HashMap::new();
    for r in cad.block_records.iter() {
        // Anonymous representations carry a copy of the graph; the
        // definition is what's converted.
        if r.name.starts_with('*')
            || imported.document.dynamic_block(&r.name).is_some()
            || imported.document.block(&r.name).is_none()
        {
            continue;
        }
        let Some(g) = graph(cad, r.handle) else { continue };
        let index = block_index(cad, r, imported);
        let Converted { def, props } = convert(cad, g, &index);
        if def.params.is_empty() && def.states.is_empty() {
            continue;
        }
        imported.document.set_dynamic_block(&r.name, Some(def));
        converted.insert(r.handle, (r.name.clone(), props));
    }
    if converted.is_empty() {
        return;
    }
    // References to a representation: the values AutoCAD saved.
    for e in cad.entities() {
        let EntityType::Insert(insert) = e else { continue };
        let Some(record) = cad.block_records.get(&insert.block_name) else { continue };
        let Some(definition) = cad.dynamic_block_definition(record.handle) else { continue };
        let Some((name, props)) = converted.get(&definition) else { continue };
        if record.handle == definition {
            continue;
        }
        let Some(&id) = imported.handles.get(&e.common().handle.value()) else { continue };
        let mut values = DynamicValues { block: name.clone(), ..Default::default() };
        for p in cad.dynamic_block_properties(definition, Some(e.common().handle)).unwrap_or_default() {
            match (props.get(&p.name), &p.value) {
                (Some(Target::Value { name, factor }), v) => {
                    if let Some(n) = number(v) {
                        let n = if *factor < 0.0 || name.ends_with(" Vertical") || name.ends_with(" Horizontal") {
                            n.abs() * factor
                        } else {
                            n * factor
                        };
                        values.values.push((name.clone(), n));
                    }
                }
                (Some(Target::Row { name, labels }), BlockEvalValue::Text(label)) => {
                    if let Some(i) = labels.iter().position(|l| l.eq_ignore_ascii_case(label.trim())) {
                        values.values.push((name.clone(), i as f64));
                    }
                }
                (Some(Target::Visibility), BlockEvalValue::Text(state)) => values.visibility = Some(state.clone()),
                _ => {}
            }
        }
        imported.document.update(id, |e| e.dynamic = Some(Box::new(values)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_model::{BlockEntity, Document};

    fn read(bytes: &[u8]) -> Document {
        crate::read(bytes).unwrap().imported.document
    }

    fn extents(es: &[BlockEntity]) -> Option<Aabb> {
        es.iter().map(|e| e.geometry.bounds()).reduce(|a, b| a.union(b))
    }

    /// Each reference's values, applied to the definition, give what
    /// AutoCAD drew for it (but for the representations in `unlike`).
    fn drawn_like_autocad(doc: &Document, unlike: &[&str]) -> usize {
        let mut checked = 0;
        for e in doc.entities() {
            let (Geometry::Insert { block, .. }, Some(v)) = (&e.geometry, e.dynamic.as_deref()) else { continue };
            if unlike.contains(&block.as_str()) {
                continue;
            }
            let def = doc.dynamic_block(&v.block).unwrap();
            let ours = extents(&def.apply(&doc.block(&v.block).unwrap().entities, v)).unwrap();
            let theirs = extents(&doc.block(block).unwrap().entities).unwrap();
            let near = |a: Point, b: Point| a.distance(b) < 1e-6;
            assert!(near(ours.min, theirs.min) && near(ours.max, theirs.max), "{block}: {ours:?} {theirs:?}");
            checked += 1;
        }
        checked
    }

    #[test]
    fn autocads_dynamic_blocks_come_in_dynamic() {
        // A point parameter whose move action takes the two lines.
        let doc = read(include_bytes!("../tests/fixtures/dynamic-blocks/point.dwg"));
        let def = doc.dynamic_block("block_translation_parameter").unwrap();
        assert!(
            matches!(&def.params[..], [Param::Point { name, objects, .. }] if name == "dynamic_pos" && objects == &[1, 2]),
            "{def:?}"
        );
        assert_eq!(drawn_like_autocad(&doc, &[]), 2);
        // Its references have grips, and new values give a new
        // representation.
        let id = doc.entities().find(|e| e.dynamic.is_some()).unwrap().id;
        assert!(!doc.dynamic_grips(id).is_empty());
        let mut doc = doc;
        let mut values = doc.dynamic_values(id).unwrap();
        values.values.retain(|(n, _)| n != "dynamic_pos Y");
        values.values.push(("dynamic_pos Y".into(), 2.0));
        assert!(doc.set_dynamic_values(id, values));
        // A linear parameter's stretch, a point's move and lookup tables.
        let doc = read(include_bytes!("../tests/fixtures/dynamic-blocks/lookup.dwg"));
        let def = doc.dynamic_block("My_Look_Block").unwrap();
        let rows = def.params.iter().find_map(|p| match p {
            Param::Lookup { name, rows, .. } if name == "Lookup Label Name" => Some(rows.clone()),
            _ => None,
        });
        let rows = rows.unwrap();
        assert_eq!(rows.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["Size 5", "Size 6", "Size 8"]);
        assert!(rows[2].1.contains(&("my_distance_label".to_string(), 8.0)));
        assert_eq!(drawn_like_autocad(&doc, &[]), 4);
        // Polar stretches; an XY parameter's array (whose rows Oxidraft's
        // arrays don't make, nor its polar stretch's turning).
        let doc = read(include_bytes!("../tests/fixtures/dynamic-blocks/polar.dwg"));
        assert_eq!(doc.dynamic_block("BLOCK_POLAR_PARAMETER").unwrap().params.len(), 2);
        assert!(drawn_like_autocad(&doc, &["*U5"]) >= 1);
        let doc = read(include_bytes!("../tests/fixtures/dynamic-blocks/xy.dwg"));
        assert!(drawn_like_autocad(&doc, &["*U4"]) >= 1);
        // A DXF of them comes in the same way.
        let cad = acadrust::DwgReader::from_stream(std::io::Cursor::new(
            include_bytes!("../tests/fixtures/dynamic-blocks/point.dwg").to_vec(),
        ))
        .read()
        .unwrap();
        let dxf = acadrust::DxfWriter::new(&cad).write_to_vec().unwrap();
        let doc = crate::read_dxf(&dxf).unwrap().document;
        assert!(doc.dynamic_block("block_translation_parameter").is_some());
        assert_eq!(drawn_like_autocad(&doc, &[]), 2);
    }
}

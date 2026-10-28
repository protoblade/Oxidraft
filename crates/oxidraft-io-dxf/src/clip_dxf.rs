//! XCLIP boundaries in DXF. A clipped INSERT's extension dictionary holds
//! ACAD_FILTER, a dictionary whose SPATIAL entry is a SPATIAL_FILTER: the
//! boundary (block coordinates after its inverse insert matrix) and whether
//! clipping is on. AutoCAD keeps an inverted clip's hole in the filter's own
//! extension dictionary, in the ACAD_XREC_ROUNDTRIP xrecord.
//!
//! The dxf crate misreads the filter's matrices, so filters are read from
//! the file's text. They're written with identity matrices, as ezdxf does,
//! and without the clipping planes' distances, which the crate writes even
//! for planes that are off (AutoCAD refuses those).
//! An inverted clip is written as the outline of everything around its
//! hole (so other programs show the same), with the hole itself in an
//! OXIDRAFT_INVERTED_CLIP xrecord: AutoCAD refuses roundtrip data it didn't
//! write itself.

use std::collections::HashMap;

use dxf::objects::{Dictionary, Object, ObjectType, SpatialFilter, XRecordObject};
use dxf::{CodePair, CodePairValue, Drawing, ExtensionGroup, ExtensionGroupItem, Handle, TransformationMatrix};
use oxidraft_geom::{Aabb, Point};
use oxidraft_model::XClip;

const XDICTIONARY: &str = "ACAD_XDICTIONARY";
const OWN_HOLE: &str = "OXIDRAFT_INVERTED_CLIP";
const ROUNDTRIP: &str = "ACAD_XREC_ROUNDTRIP";
const ROUNDTRIP_HOLE: &str = "ACAD_INVERTEDCLIP_ROUNDTRIP";

/// The identity matrix as AutoCAD reads a filter's matrices: three rows
/// of four, x first. The dxf crate writes its fields in another order
/// (m11, m21, m31, m12, …), so they're set to come out that way.
fn identity() -> TransformationMatrix {
    TransformationMatrix { m11: 1.0, m32: 1.0, m14: 1.0, ..Default::default() }
}

fn add(drawing: &mut Drawing, specific: ObjectType, owner: Handle) -> Handle {
    let mut object = Object::new(specific);
    object.common.__owner_handle = owner;
    drawing.add_object(object).common.handle
}

fn dictionary(drawing: &mut Drawing, owner: Handle) -> Handle {
    add(drawing, ObjectType::Dictionary(Dictionary::default()), owner)
}

fn set_entry(drawing: &mut Drawing, dict: Handle, name: &str, value: Handle) {
    if let Some(Object { specific: ObjectType::Dictionary(d), .. }) =
        drawing.objects_mut().find(|o| o.common.handle == dict)
    {
        d.value_handles.insert(name.to_string(), value);
    }
}

/// An extension group pointing at the extension dictionary `dict`.
fn xdictionary(dict: Handle) -> ExtensionGroup {
    ExtensionGroup {
        application_name: XDICTIONARY.into(),
        items: vec![ExtensionGroupItem::CodePair(CodePair::new_string(360, &dict.as_string()))],
    }
}

/// The handle an entity's or object's extension dictionary group points at.
pub(crate) fn xdictionary_of(groups: &[ExtensionGroup]) -> Option<u64> {
    let group = groups.iter().find(|g| g.application_name == XDICTIONARY)?;
    group.items.iter().find_map(|item| match item {
        ExtensionGroupItem::CodePair(CodePair { code: 360, value: CodePairValue::Str(s), .. }) => {
            u64::from_str_radix(s.trim(), 16).ok()
        }
        _ => None,
    })
}

/// Everything in `extents` around `hole`, as one ring: the box, then in
/// along a slit to the hole, round it the other way, and back out.
fn around(extents: Aabb, hole: &[Point]) -> Vec<Point> {
    let area: f64 = (0..hole.len()).map(|i| hole[i].cross(hole[(i + 1) % hole.len()])).sum();
    let mut hole = hole.to_vec();
    if area > 0.0 {
        hole.reverse();
    }
    let k = (0..hole.len()).min_by(|&a, &b| hole[a].x.total_cmp(&hole[b].x)).unwrap_or(0);
    hole.rotate_left(k);
    let gate = Point::new(extents.min.x, hole[0].y);
    let [bl, br, tr, tl] = extents.corners();
    let mut ring = vec![bl, br, tr, tl, gate];
    ring.extend(&hole);
    ring.extend([hole[0], gate]);
    ring
}

/// Adds `clip`'s objects for the INSERT with handle `insert` and returns
/// the extension group that points the INSERT at them. `extents` are the
/// block's, for an inverted clip's outline.
pub(crate) fn add_filter(drawing: &mut Drawing, insert: Handle, clip: &XClip, extents: Option<Aabb>) -> ExtensionGroup {
    // The first object is the root dictionary, wherever it's added.
    if drawing.objects().next().is_none() {
        dictionary(drawing, Handle::empty());
    }
    let xdict = dictionary(drawing, insert);
    let filters = dictionary(drawing, xdict);
    set_entry(drawing, xdict, "ACAD_FILTER", filters);
    let boundary = match extents.filter(|_| clip.inverted) {
        Some(e) => {
            let b = clip.boundary.iter().fold(e, |b, p| b.union(Aabb::from_points(*p, *p)));
            let margin = b.width().max(b.height()) * 0.1;
            let grown = Aabb::from_points(b.min - Point::new(margin, margin), b.max + Point::new(margin, margin));
            around(grown, &clip.boundary)
        }
        None => clip.boundary.clone(),
    };
    let filter = SpatialFilter {
        clip_boundary_definition_points: boundary.iter().map(|p| dxf::Point::new(p.x, p.y, 0.0)).collect(),
        is_clip_boundary_enabled: clip.on,
        is_front_clipping_plane: false,
        is_back_clipping_plane: false,
        inverse_transformation_matrix: identity(),
        transformation_matrix: identity(),
        ..Default::default()
    };
    let spatial = add(drawing, ObjectType::SpatialFilter(filter), filters);
    set_entry(drawing, filters, "SPATIAL", spatial);
    if clip.inverted {
        let own = dictionary(drawing, spatial);
        let data_pairs = clip
            .boundary
            .iter()
            .flat_map(|p| [CodePair::new_f64(10, p.x), CodePair::new_f64(20, p.y), CodePair::new_f64(30, 0.0)])
            .collect();
        let hole = add(drawing, ObjectType::XRecordObject(XRecordObject { data_pairs, ..Default::default() }), own);
        set_entry(drawing, own, OWN_HOLE, hole);
        if let Some(o) = drawing.objects_mut().find(|o| o.common.handle == spatial) {
            o.common.extension_data_groups.push(xdictionary(own));
        }
    }
    xdictionary(xdict)
}

/// One object of the OBJECTS section, as its code pairs.
struct RawObject<'a> {
    kind: &'a str,
    pairs: Vec<(i32, &'a str)>,
}

impl RawObject<'_> {
    fn value(&self, code: i32) -> Option<&str> {
        self.pairs.iter().find(|p| p.0 == code).map(|p| p.1)
    }

    /// A dictionary's entries.
    fn entries(&self) -> HashMap<&str, u64> {
        let mut out = HashMap::new();
        let mut name = None;
        for &(code, value) in &self.pairs {
            match code {
                3 => name = Some(value),
                350 | 360 => {
                    if let (Some(n), Ok(h)) = (name.take(), u64::from_str_radix(value, 16)) {
                        out.insert(n, h);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Its own extension dictionary.
    fn xdictionary(&self) -> Option<u64> {
        let start = self.pairs.iter().position(|p| p.0 == 102 && p.1 == "{ACAD_XDICTIONARY")?;
        let pair = self.pairs[start..].iter().find(|p| p.0 == 360)?;
        u64::from_str_radix(pair.1, 16).ok()
    }

    /// Where its subclass `name` starts.
    fn marker(&self, name: &str) -> usize {
        self.pairs.iter().position(|p| p.0 == 100 && p.1 == name).unwrap_or(0)
    }

    /// The points (codes 10 and 20) after `from`, up to the next code 102.
    fn points(&self, from: usize) -> Vec<Point> {
        let mut out = Vec::new();
        for &(code, value) in self.pairs.iter().skip(from).take_while(|p| p.0 != 102) {
            let Ok(v) = value.parse::<f64>() else { continue };
            match code {
                10 => out.push(Point::new(v, 0.0)),
                20 => {
                    if let Some(p) = out.last_mut() {
                        p.y = v;
                    }
                }
                _ => {}
            }
        }
        out
    }
}

fn objects(text: &str) -> HashMap<u64, RawObject<'_>> {
    let mut out = HashMap::new();
    let lines: Vec<&str> = text.lines().collect();
    let pairs: Vec<(i32, &str)> =
        lines.chunks(2).filter_map(|p| Some((p[0].trim().parse().ok()?, p.get(1)?.trim()))).collect();
    let Some(start) = pairs.windows(2).position(|w| w[0] == (0, "SECTION") && w[1] == (2, "OBJECTS")) else {
        return out;
    };
    let mut i = start + 2;
    while i < pairs.len() {
        let (code, kind) = pairs[i];
        i += 1;
        if code != 0 || kind == "ENDSEC" {
            if kind == "ENDSEC" {
                break;
            }
            continue;
        }
        let end = pairs[i..].iter().position(|p| p.0 == 0).map_or(pairs.len(), |n| i + n);
        if matches!(kind, "DICTIONARY" | "SPATIAL_FILTER" | "XRECORD") {
            let object = RawObject { kind, pairs: pairs[i..end].to_vec() };
            if let Some(h) = object.value(5).and_then(|h| u64::from_str_radix(h, 16).ok()) {
                out.insert(h, object);
            }
        }
        i = end;
    }
    out
}

/// The 3×4 matrix in a filter's last 24 code-40 values that takes its
/// points into block coordinates (the inverse insert matrix).
fn inverse_insert(filter: &RawObject) -> impl Fn(Point) -> Point {
    let values: Vec<f64> = filter.pairs.iter().filter(|p| p.0 == 40).filter_map(|p| p.1.parse().ok()).collect();
    let m: Vec<f64> = if values.len() >= 24 {
        values[values.len() - 24..values.len() - 12].to_vec()
    } else {
        vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    };
    move |p: Point| Point::new(m[0] * p.x + m[1] * p.y + m[3], m[4] * p.x + m[5] * p.y + m[7])
}

fn bbox_area(points: &[Point]) -> f64 {
    let b = points.iter().fold(None::<Aabb>, |b, p| {
        let q = Aabb::from_points(*p, *p);
        Some(b.map_or(q, |b| b.union(q)))
    });
    b.map_or(0.0, |b| b.width() * b.height())
}

/// The clips in a DXF file, by the extension dictionary of the INSERT
/// they belong to.
pub(crate) fn read_clips(dxf: &[u8]) -> HashMap<u64, XClip> {
    if dxf.starts_with(b"AutoCAD Binary DXF") {
        return HashMap::new();
    }
    let text = String::from_utf8_lossy(dxf);
    let objects = objects(&text);
    let entry =
        |dict: u64, name: &str| objects.get(&dict).filter(|o| o.kind == "DICTIONARY")?.entries().get(name).copied();
    let mut out = HashMap::new();
    for (&xdict, o) in &objects {
        if o.kind != "DICTIONARY" {
            continue;
        }
        let Some(filter) = entry(xdict, "ACAD_FILTER")
            .and_then(|f| entry(f, "SPATIAL"))
            .and_then(|s| objects.get(&s))
            .filter(|s| s.kind == "SPATIAL_FILTER")
        else {
            continue;
        };
        let to_block = inverse_insert(filter);
        let mut boundary: Vec<Point> =
            filter.points(filter.marker("AcDbSpatialFilter")).into_iter().map(&to_block).collect();
        if let [a, b] = boundary[..] {
            boundary = Aabb::from_points(a, b).corners().to_vec();
        }
        if boundary.len() > 3 && boundary[0].approx_eq(boundary[boundary.len() - 1]) {
            boundary.pop();
        }
        // An inverted clip's hole: ours, or AutoCAD's (whichever of its two
        // outlines is smaller, as ezdxf takes it).
        let mut inverted = false;
        let own = filter.xdictionary();
        let record = |name: &str| own.and_then(|d| entry(d, name)).and_then(|r| objects.get(&r));
        if let Some(hole) = record(OWN_HOLE) {
            boundary = hole.points(hole.marker("AcDbXrecord")).into_iter().map(&to_block).collect();
            inverted = true;
        } else if let Some(rt) = record(ROUNDTRIP)
            && let Some(at) = rt.pairs.iter().position(|p| p.0 == 102 && p.1 == ROUNDTRIP_HOLE)
        {
            let hole: Vec<Point> = rt.points(at + 1).into_iter().map(&to_block).collect();
            if hole.len() >= 3 && bbox_area(&hole) < bbox_area(&boundary) {
                boundary = hole;
            }
            inverted = true;
        }
        if boundary.len() >= 3 {
            let on = filter.value(71).is_none_or(|v| v.trim() != "0");
            out.insert(xdict, XClip { boundary, inverted, on });
        }
    }
    out
}

/// Drops the front and back clipping distances (groups 40 and 41) the
/// dxf crate writes after a SPATIAL_FILTER's flags 72 and 73 even when
/// they're 0.
pub(crate) fn drop_unused_planes(dxf: &[u8]) -> Vec<u8> {
    let lines: Vec<&[u8]> = dxf.split_inclusive(|&b| b == b'\n').collect();
    let trim = |l: &[u8]| String::from_utf8_lossy(l).trim().to_string();
    let mut out = Vec::with_capacity(dxf.len());
    let (mut in_filter, mut previous) = (false, (String::new(), String::new()));
    for pair in lines.chunks(2) {
        let (code, value) = (trim(pair[0]), pair.get(1).map(|v| trim(v)).unwrap_or_default());
        if code == "0" {
            in_filter = value == "SPATIAL_FILTER";
        }
        let unused = in_filter
            && ((code == "40" && previous == ("72".into(), "0".into()))
                || (code == "41" && previous == ("73".into(), "0".into())));
        if !unused {
            pair.iter().for_each(|l| out.extend_from_slice(l));
        }
        previous = (code, value);
    }
    out
}

//! Layouts in DXF, as far as the dxf crate doesn't read them: each LAYOUT
//! object's page setup (its AcDbPlotSettings), and VIEWPORT entities.
//! Paper space objects of the active layout are in ENTITIES (group 67 set);
//! those of the others are in their *Paper_SpaceN blocks.

use std::collections::HashMap;

use oxidraft_geom::Point;
use oxidraft_model::Paper;

/// A VIEWPORT entity as read.
pub(crate) struct RawViewport {
    /// The *Paper_SpaceN block it's in, or `None` for ENTITIES.
    pub block: Option<String>,
    /// Its owner (a block record's handle), and its own handle.
    pub owner: u64,
    pub handle: u64,
    pub layer: String,
    /// 1 is the layout's own, which shows the sheet.
    pub id: i64,
    pub center: Point,
    pub width: f64,
    pub height: f64,
    pub view_center: Point,
    pub view_height: f64,
    pub twist: f64,
    pub frozen: Vec<u64>,
    pub on: bool,
    pub locked: bool,
    /// A non-rectangular viewport's boundary: the object's handle, and
    /// its outline once found.
    pub clip_handle: u64,
    pub clip: Vec<Point>,
}

/// An object as read: its type, handle, and group codes and values.
pub(crate) type RawObject = (String, u64, Vec<(i32, String)>);

#[derive(Default)]
pub(crate) struct RawLayouts {
    /// Page setups by upper-case layout name.
    pub papers: HashMap<String, Paper>,
    /// Each layout's block record (by upper-case layout name), and the
    /// block records' upper-case names by handle.
    pub layout_records: HashMap<String, u64>,
    pub records: HashMap<u64, String>,
    pub viewports: Vec<RawViewport>,
    /// The objects that are viewports' boundaries, which the viewports
    /// draw themselves.
    pub boundaries: Vec<u64>,
    /// Each WIPEOUT's handle and boundary, in file order (the dxf crate
    /// reads their boundaries' y wrongly).
    pub wipeouts: Vec<(u64, Vec<Point>)>,
    /// GROUP objects: (handle, description, member handles); and every
    /// dictionary's (name, handle) entries, by its handle, to name them.
    pub groups: Vec<(u64, String, Vec<u64>)>,
    pub dictionaries: HashMap<u64, Vec<(String, u64)>>,
    /// XRECORD objects' group codes and values, by handle.
    pub xrecords: HashMap<u64, Vec<(i32, String)>>,
    /// AutoCAD's own objects for settings Oxidraft has (MLEADERSTYLE,
    /// TABLESTYLE, RASTERVARIABLES, WIPEOUTVARIABLES), and its variables
    /// kept in objects (DICTIONARYVAR).
    pub setting_objects: Vec<RawObject>,
    /// Header variables the dxf crate doesn't read ($CMLEADERSTYLE,
    /// $CTABLESTYLE), and the applications registered (APPID).
    pub header: HashMap<String, String>,
    pub apps: Vec<String>,
    /// Each named view's name and flags (1: a paper space view).
    pub views: Vec<(String, i32)>,
}

fn paper(plot: &[(i32, &str)]) -> Paper {
    let num = |code: i32| plot.iter().find(|p| p.0 == code).and_then(|p| p.1.parse::<f64>().ok());
    Paper {
        size_name: plot.iter().find(|p| p.0 == 4).map_or(String::new(), |p| p.1.to_string()),
        width: num(44).unwrap_or(0.0),
        height: num(45).unwrap_or(0.0),
        margins: [40, 41, 42, 43].map(|c| num(c).unwrap_or(0.0)),
        rotation: num(73).map_or(0, |r| (r as i64).rem_euclid(4) as u8),
        millimeters: num(72).is_none_or(|u| u == 1.0),
        raw: plot.iter().map(|(c, v)| (*c, v.to_string())).collect(),
    }
}

fn viewport(body: &[(i32, &str)], block: Option<String>) -> RawViewport {
    let mut v = RawViewport {
        block,
        owner: 0,
        handle: 0,
        layer: "0".into(),
        id: 0,
        center: Point::ORIGIN,
        width: 0.0,
        height: 0.0,
        view_center: Point::ORIGIN,
        view_height: 0.0,
        twist: 0.0,
        frozen: Vec::new(),
        on: true,
        locked: false,
        clip_handle: 0,
        clip: Vec::new(),
    };
    let hex = |s: &str| u64::from_str_radix(s, 16).unwrap_or(0);
    let num = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    // Groups ({ACAD_REACTORS …}) hold handles that aren't the owner.
    let mut in_group = false;
    for &(code, value) in body {
        match code {
            102 => in_group = value.starts_with('{'),
            _ if in_group => {}
            5 => v.handle = hex(value),
            330 if v.owner == 0 => v.owner = hex(value),
            8 => v.layer = value.to_string(),
            10 => v.center.x = num(value),
            20 => v.center.y = num(value),
            40 => v.width = num(value),
            41 => v.height = num(value),
            12 => v.view_center.x = num(value),
            22 => v.view_center.y = num(value),
            45 => v.view_height = num(value),
            51 => v.twist = num(value),
            68 => v.on &= num(value) != 0.0,
            69 => v.id = value.parse().unwrap_or(0),
            90 => {
                let flags = value.parse::<i64>().unwrap_or(0);
                v.locked = flags & 0x4000 != 0;
                v.on &= flags & 0x20000 == 0;
            }
            331 => v.frozen.push(hex(value)),
            340 => v.clip_handle = hex(value),
            _ => {}
        }
    }
    v
}

pub(crate) fn read(dxf: &[u8]) -> RawLayouts {
    let mut out = RawLayouts::default();
    if dxf.starts_with(b"AutoCAD Binary DXF") {
        return out;
    }
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.lines().collect();
    let pairs: Vec<(i32, &str)> =
        lines.chunks(2).filter_map(|p| Some((p[0].trim().parse().ok()?, p.get(1)?.trim()))).collect();
    let (mut section, mut block): (&str, Option<String>) = ("", None);
    // Where each polyline and circle is, by handle: a viewport's boundary
    // may be one of them.
    let mut shapes: HashMap<u64, (&str, usize, usize)> = HashMap::new();
    let mut i = 0;
    while i < pairs.len() {
        let (code, value) = pairs[i];
        i += 1;
        if code != 0 {
            continue;
        }
        let end = pairs[i..].iter().position(|p| p.0 == 0).map_or(pairs.len(), |n| i + n);
        let body = &pairs[i..end];
        match value {
            "SECTION" => {
                section = body.iter().find(|p| p.0 == 2).map_or("", |p| p.1);
                if section == "HEADER" {
                    for w in body.windows(2) {
                        if let [(9, name @ ("$CMLEADERSTYLE" | "$CTABLESTYLE")), (_, value)] = w {
                            out.header.insert(name.to_string(), value.to_string());
                        }
                    }
                }
            }
            "VIEW" if section == "TABLES" => {
                if let Some(name) = body.iter().find(|p| p.0 == 2) {
                    let flags = body.iter().find(|p| p.0 == 70).and_then(|p| p.1.parse().ok()).unwrap_or(0);
                    out.views.push((name.1.to_string(), flags));
                }
            }
            "APPID" if section == "TABLES" => {
                out.apps.extend(body.iter().filter(|p| p.0 == 2).map(|p| p.1.to_string()));
            }
            kind @ ("MLEADERSTYLE" | "TABLESTYLE" | "RASTERVARIABLES" | "WIPEOUTVARIABLES" | "DICTIONARYVAR")
                if section == "OBJECTS" =>
            {
                let handle = body.iter().find(|p| p.0 == 5).and_then(|p| u64::from_str_radix(p.1, 16).ok());
                let pairs = body.iter().map(|(c, v)| (*c, v.to_string())).collect();
                out.setting_objects.push((kind.to_string(), handle.unwrap_or(0), pairs));
            }
            "BLOCK" => block = body.iter().find(|p| p.0 == 2).map(|p| p.1.to_string()),
            "ENDBLK" => block = None,
            kind @ ("LWPOLYLINE" | "CIRCLE") if section == "ENTITIES" || section == "BLOCKS" => {
                if let Some(h) = body.iter().find(|p| p.0 == 5).and_then(|p| u64::from_str_radix(p.1, 16).ok()) {
                    shapes.insert(h, (kind, i, end));
                }
            }
            "WIPEOUT" if section == "ENTITIES" || section == "BLOCKS" => {
                let handle = body.iter().find(|p| p.0 == 5).and_then(|p| u64::from_str_radix(p.1, 16).ok());
                out.wipeouts.push((handle.unwrap_or(0), wipeout(body)));
            }
            "VIEWPORT" if section == "ENTITIES" => out.viewports.push(viewport(body, None)),
            "VIEWPORT" if section == "BLOCKS" => out.viewports.push(viewport(body, block.clone())),
            "BLOCK_RECORD" if section == "TABLES" => {
                let handle = body.iter().find(|p| p.0 == 5).and_then(|p| u64::from_str_radix(p.1, 16).ok());
                if let (Some(h), Some(name)) = (handle, body.iter().find(|p| p.0 == 2)) {
                    out.records.insert(h, name.1.to_ascii_uppercase());
                }
            }
            "GROUP" if section == "OBJECTS" => {
                let hex = |s: &str| u64::from_str_radix(s, 16).unwrap_or(0);
                let handle = body.iter().find(|p| p.0 == 5).map_or(0, |p| hex(p.1));
                let description = body.iter().find(|p| p.0 == 300).map_or(String::new(), |p| p.1.to_string());
                let members = body.iter().filter(|p| p.0 == 340).map(|p| hex(p.1)).collect();
                out.groups.push((handle, description, members));
            }
            "DICTIONARY" if section == "OBJECTS" => {
                let hex = |s: &str| u64::from_str_radix(s, 16).unwrap_or(0);
                let handle = body.iter().find(|p| p.0 == 5).map_or(0, |p| hex(p.1));
                let mut entries = Vec::new();
                let mut name: Option<&str> = None;
                for &(code, value) in body {
                    match code {
                        3 => name = Some(value),
                        350 | 360 => {
                            if let Some(n) = name.take() {
                                entries.push((n.to_string(), hex(value)));
                            }
                        }
                        _ => {}
                    }
                }
                out.dictionaries.insert(handle, entries);
            }
            "XRECORD" if section == "OBJECTS" => {
                let handle = body.iter().find(|p| p.0 == 5).and_then(|p| u64::from_str_radix(p.1, 16).ok());
                if let Some(h) = handle {
                    out.xrecords.insert(h, body.iter().map(|(c, v)| (*c, v.to_string())).collect());
                }
            }
            "LAYOUT" if section == "OBJECTS" => {
                let marker = |name: &str| body.iter().position(|p| p.0 == 100 && p.1 == name);
                if let Some(layout) = marker("AcDbLayout")
                    && let Some(name) = body[layout..].iter().find(|p| p.0 == 1)
                {
                    let key = name.1.to_ascii_uppercase();
                    if let Some(plot) = marker("AcDbPlotSettings").filter(|p| *p < layout) {
                        out.papers.insert(key.clone(), paper(&body[plot + 1..layout]));
                    }
                    // Its paper space block record.
                    let record = body[layout..].iter().find(|p| p.0 == 330);
                    if let Some(h) = record.and_then(|p| u64::from_str_radix(p.1, 16).ok()) {
                        out.layout_records.insert(key, h);
                    }
                }
            }
            _ => {}
        }
        i = end;
    }
    for v in out.viewports.iter_mut().filter(|v| v.clip_handle != 0) {
        let Some(&(kind, from, to)) = shapes.get(&v.clip_handle) else { continue };
        v.clip = outline(kind, &pairs[from..to]);
        if v.clip.len() >= 3 {
            out.boundaries.push(v.clip_handle);
        }
    }
    out
}

/// A WIPEOUT's boundary: its vertices are in the unit image's pixels,
/// from its center with y down, placed by its corner and vectors.
fn wipeout(body: &[(i32, &str)]) -> Vec<Point> {
    let num = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    let get = |code: i32| body.iter().find(|p| p.0 == code).map_or(0.0, |p| num(p.1));
    let (o, u, v) = (Point::new(get(10), get(20)), Point::new(get(11), get(21)), Point::new(get(12), get(22)));
    let sy = get(23).max(1.0);
    let xs = body.iter().filter(|p| p.0 == 14).map(|p| num(p.1));
    let ys = body.iter().filter(|p| p.0 == 24).map(|p| num(p.1));
    let mut points: Vec<Point> = xs.zip(ys).map(|(x, y)| o + u * (x + 0.5) + v * (sy - (y + 0.5))).collect();
    if points.len() == 2 {
        let (a, b) = (points[0], points[1]);
        points = vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)];
    }
    if points.len() > 3 && points[0].distance(points[points.len() - 1]) < 1e-9 {
        points.pop();
    }
    points
}

/// A boundary's outline: a polyline's vertices, or a circle as 64 sides.
fn outline(kind: &str, body: &[(i32, &str)]) -> Vec<Point> {
    let num = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    match kind {
        "CIRCLE" => {
            let get = |code: i32| body.iter().find(|p| p.0 == code).map_or(0.0, |p| num(p.1));
            let (c, r) = (Point::new(get(10), get(20)), get(40));
            (0..64).map(|k| c.polar(r, 360.0 * k as f64 / 64.0)).collect()
        }
        _ => {
            let xs = body.iter().filter(|p| p.0 == 10).map(|p| num(p.1));
            let ys = body.iter().filter(|p| p.0 == 20).map(|p| num(p.1));
            xs.zip(ys).map(|(x, y)| Point::new(x, y)).collect()
        }
    }
}

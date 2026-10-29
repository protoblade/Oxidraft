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
}

#[derive(Default)]
pub(crate) struct RawLayouts {
    /// Page setups by upper-case layout name.
    pub papers: HashMap<String, Paper>,
    /// Each layout's block record (by upper-case layout name), and the
    /// block records' upper-case names by handle.
    pub layout_records: HashMap<String, u64>,
    pub records: HashMap<u64, String>,
    pub viewports: Vec<RawViewport>,
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
            "SECTION" => section = body.iter().find(|p| p.0 == 2).map_or("", |p| p.1),
            "BLOCK" => block = body.iter().find(|p| p.0 == 2).map(|p| p.1.to_string()),
            "ENDBLK" => block = None,
            "VIEWPORT" if section == "ENTITIES" => out.viewports.push(viewport(body, None)),
            "VIEWPORT" if section == "BLOCKS" => out.viewports.push(viewport(body, block.clone())),
            "BLOCK_RECORD" if section == "TABLES" => {
                let handle = body.iter().find(|p| p.0 == 5).and_then(|p| u64::from_str_radix(p.1, 16).ok());
                if let (Some(h), Some(name)) = (handle, body.iter().find(|p| p.0 == 2)) {
                    out.records.insert(h, name.1.to_ascii_uppercase());
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
    out
}

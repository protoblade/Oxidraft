//! What Oxidraft adds to a linear dimension, in its XDATA: a style
//! override as AutoCAD keeps DIMOVERRIDE's (ACAD DSTYLE), a jog as AutoCAD
//! keeps DIMJOGLINE's (ACAD_DSTYLE_DIMJAG_POSITION: 387, 3, 389 and the
//! jog's point, as Autodesk's developer guide gives it), and its breaks,
//! jog and inspection data (OXIDRAFT_DIM). AutoCAD's inspection data
//! (ACAD_DSTYLE_DIMINSPECT) is read too, and wins over Oxidraft's.

use dxf::{XData, XDataItem};
use oxidraft_geom::Point;
use oxidraft_model::Geometry;
use oxidraft_model::annotation::{DimExtra, DimStyle, Inspection, InspectionShape};

pub(crate) const APP: &str = "OXIDRAFT_DIM";

/// AutoCAD's application for a linear dimension's jog.
pub(crate) const JAG_APP: &str = "ACAD_DSTYLE_DIMJAG_POSITION";

/// AutoCAD's application for DIMINSPECT's data: (1070 code, value) pairs,
/// as its other dimension overrides — 393 shown (1) or taken off (0), 394
/// its shape and parts (1 round, 2 angular, 4 none; 16 with the label, 32
/// with the rate), 395 the label, 396 the rate.
pub(crate) const INSPECT_APP: &str = "ACAD_DSTYLE_DIMINSPECT";

/// The dimension variables an override sets, by their DXF group codes.
const DIMTXT: i16 = 140;
const DIMASZ: i16 = 41;
const DIMEXO: i16 = 42;
const DIMEXE: i16 = 44;
const DIMGAP: i16 = 147;
const DIMDEC: i16 = 271;

fn point(p: Point) -> XDataItem {
    XDataItem::WorldSpacePosition(dxf::Point::new(p.x, p.y, 0.0))
}

/// `g`'s extras as XDATA, if it's a linear dimension that has any.
pub(crate) fn xdata(g: &Geometry) -> Vec<XData> {
    let Geometry::Dimension { extra, .. } = g else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(s) = extra.style {
        let mut vars = Vec::new();
        for (code, v) in [
            (DIMTXT, s.text_height),
            (DIMASZ, s.arrow_size),
            (DIMEXO, s.ext_offset),
            (DIMEXE, s.ext_beyond),
            (DIMGAP, s.gap),
        ] {
            vars.extend([XDataItem::Integer(code), XDataItem::Real(v)]);
        }
        vars.extend([XDataItem::Integer(DIMDEC), XDataItem::Integer(s.decimals as i16)]);
        let items = vec![XDataItem::Str("DSTYLE".into()), XDataItem::ControlGroup(vars)];
        out.push(XData { application_name: "ACAD".into(), items });
    }
    if let Some(j) = extra.jog {
        let items = vec![
            XDataItem::Integer(387),
            XDataItem::Integer(3),
            XDataItem::Integer(389),
            XDataItem::ThreeReals(j.x, j.y, 0.0),
        ];
        out.push(XData { application_name: JAG_APP.into(), items });
    }
    let mut items = Vec::new();
    for (a, b) in &extra.breaks {
        items.extend([XDataItem::Str("BREAK".into()), point(*a), point(*b)]);
    }
    if let Some(j) = extra.jog {
        items.extend([XDataItem::Str("JOG".into()), point(j)]);
    }
    if let Some(i) = &extra.inspection {
        let shape = match i.shape {
            InspectionShape::Round => 0,
            InspectionShape::Angular => 1,
            InspectionShape::None => 2,
        };
        items.extend([
            XDataItem::Str("INSPECT".into()),
            XDataItem::Integer(shape),
            XDataItem::Str(i.label.clone()),
            XDataItem::Str(i.rate.clone()),
        ]);
    }
    if !items.is_empty() {
        out.push(XData { application_name: APP.into(), items });
    }
    out
}

/// A dimension's extras from its XDATA; an override starts from `base`,
/// the drawing's style.
pub(crate) fn read(x_data: &[XData], base: DimStyle, extra: &mut DimExtra) {
    let mut acad_inspection = None;
    for x in x_data {
        match x.application_name.as_str() {
            "ACAD" => {
                let Some(vars) = x.items.iter().find_map(|i| match i {
                    XDataItem::ControlGroup(v) => Some(v),
                    _ => None,
                }) else {
                    continue;
                };
                let mut style = base;
                let mut any = false;
                for pair in vars.chunks(2) {
                    let [XDataItem::Integer(code), value] = pair else { continue };
                    let v = match value {
                        XDataItem::Real(v) | XDataItem::Distance(v) | XDataItem::ScaleFactor(v) => *v,
                        XDataItem::Integer(v) => f64::from(*v),
                        _ => continue,
                    };
                    match *code {
                        DIMTXT => style.text_height = v,
                        DIMASZ => style.arrow_size = v,
                        DIMEXO => style.ext_offset = v,
                        DIMEXE => style.ext_beyond = v,
                        DIMGAP => style.gap = v,
                        DIMDEC => style.decimals = v.max(0.0) as usize,
                        _ => continue,
                    }
                    any = true;
                }
                if any {
                    extra.style = Some(style);
                }
            }
            // AutoCAD's jog: its point.
            JAG_APP => {
                if let Some(p) = x.items.iter().find_map(|i| match i {
                    XDataItem::ThreeReals(x, y, _) => Some(Point::new(*x, *y)),
                    XDataItem::WorldSpacePosition(p) => Some(Point::new(p.x, p.y)),
                    _ => None,
                }) {
                    extra.jog = Some(p);
                }
            }
            APP => {
                let pos = |i: Option<&XDataItem>| match i {
                    Some(XDataItem::WorldSpacePosition(p)) => Some(Point::new(p.x, p.y)),
                    _ => None,
                };
                let text = |i: Option<&XDataItem>| match i {
                    Some(XDataItem::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                let mut items = x.items.iter();
                while let Some(item) = items.next() {
                    let XDataItem::Str(tag) = item else { continue };
                    match tag.as_str() {
                        "BREAK" => {
                            if let (Some(a), Some(b)) = (pos(items.next()), pos(items.next())) {
                                extra.breaks.push((a, b));
                            }
                        }
                        "JOG" => extra.jog = pos(items.next()),
                        "INSPECT" => {
                            let shape = match items.next() {
                                Some(XDataItem::Integer(1)) => InspectionShape::Angular,
                                Some(XDataItem::Integer(2)) => InspectionShape::None,
                                _ => InspectionShape::Round,
                            };
                            let (label, rate) = (text(items.next()), text(items.next()));
                            extra.inspection = Some(Inspection { shape, label, rate });
                        }
                        _ => {}
                    }
                }
            }
            INSPECT_APP => acad_inspection = Some(inspection(&x.items)),
            _ => {}
        }
    }
    if let Some(i) = acad_inspection {
        extra.inspection = i;
    }
}

/// The inspection data AutoCAD's record shows, if it's shown.
fn inspection(items: &[XDataItem]) -> Option<Inspection> {
    let value = |code: i16| {
        items.chunks(2).find_map(|pair| match pair {
            [XDataItem::Integer(c), v] if *c == code => Some(v),
            _ => None,
        })
    };
    let int = |code| match value(code) {
        Some(XDataItem::Integer(n)) => Some(*n),
        _ => None,
    };
    let text = |code| match value(code) {
        Some(XDataItem::Str(s)) => s.clone(),
        _ => String::new(),
    };
    if int(393) != Some(1) {
        return None;
    }
    let flags = int(394).unwrap_or(1);
    let shape = if flags & 2 != 0 {
        InspectionShape::Angular
    } else if flags & 4 != 0 {
        InspectionShape::None
    } else {
        InspectionShape::Round
    };
    let label = if flags & 16 != 0 { text(395) } else { String::new() };
    let rate = if flags & 32 != 0 { text(396) } else { String::new() };
    Some(Inspection { shape, label, rate })
}

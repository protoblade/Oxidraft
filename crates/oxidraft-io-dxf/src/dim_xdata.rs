//! What Oxidraft adds to a linear dimension, in its XDATA: a style
//! override as AutoCAD keeps DIMOVERRIDE's (ACAD DSTYLE), and its breaks,
//! jog and inspection data (OXIDRAFT_DIM).

use dxf::{XData, XDataItem};
use oxidraft_geom::Point;
use oxidraft_model::Geometry;
use oxidraft_model::annotation::{DimExtra, DimStyle, Inspection, InspectionShape};

pub(crate) const APP: &str = "OXIDRAFT_DIM";

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
            _ => {}
        }
    }
}

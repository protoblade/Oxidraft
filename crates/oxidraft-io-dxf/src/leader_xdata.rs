//! A multileader is written as a LEADER (and one for each added
//! arrowhead) with its text as MTEXT or its blocks as INSERTs, which other
//! programs show as they are. XDATA on the LEADER keeps the multileader
//! whole: its text, scale, added arrowheads and blocks; the other parts
//! are marked as parts, and left out when it's read back.

use dxf::{XData, XDataItem};
use oxidraft_geom::Point;
use oxidraft_model::Geometry;
use oxidraft_model::annotation::{LeaderBlock, LeaderBlocks};

pub const APP: &str = "OXIDRAFT_MLEADER";
/// Marks what's drawn for a multileader besides its LEADER.
pub const PART_APP: &str = "OXIDRAFT_PART";

/// The multileader as XDATA: its text, scale, blocks to a row (0: no
/// blocks), the added arrowheads (X, Y) and the blocks (name, value).
pub fn xdata(g: &Geometry) -> Option<XData> {
    let Geometry::Leader { content, scale, more, blocks, .. } = g else { return None };
    let per_row = blocks.as_ref().map_or(0, |b| b.per_row.max(1));
    let mut items = vec![
        XDataItem::Str(content.clone()),
        XDataItem::Real(*scale),
        XDataItem::Long(i32::try_from(per_row).unwrap_or(i32::MAX)),
    ];
    for p in more {
        items.extend([XDataItem::Real(p.x), XDataItem::Real(p.y)]);
    }
    for b in blocks.iter().flat_map(|b| &b.items) {
        items.extend([XDataItem::Str(b.name.clone()), XDataItem::Str(b.value.clone())]);
    }
    Some(XData { application_name: APP.into(), items })
}

pub fn part() -> XData {
    XData { application_name: PART_APP.into(), items: vec![XDataItem::Integer(1)] }
}

pub fn is_part(x_data: &[XData]) -> bool {
    x_data.iter().any(|x| x.application_name == PART_APP)
}

/// Fills a LEADER read back in from its XDATA.
pub fn read(x_data: &[XData], g: &mut Geometry) {
    let Some(x) = x_data.iter().find(|x| x.application_name == APP) else { return };
    let Geometry::Leader { content, scale, more, blocks, .. } = g else { return };
    let strings: Vec<&str> =
        x.items.iter().filter_map(|i| if let XDataItem::Str(s) = i { Some(s.as_str()) } else { None }).collect();
    let reals: Vec<f64> = x
        .items
        .iter()
        .filter_map(|i| if let XDataItem::Real(v) = i { Some(*v) } else { None })
        .filter(|v| v.is_finite())
        .collect();
    let per_row = x.items.iter().find_map(|i| if let XDataItem::Long(n) = i { Some(*n) } else { None });
    let (Some((text, names)), Some((s, points))) = (strings.split_first(), reals.split_first()) else { return };
    *content = text.to_string();
    if *s > 0.0 {
        *scale = *s;
    }
    *more = points.as_chunks::<2>().0.iter().map(|[x, y]| Point::new(*x, *y)).collect();
    let items: Vec<LeaderBlock> = names
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[name, value]| LeaderBlock { name: name.to_string(), value: value.to_string() })
        .collect();
    *blocks = match per_row.and_then(|n| usize::try_from(n).ok()) {
        Some(per_row) if per_row > 0 && !items.is_empty() => Some(Box::new(LeaderBlocks { items, per_row })),
        _ => None,
    };
}

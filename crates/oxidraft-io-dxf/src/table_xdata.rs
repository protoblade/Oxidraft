//! A table is written as its lines and text (and its cells' blocks), which
//! other programs show as they are; XDATA on its first line keeps the
//! table whole — its grid, cells and cell formats — and the other parts
//! are marked as its parts, so it comes back a table.

use dxf::{XData, XDataItem};
use oxidraft_geom::Point;
use oxidraft_model::table::CellFormat;
use oxidraft_model::{Attach, Geometry};

pub const APP: &str = "OXIDRAFT_TABLE";

const ATTACHES: [Attach; 9] = [
    Attach::TopLeft,
    Attach::TopCenter,
    Attach::TopRight,
    Attach::MiddleLeft,
    Attach::MiddleCenter,
    Attach::MiddleRight,
    Attach::BottomLeft,
    Attach::BottomCenter,
    Attach::BottomRight,
];

fn long(n: usize) -> XDataItem {
    XDataItem::Long(i32::try_from(n).unwrap_or(i32::MAX))
}

/// The table as XDATA, or none when it's too big for it (long cells, or
/// past AutoCAD's 16 KB a object).
pub fn xdata(g: &Geometry) -> Option<XData> {
    let Geometry::Table { position, rotation, widths, heights, cells, text_height, formats } = g else { return None };
    let mut items = vec![
        XDataItem::Str("TABLE".into()),
        XDataItem::Real(position.x),
        XDataItem::Real(position.y),
        XDataItem::Real(*rotation),
        XDataItem::Real(*text_height),
        long(widths.len()),
        long(heights.len()),
    ];
    items.extend(widths.iter().chain(heights.iter()).map(|v| XDataItem::Real(*v)));
    let mut size = 0;
    for r in 0..heights.len() {
        for c in 0..widths.len() {
            let text = cells.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default();
            if text.len() > 250 {
                return None;
            }
            size += text.len() + 4;
            items.push(XDataItem::Str(text));
        }
    }
    items.push(long(formats.len()));
    for ((r, c), f) in formats {
        items.extend([long(*r), long(*c), XDataItem::Real(f.height.unwrap_or(-1.0))]);
        let attach = f.attach.and_then(|a| ATTACHES.iter().position(|x| *x == a)).map_or(-1, |i| i as i16);
        items.push(XDataItem::Integer(attach));
        items.push(XDataItem::Str(f.block.clone().unwrap_or_default()));
    }
    (size + items.len() * 10 < 15_000).then(|| XData { application_name: APP.into(), items })
}

/// The table a line's XDATA keeps, if it does.
pub fn read(x_data: &[XData]) -> Option<Geometry> {
    let x = x_data.iter().find(|x| x.application_name == APP)?;
    let mut it = x.items.iter();
    let real = |i: Option<&XDataItem>| if let Some(XDataItem::Real(v)) = i { Some(*v) } else { None };
    let count = |i: Option<&XDataItem>| if let Some(XDataItem::Long(n)) = i { usize::try_from(*n).ok() } else { None };
    let text = |i: Option<&XDataItem>| if let Some(XDataItem::Str(s)) = i { Some(s.clone()) } else { None };
    if text(it.next())? != "TABLE" {
        return None;
    }
    let position = Point::new(real(it.next())?, real(it.next())?);
    let (rotation, text_height) = (real(it.next())?, real(it.next())?);
    let (columns, rows) = (count(it.next())?, count(it.next())?);
    if columns == 0 || rows == 0 || columns * rows > 100_000 {
        return None;
    }
    let widths = (0..columns).map(|_| real(it.next())).collect::<Option<Vec<f64>>>()?;
    let heights = (0..rows).map(|_| real(it.next())).collect::<Option<Vec<f64>>>()?;
    let cells = (0..rows)
        .map(|_| (0..columns).map(|_| text(it.next())).collect::<Option<Vec<String>>>())
        .collect::<Option<Vec<_>>>()?;
    let n = count(it.next())?;
    let mut formats = Vec::new();
    for _ in 0..n {
        let (r, c, h) = (count(it.next())?, count(it.next())?, real(it.next())?);
        let attach = if let Some(XDataItem::Integer(a)) = it.next() { *a } else { return None };
        let block = text(it.next())?;
        let f = CellFormat {
            height: (h > 0.0).then_some(h),
            attach: usize::try_from(attach).ok().and_then(|a| ATTACHES.get(a).copied()),
            block: (!block.is_empty()).then_some(block),
        };
        formats.push(((r, c), f));
    }
    Some(Geometry::Table { position, rotation, widths, heights, cells, text_height, formats })
}

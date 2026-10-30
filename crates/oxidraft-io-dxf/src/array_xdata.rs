//! An associative array's layout, kept as XDATA on its block reference
//! (other programs see the reference and its nested items): the kind, the
//! item block, then the numbers. A path is kept as a polyline. Replaced
//! items follow a "REPLACED" marker, by number (one more than their
//! index); which block each shows is its nested reference's.

use dxf::{XData, XDataItem};
use oxidraft_geom::Point;
use oxidraft_model::assoc_array::{ArrayLayout, AssocArray, MAX_ITEMS};
use oxidraft_model::{Document, Entity, EntityId, Geometry, Vertex};

pub const APP: &str = "OXIDRAFT_ARRAY";
const REPLACED: &str = "REPLACED";

fn long(n: usize) -> XDataItem {
    XDataItem::Long(i32::try_from(n).unwrap_or(i32::MAX))
}

pub fn xdata(e: &Entity) -> Option<XData> {
    let a = e.array.as_deref()?;
    let mut items = vec![XDataItem::Str(String::new()), XDataItem::Str(a.item.clone())];
    let kind = match &a.layout {
        ArrayLayout::Rect { rows, columns, row_spacing, column_spacing } => {
            items.extend([
                long(*rows),
                long(*columns),
                XDataItem::Real(*row_spacing),
                XDataItem::Real(*column_spacing),
            ]);
            "RECT"
        }
        ArrayLayout::Polar { center, items: n, fill, rotate } => {
            items.extend([XDataItem::Real(center.x), XDataItem::Real(center.y), long(*n), XDataItem::Real(*fill)]);
            items.push(XDataItem::Integer(i16::from(*rotate)));
            "POLAR"
        }
        ArrayLayout::Path { path, items: n, align } => {
            let Geometry::Polyline { vertices, closed } =
                Geometry::polyline_from_segs(&path.segments(), path.is_closed())
            else {
                return None;
            };
            items.extend([long(*n), XDataItem::Integer(i16::from(*align)), XDataItem::Integer(i16::from(closed))]);
            for v in vertices {
                items.extend([XDataItem::Real(v.point.x), XDataItem::Real(v.point.y), XDataItem::Real(v.bulge)]);
            }
            "PATH"
        }
    };
    items[0] = XDataItem::Str(kind.into());
    if !a.replaced.is_empty() {
        items.push(XDataItem::Str(REPLACED.into()));
        items.extend(a.replaced.iter().map(|(i, _)| long(i + 1)));
    }
    Some(XData { application_name: APP.into(), items })
}

/// The array the reference `id` is, from its XDATA. The item block is
/// the one its block refers to: blocks may be renamed on the way.
pub fn read(x_data: &[XData], doc: &Document, id: EntityId) -> Option<Box<AssocArray>> {
    let x = x_data.iter().find(|x| x.application_name == APP)?;
    let marker = x.items.iter().position(|i| matches!(i, XDataItem::Str(s) if s == REPLACED));
    let (head, tail) = x.items.split_at(marker.unwrap_or(x.items.len()));
    let mut strings = head.iter().filter_map(|i| if let XDataItem::Str(s) = i { Some(s.as_str()) } else { None });
    let kind = strings.next()?;
    let Geometry::Insert { block, .. } = &doc.entity(id)?.geometry else { return None };
    let replaced_at: Vec<usize> = tail
        .iter()
        .filter_map(|i| if let XDataItem::Long(n) = i { usize::try_from(*n - 1).ok() } else { None })
        .collect();
    let shown: Vec<String> = doc
        .block(block)?
        .entities
        .iter()
        .filter_map(|be| match &be.geometry {
            Geometry::Insert { block, .. } => Some(block.clone()),
            _ => None,
        })
        .collect();
    let item = shown.iter().enumerate().find(|(i, _)| !replaced_at.contains(i)).map(|(_, b)| b.clone())?;
    let replaced: Vec<(usize, String)> =
        replaced_at.iter().filter_map(|i| shown.get(*i).map(|b| (*i, b.clone()))).collect();
    let reals: Vec<f64> =
        head.iter().filter_map(|i| if let XDataItem::Real(v) = i { Some(*v) } else { None }).collect();
    let counts: Vec<usize> = head
        .iter()
        .filter_map(|i| match i {
            XDataItem::Long(n) => usize::try_from(*n).ok(),
            _ => None,
        })
        .filter(|n| (1..=MAX_ITEMS).contains(n))
        .collect();
    let flags: Vec<bool> =
        head.iter().filter_map(|i| if let XDataItem::Integer(n) = i { Some(*n != 0) } else { None }).collect();
    let layout = match kind {
        "RECT" => match (counts.as_slice(), reals.as_slice()) {
            ([rows, columns], [row_spacing, column_spacing]) => ArrayLayout::Rect {
                rows: *rows,
                columns: *columns,
                row_spacing: *row_spacing,
                column_spacing: *column_spacing,
            },
            _ => return None,
        },
        "POLAR" => match (counts.as_slice(), reals.as_slice()) {
            ([items], [x, y, fill]) => ArrayLayout::Polar {
                center: Point::new(*x, *y),
                items: *items,
                fill: *fill,
                rotate: flags.first().copied().unwrap_or(true),
            },
            _ => return None,
        },
        "PATH" => {
            let (&[items], &[align, closed]) = (counts.as_slice(), flags.as_slice()) else { return None };
            let (vertices, _) = reals.as_chunks::<3>();
            if vertices.len() < 2 {
                return None;
            }
            let vertices = vertices.iter().map(|[x, y, b]| Vertex::new(Point::new(*x, *y), *b)).collect();
            ArrayLayout::Path { path: Box::new(Geometry::Polyline { vertices, closed }), items, align }
        }
        _ => return None,
    };
    reals.iter().all(|v| v.is_finite()).then(|| Box::new(AssocArray { layout, item, replaced }))
}

//! Tables (TABLE): a grid of cells under a title row, drawn as its lines
//! and its cells' text.

use oxidraft_geom::{Point, Segment};

use crate::{Geometry, annotation::Attach};

/// A table's shape: where it hangs from and how it's turned, its columns'
/// widths and rows' heights.
pub struct Grid<'a> {
    pub position: Point,
    pub rotation: f64,
    pub widths: &'a [f64],
    pub heights: &'a [f64],
}

impl Grid<'_> {
    /// A point `x` along and `y` down from the upper-left corner.
    fn at(&self, x: f64, y: f64) -> Point {
        self.position + Point::new(x, -y).rotated_about(Point::ORIGIN, self.rotation)
    }

    pub fn width(&self) -> f64 {
        self.widths.iter().sum()
    }

    pub fn height(&self) -> f64 {
        self.heights.iter().sum()
    }

    /// Upper-left, upper-right, lower-right and lower-left.
    pub fn corners(&self) -> [Point; 4] {
        let (w, h) = (self.width(), self.height());
        [self.at(0.0, 0.0), self.at(w, 0.0), self.at(w, h), self.at(0.0, h)]
    }

    /// Its lines: the border, a line under each row, and the lines between
    /// columns below the title row.
    pub fn lines(&self) -> Vec<Segment> {
        let (w, h) = (self.width(), self.height());
        let mut out = Vec::new();
        let mut y = 0.0;
        for row in std::iter::once(0.0).chain(self.heights.iter().copied()) {
            y += row;
            out.push(Segment::new(self.at(0.0, y), self.at(w, y)));
        }
        let title = self.heights.first().copied().unwrap_or(0.0);
        let mut x = 0.0;
        for (i, col) in std::iter::once(0.0).chain(self.widths.iter().copied()).enumerate() {
            x += col;
            let edge = i == 0 || i == self.widths.len();
            out.push(Segment::new(self.at(x, if edge { 0.0 } else { title }), self.at(x, h)));
        }
        out
    }

    /// The middle of the cell at `row` and `col`; the title row's is the
    /// middle of the whole row.
    pub fn cell_center(&self, row: usize, col: usize) -> Point {
        let y = self.heights[..row].iter().sum::<f64>() + self.heights[row] / 2.0;
        let x =
            if row == 0 { self.width() / 2.0 } else { self.widths[..col].iter().sum::<f64>() + self.widths[col] / 2.0 };
        self.at(x, y)
    }

    /// The cell's box: its left edge and width, its top (down from the
    /// table's top) and height.
    fn cell_box(&self, row: usize, col: usize) -> (f64, f64, f64, f64) {
        let top = self.heights[..row].iter().sum::<f64>();
        let (left, width) =
            if row == 0 { (0.0, self.width()) } else { (self.widths[..col].iter().sum::<f64>(), self.widths[col]) };
        (left, width, top, self.heights[row])
    }

    /// Where text attached `attach` goes in the cell, `margin` in from
    /// its edges.
    pub fn cell_anchor(&self, row: usize, col: usize, attach: Attach, margin: f64) -> Point {
        let (left, width, top, height) = self.cell_box(row, col);
        let (fx, fy) = attach.fractions();
        let x = left + margin.min(width / 2.0) + fx * (width - 2.0 * margin.min(width / 2.0));
        let y = top + margin.min(height / 2.0) + fy * (height - 2.0 * margin.min(height / 2.0));
        self.at(x, y)
    }

    /// The cell's size: width and height.
    pub fn cell_size(&self, row: usize, col: usize) -> (f64, f64) {
        let (_, w, _, h) = self.cell_box(row, col);
        (w, h)
    }

    /// The cell `p` is in, as (row, column); the title row's column is 0.
    pub fn cell_at(&self, p: Point) -> Option<(usize, usize)> {
        let local = (p - self.position).rotated_about(Point::ORIGIN, -self.rotation);
        let (x, y) = (local.x, -local.y);
        if x < 0.0 || y < 0.0 || x > self.width() || y > self.height() {
            return None;
        }
        let find = |sizes: &[f64], v: f64| {
            let mut edge = 0.0;
            sizes.iter().position(|s| {
                edge += s;
                v <= edge
            })
        };
        let row = find(self.heights, y)?;
        let col = if row == 0 { 0 } else { find(self.widths, x)? };
        Some((row, col))
    }
}

/// A cell's own formatting (MATCHCELL copies it), and a block shown in
/// it in place of text (TINSERT).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CellFormat {
    pub height: Option<f64>,
    pub attach: Option<Attach>,
    pub block: Option<String>,
}

/// The format of the cell at `row`, `col`.
pub fn cell_format(formats: &[((usize, usize), CellFormat)], row: usize, col: usize) -> CellFormat {
    formats.iter().find(|(at, _)| *at == (row, col)).map(|(_, f)| f.clone()).unwrap_or_default()
}

/// `formats` with the cell at `row`, `col` given `f` (none when plain).
pub fn with_cell_format(
    formats: &[((usize, usize), CellFormat)],
    row: usize,
    col: usize,
    f: CellFormat,
) -> Vec<((usize, usize), CellFormat)> {
    let mut out: Vec<_> = formats.iter().filter(|(at, _)| *at != (row, col)).cloned().collect();
    if f != CellFormat::default() {
        out.push(((row, col), f));
    }
    out
}

/// The table drawn: its lines, and its cells' texts centered in them.
pub fn primitives(g: &Geometry) -> Vec<Geometry> {
    let Geometry::Table { position, rotation, widths, heights, cells, text_height, formats } = g else {
        return Vec::new();
    };
    let grid = Grid { position: *position, rotation: *rotation, widths, heights };
    let mut out: Vec<Geometry> =
        grid.lines().into_iter().map(|s| Geometry::Line { start: s.start, end: s.end }).collect();
    for (r, row) in cells.iter().enumerate().take(heights.len()) {
        for (c, text) in row.iter().enumerate().take(if r == 0 { 1 } else { widths.len() }) {
            let f = cell_format(formats, r, c);
            if text.trim().is_empty() || f.block.is_some() {
                continue;
            }
            // The title a size larger, as the Standard table style has it.
            let height = f.height.unwrap_or(if r == 0 { text_height * 1.4 } else { *text_height });
            let attach = f.attach.unwrap_or(Attach::MiddleCenter);
            out.push(Geometry::Text {
                look: Default::default(),
                position: grid.cell_anchor(r, c, attach, text_height * 0.4),
                height,
                rotation: *rotation,
                width: 0.0,
                attach,
                content: text.clone(),
            });
        }
    }
    out
}

/// A new table: a title row, a header row and `rows` data rows of
/// `columns` columns, each `column_width` wide, rows `row_height` tall.
/// How new tables look (TABLESTYLE): their text height and the margin
/// between a cell's text and its edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableStyle {
    pub text_height: f64,
    pub margin: f64,
}

pub const DEFAULT_TABLE_STYLE: TableStyle = TableStyle { text_height: 4.5, margin: 1.8 };

impl Default for TableStyle {
    fn default() -> Self {
        DEFAULT_TABLE_STYLE
    }
}

pub fn new_table(
    position: Point,
    columns: usize,
    rows: usize,
    column_width: f64,
    row_height: f64,
    text_height: f64,
) -> Geometry {
    let columns = columns.max(1);
    let total = rows + 2;
    let mut cells = vec![vec![String::new(); columns]; total];
    cells[0][0] = "Title".into();
    Geometry::Table {
        position,
        rotation: 0.0,
        widths: vec![column_width; columns],
        heights: vec![row_height; total],
        cells,
        text_height,
        formats: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_draws_its_grid_and_finds_its_cells() {
        let t = new_table(Point::new(0.0, 100.0), 3, 2, 20.0, 8.0, 2.5);
        let Geometry::Table { widths, heights, .. } = &t else { panic!() };
        let grid = Grid { position: Point::new(0.0, 100.0), rotation: 0.0, widths, heights };
        // Title, header and two data rows: five lines across, four down.
        assert_eq!(grid.lines().len(), 5 + 4);
        assert_eq!(grid.corners()[2], Point::new(60.0, 68.0));
        assert_eq!(grid.cell_at(Point::new(45.0, 98.0)), Some((0, 0)), "the title spans the row");
        assert_eq!(grid.cell_at(Point::new(45.0, 80.0)), Some((2, 2)));
        assert_eq!(grid.cell_at(Point::new(65.0, 80.0)), None);
        assert_eq!(grid.cell_center(1, 1), Point::new(30.0, 88.0));
        // The title's text, centered over the whole row.
        let texts: Vec<Geometry> = primitives(&t).into_iter().filter(|g| matches!(g, Geometry::Text { .. })).collect();
        assert!(
            matches!(&texts[..], [Geometry::Text { position, content, .. }] if *position == Point::new(30.0, 96.0) && content == "Title")
        );
    }
}

impl crate::Document {
    /// The block references a table's block cells show (TINSERT), each
    /// fitted into its cell.
    pub fn table_inserts(&self, g: &Geometry) -> Vec<Geometry> {
        let Geometry::Table { position, rotation, widths, heights, formats, text_height, .. } = g else {
            return Vec::new();
        };
        let grid = Grid { position: *position, rotation: *rotation, widths, heights };
        let mut out = Vec::new();
        for ((r, c), f) in formats {
            let (Some(name), true) = (&f.block, *r < heights.len() && (*r == 0 || *c < widths.len())) else { continue };
            let Some((def, ext)) = self.block(name).zip(self.block_extents(name)) else { continue };
            let (w, h) = grid.cell_size(*r, *c);
            let margin = text_height * 0.4;
            let (bw, bh) = ((ext.max.x - ext.min.x).max(1e-9), (ext.max.y - ext.min.y).max(1e-9));
            let scale = ((w - 2.0 * margin) / bw).min((h - 2.0 * margin) / bh).max(1e-9);
            let attach = f.attach.unwrap_or(Attach::MiddleCenter);
            let (fx, fy) = attach.fractions();
            // The block's extents placed in the cell as text would be.
            let anchor = grid.cell_anchor(*r, *c, attach, margin);
            let local = Point::new(ext.min.x + fx * bw, ext.max.y - fy * bh) - def.base;
            let offset = (local * scale).rotated_about(Point::ORIGIN, *rotation);
            let placement = crate::Placement { position: anchor - offset, scale, rotation: *rotation, mirror: false };
            let attributes = def.attributes_for(&placement, |_| None);
            out.push(Geometry::Insert { block: def.name.clone(), placement, attributes, clip: None });
        }
        out
    }
}

#[cfg(test)]
mod cell_tests {
    use super::*;
    use crate::{Block, BlockEntity, Color, Document};

    #[test]
    fn cells_with_their_own_format_and_blocks() {
        let mut d = Document::new();
        let square = Geometry::Polyline {
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
                .map(|(x, y)| crate::Vertex::at(Point::new(x, y)))
                .to_vec(),
            closed: true,
        };
        d.define_block(Block {
            name: "SQ".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByBlock, square)],
        });
        let Geometry::Table { position, rotation, widths, heights, mut cells, text_height, .. } =
            new_table(Point::new(0.0, 100.0), 2, 2, 20.0, 10.0, 2.5)
        else {
            unreachable!()
        };
        cells[2][0] = "LEFT".into();
        let formats = vec![
            ((2, 0), CellFormat { attach: Some(Attach::MiddleLeft), height: Some(4.0), block: None }),
            ((2, 1), CellFormat { block: Some("SQ".into()), ..Default::default() }),
        ];
        let t = Geometry::Table { position, rotation, widths, heights, cells, text_height, formats };
        let text = primitives(&t).into_iter().find_map(|g| match g {
            Geometry::Text { position, height, attach, content, .. } if content == "LEFT" => {
                Some((position, height, attach))
            }
            _ => None,
        });
        let (at, h, attach) = text.unwrap();
        assert_eq!((h, attach), (4.0, Attach::MiddleLeft));
        assert!((at.x - 1.0).abs() < 1e-9, "a margin in from the left: {at:?}");
        let inserts = d.table_inserts(&t);
        let Geometry::Insert { placement, .. } = &inserts[0] else { panic!() };
        // 10 high less margins, from a 10 unit square: scale 0.8, centered.
        assert!((placement.scale - 0.8).abs() < 1e-9, "{placement:?}");
        assert!(placement.position.approx_eq(Point::new(26.0, 71.0)), "{placement:?}");
    }
}

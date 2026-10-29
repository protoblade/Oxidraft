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

/// The table drawn: its lines, and its cells' texts centered in them.
pub fn primitives(g: &Geometry) -> Vec<Geometry> {
    let Geometry::Table { position, rotation, widths, heights, cells, text_height } = g else { return Vec::new() };
    let grid = Grid { position: *position, rotation: *rotation, widths, heights };
    let mut out: Vec<Geometry> =
        grid.lines().into_iter().map(|s| Geometry::Line { start: s.start, end: s.end }).collect();
    for (r, row) in cells.iter().enumerate().take(heights.len()) {
        for (c, text) in row.iter().enumerate().take(if r == 0 { 1 } else { widths.len() }) {
            if text.trim().is_empty() {
                continue;
            }
            // The title a size larger, as the Standard table style has it.
            let height = if r == 0 { text_height * 1.4 } else { *text_height };
            out.push(Geometry::Text {
                look: Default::default(),
                position: grid.cell_center(r, c),
                height,
                rotation: *rotation,
                width: 0.0,
                attach: Attach::MiddleCenter,
                content: text.clone(),
            });
        }
    }
    out
}

/// A new table: a title row, a header row and `rows` data rows of
/// `columns` columns, each `column_width` wide, rows `row_height` tall.
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

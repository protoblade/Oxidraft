//! Writes a small sample drawing: `cargo run -p oxidraft-io-dxf --example demo -- demo.dxf`

use oxidraft_geom::Point;
use oxidraft_model::{Color, Document, Geometry, Layer, Vertex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "demo.dxf".into());

    let mut doc = Document::new();
    doc.add_layer(Layer::new("Walls", 7));
    doc.add_layer(Layer::new("Doors", 2));
    doc.add_layer(Layer::new("Columns", 1));

    let wall = |doc: &mut Document, a: (f64, f64), b: (f64, f64)| {
        let g = Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) };
        doc.add_on_layer(g, "Walls", Color::ByLayer);
    };
    for (a, b) in [((0.0, 0.0), (100.0, 0.0)), ((100.0, 0.0), (100.0, 60.0)), ((100.0, 60.0), (0.0, 60.0))] {
        wall(&mut doc, a, b);
    }
    wall(&mut doc, (0.0, 60.0), (0.0, 35.0));
    wall(&mut doc, (0.0, 25.0), (0.0, 0.0));
    wall(&mut doc, (60.0, 0.0), (60.0, 60.0));

    doc.add_on_layer(
        Geometry::Line { start: Point::new(0.0, 25.0), end: Point::new(10.0, 25.0) },
        "Doors",
        Color::ByLayer,
    );
    for (x, y) in [(20.0, 20.0), (40.0, 20.0), (20.0, 40.0), (40.0, 40.0), (80.0, 30.0)] {
        doc.add_on_layer(Geometry::Circle { center: Point::new(x, y), radius: 3.0 }, "Columns", Color::ByLayer);
    }
    // Door swing and a table with rounded corners.
    doc.add_on_layer(
        Geometry::Arc { center: Point::new(0.0, 25.0), radius: 10.0, start_angle: 0.0, end_angle: 90.0 },
        "Doors",
        Color::ByLayer,
    );
    let b = (22.5f64).to_radians().tan();
    let table = [
        (72.0, 40.0, 0.0),
        (88.0, 40.0, b),
        (90.0, 42.0, 0.0),
        (90.0, 50.0, b),
        (88.0, 52.0, 0.0),
        (72.0, 52.0, b),
        (70.0, 50.0, 0.0),
        (70.0, 42.0, b),
    ];
    doc.add_on_layer(
        Geometry::Polyline {
            vertices: table.iter().map(|&(x, y, b)| Vertex::new(Point::new(x, y), b)).collect(),
            closed: true,
        },
        "0",
        Color::Index(4),
    );

    oxidraft_io_dxf::save_file(&doc, &path)?;
    println!("Wrote {path} ({} objects)", doc.len());
    Ok(())
}

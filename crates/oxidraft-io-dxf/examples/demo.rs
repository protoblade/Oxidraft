//! Writes a small sample drawing: `cargo run -p oxidraft-io-dxf --example demo -- demo.dxf`

use oxidraft_geom::Point;
use oxidraft_model::{Attach, Color, Document, Geometry, HatchPattern, Layer, Lineweight, Vertex};

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

    // Annotation: a title, overall dimensions, a leader, and the right room hatched
    // around the table and column.
    doc.add(Geometry::Text {
        position: Point::new(0.0, 80.0),
        height: 4.0,
        rotation: 0.0,
        width: 0.0,
        attach: Attach::TopLeft,
        content: "GROUND FLOOR PLAN".into(),
    });
    for (p1, p2, line, rotation) in [
        (Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(50.0, -9.0), 0.0),
        (Point::new(100.0, 0.0), Point::new(100.0, 60.0), Point::new(109.0, 30.0), 90.0),
        (Point::new(0.0, 60.0), Point::new(60.0, 60.0), Point::new(30.0, 66.0), 0.0),
    ] {
        doc.add(Geometry::Dimension { p1, p2, line, rotation, aligned: false, text: None, scale: 1.0 });
    }
    doc.add(Geometry::Leader {
        points: vec![Point::new(82.2, 32.2), Point::new(92.0, 42.0)],
        content: "CONCRETE COLUMN".into(),
        scale: 1.0,
    });
    if let Some(loops) = oxidraft_model::boundary::boundary_at(&doc, Point::new(65.0, 10.0)) {
        doc.add_on_layer(
            Geometry::Hatch { loops, pattern: HatchPattern::Ansi31, scale: 1.5, angle: 0.0 },
            "Hatch",
            Color::Index(8),
        );
    }

    // A center line on its own layer, heavier walls, and a solid-filled slab.
    let mut axes = Layer::new("Axes", 1);
    axes.linetype = "CENTER".into();
    doc.add_layer(axes);
    doc.add_on_layer(
        Geometry::Line { start: Point::new(30.0, -15.0), end: Point::new(30.0, 75.0) },
        "Axes",
        Color::ByLayer,
    );
    if let Some(mut walls) = doc.layer("Walls").cloned() {
        walls.lineweight = Lineweight::Hundredths(50);
        doc.update_layer(walls);
    }
    let slab: Vec<Vertex> = [(70.0, 45.0), (95.0, 45.0), (95.0, 55.0), (70.0, 55.0)]
        .iter()
        .map(|&(x, y)| Vertex::at(Point::new(x, y)))
        .collect();
    doc.add_on_layer(
        Geometry::Hatch { loops: vec![slab], pattern: HatchPattern::Solid, scale: 1.0, angle: 0.0 },
        "Hatch",
        Color::Index(9),
    );
    doc.add(Geometry::AngularDimension {
        vertex: Point::new(60.0, 0.0),
        p1: Point::new(100.0, 0.0),
        p2: Point::new(60.0, 60.0),
        arc: Point::new(75.0, 15.0),
        text: None,
        scale: 1.0,
    });

    oxidraft_io_dxf::save_file(&doc, &path)?;
    println!("Wrote {path} ({} objects)", doc.len());
    Ok(())
}

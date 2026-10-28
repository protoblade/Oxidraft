//! Reading and writing DXF, Oxidraft's native exchange format.

use std::io::{BufRead, Write};
use std::path::Path;

use dxf::LwPolylineVertex;
use dxf::entities::{Entity as DxfEntity, EntityType, LwPolyline};
use dxf::enums::AcadVersion;
use dxf::tables::Layer as DxfLayer;
use dxf::{Color as DxfColor, Drawing, Point as DxfPoint};
use oxidraft_geom::Point;
use oxidraft_model::{Block, BlockEntity, Color, Document, Geometry, Layer, Placement, Vertex};

pub type Error = dxf::DxfError;

/// Result of an import, with what was skipped.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    /// Number of DXF entities of types Oxidraft can't represent yet.
    pub skipped: usize,
}

fn to_dxf(p: Point) -> DxfPoint {
    DxfPoint::new(p.x, p.y, 0.0)
}

fn from_dxf(p: &DxfPoint) -> Point {
    Point::new(p.x, p.y)
}

fn to_dxf_color(c: Color) -> DxfColor {
    match c {
        Color::ByLayer => DxfColor::by_layer(),
        Color::Index(i) => DxfColor::from_index(i),
    }
}

fn from_dxf_color(c: &DxfColor) -> Color {
    match c.index() {
        Some(i) if !c.is_by_layer() && i > 0 => Color::Index(i),
        _ => Color::ByLayer,
    }
}

fn geometry_to_dxf(g: &Geometry) -> EntityType {
    match g {
        Geometry::Line { start, end } => EntityType::Line(dxf::entities::Line::new(to_dxf(*start), to_dxf(*end))),
        Geometry::Circle { center, radius } => EntityType::Circle(dxf::entities::Circle::new(to_dxf(*center), *radius)),
        Geometry::Arc { center, radius, start_angle, end_angle } => EntityType::Arc(dxf::entities::Arc {
            center: to_dxf(*center),
            radius: *radius,
            start_angle: *start_angle,
            end_angle: *end_angle,
            ..Default::default()
        }),
        Geometry::Polyline { vertices, closed } => {
            let mut pl = LwPolyline {
                vertices: vertices
                    .iter()
                    .map(|v| LwPolylineVertex { x: v.point.x, y: v.point.y, bulge: v.bulge, ..Default::default() })
                    .collect(),
                ..Default::default()
            };
            pl.set_is_closed(*closed);
            EntityType::LwPolyline(pl)
        }
        // A mirrored reference is a negative X scale in DXF.
        Geometry::Insert { block, placement } => EntityType::Insert(dxf::entities::Insert {
            name: block.clone(),
            location: to_dxf(placement.position),
            x_scale_factor: if placement.mirror { -placement.scale } else { placement.scale },
            y_scale_factor: placement.scale,
            z_scale_factor: placement.scale,
            rotation: placement.rotation,
            ..Default::default()
        }),
    }
}

fn dxf_entity(geometry: &Geometry, layer: &str, color: Color) -> DxfEntity {
    let mut de = DxfEntity::new(geometry_to_dxf(geometry));
    de.common.layer = layer.to_string();
    de.common.color = to_dxf_color(color);
    de
}

/// Builds a DXF drawing from a document.
pub fn to_drawing(doc: &Document) -> Drawing {
    let mut drawing = Drawing::new();
    // R2000 is the oldest version with lightweight polylines.
    drawing.header.version = AcadVersion::R2000;
    for l in doc.layers() {
        if l.name == "0" {
            continue; // Drawing::new() already has layer 0
        }
        drawing.add_layer(DxfLayer {
            name: l.name.clone(),
            color: DxfColor::from_index(l.color),
            is_layer_on: l.on,
            ..Default::default()
        });
    }
    for b in doc.blocks() {
        drawing.add_block(dxf::Block {
            name: b.name.clone(),
            base_point: to_dxf(b.base),
            entities: b.entities.iter().map(|be| dxf_entity(&be.geometry, &be.layer, be.color)).collect(),
            ..Default::default()
        });
    }
    for e in doc.entities() {
        drawing.add_entity(dxf_entity(&e.geometry, &e.layer, e.color));
    }
    drawing
}

/// Converts one DXF entity, or `None` for types Oxidraft doesn't support yet.
fn geometry_from_dxf(specific: &EntityType) -> Option<Geometry> {
    Some(match specific {
        EntityType::Line(l) => Geometry::Line { start: from_dxf(&l.p1), end: from_dxf(&l.p2) },
        EntityType::Circle(c) => Geometry::Circle { center: from_dxf(&c.center), radius: c.radius },
        EntityType::Arc(a) => Geometry::Arc {
            center: from_dxf(&a.center),
            radius: a.radius,
            start_angle: a.start_angle,
            end_angle: a.end_angle,
        },
        EntityType::LwPolyline(pl) => Geometry::Polyline {
            vertices: pl.vertices.iter().map(|v| Vertex::new(Point::new(v.x, v.y), v.bulge)).collect(),
            closed: pl.is_closed(),
        },
        // Old-style (R12) 2D polylines.
        EntityType::Polyline(pl) => Geometry::Polyline {
            vertices: pl.vertices().map(|v| Vertex::new(from_dxf(&v.location), v.bulge)).collect(),
            closed: pl.is_closed(),
        },
        // Non-uniform scales are approximated by the Y scale.
        EntityType::Insert(i) => Geometry::Insert {
            block: i.name.clone(),
            placement: Placement {
                position: from_dxf(&i.location),
                scale: if i.y_scale_factor != 0.0 { i.y_scale_factor.abs() } else { i.x_scale_factor.abs() },
                rotation: i.rotation,
                mirror: (i.x_scale_factor < 0.0) != (i.y_scale_factor < 0.0),
            },
        },
        _ => return None,
    })
}

/// Builds a document from a DXF drawing.
pub fn from_drawing(drawing: &Drawing) -> Imported {
    let mut doc = Document::new();
    for l in drawing.layers() {
        let color = l.color.index().filter(|&i| i > 0).unwrap_or(7);
        let mut layer = Layer::new(l.name.clone(), color);
        layer.on = l.is_layer_on;
        doc.add_layer(layer.clone());
        doc.update_layer(layer);
    }
    let mut skipped = 0;
    // "*Model_Space" and "*Paper_Space" hold the drawing itself, not reusable blocks.
    for b in drawing.blocks().filter(|b| !b.name.starts_with('*')) {
        let mut entities = Vec::new();
        for e in &b.entities {
            match geometry_from_dxf(&e.specific) {
                Some(geometry) => entities.push(BlockEntity {
                    layer: e.common.layer.clone(),
                    color: from_dxf_color(&e.common.color),
                    geometry,
                }),
                None => skipped += 1,
            }
        }
        doc.define_block(Block { name: b.name.clone(), base: from_dxf(&b.base_point), entities });
    }
    for e in drawing.entities() {
        match geometry_from_dxf(&e.specific) {
            Some(g) => {
                doc.add_on_layer(g, &e.common.layer, from_dxf_color(&e.common.color));
            }
            None => skipped += 1,
        }
    }
    // A freshly opened file starts with an empty undo history.
    doc.forget_history();
    Imported { document: doc, skipped }
}

pub fn write<W: Write>(doc: &Document, writer: &mut W) -> Result<(), Error> {
    to_drawing(doc).save(writer)
}

pub fn read<R: BufRead>(reader: &mut R) -> Result<Imported, Error> {
    Ok(from_drawing(&Drawing::load(reader)?))
}

pub fn save_file(doc: &Document, path: impl AsRef<Path>) -> Result<(), Error> {
    to_drawing(doc).save_file(path.as_ref())
}

pub fn load_file(path: impl AsRef<Path>) -> Result<Imported, Error> {
    Ok(from_drawing(&Drawing::load_file(path.as_ref())?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_entities_layers_and_colors() {
        let mut doc = Document::new();
        doc.add_layer(Layer::new("Walls", 1));
        doc.add_on_layer(
            Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 5.0) },
            "Walls",
            Color::ByLayer,
        );
        doc.add(Geometry::Circle { center: Point::new(3.0, 4.0), radius: 2.5 });
        let mut hidden = Layer::new("Hidden", 5);
        hidden.on = false;
        doc.add_layer(hidden.clone());
        doc.update_layer(hidden);
        doc.define_block(Block {
            name: "CHAIR".into(),
            base: Point::new(1.0, 1.0),
            entities: vec![BlockEntity {
                layer: "0".into(),
                color: Color::ByLayer,
                geometry: Geometry::Circle { center: Point::new(1.0, 1.0), radius: 0.5 },
            }],
        });
        doc.add(Geometry::Insert {
            block: "CHAIR".into(),
            placement: Placement { position: Point::new(20.0, 5.0), scale: 2.0, rotation: 30.0, mirror: true },
        });
        doc.add(Geometry::Arc { center: Point::new(1.0, 2.0), radius: 3.0, start_angle: 30.0, end_angle: 300.0 });
        doc.add(Geometry::Polyline {
            vertices: vec![
                Vertex::at(Point::new(0.0, 0.0)),
                Vertex::new(Point::new(10.0, 0.0), 0.5),
                Vertex::at(Point::new(10.0, 10.0)),
            ],
            closed: true,
        });
        doc.add_on_layer(
            Geometry::Line { start: Point::new(1.0, 1.0), end: Point::new(2.0, 2.0) },
            "0",
            Color::Index(3),
        );

        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let imported = read(&mut buf.as_slice()).unwrap();
        let back = imported.document;

        assert_eq!(imported.skipped, 0);
        assert_eq!(back.layer("Walls").map(|l| l.color), Some(1));
        assert_eq!(back.block("CHAIR"), doc.block("CHAIR"));
        assert!(!back.layer("Hidden").unwrap().on);
        let got: Vec<_> = back.entities().map(|e| (e.layer.clone(), e.color, e.geometry.clone())).collect();
        let want: Vec<_> = doc.entities().map(|e| (e.layer.clone(), e.color, e.geometry.clone())).collect();
        assert_eq!(got, want);
        assert!(!back.can_undo(), "opening a file should not be undoable");
    }
}

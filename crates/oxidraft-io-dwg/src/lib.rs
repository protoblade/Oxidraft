//! DWG, AutoCAD's native format. [acadrust] reads and writes the binary
//! file; the drawing passes through DXF in memory on the way, so whatever
//! Oxidraft reads and writes in DXF works the same in DWG.

use std::io::Cursor;
use std::path::Path;

use acadrust::{DwgReadOptions, DwgReader, DwgWriter, DxfReader, DxfWriter, types::DxfVersion};
use oxidraft_io_dxf::Imported;
use oxidraft_model::Document;

pub type Error = String;

/// The DWG versions Oxidraft saves, named as AutoCAD's Save As lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Version {
    R2000,
    R2004,
    R2007,
    R2010,
    R2013,
    #[default]
    R2018,
}

impl Version {
    /// The version a file was saved in; older ones save back as 2000, the
    /// oldest Oxidraft writes.
    fn of(v: DxfVersion) -> Version {
        match v {
            DxfVersion::AC1018 => Version::R2004,
            DxfVersion::AC1021 => Version::R2007,
            DxfVersion::AC1024 => Version::R2010,
            DxfVersion::AC1027 => Version::R2013,
            DxfVersion::AC1032 | DxfVersion::Unknown => Version::R2018,
            _ => Version::R2000,
        }
    }

    fn code(self) -> DxfVersion {
        match self {
            Version::R2000 => DxfVersion::AC1015,
            Version::R2004 => DxfVersion::AC1018,
            Version::R2007 => DxfVersion::AC1021,
            Version::R2010 => DxfVersion::AC1024,
            Version::R2013 => DxfVersion::AC1027,
            Version::R2018 => DxfVersion::AC1032,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Version::R2000 => "AutoCAD 2000",
            Version::R2004 => "AutoCAD 2004",
            Version::R2007 => "AutoCAD 2007",
            Version::R2010 => "AutoCAD 2010",
            Version::R2013 => "AutoCAD 2013",
            Version::R2018 => "AutoCAD 2018",
        }
    }
}

/// A DWG file read in, and the version it was saved in.
#[derive(Debug)]
pub struct Opened {
    pub imported: Imported,
    pub version: Version,
}

/// True for a path ending in `.dwg`, any case.
pub fn is_dwg(path: impl AsRef<Path>) -> bool {
    path.as_ref().extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg"))
}

/// Reads a DWG file's bytes. Damaged files are read as far as they go, as
/// AutoCAD's RECOVER would.
pub fn read(bytes: &[u8]) -> Result<Opened, Error> {
    // Every DWG starts with its version: AC1009 (R12) to AC1032 (2018).
    if !bytes.starts_with(b"AC1") {
        return Err("not a DWG file".into());
    }
    let options = DwgReadOptions { failsafe: true, ..Default::default() };
    let mut cad = DwgReader::from_stream_with_options(Cursor::new(bytes.to_vec()), options)
        .read()
        .map_err(|e| format!("not a readable DWG file ({e})"))?;
    let version = Version::of(cad.version);
    // Oxidraft keeps no extended data, and some trips up the DXF reader.
    for e in cad.entities_mut() {
        e.common_mut().extended_data = Default::default();
    }
    let dxf = DxfWriter::new(&cad).with_version(DxfVersion::AC1032).write_to_vec().map_err(|e| e.to_string())?;
    let imported = oxidraft_io_dxf::read(&mut dxf.as_slice()).map_err(|e| e.to_string())?;
    Ok(Opened { imported, version })
}

/// The drawing as a DWG file of `version`.
pub fn write(doc: &Document, version: Version) -> Result<Vec<u8>, Error> {
    let mut dxf = Vec::new();
    oxidraft_io_dxf::write(doc, &mut dxf).map_err(|e| e.to_string())?;
    let mut cad = DxfReader::from_reader(Cursor::new(dxf)).and_then(|r| r.read()).map_err(|e| e.to_string())?;
    cad.version = version.code();
    let mut out = Cursor::new(Vec::new());
    DwgWriter::write_to_writer(&mut out, &cad).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

pub fn load_file(path: impl AsRef<Path>) -> Result<Opened, Error> {
    read(&std::fs::read(path).map_err(|e| e.to_string())?)
}

pub fn save_file(doc: &Document, path: impl AsRef<Path>, version: Version) -> Result<(), Error> {
    std::fs::write(path, write(doc, version)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_geom::Point;
    use oxidraft_model::{
        Attach, Attribute, Block, BlockEntity, Color, Ellipse, Geometry, HatchPattern, Layer, Placement, Spline, Vertex,
    };

    fn survey() -> Document {
        let mut doc = Document::new();
        doc.add_layer(Layer::new("Граници", 3));
        doc.add_on_layer(
            Geometry::Line { start: Point::ORIGIN, end: Point::new(100.0, 0.0) },
            "Граници",
            Color::ByLayer,
        );
        doc.add(Geometry::Circle { center: Point::new(50.0, 50.0), radius: 5.0 });
        doc.add(Geometry::Text {
            position: Point::new(10.0, 10.0),
            height: 2.5,
            rotation: 30.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "УПИ XI-57, кв.30".into(),
        });
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].map(|(x, y)| Vertex::at(Point::new(x, y)));
        doc.add(Geometry::Polyline {
            vertices: square.iter().map(|v| v.with_widths(0.5, 0.5)).collect(),
            closed: true,
        });
        doc.add(Geometry::Hatch {
            loops: vec![square.to_vec()],
            pattern: HatchPattern::Ansi31,
            scale: 1.0,
            angle: 0.0,
        });
        doc.add(Geometry::Ellipse(Ellipse::full(Point::new(30.0, 30.0), Point::new(8.0, 0.0), 0.5)));
        let fit = [(0.0, 20.0), (10.0, 25.0), (20.0, 20.0)].map(|(x, y)| Point::new(x, y));
        doc.add(Geometry::Spline(Spline::through(&fit, false).unwrap()));
        doc.define_block(Block {
            name: "KW".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new(
                "0",
                Color::ByBlock,
                Geometry::Circle { center: Point::ORIGIN, radius: 1.0 },
            )],
        });
        doc.add(Geometry::Insert {
            block: "KW".into(),
            placement: Placement::at(Point::new(70.0, 20.0)),
            attributes: vec![Attribute {
                tag: "KWNOM".into(),
                value: "12".into(),
                layer: "0".into(),
                color: Color::ByBlock,
                position: Point::new(71.5, 20.0),
                height: 2.0,
                rotation: 0.0,
                attach: Attach::BottomLeft,
                invisible: false,
            }],
        });
        doc
    }

    fn kinds(doc: &Document) -> Vec<&'static str> {
        let mut k: Vec<&str> = doc.entities().map(|e| e.geometry.name()).collect();
        k.sort_unstable();
        k
    }

    #[test]
    fn every_version_reads_back_what_was_written() {
        let doc = survey();
        for version in [Version::R2000, Version::R2004, Version::R2010, Version::R2013, Version::R2018] {
            let bytes = write(&doc, version).unwrap();
            assert!(bytes.starts_with(version.code().as_str().as_bytes()), "{version:?}");
            let opened = read(&bytes).unwrap();
            assert_eq!(opened.version, version);
            let back = opened.imported.document;
            assert_eq!(kinds(&back), kinds(&doc), "{version:?}");
            assert!(back.layer("Граници").is_some_and(|l| l.color == 3), "{version:?}");
            let text = back.entities().find_map(|e| match &e.geometry {
                Geometry::Text { content, rotation, .. } => Some((content.clone(), *rotation)),
                _ => None,
            });
            let (content, rotation) = text.unwrap();
            assert!(content == "УПИ XI-57, кв.30" && (rotation - 30.0).abs() < 1e-6, "{version:?}: {content}");
            let attribute = back.entities().find_map(|e| match &e.geometry {
                Geometry::Insert { attributes, .. } => attributes.first().map(|a| a.value.clone()),
                _ => None,
            });
            assert_eq!(attribute.as_deref(), Some("12"));
            let width = back.entities().find_map(|e| match &e.geometry {
                Geometry::Polyline { vertices, .. } => oxidraft_model::constant_width(vertices),
                _ => None,
            });
            assert_eq!(width, Some(0.5));
        }
    }

    #[test]
    fn rejects_what_isnt_dwg() {
        assert!(read(b"0\nSECTION\n").is_err());
        assert!(is_dwg("plan.DWG") && !is_dwg("plan.dxf"));
    }
}

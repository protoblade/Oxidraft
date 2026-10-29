//! DWG, AutoCAD's native format. [acadrust] reads and writes the binary
//! file; the drawing passes through DXF in memory on the way, so whatever
//! Oxidraft reads and writes in DXF works the same in DWG.

use std::io::Cursor;
use std::path::Path;

use acadrust::{CadDocument, DwgReadOptions, DwgReader, DwgWriter, DxfReader, DxfWriter, types::DxfVersion};
use oxidraft_io_dxf::Imported;
use oxidraft_model::Document;

mod layouts;

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

/// The drawing as acadrust holds it, layouts and all, ready to write.
fn cad(doc: &Document) -> Result<CadDocument, Error> {
    let mut dxf = Vec::new();
    oxidraft_io_dxf::write(doc, &mut dxf).map_err(|e| e.to_string())?;
    let mut cad = DxfReader::from_reader(Cursor::new(dxf)).and_then(|r| r.read()).map_err(|e| e.to_string())?;
    layouts::finish(&mut cad, doc);
    Ok(cad)
}

/// The drawing as a DWG file of `version`.
pub fn write(doc: &Document, version: Version) -> Result<Vec<u8>, Error> {
    let mut cad = cad(doc)?;
    cad.version = version.code();
    let mut out = Cursor::new(Vec::new());
    DwgWriter::write_to_writer(&mut out, &cad).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// The drawing as a DXF file: AutoCAD 2000, or 2010 with PDF underlays
/// (2007 is the first to have them). Written by acadrust, which also
/// writes the layouts' page setups and viewports.
pub fn write_dxf(doc: &Document) -> Result<Vec<u8>, Error> {
    let pdfs = doc.image_defs().any(|d| d.is_pdf());
    let version = if pdfs { DxfVersion::AC1024 } else { DxfVersion::AC1015 };
    let mut cad = cad(doc)?;
    cad.version = version;
    DxfWriter::new(&cad).with_version(version).write_to_vec().map_err(|e| e.to_string())
}

pub fn save_dxf(doc: &Document, path: impl AsRef<Path>) -> Result<(), Error> {
    std::fs::write(path, write_dxf(doc)?).map_err(|e| e.to_string())
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
            clip: None,
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
    fn xrefs_keep_their_path_in_every_version() {
        use oxidraft_model::{Xref, XrefStatus};
        let mut doc = Document::new();
        doc.set_xref(Xref {
            name: "SITE".into(),
            path: "site.dwg".into(),
            overlay: false,
            status: XrefStatus::NotFound,
        });
        doc.define_block(Block { name: "SITE".into(), base: Point::ORIGIN, entities: Vec::new() });
        doc.add(Geometry::Insert {
            block: "SITE".into(),
            placement: Placement::at(Point::new(3.0, 4.0)),
            attributes: Vec::new(),
            clip: None,
        });
        doc.set_xref(Xref {
            name: "ROADS".into(),
            path: "C:\\maps\\roads.dxf".into(),
            overlay: true,
            status: XrefStatus::NotFound,
        });
        doc.define_block(Block { name: "ROADS".into(), base: Point::ORIGIN, entities: Vec::new() });
        let mut off = oxidraft_model::Layer::new("SITE|Walls", 5);
        off.on = false;
        doc.add_layer(off.clone());
        doc.update_layer(off);
        for version in [Version::R2000, Version::R2004, Version::R2018] {
            let back = read(&write(&doc, version).unwrap()).unwrap().imported.document;
            let site = back.xref("SITE").unwrap_or_else(|| panic!("{version:?}: no SITE"));
            assert!(site.path == "site.dwg" && !site.overlay, "{version:?}: {site:?}");
            let roads = back.xref("ROADS").unwrap_or_else(|| panic!("{version:?}: no ROADS"));
            assert!(roads.path == "C:\\maps\\roads.dxf" && roads.overlay, "{version:?}: {roads:?}");
            assert_eq!(back.xref_references("SITE"), 1);
            assert!(
                back.layer("SITE|Walls").is_some_and(|l| !l.on && l.color == 5),
                "{version:?}: xref layer settings"
            );
        }
    }

    #[test]
    fn draw_order_survives() {
        let mut doc = Document::new();
        let line = |x: f64| Geometry::Line { start: Point::new(x, 0.0), end: Point::new(x, 1.0) };
        let first = doc.add(line(0.0));
        doc.add(line(1.0));
        doc.add(line(2.0));
        doc.bring_to_front(&[first]);
        let xs = |d: &Document| -> Vec<f64> {
            d.entities_in_draw_order()
                .iter()
                .map(|e| match e.geometry {
                    Geometry::Line { start, .. } => start.x,
                    _ => f64::NAN,
                })
                .collect()
        };
        for version in [Version::R2000, Version::R2018] {
            let back = read(&write(&doc, version).unwrap()).unwrap().imported.document;
            assert_eq!(xs(&back), vec![1.0, 2.0, 0.0], "{version:?}");
        }
    }

    #[test]
    fn xclip_boundaries_survive() {
        use oxidraft_model::{Block, BlockEntity, Color, Placement, XClip};
        let mut doc = Document::new();
        let line = Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) };
        doc.define_block(Block {
            name: "B".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByLayer, line)],
        });
        let square = vec![Point::new(2.0, -1.0), Point::new(6.0, -1.0), Point::new(6.0, 1.0), Point::new(2.0, 1.0)];
        let clips = [
            XClip { boundary: square.clone(), inverted: false, on: true },
            XClip { boundary: square.clone(), inverted: true, on: true },
            XClip { boundary: square, inverted: false, on: false },
        ];
        for (i, c) in clips.iter().enumerate() {
            doc.add(Geometry::Insert {
                block: "B".into(),
                placement: Placement {
                    position: Point::new(0.0, i as f64 * 5.0),
                    scale: 2.0,
                    rotation: 30.0,
                    mirror: false,
                },
                attributes: Vec::new(),
                clip: Some(Box::new(c.clone())),
            });
        }
        for version in [Version::R2000, Version::R2018] {
            let back = read(&write(&doc, version).unwrap()).unwrap().imported.document;
            let read: Vec<Option<XClip>> = back
                .entities_in_draw_order()
                .iter()
                .map(|e| match &e.geometry {
                    Geometry::Insert { clip, .. } => clip.as_deref().cloned(),
                    _ => None,
                })
                .collect();
            assert_eq!(read, clips.iter().cloned().map(Some).collect::<Vec<_>>(), "{version:?}");
        }
    }

    #[test]
    fn layouts_survive_with_their_objects_page_setups_and_viewports() {
        use oxidraft_model::{MODEL, Paper, layout::Layout};
        let mut doc = survey();
        doc.add_layer(Layer::new("Notes", 2));
        let a3 = Paper {
            size_name: "ISO_A3_(420.00_x_297.00_MM)".into(),
            width: 420.0,
            height: 297.0,
            margins: [7.5, 20.0, 7.5, 20.0],
            rotation: 0,
            millimeters: true,
            raw: Vec::new(),
        };
        let sheet = doc.add_layout(Layout { id: MODEL, name: "Sheet A3".into(), tab_order: 1, paper: a3.clone() });
        let plan = doc.add_layout(Layout { id: MODEL, name: "Plan 1-500".into(), tab_order: 2, paper: a3 });
        doc.set_space(sheet);
        doc.add(Geometry::Text {
            position: Point::new(300.0, 20.0),
            height: 5.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "ЧЕРТЕЖ".into(),
        });
        let frame = [(10.0, 10.0), (410.0, 10.0), (410.0, 287.0), (10.0, 287.0)];
        doc.add(Geometry::Polyline {
            vertices: frame.iter().map(|&(x, y)| Vertex::at(Point::new(x, y))).collect(),
            closed: true,
        });
        let viewport = |frozen: Vec<String>, clip: Vec<Point>| Geometry::Viewport {
            center: Point::new(200.0, 150.0),
            width: 300.0,
            height: 200.0,
            view_center: Point::new(50.0, 25.0),
            view_height: 100.0,
            twist: 30.0,
            frozen,
            locked: true,
            on: true,
            clip,
        };
        doc.add(viewport(Vec::new(), Vec::new()));
        doc.set_space(plan);
        doc.add(Geometry::Line { start: Point::new(20.0, 20.0), end: Point::new(100.0, 20.0) });
        // A triangle of a viewport, in its rectangle.
        let triangle = vec![Point::new(50.0, 50.0), Point::new(350.0, 50.0), Point::new(200.0, 250.0)];
        doc.add(viewport(vec!["Notes".into()], triangle.clone()));
        doc.set_space(MODEL);
        let model = doc.len();

        let check = |back: &Document, how: &str| {
            let names: Vec<&str> = back.layouts().iter().map(|l| l.name.as_str()).collect();
            assert_eq!(names, ["Sheet A3", "Plan 1-500"], "{how}");
            let [first, second] = [back.layouts()[0].id, back.layouts()[1].id];
            let kinds = |space| {
                let mut k: Vec<&str> =
                    back.all_entities().filter(|e| e.space == space).map(|e| e.geometry.name()).collect();
                k.sort();
                k
            };
            assert_eq!(kinds(first), ["MText", "Polyline", "Viewport"], "{how}");
            assert_eq!(kinds(second), ["Line", "Viewport"], "{how}");
            assert_eq!(back.len(), model, "model space is as it was ({how})");
            let paper = &back.layouts()[0].paper;
            assert_eq!(
                (paper.width, paper.height, paper.size_name.as_str()),
                (420.0, 297.0, "ISO_A3_(420.00_x_297.00_MM)"),
                "{how}"
            );
            let vp = back
                .all_entities()
                .find(|e| e.space == second && matches!(e.geometry, Geometry::Viewport { .. }))
                .unwrap();
            let Geometry::Viewport { center, width, view_center, view_height, frozen, locked, twist, clip, .. } =
                &vp.geometry
            else {
                panic!()
            };
            // Its boundary, and twist, came back; the boundary isn't an object of its own.
            assert_eq!(clip.len(), 3, "{how}");
            assert!(clip.iter().zip(&triangle).all(|(a, b)| a.approx_eq(*b)), "{how}");
            assert!((twist - 30.0).abs() < 1e-9, "{how}");
            assert!(center.approx_eq(Point::new(200.0, 150.0)) && (*width - 300.0).abs() < 1e-9, "{how}");
            assert!(view_center.approx_eq(Point::new(50.0, 25.0)) && (*view_height - 100.0).abs() < 1e-9, "{how}");
            assert_eq!((frozen.as_slice(), *locked), (["Notes".to_string()].as_slice(), true), "{how}");
        };
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
        let dxf = write_dxf(&doc).unwrap();
        check(&oxidraft_io_dxf::read(&mut dxf.as_slice()).unwrap().document, "DXF");
    }

    #[test]
    fn wipeouts_survive() {
        let mut doc = Document::new();
        // Not a rectangle: its bounds alone wouldn't do.
        let square = vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(4.0, 10.0)];
        doc.add(Geometry::Wipeout { points: square.clone() });
        for version in [Version::R2018, Version::R2000] {
            let back = read(&write(&doc, version).unwrap()).unwrap().imported.document;
            let got = back.entities().find_map(|e| match &e.geometry {
                Geometry::Wipeout { points } => Some(points.clone()),
                _ => None,
            });
            let got = got.unwrap_or_else(|| panic!("no wipeout ({version:?})"));
            assert!(
                got.len() == 3 && got.iter().zip(&square).all(|(a, b)| a.distance(*b) < 1e-6),
                "{version:?}: {got:?}"
            );
        }
        let dxf = write_dxf(&doc).unwrap();
        let back = oxidraft_io_dxf::read(&mut dxf.as_slice()).unwrap().document;
        assert!(
            back.entities().any(|e| matches!(&e.geometry, Geometry::Wipeout { points } if points.len() == 3)),
            "via acadrust's DXF"
        );
    }

    #[test]
    fn rejects_what_isnt_dwg() {
        assert!(read(b"0\nSECTION\n").is_err());
        assert!(is_dwg("plan.DWG") && !is_dwg("plan.dxf"));
    }
}

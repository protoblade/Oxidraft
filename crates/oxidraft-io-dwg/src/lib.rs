//! DWG, AutoCAD's native format. [acadrust] reads and writes the binary
//! file; the drawing passes through DXF in memory on the way, so whatever
//! Oxidraft reads and writes in DXF works the same in DWG.

use std::io::Cursor;
use std::path::Path;

use acadrust::{CadDocument, DwgReadOptions, DwgReader, DwgWriter, DxfReader, DxfWriter, types::DxfVersion};
use oxidraft_io_dxf::Imported;
use oxidraft_model::Document;

mod fields;
mod groups;
mod layouts;
mod settings;

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
    // Oxidraft keeps no other extended data than hyperlinks and what it
    // adds to dimensions, and some trips up the DXF reader.
    for e in cad.entities_mut() {
        let dimension = matches!(e, acadrust::EntityType::Dimension(_));
        let data = &mut e.common_mut().extended_data;
        let kept: Vec<_> = data
            .records()
            .iter()
            .filter(|r| {
                r.application_name == "PE_URL"
                    || dimension && ["ACAD", "OXIDRAFT_DIM"].contains(&r.application_name.as_str())
            })
            .cloned()
            .collect();
        *data = Default::default();
        for r in kept {
            data.add_record(r);
        }
    }
    let dxf = DxfWriter::new(&cad).with_version(DxfVersion::AC1032).write_to_vec().map_err(|e| e.to_string())?;
    let mut imported = oxidraft_io_dxf::read(&mut dxf.as_slice()).map_err(|e| e.to_string())?;
    fields::restore(&cad, &mut imported);
    Ok(Opened { imported, version })
}

/// A DXF file, its texts' AutoCAD fields brought back as field codes.
pub fn read_dxf(bytes: &[u8]) -> Result<Imported, Error> {
    let mut imported = oxidraft_io_dxf::read(&mut &bytes[..]).map_err(|e| e.to_string())?;
    if bytes.windows(10).any(|w| w == b"ACAD_FIELD")
        && let Ok(cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        fields::restore(&cad, &mut imported);
    }
    Ok(imported)
}

pub fn load_dxf(path: impl AsRef<Path>) -> Result<Imported, Error> {
    read_dxf(&std::fs::read(path).map_err(|e| e.to_string())?)
}

/// The drawing as acadrust holds it, layouts and all, ready to write as
/// `version` (fields are AutoCAD 2004's: older versions get their values).
fn cad(doc: &Document, version: DxfVersion) -> Result<CadDocument, Error> {
    let mut dxf = Vec::new();
    let handles = oxidraft_io_dxf::write_with_handles(doc, &mut dxf).map_err(|e| e.to_string())?;
    let mut cad = DxfReader::from_reader(Cursor::new(dxf)).and_then(|r| r.read()).map_err(|e| e.to_string())?;
    layouts::finish(&mut cad, doc);
    groups::finish(&mut cad, doc, &handles);
    settings::finish(&mut cad, doc);
    cad.version = version;
    if version >= DxfVersion::AC1018 {
        fields::finish(&mut cad, doc, &handles);
    } else {
        fields::flatten(&mut cad, doc, &handles);
    }
    // DWG keeps CELTYPE as its linetype's handle; acadrust reads only the name.
    if let Some(lt) = cad.line_types.get(&cad.header.current_linetype_name) {
        cad.header.current_linetype_handle = lt.handle;
    }
    Ok(cad)
}

/// The drawing as a DWG file of `version`.
pub fn write(doc: &Document, version: Version) -> Result<Vec<u8>, Error> {
    let cad = cad(doc, version.code())?;
    let mut out = Cursor::new(Vec::new());
    DwgWriter::write_to_writer(&mut out, &cad).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// The drawing as a DXF file: AutoCAD 2000, or 2010 with PDF underlays
/// (2007 is the first to have them). Written by acadrust, which also
/// writes the layouts' page setups and viewports.
pub fn write_dxf(doc: &Document) -> Result<Vec<u8>, Error> {
    let newer = oxidraft_io_dxf::needs_newer_version(doc);
    let version = if newer { DxfVersion::AC1024 } else { DxfVersion::AC1015 };
    let cad = cad(doc, version)?;
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
            look: Default::default(),
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
            look: Default::default(),
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
    fn groups_survive() {
        let mut doc = Document::new();
        let a = doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let b = doc.add(Geometry::Line { start: Point::new(0.0, 5.0), end: Point::new(10.0, 5.0) });
        let c = doc.add(Geometry::Circle { center: Point::new(20.0, 0.0), radius: 2.0 });
        doc.add(Geometry::Circle { center: Point::new(40.0, 0.0), radius: 2.0 });
        doc.add_group("Walls", "the outer walls", vec![a, b]).unwrap();
        doc.add_group("", "", vec![c]).unwrap();
        let check = |back: &Document, how: &str| {
            let mut groups = back.groups();
            groups.sort_by(|x, y| x.name.cmp(&y.name));
            let summary: Vec<(String, usize)> = groups.iter().map(|g| (g.name.clone(), g.members.len())).collect();
            assert_eq!(summary.len(), 2, "{how}: {summary:?}");
            assert!(summary.contains(&("Walls".into(), 2)), "{how}: {summary:?}");
            assert!(summary.iter().any(|(n, m)| n.starts_with('*') && *m == 1), "{how}: {summary:?}");
            let walls = groups.iter().find(|g| g.name == "Walls").unwrap();
            assert_eq!(walls.description, "the outer walls", "{how}");
        };
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
        let dxf = write_dxf(&doc).unwrap();
        check(&oxidraft_io_dxf::read(&mut dxf.as_slice()).unwrap().document, "DXF");
    }

    #[test]
    fn current_properties_survive() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let current = oxidraft_model::CurrentProps {
            color: oxidraft_model::Color::Index(3),
            linetype: "HIDDEN".into(),
            lineweight: oxidraft_model::Lineweight::Hundredths(35),
            transparency: oxidraft_model::Transparency::ByLayer,
        };
        doc.set_current_props(current.clone());
        let dxf = oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document;
        assert_eq!(dxf.current_props(), &current, "DXF");
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        let back = oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document;
        assert_eq!(back.current_props(), &current, "Oxidraft's own DXF");
        for version in [Version::R2018, Version::R2000] {
            let back = read(&write(&doc, version).unwrap()).unwrap().imported.document;
            assert_eq!(back.current_props(), &current, "{version:?}");
        }
        // A linetype used only on a layout, and one loaded but unused, are kept too.
        let mut doc = Document::new();
        let sheet = doc.new_layout("Layout1").unwrap();
        doc.set_space(sheet);
        let line = doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        doc.update(line, |e| e.linetype = "CENTER".into());
        doc.set_space(oxidraft_model::MODEL);
        doc.load_linetype(oxidraft_model::Linetype::new("MINE", "Mine", &[2.0, -1.0]));
        for version in [Version::R2018, Version::R2000] {
            let back = read(&write(&doc, version).unwrap()).unwrap().imported.document;
            let own = back.own_linetypes();
            assert!(own.iter().any(|n| n == "CENTER") && own.iter().any(|n| n == "MINE"), "{version:?}: {own:?}");
        }
        // Unset: ByLayer.
        let plain = read(&write(&Document::new(), Version::R2018).unwrap()).unwrap().imported.document;
        assert_eq!(plain.current_props(), &oxidraft_model::CurrentProps::by_layer());
    }

    #[test]
    fn text_styles_survive() {
        use oxidraft_model::{Attach, TextLook, TextStyle};
        let mut doc = Document::new();
        let narrow = TextStyle {
            name: "Narrow".into(),
            font: "romans.shx".into(),
            height: 0.0,
            width_factor: 0.7,
            oblique: 15.0,
        };
        doc.set_text_style(narrow.clone()).unwrap();
        doc.set_current_text_style("Narrow").unwrap();
        let text = |content: &str, attach| Geometry::Text {
            position: Point::new(0.0, 0.0),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach,
            content: content.into(),
            look: TextLook::of(&narrow),
        };
        doc.add(text("ROOM", Attach::BottomLeft));
        doc.add(text("Two\\Plines", Attach::MiddleCenter));
        let check = |back: &Document, how: &str| {
            assert_eq!(back.text_style("narrow"), Some(&narrow), "{how}");
            assert!(back.text_style("Standard").is_some(), "{how}");
            assert_eq!(back.current_text_style().name, "Narrow", "{how}");
            for e in back.entities() {
                let Geometry::Text { look, .. } = &e.geometry else { continue };
                assert_eq!(look, &TextLook::of(&narrow), "{how}");
            }
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn edited_dimensions_survive() {
        use oxidraft_model::annotation::DimExtra;
        let mut doc = Document::new();
        let extra = DimExtra {
            text_at: Some(Point::new(30.0, 10.0)),
            text_angle: Some(15.0),
            oblique: Some(60.0),
            ..Default::default()
        };
        doc.add(Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(40.0, 0.0),
            line: Point::new(30.0, 10.0),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Box::new(extra.clone()),
        });
        let check = |back: &Document, how: &str| {
            let got = back.entities().find_map(|e| match &e.geometry {
                Geometry::Dimension { extra, .. } => Some((**extra).clone()),
                _ => None,
            });
            let got = got.unwrap_or_else(|| panic!("no dimension ({how})"));
            assert_eq!(got.text_angle.map(|a| (a * 1e6).round() / 1e6), Some(15.0), "{how}");
            assert_eq!(got.oblique.map(|a| (a * 1e6).round() / 1e6), Some(60.0), "{how}");
            let at = got.text_at.unwrap_or_else(|| panic!("text not where it was put ({how})"));
            assert!((at.x - 30.0).abs() < 1e-6, "{how}: {at:?}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn tolerances_survive() {
        let mut doc = Document::new();
        let content = "{\\Fgdt;j}%%v{\\Fgdt;n}0.05{\\Fgdt;m}%%vA%%vB";
        doc.add(Geometry::Tolerance {
            position: Point::new(10.0, 5.0),
            height: 2.5,
            rotation: 30.0,
            content: content.into(),
        });
        let check = |back: &Document, how: &str| {
            let got = back.entities().find_map(|e| match &e.geometry {
                Geometry::Tolerance { position, rotation, content, .. } => {
                    Some((*position, *rotation, content.clone()))
                }
                _ => None,
            });
            let (p, r, c) = got.unwrap_or_else(|| panic!("no tolerance ({how})"));
            assert!(p.approx_eq(Point::new(10.0, 5.0)) && (r - 30.0).abs() < 1e-6, "{how}: {p:?} {r}");
            assert_eq!(c, content, "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn leader_and_table_styles_survive() {
        use oxidraft_model::annotation::{LeaderArrow, LeaderStyle};
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let leader = LeaderStyle {
            arrowhead: LeaderArrow::Dot,
            arrow_size: 4.0,
            text_height: 3.5,
            landing_on: false,
            landing: 6.0,
        };
        let table = oxidraft_model::table::TableStyle { text_height: 3.0, margin: 1.0 };
        doc.set_annotation_styles(leader, table);
        let check = |back: &Document, how: &str| {
            assert_eq!((back.leader_style(), back.table_style()), (leader, table), "{how}");
        };
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn gradients_survive() {
        use oxidraft_model::{HatchPattern, hatch::GradientKind};
        let mut doc = Document::new();
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .map(|(x, y)| oxidraft_model::Vertex::at(Point::new(x, y)));
        let pattern = HatchPattern::Gradient { kind: GradientKind::Spherical, from: 1, to: 3 };
        doc.add(Geometry::Hatch { loops: vec![square.to_vec()], pattern, scale: 1.0, angle: 30.0 });
        let check = |back: &Document, how: &str| {
            let got = back.entities().find_map(|e| match &e.geometry {
                Geometry::Hatch { pattern, angle, .. } => Some((*pattern, *angle)),
                _ => None,
            });
            let (p, a) = got.unwrap_or_else(|| panic!("no hatch ({how})"));
            assert_eq!(p, pattern, "{how}");
            assert!((a - 30.0).abs() < 1e-6, "{how}: {a}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        check(&read(&write(&doc, Version::R2018).unwrap()).unwrap().imported.document, "R2018");
        // AutoCAD 2000's DWG predates gradients: a solid fill there.
        let old = read(&write(&doc, Version::R2000).unwrap()).unwrap().imported.document;
        assert!(old.entities().any(|e| matches!(e.geometry, Geometry::Hatch { pattern: HatchPattern::Solid, .. })));
    }

    #[test]
    fn transparency_survives() {
        use oxidraft_model::{Color, Layer, Lineweight, Transparency};
        let mut doc = Document::new();
        doc.add_layer(Layer { transparency: 30, ..Layer::new("Glass", 4) });
        let line = Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) };
        doc.add_with(line.clone(), "Glass", Color::ByLayer, "ByLayer", Lineweight::ByLayer, Transparency::Percent(40));
        doc.add_with(line, "0", Color::ByLayer, "ByLayer", Lineweight::ByLayer, Transparency::ByBlock);
        let check = |back: &Document, how: &str| {
            let mut got: Vec<Transparency> = back.entities().map(|e| e.transparency).collect();
            got.sort_by_key(|t| format!("{t:?}"));
            assert_eq!(got, [Transparency::ByBlock, Transparency::Percent(40)], "{how}");
            assert_eq!(back.layer("Glass").map(|l| l.transparency), Some(30), "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        check(&read(&write(&doc, Version::R2018).unwrap()).unwrap().imported.document, "R2018");
    }

    #[test]
    fn layer_states_survive() {
        use oxidraft_model::Layer;
        let mut doc = Document::new();
        doc.add_layer(Layer { linetype: "DASHED".into(), transparency: 20, ..Layer::new("Walls", 1) });
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        doc.save_layer_state("All on", "everything").unwrap();
        let mut walls = doc.layer("Walls").cloned().unwrap();
        walls.on = false;
        walls.color = 3;
        doc.update_layer(walls);
        doc.save_layer_state("Walls off", "").unwrap();
        let check = |back: &Document, how: &str| {
            assert_eq!(back.layer_states(), doc.layer_states(), "{how}");
        };
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
        // Restoring one puts the layers back.
        let mut back = read(&write(&doc, Version::R2018).unwrap()).unwrap().imported.document;
        back.restore_layer_state("All on").unwrap();
        let w = back.layer("Walls").unwrap();
        assert!(w.on && w.color == 1);
    }

    #[test]
    fn drawing_properties_survive() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let props = oxidraft_model::DrawingProperties {
            title: "Ground floor".into(),
            subject: "Plan".into(),
            author: "Studio".into(),
            keywords: "house, plan".into(),
            comments: "First issue".into(),
        };
        doc.set_properties(props.clone());
        let check = |back: &Document, how: &str| assert_eq!(back.properties(), &props, "{how}");
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn fields_survive_as_autocads() {
        use oxidraft_model::field;
        field::set_env(field::FieldEnv {
            now: field::Moment { year: 2026, month: 10, day: 29, ..Default::default() },
            ..Default::default()
        });
        let mut doc = Document::new();
        let circle = doc.add(Geometry::Circle { center: Point::new(0.0, 0.0), radius: 2.0 });
        for width in [0.0, 40.0] {
            let content = format!(
                r#"Issued %<\AcVar Date \f "yyyy-MM-dd">%, area {}"#,
                field::object_code(circle, "Area", "%lu2%pr2")
            );
            doc.add(Geometry::Text {
                position: Point::new(0.0, width),
                height: 2.5,
                rotation: 0.0,
                width,
                attach: Default::default(),
                content,
                look: Default::default(),
            });
        }
        let shown = "Issued 2026-10-29, area 12.57";
        let check = |back: &Document, how: &str, kept: bool| {
            let texts: Vec<&String> = back
                .entities()
                .filter_map(|e| match &e.geometry {
                    Geometry::Text { content, .. } => Some(content),
                    _ => None,
                })
                .collect();
            assert_eq!(texts.len(), 2, "{how}");
            for t in texts {
                assert_eq!(field::has_fields(t), kept, "{how}: {t}");
                assert_eq!(field::evaluate(t, back), shown, "{how}");
            }
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF", true);
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF", true);
        check(&read(&write(&doc, Version::R2018).unwrap()).unwrap().imported.document, "R2018", true);
        check(&read(&write(&doc, Version::R2000).unwrap()).unwrap().imported.document, "R2000", false);
    }

    #[test]
    fn dimension_extras_survive() {
        use oxidraft_model::annotation::{DimExtra, DimStyle, Inspection, InspectionShape};
        let extra = DimExtra {
            breaks: vec![(Point::new(10.0, 10.0), Point::new(13.0, 10.0))],
            jog: Some(Point::new(30.0, 10.0)),
            style: Some(DimStyle { text_height: 5.0, decimals: 1, ..Default::default() }),
            inspection: Some(Inspection { shape: InspectionShape::Angular, label: "Key".into(), rate: "25%".into() }),
            ..Default::default()
        };
        let mut doc = Document::new();
        doc.add(Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(40.0, 0.0),
            line: Point::new(0.0, 10.0),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Box::new(extra.clone()),
        });
        let check = |back: &Document, how: &str| {
            let got = back.entities().find_map(|e| match &e.geometry {
                Geometry::Dimension { extra, .. } => Some((**extra).clone()),
                _ => None,
            });
            assert_eq!(got.as_ref(), Some(&extra), "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn picture_adjustments_and_frames_survive() {
        let mut doc = Document::new();
        doc.set_image_def(oxidraft_model::ImageDef {
            name: "ortho".into(),
            path: "ortho.png".into(),
            page: None,
            width: 400.0,
            height: 200.0,
            status: oxidraft_model::XrefStatus::Loaded,
        });
        let adjust = oxidraft_model::PictureAdjust { brightness: 70, contrast: 40, fade: 20, monochrome: false };
        doc.add(Geometry::Image {
            image: "ortho".into(),
            origin: Point::new(0.0, 0.0),
            u: Point::new(40.0, 0.0),
            v: Point::new(0.0, 20.0),
            clip: Vec::new(),
            adjust,
        });
        doc.set_picture_frame(false, 0);
        doc.set_picture_frame(true, 2);
        doc.set_wipeout_frame(2);
        let check = |back: &Document, how: &str| {
            assert!(
                matches!(&back.entities().next().unwrap().geometry, Geometry::Image { adjust: a, .. } if *a == adjust),
                "{how}"
            );
            let frames = (back.picture_frame(false), back.picture_frame(true), back.wipeout_frame());
            assert_eq!(frames, (0, 2, 2), "{how}");
        };
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn ucs_limits_and_modes_survive() {
        use oxidraft_model::ucs::Ucs;
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let ucs = Ucs { origin: Point::new(100.0, 50.0), angle: 30.0 };
        doc.set_ucs(ucs);
        doc.save_ucs("Site", Ucs { origin: Point::new(5.0, 5.0), angle: -45.0 }).unwrap();
        doc.set_limits(Point::new(-10.0, -10.0), Point::new(841.0, 594.0));
        doc.set_limits_check(true);
        doc.set_fill(false);
        doc.set_quick_text(true);
        let check = |back: &Document, how: &str| {
            let (u, named) = (back.ucs(), back.named_ucs());
            assert!(u.origin.approx_eq(ucs.origin) && (u.angle - 30.0).abs() < 1e-9, "{how}: {u:?}");
            assert_eq!(named.len(), 1, "{how}");
            assert!(named[0].name == "Site" && (named[0].ucs.angle + 45.0).abs() < 1e-9, "{how}: {named:?}");
            assert_eq!(back.limits(), (Point::new(-10.0, -10.0), Point::new(841.0, 594.0)), "{how}");
            assert_eq!((back.limits_check(), back.fill(), back.quick_text()), (true, false, true), "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn hyperlinks_survive() {
        let mut doc = Document::new();
        let id = doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let link = oxidraft_model::Hyperlink { url: "https://example.com/a".into(), description: "Spec".into() };
        doc.update(id, |e| e.hyperlink = Some(Box::new(link.clone())));
        let check = |back: &Document, how: &str| {
            let got = back.entities().next().and_then(|e| e.hyperlink.clone());
            assert_eq!(got.as_deref(), Some(&link), "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn units_survive() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        let units = oxidraft_model::Units {
            length: oxidraft_model::LengthType::Architectural,
            length_precision: 5,
            angle: oxidraft_model::AngleType::Surveyor,
            angle_precision: 2,
            clockwise: true,
            base_angle: 90.0,
            insertion: 1,
        };
        doc.set_units(units);
        let dxf = oxidraft_io_dxf::read(&mut write_dxf(&doc).unwrap().as_slice()).unwrap().document;
        assert_eq!(dxf.units(), units, "DXF");
        for version in [Version::R2018, Version::R2000] {
            let back = read(&write(&doc, version).unwrap()).unwrap().imported.document;
            assert_eq!(back.units(), units, "{version:?}");
        }
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

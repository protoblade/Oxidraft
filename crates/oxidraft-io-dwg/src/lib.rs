//! DWG, AutoCAD's native format. [acadrust] reads and writes the binary
//! file; the drawing passes through DXF in memory on the way, so whatever
//! Oxidraft reads and writes in DXF works the same in DWG.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use acadrust::{CadDocument, DwgReadOptions, DwgReader, DwgWriter, DxfReader, DxfWriter, types::DxfVersion};
use oxidraft_io_dxf::Imported;
use oxidraft_model::Document;

/// AutoCAD's sheet set files (.dst), as acadrust reads and writes them.
pub use acadrust::sheet_set;

mod annotative;
mod dimassoc;
mod dimbreaks;
mod dynblocks;
mod fields;
mod groups;
mod layouts;
mod mleaders;
mod ole;
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
    // Autosaves (.sv$) are DWG data too.
    path.as_ref().extension().is_some_and(|e| ["dwg", "sv$"].iter().any(|x| e.eq_ignore_ascii_case(x)))
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
    keep_known_xdata(&mut cad);
    let imported = imported_from(&mut cad)?;
    Ok(Opened { imported, version })
}

/// Oxidraft keeps no other extended data than hyperlinks and what it adds
/// to its objects and dimensions, and some trips up the DXF reader.
fn keep_known_xdata(cad: &mut CadDocument) {
    for e in cad.entities_mut() {
        let dimension = matches!(e, acadrust::EntityType::Dimension(_));
        let data = &mut e.common_mut().extended_data;
        let kept: Vec<_> = data
            .records()
            .iter()
            .filter(|r| {
                [
                    "PE_URL",
                    "OXIDRAFT_ANNO",
                    "OXIDRAFT_ARRAY",
                    "OXIDRAFT_MLEADER",
                    "OXIDRAFT_PART",
                    "OXIDRAFT_TABLE",
                    "OXIDRAFT_DYN",
                ]
                .contains(&r.application_name.as_str())
                    || dimension
                        && ["ACAD", "OXIDRAFT_DIM", "ACAD_DSTYLE_DIMJAG_POSITION"]
                            .contains(&r.application_name.as_str())
            })
            .cloned()
            .collect();
        *data = Default::default();
        for r in kept {
            data.add_record(r);
        }
    }
}

/// What acadrust read, through DXF in memory: multileaders made LEADERs
/// the DXF reader knows, fields and associations brought back after.
fn imported_from(cad: &mut CadDocument) -> Result<Imported, Error> {
    mleaders::unpack(cad);
    let dxf = DxfWriter::new(cad).with_version(DxfVersion::AC1032).write_to_vec().map_err(|e| e.to_string())?;
    let mut imported = oxidraft_io_dxf::read(&mut dxf.as_slice()).map_err(|e| e.to_string())?;
    fields::restore(cad, &mut imported);
    dimassoc::restore(cad, &mut imported);
    annotative_styles(cad, &mut imported.document);
    mtext_layout(cad, &mut imported);
    dynblocks::import(cad, &mut imported);
    annotative::restore(cad, &mut imported);
    dimbreaks::restore(cad, &mut imported);
    ole::restore(cad, &mut imported);
    Ok(imported)
}

/// MTEXT's columns and line spacing as acadrust reads them (AutoCAD keeps
/// columns in an embedded object the DXF reader doesn't know).
fn mtext_layout(cad: &CadDocument, imported: &mut Imported) {
    for e in cad.entities() {
        let acadrust::EntityType::MText(m) = e else { continue };
        let Some(&id) = imported.handles.get(&e.common().handle.value()) else { continue };
        let c = &m.column_data;
        let columns = (c.column_type != 0 && c.column_count > 1).then(|| oxidraft_model::annotation::Columns {
            count: c.column_count as usize,
            width: if c.width > 0.0 { c.width } else { m.rectangle_width },
            gutter: c.gutter.max(0.0),
            heights: c.heights.clone(),
        });
        let spacing = if m.line_spacing_factor > 0.0 { m.line_spacing_factor } else { 1.0 };
        imported.document.update(id, |e| {
            if let oxidraft_model::Geometry::Text { look, .. } = &mut e.geometry {
                look.columns = columns;
                look.line_spacing = spacing;
            }
        });
    }
}

/// Text styles AutoCAD marks annotative are so here.
fn annotative_styles(cad: &CadDocument, doc: &mut Document) {
    for t in cad.text_styles.iter().filter(|t| t.annotative) {
        if let Some(mut s) = doc.text_styles().iter().find(|s| s.name.eq_ignore_ascii_case(&t.name)).cloned() {
            s.annotative = true;
            let _ = doc.set_text_style(s);
        }
    }
}

/// A DXF file, its texts' AutoCAD fields brought back as field codes. One
/// with multileaders, which the DXF reader doesn't know, is read by
/// acadrust, as a DWG is.
pub fn read_dxf(bytes: &[u8]) -> Result<Imported, Error> {
    if bytes.windows(11).any(|w| w == b"MULTILEADER")
        && let Ok(mut cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        keep_known_xdata(&mut cad);
        return imported_from(&mut cad);
    }
    let mut imported = oxidraft_io_dxf::read(&mut &bytes[..]).map_err(|e| e.to_string())?;
    if bytes.windows(10).any(|w| w == b"ACAD_FIELD")
        && let Ok(cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        fields::restore(&cad, &mut imported);
    }
    if bytes.windows(13).any(|w| w == b"ACAD_DIMASSOC")
        && let Ok(cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        dimassoc::restore(&cad, &mut imported);
    }
    if bytes.windows(15).any(|w| w == b"Embedded Object")
        && let Ok(cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        mtext_layout(&cad, &mut imported);
    }
    if bytes.windows(18).any(|w| w == b"ACAD_ENHANCEDBLOCK")
        && let Ok(cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        dynblocks::import(&cad, &mut imported);
    }
    if bytes.windows(9).any(|w| w == b"OLE2FRAME")
        && let Ok(cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        ole::restore(&cad, &mut imported);
    }
    if bytes.windows(9).any(|w| w == b"BREAKDATA")
        && let Ok(cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        dimbreaks::restore(&cad, &mut imported);
    }
    if bytes.windows(21).any(|w| w == b"ACDB_ANNOTATIONSCALES")
        && let Ok(cad) = DxfReader::from_reader(Cursor::new(bytes.to_vec())).and_then(|r| r.read())
    {
        annotative::restore(&cad, &mut imported);
    }
    Ok(imported)
}

pub fn load_dxf(path: impl AsRef<Path>) -> Result<Imported, Error> {
    read_dxf(&std::fs::read(path).map_err(|e| e.to_string())?)
}

/// The drawing as acadrust holds it, layouts and all, ready to write as
/// `version` (fields are AutoCAD 2004's: older versions get their values).
/// The handles in DXF text, each with the kind of object it's on.
fn written_handles(dxf: &[u8]) -> HashMap<u64, String> {
    let text = String::from_utf8_lossy(dxf);
    let mut lines = text.lines();
    let (mut kind, mut out) = (String::new(), HashMap::new());
    while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
        match code.trim() {
            "0" => kind = value.trim().to_string(),
            "5" => {
                if let Ok(h) = u64::from_str_radix(value.trim(), 16) {
                    out.insert(h, kind.clone());
                }
            }
            _ => {}
        }
    }
    out
}

fn cad(doc: &Document, version: DxfVersion) -> Result<CadDocument, Error> {
    let mut dxf = Vec::new();
    let handles = oxidraft_io_dxf::write_with_handles(doc, &mut dxf).map_err(|e| e.to_string())?;
    let written = written_handles(&dxf);
    let mut cad = DxfReader::from_reader(Cursor::new(dxf)).and_then(|r| r.read()).map_err(|e| e.to_string())?;
    // Handles given out from here on are above every one written (a
    // SEQEND's too, which acadrust doesn't count); and the applications
    // acadrust starts with, when the file has none of them, don't keep a
    // handle the file gives something else.
    let highest = written.keys().copied().max().unwrap_or(0);
    cad.header.handle_seed = cad.header.handle_seed.max(highest + 1);
    let clashing: Vec<String> = cad
        .app_ids
        .iter()
        .filter(|a| written.get(&a.handle.value()).is_some_and(|kind| kind != "APPID"))
        .map(|a| a.name.clone())
        .collect();
    for name in clashing {
        let h = cad.allocate_handle();
        if let Some(a) = cad.app_ids.get_mut(&name) {
            a.handle = h;
        }
    }
    layouts::finish(&mut cad, doc);
    groups::finish(&mut cad, doc, &handles);
    settings::finish(&mut cad, doc);
    // MTEXT columns, as acadrust keeps them.
    for e in doc.all_entities() {
        let oxidraft_model::Geometry::Text { look, .. } = &e.geometry else { continue };
        let (Some(c), Some(&h)) = (&look.columns, handles.get(&e.id).and_then(|h| h.first())) else { continue };
        if let Some(acadrust::EntityType::MText(m)) = cad.get_entity_mut(acadrust::types::Handle::new(h)) {
            // Dynamic columns: their heights set, or found automatically.
            m.column_data = acadrust::entities::MTextColumnData {
                column_type: 2,
                column_count: c.count as i32,
                flow_reversed: false,
                auto_height: c.heights.is_empty(),
                width: c.width,
                gutter: c.gutter,
                heights: c.heights.clone(),
            };
        }
    }
    // Annotative text styles as AutoCAD marks them.
    for s in doc.text_styles().iter().filter(|s| s.annotative) {
        if let Some(t) = cad.text_styles.iter_mut().find(|t| t.name.eq_ignore_ascii_case(&s.name)) {
            t.annotative = true;
        }
    }
    if version >= DxfVersion::AC1021 {
        mleaders::finish(&mut cad, doc, &handles);
    }
    cad.version = version;
    dimassoc::finish(&mut cad, doc, &handles);
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
    // Multileaders need AutoCAD 2007's MULTILEADER, MTEXT columns 2008's.
    let leaders = doc.all_entities().any(|e| match &e.geometry {
        oxidraft_model::Geometry::Leader { .. } => true,
        oxidraft_model::Geometry::Text { look, .. } => look.columns.is_some(),
        _ => false,
    });
    let newer = oxidraft_io_dxf::needs_newer_version(doc) || leaders;
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
            origin: Default::default(),
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
    fn mtext_columns_and_line_spacing_survive() {
        use oxidraft_model::annotation::{Columns, TextLook};
        let mut doc = Document::new();
        let look = TextLook {
            line_spacing: 1.5,
            columns: Some(Columns { count: 2, width: 40.0, gutter: 5.0, heights: vec![30.0, 30.0] }),
            ..Default::default()
        };
        doc.add(Geometry::Text {
            position: Point::new(0.0, 50.0),
            height: 2.5,
            rotation: 0.0,
            width: 40.0,
            attach: oxidraft_model::Attach::TopLeft,
            content: "Notes\\Pone\\Ptwo".into(),
            look: look.clone(),
        });
        let check = |back: &Document, how: &str| {
            let got = back.entities().find_map(|e| match &e.geometry {
                Geometry::Text { look, .. } => Some(look.clone()),
                _ => None,
            });
            let got = got.unwrap_or_default();
            assert_eq!((got.line_spacing, got.columns), (look.line_spacing, look.columns.clone()), "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF");
        check(&read(&write(&doc, Version::R2018).unwrap()).unwrap().imported.document, "DWG");
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
            big_font: "bigfont.shx".into(),
            annotative: true,
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
            // The bare DXF writer keeps no settings: whether it's annotative isn't there.
            let want = if how == "Oxidraft's DXF" {
                TextStyle { annotative: false, ..narrow.clone() }
            } else {
                narrow.clone()
            };
            assert_eq!(back.text_style("narrow"), Some(&want), "{how}");
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
            block: Some(oxidraft_model::annotation::TagShape::Hexagon),
            annotative: true,
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
        doc.add(Geometry::Hatch {
            origin: Default::default(),
            loops: vec![square.to_vec()],
            pattern,
            scale: 1.0,
            angle: 30.0,
        });
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
    fn embedded_files_travel_inside() {
        let mut doc = Document::new();
        let bytes: Vec<u8> = (0..400u32).map(|i| (i * 7 % 256) as u8).collect();
        let name = doc.embed_file("spec.txt", bytes.clone());
        doc.embed_file("unused.bin", vec![1, 2, 3]);
        let id = doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 0.0) });
        doc.update(id, |e| {
            e.hyperlink = Some(Box::new(oxidraft_model::Hyperlink {
                url: format!("{}{name}", oxidraft_io_dxf::EMBEDDED_PREFIX),
                description: "spec.txt".into(),
            }))
        });
        let check = |back: &Document, how: &str| {
            assert_eq!(back.embedded_file("spec.txt"), Some(bytes.as_slice()), "{how}");
            assert!(back.embedded_file("unused.bin").is_none(), "{how}: only what's linked to");
        };
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF");
        check(&read(&write(&doc, Version::R2018).unwrap()).unwrap().imported.document, "R2018");
        // The same contents again: the same name.
        assert_eq!(doc.embed_file("spec.txt", bytes), "spec.txt");
        assert_eq!(doc.embed_file("spec.txt", vec![9]), "spec (2).txt");
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
        let dxf = write_dxf(&doc).unwrap();
        check(&read_dxf(&dxf).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
        // AutoCAD's own jog data is there; a dimension with only that (as
        // AutoCAD writes them) comes in jogged.
        let mut cad = DxfReader::from_reader(Cursor::new(dxf)).and_then(|r| r.read()).unwrap();
        let mut found = false;
        for e in cad.entities_mut() {
            let data = &mut e.common_mut().extended_data;
            if let Some(r) = data.get_record("ACAD_DSTYLE_DIMJAG_POSITION") {
                let values: Vec<String> = r.values.iter().map(|v| format!("{v:?}")).collect();
                assert!(
                    values[0].contains("387") && values[2].contains("389") && values[3].contains("30"),
                    "{values:?}"
                );
                found = true;
                let jag = r.clone();
                *data = Default::default();
                data.add_record(jag);
            }
        }
        assert!(found, "ACAD_DSTYLE_DIMJAG_POSITION written");
        let autocad = DxfWriter::new(&cad).write_to_vec().unwrap();
        let back = read_dxf(&autocad).unwrap().document;
        let jog = back.entities().find_map(|e| match &e.geometry {
            Geometry::Dimension { extra, .. } => extra.jog,
            _ => None,
        });
        assert_eq!(jog, Some(Point::new(30.0, 10.0)));
    }

    /// A multileader as AutoCAD makes it, without Oxidraft's data: one
    /// multileader with its text, at the text's height.
    #[test]
    fn autocads_multileaders_come_in_whole() {
        use acadrust::entities::MultiLeader;
        use acadrust::types::Vector3;
        let mut cad = CadDocument::new();
        let mut m = MultiLeader::with_text(
            "SEE DETAIL",
            Vector3::new(12.0, 11.0, 0.0),
            vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 10.0, 0.0)],
        );
        m.context.text_height = 5.0;
        m.context.leader_roots[0].create_line(vec![Vector3::new(20.0, 0.0, 0.0), Vector3::new(10.0, 10.0, 0.0)]);
        cad.add_entity(acadrust::EntityType::MultiLeader(Box::new(m))).unwrap();
        let dxf = DxfWriter::new(&cad).with_version(DxfVersion::AC1024).write_to_vec().unwrap();
        let mut dwg = Cursor::new(Vec::new());
        cad.version = DxfVersion::AC1032;
        DwgWriter::write_to_writer(&mut dwg, &cad).unwrap();
        let dwg = dwg.into_inner();
        for (how, doc) in [("DXF", read_dxf(&dxf).unwrap().document), ("DWG", read(&dwg).unwrap().imported.document)] {
            let leaders: Vec<&Geometry> = doc.entities().map(|e| &e.geometry).collect();
            assert_eq!(leaders.len(), 1, "{how}: {leaders:?}");
            let Geometry::Leader { points, content, scale, more, .. } = leaders[0] else { panic!("{how}") };
            assert_eq!(content, "SEE DETAIL", "{how}");
            assert_eq!(points, &[Point::new(0.0, 0.0), Point::new(10.0, 10.0)], "{how}");
            assert_eq!(more, &[Point::new(20.0, 0.0)], "{how}: the second leader line");
            let height = oxidraft_model::annotation::DEFAULT_LEADER_STYLE.text_height * scale;
            assert!((height - 5.0).abs() < 1e-9, "{how}: {height}");
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
        let adjust = oxidraft_model::PictureAdjust {
            brightness: 70,
            contrast: 40,
            fade: 20,
            monochrome: false,
            transparent: true,
        };
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
    fn annotation_scales_survive() {
        let mut doc = Document::new();
        let text = doc.add(Geometry::Text {
            look: Default::default(),
            position: Point::new(0.0, 0.0),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Default::default(),
            content: "ROOM".into(),
        });
        doc.set_annotative(text, true);
        doc.set_anno_scale(50.0, true);
        doc.set_anno_all_visible(false);
        let check = |back: &Document, how: &str| {
            let e = back.entities().next().unwrap();
            let a = e.annotative.as_deref().unwrap_or_else(|| panic!("{how}: not annotative"));
            assert_eq!((a.shown, a.scales.clone()), (50.0, vec![1.0, 50.0]), "{how}");
            assert!(matches!(e.geometry, Geometry::Text { height, .. } if (height - 125.0).abs() < 1e-9), "{how}");
            assert_eq!((back.anno_scale(), back.anno_all_visible()), (50.0, false), "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        let back = oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document;
        assert!(back.entities().next().unwrap().annotative.is_some(), "Oxidraft's DXF");
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn associative_arrays_survive() {
        use oxidraft_model::assoc_array::ArrayLayout;
        let mut doc = Document::new();
        let c = doc.add(Geometry::Circle { center: Point::new(0.0, 0.0), radius: 1.0 });
        let l = doc.add(Geometry::Line { start: Point::new(0.0, 5.0), end: Point::new(3.0, 5.0) });
        let rect = ArrayLayout::Rect { rows: 2, columns: 3, row_spacing: 10.0, column_spacing: 20.0 };
        let grid = doc.make_array(&[c], rect.clone()).unwrap();
        // One item replaced by a square.
        let square = doc.add(Geometry::Polyline {
            vertices: [(90.0, 90.0), (92.0, 90.0), (92.0, 92.0)].map(|(x, y)| Vertex::at(Point::new(x, y))).to_vec(),
            closed: true,
        });
        assert!(doc.replace_array_items(grid, &[square], Point::new(90.0, 90.0), Some(4)));
        let arc = Geometry::Arc { center: Point::new(50.0, 50.0), radius: 10.0, start_angle: 0.0, end_angle: 180.0 };
        doc.make_array(
            &[l],
            ArrayLayout::Path { path: Box::new(arc), items: 5, align: true, spacing: Some(3.0), rows: (2, 4.0) },
        )
        .unwrap();
        // Two dimensions, whose blocks mustn't clash with the arrays'.
        for x in [0.0, 30.0] {
            doc.add(Geometry::Dimension {
                p1: Point::new(x, 0.0),
                p2: Point::new(x + 10.0, 0.0),
                line: Point::new(x, -10.0),
                rotation: 0.0,
                aligned: false,
                text: None,
                scale: 1.0,
                extra: Default::default(),
            });
        }
        let parts = |d: &Document| {
            let mut n: Vec<usize> = d
                .entities()
                .filter(|e| e.array.is_some())
                .map(|e| d.expanded(e).iter().filter(|g| !matches!(g, Geometry::Point { .. })).count())
                .collect();
            n.sort();
            n
        };
        let check = |back: &Document, how: &str| {
            let arrays: Vec<_> = back.entities().filter_map(|e| e.array.as_deref()).collect();
            assert_eq!(arrays.len(), 2, "{how}");
            assert!(arrays.iter().any(|a| a.layout == rect), "{how}");
            let grid = back.entities().find(|e| e.array.as_ref().is_some_and(|a| a.layout == rect)).unwrap();
            assert_eq!(grid.array.as_ref().unwrap().replaced.iter().map(|r| r.0).collect::<Vec<_>>(), [4], "{how}");
            assert_eq!(
                back.expanded(grid).iter().filter(|g| matches!(g, Geometry::Polyline { .. })).count(),
                1,
                "{how}: the square stands in for one item"
            );
            assert!(
                arrays.iter().any(|a| matches!(
                    a.layout,
                    ArrayLayout::Path { items: 5, align: true, spacing: Some(3.0), rows: (2, 4.0), .. }
                )),
                "{how}"
            );
            assert_eq!(parts(back), [6, 10], "{how}: five along the arc, two rows");
            // Still editable.
            let mut back = back.clone();
            let id = back.entities().find(|e| e.array.as_ref().is_some_and(|a| a.layout == rect)).unwrap().id;
            back.begin("ARRAYEDIT");
            assert!(back.set_array_layout(
                id,
                ArrayLayout::Rect { rows: 1, columns: 2, row_spacing: 1.0, column_spacing: 4.0 }
            ));
            back.commit();
            assert_eq!(parts(&back), [2, 10], "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        let back = oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document;
        check(&back, "Oxidraft's DXF");
        // Saved again: one block of each name.
        let mut again = Vec::new();
        oxidraft_io_dxf::write(&back, &mut again).unwrap();
        let text = String::from_utf8_lossy(&again);
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let (mut names, mut in_block) = (Vec::new(), false);
        for w in lines.windows(2) {
            if w[0] == "0" {
                in_block = w[1] == "BLOCK";
            } else if in_block && w[0] == "2" {
                names.push(w[1]);
                in_block = false;
            }
        }
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "{names:?}");
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn multileaders_survive_whole() {
        use oxidraft_model::annotation::{LeaderBlock, LeaderBlocks, TagShape};
        let mut doc = Document::new();
        let tag = doc.tag_block(TagShape::Hexagon);
        let items = ["7", "8", "9"].map(|v| LeaderBlock { name: tag.clone(), value: v.into() }).to_vec();
        let tags = Geometry::Leader {
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
            content: String::new(),
            scale: 2.0,
            more: vec![Point::new(20.0, 0.0)],
            blocks: Some(Box::new(LeaderBlocks { items, per_row: 2 })),
        };
        let note = Geometry::Leader {
            points: vec![Point::new(0.0, 50.0), Point::new(-10.0, 60.0)],
            content: "NOTE".into(),
            scale: 1.0,
            more: Vec::new(),
            blocks: None,
        };
        doc.add(tags.clone());
        doc.add(note.clone());
        let check = |back: &Document, how: &str| {
            let got: Vec<&Geometry> = back.entities().map(|e| &e.geometry).collect();
            assert_eq!(got.len(), 2, "{how}: {got:?}");
            assert!(got.contains(&&tags) && got.contains(&&note), "{how}: {got:?}");
            let e = back.entities().find(|e| e.geometry == tags).unwrap();
            let drawn = back.expanded(e);
            assert!(drawn.iter().any(|g| matches!(g, Geometry::Text { content, .. } if content == "9")), "{how}");
        };
        let mut ours = Vec::new();
        oxidraft_io_dxf::write(&doc, &mut ours).unwrap();
        check(&oxidraft_io_dxf::read(&mut ours.as_slice()).unwrap().document, "Oxidraft's DXF");
        let dxf = write_dxf(&doc).unwrap();
        check(&read_dxf(&dxf).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
        // AutoCAD sees MULTILEADERs, the parts gone.
        let kinds = |cad: &CadDocument| {
            let count = |f: fn(&acadrust::EntityType) -> bool| cad.entities().filter(|e| f(e)).count();
            (
                count(|e| matches!(e, acadrust::EntityType::MultiLeader(_))),
                count(|e| matches!(e, acadrust::EntityType::Leader(_) | acadrust::EntityType::MText(_))),
            )
        };
        // The gathered blocks: one anonymous block of the three references.
        let gathered = |cad: &CadDocument| {
            let content = cad.entities().find_map(|e| match e {
                acadrust::EntityType::MultiLeader(m) => m.block_content_handle,
                _ => None,
            });
            let record = cad.block_records.iter().find(|r| Some(r.handle) == content).unwrap();
            let refs = record
                .entity_handles
                .iter()
                .filter(|h| matches!(cad.get_entity(**h), Some(acadrust::EntityType::Insert(_))))
                .count();
            (record.name.starts_with("*U"), refs)
        };
        let cad = DxfReader::from_reader(Cursor::new(dxf.clone())).and_then(|r| r.read()).unwrap();
        assert_eq!(kinds(&cad), (2, 0), "DXF");
        assert_eq!(gathered(&cad), (true, 3), "DXF");
        let dwg = write(&doc, Version::R2018).unwrap();
        let cad = DwgReader::from_stream(Cursor::new(dwg.clone())).read().unwrap();
        assert_eq!(kinds(&cad), (2, 0), "DWG");
        assert_eq!(gathered(&cad), (true, 3), "DWG");
        // Read back, the anonymous block is gone.
        let back = read(&dwg).unwrap().imported.document;
        // Every handle once, the references' SEQENDs' too.
        let text = String::from_utf8_lossy(&dxf).into_owned();
        let lines: Vec<&str> = text.lines().collect();
        let mut seen = std::collections::HashSet::new();
        for pair in lines.chunks(2) {
            if let [code, value] = pair
                && code.trim() == "5"
            {
                assert!(seen.insert(value.trim().to_string()), "handle {value} twice");
            }
        }
        let names = |d: &Document| d.blocks().map(|b| b.name.clone()).collect::<Vec<_>>();
        assert!(names(&back).iter().all(|n| !n.starts_with("*U")), "{:?}", names(&back));
        assert!(names(&read_dxf(&dxf).unwrap().document).iter().all(|n| !n.starts_with("*U")));
    }

    #[test]
    fn tables_survive_whole_with_cell_formats_and_blocks() {
        use oxidraft_model::table::{CellFormat, new_table};
        let mut doc = Document::new();
        let tag = doc.tag_block(oxidraft_model::annotation::TagShape::Circle);
        let Geometry::Table { position, rotation, widths, heights, mut cells, text_height, .. } =
            new_table(Point::new(0.0, 100.0), 2, 2, 20.0, 10.0, 2.5)
        else {
            unreachable!()
        };
        cells[0][0] = "Doors".into();
        cells[2][0] = "D1, \"oak\"".into();
        let formats = vec![
            ((2, 0), CellFormat { attach: Some(oxidraft_model::Attach::MiddleLeft), height: Some(3.0), block: None }),
            ((2, 1), CellFormat { block: Some(tag), ..Default::default() }),
        ];
        let table = Geometry::Table { position, rotation, widths, heights, cells, text_height, formats };
        doc.add(table.clone());
        let check = |back: &Document, how: &str| {
            let got: Vec<&Geometry> = back.entities().map(|e| &e.geometry).collect();
            assert_eq!(got, [&table], "{how}");
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
    fn dynamic_blocks_and_their_references_survive() {
        use oxidraft_model::dynamic::{DynamicBlock, DynamicValues, Param, VisibilityState};
        use oxidraft_model::{Block, BlockEntity, Color, Placement};
        let mut doc = Document::new();
        let line = |a: (f64, f64), b: (f64, f64)| {
            BlockEntity::new(
                "0",
                Color::ByBlock,
                Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) },
            )
        };
        doc.define_block(Block {
            name: "DOOR".into(),
            base: Point::ORIGIN,
            entities: vec![line((0.0, 0.0), (900.0, 0.0)), line((0.0, 0.0), (0.0, 900.0))],
        });
        let def = DynamicBlock {
            params: vec![Param::Linear {
                name: "Width".into(),
                base: Point::ORIGIN,
                end: Point::new(900.0, 0.0),
                stretch: Some(oxidraft_geom::Aabb::from_points(Point::new(800.0, -10.0), Point::new(1000.0, 10.0))),
            }],
            states: vec![
                VisibilityState { name: "Open".into(), hidden: vec![] },
                VisibilityState { name: "Closed".into(), hidden: vec![1] },
            ],
            current_state: 0,
        };
        doc.set_dynamic_block("DOOR", Some(def.clone()));
        let id = doc.add(Geometry::Insert {
            block: "DOOR".into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: vec![],
            clip: None,
        });
        let values = DynamicValues {
            block: "DOOR".into(),
            values: vec![("Width".into(), 1200.0)],
            visibility: Some("Closed".into()),
        };
        doc.set_dynamic_values(id, values.clone());
        let check = |back: &Document, how: &str| {
            assert_eq!(back.dynamic_block("DOOR"), Some(&def), "{how}");
            let e = back.entities().find(|e| matches!(e.geometry, Geometry::Insert { .. })).unwrap();
            assert_eq!(e.dynamic.as_deref(), Some(&values), "{how}");
            let drawn = back.expanded(e);
            assert!(
                drawn.len() == 1
                    && matches!(drawn[0], Geometry::Line { end, .. } if end.approx_eq(Point::new(1200.0, 0.0))),
                "{how}: {drawn:?}"
            );
        };
        check(&read_dxf(&write_dxf(&doc).unwrap()).unwrap().document, "DXF");
        for version in [Version::R2018, Version::R2000] {
            check(&read(&write(&doc, version).unwrap()).unwrap().imported.document, &format!("{version:?}"));
        }
    }

    #[test]
    fn hatch_origins_survive() {
        let mut doc = Document::new();
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .map(|(x, y)| oxidraft_model::Vertex::at(Point::new(x, y)));
        doc.add(Geometry::Hatch {
            loops: vec![square.to_vec()],
            pattern: oxidraft_model::HatchPattern::Ansi31,
            scale: 1.0,
            angle: 0.0,
            origin: Point::new(3.0, 4.0),
        });
        let check = |back: &Document, how: &str| {
            let g = &back.entities().next().unwrap().geometry;
            assert!(
                matches!(g, Geometry::Hatch { origin, .. } if origin.approx_eq(Point::new(3.0, 4.0))),
                "{how}: {g:?}"
            );
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
    fn dimension_associations_survive_as_dimassoc() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(40.0, 0.0) });
        doc.add(Geometry::Circle { center: Point::new(80.0, 0.0), radius: 5.0 });
        let linear = doc.add(Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(40.0, 0.0),
            line: Point::new(0.0, 10.0),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Default::default(),
        });
        let radial = doc.add(Geometry::RadialDimension {
            center: Point::new(80.0, 0.0),
            point: Point::new(80.0, 5.0),
            text_at: Point::new(80.0, 9.0),
            diameter: false,
            text: None,
            scale: 1.0,
        });
        assert_eq!(doc.associate_dimension(linear), 2);
        assert_eq!(doc.associate_dimension(radial), 2);
        let check = |back: &Document, how: &str| {
            let n: usize = back.all_dim_assoc().map(|(_, r)| r.iter().flatten().count()).sum();
            assert_eq!(n, 4, "{how}");
            // Still following: stretch the line in the copy read back.
            let mut back = back.clone();
            let line = back.entities().find(|e| matches!(e.geometry, Geometry::Line { .. })).unwrap().id;
            back.begin("STRETCH");
            back.update(line, |e| {
                e.geometry = Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(50.0, 0.0) }
            });
            back.commit();
            let moved = back
                .entities()
                .any(|e| matches!(e.geometry, Geometry::Dimension { p2, .. } if p2 == Point::new(50.0, 0.0)));
            assert!(moved, "{how}");
        };
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

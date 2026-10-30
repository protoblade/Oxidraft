//! AutoCAD's OLE objects (OLE2FRAME) coming in as what they show: the
//! picture an embedded object keeps (its native raster, or the cached
//! presentation — a bitmap), as acadrust finds it in the object's compound
//! file, kept in the drawing as an embedded file and shown as an image over
//! the frame. A metafile presentation shows the frame alone. A package (a
//! file Windows' Packager wrapped) comes in as INSERTOBJ makes one: the
//! file embedded, its frame and name linked to it.
//!
//! With NATIVESAVE, what INSERTOBJ embedded goes out as AutoCAD's own: an
//! OLE package of the file, its frame and name its picture.

use std::collections::{HashMap, HashSet};

use acadrust::compound_file::{
    BinaryRecord, CompoundEntry, CompoundFile, CompoundStorage, CompoundStream, CompoundStreamContent,
};
use acadrust::entities::ole_presentation::OlePresentation;
use acadrust::entities::{Ole2Frame, OleFrameEnvelope, OleObjectType};
use acadrust::objects::ObjectType;
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use oxidraft_geom::Point;
use oxidraft_io_dxf::{EMBEDDED_PREFIX, Imported};
use oxidraft_model::{
    Attach, Color, Document, Entity, EntityId, Geometry, Hyperlink, ImageDef, Vertex, XrefStatus, layout,
};

/// The OLE package's class, {0003000C-0000-0000-C000-000000000046}.
const PACKAGE: [u8; 16] = [0x0C, 0, 0x03, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];

/// What AutoCAD writes before an OLE object's corners, and after them
/// before its compound file's length, as in an AutoCAD drawing LibreDWG
/// shows (TS1.dwg): what they mean isn't known.
const FRAME_MARKER: u16 = 0x5580;
const FRAME_TAIL: [u8; 26] = [0x02, 0x1F, 0x91, 0x14, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0];

fn stream(name: &str, data: &[u8]) -> CompoundEntry {
    CompoundEntry::Stream(CompoundStream {
        name: name.to_string(),
        class_id: [0; 16],
        state_bits: 0,
        creation_time: 0,
        modified_time: 0,
        content: CompoundStreamContent::Binary(BinaryRecord::split(data, 4096)),
    })
}

/// A string as the ANSI bytes OLE keeps, ending in a zero.
fn ansi(text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = text.chars().map(|c| if c.is_ascii() && c != '\0' { c as u8 } else { b'_' }).collect();
    out.push(0);
    out
}

/// A Windows metafile of the frame with `label` across its middle, `w` by
/// `h` (logical units, y down).
fn label_metafile(label: &str, w: i16, h: i16) -> Vec<u8> {
    let mut records: Vec<u8> = Vec::new();
    let mut largest = 3u32;
    let mut record = |function: u16, params: &[u8]| {
        let words = 3 + params.len().div_ceil(2) as u32;
        largest = largest.max(words);
        records.extend(words.to_le_bytes());
        records.extend(function.to_le_bytes());
        records.extend(params);
        if params.len() % 2 == 1 {
            records.push(0);
        }
    };
    let shorts = |v: &[i16]| v.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>();
    record(0x0103, &shorts(&[8])); // SETMAPMODE: anisotropic
    record(0x020B, &shorts(&[0, 0])); // SETWINDOWORG
    record(0x020C, &shorts(&[h, w])); // SETWINDOWEXT
    // CREATEFONTINDIRECT: Arial, its capitals a sixth of the frame high.
    let mut font = shorts(&[-(h / 4).max(1), 0, 0, 0, 400]);
    font.extend([0u8; 8]);
    font.extend(ansi("Arial"));
    record(0x02FB, &font);
    record(0x012D, &shorts(&[0])); // SELECTOBJECT
    record(0x0102, &shorts(&[1])); // SETBKMODE: transparent
    record(0x012E, &shorts(&[6 | 24])); // SETTEXTALIGN: centred on its baseline
    record(0x041B, &shorts(&[h - 1, w - 1, 0, 0])); // RECTANGLE
    let mut text: Vec<u8> = ansi(label);
    text.pop();
    let mut out = shorts(&[text.len() as i16]);
    out.extend(&text);
    if text.len() % 2 == 1 {
        out.push(0);
    }
    out.extend(shorts(&[h / 2 + h / 12, w / 2]));
    record(0x0521, &out); // TEXTOUT
    record(0x01F0, &shorts(&[0])); // DELETEOBJECT
    record(0x0000, &[]); // EOF
    let mut wmf = shorts(&[1, 9, 0x0300]);
    wmf.extend((9 + records.len() as u32 / 2).to_le_bytes());
    wmf.extend(shorts(&[1]));
    wmf.extend(largest.to_le_bytes());
    wmf.extend(shorts(&[0]));
    wmf.extend(records);
    wmf
}

/// The compound file of an OLE package of `file`, named `label`: its
/// class, the package (`\x01Ole10Native`: name, path, the file), and its
/// picture (`\x02OlePres000`, a metafile `size` big, in drawing units
/// taken as millimetres).
fn package_storage(label: &str, file: &[u8], size: Point) -> CompoundFile {
    let mut native = 2u16.to_le_bytes().to_vec();
    native.extend(ansi(label));
    native.extend(ansi(label));
    // An embedded file: its temporary path, then its contents.
    native.extend(0x0003_0000u32.to_le_bytes());
    native.extend((ansi(label).len() as u32).to_le_bytes());
    native.extend(ansi(label));
    native.extend((file.len() as u32).to_le_bytes());
    native.extend(file);
    let mut ole10 = (native.len() as u32).to_le_bytes().to_vec();
    ole10.extend(native);
    // \x01CompObj: its user type, no clipboard format, its program id.
    let mut comp = vec![0x01, 0x00, 0xFE, 0xFF, 0x03, 0x0A, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF];
    comp.extend(PACKAGE);
    for s in ["OLE Package", "", "Package"] {
        if s.is_empty() {
            comp.extend(0u32.to_le_bytes());
        } else {
            comp.extend((ansi(s).len() as u32).to_le_bytes());
            comp.extend(ansi(s));
        }
    }
    comp.extend(0x71B2_39F4u32.to_le_bytes());
    comp.extend([0u8; 12]);
    // \x01Ole: an embedded object's.
    let mut ole = 0x0200_0001u32.to_le_bytes().to_vec();
    ole.extend([0u8; 16]);
    // Its picture: a metafile with no target device, of its content.
    let longest = size.x.max(size.y).max(1e-9);
    let (w, h) = ((1000.0 * size.x / longest).round().max(1.0), (1000.0 * size.y / longest).round().max(1.0));
    let wmf = label_metafile(label, w as i16, h as i16);
    let himetric = |v: f64| (v * 100.0).round().clamp(1.0, f64::from(u32::MAX)) as u32;
    let mut pres = Vec::new();
    for v in [0xFFFF_FFFF, 3, 4, 1, 0xFFFF_FFFF, 0, 0, himetric(size.x), himetric(size.y), wmf.len() as u32] {
        pres.extend(u32::to_le_bytes(v));
    }
    pres.extend(wmf);
    let mut root = CompoundStorage::root();
    root.class_id = PACKAGE;
    root.entries = vec![
        stream("\u{1}CompObj", &comp),
        stream("\u{1}Ole", &ole),
        stream("\u{1}Ole10Native", &ole10),
        stream("\u{2}OlePres000", &pres),
    ];
    CompoundFile { root, ..CompoundFile::default() }
}

/// The file an OLE package keeps: its name and contents.
fn unpackage(o: &Ole2Frame) -> Option<(String, Vec<u8>)> {
    let root = &o.storage.compound_file.as_ref()?.root;
    if root.class_id != PACKAGE {
        return None;
    }
    let native = root.entries.iter().find_map(|e| match e {
        CompoundEntry::Stream(s) if s.name == "\u{1}Ole10Native" => Some(s.content.encode()),
        _ => None,
    })?;
    let mut at = 6;
    let mut text = || {
        let end = at + native.get(at..)?.iter().position(|b| *b == 0)?;
        let s: String = native[at..end].iter().map(|b| char::from(*b)).collect();
        at = end + 1;
        Some(s)
    };
    let label = text()?;
    let _path = text()?;
    let u32_at = |i: usize| native.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    // An embedded file's (a linked one keeps only its path).
    if u32_at(at)? != 0x0003_0000 {
        return None;
    }
    at += 8 + u32_at(at + 4)? as usize;
    let size = u32_at(at)? as usize;
    let file = native.get(at + 4..at + 4 + size)?.to_vec();
    Some((label, file))
}

/// An object INSERTOBJ embedded: its frame, and its name across it, linked
/// to the embedded file.
pub(crate) struct Packaged {
    pub frame: EntityId,
    pub labels: Vec<EntityId>,
    pub file: String,
    label: String,
    space: layout::Space,
    min: Point,
    max: Point,
}

/// The embedded file `e` links to.
fn linked_file<'a>(doc: &Document, e: &'a Entity) -> Option<&'a str> {
    let name = e.hyperlink.as_ref()?.url.strip_prefix(EMBEDDED_PREFIX)?;
    doc.embedded_file(name).is_some().then_some(name)
}

/// The objects INSERTOBJ embedded: a rectangle linked to an embedded file,
/// with the texts in it linked to the same file.
pub(crate) fn packaged(doc: &Document) -> Vec<Packaged> {
    let mut out = Vec::new();
    for e in doc.all_entities() {
        let Geometry::Polyline { vertices, closed: true } = &e.geometry else { continue };
        let Some(file) = linked_file(doc, e) else { continue };
        if vertices.len() != 4 || vertices.iter().any(|v| v.bulge != 0.0 || v.start_width != 0.0 || v.end_width != 0.0)
        {
            continue;
        }
        let p: Vec<Point> = vertices.iter().map(|v| v.point).collect();
        let min =
            Point::new(p.iter().map(|p| p.x).fold(f64::MAX, f64::min), p.iter().map(|p| p.y).fold(f64::MAX, f64::min));
        let max =
            Point::new(p.iter().map(|p| p.x).fold(f64::MIN, f64::max), p.iter().map(|p| p.y).fold(f64::MIN, f64::max));
        let corner = |q: &Point| (q.x == min.x || q.x == max.x) && (q.y == min.y || q.y == max.y);
        if max.x <= min.x || max.y <= min.y || !p.iter().all(corner) {
            continue;
        }
        let inside = |q: Point| (min.x..=max.x).contains(&q.x) && (min.y..=max.y).contains(&q.y);
        let labels: Vec<&Entity> = doc
            .all_entities()
            .filter(|t| t.space == e.space && linked_file(doc, t) == Some(file))
            .filter(|t| matches!(&t.geometry, Geometry::Text { position, .. } if inside(*position)))
            .collect();
        let label = match labels.as_slice() {
            [t] => match &t.geometry {
                Geometry::Text { content, .. } => content.clone(),
                _ => unreachable!(),
            },
            _ => e.hyperlink.as_ref().map(|l| l.description.clone()).filter(|d| !d.is_empty()).unwrap_or(file.into()),
        };
        out.push(Packaged {
            frame: e.id,
            labels: labels.iter().map(|t| t.id).collect(),
            file: file.to_string(),
            label,
            space: e.space,
            min,
            max,
        });
    }
    out
}

/// NATIVESAVE: what INSERTOBJ embedded made OLE packages, in place of
/// their frames, their names dropped (the package's picture shows it).
pub(crate) fn write(cad: &mut CadDocument, doc: &Document, handles: &HashMap<EntityId, Vec<u64>>) {
    let handle = |id: &EntityId| handles.get(id).and_then(|h| h.first()).map(|h| Handle::new(*h));
    let mut removed = HashSet::new();
    for p in packaged(doc) {
        let Some(h) = handle(&p.frame) else { continue };
        let Some(frame) = cad.get_entity(h) else { continue };
        let mut ole = Ole2Frame::new();
        ole.common = frame.common().clone();
        // The package is the file: no link to it.
        let kept: Vec<_> =
            ole.common.extended_data.records().iter().filter(|r| r.application_name != "PE_URL").cloned().collect();
        ole.common.extended_data = Default::default();
        for r in kept {
            ole.common.extended_data.add_record(r);
        }
        ole.source_application = "Package".to_string();
        ole.upper_left_corner = Vector3::new(p.min.x, p.max.y, 0.0);
        ole.lower_right_corner = Vector3::new(p.max.x, p.min.y, 0.0);
        ole.ole_object_type = OleObjectType::Embedded;
        ole.is_paper_space = p.space != layout::MODEL;
        let storage = package_storage(&p.label, doc.embedded_file(&p.file).unwrap_or_default(), p.max - p.min);
        let mut tail = FRAME_TAIL.to_vec();
        tail.extend((storage.encode().len() as u32).to_le_bytes());
        ole.envelope =
            OleFrameEnvelope::Geometry { marker: FRAME_MARKER, extension_records: BinaryRecord::split(&tail, 4096) };
        ole.storage.compound_file = Some(storage);
        if let Some(slot) = cad.get_entity_mut(h) {
            *slot = EntityType::Ole2Frame(Box::new(ole));
        }
        for label in p.labels.iter().filter_map(handle) {
            cad.remove_entity(label);
            removed.insert(label);
        }
    }
    if removed.is_empty() {
        return;
    }
    for r in cad.block_records.iter_mut() {
        r.entity_handles.retain(|h| !removed.contains(h));
    }
    for o in cad.objects.values_mut() {
        if let ObjectType::Group(g) = o {
            g.entities.retain(|h| !removed.contains(h));
        }
    }
}

/// A headerless DIB (BITMAPINFO and bits), as OLE presentations and
/// Windows resources keep bitmaps, as a .bmp file.
pub fn bmp_from_dib(dib: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |i: usize| dib.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let header = u32_at(0)? as usize;
    let bits = u16::from_le_bytes([*dib.get(14)?, *dib.get(15)?]);
    let compression = u32_at(16).unwrap_or(0);
    let used = u32_at(32).unwrap_or(0) as usize;
    let palette = if used > 0 {
        used
    } else if bits <= 8 {
        1 << bits
    } else {
        0
    };
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let offset = 14 + header + masks + palette * 4;
    let mut out = b"BM".to_vec();
    out.extend(((14 + dib.len()) as u32).to_le_bytes());
    out.extend([0, 0, 0, 0]);
    out.extend((offset as u32).to_le_bytes());
    out.extend_from_slice(dib);
    Some(out)
}

/// A raster file's extension, by its first bytes.
fn extension(file: &[u8]) -> Option<&'static str> {
    if file.starts_with(b"\x89PNG") {
        Some("png")
    } else if file.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if file.starts_with(b"BM") {
        Some("bmp")
    } else if file.starts_with(b"GIF8") {
        Some("gif")
    } else {
        None
    }
}

pub(crate) fn restore(cad: &CadDocument, imported: &mut Imported) {
    let mut n = 0;
    for e in cad.entities() {
        let EntityType::Ole2Frame(o) = e else { continue };
        let (a, b) = (o.upper_left_corner, o.lower_right_corner);
        let (min, max) = (Point::new(a.x.min(b.x), a.y.min(b.y)), Point::new(a.x.max(b.x), a.y.max(b.y)));
        if max.x - min.x <= 0.0 || max.y - min.y <= 0.0 {
            continue;
        }
        let layer = e.common().layer.clone();
        let doc = &mut imported.document;
        // A package: the file embedded, as INSERTOBJ does.
        if let Some((label, file)) = unpackage(o) {
            let stored = doc.embed_file(&label, file);
            let link = Hyperlink { url: format!("{EMBEDDED_PREFIX}{stored}"), description: label.clone() };
            let (w, h) = (max.x - min.x, max.y - min.y);
            let corners = [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)];
            let name = Geometry::Text {
                look: Default::default(),
                position: Point::new(min.x + w / 2.0, min.y + h / 2.0),
                height: h / 6.0,
                rotation: 0.0,
                width: w * 0.9,
                attach: Attach::MiddleCenter,
                content: label,
            };
            let frame = Geometry::Polyline { vertices: corners.into_iter().map(Vertex::at).collect(), closed: true };
            for g in [frame, name] {
                let id = doc.add_on_layer(g, &layer, Color::ByLayer);
                let link = link.clone();
                doc.update(id, |e| e.hyperlink = Some(Box::new(link)));
            }
            continue;
        }
        let picture = match o.presentation() {
            Some(OlePresentation::Raster(file)) => Some(file),
            Some(OlePresentation::Dib(dib)) => bmp_from_dib(&dib),
            _ => None,
        };
        n += 1;
        let mut picture_file = None;
        let shown = match picture.and_then(|f| extension(&f).map(|x| (f, x))) {
            Some((file, ext)) => {
                let stored = doc.embed_file(&format!("OLE{n}.{ext}"), file);
                picture_file = Some(stored.clone());
                let name = (n..).map(|i| format!("OLE{i}")).find(|d| doc.image_def(d).is_none()).unwrap_or_default();
                doc.set_image_def(ImageDef {
                    name: name.clone(),
                    path: format!("{}{stored}", oxidraft_io_dxf::EMBEDDED_PREFIX),
                    page: None,
                    width: 0.0,
                    height: 0.0,
                    status: XrefStatus::Loaded,
                });
                doc.add_on_layer(
                    Geometry::Image {
                        image: name,
                        origin: min,
                        u: Point::new(max.x - min.x, 0.0),
                        v: Point::new(0.0, max.y - min.y),
                        clip: Vec::new(),
                        adjust: Default::default(),
                    },
                    &layer,
                    Color::ByLayer,
                )
            }
            // Its frame, where its picture can't be shown.
            None => {
                let corners = [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)];
                doc.add_on_layer(
                    Geometry::Polyline { vertices: corners.into_iter().map(Vertex::at).collect(), closed: true },
                    &layer,
                    Color::ByLayer,
                )
            }
        };
        // The document it embeds (a spreadsheet, a text, slides), kept in
        // the drawing and opened from it (Cmd/Ctrl-click), as AutoCAD opens
        // an OLE object in the program that made it.
        let kind = acadrust::entities::ole_presentation::ole_user_type(&o.encoded_payload());
        // A picture object (Paintbrush's and the like): its picture, to edit
        // in a picture editor (the image follows).
        if let (None, Some(stored)) = (document(o), &picture_file) {
            let link = Hyperlink {
                url: format!("{EMBEDDED_PREFIX}{stored}"),
                description: kind.clone().unwrap_or_else(|| stored.clone()),
            };
            doc.update(shown, |e| e.hyperlink = Some(Box::new(link)));
        }
        if let Some((file, ext)) = document(o) {
            let stored = doc.embed_file(&format!("OLE{n}.{ext}"), file);
            let link = Hyperlink {
                url: format!("{EMBEDDED_PREFIX}{stored}"),
                description: kind.unwrap_or_else(|| stored.clone()),
            };
            doc.update(shown, |e| e.hyperlink = Some(Box::new(link)));
        }
    }
}

/// A stream of an OLE object's compound file, by name.
fn stream_of(root: &CompoundStorage, name: &str) -> Option<Vec<u8>> {
    root.entries.iter().find_map(|e| match e {
        CompoundEntry::Stream(s) if s.name.eq_ignore_ascii_case(name) => Some(s.content.encode()),
        _ => None,
    })
}

/// The document an OLE object embeds, as a file, and its extension: an
/// Office 2007 or later one (its Package stream, a zip whose parts say
/// which), an older one (the compound file itself), or a PDF.
fn document(o: &Ole2Frame) -> Option<(Vec<u8>, &'static str)> {
    let cfb = o.storage.compound_file.as_ref()?;
    let root = &cfb.root;
    if let Some(package) = stream_of(root, "Package").filter(|p| p.starts_with(b"PK")) {
        let has = |part: &[u8]| package.windows(part.len()).any(|w| w == part);
        let ext = if has(b"xl/") {
            "xlsx"
        } else if has(b"word/") {
            "docx"
        } else if has(b"ppt/") {
            "pptx"
        } else {
            "zip"
        };
        return Some((package, ext));
    }
    for (stream, ext) in [("Workbook", "xls"), ("Book", "xls"), ("WordDocument", "doc"), ("PowerPoint Document", "ppt")]
    {
        if stream_of(root, stream).is_some() {
            return Some((cfb.encode(), ext));
        }
    }
    let contents = stream_of(root, "CONTENTS")?;
    contents.starts_with(b"%PDF").then_some((contents, "pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_embedded_picture_comes_in_as_an_image() {
        use acadrust::compound_file::{
            BinaryRecord, CompoundEntry, CompoundFile, CompoundStorage, CompoundStream, CompoundStreamContent,
        };
        use acadrust::entities::{Ole2Frame, OleObjectType};
        use acadrust::types::Vector3;
        // A PNG file's bytes (what's after its signature doesn't matter here).
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend((0..200u8).collect::<Vec<_>>());
        let mut doc = oxidraft_model::Document::new();
        doc.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(1.0, 1.0) });
        let dxf = crate::write_dxf(&doc).unwrap();
        let mut cad = acadrust::DxfReader::from_reader(std::io::Cursor::new(dxf)).and_then(|r| r.read()).unwrap();
        // A static picture: the PNG as its CONTENTS stream.
        let mut ole = Ole2Frame::new();
        ole.upper_left_corner = Vector3::new(10.0, 40.0, 0.0);
        ole.lower_right_corner = Vector3::new(70.0, 5.0, 0.0);
        ole.ole_object_type = OleObjectType::Static;
        let mut root = CompoundStorage::root();
        root.entries.push(CompoundEntry::Stream(CompoundStream {
            name: "CONTENTS".to_string(),
            class_id: [0; 16],
            state_bits: 0,
            creation_time: 0,
            modified_time: 0,
            content: CompoundStreamContent::Binary(BinaryRecord::split(&png, 4096)),
        }));
        ole.storage.compound_file = Some(CompoundFile { root, ..CompoundFile::default() });
        cad.add_entity(EntityType::Ole2Frame(Box::new(ole))).unwrap();
        let back = crate::imported_from(&mut cad).unwrap().document;
        let image = back.entities().find_map(|e| match &e.geometry {
            Geometry::Image { image, origin, u, v, .. } => Some((image.clone(), *origin, *u, *v)),
            _ => None,
        });
        let (name, origin, u, v) = image.expect("the picture");
        assert_eq!((origin, u, v), (Point::new(10.0, 5.0), Point::new(60.0, 0.0), Point::new(0.0, 35.0)));
        let def = back.image_def(&name).unwrap();
        let file = def.path.strip_prefix(oxidraft_io_dxf::EMBEDDED_PREFIX).unwrap();
        assert_eq!(back.embedded_file(file), Some(png.as_slice()));
        // Linked to its picture, to edit in a picture editor.
        let linked = back.entities().find_map(|e| e.hyperlink.as_ref().map(|l| l.url.clone()));
        assert_eq!(linked, Some(def.path.clone()));
        // Saved, the picture goes with the drawing.
        let again = crate::read(&crate::write(&back, crate::Version::R2018).unwrap()).unwrap().imported.document;
        assert_eq!(again.embedded_file(file), Some(png.as_slice()));
    }

    /// INSERTOBJ's object for `notes.txt`: its frame and name, linked to it.
    fn inserted_object(doc: &mut Document, file: &[u8]) {
        let name = doc.embed_file("notes.txt", file.to_vec());
        let link = Hyperlink { url: format!("{EMBEDDED_PREFIX}{name}"), description: "notes.txt".into() };
        let corners = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)].map(|(x, y)| Vertex::at(Point::new(x, y)));
        let text = Geometry::Text {
            look: Default::default(),
            position: Point::new(25.0, 15.0),
            height: 5.0,
            rotation: 0.0,
            width: 45.0,
            attach: Attach::MiddleCenter,
            content: "notes.txt".into(),
        };
        for g in [Geometry::Polyline { vertices: corners.to_vec(), closed: true }, text] {
            let id = doc.add(g);
            let link = link.clone();
            doc.update(id, |e| e.hyperlink = Some(Box::new(link)));
        }
    }

    #[test]
    fn nativesave_makes_insertobj_an_ole_package() {
        let file = b"Pour the slab on Tuesday.\n".repeat(40);
        let mut doc = Document::new();
        inserted_object(&mut doc, &file);
        doc.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(80.0, 0.0) });
        crate::set_native_objects(true);
        let dwg = crate::write(&doc, crate::Version::R2018).unwrap();
        let dxf = crate::write_dxf(&doc).unwrap();
        crate::set_native_objects(false);
        // As AutoCAD reads it: an OLE package in the frame's place, its name
        // gone with it, the file only in the package.
        let cad = acadrust::DwgReader::from_stream(std::io::Cursor::new(dwg.clone())).read().unwrap();
        let kinds: Vec<&str> = cad.entities().map(|e| e.as_entity().entity_type()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "OLE2FRAME").count(), 1, "{kinds:?}");
        assert!(!kinds.iter().any(|k| ["TEXT", "MTEXT", "LWPOLYLINE"].contains(k)), "{kinds:?}");
        let ole = cad
            .entities()
            .find_map(|e| match e {
                EntityType::Ole2Frame(o) => Some(o),
                _ => None,
            })
            .unwrap();
        assert_eq!((ole.upper_left_corner.x, ole.upper_left_corner.y), (0.0, 30.0));
        assert_eq!((ole.lower_right_corner.x, ole.lower_right_corner.y), (50.0, 0.0));
        assert!(ole.common.extended_data.records().iter().all(|r| r.application_name != "PE_URL"));
        let data = ole.encoded_payload();
        assert_eq!(&data[..2], &[0x80, 0x55]);
        assert_eq!(&data[128..136], &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1], "the compound file at 128");
        assert_eq!(u32::from_le_bytes(data[124..128].try_into().unwrap()) as usize, data.len() - 128);
        assert_eq!(acadrust::entities::ole_presentation::ole_user_type(&data).as_deref(), Some("OLE Package"));
        assert!(matches!(ole.presentation(), Some(OlePresentation::Wmf(_))));
        assert_eq!(unpackage(ole), Some(("notes.txt".to_string(), file.clone())));
        assert!(
            !cad.objects
                .values()
                .any(|o| matches!(o, ObjectType::XRecord(x) if x.entries.iter().any(|e| e.code == 310)))
        );
        // Oxidraft reads INSERTOBJ's object back, from the DWG and the DXF.
        for back in [crate::read(&dwg).unwrap().imported.document, crate::read_dxf(&dxf).unwrap().document] {
            let linked: Vec<_> = back.all_entities().filter(|e| e.hyperlink.is_some()).collect();
            assert_eq!(linked.len(), 2);
            assert!(
                linked.iter().any(|e| matches!(&e.geometry, Geometry::Text { content, .. } if content == "notes.txt"))
            );
            assert!(
                linked.iter().any(
                    |e| matches!(&e.geometry, Geometry::Polyline { vertices, closed: true } if vertices.len() == 4)
                )
            );
            let url = &linked[0].hyperlink.as_ref().unwrap().url;
            let name = url.strip_prefix(EMBEDDED_PREFIX).unwrap();
            assert_eq!(back.embedded_file(name), Some(file.as_slice()));
            assert_eq!(back.all_entities().filter(|e| matches!(e.geometry, Geometry::Line { .. })).count(), 1);
        }
        // Without NATIVESAVE, Oxidraft's own form.
        let plain =
            acadrust::DwgReader::from_stream(std::io::Cursor::new(crate::write(&doc, crate::Version::R2018).unwrap()))
                .read()
                .unwrap();
        assert!(!plain.entities().any(|e| matches!(e, EntityType::Ole2Frame(_))));
    }

    #[test]
    fn office_documents_come_in_to_open() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(1.0, 1.0) });
        let dxf = crate::write_dxf(&doc).unwrap();
        let mut cad = acadrust::DxfReader::from_reader(std::io::Cursor::new(dxf)).and_then(|r| r.read()).unwrap();
        // An Excel 2007 worksheet (its workbook a zip in the Package stream)
        // and an older Word document (the compound file is the .doc).
        // (Bytes like a zip's, naming its workbook part.)
        let xlsx = b"PK\x03\x04 xl/workbook.xml <workbook/>".to_vec();
        let mut comp = vec![0x01, 0x00, 0xFE, 0xFF, 0x03, 0x0A, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF];
        comp.extend([0u8; 16]);
        comp.extend((ansi("Microsoft Excel Worksheet").len() as u32).to_le_bytes());
        comp.extend(ansi("Microsoft Excel Worksheet"));
        for (x, streams) in [
            (0.0, vec![stream("Package", &xlsx), stream("\u{1}CompObj", &comp)]),
            (100.0, vec![stream("WordDocument", b"word's own")]),
        ] {
            let mut ole = Ole2Frame::new();
            ole.upper_left_corner = Vector3::new(x, 40.0, 0.0);
            ole.lower_right_corner = Vector3::new(x + 60.0, 5.0, 0.0);
            let mut root = CompoundStorage::root();
            root.entries = streams;
            ole.storage.compound_file = Some(CompoundFile { root, ..CompoundFile::default() });
            cad.add_entity(EntityType::Ole2Frame(Box::new(ole))).unwrap();
        }
        let back = crate::imported_from(&mut cad).unwrap().document;
        let links: Vec<&Hyperlink> = back.entities().filter_map(|e| e.hyperlink.as_deref()).collect();
        assert_eq!(links.len(), 2, "{links:?}");
        let file = |l: &Hyperlink| back.embedded_file(l.url.strip_prefix(EMBEDDED_PREFIX).unwrap()).unwrap().to_vec();
        let excel = links.iter().find(|l| l.url.ends_with(".xlsx")).expect("the workbook");
        assert_eq!(file(excel), xlsx);
        assert_eq!(excel.description, "Microsoft Excel Worksheet");
        let word = links.iter().find(|l| l.url.ends_with(".doc")).expect("the document");
        assert!(file(word).starts_with(&[0xD0, 0xCF, 0x11, 0xE0]), "the compound file itself");
    }

    #[test]
    fn a_dib_made_a_bitmap_file() {
        // A 1x1 24-bit DIB: its 40-byte header and one padded pixel.
        let mut dib = 40u32.to_le_bytes().to_vec();
        dib.extend(1i32.to_le_bytes());
        dib.extend(1i32.to_le_bytes());
        dib.extend(1u16.to_le_bytes());
        dib.extend(24u16.to_le_bytes());
        dib.extend([0u8; 24]);
        dib.extend([0, 0, 255, 0]);
        let bmp = bmp_from_dib(&dib).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 54, "pixels after both headers");
        assert_eq!(extension(&bmp), Some("bmp"));
    }
}

//! AutoCAD's OLE objects (OLE2FRAME) coming in as what they show: the
//! picture an embedded object keeps (its native raster, or the cached
//! presentation — a bitmap), as acadrust finds it in the object's compound
//! file, kept in the drawing as an embedded file and shown as an image over
//! the frame. A metafile presentation shows the frame alone.

use acadrust::entities::ole_presentation::OlePresentation;
use acadrust::{CadDocument, EntityType};
use oxidraft_geom::Point;
use oxidraft_io_dxf::Imported;
use oxidraft_model::{Color, Geometry, ImageDef, Vertex, XrefStatus};

/// A headerless DIB (BITMAPINFO and bits) as a .bmp file.
fn bmp_from_dib(dib: &[u8]) -> Option<Vec<u8>> {
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
        let picture = match o.presentation() {
            Some(OlePresentation::Raster(file)) => Some(file),
            Some(OlePresentation::Dib(dib)) => bmp_from_dib(&dib),
            _ => None,
        };
        n += 1;
        match picture.and_then(|f| extension(&f).map(|x| (f, x))) {
            Some((file, ext)) => {
                let stored = doc.embed_file(&format!("OLE{n}.{ext}"), file);
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
                );
            }
            // Its frame, where its picture can't be shown.
            None => {
                let corners = [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)];
                doc.add_on_layer(
                    Geometry::Polyline { vertices: corners.into_iter().map(Vertex::at).collect(), closed: true },
                    &layer,
                    Color::ByLayer,
                );
            }
        }
    }
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
        // Saved, the picture goes with the drawing.
        let again = crate::read(&crate::write(&back, crate::Version::R2018).unwrap()).unwrap().imported.document;
        assert_eq!(again.embedded_file(file), Some(png.as_slice()));
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

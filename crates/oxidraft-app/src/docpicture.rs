//! Pictures of the documents AutoCAD's OLE objects embed (Office
//! documents and PDFs), as AutoCAD shows an OLE object by the picture its
//! program draws: a PDF's first page drawn here, an Office document's by
//! the system (Quick Look on macOS; LibreOffice elsewhere, where it's
//! installed). A document saved in its program shows in the drawing as
//! saved; one AutoCAD drew as a metafile (which Oxidraft doesn't draw)
//! shows its document, not an empty frame.

use std::path::Path;
use std::process::{Command, Stdio};

use image::{ImageFormat, Rgba, RgbaImage};
use oxidraft_geom::Point;
use oxidraft_io_dxf::EMBEDDED_PREFIX;
use oxidraft_model::{EntityId, Geometry, ImageDef, XrefStatus};

use crate::OxidraftApp;

/// The documents whose pictures can be drawn, by extension.
const DOCUMENTS: [&str; 8] = ["xlsx", "xlsm", "xls", "docx", "doc", "pptx", "ppt", "pdf"];

fn is_document(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| DOCUMENTS.iter().any(|d| ext.eq_ignore_ascii_case(d)))
}

/// A picture of document `name` (its extension says what it is) with
/// contents `bytes`, on white.
pub(crate) fn picture(name: &str, bytes: &[u8]) -> Option<RgbaImage> {
    let dir = std::env::temp_dir().join(format!("oxidraft-pictures-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let file = dir.join(name.replace(['/', '\\'], "_"));
    std::fs::write(&file, bytes).ok()?;
    let drawn = if name.to_ascii_lowercase().ends_with(".pdf") {
        oxidraft_raster::decode(&file, 0, 1200).ok().and_then(|p| RgbaImage::from_raw(p.width, p.height, p.rgba))
    } else {
        drawn_by_system(&file, &dir).and_then(|png| image::load_from_memory(&png).ok()).map(|i| i.to_rgba8())
    };
    let _ = std::fs::remove_file(&file);
    let mut drawn = drawn?;
    // On white, as the page is.
    for p in drawn.pixels_mut() {
        let a = u16::from(p[3]);
        for c in 0..3 {
            p[c] = ((u16::from(p[c]) * a + 255 * (255 - a)) / 255) as u8;
        }
        p[3] = 255;
    }
    Some(drawn)
}

/// Quick Look's picture of `file`, as PNG, made in `dir`.
#[cfg(target_os = "macos")]
fn drawn_by_system(file: &Path, dir: &Path) -> Option<Vec<u8>> {
    let out = dir.join(format!("{}.png", file.file_name()?.to_string_lossy()));
    let _ = std::fs::remove_file(&out);
    let status = Command::new("qlmanage")
        .args(["-t", "-s", "1200", "-o"])
        .arg(dir)
        .arg(file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    let png = std::fs::read(&out).ok().filter(|_| status.success());
    let _ = std::fs::remove_file(&out);
    png
}

/// LibreOffice's picture of `file` (its first page), as PNG, made in
/// `dir`: soffice on the PATH, or where Windows installs it.
#[cfg(not(target_os = "macos"))]
fn drawn_by_system(file: &Path, dir: &Path) -> Option<Vec<u8>> {
    let out = dir.join(format!("{}.png", file.file_stem()?.to_string_lossy()));
    for program in ["soffice", "libreoffice", r"C:\Program Files\LibreOffice\program\soffice.exe"] {
        let _ = std::fs::remove_file(&out);
        let made = Command::new(program)
            .args(["--headless", "--convert-to", "png", "--outdir"])
            .arg(dir)
            .arg(file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if made && let Ok(png) = std::fs::read(&out) {
            let _ = std::fs::remove_file(&out);
            return Some(png);
        }
    }
    None
}

/// `picture` in the middle of a white one as wide for its height as
/// `aspect`, so a frame shows it unstretched.
fn fitted(picture: &RgbaImage, aspect: f64) -> RgbaImage {
    let (w, h) = (f64::from(picture.width()), f64::from(picture.height()));
    if !(aspect.is_finite() && aspect > 0.0) || (w / h - aspect).abs() < 1e-3 {
        return picture.clone();
    }
    let (width, height) = if w / h < aspect { ((h * aspect).round(), h) } else { (w, (w / aspect).round()) };
    let mut out = RgbaImage::from_pixel(width as u32, height as u32, Rgba([255, 255, 255, 255]));
    let x = (width as u32 - picture.width()) / 2;
    let y = (height as u32 - picture.height()) / 2;
    image::imageops::overlay(&mut out, picture, i64::from(x), i64::from(y));
    out
}

/// `picture` as a file of the kind `name`'s extension says (PNG unless
/// it's a JPEG or BMP).
fn encoded(picture: &RgbaImage, name: &str) -> Option<Vec<u8>> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let format = match ext.as_str() {
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "bmp" => ImageFormat::Bmp,
        _ => ImageFormat::Png,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    let rgb = image::DynamicImage::ImageRgba8(picture.clone());
    let rgb = if format == ImageFormat::Png { rgb } else { image::DynamicImage::ImageRgb8(rgb.to_rgb8()) };
    rgb.write_to(&mut out, format).ok()?;
    Some(out.into_inner())
}

impl OxidraftApp {
    /// The objects that show embedded document `name`: (object, its
    /// geometry).
    fn showing(&self, name: &str) -> Vec<(EntityId, Geometry)> {
        let url = format!("{EMBEDDED_PREFIX}{name}");
        self.doc
            .all_entities()
            .filter(|e| e.hyperlink.as_ref().is_some_and(|l| l.url == url))
            .map(|e| (e.id, e.geometry.clone()))
            .collect()
    }

    /// OLE objects AutoCAD drew as metafiles (their frames, each linked
    /// to its document): their documents' pictures in the frames. Not
    /// INSERTOBJ's files, which show their names.
    pub(crate) fn picture_document_frames(&mut self) {
        let documents: Vec<String> =
            self.doc.embedded_files().map(|(n, _)| n.clone()).filter(|n| is_document(n)).collect();
        for name in documents {
            let showing = self.showing(&name);
            if showing.iter().any(|(_, g)| matches!(g, Geometry::Text { .. })) {
                continue;
            }
            let frames: Vec<(EntityId, Point, Point)> = showing
                .iter()
                .filter_map(|(id, g)| {
                    let Geometry::Polyline { vertices, closed: true } = g else { return None };
                    let (min, max) = rectangle(vertices.iter().map(|v| v.point).collect::<Vec<_>>().as_slice())?;
                    vertices.iter().all(|v| v.bulge == 0.0).then_some((*id, min, max))
                })
                .collect();
            if frames.is_empty() {
                continue;
            }
            let Some(drawn) = self.doc.embedded_file(&name).and_then(|b| picture(&name, b)) else { continue };
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s).to_string();
            for (id, min, max) in frames {
                let size = max - min;
                let Some(png) = encoded(&fitted(&drawn, size.x / size.y), "picture.png") else { continue };
                let stored = self.doc.embed_file(&format!("{stem}.png"), png);
                let image = (1..)
                    .map(|i| if i == 1 { stem.clone() } else { format!("{stem}({i})") })
                    .find(|n| self.doc.image_def(n).is_none())
                    .unwrap_or_default();
                self.doc.set_image_def(ImageDef {
                    name: image.clone(),
                    path: format!("{EMBEDDED_PREFIX}{stored}"),
                    page: None,
                    width: 0.0,
                    height: 0.0,
                    status: XrefStatus::Loaded,
                });
                self.doc.modify(
                    id,
                    Geometry::Image {
                        image,
                        origin: min,
                        u: Point::new(size.x, 0.0),
                        v: Point::new(0.0, size.y),
                        clip: Vec::new(),
                        adjust: Default::default(),
                    },
                );
            }
        }
        crate::xref::copy_out_embedded(&self.doc);
    }

    /// The pictures of the objects showing embedded document `name`, drawn
    /// again from it as it's now saved; each its own picture's file and
    /// kind. Not a step to undo, as the document's change isn't.
    pub(crate) fn redraw_document_pictures(&mut self, name: &str) {
        if !is_document(name) {
            return;
        }
        let images: Vec<(String, Point, Point)> = self
            .showing(name)
            .into_iter()
            .filter_map(|(_, g)| match g {
                Geometry::Image { image, u, v, .. } => Some((image, u, v)),
                _ => None,
            })
            .collect();
        if images.is_empty() {
            return;
        }
        let Some(drawn) = self.doc.embedded_file(name).and_then(|b| picture(name, b)) else { return };
        let mut redrawn = Vec::new();
        for (image, u, v) in images {
            let Some(file) = self
                .doc
                .image_def(&image)
                .and_then(|d| d.path.strip_prefix(EMBEDDED_PREFIX).map(str::to_string))
                .filter(|f| f != name)
            else {
                continue;
            };
            let Some(bytes) = encoded(&fitted(&drawn, u.length() / v.length()), &file) else { continue };
            if self.doc.update_embedded_file(&file, bytes) {
                redrawn.push(image);
            }
        }
        crate::xref::copy_out_embedded(&self.doc);
        for image in redrawn {
            self.load_image(&image);
        }
    }
}

/// The corners of an axis-aligned rectangle of four points, if they are one.
fn rectangle(points: &[Point]) -> Option<(Point, Point)> {
    if points.len() != 4 {
        return None;
    }
    let min = Point::new(
        points.iter().map(|p| p.x).fold(f64::MAX, f64::min),
        points.iter().map(|p| p.y).fold(f64::MAX, f64::min),
    );
    let max = Point::new(
        points.iter().map(|p| p.x).fold(f64::MIN, f64::max),
        points.iter().map(|p| p.y).fold(f64::MIN, f64::max),
    );
    let on_corner = |p: &Point| {
        ((p.x - min.x).abs() < 1e-9 || (p.x - max.x).abs() < 1e-9)
            && ((p.y - min.y).abs() < 1e-9 || (p.y - max.y).abs() < 1e-9)
    };
    (max.x - min.x > 0.0 && max.y - min.y > 0.0 && points.iter().all(on_corner)).then_some((min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_fit_their_frames() {
        let p = RgbaImage::from_pixel(100, 50, Rgba([0, 0, 0, 255]));
        let f = fitted(&p, 1.0);
        assert_eq!(f.dimensions(), (100, 100));
        assert_eq!(
            (f.get_pixel(50, 10)[0], f.get_pixel(50, 50)[0]),
            (255, 0),
            "white above, the picture in the middle"
        );
        assert_eq!(fitted(&p, 4.0).dimensions(), (200, 50));
        let bmp = encoded(&p, "OLE1.bmp").unwrap();
        assert!(bmp.starts_with(b"BM"));
        assert!(encoded(&p, "OLE1.jpg").unwrap().starts_with(&[0xFF, 0xD8]));
        assert!(encoded(&p, "x.png").unwrap().starts_with(b"\x89PNG"));
        assert_eq!(
            rectangle(&[Point::new(0.0, 0.0), Point::new(2.0, 0.0), Point::new(2.0, 1.0), Point::new(0.0, 1.0)]),
            Some((Point::ORIGIN, Point::new(2.0, 1.0)))
        );
        assert_eq!(
            rectangle(&[Point::new(0.0, 0.0), Point::new(2.0, 0.5), Point::new(2.0, 1.0), Point::new(0.0, 1.0)]),
            None
        );
    }
}

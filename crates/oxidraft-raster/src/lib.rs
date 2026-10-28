//! Raster and PDF underlays: where an image belongs in the drawing (from
//! GeoTIFF tags or a world file), and its pixels, scaled down to what the
//! screen and plots need. A PDF page is rendered to pixels the same way.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use oxidraft_geom::Point;

/// Where an image lies in the drawing: its lower-left corner, and the
/// vectors along its bottom edge (its whole width) and its left edge (its
/// whole height, upward).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub origin: Point,
    pub u: Point,
    pub v: Point,
}

impl Placement {
    /// From the upper-left corner and the vectors across and down the
    /// image, as GeoTIFFs and world files describe it.
    fn from_upper_left(upper_left: Point, across: Point, down: Point) -> Placement {
        Placement { origin: upper_left + down, u: across, v: -down }
    }
}

/// What a file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// Pixels across and down; for a PDF page, its size in points.
    pub width: f64,
    pub height: f64,
    /// Pages in a PDF (0 for an image).
    pub pages: usize,
    /// Where the file says it belongs (GeoTIFF tags or a world file).
    pub georef: Option<Placement>,
}

/// Decoded pixels, RGBA, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

fn is_tiff(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff"))
}

/// The world files that may sit next to an image: "map.tfw" (first and
/// last letters of the extension and "w"), "map.tifw", "map.wld".
fn world_files(path: &Path) -> Vec<PathBuf> {
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let mut names = Vec::new();
    if let (Some(first), Some(last)) = (ext.chars().next(), ext.chars().last()) {
        names.push(format!("{first}{last}w"));
    }
    names.push(format!("{ext}w"));
    names.push("wld".into());
    names
        .into_iter()
        .flat_map(|n| [n.to_ascii_lowercase(), n.to_ascii_uppercase()])
        .map(|n| path.with_extension(n))
        .collect()
}

/// A world file's placement for an image `width` x `height` pixels: its six
/// lines are the pixel's size across (A) and its rotation (D, B), the
/// pixel's size down (E, negative), and the center of the upper-left pixel
/// (C, F).
fn read_world_file(path: &Path, width: f64, height: f64) -> Option<Placement> {
    let text = world_files(path).into_iter().find_map(|p| std::fs::read_to_string(p).ok())?;
    let n: Vec<f64> = text.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    let [a, d, b, e, c, f] = n.get(..6)?.try_into().ok()?;
    let upper_left = Point::new(c - a / 2.0 - b / 2.0, f - d / 2.0 - e / 2.0);
    Some(Placement::from_upper_left(upper_left, Point::new(a * width, d * width), Point::new(b * height, e * height)))
}

/// A GeoTIFF's placement: a full model transformation, or a tie point and
/// pixel size. Tie points of "pixel is point" rasters mark pixel centers.
fn read_geotiff(path: &Path) -> Option<Placement> {
    use tiff::tags::Tag;
    let mut decoder = tiff::decoder::Decoder::new(BufReader::new(File::open(path).ok()?)).ok()?;
    let (w, h) = decoder.dimensions().ok()?;
    let (w, h) = (f64::from(w), f64::from(h));
    let point_raster = decoder
        .get_tag_u16_vec(Tag::GeoKeyDirectoryTag)
        .ok()
        .is_some_and(|keys| keys.chunks(4).skip(1).any(|k| k.len() == 4 && k[0] == 1025 && k[1] == 0 && k[3] == 2));
    let (upper_left, across, down) = if let Ok(m) = decoder.get_tag_f64_vec(Tag::ModelTransformationTag)
        && m.len() >= 8
    {
        (Point::new(m[3], m[7]), Point::new(m[0], m[4]), Point::new(m[1], m[5]))
    } else {
        let scale = decoder.get_tag_f64_vec(Tag::ModelPixelScaleTag).ok()?;
        let tie = decoder.get_tag_f64_vec(Tag::ModelTiepointTag).ok()?;
        let (sx, sy) = (*scale.first()?, *scale.get(1)?);
        let (i, j, x, y) = (*tie.first()?, *tie.get(1)?, *tie.get(3)?, *tie.get(4)?);
        (Point::new(x - i * sx, y + j * sy), Point::new(sx, 0.0), Point::new(0.0, -sy))
    };
    let upper_left = if point_raster { upper_left - (across + down) * 0.5 } else { upper_left };
    Some(Placement::from_upper_left(upper_left, across * w, down * h))
}

fn pdf_page_size(data: Vec<u8>, page: usize) -> Result<(f64, f64, usize), String> {
    let pdf = hayro::hayro_syntax::Pdf::new(data).map_err(|e| format!("not a readable PDF ({e:?})"))?;
    let pages = pdf.pages();
    let p = pages.get(page).ok_or_else(|| format!("the PDF has {} pages", pages.len()))?;
    let (w, h) = p.render_dimensions();
    Ok((f64::from(w), f64::from(h), pages.len()))
}

/// What a file is and where it belongs, without decoding its pixels.
pub fn probe(path: &Path, page: usize) -> Result<Info, String> {
    if is_pdf(path) {
        let (width, height, pages) = pdf_page_size(std::fs::read(path).map_err(|e| e.to_string())?, page)?;
        return Ok(Info { width, height, pages, georef: None });
    }
    let (w, h) = image::image_dimensions(path).map_err(|e| format!("not a readable image ({e})"))?;
    let (width, height) = (f64::from(w), f64::from(h));
    let georef = is_tiff(path).then(|| read_geotiff(path)).flatten().or_else(|| read_world_file(path, width, height));
    Ok(Info { width, height, pages: 0, georef })
}

/// The pixels of an image, or of a PDF page rendered at up to 200 dpi,
/// scaled down to at most `max_side` pixels across or down.
pub fn decode(path: &Path, page: usize, max_side: u32) -> Result<Pixels, String> {
    if is_pdf(path) {
        return render_pdf(path, page, max_side);
    }
    let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?;
    reader.no_limits();
    let image = reader.with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?;
    let image = if image.width().max(image.height()) > max_side { image.thumbnail(max_side, max_side) } else { image };
    let rgba = image.to_rgba8();
    Ok(Pixels { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

fn render_pdf(path: &Path, page: usize, max_side: u32) -> Result<Pixels, String> {
    use hayro::vello_cpu::{color::palette::css::WHITE, peniko::ImageAlphaType};
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let pdf = hayro::hayro_syntax::Pdf::new(data).map_err(|e| format!("not a readable PDF ({e:?})"))?;
    let p = pdf.pages().get(page).ok_or("no such page")?;
    let (w, h) = p.render_dimensions();
    let scale = (max_side.min(16_000) as f32 / w.max(h)).min(200.0 / 72.0);
    let settings = hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
    let interpreter = hayro::hayro_interpret::InterpreterSettings::default();
    let pixmap =
        hayro::render(p, &hayro::RenderCache::new(), &interpreter, &hayro::RenderSettings::default(), &settings);
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    Ok(Pixels { width, height, rgba: pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oxidraft-raster-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn world_files_place_images() {
        let png = temp("ortho.png");
        image::RgbaImage::from_pixel(40, 20, image::Rgba([10, 20, 30, 255])).save(&png).unwrap();
        // 0.5 units a pixel, upper-left pixel centered on (1000.25, 2009.75).
        std::fs::write(png.with_extension("pgw"), "0.5\n0\n0\n-0.5\n1000.25\n2009.75\n").unwrap();
        let info = probe(&png, 0).unwrap();
        assert_eq!((info.width, info.height, info.pages), (40.0, 20.0, 0));
        let g = info.georef.unwrap();
        assert!(g.origin.approx_eq(Point::new(1000.0, 2000.0)), "{g:?}");
        assert!(g.u.approx_eq(Point::new(20.0, 0.0)) && g.v.approx_eq(Point::new(0.0, 10.0)));
        let px = decode(&png, 0, 16).unwrap();
        assert_eq!((px.width, px.height), (16, 8));
        assert_eq!(&px.rgba[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn geotiff_tags_place_images() {
        use tiff::{encoder::TiffEncoder, encoder::colortype, tags::Tag};
        let path = temp("tile.tif");
        let mut enc = TiffEncoder::new(File::create(&path).unwrap()).unwrap();
        let mut img = enc.new_image::<colortype::RGB8>(30, 10).unwrap();
        img.encoder().write_tag(Tag::ModelPixelScaleTag, &[2.0f64, 2.0, 0.0][..]).unwrap();
        img.encoder().write_tag(Tag::ModelTiepointTag, &[0.0f64, 0.0, 0.0, 500_000.0, 4_700_020.0, 0.0][..]).unwrap();
        img.write_data(&[128u8; 30 * 10 * 3]).unwrap();
        let g = probe(&path, 0).unwrap().georef.unwrap();
        assert!(g.origin.approx_eq(Point::new(500_000.0, 4_700_000.0)), "{g:?}");
        assert!(g.u.approx_eq(Point::new(60.0, 0.0)) && g.v.approx_eq(Point::new(0.0, 20.0)));
        assert_eq!(decode(&path, 0, 4096).unwrap().width, 30);
    }

    #[test]
    fn pdf_pages_render() {
        // A one-page PDF, 200 x 100 points, with a black rectangle.
        let content = b"0 0 0 rg 20 20 160 60 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend(b"xref\n0 5\n0000000000 65535 f \n");
        for o in offsets {
            pdf.extend(format!("{o:010} 00000 n \n").as_bytes());
        }
        pdf.extend(format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        let path = temp("plan.pdf");
        std::fs::write(&path, pdf).unwrap();
        let info = probe(&path, 0).unwrap();
        assert_eq!((info.width, info.height, info.pages), (200.0, 100.0, 1));
        let px = decode(&path, 0, 400).unwrap();
        assert_eq!((px.width, px.height), (400, 200));
        let at = |x: u32, y: u32| px.rgba[((y * px.width + x) * 4) as usize];
        assert_eq!((at(5, 5), at(200, 100)), (255, 0), "white page, black rectangle");
    }
}

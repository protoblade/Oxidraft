//! Raster and PDF underlays: where an image belongs in the drawing (from
//! GeoTIFF tags or a world file), and its pixels, scaled down to what the
//! screen and plots need: the whole image, or the part of it in view. A
//! PDF page is rendered to pixels the same way. TIFFs are read a strip or
//! tile at a time, only those a part covers, so large orthophotos never
//! have to be decoded whole.

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

/// A part of an image: pixel columns `x0..x1` and rows `y0..y1`, from the
/// top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Region {
    pub fn width(&self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }

    pub fn height(&self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }

    /// True if `other` lies inside this one.
    pub fn covers(&self, other: &Region) -> bool {
        self.x0 <= other.x0 && self.y0 <= other.y0 && self.x1 >= other.x1 && self.y1 >= other.y1
    }
}

/// Decoded pixels, RGBA, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub mod pdf_layers;
pub mod pdf_vectors;

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
    let (w, h) = dimensions(path)?;
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
    if is_tiff(path)
        && let Ok((w, h)) = dimensions(path)
    {
        let scale = (f64::from(max_side) / f64::from(w.max(h))).min(1.0);
        let out = |n: u32| ((f64::from(n) * scale).round() as u32).max(1);
        if let Ok(px) = read_tiff(path, Region { x0: 0, y0: 0, x1: w, y1: h }, out(w), out(h)) {
            return Ok(px);
        }
    }
    let image = open_image(path)?;
    let image = if image.width().max(image.height()) > max_side { image.thumbnail(max_side, max_side) } else { image };
    let rgba = image.to_rgba8();
    Ok(Pixels { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

/// `region` of an image (not a PDF), scaled to `width` × `height` pixels.
pub fn decode_region(path: &Path, region: Region, width: u32, height: u32) -> Result<Pixels, String> {
    if region.width() == 0 || region.height() == 0 || width == 0 || height == 0 {
        return Err("nothing to read".into());
    }
    if is_tiff(path)
        && let Ok(px) = read_tiff(path, region, width, height)
    {
        return Ok(px);
    }
    // Other formats are decoded whole, then cut.
    let image = open_image(path)?;
    let (x1, y1) = (region.x1.min(image.width()), region.y1.min(image.height()));
    if region.x0 >= x1 || region.y0 >= y1 {
        return Err("outside the image".into());
    }
    let part = image.crop_imm(region.x0, region.y0, x1 - region.x0, y1 - region.y0);
    let rgba = part.resize_exact(width, height, image::imageops::FilterType::Triangle).to_rgba8();
    Ok(Pixels { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

/// Pixels across and down. TIFFs are asked directly: the image crate can't
/// open every kind (JPEG-compressed YCbCr, palettes).
fn dimensions(path: &Path) -> Result<(u32, u32), String> {
    if is_tiff(path)
        && let Ok(file) = File::open(path)
        && let Ok(mut d) = tiff::decoder::Decoder::new(BufReader::new(file))
        && let Ok(dims) = d.dimensions()
    {
        return Ok(dims);
    }
    image::image_dimensions(path).map_err(|e| format!("not a readable image ({e})"))
}

fn open_image(path: &Path) -> Result<image::DynamicImage, String> {
    let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?;
    reader.no_limits();
    reader.with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())
}

/// One pixel of a TIFF's samples as RGBA, by its color type.
fn tiff_rgba(color: tiff::ColorType, palette: &[u16], s: &[u8]) -> [u8; 4] {
    use tiff::ColorType;
    match color {
        ColorType::Gray(_) | ColorType::Multiband { .. } if s.len() < 3 => [s[0], s[0], s[0], 255],
        ColorType::GrayA(_) => [s[0], s[0], s[0], s[1]],
        ColorType::RGBA(_) => [s[0], s[1], s[2], s[3]],
        ColorType::YCbCr(_) => {
            // JPEG-compressed orthophotos keep YCbCr (JFIF's conversion).
            let (y, cb, cr) = (f32::from(s[0]), f32::from(s[1]) - 128.0, f32::from(s[2]) - 128.0);
            let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            [c(y + 1.402 * cr), c(y - 0.344_136 * cb - 0.714_136 * cr), c(y + 1.772 * cb), 255]
        }
        ColorType::Palette(_) => {
            let n = palette.len() / 3;
            let i = usize::from(s[0]);
            let at = |k: usize| palette.get(k * n + i).map_or(0, |v| (v >> 8) as u8);
            [at(0), at(1), at(2), 255]
        }
        ColorType::CMYK(_) | ColorType::CMYKA(_) => {
            let k = 255 - u16::from(s[3]);
            let c = |v: u8| ((255 - u16::from(v)) * k / 255) as u8;
            [c(s[0]), c(s[1]), c(s[2]), 255]
        }
        // RGB, and the first three bands of anything else.
        _ => [s[0], s[1], s[2], 255],
    }
}

/// Reads `region` of a TIFF a chunk (strip or tile) at a time, only the
/// chunks it covers, into `width` × `height` pixels. Shrinking averages
/// four samples a pixel.
fn read_tiff(path: &Path, region: Region, width: u32, height: u32) -> Result<Pixels, String> {
    use tiff::ColorType;
    use tiff::decoder::{ChunkType, Decoder, DecodingResult, Limits};
    use tiff::tags::Tag;
    let err = |e: tiff::TiffError| e.to_string();
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut d = Decoder::new(BufReader::new(file)).map_err(err)?.with_limits(Limits::unlimited());
    let (w, h) = d.dimensions().map_err(err)?;
    let color = d.colortype().map_err(err)?;
    let samples = match color {
        ColorType::Gray(_) | ColorType::Palette(_) => 1,
        ColorType::GrayA(_) => 2,
        ColorType::RGB(_) | ColorType::YCbCr(_) => 3,
        ColorType::RGBA(_) | ColorType::CMYK(_) => 4,
        ColorType::CMYKA(_) => 5,
        ColorType::Multiband { num_samples, .. } => usize::from(num_samples),
        _ => return Err("unsupported color type".into()),
    };
    // Separate planes of samples are left to the general decoder.
    if !matches!(color.bit_depth(), 8 | 16) || d.get_tag_u32(Tag::PlanarConfiguration).is_ok_and(|p| p == 2) {
        return Err("unsupported sample layout".into());
    }
    let palette = match color {
        ColorType::Palette(_) => d.get_tag_u16_vec(Tag::ColorMap).map_err(err)?,
        _ => Vec::new(),
    };
    let r = Region { x0: region.x0, y0: region.y0, x1: region.x1.min(w), y1: region.y1.min(h) };
    if r.width() == 0 || r.height() == 0 {
        return Err("outside the image".into());
    }
    let (cw, ch) = d.chunk_dimensions();
    let across = match d.get_chunk_type() {
        ChunkType::Strip => 1,
        ChunkType::Tile => w.div_ceil(cw),
    };
    // Where each output pixel samples the image: once, or twice each way
    // when shrinking. Both lists run in order.
    let per = if r.width() >= 2 * width || r.height() >= 2 * height { 2 } else { 1 };
    let positions = |from: u32, len: u32, out: u32| -> Vec<(u32, usize)> {
        let step = f64::from(len) / f64::from(out * per);
        (0..out * per)
            .map(|i| ((from + ((f64::from(i) + 0.5) * step) as u32).min(from + len - 1), (i / per) as usize))
            .collect()
    };
    let (xs, ys) = (positions(r.x0, r.width(), width), positions(r.y0, r.height(), height));
    let mut sums = vec![[0u16; 4]; (width * height) as usize];
    let within = |list: &[(u32, usize)], lo: u32, hi: u32| {
        list.partition_point(|p| p.0 < lo)..list.partition_point(|p| p.0 < hi)
    };
    for ty in r.y0 / ch..=(r.y1 - 1) / ch {
        let rows = within(&ys, ty * ch, (ty + 1) * ch);
        if rows.is_empty() {
            continue;
        }
        for tx in r.x0 / cw..=(r.x1 - 1) / cw {
            let cols = within(&xs, tx * cw, (tx + 1) * cw);
            if cols.is_empty() {
                continue;
            }
            let index = ty * across + tx;
            let data: Vec<u8> = match d.read_chunk(index).map_err(err)? {
                DecodingResult::U8(v) => v,
                DecodingResult::U16(v) => v.into_iter().map(|x| (x >> 8) as u8).collect(),
                _ => return Err("unsupported sample format".into()),
            };
            let row_len = d.chunk_data_dimensions(index).0 as usize * samples;
            for &(sy, oy) in &ys[rows.clone()] {
                let row = (sy - ty * ch) as usize * row_len;
                for &(sx, ox) in &xs[cols.clone()] {
                    let at = row + (sx - tx * cw) as usize * samples;
                    let Some(px) = data.get(at..at + samples) else { continue };
                    let sum = &mut sums[oy * width as usize + ox];
                    for (s, v) in sum.iter_mut().zip(tiff_rgba(color, &palette, px)) {
                        *s += u16::from(v);
                    }
                }
            }
        }
    }
    let n = (per * per) as u16;
    let rgba = sums.into_iter().flat_map(|s| s.map(|v| (v / n) as u8)).collect();
    Ok(Pixels { width, height, rgba })
}

/// Page `page` of the PDF in `data` at `dpi` dots to the inch, on white
/// (PNG and JPEG plots).
pub fn render_pdf_data(data: Vec<u8>, page: usize, dpi: f32) -> Result<Pixels, String> {
    use hayro::vello_cpu::{color::palette::css::WHITE, peniko::ImageAlphaType};
    let pdf = hayro::hayro_syntax::Pdf::new(data).map_err(|e| format!("not a readable PDF ({e:?})"))?;
    let p = pdf.pages().get(page).ok_or("no such page")?;
    let (w, h) = p.render_dimensions();
    // At most 16000 pixels a side.
    let scale = (dpi / 72.0).min(16_000.0 / w.max(h)).max(0.01);
    let settings = hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
    let interpreter = hayro::hayro_interpret::InterpreterSettings::default();
    let pixmap =
        hayro::render(p, &hayro::RenderCache::new(), &interpreter, &hayro::RenderSettings::default(), &settings);
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    Ok(Pixels { width, height, rgba: pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied) })
}

fn render_pdf(path: &Path, page: usize, max_side: u32) -> Result<Pixels, String> {
    use hayro::vello_cpu::{color::palette::css::WHITE, peniko::ImageAlphaType};
    // Without the layers turned off for it (ULAYERS).
    let data = pdf_layers::read_pdf(path).map_err(|e| e.to_string())?;
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
    fn parts_of_tiled_and_striped_tiffs_read_only_what_they_cover() {
        use tiff::{encoder::TiffEncoder, encoder::colortype};
        // A 300 x 200 gradient: red across, green down.
        let pixel = |x: u32, y: u32| [(x * 255 / 299) as u8, (y * 255 / 199) as u8, 77];
        let data: Vec<u8> = (0..200).flat_map(|y| (0..300).flat_map(move |x| pixel(x, y))).collect();
        let striped = temp("striped.tif");
        let mut enc = TiffEncoder::new(File::create(&striped).unwrap()).unwrap();
        let mut img = enc.new_image::<colortype::RGB8>(300, 200).unwrap();
        img.rows_per_strip(7).unwrap();
        img.write_data(&data).unwrap();
        let region = Region { x0: 100, y0: 50, x1: 160, y1: 90 };
        // At full resolution the part is the image's own pixels.
        let px = decode_region(&striped, region, 60, 40).unwrap();
        assert_eq!((px.width, px.height), (60, 40));
        let at = |px: &Pixels, x: u32, y: u32| px.rgba[((y * px.width + x) * 4) as usize..][..4].to_vec();
        assert_eq!(at(&px, 0, 0), [pixel(100, 50).as_slice(), &[255]].concat());
        assert_eq!(at(&px, 59, 39), [pixel(159, 89).as_slice(), &[255]].concat());
        // Shrunk, each pixel averages its share.
        let small = decode_region(&striped, region, 15, 10).unwrap();
        let p = at(&small, 7, 5);
        assert!((i32::from(p[0]) - i32::from(pixel(130, 70)[0])).abs() <= 2, "{p:?}");
        // The whole image, as the screen's overview.
        let whole = decode(&striped, 0, 150).unwrap();
        assert_eq!((whole.width, whole.height), (150, 100));
        assert!(decode_region(&striped, Region { x0: 400, y0: 0, x1: 500, y1: 10 }, 10, 1).is_err());

        // The same image in 64 x 64 tiles; a part across four of them.
        let tiled = temp("tiled.tif");
        std::fs::write(&tiled, tiled_rgb(300, 200, 64, &pixel)).unwrap();
        let region = Region { x0: 50, y0: 40, x1: 140, y1: 100 };
        let px = decode_region(&tiled, region, 90, 60).unwrap();
        for (x, y) in [(0, 0), (13, 23), (14, 24), (89, 59), (45, 30)] {
            assert_eq!(at(&px, x, y), [pixel(50 + x, 40 + y).as_slice(), &[255]].concat(), "({x}, {y})");
        }
        let whole = decode(&tiled, 0, 300).unwrap();
        assert_eq!(at(&whole, 299, 199), [pixel(299, 199).as_slice(), &[255]].concat(), "the padded edge tiles");
    }

    /// An uncompressed RGB TIFF in `tile` x `tile` tiles (the tiff crate
    /// writes only strips).
    fn tiled_rgb(w: u32, h: u32, tile: u32, pixel: &dyn Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let (across, down) = (w.div_ceil(tile), h.div_ceil(tile));
        let mut out = b"II*\0\0\0\0\0".to_vec();
        let mut offsets = Vec::new();
        for ty in 0..down {
            for tx in 0..across {
                offsets.push(out.len() as u32);
                for y in ty * tile..(ty + 1) * tile {
                    for x in tx * tile..(tx + 1) * tile {
                        out.extend(if x < w && y < h { pixel(x, y) } else { [0; 3] });
                    }
                }
            }
        }
        let bits = out.len() as u32;
        out.extend([8u8, 0, 8, 0, 8, 0]);
        let offsets_at = out.len() as u32;
        offsets.iter().for_each(|o| out.extend(o.to_le_bytes()));
        let counts_at = out.len() as u32;
        offsets.iter().for_each(|_| out.extend((tile * tile * 3).to_le_bytes()));
        let ifd = out.len() as u32;
        out[4..8].copy_from_slice(&ifd.to_le_bytes());
        let n = offsets.len() as u32;
        // (tag, type: 3 short / 4 long, count, value or offset)
        let entries: [(u16, u16, u32, u32); 11] = [
            (256, 4, 1, w),
            (257, 4, 1, h),
            (258, 3, 3, bits),
            (259, 3, 1, 1),
            (262, 3, 1, 2),
            (277, 3, 1, 3),
            (284, 3, 1, 1),
            (322, 4, 1, tile),
            (323, 4, 1, tile),
            (324, 4, n, offsets_at),
            (325, 4, n, counts_at),
        ];
        out.extend((entries.len() as u16).to_le_bytes());
        for (tag, kind, count, value) in entries {
            out.extend(tag.to_le_bytes());
            out.extend(kind.to_le_bytes());
            out.extend(count.to_le_bytes());
            match (kind, count) {
                (3, 1) => out.extend([(value as u16).to_le_bytes(), [0, 0]].concat()),
                _ => out.extend(value.to_le_bytes()),
            }
        }
        out.extend(0u32.to_le_bytes());
        out
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

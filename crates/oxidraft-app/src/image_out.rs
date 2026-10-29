//! PNGOUT, JPGOUT and BMPOUT: the drawing area saved as an image, as it
//! shows on screen, without the cursor, grid, grips or selection.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, ColorImage};

use crate::app::OxidraftApp;

/// An image being saved: drawn without what's on top for a frame, then
/// captured; the capture reports how it went.
pub(crate) struct ImageOut {
    path: PathBuf,
    frames: u32,
    requested: bool,
    result: Arc<Mutex<Option<String>>>,
}

/// Pixels `[x0, y0, x1, y1]` of a screenshot saved to `path`, in the
/// format its extension names (PNG, JPEG or BMP).
pub(crate) fn save_crop(image: &ColorImage, crop: [usize; 4], path: &Path) -> Result<(), String> {
    let [w, h] = image.size;
    let (x0, y0) = (crop[0].min(w), crop[1].min(h));
    let (x1, y1) = (crop[2].clamp(x0, w), crop[3].clamp(y0, h));
    let format = image::ImageFormat::from_path(path).map_err(|e| e.to_string())?;
    // JPEG and BMP have no use for transparency.
    let alpha = format == image::ImageFormat::Png;
    let mut bytes = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        for p in &image.pixels[y * w + x0..y * w + x1] {
            let [r, g, b, a] = p.to_srgba_unmultiplied();
            bytes.extend_from_slice(&[r, g, b]);
            if alpha {
                bytes.push(a);
            }
        }
    }
    let color = if alpha { image::ExtendedColorType::Rgba8 } else { image::ExtendedColorType::Rgb8 };
    image::save_buffer_with_format(path, &bytes, (x1 - x0) as u32, (y1 - y0) as u32, color, format)
        .map_err(|e| e.to_string())
}

impl OxidraftApp {
    /// PNGOUT, JPGOUT, BMPOUT: where the image goes.
    pub(crate) fn image_out_command(&mut self, name: &str, extensions: &[&str]) {
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let dialog = rfd::FileDialog::new()
            .add_filter(format!("{name} image"), extensions)
            .set_file_name(format!("{stem}.{}", extensions[0]));
        if let Some(path) = dialog.save_file() {
            self.start_image_out(path);
        }
    }

    pub fn start_image_out(&mut self, path: PathBuf) {
        self.image_out = Some(ImageOut { path, frames: 0, requested: false, result: Arc::default() });
    }

    /// True while the drawing is drawn for an image.
    pub(crate) fn capturing_image(&self) -> bool {
        self.image_out.is_some()
    }

    /// True while an image is being saved (for tests).
    pub fn capturing(&self) -> bool {
        self.capturing_image()
    }

    /// Each frame while saving an image: once a frame has been drawn
    /// clean, the screenshot of the next; then what came of it.
    pub(crate) fn image_out_frame(&mut self, ctx: &egui::Context) {
        let Some(out) = &mut self.image_out else { return };
        let done = out.result.lock().ok().and_then(|mut r| r.take());
        if let Some(message) = done {
            self.image_out = None;
            self.engine.log(message);
            return;
        }
        out.frames += 1;
        if out.frames == 2 && !out.requested {
            out.requested = true;
            let ppp = ctx.pixels_per_point();
            let r = self.drawing_rect;
            let px = |v: f32| (v * ppp).round().max(0.0) as usize;
            let crop = [px(r.left()), px(r.top()), px(r.right()), px(r.bottom())];
            let (path, result) = (out.path.clone(), out.result.clone());
            ctx.request_screenshot(move |image| {
                let message = match save_crop(&image, crop, &path) {
                    Ok(()) => format!("Saved {}", path.display()),
                    Err(e) => format!("Could not save {}: {e}", path.display()),
                };
                if let Ok(mut r) = result.lock() {
                    *r = Some(message);
                }
            });
        }
        if out.frames > 240 {
            self.image_out = None;
            self.engine.log("Could not capture the drawing.".to_string());
            return;
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    #[test]
    fn crops_and_saves_by_extension() {
        let mut image = ColorImage::filled([4, 3], Color32::from_rgb(10, 20, 30));
        image.pixels[4 + 1] = Color32::RED;
        let dir = std::env::temp_dir().join(format!("oxidraft-pngout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for ext in ["png", "jpg", "bmp"] {
            let path = dir.join(format!("view.{ext}"));
            save_crop(&image, [1, 1, 3, 3], &path).unwrap();
            let back = image::open(&path).unwrap();
            assert_eq!((back.width(), back.height()), (2, 2), "{ext}");
        }
        let png = image::open(dir.join("view.png")).unwrap().to_rgba8();
        assert_eq!(png.get_pixel(0, 0).0, [255, 0, 0, 255]);
        std::fs::remove_dir_all(&dir).ok();
    }
}

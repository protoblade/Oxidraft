//! Image and PDF underlays (IMAGEATTACH, PDFATTACH): pictures shown in the
//! drawing from files beside it. The drawing keeps where each file is and
//! where its picture lies; the app reads the pixels when they're wanted.

use oxidraft_geom::Point;

use crate::{Document, Geometry, XrefStatus};

/// An attached image file or PDF page.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageDef {
    /// The name references use.
    pub name: String,
    /// Where the file is, as saved: a full path, one relative to this
    /// drawing's folder, or a bare file name.
    pub path: String,
    /// The page of a PDF (from 0); `None` for an image.
    pub page: Option<usize>,
    /// Pixels across and down, or a PDF page's size in points (0 until
    /// the file has been read).
    pub width: f64,
    pub height: f64,
    pub status: XrefStatus,
}

impl ImageDef {
    pub fn is_pdf(&self) -> bool {
        self.page.is_some()
    }
}

/// ADJUST: how a picture shows — brightness and contrast (0 to 100, 50 as
/// it is), fade toward the background (0 to 100), and, for a PDF, in
/// grays (monochrome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureAdjust {
    pub brightness: u8,
    pub contrast: u8,
    pub fade: u8,
    pub monochrome: bool,
}

impl Default for PictureAdjust {
    fn default() -> Self {
        PictureAdjust { brightness: 50, contrast: 50, fade: 0, monochrome: false }
    }
}

impl PictureAdjust {
    pub fn is_plain(&self) -> bool {
        *self == PictureAdjust::default()
    }

    /// What shows: for a PDF its fade and monochrome (its contrast is
    /// AutoCAD's underlay setting, kept as it is), for an image its
    /// brightness, contrast and fade.
    pub fn shown(&self, pdf: bool) -> PictureAdjust {
        if pdf {
            PictureAdjust { fade: self.fade, monochrome: self.monochrome, ..Default::default() }
        } else {
            PictureAdjust { monochrome: false, ..*self }
        }
    }

    /// One color channel (0 to 255) adjusted: contrast about the middle,
    /// brightness up or down, then faded toward `background`.
    pub fn channel(&self, c: u8, background: u8) -> u8 {
        let contrast = f64::from(self.contrast) / 50.0;
        let v = (f64::from(c) - 127.5) * contrast + 127.5 + (f64::from(self.brightness) - 50.0) * 2.55;
        let fade = f64::from(self.fade.min(100)) / 100.0;
        let v = v.clamp(0.0, 255.0) * (1.0 - fade) + f64::from(background) * fade;
        v.round().clamp(0.0, 255.0) as u8
    }

    /// RGBA pixels adjusted, faded toward a white background.
    pub fn apply(&self, rgba: &mut [u8]) {
        for px in rgba.as_chunks_mut::<4>().0 {
            if self.monochrome {
                let gray =
                    (0.299 * f64::from(px[0]) + 0.587 * f64::from(px[1]) + 0.114 * f64::from(px[2])).round() as u8;
                px[..3].fill(gray);
            }
            for c in &mut px[..3] {
                *c = self.channel(*c, 255);
            }
        }
    }
}

/// The corners of a picture from its lower-left corner and its edge
/// vectors: lower-left, lower-right, upper-right, upper-left.
pub fn image_corners(origin: Point, u: Point, v: Point) -> [Point; 4] {
    [origin, origin + u, origin + u + v, origin + v]
}

impl Document {
    /// How many pictures in the drawing show `name`.
    pub fn image_references(&self, name: &str) -> usize {
        self.entities()
            .filter(|e| matches!(&e.geometry, Geometry::Image { image, .. } if image.eq_ignore_ascii_case(name)))
            .count()
    }

    /// DETACH: removes an image or PDF underlay and its references.
    pub fn detach_image(&mut self, name: &str) {
        let ids: Vec<_> = self
            .entities()
            .filter(|e| matches!(&e.geometry, Geometry::Image { image, .. } if image.eq_ignore_ascii_case(name)))
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.remove(id);
        }
        self.remove_image_def(name);
    }

    /// A PDF page's size has been read: references read from a file, whose
    /// size was only known as a scale (one unit of scale an inch), take
    /// their real size.
    pub fn size_pdf(&mut self, name: &str, width: f64, height: f64) {
        let Some(mut def) = self.image_def(name).cloned() else { return };
        if def.width > 0.0 && def.height > 0.0 {
            return;
        }
        let ids: Vec<_> = self
            .entities()
            .filter(|e| matches!(&e.geometry, Geometry::Image { image, .. } if image.eq_ignore_ascii_case(name)))
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.update(id, |e| {
                if let Geometry::Image { u, v, .. } = &mut e.geometry {
                    *u = *u * (width / 72.0);
                    *v = *v * (height / 72.0);
                }
            });
        }
        (def.width, def.height) = (width, height);
        self.set_image_def(def);
    }
}

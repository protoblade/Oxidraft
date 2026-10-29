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

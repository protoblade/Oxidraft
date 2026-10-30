//! Handwriting on a marked-up plot read as text, as Markup Assist reads
//! it but without Autodesk's cloud: macOS's own text recognition (Vision)
//! over the plot's colored ink alone (a red or blue pen's), so the plot's
//! own black lines and text are left out. Elsewhere it isn't read.

use std::sync::mpsc::{Receiver, TryRecvError};

use oxidraft_geom::Point;
use oxidraft_model::{Color, Geometry, Space};
use oxidraft_raster::Pixels;

use crate::app::OxidraftApp;

/// Handwriting being read in the background: what it's read from, and
/// where that picture lies (its corner and sides) in which space.
pub(crate) struct Pending {
    pub done: Receiver<Result<Vec<Read>, String>>,
    pub origin: Point,
    pub u: Point,
    pub v: Point,
    pub space: Space,
}

/// A line of text read, and its box from the picture's lower-left corner
/// in fractions of its width and height.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub text: String,
    pub min: (f64, f64),
    pub max: (f64, f64),
}

/// The picture's colored ink (far from gray) as black on white; `None`
/// when it has none.
pub fn ink(p: &Pixels) -> Option<image::GrayImage> {
    let mut any = false;
    let mask = image::GrayImage::from_fn(p.width, p.height, |x, y| {
        let i = (y as usize * p.width as usize + x as usize) * 4;
        let [r, g, b, a] = [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2], p.rgba[i + 3]];
        let spread = r.max(g).max(b) - r.min(g).min(b);
        let colored = a > 128 && spread > 80;
        any |= colored;
        image::Luma([if colored { 0 } else { 255 }])
    });
    any.then_some(mask)
}

/// The text written in the colored ink of picture `p`.
pub fn read_markups(p: &Pixels) -> Result<Vec<Read>, String> {
    let Some(mask) = ink(p) else { return Ok(Vec::new()) };
    let mut png = Vec::new();
    mask.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    recognize(&png)
}

impl OxidraftApp {
    /// Reads the handwriting on the picture `file` (an image, or a PDF's
    /// first page) in the background, its text to go where it's written
    /// over the picture placed at `origin` with sides `u` and `v`.
    pub(crate) fn read_handwriting(&mut self, file: &std::path::Path, origin: Point, u: Point, v: Point) {
        let (send, done) = std::sync::mpsc::channel();
        let file = file.to_path_buf();
        std::thread::spawn(move || {
            let _ = send.send(oxidraft_raster::decode(&file, 0, 3000).and_then(|p| read_markups(&p)));
        });
        self.handwriting = Some(Pending { done, origin, u, v, space: self.doc.space() });
    }

    /// Whether handwriting is still being read.
    pub fn reading_handwriting(&self) -> bool {
        self.handwriting.is_some()
    }

    /// The handwriting read, once it is: as text on layer MARKUP where
    /// it's written, as Markup Assist offers it.
    pub(crate) fn take_handwriting(&mut self, ctx: &eframe::egui::Context) {
        let Some(p) = &self.handwriting else { return };
        let read = match p.done.try_recv() {
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
            Err(TryRecvError::Disconnected) => Err(String::new()),
            Ok(read) => read,
        };
        let Some(p) = self.handwriting.take() else { return };
        let read = match read {
            Ok(read) if !read.is_empty() => read,
            Ok(_) => return,
            Err(e) => {
                if !e.is_empty() {
                    self.engine.log(e);
                }
                return;
            }
        };
        let was = self.doc.space();
        self.doc.set_space(p.space);
        self.doc.begin("MARKUP");
        let n = read.len();
        for r in read {
            let (x, y) = (r.min.0, (r.min.1 + r.max.1) / 2.0);
            let text = Geometry::Text {
                position: p.origin + p.u * x + p.v * y,
                height: (r.max.1 - r.min.1) * p.v.length() * 0.55,
                rotation: 0.0,
                width: 0.0,
                attach: oxidraft_model::Attach::MiddleLeft,
                content: r.text,
                look: Default::default(),
            };
            self.doc.add_on_layer(text, "MARKUP", Color::ByLayer);
        }
        self.doc.commit();
        self.doc.set_space(was);
        self.engine.log(format!("{n} line(s) of handwriting on the markup read as text on layer MARKUP."));
    }
}

#[cfg(target_os = "macos")]
fn recognize(png: &[u8]) -> Result<Vec<Read>, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSObjectNSKeyValueCoding, NSValue, ns_string};
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};
    objc2::rc::autoreleasepool(|_| {
        let data = NSData::with_bytes(png);
        let handler =
            VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &NSDictionary::new());
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        let requests: &VNRequest = &request;
        handler
            .performRequests_error(&NSArray::from_slice(&[requests]))
            .map_err(|e| e.localizedDescription().to_string())?;
        let mut out = Vec::new();
        for o in request.results().unwrap_or_default().iter() {
            let Some(best) = o.topCandidates(1).firstObject() else { continue };
            // Its box, asked for by name: a rectangle in an NSValue.
            let r = o.valueForKey(ns_string!("boundingBox")).and_then(|v| v.downcast::<NSValue>().ok());
            let Some(r) = r.and_then(|v| v.get_rect()) else { continue };
            out.push(Read {
                text: best.string().to_string(),
                min: (r.origin.x, r.origin.y),
                max: (r.origin.x + r.size.width, r.origin.y + r.size.height),
            });
        }
        Ok(out)
    })
}

#[cfg(not(target_os = "macos"))]
fn recognize(_: &[u8]) -> Result<Vec<Read>, String> {
    Err("Handwriting is read on macOS only.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF with red text over black.
    fn plot() -> Vec<u8> {
        let content =
            "BT /F1 40 Tf 1 0 0 rg 60 300 Td (Move door here) Tj ET BT /F1 40 Tf 0 0 0 rg 60 120 Td (PLAN VIEW) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes(),
        );
        out
    }

    #[test]
    fn the_colored_ink_alone() {
        let p = Pixels { width: 3, height: 1, rgba: vec![200, 20, 20, 255, 30, 30, 30, 255, 250, 250, 250, 255] };
        let mask = ink(&p).unwrap();
        assert_eq!(mask.as_raw(), &[0, 255, 255], "the red pixel alone");
        assert!(ink(&Pixels { width: 1, height: 1, rgba: vec![0, 0, 0, 255] }).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_red_text_read_where_it_is() {
        let p = oxidraft_raster::render_pdf_data(plot(), 0, 150.0).unwrap();
        let read = read_markups(&p).unwrap();
        assert_eq!(read.len(), 1, "{read:?}");
        assert_eq!(read[0].text, "Move door here");
        // Its box: from x 60 of 600, its baseline at 300 of 400.
        assert!((read[0].min.0 - 0.1).abs() < 0.03, "{read:?}");
        assert!((read[0].min.1 - 0.75).abs() < 0.05, "{read:?}");
    }
}

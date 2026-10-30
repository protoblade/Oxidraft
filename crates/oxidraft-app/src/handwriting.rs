//! Handwriting on a marked-up plot read as text, as Markup Assist reads
//! it but without Autodesk's cloud: the system's own text recognition
//! (Vision on macOS, Windows.Media.Ocr on Windows, Tesseract elsewhere
//! where it's installed) over the plot's colored ink, and over all of it
//! less the text the drawing plots there (a black pen's writing).

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

/// A line's words, lower case, letters and digits only.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// Whether two boxes overlap.
fn overlap(a: &Read, b: &Read) -> bool {
    a.min.0 < b.max.0 && b.min.0 < a.max.0 && a.min.1 < b.max.1 && b.min.1 < a.max.1
}

/// The text written on picture `p`: in colored ink, and in any ink where
/// what's read isn't mostly words the plot itself has (`plotted`, the
/// text the drawing plots there), as a black pen's writing isn't.
pub fn read_markups(p: &Pixels, plotted: &[String]) -> Result<Vec<Read>, String> {
    read_markups_by(p, plotted, &recognize)
}

/// What reads a picture (a PNG): its lines of text.
type Recognizer<'a> = dyn Fn(&[u8]) -> Result<Vec<Read>, String> + 'a;

/// [`read_markups`], each picture read by `recognize`.
fn read_markups_by(p: &Pixels, plotted: &[String], recognize: &Recognizer<'_>) -> Result<Vec<Read>, String> {
    let png = |img: &image::GrayImage| {
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map(|_| png)
            .map_err(|e| e.to_string())
    };
    let mut out = match ink(p) {
        Some(mask) => recognize(&png(&mask)?)?,
        None => Vec::new(),
    };
    // Everything written, in grays, less the plot's own.
    let gray = image::GrayImage::from_fn(p.width, p.height, |x, y| {
        let i = (y as usize * p.width as usize + x as usize) * 4;
        let [r, g, b, a] = [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2], p.rgba[i + 3]];
        let lum = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
        image::Luma([if a < 128 { 255 } else { lum as u8 }])
    });
    let known: std::collections::HashSet<String> = plotted.iter().flat_map(|t| words(t)).collect();
    for r in recognize(&png(&gray)?)? {
        let w = words(&r.text);
        let new = w.iter().filter(|w| !known.contains(*w)).count();
        if new * 2 > w.len() && !out.iter().any(|o| overlap(o, &r)) {
            out.push(r);
        }
    }
    Ok(out)
}

impl OxidraftApp {
    /// Reads the handwriting on the picture `file` (an image, or a PDF's
    /// first page; `plotted` the text the drawing plots on it) in the
    /// background, its text to go where it's written over the picture
    /// placed at `origin` with sides `u` and `v`.
    pub(crate) fn read_handwriting(
        &mut self,
        file: &std::path::Path,
        plotted: Vec<String>,
        origin: Point,
        u: Point,
        v: Point,
    ) {
        let (send, done) = std::sync::mpsc::channel();
        let file = file.to_path_buf();
        std::thread::spawn(move || {
            let _ = send.send(oxidraft_raster::decode(&file, 0, 3000).and_then(|p| read_markups(&p, &plotted)));
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

/// Windows' own text recognition (Windows.Media.Ocr), in the languages
/// the user's profile has.
#[cfg(windows)]
fn recognize(png: &[u8]) -> Result<Vec<Read>, String> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    let run = || -> windows::core::Result<Vec<Read>> {
        let stream = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&stream)?;
        writer.WriteBytes(png)?;
        writer.StoreAsync()?.join()?;
        writer.FlushAsync()?.join()?;
        writer.DetachStream()?;
        stream.Seek(0)?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
        let bitmap = decoder.GetSoftwareBitmapAsync()?.join()?;
        let bitmap =
            SoftwareBitmap::ConvertWithAlpha(&bitmap, BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied)?;
        let (w, h) = (f64::from(bitmap.PixelWidth()?), f64::from(bitmap.PixelHeight()?));
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut out = Vec::new();
        for line in result.Lines()? {
            // The line's box: its words' together (top-left origin, pixels).
            let mut bounds: Option<(f64, f64, f64, f64)> = None;
            for word in line.Words()? {
                let r = word.BoundingRect()?;
                let (x0, y0) = (f64::from(r.X), f64::from(r.Y));
                let (x1, y1) = (x0 + f64::from(r.Width), y0 + f64::from(r.Height));
                bounds = Some(match bounds {
                    Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
                    None => (x0, y0, x1, y1),
                });
            }
            let Some((x0, y0, x1, y1)) = bounds else { continue };
            out.push(Read { text: line.Text()?.to_string(), min: (x0 / w, 1.0 - y1 / h), max: (x1 / w, 1.0 - y0 / h) });
        }
        Ok(out)
    };
    run().map_err(|e| format!("Handwriting couldn't be read: {}", e.message()))
}

/// Elsewhere, Tesseract's (the `tesseract` command), where it's installed.
#[cfg(not(any(target_os = "macos", windows)))]
fn recognize(png: &[u8]) -> Result<Vec<Read>, String> {
    let file = std::env::temp_dir().join(format!("oxidraft-markup-{}.png", std::process::id()));
    std::fs::write(&file, png).map_err(|e| e.to_string())?;
    let size = image::load_from_memory(png).map(|i| (i.width(), i.height())).map_err(|e| e.to_string())?;
    let run = std::process::Command::new("tesseract").arg(&file).args(["stdout", "tsv"]).output();
    std::fs::remove_file(&file).ok();
    let out = run.map_err(|_| "Handwriting is read by tesseract, which isn't installed.".to_string())?;
    Ok(tesseract_lines(&String::from_utf8_lossy(&out.stdout), size))
}

/// Tesseract's TSV (a row a word: level, page, block, paragraph, line,
/// word, left, top, width, height, confidence, text) as lines of text, on
/// a picture `size` pixels big.
#[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
fn tesseract_lines(tsv: &str, (w, h): (u32, u32)) -> Vec<Read> {
    // Each line: its block, paragraph and number, its words, its box.
    type Line = ((u32, u32, u32), Vec<String>, (f64, f64, f64, f64));
    let mut lines: Vec<Line> = Vec::new();
    for row in tsv.lines().skip(1) {
        let f: Vec<&str> = row.split('\t').collect();
        let [level, _, block, par, line, _, left, top, width, height, _, text] = f[..] else { continue };
        let num = |v: &str| v.trim().parse::<f64>().unwrap_or(0.0);
        if level.trim() != "5" || text.trim().is_empty() {
            continue;
        }
        let key = (num(block) as u32, num(par) as u32, num(line) as u32);
        let (x0, y0) = (num(left), num(top));
        let (x1, y1) = (x0 + num(width), y0 + num(height));
        match lines.iter_mut().find(|l| l.0 == key) {
            Some(l) => {
                l.1.push(text.trim().to_string());
                l.2 = (l.2.0.min(x0), l.2.1.min(y0), l.2.2.max(x1), l.2.3.max(y1));
            }
            None => lines.push((key, vec![text.trim().to_string()], (x0, y0, x1, y1))),
        }
    }
    let (w, h) = (f64::from(w.max(1)), f64::from(h.max(1)));
    lines
        .into_iter()
        .map(|(_, words, (x0, y0, x1, y1))| Read {
            text: words.join(" "),
            min: (x0 / w, 1.0 - y1 / h),
            max: (x1 / w, 1.0 - y0 / h),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF with red text, the plot's black text and something
    /// written in black.
    fn plot() -> Vec<u8> {
        let content = "BT /F1 40 Tf 1 0 0 rg 60 300 Td (Move door here) Tj ET \
                       BT /F1 40 Tf 0 0 0 rg 60 120 Td (PLAN VIEW) Tj ET \
                       BT /F1 32 Tf 0 0 0 rg 330 40 Td (Check this) Tj ET";
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
    fn tesseracts_words_made_lines() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   4\t1\t1\t1\t1\t0\t10\t20\t200\t30\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t10\t20\t80\t30\t91\tMove\n\
                   5\t1\t1\t1\t1\t2\t100\t22\t110\t28\t90\tdoor\n\
                   5\t1\t2\t1\t1\t1\t50\t300\t60\t20\t88\tOK\n";
        let lines = tesseract_lines(tsv, (400, 400));
        assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Move door", "OK"]);
        assert_eq!((lines[0].min, lines[0].max), ((0.025, 0.875), (0.525, 0.95)));
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
        // The plot has PLAN VIEW: what else is written is markup, in red
        // or in black.
        let read = read_markups(&p, &["PLAN VIEW".to_string()]).unwrap();
        let texts: Vec<&str> = read.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["Move door here", "Check this"], "{read:?}");
        // Its box: from x 60 of 600, its baseline at 300 of 400.
        assert!((read[0].min.0 - 0.1).abs() < 0.03, "{read:?}");
        assert!((read[0].min.1 - 0.75).abs() < 0.05, "{read:?}");
    }

    /// The flow every system's recognition goes through, with Tesseract's
    /// rows (as its TSV gives them, made here for this plot's words): the
    /// red ink's line, then the grays' less the plot's own and what the ink
    /// already gave.
    #[test]
    fn markups_from_tesseracts_rows() {
        let p = oxidraft_raster::render_pdf_data(plot(), 0, 150.0).unwrap();
        let size = (p.width, p.height);
        const HEAD: &str =
            "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";
        let ink = "5\t1\t1\t1\t1\t1\t125\t148\t190\t60\t93\tMove\n\
                   5\t1\t1\t1\t1\t2\t330\t148\t150\t60\t92\tdoor\n\
                   5\t1\t1\t1\t1\t3\t495\t148\t210\t60\t94\there\n";
        let gray = "5\t1\t1\t1\t1\t1\t125\t148\t190\t60\t90\tMove\n\
                    5\t1\t1\t1\t1\t2\t330\t148\t150\t60\t91\tdoor\n\
                    5\t1\t1\t1\t1\t3\t495\t148\t210\t60\t92\there\n\
                    5\t1\t2\t1\t1\t1\t125\t523\t150\t60\t96\tPLAN\n\
                    5\t1\t2\t1\t1\t2\t290\t523\t140\t60\t95\tVIEW\n\
                    5\t1\t3\t1\t1\t1\t687\t702\t160\t50\t89\tCheck\n\
                    5\t1\t3\t1\t1\t2\t860\t702\t90\t50\t90\tthis\n";
        let calls = std::cell::Cell::new(0);
        let read = read_markups_by(&p, &["PLAN VIEW".to_string()], &|png| {
            let picture = image::load_from_memory(png).map_err(|e| e.to_string())?;
            assert_eq!((picture.width(), picture.height()), size);
            calls.set(calls.get() + 1);
            let rows = if calls.get() == 1 { ink } else { gray };
            Ok(tesseract_lines(&format!("{HEAD}{rows}"), size))
        })
        .unwrap();
        assert_eq!(calls.get(), 2, "the red ink, then all of it");
        let texts: Vec<&str> = read.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["Move door here", "Check this"], "{read:?}");
        assert!((read[0].min.0 - 0.1).abs() < 0.03 && (read[0].min.1 - 0.75).abs() < 0.05, "{read:?}");
    }
}

//! SHARE and MARKUP without Autodesk's cloud. SHARE packs what someone
//! else needs to look at the drawing into one zip file to send: the
//! drawing as DWG, a PDF of its sheets, and the files it uses. MARKUP
//! (MARKUPIMPORT) brings a marked-up plot back: the PDF or image laid over
//! the sheet it was plotted from, on layer MARKUP, to work from, and a
//! PDF's markups (comments as text, drawn markups as lines) with it, as
//! Markup Assist offers them.

use std::path::Path;

use oxidraft_geom::Point;
use oxidraft_model::{Color, Geometry, ImageDef, Layer, MODEL, XrefStatus};
use oxidraft_plot::{PlotArea, Sheet};

use crate::app::OxidraftApp;

/// A name no one guesses: 16 hex digits from the system's randomness.
fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let part = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("{:08x}{:08x}", part() as u32, part() as u32)
}

/// `files` put at `location` — a folder, or an http(s) folder they're
/// PUT in (`user:password@` in it signs in) — and the links to them:
/// under `link`, or the location itself.
pub(crate) fn upload(location: &str, link: &str, files: &[(String, Vec<u8>)]) -> Result<Vec<String>, String> {
    let join = |base: &str, name: &str| format!("{}/{name}", base.trim_end_matches('/'));
    let location = location.trim();
    let web = location.starts_with("http://") || location.starts_with("https://");
    let base = if web {
        // The sign-in taken out of the address.
        let (scheme, rest) = location.split_once("://").unwrap_or(("https", location));
        let (host, path) = rest.split_once('/').map_or((rest, ""), |(h, p)| (h, p));
        let (user, host) = match host.rsplit_once('@') {
            Some((user, host)) => (Some(user), host),
            None => (None, host),
        };
        let plain = format!("{scheme}://{host}/{path}");
        let agent: ureq::Agent =
            ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(120))).build().into();
        for (name, bytes) in files {
            let mut put = agent.put(&join(&plain, name)).header("User-Agent", "oxidraft");
            if let Some(user) = user {
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(user);
                put = put.header("Authorization", format!("Basic {encoded}"));
            }
            put.send(&bytes[..]).map_err(|e| format!("Could not upload {name}: {e}"))?;
        }
        plain
    } else {
        let folder = match location.strip_prefix("~/") {
            Some(rest) => std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join(rest),
            None => std::path::PathBuf::from(location),
        };
        std::fs::create_dir_all(&folder).map_err(|e| format!("Could not make {}: {e}", folder.display()))?;
        for (name, bytes) in files {
            std::fs::write(folder.join(name), bytes).map_err(|e| format!("Could not write {name}: {e}"))?;
        }
        format!("file://{}", folder.display())
    };
    let base = if link.trim().is_empty() { base } else { link.trim().to_string() };
    Ok(files.iter().map(|(name, _)| join(&base, name)).collect())
}

impl OxidraftApp {
    /// SHARE: with a share location set, the drawing's sheets and package
    /// put there and a link to them given (in the background); otherwise
    /// asks where the package goes, then writes it.
    pub(crate) fn share(&mut self) {
        if !self.settings.share_location.trim().is_empty() {
            if let Err(e) = self.share_online() {
                self.engine.log(e);
            }
            return;
        }
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(path) = rfd::FileDialog::new()
            .set_title("Share: the package to send")
            .add_filter("Zip", &["zip"])
            .set_file_name(format!("{stem} shared.zip"))
            .save_file()
        else {
            return;
        };
        match self.share_to(&path) {
            Ok(n) => self.engine.log(format!("Share package of {n} file(s) written to {}", path.display())),
            Err(e) => self.engine.log(e),
        }
    }

    /// Writes the share package to `path`; how many files are in it.
    pub fn share_to(&mut self, path: &Path) -> Result<usize, String> {
        let files = self.share_files()?;
        let n = files.len();
        std::fs::write(path, oxidraft_plot::zip::zip(&files))
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(n)
    }

    /// SHARE to the share location: the sheets as a PDF to look at, and the
    /// package, under a name no one guesses; the links once they're there.
    pub fn share_online(&mut self) -> Result<(), String> {
        let files = self.share_files()?;
        let stem: String = Path::new(&self.file_name())
            .file_stem()
            .map(|s| s.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect())
            .unwrap_or_default();
        let name = format!("{stem}-{}", token());
        let mut out = Vec::new();
        if let Some((_, pdf)) = files.iter().find(|(n, _)| n.ends_with(".pdf")) {
            out.push((format!("{name}.pdf"), pdf.clone()));
        }
        out.push((format!("{name}.zip"), oxidraft_plot::zip::zip(&files)));
        let (location, link) = (self.settings.share_location.clone(), self.settings.share_link.clone());
        let (send, done) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(upload(&location, &link, &out));
        });
        self.sharing = Some(done);
        self.engine.log("Sharing...");
        Ok(())
    }

    /// Where SHARE shares to, and the address that's seen at (the
    /// Options dialog's Share).
    pub fn set_share_location(&mut self, location: &str, link: &str) {
        let settings = crate::settings::Settings {
            share_location: location.to_string(),
            share_link: link.to_string(),
            ..self.settings.clone()
        };
        self.keep_settings(settings);
    }

    /// Whether SHARE is still uploading.
    pub fn sharing(&self) -> bool {
        self.sharing.is_some()
    }

    /// SHARE's links, once it's done: the first (the sheets to look at)
    /// copied to the clipboard.
    pub(crate) fn take_share(&mut self, ctx: &eframe::egui::Context) {
        let Some(done) = &self.sharing else { return };
        let result = match done.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("Sharing stopped.".to_string()),
            Ok(result) => result,
        };
        self.sharing = None;
        match result {
            Ok(links) => {
                if let Some(first) = links.first() {
                    ctx.copy_text(first.clone());
                }
                self.engine.log(format!("Shared (the link is on the clipboard): {}", links.join("  ")));
            }
            Err(e) => self.engine.log(e),
        }
    }

    /// What SHARE shares: the drawing as DWG, a PDF of its sheets, and the
    /// files it uses.
    fn share_files(&mut self) -> Result<Vec<(String, Vec<u8>)>, String> {
        let stem =
            Path::new(&self.file_name()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let dwg = oxidraft_io_dwg::write(&self.doc, self.dwg_version.unwrap_or_default()).map_err(|e| e.to_string())?;
        files.push((format!("{stem}.dwg"), dwg));
        // Its sheets: the layouts, or what PLOT plots in model space.
        let spaces: Vec<_> = match self.doc.layouts().iter().map(|l| l.id).collect::<Vec<_>>() {
            l if l.is_empty() => vec![MODEL],
            l => l,
        };
        let sheets: Vec<Sheet> = {
            let pictures = self.plot_pictures(true);
            spaces
                .iter()
                .filter_map(|&s| {
                    let mut settings = self.plot_settings;
                    settings.area = if s == MODEL { PlotArea::Extents } else { PlotArea::Layout };
                    oxidraft_plot::render_with_pictures(&self.doc, s, &settings, &pictures)
                })
                .collect()
        };
        if !sheets.is_empty() {
            files.push((format!("{stem}.pdf"), oxidraft_plot::pdf_pages(&sheets, &self.file_name())));
        }
        // The files it uses, by name, as they're found.
        let host = self.path.clone();
        let saved: Vec<String> =
            self.doc.xrefs().map(|x| x.path.clone()).chain(self.doc.image_defs().map(|d| d.path.clone())).collect();
        for s in saved {
            if let Some(found) = crate::xref::resolve(host.as_deref(), &s)
                && let Ok(data) = std::fs::read(&found)
            {
                let name = found.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if !files.iter().any(|(n, _)| *n == name) {
                    files.push((name, data));
                }
            }
        }
        Ok(files)
    }

    /// MARKUP: picks the marked-up PDF or image.
    pub(crate) fn markup_import(&mut self) {
        let Some(file) = rfd::FileDialog::new()
            .set_title("Markup Import: the marked-up plot")
            .add_filter("PDF, DWF or image", &["pdf", "dwf", "dwfx", "png", "jpg", "jpeg", "tif", "tiff", "bmp"])
            .pick_file()
        else {
            return;
        };
        if let Err(e) = self.markup_import_file(&file) {
            self.engine.log(e);
        }
    }

    /// Lays the marked-up plot `file` over the sheet it was plotted from:
    /// the layout shown on its page setup, or model space as the last plot
    /// placed it; on layer MARKUP (red), on top.
    pub fn markup_import_file(&mut self, file: &Path) -> Result<(), String> {
        let space = self.shown_space();
        let mut settings = self.plot_settings;
        settings.area = match settings.area {
            _ if space != MODEL => PlotArea::Layout,
            PlotArea::Display(_) => PlotArea::Display(self.displayed()),
            PlotArea::Layout => PlotArea::Extents,
            area => area,
        };
        let layout = settings.layout(&self.doc, space).ok_or("There's no sheet here to lay it over.")?;
        // A DWFx markup: its lines and text, in red on layer MARKUP.
        if file.extension().is_some_and(|e| ["dwfx", "dwf", "xps"].iter().any(|x| e.eq_ignore_ascii_case(x))) {
            let data = std::fs::read(file).map_err(|e| format!("Could not read {}: {e}", file.display()))?;
            let objects = crate::dwf_in::read_any_dwf(&data)?;
            let origin = layout.window.min - layout.origin * (1.0 / layout.scale);
            let placement =
                oxidraft_model::Placement { position: origin, scale: 1.0 / layout.scale, rotation: 0.0, mirror: false };
            self.engine.cancel(&mut self.doc);
            self.doc.begin("MARKUP");
            if self.doc.layer("MARKUP").is_none() {
                self.doc.add_layer(Layer::new("MARKUP", 1));
            }
            let n = objects.len();
            for (g, _) in objects {
                self.doc.add_on_layer(placement.place(Point::ORIGIN, &g), "MARKUP", Color::ByLayer);
            }
            self.doc.commit();
            self.engine.log(format!("{n} markup object(s) laid over the sheet on layer MARKUP."));
            return Ok(());
        }
        let pdf = oxidraft_raster::is_pdf(file);
        let info = oxidraft_raster::probe(file, 0)?;
        // The page over the whole sheet (an image's height as its shape has it).
        let (w_mm, h_mm) = if pdf {
            (info.width / 72.0 * 25.4, info.height / 72.0 * 25.4)
        } else {
            (layout.width, layout.width * info.height / info.width.max(1.0))
        };
        let to_drawing = |p: Point| layout.window.min + (p - layout.origin) * (1.0 / layout.scale);
        let origin = to_drawing(Point::ORIGIN);
        let u = to_drawing(Point::new(w_mm, 0.0)) - origin;
        let v = to_drawing(Point::new(0.0, h_mm)) - origin;
        let name = {
            let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Markup".into());
            let base = oxidraft_model::sanitize_name(&format!("Markup {stem}"));
            (1..)
                .map(|i| if i == 1 { base.clone() } else { format!("{base} ({i})") })
                .find(|n| self.doc.image_def(n).is_none())
                .unwrap_or(base)
        };
        let def = ImageDef {
            name: name.clone(),
            path: file.to_string_lossy().into_owned(),
            page: pdf.then_some(0),
            width: info.width,
            height: info.height,
            status: XrefStatus::Loaded,
        };
        self.engine.cancel(&mut self.doc);
        self.doc.begin("MARKUP");
        if self.doc.layer("MARKUP").is_none() {
            self.doc.add_layer(Layer::new("MARKUP", 1));
        }
        self.doc.set_image_def(def.clone());
        let image = Geometry::Image {
            adjust: oxidraft_model::PictureAdjust { transparent: true, ..Default::default() },
            image: name.clone(),
            origin,
            u,
            v,
            clip: Vec::new(),
        };
        self.doc.add_on_layer(image, "MARKUP", Color::ByLayer);
        // A PDF's markups — its comments and what reviewers drew — as text
        // and lines on layer MARKUP, as Markup Assist offers them.
        let mut markups = 0;
        if pdf && let Ok(data) = std::fs::read(file) {
            let mm = 25.4 / 72.0;
            let at = |(x, y): (f64, f64)| to_drawing(Point::new(x * mm, y * mm));
            for m in oxidraft_raster::pdf_markups::pdf_markups(&data, 0) {
                let (g, rgb) = match m {
                    oxidraft_raster::pdf_markups::Markup::Text { at: p, height, text, color } => (
                        Geometry::Text {
                            position: at(p),
                            height: height * mm * 0.7 / layout.scale,
                            rotation: 0.0,
                            width: 0.0,
                            attach: oxidraft_model::Attach::TopLeft,
                            content: text.lines().collect::<Vec<_>>().join("\\P"),
                            look: Default::default(),
                        },
                        color,
                    ),
                    oxidraft_raster::pdf_markups::Markup::Shape { points, closed, color } => {
                        let pts: Vec<Point> = points.into_iter().map(at).collect();
                        let g = if pts.len() == 2 && !closed {
                            Geometry::Line { start: pts[0], end: pts[1] }
                        } else {
                            Geometry::Polyline {
                                vertices: pts.into_iter().map(oxidraft_model::Vertex::at).collect(),
                                closed,
                            }
                        };
                        (g, color)
                    }
                };
                self.doc.add_on_layer(g, "MARKUP", Color::Index(crate::pdf_import::nearest_aci(rgb)));
                markups += 1;
            }
        }
        self.doc.commit();
        self.load_image(&name);
        let taken = if markups > 0 { format!(", its {markups} markup(s) as text and lines") } else { String::new() };
        self.engine.log(format!(
            "Markup \"{name}\" laid over the sheet on layer MARKUP{taken}: turn the layer off to compare, PDFIMPORT to take its lines."
        ));
        // What's handwritten on it in a colored pen, as text where it's
        // written (read in the background).
        self.read_handwriting(file, origin, u, v);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    fn files() -> Vec<(String, Vec<u8>)> {
        vec![("plan-1a2b.pdf".into(), b"%PDF".to_vec()), ("plan-1a2b.zip".into(), b"PK\x03\x04".to_vec())]
    }

    #[test]
    fn shared_into_a_folder_with_its_address() {
        let dir = std::env::temp_dir().join(format!("oxidraft-share-{}", std::process::id()));
        let links = upload(&dir.join("public").to_string_lossy(), "https://files.example.com/s/", &files()).unwrap();
        assert_eq!(links, ["https://files.example.com/s/plan-1a2b.pdf", "https://files.example.com/s/plan-1a2b.zip"]);
        assert_eq!(std::fs::read(dir.join("public/plan-1a2b.zip")).unwrap(), b"PK\x03\x04");
        // No address of its own: the folder's.
        let links = upload(&dir.to_string_lossy(), "", &files()).unwrap();
        assert_eq!(links[0], format!("file://{}/plan-1a2b.pdf", dir.display()));
        std::fs::remove_dir_all(&dir).ok();
        assert_ne!(token(), token(), "names no one guesses");
    }

    #[test]
    fn shared_to_a_web_folder_by_put() {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let seen = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..2 {
                let (stream, _) = server.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = Vec::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    head.push(line.trim().to_string());
                }
                let length = head
                    .iter()
                    .find_map(|h| {
                        h.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).unwrap();
                let mut stream = stream;
                stream.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                seen.push((head, body));
            }
            seen
        });
        let location = format!("http://me:secret@127.0.0.1:{port}/dav/shared");
        let links = upload(&location, "", &files()).unwrap();
        assert_eq!(links[0], format!("http://127.0.0.1:{port}/dav/shared/plan-1a2b.pdf"), "without the sign-in");
        let seen = seen.join().unwrap();
        assert_eq!(seen[0].0[0], "PUT /dav/shared/plan-1a2b.pdf HTTP/1.1");
        // "me:secret" in base64.
        assert!(
            seen[0]
                .0
                .iter()
                .any(|h| h == "authorization: Basic bWU6c2VjcmV0" || h == "Authorization: Basic bWU6c2VjcmV0")
        );
        assert_eq!(seen[1].1, b"PK\x03\x04");
    }
}

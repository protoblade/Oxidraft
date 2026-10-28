//! Image and PDF underlays in the app: their pixels read in the background
//! and only when asked for (so many GeoTIFF tiles can be attached at once
//! without opening any), IMAGEATTACH and PDFATTACH's dialog, attaching
//! many georeferenced images in one go, and drawing pictures.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Painter, Pos2, Rect, Shape, Stroke};
use oxidraft_commands::ViewRequest;
use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Color, Geometry, ImageDef, XrefStatus, hatch, image_corners, sanitize_name};
use oxidraft_plot::Picture;
use oxidraft_raster::Info;

use crate::{
    app::OxidraftApp,
    theme,
    xref::{PathType, resolve},
};

/// Pictures on screen are scaled down to at most this many pixels a side.
const SCREEN_SIDE: u32 = 2048;
/// Plots read them again, sharper.
const PLOT_SIDE: u32 = 4096;
/// Pictures read at once in the background.
const WORKERS: usize = 2;

/// A picture's pixels, as far as they've been read.
pub(crate) enum Raster {
    Loading,
    Ready(egui::TextureHandle),
    Failed(String),
}

struct Job {
    key: String,
    path: PathBuf,
    page: usize,
}

type Decoded = (String, Result<oxidraft_raster::Pixels, String>);

/// Reads pictures on background threads, a couple at a time.
pub(crate) struct Loader {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Decoded>,
}

impl Default for Loader {
    fn default() -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        let (finished, done) = mpsc::channel::<Decoded>();
        let queue = Arc::new(Mutex::new(queue));
        for _ in 0..WORKERS {
            let (queue, finished) = (queue.clone(), finished.clone());
            std::thread::spawn(move || {
                loop {
                    let next = match queue.lock() {
                        Ok(q) => q.recv(),
                        Err(_) => return,
                    };
                    let Ok(job) = next else { return };
                    let result = oxidraft_raster::decode(&job.path, job.page, SCREEN_SIDE);
                    if finished.send((job.key, result)).is_err() {
                        return;
                    }
                }
            });
        }
        Self { jobs, done }
    }
}

/// IMAGEATTACH and PDFATTACH's dialog.
pub(crate) struct ImageAttachDialog {
    file: PathBuf,
    info: Info,
    name: String,
    path_type: PathType,
    page: usize,
    use_georef: bool,
    on_screen: bool,
    at: Point,
    scale: f64,
    rotation: f64,
}

fn key(name: &str) -> String {
    name.to_ascii_uppercase()
}

impl OxidraftApp {
    /// A name for a picture from `file`: its own name, made valid, and kept
    /// apart from other pictures' (the same file keeps its name).
    fn picture_name(&self, file: &Path, page: Option<usize>) -> String {
        let stem = file.file_stem().map_or("Image".into(), |s| s.to_string_lossy().into_owned());
        let base = sanitize_name(&match page {
            Some(p) => format!("{stem} - {}", p + 1),
            None => stem,
        });
        let same_file = |d: &ImageDef| {
            d.page == page
                && resolve(self.path.as_deref(), &d.path).and_then(|p| p.canonicalize().ok())
                    == file.canonicalize().ok()
        };
        (1..)
            .map(|i| if i == 1 { base.clone() } else { format!("{base} ({i})") })
            .find(|n| self.doc.image_def(n).is_none_or(same_file))
            .unwrap_or(base)
    }

    /// Starts reading a picture's file, unless it's read or being read.
    fn request_picture(&mut self, def: &ImageDef) {
        let k = key(&def.name);
        if matches!(self.rasters.get(&k), Some(Raster::Loading | Raster::Ready(_))) {
            return;
        }
        match resolve(self.path.as_deref(), &def.path) {
            Some(path) => {
                let job = Job { key: k.clone(), path, page: def.page.unwrap_or(0) };
                if self.loader.jobs.send(job).is_ok() {
                    self.rasters.insert(k, Raster::Loading);
                }
            }
            None => {
                self.rasters.insert(k, Raster::Failed("not found".into()));
            }
        }
    }

    /// Takes in the pictures read in the background.
    pub(crate) fn receive_pictures(&mut self, ctx: &egui::Context) {
        while let Ok((k, result)) = self.loader.done.try_recv() {
            // Unloaded while it was being read.
            if !matches!(self.rasters.get(&k), Some(Raster::Loading)) {
                continue;
            }
            let raster = match result {
                Ok(px) => {
                    let image =
                        egui::ColorImage::from_rgba_unmultiplied([px.width as usize, px.height as usize], &px.rgba);
                    Raster::Ready(ctx.load_texture(format!("picture {k}"), image, egui::TextureOptions::LINEAR))
                }
                Err(e) => {
                    self.engine.log(format!("Unable to load image \"{k}\": {e}"));
                    Raster::Failed(e)
                }
            };
            self.rasters.insert(k, raster);
            self.raster_revision += 1;
        }
        if self.rasters.values().any(|r| matches!(r, Raster::Loading)) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// The texture a picture shows on screen, once it's read.
    pub(crate) fn picture_texture(&self, name: &str) -> Option<egui::TextureId> {
        match self.rasters.get(&key(name)) {
            Some(Raster::Ready(t)) => Some(t.id()),
            _ => None,
        }
    }

    /// True while pictures are being read.
    pub fn pictures_loading(&self) -> bool {
        self.rasters.values().any(|r| matches!(r, Raster::Loading))
    }

    /// The pictures a plot shows: those loaded on screen, read again at
    /// plot resolution (`sharp`), or as stand-ins for the preview.
    pub(crate) fn plot_pictures(&self, sharp: bool) -> impl Fn(&str) -> Option<Arc<Picture>> + '_ {
        let cache: std::cell::RefCell<HashMap<String, Option<Arc<Picture>>>> = Default::default();
        move |name: &str| {
            self.picture_texture(name)?;
            if !sharp {
                return Some(Arc::new(Picture { width: 0, height: 0, rgba: Vec::new() }));
            }
            cache
                .borrow_mut()
                .entry(key(name))
                .or_insert_with(|| {
                    let def = self.doc.image_def(name)?;
                    let path = resolve(self.path.as_deref(), &def.path)?;
                    let px = oxidraft_raster::decode(&path, def.page.unwrap_or(0), PLOT_SIDE).ok()?;
                    Some(Arc::new(Picture { width: px.width, height: px.height, rgba: px.rgba }))
                })
                .clone()
        }
    }

    /// After opening: sizes PDF pages, notes missing files, and starts
    /// reading the pictures that were loaded when the drawing was saved.
    pub(crate) fn load_pictures(&mut self) {
        self.rasters.clear();
        self.image_selected.clear();
        let defs: Vec<ImageDef> = self.doc.image_defs().cloned().collect();
        for def in defs {
            let Some(found) = resolve(self.path.as_deref(), &def.path) else {
                self.doc.set_image_status(&def.name, XrefStatus::NotFound);
                self.engine.log(format!("Image \"{}\" not found: {}", def.name, def.path));
                continue;
            };
            if def.is_pdf()
                && def.width <= 0.0
                && let Ok(info) = oxidraft_raster::probe(&found, def.page.unwrap_or(0))
            {
                self.doc.size_pdf(&def.name, info.width, info.height);
            }
            if def.status != XrefStatus::Unloaded {
                self.doc.set_image_status(&def.name, XrefStatus::Loaded);
                self.request_picture(&def);
            }
        }
    }

    /// The palette's Load: reads the file and shows the picture.
    pub fn load_image(&mut self, name: &str) {
        let Some(def) = self.doc.image_def(name).cloned() else { return };
        if resolve(self.path.as_deref(), &def.path).is_none() {
            self.engine.log(format!("Image \"{}\" not found: {}", def.name, def.path));
            return;
        }
        self.rasters.remove(&key(name));
        self.doc.begin("IMAGE");
        self.doc.set_image_status(name, XrefStatus::Loaded);
        self.doc.commit();
        self.request_picture(&def);
    }

    /// The palette's Unload: keeps the image attached, shows its frame only.
    pub fn unload_image(&mut self, name: &str) {
        self.rasters.remove(&key(name));
        self.raster_revision += 1;
        self.doc.begin("IMAGE");
        self.doc.set_image_status(name, XrefStatus::Unloaded);
        self.doc.commit();
    }

    pub fn detach_image(&mut self, name: &str) {
        self.engine.cancel(&mut self.doc);
        self.rasters.remove(&key(name));
        self.raster_revision += 1;
        self.doc.begin("IMAGE");
        self.doc.detach_image(name);
        self.doc.commit();
        self.image_selected.retain(|n| !n.eq_ignore_ascii_case(name));
    }

    /// Loads the unloaded images the drawing area shows part of.
    pub fn load_images_in_view(&mut self) {
        let r = self.canvas_rect;
        let view = Aabb::from_points(self.view.to_world(r, r.left_bottom()), self.view.to_world(r, r.right_top()));
        let overlaps =
            |b: &Aabb| b.max.x >= view.min.x && b.min.x <= view.max.x && b.max.y >= view.min.y && b.min.y <= view.max.y;
        let mut names: Vec<String> = Vec::new();
        for e in self.doc.entities() {
            if let Geometry::Image { image, .. } = &e.geometry
                && self.doc.is_shown(&e.layer)
                && overlaps(&e.geometry.bounds())
                && self
                    .doc
                    .image_def(image)
                    .is_some_and(|d| d.status != XrefStatus::Loaded || self.picture_texture(image).is_none())
                && !names.iter().any(|n| n.eq_ignore_ascii_case(image))
            {
                names.push(image.clone());
            }
        }
        self.engine.log(format!("Loading {} image{} in view.", names.len(), if names.len() == 1 { "" } else { "s" }));
        for n in names {
            self.load_image(&n);
        }
    }

    /// IMAGEATTACH: one image opens the Attach Image dialog; several are
    /// attached where their georeferencing puts them, without reading them.
    pub(crate) fn image_attach(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("Images (GeoTIFF, TIFF, PNG, JPEG, BMP)", &["tif", "tiff", "png", "jpg", "jpeg", "bmp"])
            .pick_files()
            .unwrap_or_default();
        match files.len() {
            0 => {}
            1 => self.open_image_dialog(files[0].clone()),
            _ => self.attach_images_unloaded(&files),
        }
    }

    /// PDFATTACH: a page of a PDF, placed by hand.
    pub(crate) fn pdf_attach(&mut self) {
        if let Some(file) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
            self.open_image_dialog(file);
        }
    }

    /// Reads what `file` is and opens the Attach dialog for it.
    pub fn open_image_dialog(&mut self, file: PathBuf) {
        let info = match oxidraft_raster::probe(&file, 0) {
            Ok(info) => info,
            Err(e) => {
                self.engine.log(format!("Can't attach {}: {e}", file.display()));
                return;
            }
        };
        let page = 0;
        let name = self.picture_name(&file, oxidraft_raster::is_pdf(&file).then_some(page));
        self.image_attach_dialog = Some(ImageAttachDialog {
            use_georef: info.georef.is_some(),
            file,
            info,
            name,
            path_type: if self.path.is_some() { PathType::Relative } else { PathType::Full },
            page,
            on_screen: true,
            at: Point::ORIGIN,
            scale: 1.0,
            rotation: 0.0,
        });
    }

    /// Attaches many images at once where their georeferencing (GeoTIFF
    /// tags or a world file) puts them, each on a layer of its own name,
    /// unloaded: only their frames show until they're loaded from the
    /// External References palette. Files without georeferencing are left
    /// out, to be attached one at a time.
    pub fn attach_images_unloaded(&mut self, files: &[PathBuf]) {
        self.engine.cancel(&mut self.doc);
        self.engine.log("Command: IMAGEATTACH");
        let (mut placed, mut extent, mut skipped) = (0, None::<Aabb>, Vec::new());
        self.doc.begin("IMAGEATTACH");
        for file in files {
            let label = file.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
            let georef = match oxidraft_raster::probe(file, 0) {
                Ok(Info { georef: Some(g), width, height, .. }) => (g, width, height),
                Ok(_) => {
                    skipped.push(format!("{label} (no georeferencing)"));
                    continue;
                }
                Err(e) => {
                    skipped.push(format!("{label} ({e})"));
                    continue;
                }
            };
            let (g, width, height) = georef;
            let name = self.picture_name(file, None);
            let def = ImageDef {
                name: name.clone(),
                path: self.saved_path(file, if self.path.is_some() { PathType::Relative } else { PathType::Full }),
                page: None,
                width,
                height,
                status: XrefStatus::Unloaded,
            };
            self.doc.set_image_def(def);
            let image = Geometry::Image { image: name.clone(), origin: g.origin, u: g.u, v: g.v, clip: Vec::new() };
            let bounds = image.bounds();
            extent = Some(extent.map_or(bounds, |b| b.union(bounds)));
            let id = self.doc.add_on_layer(image, &sanitize_name(&name), Color::ByLayer);
            self.doc.send_to_back(&[id]);
            placed += 1;
        }
        self.doc.commit();
        self.engine.log(format!(
            "Attached {placed} image{} where their georeferencing puts them, not loaded: load them from the External References palette (XR).",
            if placed == 1 { "" } else { "s" }
        ));
        if !skipped.is_empty() {
            self.engine.log(format!("Left out: {}. Attach these one at a time to place them.", skipped.join(", ")));
        }
        if let Some(b) = extent {
            self.pending_views.push(ViewRequest::Window(b));
        }
    }

    /// The Attach dialog's OK.
    fn finish_image_attach(&mut self, d: ImageAttachDialog) -> Result<(), String> {
        oxidraft_model::check_name(&d.name)?;
        let pdf = oxidraft_raster::is_pdf(&d.file);
        let info = if pdf { oxidraft_raster::probe(&d.file, d.page)? } else { d.info.clone() };
        let def = ImageDef {
            name: d.name.clone(),
            path: self.saved_path(&d.file, d.path_type),
            page: pdf.then_some(d.page),
            width: info.width,
            height: info.height,
            status: XrefStatus::Loaded,
        };
        let (u, v, at) = match info.georef.filter(|_| d.use_georef && !pdf) {
            Some(g) => (g.u, g.v, Some(g.origin)),
            None => {
                // An image is `scale` units wide; a PDF page its size in millimeters times `scale`.
                let (w, h) = if pdf {
                    (info.width / 72.0 * 25.4 * d.scale, info.height / 72.0 * 25.4 * d.scale)
                } else {
                    (d.scale, d.scale * info.height / info.width.max(1.0))
                };
                let at = (!d.on_screen).then_some(d.at);
                (Point::ORIGIN.polar(w, d.rotation), Point::ORIGIN.polar(h, d.rotation + 90.0), at)
            }
        };
        self.rasters.remove(&key(&def.name));
        self.request_picture(&def);
        self.engine.attach_image(def, u, v, at, &mut self.doc);
        Ok(())
    }

    pub(crate) fn image_attach_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.image_attach_dialog.take() else { return };
        let pdf = oxidraft_raster::is_pdf(&d.file);
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let title = if pdf { "Attach PDF Underlay" } else { "Attach Image" };
        egui::Window::new(title)
            .id(egui::Id::unique("attach_image"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([320.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                egui::Grid::new("attach_image_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Name:");
                    let field = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(240.0));
                    self.text_fields.push(field.id);
                    ui.end_row();
                    ui.label("Found in:");
                    ui.add(egui::Label::new(d.file.display().to_string()).truncate());
                    ui.end_row();
                    ui.label(if pdf { "Page:" } else { "Size:" });
                    if pdf {
                        let mut page = d.page + 1;
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut page).range(1..=d.info.pages.max(1)));
                            ui.colored_label(theme::TEXT_DIM, format!("of {}", d.info.pages));
                        });
                        d.page = page - 1;
                    } else {
                        ui.label(format!("{} x {} pixels", d.info.width, d.info.height));
                    }
                    ui.end_row();
                    ui.label("Path type:");
                    egui::ComboBox::from_id_salt("attach_image_path")
                        .selected_text(match d.path_type {
                            PathType::Full => "Full path",
                            PathType::Relative => "Relative path",
                            PathType::None => "No path",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.path_type, PathType::Full, "Full path");
                            ui.add_enabled_ui(self.path.is_some(), |ui| {
                                ui.selectable_value(&mut d.path_type, PathType::Relative, "Relative path");
                            });
                            ui.selectable_value(&mut d.path_type, PathType::None, "No path");
                        });
                    ui.end_row();
                    if let Some(g) = d.info.georef {
                        ui.label("Placement:");
                        ui.vertical(|ui| {
                            ui.checkbox(&mut d.use_georef, "Use its georeferencing");
                            ui.colored_label(
                                theme::TEXT_DIM,
                                format!(
                                    "{:.4} units a pixel, corner at {:.3}, {:.3}",
                                    g.u.length() / d.info.width.max(1.0),
                                    g.origin.x,
                                    g.origin.y
                                ),
                            );
                        });
                        ui.end_row();
                    }
                    ui.add_enabled_ui(!d.use_georef || pdf, |ui| ui.label("Insertion point:"));
                    ui.add_enabled_ui(!d.use_georef || pdf, |ui| {
                        ui.vertical(|ui| {
                            ui.checkbox(&mut d.on_screen, "Specify on-screen");
                            ui.add_enabled_ui(!d.on_screen, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("X:");
                                    ui.add(egui::DragValue::new(&mut d.at.x).speed(1.0).max_decimals(4));
                                    ui.label("Y:");
                                    ui.add(egui::DragValue::new(&mut d.at.y).speed(1.0).max_decimals(4));
                                });
                            });
                        });
                    });
                    ui.end_row();
                    ui.add_enabled_ui(!d.use_georef || pdf, |ui| ui.label("Scale:"));
                    ui.add_enabled_ui(!d.use_georef || pdf, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut d.scale).speed(0.01).range(0.0001..=f64::MAX).max_decimals(4),
                            );
                            let hint = if pdf { "x page size in mm" } else { "units wide" };
                            ui.colored_label(theme::TEXT_DIM, hint);
                        });
                    });
                    ui.end_row();
                    ui.add_enabled_ui(!d.use_georef || pdf, |ui| ui.label("Rotation:"));
                    ui.add_enabled_ui(!d.use_georef || pdf, |ui| {
                        ui.add(egui::DragValue::new(&mut d.rotation).speed(1.0).suffix("°").max_decimals(4));
                    });
                    ui.end_row();
                });
                ui.separator();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    ok = ui.button("OK").clicked();
                });
            });
        if !open || cancel {
            return;
        }
        if ok {
            if let Err(e) = self.finish_image_attach(d) {
                self.engine.log(e);
            }
            return;
        }
        self.image_attach_dialog = Some(d);
    }

    /// A picture: its pixels when they're loaded (inside its clipping
    /// boundary), else its frame with its name and state. A thick stroke
    /// (selection) draws the frame only.
    pub(crate) fn draw_image(&self, painter: &Painter, rect: Rect, g: &Geometry, stroke: Stroke) {
        let Geometry::Image { image, origin, u, v, clip } = g else { return };
        let to = |p: Point| self.view.to_screen(rect, p);
        let ring: Vec<Point> = if clip.len() >= 3 { clip.clone() } else { image_corners(*origin, *u, *v).to_vec() };
        let frame: Vec<Pos2> = ring.iter().map(|p| to(*p)).collect();
        let texture = self.picture_texture(image);
        if stroke.width < 4.0
            && let Some(texture) = texture
        {
            // Where a point falls in the picture, as texture coordinates.
            let det = u.cross(*v);
            let uv = |p: Point| {
                let d = p - *origin;
                let (a, b) = if det.abs() > 0.0 { (d.cross(*v) / det, u.cross(d) / det) } else { (0.0, 0.0) };
                egui::pos2(a as f32, (1.0 - b) as f32)
            };
            let mut mesh = egui::Mesh::with_texture(texture);
            for t in hatch::triangles(std::slice::from_ref(&ring)) {
                let i = mesh.vertices.len() as u32;
                for p in t {
                    mesh.vertices.push(egui::epaint::Vertex { pos: to(p), uv: uv(p), color: Color32::WHITE });
                }
                mesh.add_triangle(i, i + 1, i + 2);
            }
            painter.add(Shape::mesh(mesh));
        }
        let frame_stroke =
            if stroke.width >= 4.0 { stroke } else { Stroke::new(1.0, stroke.color.gamma_multiply(0.7)) };
        painter.add(Shape::closed_line(frame, frame_stroke));
        if texture.is_none() && stroke.width < 4.0 {
            let center = to(ring.iter().fold(Point::ORIGIN, |s, p| s + *p) * (1.0 / ring.len() as f64));
            let state = match self.rasters.get(&key(image)) {
                Some(Raster::Loading) => "Loading…".to_string(),
                Some(Raster::Failed(e)) => e.clone(),
                _ => self.doc.image_def(image).map_or("Not Found", |d| d.status.label()).to_string(),
            };
            let size = to(*origin).distance(to(*origin + *u));
            if size > 60.0 {
                let text = format!("{image}\n{state}");
                painter.text(center, Align2::CENTER_CENTER, text, FontId::proportional(12.0), theme::TEXT_DIM);
            }
        }
    }
}

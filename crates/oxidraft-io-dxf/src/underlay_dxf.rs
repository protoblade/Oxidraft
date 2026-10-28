//! Images (IMAGE + IMAGEDEF) and PDF underlays (PDFUNDERLAY +
//! PDFDEFINITION) in DXF. The definitions live in the OBJECTS section,
//! listed in the root dictionary's ACAD_IMAGE_DICT and ACAD_PDFDEFINITIONS.
//! The dxf crate calls PDF definitions UNDERLAYDEFINITION, so the type
//! name is swapped on the way in and out.

use std::collections::HashMap;

use dxf::entities::EntityType;
use dxf::enums::ImageClippingBoundaryType;
use dxf::objects::{Dictionary, ImageDefinition, Object, ObjectType, UnderlayDefinition};
use dxf::{Drawing, Handle};
use oxidraft_geom::Point;
use oxidraft_model::{Document, Geometry, ImageDef, XrefStatus};

use crate::{from_dxf, from_dxf_vector, to_dxf, to_dxf_vector};

/// Each definition's handle in the file, by upper-case name.
pub(crate) type Handles = HashMap<String, (Handle, ImageDef)>;

fn dictionary(entries: HashMap<String, Handle>) -> Object {
    Object::new(ObjectType::Dictionary(Dictionary { value_handles: entries, ..Default::default() }))
}

fn set_owner(drawing: &mut Drawing, handle: Handle, owner: Handle) {
    if let Some(o) = drawing.objects_mut().find(|o| o.common.handle == handle) {
        o.common.__owner_handle = owner;
    }
}

/// Adds the definitions, their dictionaries and a root dictionary.
pub(crate) fn add_definitions(drawing: &mut Drawing, doc: &Document) -> Handles {
    let mut out = Handles::new();
    let defs: Vec<ImageDef> = doc.image_defs().cloned().collect();
    if defs.is_empty() {
        return out;
    }
    // The root dictionary comes first in OBJECTS.
    let root = drawing.add_object(dictionary(HashMap::new())).common.handle;
    let images = drawing.add_object(dictionary(HashMap::new())).common.handle;
    let pdfs = drawing.add_object(dictionary(HashMap::new())).common.handle;
    let (mut image_entries, mut pdf_entries) = (HashMap::new(), HashMap::new());
    for def in defs {
        let (object, owner) = match def.page {
            Some(page) => {
                let d = UnderlayDefinition { file_name: def.path.clone(), name: (page + 1).to_string() };
                (Object::new(ObjectType::UnderlayDefinition(d)), pdfs)
            }
            None => {
                let d = ImageDefinition {
                    file_path: def.path.clone(),
                    image_width: def.width as i32,
                    image_height: def.height as i32,
                    pixel_width: 1.0,
                    pixel_height: 1.0,
                    // Unloaded images stay unloaded when the drawing is opened again.
                    is_image_loaded: def.status == XrefStatus::Loaded,
                    ..Default::default()
                };
                (Object::new(ObjectType::ImageDefinition(d)), images)
            }
        };
        let handle = drawing.add_object(object).common.handle;
        set_owner(drawing, handle, owner);
        let entries = if def.page.is_some() { &mut pdf_entries } else { &mut image_entries };
        entries.insert(def.name.clone(), handle);
        out.insert(def.name.to_ascii_uppercase(), (handle, def));
    }
    let root_entries =
        HashMap::from([("ACAD_IMAGE_DICT".to_string(), images), ("ACAD_PDFDEFINITIONS".to_string(), pdfs)]);
    for (handle, entries) in [(root, root_entries), (images, image_entries), (pdfs, pdf_entries)] {
        if let Some(Object { specific: ObjectType::Dictionary(d), .. }) =
            drawing.objects_mut().find(|o| o.common.handle == handle)
        {
            d.value_handles = entries;
        }
    }
    set_owner(drawing, images, root);
    set_owner(drawing, pdfs, root);
    out
}

/// Solves `p = origin + a·u + b·v` for `(a, b)`.
fn picture_coords(p: Point, origin: Point, u: Point, v: Point) -> (f64, f64) {
    let det = u.cross(v);
    if det.abs() < 1e-300 {
        return (0.0, 0.0);
    }
    let d = p - origin;
    (d.cross(v) / det, u.cross(d) / det)
}

/// The entity for a picture, or `None` if its definition is missing.
pub(crate) fn entity(g: &Geometry, handles: &Handles) -> Option<EntityType> {
    let Geometry::Image { image, origin, u, v, clip } = g else { return None };
    let (handle, def) = handles.get(&image.to_ascii_uppercase())?;
    if def.is_pdf() {
        // Underlay scales count drawing units per inch of the page.
        let (w, h) = (def.width / 72.0, def.height / 72.0);
        let scale = |len: f64, inches: f64| if inches > 0.0 { len / inches } else { len };
        let mut e = dxf::entities::PdfUnderlay {
            insertion_point: to_dxf(*origin),
            x_scale: scale(u.length(), w),
            y_scale: scale(v.length(), h),
            z_scale: 1.0,
            rotation_angle: Point::ORIGIN.angle_to(*u),
            ..Default::default()
        };
        e.__object_handle = *handle;
        e.set_is_underlay_on(true);
        return Some(EntityType::PdfUnderlay(e));
    }
    let (w, h) = (def.width.max(1.0), def.height.max(1.0));
    let mut e = dxf::entities::Image {
        location: to_dxf(*origin),
        u_vector: to_dxf_vector(*u * (1.0 / w)),
        v_vector: to_dxf_vector(*v * (1.0 / h)),
        image_size: dxf::Vector::new(w, h, 0.0),
        image_def_reference: format!("{:X}", handle.0),
        use_clipping: clip.len() >= 3,
        ..Default::default()
    };
    e.set_show_image(true);
    e.set_show_image_when_not_alligned(true);
    if clip.len() >= 3 {
        // Clipping boundaries are in pixels from the image's upper-left corner.
        e.set_use_clipping_boundary(true);
        e.clipping_type = ImageClippingBoundaryType::Polygonal;
        let mut pixels: Vec<dxf::Point> = clip
            .iter()
            .map(|p| {
                let (a, b) = picture_coords(*p, *origin, *u, *v);
                dxf::Point::new(a * w - 0.5, (1.0 - b) * h - 0.5, 0.0)
            })
            .collect();
        pixels.push(pixels[0].clone());
        e.clipping_vertex_count = pixels.len() as i32;
        e.clipping_vertices = pixels;
    }
    Some(EntityType::Image(e))
}

/// The pictures' definitions in a file, by handle, with the names their
/// dictionaries give them.
pub(crate) struct Definitions {
    by_handle: HashMap<u64, ImageDef>,
}

impl Definitions {
    pub(crate) fn read(drawing: &Drawing) -> Definitions {
        let mut names: HashMap<u64, String> = HashMap::new();
        for o in drawing.objects() {
            if let ObjectType::Dictionary(d) = &o.specific {
                for (name, h) in &d.value_handles {
                    names.insert(h.0, name.clone());
                }
            }
        }
        let stem = |path: &str| {
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            file.rsplit_once('.').map_or(file, |(s, _)| s).to_string()
        };
        let mut by_handle = HashMap::new();
        for o in drawing.objects() {
            let def = match &o.specific {
                ObjectType::ImageDefinition(d) => ImageDef {
                    name: String::new(),
                    path: d.file_path.clone(),
                    page: None,
                    width: f64::from(d.image_width),
                    height: f64::from(d.image_height),
                    status: if d.is_image_loaded { XrefStatus::Loaded } else { XrefStatus::Unloaded },
                },
                ObjectType::UnderlayDefinition(d) => ImageDef {
                    name: String::new(),
                    path: d.file_name.clone(),
                    page: Some(d.name.trim().parse::<usize>().map_or(0, |p| p.saturating_sub(1))),
                    width: 0.0,
                    height: 0.0,
                    status: XrefStatus::Loaded,
                },
                _ => continue,
            };
            let name = names.get(&o.common.handle.0).cloned().unwrap_or_else(|| stem(&def.path));
            by_handle.insert(o.common.handle.0, ImageDef { name, ..def });
        }
        Definitions { by_handle }
    }

    /// The picture an IMAGE or PDFUNDERLAY shows; its definition is added
    /// to `doc` the first time.
    pub(crate) fn geometry(&self, e: &EntityType, doc: &mut Document) -> Option<Geometry> {
        let (def, geometry) = match e {
            EntityType::Image(i) => {
                let handle = u64::from_str_radix(i.image_def_reference.trim(), 16).ok()?;
                let def = self.by_handle.get(&handle)?;
                let (w, h) = (i.image_size.x.max(1.0), i.image_size.y.max(1.0));
                let (origin, u, v) =
                    (from_dxf(&i.location), from_dxf_vector(&i.u_vector) * w, from_dxf_vector(&i.v_vector) * h);
                let at = |px: f64, py: f64| origin + u * ((px + 0.5) / w) + v * (1.0 - (py + 0.5) / h);
                let mut clip: Vec<Point> = i.clipping_vertices.iter().map(|p| at(p.x, p.y)).collect();
                if i.clipping_type == ImageClippingBoundaryType::Rectangular && clip.len() == 2 {
                    let (a, b) = (i.clipping_vertices[0].clone(), i.clipping_vertices[1].clone());
                    clip = vec![at(a.x, a.y), at(b.x, a.y), at(b.x, b.y), at(a.x, b.y)];
                }
                if clip.len() > 3 && clip[0].approx_eq(clip[clip.len() - 1]) {
                    clip.pop();
                }
                if !i.use_clipping || clip.len() < 3 {
                    clip.clear();
                }
                let image = def.name.clone();
                (def, Geometry::Image { image, origin, u, v, clip })
            }
            // Until the PDF is read, its page is an inch across at scale 1.
            EntityType::PdfUnderlay(p) => {
                let def = self.by_handle.get(&p.__object_handle.0)?;
                let r = p.rotation_angle;
                let (u, v) = (Point::ORIGIN.polar(p.x_scale, r), Point::ORIGIN.polar(p.y_scale, r + 90.0));
                let image = def.name.clone();
                (def, Geometry::Image { image, origin: from_dxf(&p.insertion_point), u, v, clip: Vec::new() })
            }
            _ => return None,
        };
        if doc.image_def(&def.name).is_none() {
            doc.set_image_def(def.clone());
        }
        Some(geometry)
    }
}

/// Swaps one object type name for another in DXF text, byte for byte
/// (older files aren't UTF-8).
pub(crate) fn rename_type(dxf: &[u8], from: &str, to: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(dxf.len());
    for line in dxf.split_inclusive(|&b| b == b'\n') {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        let body = body.strip_suffix(b"\r").unwrap_or(body);
        if body == from.as_bytes() {
            out.extend_from_slice(to.as_bytes());
            out.extend_from_slice(&line[body.len()..]);
        } else {
            out.extend_from_slice(line);
        }
    }
    out
}

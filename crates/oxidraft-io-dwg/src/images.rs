//! Pictures on the way out, as AutoCAD writes them: each IMAGE with its
//! IMAGEDEF_REACTOR and a clip boundary (the whole picture when it isn't
//! clipped), each IMAGEDEF with its pixel size.

use acadrust::CadDocument;
use acadrust::entities::{ClipBoundary, ClipType, EntityType};
use acadrust::objects::{ImageDefinitionReactor, ObjectType};
use acadrust::types::{Handle, Vector2};

pub(crate) fn finish(cad: &mut CadDocument) {
    // The DXF writer the drawing goes out through puts the pixel height in
    // group 12, not 21, so it arrives as 0.
    for o in cad.objects.values_mut() {
        if let ObjectType::ImageDefinition(d) = o {
            if d.pixel_size.0.is_nan() || d.pixel_size.0 <= 0.0 {
                d.pixel_size.0 = 1.0;
            }
            if d.pixel_size.1.is_nan() || d.pixel_size.1 <= 0.0 {
                d.pixel_size.1 = d.pixel_size.0;
            }
        }
    }
    let images: Vec<Handle> = cad
        .entities()
        .filter_map(|e| match e {
            EntityType::RasterImage(i) => Some(i.common.handle),
            _ => None,
        })
        .collect();
    for h in images {
        let Some(EntityType::RasterImage(image)) = cad.get_entity_mut(h) else { continue };
        // A rectangle has its two corners, a polygon three points or more;
        // with fewer, AutoCAD's own: the whole picture, in pixels.
        let enough = match image.clip_boundary.clip_type {
            ClipType::Rectangular => 2,
            ClipType::Polygonal => 3,
        };
        if image.clip_boundary.vertices.len() < enough {
            image.clip_boundary = ClipBoundary::rectangular(
                Vector2::new(-0.5, -0.5),
                Vector2::new(image.size.x - 0.5, image.size.y - 0.5),
            );
            image.clipping_enabled = false;
        }
        if image.definition_reactor_handle.is_some_and(|r| !r.is_null()) {
            continue;
        }
        let reactor = cad.allocate_handle();
        if let Some(EntityType::RasterImage(image)) = cad.get_entity_mut(h) {
            image.definition_reactor_handle = Some(reactor);
        }
        cad.objects.insert(
            reactor,
            ObjectType::ImageDefinitionReactor(ImageDefinitionReactor { handle: reactor, owner: h, image_handle: h }),
        );
    }
}

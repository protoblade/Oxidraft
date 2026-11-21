//! The drawing as last drawn, kept while only the cursor moves over it.
//! Drawing a big drawing's objects again for each mouse move is what makes
//! it slow, and nothing of them changes but what's under the cursor, which
//! is drawn over the kept picture. Kept already turned into triangles, so
//! showing it again costs next to nothing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, Context, Event, Mesh, Painter, Rect, Shape, TextureId,
    emath::TSTransform,
    epaint::{ClippedShape, Tessellator},
};
use oxidraft_model::EntityId;
use oxidraft_model::layout::Space;

use crate::underlay::DetailWant;
use crate::view::View;

/// Everything a picture of objects is drawn from: one drawn from anything
/// different is drawn again.
#[derive(Clone, PartialEq)]
pub(crate) struct Key {
    /// The drawing ([`oxidraft_model::Document::instance`]) and its revision.
    pub drawing: (u64, u64),
    pub space: Space,
    pub rect: Rect,
    pub clip: Rect,
    pub view: View,
    pub sheet_scale: Option<f64>,
    pub frozen: Vec<String>,
    pub selected: Vec<EntityId>,
    /// Traces in front and the tracing paper's opacity; lineweights shown.
    pub traces: (bool, u8),
    pub lineweights: bool,
    /// The colors the theme draws with: color 7, the background, the
    /// selection's glow.
    pub colors: [Color32; 3],
    pub pixels_per_point: f32,
    /// What the drawing's pictures show ([`crate::app::OxidraftApp::pictures_shown`]).
    pub images: (u64, Vec<(String, egui::TextureId)>),
}

/// A picture kept, with what it was drawn from.
struct Kept {
    key: Key,
    /// In draw order, each with its clip rectangle: runs of shapes merged
    /// into one mesh, and what can't merge (another texture, a transform)
    /// as it is.
    pieces: Vec<ClippedShape>,
    made: Instant,
    /// The font atlas's size and how full it was: text points into it, so
    /// the picture goes when egui starts the atlas again.
    atlas: ([usize; 2], f32),
    /// The last pass it was shown in.
    shown: u64,
    /// The sharper parts of pictures it wants read, wanted again each time
    /// it's shown (those not wanted are let go).
    wants: Vec<DetailWant>,
}

/// Even unchanged, a picture is drawn again this often, in case something
/// it's drawn from changed with no input to notice.
const MAX_AGE: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct Pictures {
    kept: Vec<Kept>,
}

impl Pictures {
    /// Paints the picture kept for `key` with `painter`, if there's one and
    /// it may still be shown: true if it was.
    pub(crate) fn show(&mut self, painter: &Painter, key: &Key, wants: &mut Vec<DetailWant>) -> bool {
        let ctx = painter.ctx();
        let atlas = atlas(ctx);
        let pass = ctx.cumulative_pass_nr();
        let Some(kept) = self.kept.iter_mut().find(|k| &k.key == key) else { return false };
        let fresh = kept.shown == pass || only_pointer_moved(ctx);
        if !fresh || kept.made.elapsed() > MAX_AGE || atlas.0 != kept.atlas.0 || atlas.1 < kept.atlas.1 {
            return false;
        }
        kept.shown = pass;
        wants.extend(kept.wants.iter().cloned());
        ctx.graphics_mut(|g| {
            let list = g.entry(painter.layer_id());
            for piece in &kept.pieces {
                let at = list.add(piece.clip_rect, piece.shape.clone());
                if piece.transform_after_tessellation != TSTransform::IDENTITY {
                    list.mutate_shape(at, |s| s.transform_after_tessellation = piece.transform_after_tessellation);
                }
            }
        });
        true
    }

    /// Keeps `shapes`, as drawn for `key`, turned into triangles; the
    /// pictures not shown lately go.
    pub(crate) fn keep(
        &mut self,
        ctx: &Context,
        key: Key,
        shapes: impl Iterator<Item = ClippedShape>,
        wants: Vec<DetailWant>,
    ) {
        let pass = ctx.cumulative_pass_nr();
        self.kept.retain(|k| k.key != key && pass.saturating_sub(k.shown) <= 1);
        self.kept.truncate(15);
        let atlas = atlas(ctx);
        let options = ctx.tessellation_options(|o| *o);
        let mut tessellator = Tessellator::new(key.pixels_per_point, options, atlas.0, Vec::new());
        let mut pieces = Vec::new();
        let mut run: Option<(Rect, Mesh)> = None;
        let piece = |(clip, mesh): (Rect, Mesh)| ClippedShape::new(clip, Shape::Mesh(Arc::new(mesh)));
        for shape in shapes {
            if !merges(&shape.shape) || shape.transform_after_tessellation != TSTransform::IDENTITY {
                pieces.extend(run.take().map(piece));
                pieces.push(shape);
                continue;
            }
            if run.as_ref().is_some_and(|(clip, _)| *clip != shape.clip_rect) {
                pieces.extend(run.take().map(piece));
            }
            let (_, mesh) = run.get_or_insert_with(|| (shape.clip_rect, Mesh::default()));
            tessellator.set_clip_rect(shape.clip_rect);
            tessellator.tessellate_shape(shape.shape, mesh);
        }
        pieces.extend(run.take().map(piece));
        // Shown when kept: its wants are there already.
        self.kept.push(Kept { key, pieces, made: Instant::now(), atlas, shown: pass, wants });
    }
}

/// The font atlas's size and how full it is.
fn atlas(ctx: &Context) -> ([usize; 2], f32) {
    ctx.fonts(|f| (f.font_image_size(), f.font_atlas_fill_ratio()))
}

/// True if a shape can go into a mesh on the font texture with others:
/// all but pictures (their own textures) and paint callbacks.
fn merges(shape: &Shape) -> bool {
    match shape {
        Shape::Mesh(m) => m.texture_id == TextureId::default(),
        Shape::Rect(r) => r.brush.is_none(),
        Shape::Vec(v) => v.iter().all(merges),
        Shape::Callback(_) => false,
        _ => true,
    }
}

/// True if what came in this pass only moved the pointer.
fn only_pointer_moved(ctx: &Context) -> bool {
    ctx.input(|i| {
        !i.events.is_empty() && i.events.iter().all(|e| matches!(e, Event::PointerMoved(_) | Event::MouseMoved(_)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Key as KeyName, Modifiers, RawInput, Stroke, pos2, vec2};

    fn key(revision: u64) -> Key {
        Key {
            drawing: (1, revision),
            space: oxidraft_model::MODEL,
            rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0)),
            clip: Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0)),
            view: View::default(),
            sheet_scale: None,
            frozen: Vec::new(),
            selected: Vec::new(),
            traces: (false, 0),
            lineweights: false,
            colors: [Color32::WHITE; 3],
            pixels_per_point: 1.0,
            images: (0, Vec::new()),
        }
    }

    /// One pass with `events`: whether the picture for `key` was shown
    /// (kept first when `keep`), and how many shapes were painted.
    fn pass(ctx: &Context, pictures: &mut Pictures, events: Vec<Event>, key: &Key, keep: bool) -> (bool, usize) {
        let mut shown = false;
        let out = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
            let painter = ui.painter();
            if keep {
                let line = Shape::line_segment([pos2(10.0, 10.0), pos2(200.0, 90.0)], Stroke::new(1.0, Color32::RED));
                let text = painter.fonts_mut(|f| {
                    Shape::text(
                        f,
                        pos2(20.0, 20.0),
                        egui::Align2::LEFT_TOP,
                        "kept",
                        egui::FontId::proportional(12.0),
                        Color32::WHITE,
                    )
                });
                let shapes = [line, text].map(|s| ClippedShape::new(key.clip, s));
                pictures.keep(ui.ctx(), key.clone(), shapes.into_iter(), Vec::new());
            }
            shown = pictures.show(painter, key, &mut Vec::new());
        });
        let mut out = out;
        out.textures_delta.clear();
        (shown, out.shapes.len())
    }

    #[test]
    fn shown_again_only_while_the_pointer_alone_moves() {
        let ctx = Context::default();
        let mut pictures = Pictures::default();
        let moved = || vec![Event::PointerMoved(pos2(50.0, 50.0))];
        assert!(!pass(&ctx, &mut pictures, Vec::new(), &key(1), false).0, "nothing kept yet");
        let (shown, painted) = pass(&ctx, &mut pictures, Vec::new(), &key(1), true);
        assert!(shown, "shown as it's kept");
        assert_eq!(painted, 1, "the line and the text in one mesh");
        assert!(pass(&ctx, &mut pictures, moved(), &key(1), false).0, "the pointer moved");
        assert!(pass(&ctx, &mut pictures, moved(), &key(1), false).0);
        assert!(!pass(&ctx, &mut pictures, moved(), &key(2), false).0, "the drawing changed");
        let typed = vec![Event::Key {
            key: KeyName::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }];
        assert!(!pass(&ctx, &mut pictures, typed, &key(1), false).0, "a key was pressed");
        assert!(!pass(&ctx, &mut pictures, Vec::new(), &key(1), false).0, "a pass with nothing in it");
    }
}

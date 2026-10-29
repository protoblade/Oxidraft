//! The fonts text styles name, as egui fonts so the drawing area and the
//! plot preview show text in them (the same files plots embed). A font
//! added this frame draws from the next, when egui has it; until then, and
//! when it isn't found, text shows in the default typeface.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use eframe::egui::{self, Context, FontData, FontDefinitions, FontFamily, Id};
use oxidraft_plot::font::typeface;

/// The style fonts given to one egui context.
#[derive(Clone, Default)]
struct Loaded {
    /// Every font given, by its key.
    given: Vec<String>,
    /// Those egui has (given before this frame).
    ready: HashSet<String>,
    /// Given this frame.
    pending: Vec<String>,
}

fn id() -> Id {
    Id::unique("oxidraft_style_fonts")
}

fn definitions(keys: &[String]) -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallback = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for key in keys {
        let face = typeface(key);
        defs.font_data.insert(key.clone(), Arc::new(FontData::from_static(face.data())));
        // Characters the font lacks come from the default ones.
        let family = std::iter::once(key.clone()).chain(fallback.iter().cloned()).collect();
        defs.families.insert(FontFamily::Name(key.as_str().into()), family);
    }
    defs
}

/// Makes the fonts `fonts` name (looked for beside the drawing in `dir`
/// too) egui fonts; called once a frame.
pub(crate) fn load<'a>(ctx: &Context, dir: Option<&Path>, fonts: impl IntoIterator<Item = &'a str>) {
    if let Some(dir) = dir {
        oxidraft_plot::font::add_search_dir(dir);
    }
    let mut s: Loaded = ctx.data(|d| d.get_temp::<Arc<Loaded>>(id())).map(|s| (*s).clone()).unwrap_or_default();
    let had = (s.given.len(), s.pending.len());
    // What was given last frame is in use now.
    s.ready.extend(s.pending.drain(..));
    for font in fonts {
        let face = typeface(font);
        if face.is_default() || s.given.iter().any(|k| k == face.key()) {
            continue;
        }
        s.given.push(face.key().to_string());
        s.pending.push(face.key().to_string());
    }
    if s.given.len() > had.0 {
        ctx.set_fonts(definitions(&s.given));
    }
    if (s.given.len(), s.pending.len()) != had || had.1 > 0 {
        ctx.data_mut(|d| d.insert_temp(id(), Arc::new(s)));
    }
}

/// The egui family to show text of a style with font `font` in.
pub(crate) fn family(ctx: &Context, font: &str) -> FontFamily {
    let face = typeface(font);
    if face.is_default() {
        return FontFamily::Proportional;
    }
    let ready = ctx.data(|d| d.get_temp::<Arc<Loaded>>(id())).is_some_and(|s| s.ready.contains(face.key()));
    if ready { FontFamily::Name(face.key().into()) } else { FontFamily::Proportional }
}

/// A font of that family at `size`.
pub(crate) fn font_id(ctx: &Context, font: &str, size: f32) -> egui::FontId {
    egui::FontId::new(size, family(ctx, font))
}

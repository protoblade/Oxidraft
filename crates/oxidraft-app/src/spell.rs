//! SPELL (`SP`): AutoCAD's Check Spelling dialog. Words in text,
//! attributes, dimension text and table cells are checked against the
//! system's word list and the user's own (Add to Dictionary); each one
//! not found shows with suggestions, to Ignore or Change, once or all.

use std::collections::HashSet;
use std::sync::OnceLock;

use eframe::egui::{self, Align, Layout, RichText, vec2};
use oxidraft_commands::ViewRequest;
use oxidraft_geom::Aabb;
use oxidraft_model::{EntityId, Geometry, field};

use crate::{app::OxidraftApp, theme};

/// The word lists looked for: macOS's, then Linux's.
const WORD_LISTS: [&str; 3] =
    ["/usr/share/dict/words", "/usr/share/dict/american-english", "/usr/share/dict/british-english"];

/// The words Oxidraft carries, for systems without a list of their own
/// (Windows): Webster's Second, as FreeBSD keeps it (see assets).
const OWN_WORDS: &[u8] = include_bytes!("../assets/words.gz");

/// The system's words, in lowercase; Oxidraft's own where it has none.
fn dictionary() -> &'static HashSet<String> {
    static WORDS: OnceLock<HashSet<String>> = OnceLock::new();
    WORDS.get_or_init(|| {
        let system: HashSet<String> = WORD_LISTS
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .flat_map(|s| s.lines().map(|w| w.trim().to_lowercase()).collect::<Vec<_>>())
            .filter(|w| !w.is_empty())
            .collect();
        if !system.is_empty() {
            return system;
        }
        own_words()
    })
}

fn own_words() -> HashSet<String> {
    let mut text = String::new();
    let _ = std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(OWN_WORDS), &mut text);
    text.lines().filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// True if `word` is a word: in the lists, or made from one by the
/// endings word lists leave out (plurals, -ed, -ing, -er, -est, -ly, 's).
fn known(word: &str, words: &dyn Fn(&str) -> bool) -> bool {
    let w = word.to_lowercase();
    let w = w.strip_suffix("'s").unwrap_or(&w);
    if words(w) {
        return true;
    }
    const ENDINGS: [(&str, &[&str]); 12] = [
        ("ies", &["y"]),
        ("ied", &["y"]),
        ("ier", &["y"]),
        ("iest", &["y"]),
        ("ily", &["y"]),
        ("es", &["", "e"]),
        ("s", &[""]),
        ("ed", &["", "e"]),
        ("ing", &["", "e"]),
        ("er", &["", "e"]),
        ("est", &["", "e"]),
        ("ly", &["", "le"]),
    ];
    ENDINGS.iter().any(|(ending, stems)| {
        let Some(stem) = w.strip_suffix(ending).filter(|s| s.chars().count() >= 2) else { return false };
        stems.iter().any(|s| words(&format!("{stem}{s}")))
            // A doubled last letter: stopped, planning, bigger.
            || matches!(*ending, "ed" | "ing" | "er" | "est")
                && stem.len() >= 3
                && stem.as_bytes()[stem.len() - 1] == stem.as_bytes()[stem.len() - 2]
                && words(&stem[..stem.len() - 1])
    })
}

/// Words one letter away from `word` (deleted, swapped, changed or added)
/// that are words, then two away if none are.
fn suggestions(word: &str, words: &dyn Fn(&str) -> bool) -> Vec<String> {
    let edits = |w: &str| -> Vec<String> {
        let chars: Vec<char> = w.chars().collect();
        let mut out = Vec::new();
        for i in 0..=chars.len() {
            if i < chars.len() {
                out.push(chars[..i].iter().chain(&chars[i + 1..]).collect());
            }
            if i + 1 < chars.len() {
                let mut c = chars.clone();
                c.swap(i, i + 1);
                out.push(c.into_iter().collect());
            }
            for l in 'a'..='z' {
                if i < chars.len() {
                    let mut c = chars.clone();
                    c[i] = l;
                    out.push(c.into_iter().collect());
                }
                let mut c = chars.clone();
                c.insert(i, l);
                out.push(c.into_iter().collect());
            }
        }
        out
    };
    let lower = word.to_lowercase();
    let first = edits(&lower);
    let mut found: Vec<String> = first.iter().filter(|w| words(w)).cloned().collect();
    if found.is_empty() && lower.chars().count() <= 12 {
        found = first.iter().flat_map(|w| edits(w)).filter(|w| words(w)).collect();
    }
    let mut seen = HashSet::new();
    found.retain(|w| *w != lower && seen.insert(w.clone()));
    found.truncate(10);
    // In the word's own case.
    let upper = word.chars().all(|c| !c.is_lowercase());
    let capital = word.chars().next().is_some_and(char::is_uppercase);
    found
        .into_iter()
        .map(|w| {
            if upper && word.chars().count() > 1 {
                w.to_uppercase()
            } else if capital {
                let mut c = w.chars();
                c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
            } else {
                w
            }
        })
        .collect()
}

/// The words of a text, with where each starts: MTEXT format codes,
/// special characters (%%d) and fields skipped, and words with digits in
/// them left out.
fn words(text: &str) -> Vec<(usize, &str)> {
    let fields: Vec<_> = field::fields(text).into_iter().map(|(r, _)| r).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if let Some(r) = fields.iter().find(|r| r.start == i) {
            i = r.end;
            continue;
        }
        let rest = &text[i..];
        let c = rest.chars().next().unwrap_or(' ');
        if c == '\\' {
            i += match rest[1..].chars().next() {
                Some(n) if "PNnLlOoKk~\\{}".contains(n) => 1 + n.len_utf8(),
                Some(_) => rest.find(';').map_or(rest.len(), |p| p + 1),
                None => 1,
            };
            continue;
        }
        if rest.starts_with("%%") {
            i += rest.chars().take(3).map(char::len_utf8).sum::<usize>();
            continue;
        }
        if c.is_alphanumeric() {
            let len: usize =
                rest.chars().take_while(|ch| ch.is_alphanumeric() || *ch == '\'').map(char::len_utf8).sum();
            let token = rest[..len].trim_end_matches('\'');
            if token.chars().count() >= 2 && !token.chars().any(|ch| ch.is_numeric()) {
                out.push((i, token));
            }
            i += len;
            continue;
        }
        i += c.len_utf8();
    }
    out
}

/// Which of an object's texts a word is in.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    Content,
    Attribute(usize),
    Cell(usize, usize),
}

/// The texts of `g` that are checked.
fn texts(g: &Geometry) -> Vec<(Place, &str)> {
    match g {
        Geometry::Text { content, .. } => vec![(Place::Content, content.as_str())],
        Geometry::Dimension { text: Some(t), .. }
        | Geometry::RadialDimension { text: Some(t), .. }
        | Geometry::AngularDimension { text: Some(t), .. } => vec![(Place::Content, t.as_str())],
        Geometry::Insert { attributes, .. } => {
            attributes.iter().enumerate().map(|(i, a)| (Place::Attribute(i), a.value.as_str())).collect()
        }
        Geometry::Table { cells, .. } => cells
            .iter()
            .enumerate()
            .flat_map(|(r, row)| row.iter().enumerate().map(move |(c, s)| (Place::Cell(r, c), s.as_str())))
            .collect(),
        _ => Vec::new(),
    }
}

/// `g`'s text at `place`, to change.
fn text_mut(g: &mut Geometry, place: Place) -> Option<&mut String> {
    match (g, place) {
        (Geometry::Text { content, .. }, Place::Content) => Some(content),
        (
            Geometry::Dimension { text: Some(t), .. }
            | Geometry::RadialDimension { text: Some(t), .. }
            | Geometry::AngularDimension { text: Some(t), .. },
            Place::Content,
        ) => Some(t),
        (Geometry::Insert { attributes, .. }, Place::Attribute(i)) => attributes.get_mut(i).map(|a| &mut a.value),
        (Geometry::Table { cells, .. }, Place::Cell(r, c)) => cells.get_mut(r).and_then(|row| row.get_mut(c)),
        _ => None,
    }
}

/// `text` with its `nth` `word` (as [`words`] finds them) made `new`.
fn replace_nth(text: &str, word: &str, nth: usize, new: &str) -> String {
    match words(text).into_iter().filter(|(_, w)| *w == word).nth(nth) {
        Some((at, _)) => format!("{}{new}{}", &text[..at], &text[at + word.len()..]),
        None => text.to_string(),
    }
}

/// A word not found: in which object, which of its texts, and which
/// occurrence of it there.
#[derive(Debug, Clone, PartialEq)]
struct Miss {
    id: EntityId,
    place: Place,
    word: String,
    nth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Scope {
    Drawing,
    Space,
    Selected,
}

pub(crate) struct SpellDialog {
    scope: Scope,
    /// Still to show, the current one first; `None` before Start.
    misses: Option<Vec<Miss>>,
    suggestions: Vec<String>,
    change_to: String,
    ignored: HashSet<String>,
    status: String,
}

impl OxidraftApp {
    pub(crate) fn open_spell(&mut self) {
        let scope = if self.engine.selection().is_empty() { Scope::Drawing } else { Scope::Selected };
        let status = if dictionary().is_empty() {
            format!("No word list was found ({}): only your own words are known.", WORD_LISTS[0])
        } else {
            String::new()
        };
        self.spell = Some(SpellDialog {
            scope,
            misses: None,
            suggestions: Vec::new(),
            change_to: String::new(),
            ignored: HashSet::new(),
            status,
        });
    }

    /// True if `w` is in the system's words or the user's.
    fn spelled(&self, w: &str) -> bool {
        let own: HashSet<String> = self.settings.words.iter().map(|w| w.to_lowercase()).collect();
        known(w, &|x| dictionary().contains(x) || own.contains(x))
    }

    fn find_misses(&self, scope: Scope) -> Vec<Miss> {
        let ids: Vec<EntityId> = match scope {
            Scope::Drawing => self.doc.all_entities().map(|e| e.id).collect(),
            Scope::Space => self.doc.entities_in_draw_order().iter().map(|e| e.id).collect(),
            Scope::Selected => self.engine.selection().to_vec(),
        };
        let mut out = Vec::new();
        for id in ids {
            let Some(e) = self.doc.entity(id) else { continue };
            for (place, text) in texts(&e.geometry) {
                let mut counts: std::collections::HashMap<&str, usize> = Default::default();
                for (_, w) in words(text) {
                    let nth = counts.entry(w).or_default();
                    if !self.spelled(w) {
                        out.push(Miss { id, place, word: w.to_string(), nth: *nth });
                    }
                    *nth += 1;
                }
            }
        }
        out
    }

    /// Shows the next word not found (zoomed to and selected), or that
    /// the check is done.
    fn next_miss(&mut self, d: &mut SpellDialog) {
        let Some(misses) = &mut d.misses else { return };
        misses.retain(|m| !d.ignored.contains(&m.word.to_lowercase()));
        match misses.first() {
            Some(m) => {
                d.suggestions = suggestions(&m.word, &|x| {
                    dictionary().contains(x) || self.settings.words.iter().any(|w| w.eq_ignore_ascii_case(x))
                });
                d.change_to = d.suggestions.first().cloned().unwrap_or_else(|| m.word.clone());
                if let Some(b) = self
                    .doc
                    .entity(m.id)
                    .map(|e| self.doc.expanded(e))
                    .and_then(|g| g.iter().map(Geometry::bounds).reduce(Aabb::union))
                {
                    let pad = (b.max - b.min).length().max(1.0) * 0.5;
                    let grown = Aabb {
                        min: b.min - oxidraft_geom::Point::new(pad, pad),
                        max: b.max + oxidraft_geom::Point::new(pad, pad),
                    };
                    self.pending_views.push(ViewRequest::Window(grown));
                }
                self.engine.set_selection(vec![m.id]);
            }
            None => {
                d.misses = None;
                d.suggestions.clear();
                d.change_to.clear();
                d.status = "Spelling check complete.".into();
                self.engine.log(d.status.clone());
            }
        }
    }

    /// Changes `m`'s word to `new`; later misses of the same word in the
    /// same text move up one.
    fn change_word(&mut self, m: &Miss, new: &str, later: &mut [Miss]) {
        self.doc.update(m.id, |e| {
            if let Some(t) = text_mut(&mut e.geometry, m.place) {
                *t = replace_nth(t, &m.word, m.nth, new);
            }
        });
        for l in later.iter_mut().filter(|l| l.id == m.id && l.place == m.place && l.word == m.word && l.nth > m.nth) {
            l.nth -= 1;
        }
    }

    pub(crate) fn spell_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.spell.take() else { return };
        let (mut open, mut close) = (true, false);
        let (mut start, mut add, mut ignore, mut ignore_all, mut change, mut change_all) =
            (false, false, false, false, false, false);
        let mut fields = Vec::new();
        let current = d.misses.as_ref().and_then(|m| m.first()).cloned();
        egui::Window::new("Check Spelling")
            .id(egui::Id::unique("spell_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 140.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Where to check:");
                    let names = [
                        (Scope::Drawing, "Entire drawing"),
                        (Scope::Space, "Current space/layout"),
                        (Scope::Selected, "Selected objects"),
                    ];
                    let shown = names.iter().find(|n| n.0 == d.scope).map_or("", |n| n.1);
                    egui::ComboBox::from_id_salt("spell_scope").selected_text(shown).show_ui(ui, |ui| {
                        for (s, name) in names {
                            ui.selectable_value(&mut d.scope, s, name);
                        }
                    });
                });
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        ui.label("Not in dictionary:");
                        let word = current.as_ref().map_or("", |m| m.word.as_str());
                        ui.label(RichText::new(word).strong().monospace());
                        ui.label("Suggestions:");
                        let r = ui.add(egui::TextEdit::singleline(&mut d.change_to).desired_width(200.0));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Change to"));
                        fields.push(r.id);
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(vec2(200.0, 150.0));
                            egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                                for s in &d.suggestions {
                                    if ui.selectable_label(d.change_to == *s, s).clicked() {
                                        d.change_to.clone_from(s);
                                    }
                                }
                            });
                        });
                    });
                    ui.vertical(|ui| {
                        let checking = current.is_some();
                        let ready = checking && !d.change_to.trim().is_empty();
                        let button = |ui: &mut egui::Ui, enabled: bool, label: &str| {
                            ui.add_enabled_ui(enabled, |ui| {
                                ui.add_sized([130.0, 22.0], egui::Button::new(label)).clicked()
                            })
                            .inner
                        };
                        start = button(ui, !checking, "Start");
                        add = button(ui, checking, "Add to Dictionary");
                        ignore = button(ui, checking, "Ignore");
                        ignore_all = button(ui, checking, "Ignore All");
                        change = button(ui, ready, "Change");
                        change_all = button(ui, ready, "Change All");
                    });
                });
                if !d.status.is_empty() {
                    ui.colored_label(theme::text_dim(), &d.status);
                }
                let row = vec2(ui.available_width(), 24.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button("Close").clicked();
                });
            });
        self.text_fields.extend(fields);
        if start {
            d.status.clear();
            d.ignored.clear();
            d.misses = Some(self.find_misses(d.scope));
            self.next_miss(&mut d);
        }
        if let Some(m) = current {
            let lower = m.word.to_lowercase();
            if add {
                let mut settings = self.settings.clone();
                settings.words.push(m.word.clone());
                self.apply_settings(ctx, settings);
                d.ignored.insert(lower);
            } else if ignore_all {
                d.ignored.insert(lower);
            } else if ignore || change || change_all {
                let new = d.change_to.trim().to_string();
                let misses = d.misses.as_mut().map(std::mem::take).unwrap_or_default();
                let mut rest: Vec<Miss> = misses.into_iter().skip(1).collect();
                if change || change_all {
                    self.doc.begin("SPELL");
                    self.change_word(&m, &new, &mut rest);
                    if change_all {
                        while let Some(i) = rest.iter().position(|r| r.word == m.word) {
                            let r = rest.remove(i);
                            self.change_word(&r, &new, &mut rest);
                        }
                    }
                    self.doc.commit();
                }
                d.misses = Some(rest);
            }
            if add || ignore || ignore_all || change || change_all {
                self.next_miss(&mut d);
            }
        }
        if open && !close {
            self.spell = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words_of(list: &[&str]) -> impl Fn(&str) -> bool {
        let set: HashSet<String> = list.iter().map(|s| s.to_string()).collect();
        move |w| set.contains(w)
    }

    #[test]
    fn words_skip_codes_fields_and_numbers() {
        let text = r"{\fArial|b1;CONCRETE} wall\Pdoor's %%c100 M10 %<\AcVar Date>% x";
        let found: Vec<&str> = words(text).into_iter().map(|(_, w)| w).collect();
        assert_eq!(found, ["CONCRETE", "wall", "door's"]);
    }

    #[test]
    fn endings_word_lists_leave_out() {
        let w = words_of(&["column", "body", "stop", "plan", "make"]);
        for word in ["columns", "Columns", "bodies", "stopped", "planning", "making", "COLUMN's"] {
            assert!(known(word, &w), "{word}");
        }
        assert!(!known("colums", &w));
    }

    #[test]
    fn suggestions_in_the_words_case() {
        let w = words_of(&["column", "colon", "wall"]);
        assert_eq!(suggestions("colum", &w), ["column"]);
        assert_eq!(suggestions("COLUM", &w), ["COLUMN"]);
        assert_eq!(suggestions("Wal", &w), ["Wall"]);
    }

    #[test]
    fn changes_the_right_occurrence() {
        assert_eq!(replace_nth("teh cat and teh dog", "teh", 1, "the"), "teh cat and the dog");
        assert_eq!(replace_nth(r"\Pteh", "teh", 0, "the"), r"\Pthe");
    }

    #[test]
    fn its_own_words_where_the_system_has_none() {
        let own = super::own_words();
        assert!(own.len() > 200_000);
        assert!(own.contains("drawing") && own.contains("window") && !own.contains("drawnig"));
    }
}

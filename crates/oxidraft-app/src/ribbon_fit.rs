//! The ribbon fitted to the window: every panel stays, each showing as
//! many of its items (a large button, a column of small ones…) as there's
//! room for, the rest in its slide-out. Items go from the widest panel
//! first, its last ones first; a panel left with none shows one button
//! that opens them all. Some panels (Layers, Properties, the editors')
//! keep all theirs.
//! Each item's width is measured as it's drawn, and the next frame fits
//! by those widths.

use std::collections::HashMap;

use eframe::egui::Ui;

/// Which of a panel's items a pass over it draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pass {
    /// The ribbon's: the first `shown`.
    Ribbon,
    /// The slide-out's: those the ribbon hasn't room for.
    Hidden,
    /// The slide-out's own (what it always has).
    More,
}

/// A pass over a panel's items: which it draws, and their widths as drawn.
pub(crate) struct Items {
    pass: Pass,
    shown: usize,
    count: usize,
    widths: Vec<(usize, f32)>,
    has_more: bool,
}

impl Items {
    pub(crate) fn new(pass: Pass, shown: usize) -> Self {
        Self { pass, shown, count: 0, widths: Vec::new(), has_more: false }
    }

    /// One of the panel's items, drawn by `add` if this pass draws it.
    pub(crate) fn item(&mut self, ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        let i = self.count;
        self.count += 1;
        let drawn = match self.pass {
            Pass::Ribbon => i < self.shown,
            Pass::Hidden => i >= self.shown,
            Pass::More => false,
        };
        if drawn {
            let width = ui.scope(add).response.rect.width();
            if self.pass == Pass::Ribbon {
                self.widths.push((i, width));
            }
        }
    }

    /// What the panel's slide-out always has, after the items the ribbon
    /// hasn't room for.
    pub(crate) fn more(&mut self, ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        self.has_more = true;
        if self.pass == Pass::More {
            add(ui);
        }
    }

    /// The widths of the items drawn in the ribbon, by index.
    pub(crate) fn measured(&self) -> &[(usize, f32)] {
        &self.widths
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    pub(crate) fn has_more(&self) -> bool {
        self.has_more
    }

    /// Whether the ribbon hasn't room for some of them.
    pub(crate) fn any_hidden(&self) -> bool {
        self.shown < self.count
    }
}

/// A panel as last drawn: its title's width, whether it keeps all its
/// items, and each item's width.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Panel {
    pub key: String,
    pub fixed: bool,
    pub title: f32,
    pub widths: Vec<f32>,
}

/// An item's width before it's been drawn.
const UNMEASURED: f32 = 60.0;
/// A panel's padding about its items, and the line after it.
const PADDING: f32 = 9.0;

impl Panel {
    /// The narrowest a panel's title lets it be.
    pub(crate) fn title_room(title: f32) -> f32 {
        title + 30.0
    }

    /// Its width showing its first `n` items (none: its one button).
    fn width(&self, n: usize) -> f32 {
        let items = if n == 0 && !self.widths.is_empty() {
            (self.title + 10.0).max(58.0)
        } else {
            self.widths[..n].iter().sum()
        };
        (items + PADDING - 1.0).max(Self::title_room(self.title)) + 1.0
    }
}

/// How many of each panel's items fit in `room`: all where they do; else
/// the widest panel gives up its last item, again and again.
pub(crate) fn fit(panels: &[Panel], room: f32) -> Vec<usize> {
    let mut shown: Vec<usize> = panels.iter().map(|p| p.widths.len()).collect();
    let total = |shown: &[usize]| panels.iter().zip(shown).map(|(p, n)| p.width(*n)).sum::<f32>();
    while total(&shown) > room {
        let widest = (0..panels.len())
            .filter(|&k| !panels[k].fixed && shown[k] > 0)
            .max_by(|&a, &b| panels[a].width(shown[a]).total_cmp(&panels[b].width(shown[b])));
        match widest {
            Some(k) => shown[k] -= 1,
            None => break,
        }
    }
    shown
}

/// The ribbon's fitting, from frame to frame: each tab's panels as last
/// drawn, and how many items each shows this frame.
#[derive(Default)]
pub(crate) struct RibbonFit {
    tabs: HashMap<String, Vec<Panel>>,
    tab: String,
    room: f32,
    shown: HashMap<String, usize>,
    drawn: Vec<Panel>,
}

impl RibbonFit {
    /// Starts a frame of tab `tab`, `room` wide.
    pub(crate) fn begin(&mut self, tab: String, room: f32) {
        let last = self.tabs.get(&tab).cloned().unwrap_or_default();
        self.shown = last.iter().map(|p| p.key.clone()).zip(fit(&last, room)).collect();
        (self.tab, self.room) = (tab, room);
        self.drawn.clear();
    }

    /// The key of the next panel drawn, titled `title`.
    pub(crate) fn next_key(&self, title: &str) -> String {
        format!("{}:{title}", self.drawn.len())
    }

    /// How many of panel `key`'s items to show (all, the first time).
    pub(crate) fn shown(&self, key: &str) -> usize {
        self.shown.get(key).copied().unwrap_or(usize::MAX)
    }

    /// Panel `key` drawn: its items' widths as measured (those not drawn
    /// as they were last).
    pub(crate) fn drawn(&mut self, key: String, fixed: bool, title: f32, count: usize, measured: &[(usize, f32)]) {
        let last = self.tabs.get(&self.tab).and_then(|t| t.iter().find(|p| p.key == key));
        let mut widths = last.map(|p| p.widths.clone()).unwrap_or_default();
        widths.resize(count, UNMEASURED);
        for &(i, w) in measured {
            if let Some(slot) = widths.get_mut(i) {
                *slot = w;
            }
        }
        self.drawn.push(Panel { key, fixed, title, widths });
    }

    /// Ends the frame: whether the next one fits differently (and should
    /// follow at once).
    pub(crate) fn end(&mut self) -> bool {
        let drawn = std::mem::take(&mut self.drawn);
        let next: HashMap<String, usize> = drawn.iter().map(|p| p.key.clone()).zip(fit(&drawn, self.room)).collect();
        let changed = next != self.shown;
        self.tabs.insert(self.tab.clone(), drawn);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(key: &str, fixed: bool, widths: &[f32]) -> Panel {
        Panel { key: key.into(), fixed, title: 30.0, widths: widths.to_vec() }
    }

    #[test]
    fn the_widest_panel_gives_up_items_first() {
        let panels =
            [panel("Draw", false, &[60.0; 5]), panel("Layers", true, &[300.0]), panel("Block", false, &[60.0, 60.0])];
        let full: f32 = panels.iter().map(|p| p.width(p.widths.len())).sum();
        assert_eq!(fit(&panels, full), [5, 1, 2], "all of them, with room");
        assert_eq!(fit(&panels, full - 60.0), [4, 1, 2]);
        assert_eq!(fit(&panels, full - 61.0), [3, 1, 2], "Draw is still the wider");
        // No room at all: every panel kept, as its one button; Layers whole.
        assert_eq!(fit(&panels, 0.0), [0, 1, 0]);
    }
}

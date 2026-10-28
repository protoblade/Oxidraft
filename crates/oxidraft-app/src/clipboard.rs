//! Copy and paste through the system clipboard. Copied objects go there as
//! DXF text, so they paste into another Oxidraft window too.

use eframe::egui::{self, Event, Key, Modifiers};
use oxidraft_model::Document;

use crate::app::OxidraftApp;

/// True if clipboard text is a DXF drawing rather than ordinary text.
fn looks_like_dxf(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with('0') && t.contains("SECTION") && t.contains("ENTITIES")
}

/// The drawing in clipboard text, if it's DXF with objects in it.
fn clip_from(text: &str) -> Option<Document> {
    if !looks_like_dxf(text) {
        return None;
    }
    let doc = oxidraft_io_dxf::read(&mut text.as_bytes()).ok()?.document;
    let empty = doc.entities().next().is_none();
    (!empty).then_some(doc)
}

impl OxidraftApp {
    /// Puts what a command just copied on the clipboard.
    pub(crate) fn publish_copied(&mut self, ctx: &egui::Context) {
        let Some(clip) = self.engine.take_copied() else { return };
        let mut buf = Vec::new();
        if oxidraft_io_dxf::write(&clip, &mut buf).is_err() {
            return;
        }
        let text = String::from_utf8_lossy(&buf).into_owned();
        ctx.copy_text(text.clone());
        self.clipboard = Some(text);
    }

    /// Typed PASTECLIP or PASTEORIG: from the system clipboard, or else what
    /// was last copied here.
    pub(crate) fn paste_command(&mut self, original: bool) {
        let system = arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok().filter(|t| looks_like_dxf(t));
        match system.or_else(|| self.clipboard.clone()).as_deref().and_then(clip_from) {
            Some(clip) => self.engine.paste(clip, original, &mut self.doc),
            None => self.engine.log("Nothing to paste: the clipboard holds no drawing objects."),
        }
    }

    /// Cmd+C, Cmd+X, Cmd+Shift+C and Cmd+V. Copying text typed on the
    /// command line, or in a field, still works as usual.
    pub(crate) fn handle_clipboard_keys(&mut self, ctx: &egui::Context) {
        if self.editing_field(ctx) {
            return;
        }
        let typing = !self.command_text.is_empty();
        let copy_base = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::C));
        let (mut copy, mut cut, mut pasted) = (false, false, None);
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Copy if !typing => {
                    copy = true;
                    false
                }
                Event::Cut if !typing => {
                    cut = true;
                    false
                }
                Event::Paste(text) => match clip_from(text) {
                    Some(clip) => {
                        pasted = Some(clip);
                        false
                    }
                    None => true,
                },
                _ => true,
            });
        });
        if copy_base {
            self.run("COPYBASE");
        } else if copy {
            self.run("COPYCLIP");
        } else if cut {
            self.run("CUTCLIP");
        }
        if let Some(clip) = pasted {
            self.engine.paste(clip, false, &mut self.doc);
        }
    }
}

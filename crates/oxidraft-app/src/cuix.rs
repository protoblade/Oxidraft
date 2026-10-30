//! AutoCAD's customization files loaded as AutoCAD's CUILOAD loads a
//! partial one: a .cuix (a zip of XML parts and the images its buttons
//! use) or the single XML .cui of AutoCAD 2006 to 2009. Its commands
//! (their macros and button images), ribbon tabs and panels and keyboard
//! shortcuts join those made in CUI, under the file's customization group,
//! until unloaded. The Load/Unload Customizations dialog (CUILOAD,
//! CUIUNLOAD, MENULOAD) lists the groups loaded, to unload, and goes back
//! to the built-in shortcuts and Quick Access Toolbar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, vec2};

use crate::app::OxidraftApp;
use crate::menubar::{CustomMenu, CustomToolbar, MenuEntry};
use crate::settings::{CustomCommand, RibbonPanel, Settings};
use crate::theme;

/// An XML element: its name, attributes, text and the elements in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Element>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Every element in it (itself first) that `pick` picks, in the order
    /// they're written.
    fn every<'a>(&'a self, pick: &dyn Fn(&Element) -> bool, out: &mut Vec<&'a Element>) {
        if pick(self) {
            out.push(self);
        }
        for c in &self.children {
            c.every(pick, out);
        }
    }

    fn named(&self, name: &str) -> Vec<&Element> {
        let mut out = Vec::new();
        self.every(&|e| e.name == name, &mut out);
        out
    }
}

/// XML text with its entities and character references read.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e < 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let c = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => match entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                Some(hex) => u32::from_str_radix(hex, 16).ok().and_then(char::from_u32),
                None => entity.strip_prefix('#').and_then(|d| d.parse().ok()).and_then(char::from_u32),
            },
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A tag's name and attributes, from what's between its `<` and `>`.
fn tag_parts(tag: &str) -> (String, Vec<(String, String)>) {
    let tag = tag.trim();
    let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
    let mut attrs = Vec::new();
    let mut rest = &tag[name_end..];
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|q| matches!(q, '"' | '\'')) else { break };
        let Some(close) = after[1..].find(quote) else { break };
        attrs.push((name, unescape(&after[1..1 + close])));
        rest = &after[close + 2..];
    }
    (tag[..name_end].to_string(), attrs)
}

/// Where the tag starting `s` ends: its `>`, outside quoted values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('>', None) => return Some(i),
            _ => {}
        }
    }
    None
}

/// XML text as its elements, under a nameless root.
pub(crate) fn parse_xml(text: &str) -> Element {
    let mut stack = vec![Element::default()];
    let close = |stack: &mut Vec<Element>| {
        if stack.len() > 1
            && let Some(e) = stack.pop()
            && let Some(parent) = stack.last_mut()
        {
            parent.children.push(e);
        }
    };
    let mut rest = text;
    while let Some(lt) = rest.find('<') {
        if let Some(top) = stack.last_mut() {
            top.text.push_str(&unescape(&rest[..lt]));
        }
        rest = &rest[lt..];
        if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").unwrap_or(body.len());
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&body[..end]);
            }
            rest = body.get(end + 3..).unwrap_or("");
            continue;
        }
        if rest.starts_with("<!--") {
            rest = rest.find("-->").map_or("", |i| &rest[i + 3..]);
            continue;
        }
        let Some(gt) = tag_end(rest) else { break };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        if tag.starts_with(['?', '!']) {
            continue;
        }
        if tag.starts_with('/') {
            close(&mut stack);
            continue;
        }
        let empty = tag.ends_with('/');
        let (name, attrs) = tag_parts(tag.trim_end_matches('/'));
        stack.push(Element { name, attrs, ..Default::default() });
        if empty {
            close(&mut stack);
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().unwrap_or_default()
}

/// A part's text: UTF-8 or UTF-16 (by its byte order mark).
fn part_text(data: &[u8]) -> String {
    match data {
        [0xFF, 0xFE, rest @ ..] => String::from_utf16_lossy(
            &rest.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect::<Vec<_>>(),
        ),
        [0xFE, 0xFF, rest @ ..] => String::from_utf16_lossy(
            &rest.as_chunks::<2>().0.iter().map(|b| u16::from_be_bytes(*b)).collect::<Vec<_>>(),
        ),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(data).into_owned(),
    }
}

/// What a customization file holds, read.
#[derive(Debug, Default, PartialEq)]
pub struct Customization {
    /// Its customization group's name.
    pub group: String,
    /// Its commands, their images the names of files in `images`.
    pub commands: Vec<CustomCommand>,
    pub panels: Vec<RibbonPanel>,
    /// Keys and the id of the command they run.
    pub shortcuts: Vec<(String, String)>,
    /// Its menus (those of the menu bar) and toolbars.
    pub menus: Vec<CustomMenu>,
    pub toolbars: Vec<CustomToolbar>,
    /// The images its buttons use: file name and contents.
    pub images: Vec<(String, Vec<u8>)>,
    pub workspaces: Vec<CuiWorkspace>,
}

/// A workspace a customization file defines: the menu bar shown or not,
/// the toolbars it shows and its ribbon's first tab.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CuiWorkspace {
    pub name: String,
    pub menubar: bool,
    pub toolbars: Vec<String>,
    pub tab: Option<String>,
}

/// The file name in a path written either way.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A customization file's commands, ribbon and shortcuts; `fallback`
/// names its group when it doesn't. Its buttons' images are found in
/// `resources` (name and file) too, where the file doesn't hold them.
pub fn read_customization(
    data: &[u8],
    fallback: &str,
    resources: Vec<(String, Vec<u8>)>,
) -> Result<Customization, String> {
    let parts = if data.starts_with(b"PK") {
        crate::datalink::unzip(data)
    } else {
        vec![("customization.cui".to_string(), data.to_vec())]
    };
    let mut docs = Vec::new();
    let mut images = Vec::new();
    for (name, content) in parts {
        let lower = name.to_ascii_lowercase();
        if [".png", ".bmp", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e)) {
            images.push((file_name(&name).to_string(), content));
        } else if !lower.contains("content_types") && !lower.ends_with(".rels") {
            let text = part_text(&content);
            if text.trim_start().starts_with('<') {
                docs.push(parse_xml(&text));
            }
        }
    }
    let stem = |s: &str| s.rsplit_once('.').map_or(s, |(a, _)| a).to_ascii_lowercase();
    let held: std::collections::HashSet<String> = images.iter().map(|(f, _)| stem(f)).collect();
    images.extend(resources.into_iter().filter(|(f, _)| !held.contains(&stem(f))));
    let every = |name: &str| docs.iter().flat_map(|d| d.named(name)).collect::<Vec<_>>();
    if every("CustSection").is_empty() && every("MenuGroup").is_empty() && every("RibbonRoot").is_empty() {
        return Err("Not an AutoCAD customization file.".into());
    }
    let group = every("MenuGroup")
        .iter()
        .find_map(|g| g.attr("Name").filter(|n| !n.trim().is_empty()))
        .unwrap_or(fallback)
        .to_string();
    let text_of = |e: &Element, name: &str| e.child(name).map(|c| c.text.trim().to_string()).unwrap_or_default();
    // Commands: their UIDs made unique to the group.
    let id = |uid: &str| format!("{group}:{uid}");
    let mut commands = Vec::new();
    let mut known: HashMap<String, String> = HashMap::new();
    for m in every("MenuMacro") {
        let Some(uid) = m.attr("UID") else { continue };
        let body = m.child("Macro").unwrap_or(m);
        let image = ["SmallImage", "LargeImage"]
            .iter()
            .filter_map(|n| body.child(n)?.attr("Name"))
            .find_map(|n| {
                let want = file_name(n).to_ascii_lowercase();
                let stem = |s: &str| s.rsplit_once('.').map_or(s, |(a, _)| a).to_string();
                images
                    .iter()
                    .find(|(f, _)| f.to_ascii_lowercase() == want || stem(&f.to_ascii_lowercase()) == stem(&want))
                    .map(|(f, _)| f.clone())
            })
            .unwrap_or_default();
        let name = Some(text_of(body, "Name")).filter(|n| !n.is_empty()).unwrap_or_else(|| uid.to_string());
        known.insert(uid.to_string(), id(uid));
        commands.push(CustomCommand {
            id: id(uid),
            name: name.replace('|', "/"),
            macro_text: text_of(body, "Command"),
            image,
            group: group.clone(),
        });
    }
    // The command an element runs: the macro one of its attributes names,
    // or one of the elements in it (a menu item's MacroRef).
    fn macro_of(e: &Element, known: &HashMap<String, String>) -> Option<String> {
        e.attrs.iter().find_map(|(_, v)| known.get(v).cloned()).or_else(|| {
            e.children
                .iter()
                .filter(|c| !c.name.contains("Button") && c.name != "PopMenuItem")
                .find_map(|c| macro_of(c, known))
        })
    }
    let runs = |e: &Element| macro_of(e, &known);
    // Panels: a button a command, a split button its commands as `A|B|C`.
    fn entries(e: &Element, runs: &dyn Fn(&Element) -> Option<String>, out: &mut Vec<String>) {
        for c in &e.children {
            if c.name.contains("SplitButton") {
                let mut buttons = Vec::new();
                c.every(&|b| b.name.ends_with("Button") && !b.name.contains("SplitButton"), &mut buttons);
                let ids: Vec<String> = buttons.iter().filter_map(|b| runs(b)).collect();
                if !ids.is_empty() {
                    out.push(ids.join("|"));
                }
            } else if c.name.ends_with("Button")
                && let Some(id) = runs(c)
            {
                out.push(id);
            } else {
                entries(c, runs, out);
            }
        }
    }
    let mut panels: Vec<(String, RibbonPanel)> = Vec::new();
    for p in every("RibbonPanelSource") {
        let mut commands = Vec::new();
        entries(p, &runs, &mut commands);
        let uid = p.attr("UID").unwrap_or_default().to_string();
        let name = p.attr("Text").or_else(|| p.attr("Name")).filter(|t| !t.is_empty()).unwrap_or(&uid).to_string();
        if !commands.is_empty() {
            panels.push((uid, RibbonPanel { tab: String::new(), name, commands, group: group.clone() }));
        }
    }
    // Each on the tabs that show it; those on none on a tab of the group's.
    let mut placed = Vec::new();
    let mut on_tabs = std::collections::HashSet::new();
    for t in every("RibbonTabSource") {
        let tab = t.attr("Text").or_else(|| t.attr("Name")).unwrap_or(&group).to_string();
        let mut refs = Vec::new();
        t.every(&|e| e.name.contains("PanelSourceReference"), &mut refs);
        for r in refs {
            if let Some((uid, p)) = panels.iter().find(|(uid, _)| r.attrs.iter().any(|(_, v)| v == uid)) {
                on_tabs.insert(uid.clone());
                placed.push(RibbonPanel { tab: tab.clone(), ..p.clone() });
            }
        }
    }
    for (uid, p) in &panels {
        if !on_tabs.contains(uid) {
            placed.push(RibbonPanel { tab: group.clone(), ..p.clone() });
        }
    }
    // Keyboard shortcuts.
    let mut shortcuts = Vec::new();
    for a in every("Accelerator") {
        let Some(command) = runs(a) else { continue };
        let mut keys = Vec::new();
        a.every(&|e| e.name.contains("ShortcutKey"), &mut keys);
        if let Some(k) = keys.first().map(|k| k.text.trim()).filter(|k| !k.is_empty()) {
            shortcuts.push((k.to_string(), command));
        }
    }
    // The menu bar's menus (POP1 to POP499; the others are shortcut
    // menus), each item a command, a line, or a menu of its own.
    let label = |e: &Element| text_of(e, "Name").replace('&', "");
    fn items(
        e: &Element,
        label: &dyn Fn(&Element) -> String,
        runs: &dyn Fn(&Element) -> Option<String>,
    ) -> Vec<MenuEntry> {
        let mut out = Vec::new();
        for c in &e.children {
            match c.name.as_str() {
                "PopMenu" => {
                    out.push(MenuEntry { label: label(c), command: String::new(), items: items(c, label, runs) })
                }
                "PopMenuItem" => {
                    let command = runs(c).unwrap_or_default();
                    let separator = c.attr("IsSeparator").is_some_and(|v| v.eq_ignore_ascii_case("true"));
                    if separator || command.is_empty() {
                        out.push(MenuEntry::default());
                    } else {
                        out.push(MenuEntry { label: label(c), command, items: Vec::new() });
                    }
                }
                _ => {}
            }
        }
        out
    }
    let pulldown = |m: &Element| {
        let aliases: Vec<String> =
            m.children.iter().filter(|c| c.name == "Alias").map(|c| c.text.trim().to_ascii_uppercase()).collect();
        aliases.is_empty()
            || aliases.iter().any(|a| {
                a.strip_prefix("POP").and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| (1..500).contains(&n))
            })
    };
    let mut menus = Vec::new();
    for root in every("PopMenuRoot") {
        for m in root.children.iter().filter(|m| m.name == "PopMenu" && pulldown(m)) {
            let entries = items(m, &label, &runs);
            if entries.iter().any(|i| !i.command.is_empty() || !i.items.is_empty()) {
                menus.push(CustomMenu { name: label(m), items: entries, group: group.clone() });
            }
        }
    }
    // Toolbars: their buttons, `""` a line between them.
    let mut toolbars = Vec::new();
    for t in every("Toolbar") {
        let commands: Vec<String> = t
            .children
            .iter()
            .filter_map(|c| match c.name.as_str() {
                "ToolbarSeparator" => Some(String::new()),
                "ToolbarButton" => runs(c),
                _ => None,
            })
            .collect();
        if commands.iter().any(|c| !c.is_empty()) {
            toolbars.push(CustomToolbar { name: label(t), commands, group: group.clone() });
        }
    }
    // Workspaces: the toolbars and ribbon tabs each shows, by their UIDs.
    let by_uid = |kind: &str, name: &dyn Fn(&Element) -> String| -> HashMap<String, String> {
        every(kind).iter().filter_map(|e| Some((e.attr("UID")?.to_string(), name(e)))).collect()
    };
    let toolbar_names = by_uid("Toolbar", &label);
    let tab_names = by_uid("RibbonTabSource", &|t: &Element| {
        t.attr("Text").or_else(|| t.attr("Name")).unwrap_or_default().to_string()
    });
    let hidden =
        |v: &str| ["0", "false", "no", "hide", "hidden", "off"].contains(&v.trim().to_ascii_lowercase().as_str());
    let shown = |e: &Element| {
        ["Display", "Show", "Visible", "ToolbarVisible"].iter().filter_map(|a| e.attr(a)).all(|v| !hidden(v))
    };
    let target = |e: &Element, names: &HashMap<String, String>| e.attrs.iter().find_map(|(_, v)| names.get(v).cloned());
    let mut workspaces = Vec::new();
    for w in every("Workspace") {
        let name = Some(text_of(w, "Name")).filter(|n| !n.is_empty()).or_else(|| w.attr("Name").map(str::to_string));
        let Some(name) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) else { continue };
        let (mut bars, mut tabs) = (Vec::new(), Vec::new());
        w.every(&|e| e.name == "WorkspaceToolbar", &mut bars);
        w.every(&|e| e.name == "WorkspaceRibbonTabSourceReference", &mut tabs);
        workspaces.push(CuiWorkspace {
            name,
            menubar: w
                .attr("MenuBar")
                .is_some_and(|v| ["on", "true", "yes", "1", "show"].contains(&v.trim().to_ascii_lowercase().as_str())),
            toolbars: bars.iter().filter(|t| shown(t)).filter_map(|t| target(t, &toolbar_names)).collect(),
            tab: tabs.iter().filter(|t| shown(t)).find_map(|t| target(t, &tab_names)),
        });
    }
    let used: std::collections::HashSet<&str> = commands.iter().map(|c| c.image.as_str()).collect();
    images.retain(|(f, _)| used.contains(f.as_str()));
    Ok(Customization { group, commands, panels: placed, shortcuts, menus, toolbars, images, workspaces })
}

/// The command a macro starts (`^C^C_LINE` starts LINE), if it starts one
/// rather than AutoLISP.
pub(crate) fn macro_command(text: &str) -> Option<String> {
    let mut rest = text.trim().trim_start_matches('*');
    while let Some(r) = rest.strip_prefix("^C").or_else(|| rest.strip_prefix("^c")) {
        rest = r;
    }
    let word = rest.split([' ', ';', '\\']).next()?.trim_start_matches(['_', '.', '\'']);
    (!word.is_empty() && !word.starts_with('(')).then(|| word.to_ascii_uppercase())
}

/// A macro's stretch up to a pause (or its end) as what's entered: a
/// space or `;` ends each; the last is entered at the macro's end unless
/// an Enter or a pause ends it.
fn macro_tokens(segment: &str, paused: bool) -> Vec<String> {
    let mut tokens: Vec<String> = segment.split([' ', ';']).map(str::to_string).collect();
    if paused || segment.ends_with([' ', ';']) || segment.is_empty() {
        tokens.pop();
    }
    tokens
}

/// The Load/Unload Customizations dialog.
#[derive(Default)]
pub(crate) struct CuiLoadDialog {
    message: String,
}

impl OxidraftApp {
    /// The customization groups loaded.
    pub fn customization_groups(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let groups =
            self.settings.commands.iter().map(|c| &c.group).chain(self.settings.ribbon_panels.iter().map(|p| &p.group));
        for g in groups {
            if !g.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(g)) {
                out.push(g.clone());
            }
        }
        out
    }

    /// Where a group's button images are kept.
    fn customization_dir(&self, group: &str) -> PathBuf {
        let base = self
            .settings_path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| std::env::temp_dir().join("oxidraft"));
        let safe: String = group.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
        base.join("cui").join(safe)
    }

    /// CUILOAD: loads the customization file at `path` (in place of its
    /// group, if that's loaded already); the group's name.
    pub fn load_customization(&mut self, path: &Path) -> Result<String, String> {
        let data = std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        // Images it names but doesn't hold: in a resource DLL of its name
        // beside it, as AutoCAD keeps them (acad.cuix's in acadbtn.xmx).
        let dir = path.parent().unwrap_or(Path::new("."));
        let resources = [format!("{stem}.dll"), format!("{stem}.xmx"), format!("{stem}btn.xmx")]
            .iter()
            .filter_map(|f| std::fs::read(dir.join(f)).ok())
            .flat_map(|d| crate::pe_resources::resource_images(&d))
            .collect();
        let c = read_customization(&data, &stem, resources)?;
        let dir = self.customization_dir(&c.group);
        if !c.images.is_empty() {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        let mut saved = HashMap::new();
        for (name, bytes) in &c.images {
            let file = dir.join(name);
            std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
            saved.insert(name.clone(), file.to_string_lossy().into_owned());
        }
        let mut settings = self.without_group(&c.group);
        for mut command in c.commands {
            command.image = saved.get(&command.image).cloned().unwrap_or_default();
            settings.commands.push(command);
        }
        settings.ribbon_panels.extend(c.panels);
        settings.menus.extend(c.menus);
        // Toolbars shown, named apart from those there are.
        let taken = |n: &str, s: &Settings| {
            crate::menubar::TOOLBARS.iter().any(|(b, _)| b.eq_ignore_ascii_case(n))
                || s.custom_toolbars.iter().any(|x| x.name.eq_ignore_ascii_case(n))
        };
        let mut renamed = HashMap::new();
        for mut t in c.toolbars {
            if taken(&t.name, &settings) {
                let name = format!("{} ({})", t.name, t.group);
                renamed.insert(std::mem::replace(&mut t.name, name), t.name.clone());
            }
            settings.toolbars.push(t.name.clone());
            settings.custom_toolbars.push(t);
        }
        // Its workspaces (but AutoCAD's Drafting & Annotation, Oxidraft's
        // own), named apart from those there are.
        let tabs = crate::chrome::custom_tabs_of(&settings.ribbon_panels);
        for w in c.workspaces {
            if w.name.eq_ignore_ascii_case(crate::customize::DRAFTING) {
                continue;
            }
            let toolbars = w
                .toolbars
                .iter()
                .map(|t| renamed.get(t).unwrap_or(t).clone())
                .filter(|t| taken(t, &settings))
                .collect();
            let ribbon_tab = w.tab.as_deref().and_then(|t| crate::chrome::ribbon_tab_index(t, &tabs)).unwrap_or(0);
            let mut name = w.name;
            if settings.workspaces.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
                name = format!("{name} ({})", c.group);
            }
            settings.workspaces.push(crate::customize::Workspace {
                name,
                menubar: w.menubar,
                toolbars,
                ribbon_tab,
                group: c.group.clone(),
                ..Default::default()
            });
        }
        for (keys, command) in c.shortcuts {
            settings.shortcuts.retain(|(k, _)| !k.eq_ignore_ascii_case(&keys));
            settings.shortcuts.push((keys, command));
        }
        self.button_images.clear();
        self.keep_settings(settings);
        Ok(c.group)
    }

    /// The settings without group `group`'s commands, panels and the
    /// shortcuts that ran them.
    fn without_group(&self, group: &str) -> Settings {
        let mut s = self.settings.clone();
        let gone: Vec<String> =
            s.commands.iter().filter(|c| c.group.eq_ignore_ascii_case(group)).map(|c| c.id.clone()).collect();
        s.commands.retain(|c| !c.group.eq_ignore_ascii_case(group));
        s.ribbon_panels.retain(|p| !p.group.eq_ignore_ascii_case(group));
        s.menus.retain(|m| !m.group.eq_ignore_ascii_case(group));
        let toolbars: Vec<String> =
            s.custom_toolbars.iter().filter(|t| t.group.eq_ignore_ascii_case(group)).map(|t| t.name.clone()).collect();
        s.custom_toolbars.retain(|t| !t.group.eq_ignore_ascii_case(group));
        s.toolbars.retain(|t| !toolbars.contains(t));
        s.shortcuts.retain(|(_, c)| !gone.contains(c));
        s.workspaces.retain(|w| !w.group.eq_ignore_ascii_case(group));
        if !s.workspaces.iter().any(|w| w.name.eq_ignore_ascii_case(&s.workspace)) {
            s.workspace.clear();
        }
        s.quick_access.retain(|c| !gone.contains(c));
        s.shortcut_menu.retain(|c| !gone.contains(c));
        s
    }

    /// The group's commands, panels and shortcuts gone.
    pub fn unload_customization(&mut self, group: &str) {
        let settings = self.without_group(group);
        self.keep_settings(settings);
    }

    /// The command of your own (CUI, CUILOAD) named `id`.
    pub(crate) fn custom_command(&self, id: &str) -> Option<&CustomCommand> {
        let id = id.trim();
        self.settings.commands.iter().find(|c| c.id.eq_ignore_ascii_case(id))
    }

    /// A button image of your own, read once.
    pub(crate) fn button_image(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        if path.is_empty() {
            return None;
        }
        self.button_images
            .entry(path.to_string())
            .or_insert_with(|| {
                let image = image::open(path).ok()?.into_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                Some(ctx.load_texture(path, pixels, egui::TextureOptions::LINEAR))
            })
            .clone()
    }

    /// Runs a CUI macro as AutoCAD does: `^C^C` cancels what's running,
    /// a space or `;` is Enter (and the macro's end, unless one ends it),
    /// `_` and `.` before a name are dropped; at a `\` the macro waits for
    /// the user's input, then goes on. AutoLISP (`(command ...)`) is
    /// evaluated.
    pub(crate) fn run_macro(&mut self, text: &str) {
        self.command_text.clear();
        self.macro_rest = None;
        let mut rest = text.trim().trim_start_matches('*');
        let mut cancel = false;
        while let Some(r) = rest.strip_prefix("^C").or_else(|| rest.strip_prefix("^c")) {
            rest = r;
            cancel = true;
        }
        if cancel && self.engine.is_active() {
            self.engine.cancel(&mut self.doc);
        }
        let rest = rest.replace("^M", ";");
        let rest = rest.trim_start();
        if rest.starts_with('(') {
            self.lisp_typed(rest);
            return;
        }
        let (segment, after) = match rest.split_once('\\') {
            Some((before, after)) => (before, Some(after)),
            None => (rest, None),
        };
        let tokens = macro_tokens(segment, after.is_some());
        let Some(command) = tokens.first().map(|c| c.trim_start_matches(['_', '.', '\''])) else { return };
        if command.is_empty() || self.run_app_command(command) {
            return;
        }
        self.engine.run(command, &mut self.doc);
        self.feed_macro(&tokens[1..], after);
        self.lisp_pump();
    }

    /// Enters a macro's `tokens`; what's `after` a pause waits for the
    /// user's input.
    fn feed_macro(&mut self, tokens: &[String], after: Option<&str>) {
        let (cursor, tolerance) = (self.cursor.unwrap_or(oxidraft_geom::Point::ORIGIN), self.pick_tolerance());
        for t in tokens {
            let t = t.trim_start_matches('_');
            // Points as written: absolute, whatever dynamic input does
            // with typed ones.
            let point = t.contains(',') && t.split(',').all(|v| v.trim().parse::<f64>().is_ok());
            let t = if point { format!("#{t}") } else { t.to_string() };
            self.engine.submit(&t, &mut self.doc, cursor, tolerance);
        }
        if let Some(after) = after
            && self.engine.is_active()
        {
            self.macro_rest = Some((after.to_string(), self.engine.inputs_fed()));
        }
    }

    /// A paused macro goes on once the user has given the command an
    /// input; it's dropped when the command ends.
    pub(crate) fn macro_pump(&mut self) {
        let Some((text, fed)) = &self.macro_rest else { return };
        if !self.engine.is_active() {
            self.macro_rest = None;
            return;
        }
        if self.engine.inputs_fed() == *fed {
            return;
        }
        let text = text.clone();
        self.macro_rest = None;
        let (segment, after) = match text.split_once('\\') {
            Some((before, after)) => (before, Some(after)),
            None => (text.as_str(), None),
        };
        let tokens = macro_tokens(segment, after.is_some());
        self.feed_macro(&tokens, after);
    }

    pub(crate) fn open_cui_load(&mut self) {
        self.cui_load = Some(CuiLoadDialog::default());
    }

    /// The Load/Unload Customizations dialog: the groups loaded, each to
    /// unload, and a file to load.
    pub(crate) fn cui_load_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.cui_load.take() else { return };
        let (mut open, mut close, mut browse, mut unload, mut reset) = (true, false, false, None, false);
        let groups = self.customization_groups();
        egui::Window::new("Load/Unload Customizations")
            .id(egui::Id::unique("cui_load"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([360.0, 160.0])
            .constrain_to(self.dialog_area(ctx))
            .show(ctx, |ui| {
                ui.label(RichText::new("Loaded Customization Groups").strong());
                if groups.is_empty() {
                    ui.colored_label(theme::text_dim(), "None loaded.");
                }
                for g in &groups {
                    ui.horizontal(|ui| {
                        ui.label(g);
                        if ui.small_button("Unload").clicked() {
                            unload = Some(g.clone());
                        }
                    });
                }
                if !d.message.is_empty() {
                    ui.colored_label(theme::text_dim(), &d.message);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.set_min_width(320.0);
                    browse = ui.button("Load...").on_hover_text("Load a .cuix or .cui file").clicked();
                    reset = ui
                        .button("Built-in Shortcuts")
                        .on_hover_text("Back to the built-in shortcuts and Quick Access Toolbar")
                        .clicked();
                    close = ui.add(egui::Button::new("Close").min_size(vec2(60.0, 0.0))).clicked();
                });
            });
        if let Some(g) = unload {
            self.unload_customization(&g);
            d.message = format!("Unloaded {g}.");
        }
        if reset {
            let settings = Settings { shortcuts: Vec::new(), quick_access: Vec::new(), ..self.settings.clone() };
            self.keep_settings(settings);
            d.message = "Back to the built-in shortcuts and Quick Access Toolbar.".into();
        }
        if browse
            && let Some(file) = rfd::FileDialog::new().add_filter("Customization files", &["cuix", "cui"]).pick_file()
        {
            d.message = match self.load_customization(&file) {
                Ok(g) => format!("Loaded {g}."),
                Err(e) => e,
            };
        }
        if open && !close {
            self.cui_load = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<CustSection xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <!-- a partial customization -->
  <MenuGroup Name="SITE">
    <MacroGroup Name="SITE">
      <MenuMacro UID="MM_TREE">
        <Macro type="Any">
          <Name xlate="true" UID="XLS_1">Tree &amp; Shrub</Name>
          <Command>^C^C_CIRCLE;\2;</Command>
          <SmallImage Name="tree_16.png" />
        </Macro>
      </MenuMacro>
      <MenuMacro UID="MM_KERB">
        <Macro type="Any"><Name>Kerb</Name><Command>^C^C_PLINE</Command></Macro>
      </MenuMacro>
      <MenuMacro UID="MM_LAMP">
        <Macro type="Any"><Name>Lamp</Name><Command>^C^C(princ "lamp")</Command></Macro>
      </MenuMacro>
    </MacroGroup>
    <MenuRoot>
      <PopMenuRoot>
        <PopMenu UID="PM_1"><Alias>POP501</Alias><Name>Shortcut</Name>
          <PopMenuItem UID="PI_0"><Name>Kerb</Name><MenuItem><MacroRef MenuMacroID="MM_KERB"/></MenuItem></PopMenuItem>
        </PopMenu>
        <PopMenu UID="PM_2"><Alias>POP12</Alias><Name>&amp;Site</Name>
          <PopMenuItem UID="PI_1"><Name>&amp;Tree</Name><MenuItem><MacroRef MenuMacroID="MM_TREE"/></MenuItem></PopMenuItem>
          <PopMenuItem UID="PI_2" IsSeparator="true"/>
          <PopMenu UID="PM_3"><Name>More</Name>
            <PopMenuItem UID="PI_3"><Name>Lamp</Name><MenuItem><MacroRef MenuMacroID="MM_LAMP"/></MenuItem></PopMenuItem>
          </PopMenu>
        </PopMenu>
      </PopMenuRoot>
      <ToolbarRoot>
        <Toolbar UID="TB_1"><Name>Draw</Name>
          <ToolbarButton UID="TB_B1"><Name>Tree</Name><MenuItem><MacroRef MenuMacroID="MM_TREE"/></MenuItem></ToolbarButton>
          <ToolbarSeparator UID="TB_S1"/>
          <ToolbarButton UID="TB_B2" MenuMacroID="MM_KERB"><Name>Kerb</Name></ToolbarButton>
        </Toolbar>
      </ToolbarRoot>
      <AcceleratorRoot>
        <Accelerator UID="ACC_1" MenuMacroID="MM_KERB"><AccelShortcutKey>CTRL+SHIFT+K</AccelShortcutKey></Accelerator>
      </AcceleratorRoot>
    </MenuRoot>
  </MenuGroup>
  <RibbonRoot>
    <RibbonPanelSourceCollection>
      <RibbonPanelSource UID="RP_SITE" Text="Site Works">
        <RibbonRow UID="R1">
          <RibbonCommandButton UID="B1" Text="Tree" MenuMacroID="MM_TREE" />
          <RibbonSplitButton UID="S1">
            <RibbonCommandButton UID="B2" MenuMacroID="MM_KERB" />
            <RibbonCommandButton UID="B3" MenuMacroID="MM_LAMP" />
          </RibbonSplitButton>
        </RibbonRow>
      </RibbonPanelSource>
      <RibbonPanelSource UID="RP_LOOSE" Text="Loose"><RibbonRow><RibbonCommandButton MenuMacroID="MM_LAMP"/></RibbonRow></RibbonPanelSource>
    </RibbonPanelSourceCollection>
    <RibbonTabSourceCollection>
      <RibbonTabSource UID="RT_1" Text="Landscape">
        <RibbonPanelSourceReference UID="REF1" PanelId="RP_SITE" />
      </RibbonTabSource>
    </RibbonTabSourceCollection>
  </RibbonRoot>
</CustSection>"#;

    #[test]
    fn images_from_a_resource_dll() {
        let xml = r#"<CustSection><MenuGroup Name="G"><MacroGroup Name="G">
  <MenuMacro UID="M1"><Macro type="Any"><Name>Tree</Name><Command>^C^C_CIRCLE</Command>
    <SmallImage Name="RCDATA_16_TREE"/></Macro></MenuMacro>
  <MenuMacro UID="M2"><Macro type="Any"><Name>Kerb</Name><Command>^C^C_PLINE</Command>
    <SmallImage Name="RCDATA_16_MISSING"/></Macro></MenuMacro>
</MacroGroup></MenuGroup></CustSection>"#;
        let png = oxidraft_plot::png::png(&oxidraft_plot::Picture { width: 1, height: 1, rgba: vec![9; 4] }, false);
        let dll = crate::pe_resources::tests::dll(&[(10, "RCDATA_16_TREE", &png), (10, "RCDATA_16_OTHER", &png)]);
        let c = read_customization(xml.as_bytes(), "x", crate::pe_resources::resource_images(&dll)).unwrap();
        let images: Vec<&str> = c.commands.iter().map(|c| c.image.as_str()).collect();
        assert_eq!(images, ["RCDATA_16_TREE.png", ""]);
        assert_eq!(c.images, [("RCDATA_16_TREE.png".to_string(), png)], "only the images used");
    }

    #[test]
    fn workspaces_read() {
        let xml = r#"<CustSection><MenuGroup Name="G"><MenuRoot>
  <ToolbarRoot><Toolbar UID="TB1"><Name>One</Name></Toolbar><Toolbar UID="TB2"><Name>Two</Name></Toolbar></ToolbarRoot>
  <WorkspaceRoot><Workspace UID="W1" MenuBar="off"><Name>Plain</Name>
    <WorkspaceToolbar ToolbarId="TB1" Display="0"/><WorkspaceToolbar ToolbarId="TB2" Display="1"/>
    <WorkspaceToolbar ToolbarId="ELSEWHERE"/>
    <WorkspaceRibbonRoot><WorkspaceRibbonTabSourceReference TabId="NONE"/></WorkspaceRibbonRoot>
  </Workspace></WorkspaceRoot>
</MenuRoot></MenuGroup></CustSection>"#;
        let c = read_customization(xml.as_bytes(), "x", Vec::new()).unwrap();
        assert_eq!(
            c.workspaces,
            [CuiWorkspace { name: "Plain".into(), menubar: false, toolbars: vec!["Two".into()], tab: None }]
        );
    }

    #[test]
    fn the_command_a_macro_starts() {
        assert_eq!(macro_command("^C^C_line").as_deref(), Some("LINE"));
        assert_eq!(macro_command("*^C^C._circle;\\2;").as_deref(), Some("CIRCLE"));
        assert_eq!(macro_command("^C^C(princ)"), None);
    }

    #[test]
    fn macros_split_at_enters_and_pauses() {
        assert_eq!(macro_tokens("_LINE;0,0;10,0;;", false), ["_LINE", "0,0", "10,0", ""]);
        assert_eq!(macro_tokens("_LINE", false), ["_LINE"]);
        assert_eq!(macro_tokens("_CIRCLE;", true), ["_CIRCLE"]);
        assert_eq!(macro_tokens("2;", false), ["2"]);
        assert!(macro_tokens("", true).is_empty() && macro_tokens("", false).is_empty());
    }

    #[test]
    fn xml_elements_attributes_and_text() {
        let root = parse_xml("<?xml?><a x='1' y=\"&lt;2&gt;\"><b>t &amp; u</b><c/><![CDATA[<raw>]]></a>");
        let a = &root.children[0];
        assert_eq!((a.name.as_str(), a.attr("x"), a.attr("Y")), ("a", Some("1"), Some("<2>")));
        assert_eq!(a.children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["b", "c"]);
        assert_eq!(a.children[0].text, "t & u");
        assert_eq!(a.text, "<raw>");
        assert_eq!(unescape("&#65;&#x42;&bogus"), "AB&bogus");
    }

    #[test]
    fn a_customization_file_read() {
        // As a .cuix: a zip of its parts and images.
        let png = oxidraft_plot::png::png(&oxidraft_plot::Picture { width: 1, height: 1, rgba: vec![0; 4] }, false);
        let zip = oxidraft_plot::zip::zip(&[
            ("[Content_Types].xml".into(), b"<Types/>".to_vec()),
            ("MenuGroup.cui".into(), MENU.as_bytes().to_vec()),
            ("Images/tree_16.png".into(), png.clone()),
            ("Images/unused.png".into(), png),
        ]);
        let c = read_customization(&zip, "fallback", Vec::new()).unwrap();
        assert_eq!(c.group, "SITE");
        let names: Vec<&str> = c.commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Tree & Shrub", "Kerb", "Lamp"]);
        assert_eq!(c.commands[0].macro_text, "^C^C_CIRCLE;\\2;");
        assert_eq!((c.commands[0].id.as_str(), c.commands[0].image.as_str()), ("SITE:MM_TREE", "tree_16.png"));
        assert_eq!(c.images.len(), 1, "only the images used");
        let site = &c.panels[0];
        assert_eq!((site.tab.as_str(), site.name.as_str()), ("Landscape", "Site Works"));
        assert_eq!(site.commands, ["SITE:MM_TREE", "SITE:MM_KERB|SITE:MM_LAMP"]);
        assert_eq!((c.panels[1].tab.as_str(), c.panels[1].name.as_str()), ("SITE", "Loose"), "on no tab");
        assert_eq!(c.shortcuts, [("CTRL+SHIFT+K".to_string(), "SITE:MM_KERB".to_string())]);
        assert!(crate::customize::parse_shortcut(&c.shortcuts[0].0).is_some());
        // The menu bar's menu (not the shortcut menu), and the toolbar.
        assert_eq!(c.menus.len(), 1);
        let site = &c.menus[0];
        assert_eq!(site.name, "Site");
        let labels: Vec<(&str, &str)> = site.items.iter().map(|i| (i.label.as_str(), i.command.as_str())).collect();
        assert_eq!(labels, [("Tree", "SITE:MM_TREE"), ("", ""), ("More", "")]);
        assert_eq!(site.items[2].items[0].command, "SITE:MM_LAMP");
        assert_eq!(c.toolbars[0].name, "Draw");
        assert_eq!(c.toolbars[0].commands, ["SITE:MM_TREE", "", "SITE:MM_KERB"]);
        // As a .cui: the XML alone.
        assert_eq!(read_customization(MENU.as_bytes(), "x", Vec::new()).unwrap().commands.len(), 3);
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain(MENU.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(read_customization(&utf16, "x", Vec::new()).unwrap().group, "SITE");
        assert!(read_customization(b"<html/>", "x", Vec::new()).is_err());
    }
}

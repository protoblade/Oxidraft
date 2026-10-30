//! AutoCAD's sheet set files (.dst) in the Sheet Set Manager: opened as
//! sheet sets and saved as AutoCAD reads them (its sheet set XML in its
//! cipher, which acadrust reads and writes). What AutoCAD keeps that
//! Oxidraft doesn't show (resources, publish options, view categories,
//! callout blocks, empty subsets, a sheet's views and description) is
//! carried through a save.

use std::path::Path;

use oxidraft_io_dwg::sheet_set::{
    self as dst, CUSTOM_SHEET_PROP, CUSTOM_SHEET_SET_PROP, ComponentKind, Element, LayoutReference, SheetSetDatabase,
};

use crate::sheet_set::{SheetRef, SheetSet, drawing_path, relative};

/// Whether `path` names a .dst.
pub fn is_dst(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dst"))
}

/// A sheet's drawing and layout, as matched between saves.
fn sheet_key(file: &str, layout: &str) -> String {
    format!("{}|{}", dst::path_key(file), layout.to_lowercase())
}

/// The .dst at `path` as a sheet set.
pub fn read_dst(path: &Path) -> Result<SheetSet, String> {
    let data = std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let mut db =
        SheetSetDatabase::from_bytes(&data).map_err(|e| format!("{} isn't a sheet set: {e}", path.display()))?;
    db.path = Some(path.to_string_lossy().into_owned());
    let root = db.sheet_set();
    let mut set = SheetSet {
        name: db.name().to_string(),
        description: root.prop("Desc").unwrap_or("").to_string(),
        properties: dst::custom_properties(root)
            .into_iter()
            .filter(|(_, _, flags)| flags & CUSTOM_SHEET_PROP == 0)
            .map(|(n, v, _)| (n, v))
            .collect(),
        ..Default::default()
    };
    fn walk(db: &SheetSetDatabase, el: &Element, subset: &str, path: &Path, set: &mut SheetSet) {
        for c in &el.children {
            match ComponentKind::of(c) {
                Some(ComponentKind::Subset) => {
                    let name = c.prop("Name").unwrap_or("");
                    let inner = if subset.is_empty() { name.to_string() } else { format!("{subset}/{name}") };
                    walk(db, c, &inner, path, set);
                }
                Some(ComponentKind::Sheet) => {
                    let layout = db.layout_reference(c.id(), "Layout").unwrap_or_default();
                    let drawing = relative(path, Path::new(&layout.file_name));
                    for v in db.sheet_views(c) {
                        let view = v.named("NamedView").and_then(|r| r.prop("Name"));
                        if let (Some(view), Some(number)) = (view, v.prop("Number")) {
                            set.view_numbers.push(((drawing.clone(), view.to_string()), number.to_string()));
                        }
                    }
                    set.sheets.push(SheetRef {
                        number: c.prop("Number").unwrap_or("").to_string(),
                        title: c.prop("Title").unwrap_or("").to_string(),
                        drawing,
                        layout: layout.name,
                        properties: dst::custom_properties(c).into_iter().map(|(n, v, _)| (n, v)).collect(),
                        subset: subset.to_string(),
                    });
                }
                _ => {}
            }
        }
    }
    walk(&db, root, "", path, &mut set);
    set.dst = Some(Box::new(db));
    Ok(set)
}

/// The sheets in `el` and its subsets, taken out: each with its key.
fn take_sheets(db: &SheetSetDatabase, el: &mut Element, out: &mut Vec<(String, Element)>) {
    let mut kept = Vec::with_capacity(el.children.len());
    for mut c in std::mem::take(&mut el.children) {
        match ComponentKind::of(&c) {
            Some(ComponentKind::Sheet) => {
                let key = c
                    .named("Layout")
                    .map_or(String::new(), |r| sheet_key(&db.resolve_file(r), r.prop("Name").unwrap_or("")));
                out.push((key, c));
            }
            Some(ComponentKind::Subset) => {
                take_sheets(db, &mut c, out);
                kept.push(c);
            }
            _ => kept.push(c),
        }
    }
    el.children = kept;
}

/// Saves `set` at `path` as a .dst, on what it was read from (if it was):
/// the database written, to save on next time.
pub fn write_dst(set: &SheetSet, path: &Path) -> Result<SheetSetDatabase, String> {
    let mut db = match &set.dst {
        Some(db) => (**db).clone(),
        None => SheetSetDatabase::new(&set.name, &set.description),
    };
    db.path = Some(path.to_string_lossy().into_owned());
    let lost = || "The sheet set file is damaged.".to_string();
    // The set: its name, description and custom properties.
    let root = db.sheet_set_mut();
    root.set_prop("Name", &set.name);
    root.set_prop("Desc", &set.description);
    for (name, _, flags) in dst::custom_properties(root) {
        if flags & CUSTOM_SHEET_PROP == 0 && !set.properties.iter().any(|(n, _)| *n == name) {
            dst::remove_custom_property(root, &name);
        }
    }
    for (name, value) in &set.properties {
        dst::set_custom_property(root, name, value, CUSTOM_SHEET_SET_PROP);
    }
    // The sheets taken out, to go back in the set's order (keeping what
    // AutoCAD keeps with each), in their subsets.
    let mut old = Vec::new();
    let mut root = db.sheet_set().clone();
    take_sheets(&db, &mut root, &mut old);
    *db.sheet_set_mut() = root;
    let set_id = db.sheet_set().id().to_string();
    for s in &set.sheets {
        let mut parent = set_id.clone();
        for name in s.subset.split('/').filter(|n| !n.is_empty()) {
            let found = db.find(&parent).and_then(|p| {
                p.children
                    .iter()
                    .find(|c| ComponentKind::of(c) == Some(ComponentKind::Subset) && c.prop("Name") == Some(name))
                    .map(|c| c.id().to_string())
            });
            parent = match found {
                Some(id) => id,
                None => db.add_subset(&parent, name, "").ok_or_else(lost)?,
            };
        }
        let file = drawing_path(Some(path), s).to_string_lossy().into_owned();
        let key = sheet_key(&file, &s.layout);
        let id = match old.iter().position(|(k, _)| *k == key) {
            Some(i) => {
                let sheet = old.remove(i).1;
                let id = sheet.id().to_string();
                db.find_mut(&parent).ok_or_else(lost)?.children.push(sheet);
                id
            }
            None => db.add_sheet(&parent, &s.number, &s.title, "").ok_or_else(lost)?,
        };
        let handle = db
            .layout_reference(&id, "Layout")
            .filter(|r| r.name.eq_ignore_ascii_case(&s.layout))
            .map(|r| r.handle)
            .unwrap_or_default();
        db.set_layout_reference(&id, "Layout", &LayoutReference { file_name: file, name: s.layout.clone(), handle });
        let el = db.find_mut(&id).ok_or_else(lost)?;
        el.set_prop("Number", &s.number);
        el.set_prop("Title", &s.title);
        for (name, _, _) in dst::custom_properties(el) {
            if !s.properties.iter().any(|(n, _)| *n == name) {
                dst::remove_custom_property(el, &name);
            }
        }
        for (name, value) in &s.properties {
            dst::set_custom_property(el, name, value, CUSTOM_SHEET_PROP);
        }
        // Its views' numbers.
        if let Some(views) = el.named_mut("SheetViews") {
            for v in views.children.iter_mut().filter(|v| v.name == "AcSmSheetView") {
                let name = v.named("NamedView").and_then(|r| r.prop("Name")).unwrap_or("").to_string();
                let number =
                    set.view_numbers.iter().find(|((d, n), _)| *d == s.drawing && n.eq_ignore_ascii_case(&name));
                if let Some((_, number)) = number {
                    v.set_prop("Number", number);
                }
            }
        }
    }
    db.write(&path.to_string_lossy()).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_set_saved_as_dst_and_read_back() {
        let dir = std::env::temp_dir().join(format!("oxidraft-dst-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("plans")).unwrap();
        let path = dir.join("Project.dst");
        let sheet = |number: &str, layout: &str, subset: &str| SheetRef {
            number: number.into(),
            title: format!("{layout} title"),
            drawing: "plans/site.dwg".into(),
            layout: layout.into(),
            properties: vec![("Drawn".into(), "AB".into())],
            subset: subset.into(),
        };
        let mut set = SheetSet {
            name: "Project".into(),
            description: "A test".into(),
            sheets: vec![sheet("01", "Plan", ""), sheet("02", "Details", "Construction/Ground")],
            properties: vec![("Client".into(), "Acme".into())],
            ..Default::default()
        };
        let db = write_dst(&set, &path).unwrap();
        // Ciphered: not XML as it lies on disk.
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.starts_with(b"<"));
        assert!(db.to_xml().contains("AcSmSheetSet"));
        let back = read_dst(&path).unwrap();
        assert_eq!((back.name.as_str(), back.description.as_str()), ("Project", "A test"));
        assert_eq!(back.properties, set.properties);
        assert_eq!(back.sheets, set.sheets);
        // Reordered and one retitled: what AutoCAD kept with a sheet stays.
        set = back;
        let id = |set: &SheetSet| {
            let db = set.dst.as_ref().unwrap();
            db.sheets().iter().map(|s| s.id().to_string()).collect::<Vec<_>>()
        };
        let ids = id(&set);
        set.sheets.swap(0, 1);
        set.sheets[0].title = "New".into();
        set.sheets[0].subset.clear();
        let db = write_dst(&set, &path).unwrap();
        let again = read_dst(&path).unwrap();
        assert_eq!(again.sheets, set.sheets, "in the new order, the one retitled out of its subset");
        assert_eq!(id(&again).len(), 2);
        assert!(ids.iter().all(|i| db.find(i).is_some()), "the same sheets, moved");
        assert!(
            db.sheets().iter().all(|s| s.named("Layout").and_then(|r| r.prop("Relative_FileName")).is_some()),
            "drawings named relative to the set"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

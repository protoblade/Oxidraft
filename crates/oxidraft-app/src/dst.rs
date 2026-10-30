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
    set.selections = selections(&db, path);
    set.dst = Some(Box::new(db));
    Ok(set)
}

/// New IDs throughout an element copied from another.
fn renew_ids(el: &mut Element) {
    if let Some(a) = el.attrs.iter_mut().find(|(k, _)| k == "ID") {
        a.1 = dst::new_id();
    }
    for c in &mut el.children {
        renew_ids(c);
    }
}

/// Whether `el` names sheet `id` anywhere in it.
fn names(el: &Element, id: &str) -> bool {
    el.text.trim().eq_ignore_ascii_case(id)
        || el.attrs.iter().any(|(k, v)| k != "ID" && v.eq_ignore_ascii_case(id))
        || el.children.iter().any(|c| names(c, id))
}

/// `el` with sheet `from` named as `to` instead.
fn rename(el: &mut Element, from: &str, to: &str) {
    if el.text.trim().eq_ignore_ascii_case(from) {
        el.text = to.to_string();
    }
    for (k, v) in &mut el.attrs {
        if k != "ID" && v.eq_ignore_ascii_case(from) {
            *v = to.to_string();
        }
    }
    for c in &mut el.children {
        rename(c, from, to);
    }
}

/// The set's sheet selections into the .dst: AutoCAD's as they are, one
/// made or changed in Oxidraft in the form an AutoCAD selection in the set
/// takes (its class, and the element in it that names a sheet), when the
/// set has one to follow; one removed in Oxidraft gone.
fn write_selections(db: &mut SheetSetDatabase, set: &SheetSet, path: &Path) {
    let ids: Vec<(String, (String, String))> = db
        .sheets()
        .iter()
        .filter_map(|s| {
            let layout = db.layout_reference(s.id(), "Layout")?;
            Some((s.id().to_string(), (relative(path, Path::new(&layout.file_name)), layout.name.to_lowercase())))
        })
        .collect();
    let id_of = |(drawing, layout): &(String, String)| {
        ids.iter().find(|(_, k)| k.0 == *drawing && k.1 == layout.to_lowercase()).map(|(id, _)| id.clone())
    };
    let current = selections(db, path);
    let Some(sets) = db.sheet_set_mut().named_mut("SheetSelSets") else { return };
    // A selection to follow, and its child that names a sheet.
    let template = sets.children.iter().find_map(|s| {
        s.prop("Name")?;
        let reference = s.children.iter().find(|c| ids.iter().any(|(id, _)| names(c, id)))?;
        let id = ids.iter().find(|(id, _)| names(reference, id))?.0.clone();
        Some((s.clone(), reference.clone(), id))
    });
    sets.children.retain(|c| c.prop("Name").is_none_or(|n| set.selections.iter().any(|s| s.name == n)));
    let Some((selection, reference, named)) = template else { return };
    for s in &set.selections {
        if current.iter().any(|c| c.name == s.name && c.sheets == s.sheets) {
            continue;
        }
        let mut el = selection.clone();
        el.children.retain(|c| !ids.iter().any(|(id, _)| names(c, id)));
        renew_ids(&mut el);
        el.set_prop("Name", &s.name);
        for sheet in s.sheets.iter().filter_map(id_of) {
            let mut r = reference.clone();
            renew_ids(&mut r);
            rename(&mut r, &named, &sheet);
            el.children.push(r);
        }
        match sets.children.iter_mut().find(|c| c.prop("Name") == Some(s.name.as_str())) {
            Some(old) => *old = el,
            None => sets.children.push(el),
        }
    }
}

/// AutoCAD's sheet selections: each named one in the set's SheetSelSets,
/// with the sheets it names (by their IDs, wherever in it they're named).
fn selections(db: &SheetSetDatabase, path: &Path) -> Vec<crate::sheet_set::SheetSelection> {
    let sheets: Vec<(String, (String, String))> = db
        .sheets()
        .iter()
        .filter_map(|s| {
            let layout = db.layout_reference(s.id(), "Layout")?;
            Some((s.id().to_string(), (relative(path, Path::new(&layout.file_name)), layout.name)))
        })
        .collect();
    fn named(el: &Element, sheets: &[(String, (String, String))], out: &mut Vec<(String, String)>) {
        let mut values: Vec<&str> = el.attrs.iter().filter(|(k, _)| k != "ID").map(|(_, v)| v.as_str()).collect();
        values.push(el.text.as_str());
        for v in values {
            if let Some((_, sheet)) = sheets.iter().find(|(id, _)| id.eq_ignore_ascii_case(v.trim())) {
                out.push(sheet.clone());
            }
        }
        for c in &el.children {
            named(c, sheets, out);
        }
    }
    let Some(sets) = db.sheet_set().named("SheetSelSets") else { return Vec::new() };
    sets.children
        .iter()
        .filter_map(|s| {
            let name = s.prop("Name")?.to_string();
            let mut picked = Vec::new();
            named(s, &sheets, &mut picked);
            picked.dedup();
            Some(crate::sheet_set::SheetSelection { name, sheets: picked })
        })
        .collect()
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
    write_selections(&mut db, set, path);
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
        let mut db = write_dst(&set, &path).unwrap();
        // A selection AutoCAD keeps: its name and the sheets it refers to.
        let first = db.sheets()[0].id().to_string();
        let sets = db.sheet_set_mut().named_mut("SheetSelSets").unwrap();
        let mut selection = dst::object("AcSmSheetSelSet", "g00000000-0000-0000-0000-000000000000", None);
        selection.set_prop("Name", "Issued");
        let mut reference = dst::object("AcSmObjectReference", dst::CLSID_OBJECT_REFERENCE, None);
        reference.set_prop_vt("ReferencedObject", -1, &first);
        selection.children.push(reference);
        sets.children.push(selection);
        db.write(&path.to_string_lossy()).unwrap();
        let again = read_dst(&path).unwrap();
        assert_eq!(again.selections.len(), 1);
        assert_eq!(again.selections[0].name, "Issued");
        assert_eq!(again.selections[0].sheets, [(again.sheets[0].drawing.clone(), again.sheets[0].layout.clone())]);
        assert_eq!(again.sheets, set.sheets, "in the new order, the one retitled out of its subset");
        assert_eq!(id(&again).len(), 2);
        assert!(ids.iter().all(|i| db.find(i).is_some()), "the same sheets, moved");
        assert!(
            db.sheets().iter().all(|s| s.named("Layout").and_then(|r| r.prop("Relative_FileName")).is_some()),
            "drawings named relative to the set"
        );
        // One saved in Oxidraft goes in as AutoCAD's does; AutoCAD's stays.
        let mut set = again;
        set.save_selection("Both", &[0, 1]);
        let db2 = write_dst(&set, &path).unwrap();
        let back = read_dst(&path).unwrap();
        let names: Vec<&str> = back.selections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Issued", "Both"]);
        assert_eq!(back.selections[1].sheets.len(), 2);
        let sets = db2.sheet_set().named("SheetSelSets").unwrap();
        assert!(sets.children.iter().all(|c| c.name == "AcSmSheetSelSet"), "in AutoCAD's form");
        std::fs::remove_dir_all(&dir).ok();
    }
}

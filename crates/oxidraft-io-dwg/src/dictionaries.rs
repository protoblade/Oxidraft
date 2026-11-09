//! The named objects every AutoCAD drawing has, added where the drawing
//! going out lacks them: ACAD_GROUP, ACAD_MLINESTYLE with its Standard
//! style, ACAD_PLOTSETTINGS, ACAD_MATERIAL, ACAD_COLOR, ACAD_VISUALSTYLE,
//! and ACAD_PLOTSTYLENAME with its Normal plot style, which each layer
//! points at (group 390, which AutoCAD requires of a layer).

use acadrust::CadDocument;
use acadrust::objects::{Dictionary, DictionaryWithDefault, MLineStyle, ObjectType, PlaceHolder};
use acadrust::types::Handle;

pub(crate) fn finish(cad: &mut CadDocument) {
    let root = cad.header.named_objects_dict_handle;
    if !matches!(cad.objects.get(&root), Some(ObjectType::Dictionary(_))) {
        return;
    }
    cad.header.acad_group_dict_handle = dictionary(cad, root, "ACAD_GROUP");
    let styles = dictionary(cad, root, "ACAD_MLINESTYLE");
    cad.header.acad_mlinestyle_dict_handle = styles;
    let standard = match entry(cad, styles, "Standard") {
        Some(h) => h,
        None => {
            let h = cad.allocate_handle();
            let mut s = MLineStyle::standard();
            s.handle = h;
            s.owner = styles;
            cad.objects.insert(h, ObjectType::MLineStyle(s));
            if let Some(ObjectType::Dictionary(d)) = cad.objects.get_mut(&styles) {
                d.add_entry("Standard", h);
            }
            h
        }
    };
    if cad.header.current_multiline_style_handle.is_null() {
        cad.header.current_multiline_style_handle = standard;
    }
    cad.header.acad_plotsettings_dict_handle = dictionary(cad, root, "ACAD_PLOTSETTINGS");
    cad.header.acad_material_dict_handle = dictionary(cad, root, "ACAD_MATERIAL");
    cad.header.acad_color_dict_handle = dictionary(cad, root, "ACAD_COLOR");
    cad.header.acad_visualstyle_dict_handle = dictionary(cad, root, "ACAD_VISUALSTYLE");
    if entry(cad, root, "ACAD_PLOTSTYLENAME").is_none() {
        let (dict, normal) = (cad.allocate_handle(), cad.allocate_handle());
        let mut d = DictionaryWithDefault::new();
        d.handle = dict;
        d.owner = root;
        d.default_handle = normal;
        d.entries.push(("Normal".into(), normal));
        cad.objects.insert(dict, ObjectType::DictionaryWithDefault(d));
        let mut p = PlaceHolder::new();
        p.handle = normal;
        p.owner = dict;
        cad.objects.insert(normal, ObjectType::PlaceHolder(p));
        if let Some(ObjectType::Dictionary(r)) = cad.objects.get_mut(&root) {
            r.add_entry("ACAD_PLOTSTYLENAME", dict);
        }
        cad.header.acad_plotstylename_dict_handle = dict;
    }
}

/// What `name` names in the dictionary `dict`, if it's there.
fn entry(cad: &CadDocument, dict: Handle, name: &str) -> Option<Handle> {
    match cad.objects.get(&dict) {
        Some(ObjectType::Dictionary(d)) => d.get(name).filter(|h| cad.objects.contains_key(h)),
        _ => None,
    }
}

/// The dictionary `name` in the root dictionary, made empty if there's none.
fn dictionary(cad: &mut CadDocument, root: Handle, name: &str) -> Handle {
    if let Some(h) = entry(cad, root, name).filter(|h| matches!(cad.objects.get(h), Some(ObjectType::Dictionary(_)))) {
        return h;
    }
    let h = cad.allocate_handle();
    let mut d = Dictionary::new();
    d.handle = h;
    d.owner = root;
    cad.objects.insert(h, ObjectType::Dictionary(d));
    if let Some(ObjectType::Dictionary(r)) = cad.objects.get_mut(&root) {
        r.add_entry(name, h);
    }
    h
}

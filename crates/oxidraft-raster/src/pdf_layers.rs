//! A PDF's layers (optional content groups), for ULAYERS: their names
//! and whether they show, and the PDF with some turned off — an
//! incremental update giving it a default configuration that hides them,
//! which the renderer and PDFIMPORT then follow.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::dict::keys::{BASE_STATE, D, NAME, OCGS, OCPROPERTIES, OFF, ON};
use hayro::hayro_syntax::object::{Array, Dict, Name, ObjRef, ObjectIdentifier};

/// A PDF text string: UTF-16 with its byte-order mark, else one byte a
/// character.
fn text(bytes: &[u8]) -> String {
    match bytes.strip_prefix(&[0xFE, 0xFF]) {
        Some(rest) => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
            String::from_utf16_lossy(&units)
        }
        None => bytes.iter().map(|b| char::from(*b)).collect(),
    }
}

/// Each layer's reference and name.
fn groups(pdf: &Pdf) -> Vec<(ObjRef, String)> {
    let xref = pdf.xref();
    let Some(catalog) = xref.get::<Dict<'_>>(xref.root_id()) else { return Vec::new() };
    let Some(ocp) = catalog.get::<Dict<'_>>(OCPROPERTIES) else { return Vec::new() };
    let Some(ocgs) = ocp.get::<Array<'_>>(OCGS) else { return Vec::new() };
    ocgs.raw_iter()
        .filter_map(|item| {
            let r = item.as_obj_ref()?;
            let group = xref.get::<Dict<'_>>(r.into())?;
            let name = group.get::<hayro::hayro_syntax::object::String<'_>>(NAME).map(|s| text(s.as_bytes()))?;
            Some((r, name))
        })
        .collect()
}

fn refs(arr: Option<Array<'_>>) -> Vec<ObjRef> {
    arr.map(|a| a.raw_iter().filter_map(|i| i.as_obj_ref()).collect()).unwrap_or_default()
}

/// The PDF's layers, and whether each shows as the file has it.
pub fn pdf_layers(data: &[u8]) -> Vec<(String, bool)> {
    let Ok(pdf) = Pdf::new(data.to_vec()) else { return Vec::new() };
    let xref = pdf.xref();
    let config = xref
        .get::<Dict<'_>>(xref.root_id())
        .and_then(|c| c.get::<Dict<'_>>(OCPROPERTIES))
        .and_then(|o| o.get::<Dict<'_>>(D));
    let base_off = config.as_ref().and_then(|d| d.get::<Name<'_>>(BASE_STATE)).is_some_and(|n| n.as_ref() == b"OFF");
    let on = refs(config.as_ref().and_then(|d| d.get::<Array<'_>>(ON)));
    let off = refs(config.as_ref().and_then(|d| d.get::<Array<'_>>(OFF)));
    groups(&pdf)
        .into_iter()
        .map(|(r, name)| {
            let shows = if base_off { on.contains(&r) } else { !off.contains(&r) };
            (name, shows)
        })
        .collect()
}

/// Where `part` (a slice of `whole`'s bytes) starts in it.
fn offset_in(whole: &[u8], part: &[u8]) -> Option<usize> {
    let start = (part.as_ptr() as usize).checked_sub(whole.as_ptr() as usize)?;
    (start + part.len() <= whole.len()).then_some(start)
}

/// The last number after `key` in the file (the trailer's).
fn last_number_after(data: &[u8], key: &[u8]) -> Option<i64> {
    let at = data.windows(key.len()).rposition(|w| w == key)? + key.len();
    let digits: String = data[at..]
        .iter()
        .map(|b| char::from(*b))
        .skip_while(|c| c.is_whitespace())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The PDF with the layers named in `off` hidden (and the others shown),
/// or `None` when it has no layers to change.
pub fn with_layers_off(data: &[u8], off: &[String]) -> Option<Vec<u8>> {
    let pdf = Pdf::new(data.to_vec()).ok()?;
    let xref = pdf.xref();
    let root = xref.root_id();
    let catalog = xref.get::<Dict<'_>>(root)?;
    let ocp = catalog.get::<Dict<'_>>(OCPROPERTIES)?;
    let hidden: Vec<String> = groups(&pdf)
        .into_iter()
        .filter(|(_, name)| off.iter().any(|o| o == name))
        .map(|(r, _)| format!("{} {} R", r.obj_number, r.gen_number))
        .collect();
    let config = format!("<< /BaseState /ON /OFF [{}] >>", hidden.join(" "));
    // The object to replace: the configuration itself when it's an object
    // of its own, else what holds it inline.
    let (id, body): (ObjectIdentifier, Vec<u8>) = match ocp.get_ref(D) {
        Some(r) => (r.into(), config.into_bytes()),
        None => {
            let holder: (ObjectIdentifier, Dict<'_>) = match catalog.get_ref(OCPROPERTIES) {
                Some(r) => (r.into(), ocp.clone()),
                None => (root, catalog.clone()),
            };
            let raw = holder.1.data();
            let replaced = match ocp.get::<Dict<'_>>(D) {
                Some(d) => {
                    let at = offset_in(raw, d.data())?;
                    [&raw[..at], config.as_bytes(), &raw[at + d.data().len()..]].concat()
                }
                // No configuration yet: one after the OCProperties' opening.
                None => {
                    let at = offset_in(raw, ocp.data())? + 2;
                    [&raw[..at], format!(" /D {config} ").as_bytes(), &raw[at..]].concat()
                }
            };
            (holder.0, replaced)
        }
    };
    let prev = last_number_after(data, b"startxref")?;
    let size = last_number_after(data, b"/Size")?;
    let mut out = data.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let at = out.len();
    out.extend_from_slice(format!("{} {} obj\n", id.obj_number, id.gen_number).as_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(b"\nendobj\n");
    let xref_at = out.len();
    out.extend_from_slice(
        format!(
            "xref\n0 1\n0000000000 65535 f \n{} 1\n{at:010} {:05} n \ntrailer\n<< /Size {size} /Root {} {} R /Prev {prev} >>\nstartxref\n{xref_at}\n%%EOF\n",
            id.obj_number, id.gen_number, root.obj_number, root.gen_number
        )
        .as_bytes(),
    );
    Some(out)
}

/// The layers turned off in each PDF file, which rendering and importing
/// follow (ULAYERS).
fn layers_off() -> &'static Mutex<HashMap<PathBuf, Vec<String>>> {
    static OFF: OnceLock<Mutex<HashMap<PathBuf, Vec<String>>>> = OnceLock::new();
    OFF.get_or_init(Default::default)
}

/// Hides `off` (by name) in the PDF at `path` from now on.
pub fn set_layers_off(path: &Path, off: Vec<String>) {
    if let Ok(mut map) = layers_off().lock() {
        if off.is_empty() {
            map.remove(path);
        } else {
            map.insert(path.to_path_buf(), off);
        }
    }
}

/// The PDF at `path`, read with the layers turned off for it hidden.
pub fn read_pdf(path: &Path) -> std::io::Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    let off = layers_off().lock().ok().and_then(|m| m.get(path).cloned()).unwrap_or_default();
    if off.is_empty() {
        return Ok(data);
    }
    Ok(with_layers_off(&data, &off).unwrap_or(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf_vectors::pdf_vectors;

    /// A page with a line on layer Walls and one on Doors.
    fn layered_pdf() -> Vec<u8> {
        let content = "/OC /L1 BDC 0 0 m 100 0 l S EMC /OC /L2 BDC 0 50 m 100 50 l S EMC";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R] /D << /Order [5 0 R 6 0 R] >> >> >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Properties << /L1 5 0 R /L2 6 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /OCG /Name (Walls) >>".to_string(),
            "<< /Type /OCG /Name (Doors) >>".to_string(),
        ];
        let mut out = b"%PDF-1.5\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes(),
        );
        out
    }

    #[test]
    fn layers_list_and_turn_off() {
        let pdf = layered_pdf();
        assert_eq!(pdf_layers(&pdf), [("Walls".to_string(), true), ("Doors".to_string(), true)]);
        assert_eq!(pdf_vectors(pdf.clone(), 0).unwrap().paths.len(), 2);
        let hidden = with_layers_off(&pdf, &["Doors".to_string()]).unwrap();
        assert_eq!(pdf_layers(&hidden), [("Walls".to_string(), true), ("Doors".to_string(), false)]);
        let left = pdf_vectors(hidden.clone(), 0).unwrap();
        assert_eq!(left.paths.len(), 1);
        assert!(left.paths[0].parts[0].0[0].y.abs() < 1e-6, "the wall's line, at the bottom");
        // Turned on again, from the changed file.
        let back = with_layers_off(&hidden, &[]).unwrap();
        assert_eq!(pdf_vectors(back, 0).unwrap().paths.len(), 2);
    }
}

//! The images a Windows resource DLL keeps, as AutoCAD keeps the button
//! images a customization file names but doesn't hold: in a DLL of the
//! .cuix's name beside it (acad.cuix's in acadbtn.xmx). Its resources
//! that are PNG, BMP or JPEG files, and its bitmaps, by name.

fn u16_at(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

/// A resource directory's entries: each one's name (or number) and where
/// its subdirectory or data is, from the resource section's start.
fn entries(d: &[u8], base: usize, dir: usize) -> Vec<(String, u32)> {
    let count = |i| u16_at(d, base + dir + i).map_or(0, usize::from);
    (0..count(12) + count(14))
        .filter_map(|k| {
            let at = base + dir + 16 + k * 8;
            let (name, to) = (u32_at(d, at)?, u32_at(d, at + 4)?);
            let name = if name & 0x8000_0000 != 0 {
                let s = base + (name & 0x7FFF_FFFF) as usize;
                let len = usize::from(u16_at(d, s)?);
                let units: Vec<u16> = (0..len).filter_map(|j| u16_at(d, s + 2 + j * 2)).collect();
                String::from_utf16_lossy(&units)
            } else {
                format!("#{name}")
            };
            Some((name, to))
        })
        .collect()
}

/// Every image among the resources of the PE file `data`: its name with
/// the extension its contents call for, and the file.
pub fn resource_images(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let found = || -> Option<Vec<(String, Vec<u8>)>> {
        let pe = u32_at(data, 0x3C)? as usize;
        if data.get(..2)? != b"MZ" || data.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let sections = usize::from(u16_at(data, pe + 6)?);
        let optional = pe + 24;
        let directories = match u16_at(data, optional)? {
            0x10B => optional + 96,
            0x20B => optional + 112,
            _ => return None,
        };
        let rva = u32_at(data, directories + 16)?;
        let table = optional + usize::from(u16_at(data, pe + 20)?);
        // Where an address is in the file.
        let offset = |rva: u32| {
            (0..sections).find_map(|k| {
                let s = table + k * 40;
                let (size, va, raw) =
                    (u32_at(data, s + 8)?.max(u32_at(data, s + 16)?), u32_at(data, s + 12)?, u32_at(data, s + 20)?);
                (va..va.checked_add(size)?).contains(&rva).then(|| (raw + (rva - va)) as usize)
            })
        };
        let base = offset(rva)?;
        let mut out = Vec::new();
        // Type, name, language, then the data.
        for (kind, types) in entries(data, base, 0) {
            for (name, names) in entries(data, base, (types & 0x7FFF_FFFF) as usize) {
                let Some((_, entry)) = entries(data, base, (names & 0x7FFF_FFFF) as usize).into_iter().next() else {
                    continue;
                };
                let entry = base + (entry & 0x7FFF_FFFF) as usize;
                let (Some(at), Some(size)) = (u32_at(data, entry).and_then(offset), u32_at(data, entry + 4)) else {
                    continue;
                };
                let Some(bytes) = data.get(at..at + size as usize) else { continue };
                let file = if bytes.starts_with(b"\x89PNG") {
                    Some((bytes.to_vec(), "png"))
                } else if bytes.starts_with(b"BM") {
                    Some((bytes.to_vec(), "bmp"))
                } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
                    Some((bytes.to_vec(), "jpg"))
                } else if kind == "#2" {
                    oxidraft_io_dwg::bmp_from_dib(bytes).map(|b| (b, "bmp"))
                } else {
                    None
                };
                if let Some((file, ext)) = file {
                    out.push((format!("{name}.{ext}"), file));
                }
            }
        }
        Some(out)
    };
    found().unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A resource-only DLL holding `resources`: each one's type (RCDATA
    /// 10, BITMAP 2), name and contents.
    pub(crate) fn dll(resources: &[(u32, &str, &[u8])]) -> Vec<u8> {
        const VA: u32 = 0x1000;
        // The tree: a type directory, a name directory for each resource
        // and a language directory under it, then names, data entries and
        // the data.
        let types: Vec<u32> = {
            let mut t: Vec<u32> = resources.iter().map(|r| r.0).collect();
            t.dedup();
            t
        };
        let dir = |n: usize| 16 + 8 * n;
        let mut sizes = dir(types.len());
        let type_dirs: Vec<usize> = types
            .iter()
            .map(|t| {
                let at = sizes;
                sizes += dir(resources.iter().filter(|r| r.0 == *t).count());
                at
            })
            .collect();
        let lang_dirs: Vec<usize> = resources
            .iter()
            .map(|_| {
                let at = sizes;
                sizes += dir(1);
                at
            })
            .collect();
        let names: Vec<usize> = resources
            .iter()
            .map(|r| {
                let at = sizes;
                sizes += 2 + 2 * r.1.encode_utf16().count();
                at
            })
            .collect();
        sizes = sizes.next_multiple_of(4);
        let data_entries: Vec<usize> = resources
            .iter()
            .map(|_| {
                let at = sizes;
                sizes += 16;
                at
            })
            .collect();
        let mut tree = vec![0u8; sizes];
        let put16 = |t: &mut Vec<u8>, at: usize, v: u16| t[at..at + 2].copy_from_slice(&v.to_le_bytes());
        let put32 = |t: &mut Vec<u8>, at: usize, v: u32| t[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put16(&mut tree, 14, types.len() as u16);
        for (k, t) in types.iter().enumerate() {
            put32(&mut tree, 16 + k * 8, *t);
            put32(&mut tree, 20 + k * 8, 0x8000_0000 | type_dirs[k] as u32);
            let mine: Vec<usize> = (0..resources.len()).filter(|i| resources[*i].0 == *t).collect();
            put16(&mut tree, type_dirs[k] + 12, mine.len() as u16);
            for (j, i) in mine.into_iter().enumerate() {
                put32(&mut tree, type_dirs[k] + 16 + j * 8, 0x8000_0000 | names[i] as u32);
                put32(&mut tree, type_dirs[k] + 20 + j * 8, 0x8000_0000 | lang_dirs[i] as u32);
            }
        }
        for (i, r) in resources.iter().enumerate() {
            put16(&mut tree, lang_dirs[i] + 14, 1);
            put32(&mut tree, lang_dirs[i] + 16, 1033);
            put32(&mut tree, lang_dirs[i] + 20, data_entries[i] as u32);
            let units: Vec<u16> = r.1.encode_utf16().collect();
            put16(&mut tree, names[i], units.len() as u16);
            for (j, u) in units.iter().enumerate() {
                put16(&mut tree, names[i] + 2 + j * 2, *u);
            }
            let at = tree.len();
            tree.extend(r.2);
            put32(&mut tree, data_entries[i], VA + at as u32);
            put32(&mut tree, data_entries[i] + 4, r.2.len() as u32);
        }
        // Headers: DOS, PE, COFF, a PE32 optional header and one section.
        let mut out = vec![0u8; 0x200];
        out[..2].copy_from_slice(b"MZ");
        out[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        out[0x40..0x44].copy_from_slice(b"PE\0\0");
        out[0x44..0x46].copy_from_slice(&0x14Cu16.to_le_bytes());
        out[0x46..0x48].copy_from_slice(&1u16.to_le_bytes());
        out[0x54..0x56].copy_from_slice(&224u16.to_le_bytes());
        let optional = 0x58;
        out[optional..optional + 2].copy_from_slice(&0x10Bu16.to_le_bytes());
        out[optional + 92..optional + 96].copy_from_slice(&16u32.to_le_bytes());
        out[optional + 112..optional + 116].copy_from_slice(&VA.to_le_bytes());
        out[optional + 116..optional + 120].copy_from_slice(&(tree.len() as u32).to_le_bytes());
        let s = optional + 224;
        out[s..s + 8].copy_from_slice(b".rsrc\0\0\0");
        out[s + 8..s + 12].copy_from_slice(&(tree.len() as u32).to_le_bytes());
        out[s + 12..s + 16].copy_from_slice(&VA.to_le_bytes());
        out[s + 16..s + 20].copy_from_slice(&(tree.len() as u32).to_le_bytes());
        out[s + 20..s + 24].copy_from_slice(&0x200u32.to_le_bytes());
        out.extend(tree);
        out
    }

    #[test]
    fn a_dlls_images_by_name() {
        let png = oxidraft_plot::png::png(&oxidraft_plot::Picture { width: 1, height: 1, rgba: vec![9; 4] }, false);
        // A 1x1 24-bit bitmap: its header and one padded pixel.
        let mut dib = 40u32.to_le_bytes().to_vec();
        dib.extend(1i32.to_le_bytes());
        dib.extend(1i32.to_le_bytes());
        dib.extend(1u16.to_le_bytes());
        dib.extend(24u16.to_le_bytes());
        dib.extend([0u8; 24]);
        dib.extend([0, 0, 255, 0]);
        let data = dll(&[(10, "RCDATA_16_TREE", &png), (10, "NOTES", b"not a picture"), (2, "KERB_BMP", &dib)]);
        let images = resource_images(&data);
        let names: Vec<&str> = images.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["RCDATA_16_TREE.png", "KERB_BMP.bmp"]);
        assert_eq!(images[0].1, png);
        assert!(image::load_from_memory(&images[1].1).is_ok(), "the bitmap a .bmp file");
        assert!(resource_images(b"MZ not a dll").is_empty());
    }
}

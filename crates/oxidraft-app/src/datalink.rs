//! DATALINK and DATALINKUPDATE: a table made from a spreadsheet — a CSV
//! file or an Excel workbook's first sheet — placed like TABLE, its title
//! the file's name, and kept linked (its hyperlink, described "Data link",
//! names the file, so Cmd/Ctrl-click opens it); DATALINKUPDATE reads the
//! linked files again.

use std::io::Read as _;
use std::path::Path;

use oxidraft_geom::Point;
use oxidraft_model::{EntityId, Geometry, Hyperlink};

use crate::app::OxidraftApp;

/// How a table's link to its spreadsheet is described.
pub const DATA_LINK: &str = "Data link";

/// CSV text as rows of cells (quoted cells may hold commas, quotes and
/// line breaks).
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let (mut row, mut cell, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',' | ';', false) => row.push(std::mem::take(&mut cell)),
            ('\n', false) => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            ('\r', false) => {}
            (c, _) => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// The files in a zip archive: name and contents.
pub(crate) fn unzip(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16_at = |i: usize| data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |i: usize| data.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    // The central directory has the sizes, whatever the local headers say.
    let Some(end) = (0..data.len().saturating_sub(21)).rev().find(|&i| u32_at(i) == Some(0x0605_4b50)) else {
        return Vec::new();
    };
    let (count, mut at) = (u16_at(end + 10).unwrap_or(0), u32_at(end + 16).unwrap_or(0));
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(at) != Some(0x0201_4b50) {
            break;
        }
        let (method, size) = (u16_at(at + 10).unwrap_or(0), u32_at(at + 20).unwrap_or(0));
        let (name_len, extra, comment) =
            (u16_at(at + 28).unwrap_or(0), u16_at(at + 30).unwrap_or(0), u16_at(at + 32).unwrap_or(0));
        let local = u32_at(at + 42).unwrap_or(0);
        let name = String::from_utf8_lossy(data.get(at + 46..at + 46 + name_len).unwrap_or(&[])).into_owned();
        at += 46 + name_len + extra + comment;
        let (Some(lname), Some(lextra)) = (u16_at(local + 26), u16_at(local + 28)) else { continue };
        let start = local + 30 + lname + lextra;
        let Some(body) = data.get(start..start + size) else { continue };
        let content = match method {
            0 => body.to_vec(),
            8 => {
                let mut v = Vec::new();
                if flate2::read::DeflateDecoder::new(body).read_to_end(&mut v).is_err() {
                    continue;
                }
                v
            }
            _ => continue,
        };
        out.push((name, content));
    }
    out
}

/// XML text with its entities read.
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The text of each `<t>` in `xml` within each `<si>` (shared strings).
fn shared_strings(xml: &str) -> Vec<String> {
    xml.split("<si>")
        .skip(1)
        .map(|si| {
            si.split("<t")
                .skip(1)
                .filter_map(|t| {
                    let body = t.split_once('>')?.1;
                    Some(unescape(body.split("</t>").next().unwrap_or("")))
                })
                .collect()
        })
        .collect()
}

/// A column's number from a cell reference's letters (A 0, Z 25, AA 26).
fn column_of(reference: &str) -> usize {
    reference
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .fold(0, |n, c| n * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1))
        .saturating_sub(1)
}

/// An Excel workbook's first sheet as rows of cells.
pub fn parse_xlsx(data: &[u8]) -> Result<Vec<Vec<String>>, String> {
    let files = unzip(data);
    let file = |name: &str| files.iter().find(|(n, _)| n == name).map(|(_, c)| String::from_utf8_lossy(c).into_owned());
    let sheet = file("xl/worksheets/sheet1.xml").ok_or("Not an Excel workbook (no first sheet).")?;
    let strings = file("xl/sharedStrings.xml").map(|x| shared_strings(&x)).unwrap_or_default();
    let mut rows = Vec::new();
    for row in sheet.split("<row").skip(1) {
        let row = row.split("</row>").next().unwrap_or("");
        let mut cells: Vec<String> = Vec::new();
        for c in row.split("<c ").skip(1) {
            let (attrs, rest) = c.split_once('>').unwrap_or((c, ""));
            let attr = |name: &str| {
                attrs.split_whitespace().find_map(|a| a.strip_prefix(name)?.strip_prefix("=\"")?.split('"').next())
            };
            let col = attr("r").map_or(cells.len(), column_of);
            let value = if attrs.trim_end().ends_with('/') {
                String::new()
            } else if attr("t") == Some("inlineStr") {
                rest.split("<t")
                    .nth(1)
                    .and_then(|t| t.split_once('>'))
                    .map(|(_, b)| unescape(b.split("</t>").next().unwrap_or("")))
                    .unwrap_or_default()
            } else {
                let v = rest.split("<v>").nth(1).and_then(|v| v.split("</v>").next()).unwrap_or("");
                match attr("t") {
                    Some("s") => v.parse::<usize>().ok().and_then(|i| strings.get(i).cloned()).unwrap_or_default(),
                    _ => unescape(v),
                }
            };
            if cells.len() <= col {
                cells.resize(col + 1, String::new());
            }
            cells[col] = value;
        }
        rows.push(cells);
    }
    Ok(rows)
}

/// A spreadsheet file's cells.
pub fn read_sheet(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let data = std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let rows = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("xlsx")) {
        parse_xlsx(&data)?
    } else {
        parse_csv(&String::from_utf8_lossy(&data))
    };
    if rows.iter().all(|r| r.iter().all(|c| c.trim().is_empty())) {
        return Err(format!("{} has no cells.", path.display()));
    }
    Ok(rows)
}

/// A table's grid and cells for `rows` under the title `title`: columns
/// as wide as their longest cell, rows two text heights high.
pub fn table_parts(title: &str, rows: &[Vec<String>], text_height: f64) -> (Vec<f64>, Vec<f64>, Vec<Vec<String>>) {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let widths: Vec<f64> = (0..columns)
        .map(|c| {
            let chars = rows.iter().filter_map(|r| r.get(c)).map(|s| s.chars().count()).max().unwrap_or(0);
            (chars as f64 * text_height * 0.75 + text_height * 2.0).max(text_height * 6.0)
        })
        .collect();
    let mut cells = vec![vec![String::new(); columns]];
    cells[0][0] = title.into();
    cells.extend(rows.iter().map(|r| (0..columns).map(|c| r.get(c).cloned().unwrap_or_default()).collect()));
    let heights = vec![text_height * 2.0; cells.len()];
    (widths, heights, cells)
}

impl OxidraftApp {
    /// DATALINK: a spreadsheet picked becomes a table to place.
    pub(crate) fn data_link(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("Spreadsheet", &["xlsx", "csv"]).pick_file() else { return };
        if let Err(e) = self.data_link_file(&file) {
            self.engine.log(e);
        }
    }

    /// The table `file` makes, to place; it's linked once placed.
    pub fn data_link_file(&mut self, file: &Path) -> Result<(), String> {
        let rows = read_sheet(file)?;
        let title = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let text_height = self.doc.table_style().text_height;
        let (widths, heights, cells) = table_parts(&title, &rows, text_height);
        let table = Geometry::Table {
            position: Point::ORIGIN,
            rotation: 0.0,
            widths,
            heights,
            cells,
            text_height,
            formats: Vec::new(),
        };
        let newest = self.doc.all_entities().map(|e| e.id).max().unwrap_or(EntityId(0));
        self.pending_data_link = Some((file.to_string_lossy().into_owned(), newest));
        self.engine.insert_table(table, &mut self.doc);
        Ok(())
    }

    /// Links the table DATALINK placed, once it's placed.
    pub(crate) fn finish_data_link(&mut self) {
        if self.engine.is_active() {
            return;
        }
        let Some((path, newest)) = self.pending_data_link.take() else { return };
        let placed = self
            .doc
            .all_entities()
            .filter(|e| e.id > newest && matches!(e.geometry, Geometry::Table { .. }))
            .map(|e| e.id)
            .last();
        if let Some(id) = placed {
            let link = Hyperlink { url: path, description: DATA_LINK.into() };
            self.doc.begin("DATALINK");
            self.doc.update(id, |e| e.hyperlink = Some(Box::new(link)));
            self.doc.commit();
        }
    }

    /// DATALINKUPDATE: every linked table filled from its file again; how
    /// many.
    pub fn data_link_update(&mut self) -> usize {
        let linked: Vec<(EntityId, String)> = self
            .doc
            .all_entities()
            .filter(|e| matches!(e.geometry, Geometry::Table { .. }))
            .filter_map(|e| e.hyperlink.as_ref().filter(|h| h.description == DATA_LINK).map(|h| (e.id, h.url.clone())))
            .collect();
        let mut updated = 0;
        self.doc.begin("DATALINKUPDATE");
        for (id, url) in linked {
            let file = crate::xref::resolve(self.path.as_deref(), &url).unwrap_or_else(|| url.clone().into());
            match read_sheet(&file) {
                Ok(rows) => {
                    self.doc.update(id, |e| {
                        if let Geometry::Table { widths, heights, cells, text_height, .. } = &mut e.geometry {
                            let title = cells.first().and_then(|r| r.first()).cloned().unwrap_or_default();
                            (*widths, *heights, *cells) = table_parts(&title, &rows, *text_height);
                        }
                    });
                    updated += 1;
                }
                Err(e) => self.engine.log(e),
            }
        }
        self.doc.commit();
        self.engine.log(format!("{updated} linked table(s) updated."));
        updated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_and_xlsx_cells() {
        assert_eq!(parse_csv("a,\"b, c\"\n1,\"say \"\"hi\"\"\"\n"), vec![vec!["a", "b, c"], vec!["1", "say \"hi\""]]);
        assert_eq!(column_of("C7"), 2);
        assert_eq!(column_of("AA1"), 26);
        // A workbook: shared strings, a number, an inline string, a gap.
        let sheet = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" t="s"><v>1</v></c></row><row r="2"><c r="A2"><v>12.5</v></c><c r="B2" t="inlineStr"><is><t>x &amp; y</t></is></c></row></sheetData></worksheet>"#;
        let strings =
            r#"<sst><si><t>Door</t></si><si><r><t>Fire</t></r><r><t xml:space="preserve"> rating</t></r></si></sst>"#;
        let files = vec![
            ("xl/worksheets/sheet1.xml".to_string(), sheet.as_bytes().to_vec()),
            ("xl/sharedStrings.xml".to_string(), strings.as_bytes().to_vec()),
        ];
        let rows = parse_xlsx(&oxidraft_plot::zip::zip(&files)).unwrap();
        assert_eq!(rows, vec![vec!["Door", "", "Fire rating"], vec!["12.5", "x & y"]]);
        let (widths, heights, cells) = table_parts("Doors", &rows, 2.5);
        assert_eq!(
            (widths.len(), heights.len(), cells[0][0].as_str(), cells[1][2].as_str()),
            (3, 3, "Doors", "Fire rating")
        );
    }
}

//! More of AutoCAD LT's commands, the app's side: attribute extraction
//! (EATTEXT, ATTEXT, DATAEXTRACTION) to CSV, WMFIN, PUBLISHTOWEB,
//! PSETUPIN, the quick views (QVDRAWING, QVLAYOUT), CUIEXPORT and
//! CUIIMPORT, and hyperlinks pasted or views gone to.

use std::path::Path;

use eframe::egui;
use oxidraft_geom::Point;
use oxidraft_model::{Attach, EntityId, Geometry, HatchPattern, MODEL, Vertex};
use oxidraft_plot::{PlotArea, Plotter};
use serde::{Deserialize, Serialize};

use crate::app::OxidraftApp;

/// What CUIEXPORT writes and CUIIMPORT reads.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Customization {
    pub shortcuts: Vec<(String, String)>,
    pub quick_access: Vec<String>,
    pub aliases: Vec<(String, String)>,
    pub workspaces: Vec<crate::customize::Workspace>,
    pub commands: Vec<crate::settings::CustomCommand>,
    pub ribbon_panels: Vec<crate::settings::RibbonPanel>,
}

/// The block references' attributes as CSV: block name, count 1, X, Y,
/// layer, then a column for each tag found.
pub fn attributes_csv(doc: &oxidraft_model::Document) -> (String, usize) {
    let mut tags: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for e in doc.all_entities() {
        let Geometry::Insert { block, placement, attributes, .. } = &e.geometry else { continue };
        if attributes.is_empty() {
            continue;
        }
        for a in attributes {
            if !tags.iter().any(|t| t.eq_ignore_ascii_case(&a.tag)) {
                tags.push(a.tag.clone());
            }
        }
        rows.push((block.clone(), placement.position, e.layer.clone(), attributes.clone()));
    }
    let field =
        |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let mut out = String::from("Name,Count,Position X,Position Y,Layer");
    for t in &tags {
        out.push(',');
        out.push_str(&field(t));
    }
    out.push('\n');
    let n = rows.len();
    for (block, at, layer, attributes) in rows {
        out.push_str(&format!("{},1,{},{},{}", field(&block), at.x, at.y, field(&layer)));
        for t in &tags {
            let v = attributes.iter().find(|a| a.tag.eq_ignore_ascii_case(t)).map_or("", |a| a.value.as_str());
            out.push(',');
            out.push_str(&field(v));
        }
        out.push('\n');
    }
    (out, n)
}

/// A placeable Windows Metafile's drawing as objects, in millimeters with
/// y up: polylines and lines, polygons as solid hatches, text.
pub fn wmf_objects(data: &[u8]) -> Result<Vec<Geometry>, String> {
    let u16_at = |i: usize| data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let i16_at = |i: usize| u16_at(i).map(|v| v as i16);
    let u32_at = |i: usize| data.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let placeable = u32_at(0) == Some(0x9AC6_CDD7);
    let (mut at, per_inch, bottom) = if placeable {
        (22, f64::from(u16_at(14).unwrap_or(1440).max(1)), f64::from(i16_at(12).unwrap_or(0)))
    } else {
        (0, 1440.0, 0.0)
    };
    let header_words = u16_at(at + 2).ok_or("Not a Windows Metafile.")?;
    if u16_at(at) != Some(1) && u16_at(at) != Some(2) {
        return Err("Not a Windows Metafile.".into());
    }
    at += header_words as usize * 2;
    let k = 25.4 / per_inch;
    let mut window_org = (0.0, 0.0);
    let mut height = bottom;
    let to = |x: i16, y: i16, org: (f64, f64), h: f64| {
        Point::new((f64::from(x) - org.0) * k, (h - (f64::from(y) - org.1)) * k)
    };
    let mut out = Vec::new();
    let mut pen = Point::ORIGIN;
    let mut font_height = 12.0;
    let mut escapement = 0.0;
    while at + 6 <= data.len() {
        let words = u32_at(at).unwrap_or(0) as usize;
        let function = u16_at(at + 4).unwrap_or(0);
        if words < 3 || function == 0 {
            break;
        }
        let p = at + 6;
        let points = |n: usize, from: usize| -> Vec<(i16, i16)> {
            (0..n).filter_map(|i| Some((i16_at(from + 4 * i)?, i16_at(from + 4 * i + 2)?))).collect()
        };
        match function {
            // SetWindowOrg, SetWindowExt: y then x.
            0x020B => window_org = (f64::from(i16_at(p + 2).unwrap_or(0)), f64::from(i16_at(p).unwrap_or(0))),
            0x020C if !placeable || height == 0.0 => height = f64::from(i16_at(p).unwrap_or(0)),
            0x0214 => pen = to(i16_at(p + 2).unwrap_or(0), i16_at(p).unwrap_or(0), window_org, height),
            0x0213 => {
                let end = to(i16_at(p + 2).unwrap_or(0), i16_at(p).unwrap_or(0), window_org, height);
                out.push(Geometry::Line { start: pen, end });
                pen = end;
            }
            // Polyline, Polygon.
            0x0325 | 0x0324 => {
                let n = u16_at(p).unwrap_or(0) as usize;
                let pts: Vec<Point> = points(n, p + 2).into_iter().map(|(x, y)| to(x, y, window_org, height)).collect();
                if function == 0x0324 && pts.len() >= 3 {
                    let loops = vec![pts.iter().map(|q| Vertex::at(*q)).collect()];
                    out.push(Geometry::Hatch {
                        loops,
                        pattern: HatchPattern::Solid,
                        scale: 1.0,
                        angle: 0.0,
                        origin: Point::ORIGIN,
                    });
                } else if pts.len() >= 2 {
                    out.push(Geometry::Polyline { vertices: pts.into_iter().map(Vertex::at).collect(), closed: false });
                }
            }
            0x0538 => {
                let n = u16_at(p).unwrap_or(0) as usize;
                let counts: Vec<usize> = (0..n).filter_map(|i| u16_at(p + 2 + 2 * i).map(usize::from)).collect();
                let mut from = p + 2 + 2 * n;
                let mut loops = Vec::new();
                for c in counts {
                    loops.push(
                        points(c, from).into_iter().map(|(x, y)| Vertex::at(to(x, y, window_org, height))).collect(),
                    );
                    from += 4 * c;
                }
                out.push(Geometry::Hatch {
                    loops,
                    pattern: HatchPattern::Solid,
                    scale: 1.0,
                    angle: 0.0,
                    origin: Point::ORIGIN,
                });
            }
            0x02FB => {
                font_height = f64::from(i16_at(p).unwrap_or(12)).abs();
                escapement = f64::from(i16_at(p + 4).unwrap_or(0)) / 10.0;
            }
            // ExtTextOut: y, x, count, options (and a rectangle), text.
            0x0A32 => {
                let (y, x, n, options) = (i16_at(p), i16_at(p + 2), u16_at(p + 4), u16_at(p + 6));
                if let (Some(y), Some(x), Some(n), Some(options)) = (y, x, n, options) {
                    let start = p + 8 + if options & 0x0006 != 0 { 8 } else { 0 };
                    let text: String =
                        data.get(start..start + n as usize).unwrap_or(&[]).iter().map(|b| char::from(*b)).collect();
                    out.push(Geometry::Text {
                        position: to(x, y, window_org, height),
                        height: font_height * k * 0.7,
                        rotation: escapement,
                        width: 0.0,
                        attach: Attach::BottomLeft,
                        content: text,
                        look: Default::default(),
                    });
                }
            }
            _ => {}
        }
        at += words * 2;
    }
    Ok(out)
}

impl OxidraftApp {
    /// EATTEXT: the attributes to a CSV file picked.
    pub(crate) fn extract_attributes(&mut self) {
        let (csv, n) = attributes_csv(&self.doc);
        if n == 0 {
            self.engine.log("No block references with attributes.");
            return;
        }
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("Attributes.csv").save_file()
        else {
            return;
        };
        match std::fs::write(&path, csv) {
            Ok(()) => self.engine.log(format!("{n} block reference(s) written to {}", path.display())),
            Err(e) => self.engine.log(format!("Could not write {}: {e}", path.display())),
        }
    }

    /// WMFIN: picks a metafile and brings it in at the origin.
    pub(crate) fn wmf_in(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("Windows Metafile", &["wmf"]).pick_file() else { return };
        match self.wmf_in_file(&file) {
            Ok(n) => self.engine.log(format!("{n} object(s) brought in from {}", file.display())),
            Err(e) => self.engine.log(e),
        }
    }

    pub fn wmf_in_file(&mut self, file: &Path) -> Result<usize, String> {
        let data = std::fs::read(file).map_err(|e| format!("Could not read {}: {e}", file.display()))?;
        let objects = wmf_objects(&data)?;
        self.engine.cancel(&mut self.doc);
        self.doc.begin("WMFIN");
        let n = objects.len();
        for g in objects {
            self.doc.add(g);
        }
        self.doc.commit();
        Ok(n)
    }

    /// PUBLISHTOWEB: a folder picked gets a web page showing the
    /// drawing's sheets, each as a PNG image.
    pub(crate) fn publish_to_web(&mut self) {
        let Some(folder) = rfd::FileDialog::new().set_title("Publish to Web: folder").pick_folder() else { return };
        match self.publish_to_web_in(&folder) {
            Ok(n) => {
                self.engine.log(format!("Web page of {n} sheet(s) written to {}", folder.join("index.html").display()))
            }
            Err(e) => self.engine.log(e),
        }
    }

    pub fn publish_to_web_in(&mut self, folder: &Path) -> Result<usize, String> {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let mut spaces: Vec<(String, oxidraft_model::Space)> = vec![("Model".into(), MODEL)];
        spaces.extend(self.doc.layouts().iter().map(|l| (l.name.clone(), l.id)));
        let title = self.file_name();
        let mut figures = String::new();
        let mut n = 0;
        for (i, (name, space)) in spaces.iter().enumerate() {
            let mut settings = self.plot_settings;
            settings.area = if *space == MODEL { PlotArea::Extents } else { PlotArea::Layout };
            let Some(sheet) = oxidraft_plot::render(&self.doc, *space, &settings) else { continue };
            let png = self.plot_bytes(Plotter::Png, std::slice::from_ref(&sheet))?;
            let file = format!("sheet{}.png", i + 1);
            std::fs::write(folder.join(&file), png).map_err(|e| e.to_string())?;
            let esc = name.replace('&', "&amp;").replace('<', "&lt;");
            figures.push_str(&format!(
                "<figure><img src=\"{file}\" alt=\"{esc}\"><figcaption>{esc}</figcaption></figure>\n"
            ));
            n += 1;
        }
        let esc = title.replace('&', "&amp;").replace('<', "&lt;");
        let html = format!(
            "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{esc}</title>\n<style>body{{font-family:sans-serif;margin:24px;background:#f4f4f4}}figure{{background:#fff;padding:12px;margin:0 0 24px}}img{{max-width:100%}}</style></head>\n<body><h1>{esc}</h1>\n{figures}</body></html>\n"
        );
        std::fs::write(folder.join("index.html"), html).map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// PSETUPIN: another drawing's page setup (its layout of the same name,
    /// else its first) onto the layout shown.
    pub(crate) fn psetupin(&mut self) {
        let Some(file) = rfd::FileDialog::new().add_filter("Drawing", &["dwg", "dxf"]).pick_file() else { return };
        match self.psetupin_from(&file) {
            Ok(name) => self.engine.log(format!("Page setup of \"{name}\" brought in.")),
            Err(e) => self.engine.log(e),
        }
    }

    pub fn psetupin_from(&mut self, file: &Path) -> Result<String, String> {
        let space = self.shown_space();
        let here = self.doc.layout(space).cloned().ok_or("PSETUPIN works on a layout tab.")?;
        let other = crate::xref::read_drawing(file)?;
        let layouts = other.layouts();
        let from = layouts
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(&here.name))
            .or_else(|| layouts.first())
            .ok_or("That drawing has no layouts.")?;
        let name = from.name.clone();
        self.doc.begin("PSETUPIN");
        self.doc.set_paper(space, from.paper.clone());
        self.doc.commit();
        Ok(name)
    }

    /// QVLAYOUT: the layouts, to pick one.
    pub(crate) fn quick_view_layouts(&mut self, ctx: &egui::Context) {
        if !self.qv_layouts {
            return;
        }
        let mut open = true;
        let mut pick = None;
        egui::Window::new("Quick View Layouts")
            .id(egui::Id::unique("qvlayout"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos([400.0, 200.0])
            .show(ctx, |ui| {
                let shown = self.shown_space();
                if ui.selectable_label(shown == MODEL, "Model").clicked() {
                    pick = Some(MODEL);
                }
                for l in self.doc.layouts() {
                    if ui.selectable_label(shown == l.id, &l.name).clicked() {
                        pick = Some(l.id);
                    }
                }
            });
        if let Some(space) = pick {
            self.switch_space(space);
            open = false;
        }
        self.qv_layouts = open;
    }

    /// CUIEXPORT: your customization to a file picked.
    pub(crate) fn cui_export(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Customization", &["json"])
            .set_file_name("Oxidraft.cui.json")
            .save_file()
        else {
            return;
        };
        let c = Customization {
            shortcuts: self.settings.shortcuts.clone(),
            quick_access: self.settings.quick_access.clone(),
            aliases: self.settings.aliases.clone(),
            workspaces: self.settings.workspaces.clone(),
            commands: self.settings.commands.clone(),
            ribbon_panels: self.settings.ribbon_panels.clone(),
        };
        let written = serde_json::to_string_pretty(&c)
            .map_err(|e| e.to_string())
            .and_then(|t| std::fs::write(&path, t).map_err(|e| e.to_string()));
        match written {
            Ok(()) => self.engine.log(format!("Customization saved to {}", path.display())),
            Err(e) => self.engine.log(format!("Could not save it: {e}")),
        }
    }

    /// CUIIMPORT: a customization file's shortcuts, toolbar commands,
    /// aliases and workspaces, added to yours (theirs win).
    pub fn cui_import_from(&mut self, path: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let c: Customization = serde_json::from_str(&text).map_err(|e| format!("Not a customization file: {e}"))?;
        let mut s = self.settings.clone();
        for (k, cmd) in c.shortcuts {
            s.shortcuts.retain(|(x, _)| !x.eq_ignore_ascii_case(&k));
            s.shortcuts.push((k, cmd));
        }
        for q in c.quick_access {
            if !s.quick_access.contains(&q) {
                s.quick_access.push(q);
            }
        }
        for (a, cmd) in c.aliases {
            s.aliases.retain(|(x, _)| !x.eq_ignore_ascii_case(&a));
            s.aliases.push((a, cmd));
        }
        for w in c.workspaces {
            s.workspaces.retain(|x| !x.name.eq_ignore_ascii_case(&w.name));
            s.workspaces.push(w);
        }
        for command in c.commands {
            s.commands.retain(|x| !x.id.eq_ignore_ascii_case(&command.id));
            s.commands.push(command);
        }
        for p in c.ribbon_panels {
            if !s.ribbon_panels.contains(&p) {
                s.ribbon_panels.push(p);
            }
        }
        self.keep_settings(s);
        Ok(())
    }

    /// CUIIMPORT: AutoCAD's .cuix or .cui (loaded as CUILOAD loads
    /// them), or a file CUIEXPORT wrote.
    pub(crate) fn cui_import(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Customization", &["cuix", "cui", "json"]).pick_file()
        else {
            return;
        };
        let json = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let taken = if json { self.cui_import_from(&path) } else { self.load_customization(&path).map(|_| ()) };
        match taken {
            Ok(()) => self.engine.log(format!("Customization taken in from {}", path.display())),
            Err(e) => self.engine.log(e),
        }
    }

    /// PASTEASHYPERLINK: the clipboard's text on `ids` as their hyperlink.
    pub(crate) fn paste_as_hyperlink(&mut self, ids: &[EntityId]) {
        let text = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
        let url = text.trim();
        if url.is_empty() {
            self.engine.log("The clipboard has no text to link to.");
            return;
        }
        let link = oxidraft_model::Hyperlink { url: url.into(), description: String::new() };
        self.doc.begin("PASTEASHYPERLINK");
        for id in ids {
            self.doc.update(*id, |e| e.hyperlink = Some(Box::new(link.clone())));
        }
        self.doc.commit();
    }

    /// VIEWGO: the named view `name` made current.
    pub(crate) fn go_to_view(&mut self, name: &str) {
        if let Some(view) = self.doc.views().iter().find(|v| v.name.eq_ignore_ascii_case(name)).cloned() {
            self.restore_view(&view);
        }
    }

    /// MARKUPCLOSE: the markups hidden (layer MARKUP off).
    pub(crate) fn markup_close(&mut self) {
        if let Some(l) = self.doc.layer("MARKUP").cloned() {
            self.doc.begin("MARKUPCLOSE");
            self.doc.update_layer(oxidraft_model::Layer { on: false, ..l });
            self.doc.commit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidraft_model::{Attribute, Color, Document, Placement};

    #[test]
    fn attributes_extract_as_csv() {
        let mut d = Document::new();
        let a = |tag: &str, value: &str| Attribute {
            tag: tag.into(),
            value: value.into(),
            layer: "0".into(),
            color: Color::ByLayer,
            position: Point::ORIGIN,
            height: 2.5,
            rotation: 0.0,
            attach: Attach::BottomLeft,
            invisible: false,
        };
        d.add(Geometry::Insert {
            block: "DOOR".into(),
            placement: Placement::at(Point::new(10.0, 5.0)),
            attributes: vec![a("NO", "D1"), a("FIRE", "EI30")],
            clip: None,
        });
        let (csv, n) = attributes_csv(&d);
        assert_eq!(n, 1);
        assert_eq!(csv, "Name,Count,Position X,Position Y,Layer,NO,FIRE\nDOOR,1,10,5,0,D1,EI30\n");
    }

    #[test]
    fn a_metafile_comes_back_in() {
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 50.0) });
        let settings = oxidraft_plot::PlotSettings { area: PlotArea::Extents, ..Default::default() };
        let sheet = oxidraft_plot::render(&doc, MODEL, &settings).unwrap();
        let objects = wmf_objects(&oxidraft_plot::wmf::wmf(&sheet)).unwrap();
        let line = objects.iter().find_map(|g| match g {
            Geometry::Polyline { vertices, .. } => Some((vertices[0].point, vertices[1].point)),
            _ => None,
        });
        let (a, b) = line.expect("the line, as a polyline");
        // The same slope as drawn, on the sheet in millimeters.
        assert!(((b.y - a.y) / (b.x - a.x) - 0.5).abs() < 0.01, "{a:?} {b:?}");
    }
}

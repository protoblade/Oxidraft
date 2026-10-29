//! ETRANSMIT: the drawing and the files it needs (attached drawings,
//! images, PDFs) copied together into one folder, the copy pointing at
//! them there, with a transmittal report of what went in.

use std::path::{Path, PathBuf};

use oxidraft_model::{ImageDef, Xref};

use crate::app::OxidraftApp;
use crate::xref::resolve;

impl OxidraftApp {
    /// ETRANSMIT: picks the folder the transmittal goes to.
    pub(crate) fn etransmit(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().set_title("Create Transmittal: folder").pick_folder() {
            match self.transmit_to(&folder) {
                Ok(n) => self.engine.log(format!("Transmittal of {n} file(s) created in {}", folder.display())),
                Err(e) => self.engine.log(format!("Could not create the transmittal: {e}")),
            }
        }
    }

    /// Copies the drawing and its files into `folder`; how many files.
    pub fn transmit_to(&mut self, folder: &Path) -> Result<usize, String> {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let mut copy = self.doc.clone();
        let mut report = vec![format!("Transmittal of {}", self.file_name()), String::new(), "Files:".into()];
        let mut missing = Vec::new();
        let mut taken: Vec<PathBuf> = Vec::new();
        // Each file once, by its name in the folder.
        let mut take = |from: &Path| -> Result<String, String> {
            let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("no file name")?;
            if !taken.iter().any(|t| t == from) {
                std::fs::copy(from, folder.join(&name)).map_err(|e| format!("{}: {e}", from.display()))?;
                taken.push(from.to_path_buf());
            }
            Ok(name)
        };
        let host = self.path.clone();
        let xrefs: Vec<Xref> = copy.xrefs().cloned().collect();
        for x in xrefs {
            match resolve(host.as_deref(), &x.path) {
                Some(found) => {
                    let name = take(&found)?;
                    report.push(format!("  {name}  (attached drawing {})", x.name));
                    copy.set_xref(Xref { path: name, ..x });
                }
                None => missing.push(format!("  {}  (attached drawing {})", x.path, x.name)),
            }
        }
        let defs: Vec<ImageDef> = copy.image_defs().cloned().collect();
        for d in defs {
            match resolve(host.as_deref(), &d.path) {
                Some(found) => {
                    let name = take(&found)?;
                    let kind = if d.is_pdf() { "PDF underlay" } else { "image" };
                    report.push(format!("  {name}  ({kind} {})", d.name));
                    copy.set_image_def(ImageDef { path: name, ..d });
                }
                None => missing.push(format!("  {}  (picture {})", d.path, d.name)),
            }
        }
        // The drawing itself, in its own format.
        let file = self.file_name();
        let target = folder.join(&file);
        let written = if oxidraft_io_dwg::is_dwg(&target) {
            oxidraft_io_dwg::save_file(&copy, &target, self.dwg_version.unwrap_or_default())
        } else {
            oxidraft_io_dwg::save_dxf(&copy, &target)
        };
        written?;
        report.insert(3, format!("  {file}  (the drawing)"));
        if !missing.is_empty() {
            report.push(String::new());
            report.push("Not found, left out:".into());
            report.extend(missing);
        }
        let stem = Path::new(&file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::write(folder.join(format!("{stem}.txt")), report.join("\n") + "\n").map_err(|e| e.to_string())?;
        Ok(taken.len() + 1)
    }
}

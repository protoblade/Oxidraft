//! Plots a DXF or DWG file to PDF, fitted to the paper:
//! `cargo run -p oxidraft-plot --example plot -- drawing.dwg drawing.pdf [paper index] [portrait]`

use oxidraft_plot::{PAPERS, PlotSettings, pdf, render};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("usage: plot <in.dwg|in.dxf> <out.pdf> [paper index] [portrait]")?;
    let output = args.next().unwrap_or_else(|| format!("{input}.pdf"));
    let paper = args.next().and_then(|i| i.parse::<usize>().ok()).and_then(|i| PAPERS.get(i)).copied();
    let portrait = args.next().is_some_and(|a| a == "portrait");
    let doc = if oxidraft_io_dwg::is_dwg(&input) {
        oxidraft_io_dwg::load_file(&input)?.imported.document
    } else {
        oxidraft_io_dxf::load_file(&input)?.document
    };
    let settings = PlotSettings { paper: paper.unwrap_or(PAPERS[0]), landscape: !portrait, ..PlotSettings::default() };
    let sheet = render(&doc, &settings).ok_or("nothing to plot")?;
    std::fs::write(&output, pdf(&sheet, &input))?;
    println!("Wrote {output}: {} items, {:.2} mm per unit", sheet.items.len(), sheet.layout.scale);
    Ok(())
}

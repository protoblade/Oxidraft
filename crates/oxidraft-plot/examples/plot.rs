//! Plots a DXF or DWG file to PDF: model space fitted to the paper, or a
//! layout as its page setup has it:
//! `cargo run -p oxidraft-plot --example plot -- drawing.dwg drawing.pdf [paper index | layout name] [portrait]`

use oxidraft_plot::{PAPERS, PlotArea, PlotSettings, pdf, render};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: plot <in.dwg|in.dxf> <out.pdf> [paper index | layout name] [portrait]";
    let input = args.next().ok_or(usage)?;
    let output = args.next().unwrap_or_else(|| format!("{input}.pdf"));
    let which = args.next();
    let portrait = args.next().is_some_and(|a| a == "portrait");
    let doc = if oxidraft_io_dwg::is_dwg(&input) {
        oxidraft_io_dwg::load_file(&input)?.imported.document
    } else {
        oxidraft_io_dxf::load_file(&input)?.document
    };
    let paper = which.as_deref().and_then(|i| i.parse::<usize>().ok()).and_then(|i| PAPERS.get(i)).copied();
    let layout = which.as_deref().filter(|_| paper.is_none()).map(|name| {
        doc.layouts().into_iter().find(|l| l.name.eq_ignore_ascii_case(name)).ok_or(format!("no layout {name}"))
    });
    let mut settings =
        PlotSettings { paper: paper.unwrap_or(PAPERS[0]), landscape: !portrait, ..PlotSettings::default() };
    let space = match layout {
        Some(l) => {
            settings.area = PlotArea::Layout;
            l?.id
        }
        None => oxidraft_model::MODEL,
    };
    let sheet = render(&doc, space, &settings).ok_or("nothing to plot")?;
    std::fs::write(&output, pdf(&sheet, &input))?;
    println!("Wrote {output}: {} items, {:.2} mm per unit", sheet.items.len(), sheet.layout.scale);
    Ok(())
}

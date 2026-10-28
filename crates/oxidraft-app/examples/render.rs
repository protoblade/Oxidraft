//! Renders a DXF file headlessly to a PNG, for checking how drawings look:
//! `cargo run -p oxidraft-app --example render -- drawing.dxf out.png [x1 y1 x2 y2]`
//! The optional corners zoom to a window instead of the drawing's extents.

use eframe::egui::vec2;
use egui_kittest::Harness;
use oxidraft_app::OxidraftApp;
use oxidraft_geom::Point;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(input), Some(output)) = (args.first(), args.get(1)) else {
        eprintln!("usage: render <drawing.dxf> <out.png> [x1 y1 x2 y2]");
        std::process::exit(2);
    };
    let window: Vec<f64> = args[2..].iter().filter_map(|a| a.parse().ok()).collect();
    let input = std::path::PathBuf::from(input);
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).wgpu().build_eframe(|cc| {
        let mut app = OxidraftApp::new(cc);
        app.open_path(input);
        if let [x1, y1, x2, y2] = window[..] {
            app.zoom_to(Point::new(x1, y1), Point::new(x2, y2));
        }
        app
    });
    h.run_steps(4);
    h.render().expect("render").save(output).expect("save png");
    println!("{output}");
}

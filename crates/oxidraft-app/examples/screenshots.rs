//! Renders the app headlessly in a few typical states and saves PNGs:
//! `cargo run -p oxidraft-app --example screenshots -- <out-dir>`
//!
//! Handy for reviewing UI changes without clicking through the app.

use std::path::{Path, PathBuf};

use eframe::egui::{Event, PointerButton, vec2};
use egui_kittest::{Harness, kittest::Queryable};
use oxidraft_app::{
    OxidraftApp, Settings,
    theme::{Background, ColorTheme},
    update::{Release, UpdateState},
};
use oxidraft_geom::Point;
use oxidraft_model::{
    Attach, Attribute, Block, BlockEntity, Color, Document, Ellipse, Geometry, HatchPattern, Layer, Placement, Spline,
    Vertex,
};

fn sample() -> Document {
    let mut doc = Document::new();
    doc.add_layer(Layer::new("Walls", 7));
    doc.add_layer(Layer::new("Doors", 2));
    doc.add_layer(Layer::new("Columns", 1));
    let mut wall = |a: (f64, f64), b: (f64, f64)| {
        let g = Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) };
        doc.add_on_layer(g, "Walls", Color::ByLayer);
    };
    wall((0.0, 0.0), (100.0, 0.0));
    wall((100.0, 0.0), (100.0, 60.0));
    wall((100.0, 60.0), (0.0, 60.0));
    wall((0.0, 60.0), (0.0, 35.0));
    wall((0.0, 25.0), (0.0, 0.0));
    wall((60.0, 0.0), (60.0, 60.0));
    doc.add_on_layer(
        Geometry::Line { start: Point::new(0.0, 25.0), end: Point::new(10.0, 25.0) },
        "Doors",
        Color::ByLayer,
    );
    for (x, y) in [(20.0, 20.0), (40.0, 20.0), (20.0, 40.0), (40.0, 40.0), (80.0, 30.0)] {
        doc.add_on_layer(Geometry::Circle { center: Point::new(x, y), radius: 3.0 }, "Columns", Color::ByLayer);
    }
    // Door swing and a table with rounded corners.
    doc.add_on_layer(
        Geometry::Arc { center: Point::new(0.0, 25.0), radius: 10.0, start_angle: 0.0, end_angle: 90.0 },
        "Doors",
        Color::ByLayer,
    );
    let b = (22.5f64).to_radians().tan();
    let table = [
        (72.0, 40.0, 0.0),
        (88.0, 40.0, b),
        (90.0, 42.0, 0.0),
        (90.0, 50.0, b),
        (88.0, 52.0, 0.0),
        (72.0, 52.0, b),
        (70.0, 50.0, 0.0),
        (70.0, 42.0, b),
    ];
    doc.add_on_layer(
        Geometry::Polyline {
            vertices: table.iter().map(|&(x, y, b)| Vertex::new(Point::new(x, y), b)).collect(),
            closed: true,
        },
        "0",
        Color::Index(4),
    );
    // Annotation: a title, overall dimensions, a leader, and the right room hatched
    // around the table and column.
    doc.add(Geometry::Text {
        look: Default::default(),
        position: Point::new(0.0, 80.0),
        height: 4.0,
        rotation: 0.0,
        width: 0.0,
        attach: Attach::TopLeft,
        content: "GROUND FLOOR PLAN".into(),
    });
    for (p1, p2, line, rotation) in [
        (Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(50.0, -9.0), 0.0),
        (Point::new(100.0, 0.0), Point::new(100.0, 60.0), Point::new(109.0, 30.0), 90.0),
        (Point::new(0.0, 60.0), Point::new(60.0, 60.0), Point::new(30.0, 66.0), 0.0),
    ] {
        doc.add(Geometry::Dimension {
            extra: Default::default(),
            p1,
            p2,
            line,
            rotation,
            aligned: false,
            text: None,
            scale: 1.0,
        });
    }
    doc.add(Geometry::Leader {
        points: vec![Point::new(82.2, 32.2), Point::new(92.0, 42.0)],
        content: "CONCRETE COLUMN".into(),
        scale: 1.0,
    });
    if let Some(loops) = oxidraft_model::boundary::boundary_at(&doc, Point::new(65.0, 10.0)) {
        doc.add_on_layer(
            Geometry::Hatch { loops, pattern: HatchPattern::Ansi31, scale: 1.5, angle: 0.0 },
            "Hatch",
            Color::Index(8),
        );
    }
    doc
}

fn harness() -> Harness<'static, OxidraftApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).wgpu().build_eframe(|cc| {
        let mut app = OxidraftApp::new(cc);
        app.set_document(sample());
        app
    });
    h.run_steps(3);
    h
}

fn hover(h: &mut Harness<'_, OxidraftApp>, p: Point) {
    let pos = h.state().screen_pos(p);
    h.hover_at(pos);
    h.run_steps(2);
}

fn click(h: &mut Harness<'_, OxidraftApp>, p: Point) {
    let pos = h.state().screen_pos(p);
    h.hover_at(pos);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() });
        h.run_steps(1);
    }
}

fn save(h: &mut Harness<'_, OxidraftApp>, dir: &Path, name: &str) {
    let image = h.render().expect("render");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save png");
    println!("{}", path.display());
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "screenshots".into()));
    std::fs::create_dir_all(&dir).unwrap();

    // Idle, hovering a wall (rollover highlight).
    let mut h = harness();
    hover(&mut h, Point::new(30.0, 0.3));
    save(&mut h, &dir, "1-idle-hover");

    // LINE in progress with polar tracking and dynamic input.
    let mut h = harness();
    h.state_mut().type_command("l");
    h.run_steps(1);
    click(&mut h, Point::new(60.0, 30.0));
    hover(&mut h, Point::new(85.0, 30.6));
    save(&mut h, &dir, "2-line-polar");

    // Endpoint snap while picking a point.
    hover(&mut h, Point::new(99.4, 59.3));
    save(&mut h, &dir, "3-line-snap");

    // Selection with grips, hovering a grip.
    let mut h = harness();
    click(&mut h, Point::new(30.0, 0.0));
    click(&mut h, Point::new(100.0, 30.0));
    click(&mut h, Point::new(83.0, 30.0));
    hover(&mut h, Point::new(100.0, 0.0));
    save(&mut h, &dir, "4-grips");

    // Crossing window in progress.
    let mut h = harness();
    click(&mut h, Point::new(50.0, 50.0));
    hover(&mut h, Point::new(15.0, 15.0));
    save(&mut h, &dir, "5-crossing-window");

    // ROTATE preview: select the right-hand room, base point at its corner.
    let mut h = harness();
    click(&mut h, Point::new(110.0, 65.0));
    click(&mut h, Point::new(55.0, -5.0));
    h.state_mut().type_command("ro");
    h.run_steps(1);
    click(&mut h, Point::new(60.0, 0.0));
    hover(&mut h, Point::new(95.0, 25.0));
    save(&mut h, &dir, "7-rotate-preview");

    // Selection preview while a window is open.
    let mut h = harness();
    click(&mut h, Point::new(-5.0, 65.0));
    hover(&mut h, Point::new(50.0, 10.0));
    save(&mut h, &dir, "8-window-preview");

    // PLINE in arc mode: tangent arc preview.
    let mut h = harness();
    h.state_mut().type_command("pl");
    h.run_steps(1);
    click(&mut h, Point::new(64.0, 8.0));
    click(&mut h, Point::new(84.0, 8.0));
    h.state_mut().type_command("a");
    h.run_steps(1);
    hover(&mut h, Point::new(94.0, 20.0));
    save(&mut h, &dir, "9-pline-arc");

    // TRIM: pickbox and rollover on the object to trim.
    let mut h = harness();
    h.state_mut().type_command("tr");
    h.run_steps(1);
    hover(&mut h, Point::new(30.0, 60.3));
    save(&mut h, &dir, "10-trim-pick");

    // OFFSET: preview on the cursor's side of the table.
    let mut h = harness();
    h.state_mut().type_command("o");
    h.run_steps(1);
    h.state_mut().type_command("3");
    h.run_steps(1);
    click(&mut h, Point::new(80.0, 40.0));
    hover(&mut h, Point::new(80.0, 36.0));
    save(&mut h, &dir, "11-offset-preview");

    // Update available: status bar badge and dialog.
    let mut h = harness();
    let release = Release {
        tag_name: "v0.2.0".into(),
        html_url: "https://github.com/protoblade/Oxidraft/releases/tag/v0.2.0".into(),
        body: Some(
            "What's new\n- PLINE, ARC, RECTANG\n- OFFSET, TRIM, EXTEND, FILLET\n- Arcs and polylines in DXF".into(),
        ),
        assets: Vec::new(),
    };
    h.state_mut().updater().set(UpdateState::Available(release));
    h.state_mut().show_update_dialog();
    h.run_steps(3);
    save(&mut h, &dir, "12-update-available");

    // Layer Properties Manager, with the Columns layer locked (drawn faded),
    // long survey layer names and a long linetype name, and three layers
    // picked with Shift to change together.
    let mut h = harness();
    let mut columns = h.state_mut().document_mut().layer("Columns").unwrap().clone();
    columns.locked = true;
    h.state_mut().document_mut().update_layer(columns);
    {
        let doc = h.state_mut().document_mut();
        doc.define_linetype(oxidraft_model::Linetype {
            name: "Полумасивна ограда обща за двата имота".into(),
            description: String::new(),
            pattern: vec![2.0, -1.0],
        });
        for name in
            ["CAD_BORDER_Half_Mass_Fence_common", "CAD_BORDER_Massive_Fence_L_R", "CarevaLivada_VARBANOVO1_modifiedG"]
        {
            doc.add_layer(Layer {
                linetype: "Полумасивна ограда обща за двата имота".into(), ..Layer::new(name, 250)
            });
        }
    }
    h.state_mut().type_command("la");
    h.run_steps(3);
    h.get_by_label("CAD_BORDER_Half_Mass_Fence_common").click();
    h.run_steps(1);
    h.get_by_label("CarevaLivada_VARBANOVO1_modifiedG").click_modifiers(eframe::egui::Modifiers::SHIFT);
    h.run_steps(2);
    save(&mut h, &dir, "13-layer-manager");

    // Properties palette for a selected wall, then the same docked at the
    // right.
    let mut h = harness();
    click(&mut h, Point::new(100.0, 30.0));
    h.state_mut().type_command("pr");
    h.run_steps(3);
    save(&mut h, &dir, "14-properties");
    h.get_by_label("Dock").click();
    h.run_steps(2);
    h.get_by_label("Dock Right").click();
    h.run_steps(3);
    save(&mut h, &dir, "14b-properties-right");

    // BLOCK from the table, then INSERT with a preview at the cursor.
    let mut h = harness();
    h.state_mut().type_command("b");
    h.run_steps(1);
    h.state_mut().type_command("TABLE");
    h.run_steps(1);
    click(&mut h, Point::new(70.0, 40.0));
    click(&mut h, Point::new(68.0, 38.0));
    click(&mut h, Point::new(92.0, 54.0));
    h.state_mut().type_command("");
    h.run_steps(1);
    h.state_mut().type_command("i");
    h.run_steps(1);
    h.state_mut().type_command("");
    h.run_steps(1);
    hover(&mut h, Point::new(10.0, 45.0));
    save(&mut h, &dir, "15-insert-preview");

    // HATCH: picking inside the left room's lower-left area.
    let mut h = harness();
    h.state_mut().type_command("h");
    h.run_steps(1);
    hover(&mut h, Point::new(30.0, 10.0));
    save(&mut h, &dir, "16-hatch-pick");

    // DIMLINEAR: the dimension follows the cursor before it's placed.
    let mut h = harness();
    h.state_mut().type_command("dli");
    h.run_steps(1);
    click(&mut h, Point::new(60.0, 0.0));
    click(&mut h, Point::new(60.0, 60.0));
    hover(&mut h, Point::new(54.0, 40.0));
    save(&mut h, &dir, "17-dimension-preview");

    // DIVIDE a wall into 6 with PDMODE 35 points, then LIST it.
    let mut h = harness();
    for typed in ["pdmode", "35", "div"] {
        h.state_mut().type_command(typed);
        h.run_steps(1);
    }
    click(&mut h, Point::new(30.0, 0.0));
    h.state_mut().type_command("6");
    h.run_steps(1);
    click(&mut h, Point::new(30.0, 0.0));
    h.state_mut().type_command("list");
    h.run_steps(2);
    hover(&mut h, Point::new(70.0, 45.0));
    save(&mut h, &dir, "18-divide-and-list");

    // The attribute editor on a survey point block.
    let mut h = harness();
    let doc = h.state_mut().document_mut();
    let circle = Geometry::Circle { center: Point::ORIGIN, radius: 1.5 };
    doc.define_block(Block {
        name: "KW".into(),
        base: Point::ORIGIN,
        entities: vec![BlockEntity::new("0", Color::ByBlock, circle)],
    });
    let attribute = |tag: &str, value: &str, dy: f64| Attribute {
        tag: tag.into(),
        value: value.into(),
        layer: "0".into(),
        color: Color::ByBlock,
        position: Point::new(52.0, 50.0 + dy),
        height: 2.0,
        rotation: 0.0,
        attach: Attach::BottomLeft,
        invisible: false,
    };
    doc.add(Geometry::Insert {
        block: "KW".into(),
        placement: Placement::at(Point::new(50.0, 50.0)),
        attributes: vec![attribute("KWNOM", "9", 0.0), attribute("ELEV", "101.52", -3.0)],
        clip: None,
    });
    h.run_steps(1);
    h.state_mut().type_command("eattedit");
    h.run_steps(1);
    click(&mut h, Point::new(51.5, 50.0));
    h.run_steps(2);
    save(&mut h, &dir, "19-attribute-editor");

    // Ellipses, a spline, construction lines, donuts, a tapered polyline,
    // a polygon and a revision cloud; the ellipse selected.
    let mut h = harness();
    let doc = h.state_mut().document_mut();
    doc.add(Geometry::Ellipse(Ellipse::full(Point::new(30.0, 30.0), Point::new(12.0, 0.0), 0.5)));
    doc.add(Geometry::Ellipse(Ellipse {
        center: Point::new(30.0, 30.0),
        major: Point::new(0.0, 22.0),
        ratio: 0.6,
        start: 300.0,
        end: 60.0,
    }));
    let fit = [(62.0, 5.0), (70.0, 18.0), (82.0, 8.0), (97.0, 24.0)].map(|(x, y)| Point::new(x, y));
    doc.add(Geometry::Spline(Spline::through(&fit, false).expect("a spline")));
    doc.add(Geometry::Xline { base: Point::ORIGIN, direction: Point::new(3.0, 1.0) });
    doc.add(Geometry::Ray { base: Point::new(100.0, 60.0), direction: Point::new(1.0, 0.4) });
    doc.add(Geometry::Polyline {
        vertices: vec![Vertex::at(Point::new(8.0, 72.0)).with_widths(0.0, 3.0), Vertex::at(Point::new(30.0, 72.0))],
        closed: false,
    });
    h.run_steps(1);
    for typed in [
        "do", "2", "4", "50,10", "", "do", "0", "3", "56,10", "", "pol", "6", "50,48", "i", "4", "revcloud", "a", "2.5",
    ] {
        h.state_mut().type_command(typed);
        h.run_steps(1);
    }
    click(&mut h, Point::new(67.0, 37.0));
    click(&mut h, Point::new(93.0, 55.0));
    click(&mut h, Point::new(42.0, 30.0));
    hover(&mut h, Point::new(40.0, 70.0));
    save(&mut h, &dir, "20-curves");

    // PLOT: the dialog, with the plan fitted to A4 landscape, and its preview.
    let mut h = harness();
    h.state_mut().type_command("plot");
    h.run_steps(6);
    save(&mut h, &dir, "21-plot-dialog");
    h.state_mut().type_command("");
    let mut h = harness();
    h.state_mut().type_command("preview");
    h.run_steps(2);
    save(&mut h, &dir, "22-plot-preview");

    // An attached site survey, faded, with the External References palette.
    let mut site = Document::new();
    site.add_layer(Layer::new("Parcels", 3));
    site.add_layer(Layer::new("Labels", 2));
    for (a, b) in [((-20.0, -15.0), (130.0, -15.0)), ((130.0, -15.0), (130.0, 85.0)), ((-20.0, 85.0), (130.0, 85.0))] {
        let g = Geometry::Line { start: Point::new(a.0, a.1), end: Point::new(b.0, b.1) };
        site.add_on_layer(g, "Parcels", Color::ByLayer);
    }
    site.add_on_layer(
        Geometry::Text {
            look: Default::default(),
            position: Point::new(110.0, 75.0),
            height: 3.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "ПИ 5022".into(),
        },
        "Labels",
        Color::ByLayer,
    );
    let site_path = dir.join("site-survey.dxf");
    oxidraft_io_dxf::save_file(&site, &site_path).expect("save the survey");
    let mut h = harness();
    h.state_mut().attach_file(site_path);
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    click(&mut h, Point::new(0.0, 0.0));
    h.state_mut().type_command("xr");
    h.run_steps(2);
    h.get_by_label("   site-survey").click();
    h.run_steps(3);
    save(&mut h, &dir, "23-external-references");

    // Four georeferenced tiles attached at once, their layers off: two
    // turned on (and so loaded), with the palette listing them all.
    let mut tiles = Vec::new();
    for (i, (x, y)) in [(-20.0, -20.0), (60.0, -20.0), (-20.0, 40.0), (60.0, 40.0)].into_iter().enumerate() {
        let png = dir.join(format!("ortho_{}.png", i + 1));
        let img = image::RgbaImage::from_fn(160, 120, |px, py| {
            let (fx, fy) = (f64::from(px) + x * 2.0, f64::from(py) - y * 2.0);
            let field = ((fx / 23.0).sin() + (fy / 17.0).cos()) * 0.5 + 0.5;
            let g = (90.0 + 80.0 * field) as u8;
            image::Rgba([g / 2, g, g / 3, 255])
        });
        img.save(&png).expect("save a tile");
        // Half a unit a pixel: each tile is 80 x 60 units.
        let world = format!("0.5\n0\n0\n-0.5\n{}\n{}\n", x + 0.25, y + 60.0 - 0.25);
        std::fs::write(png.with_extension("pgw"), world).expect("save its world file");
        tiles.push(png);
    }
    let mut h = harness();
    h.state_mut().attach_images_unloaded(&tiles);
    h.run_steps(2);
    // Their layers start off; turning two on loads those.
    for layer in ["ortho_1", "ortho_4"] {
        let doc = h.state_mut().document_mut();
        let l = doc.layer(layer).cloned().expect("its layer");
        doc.update_layer(Layer { on: true, ..l });
        h.run_steps(1);
    }
    for _ in 0..100 {
        h.run_steps(1);
        if !h.state().pictures_loading() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.state_mut().type_command("xr");
    h.run_steps(2);
    h.get_by_label("   ortho_2").click();
    h.run_steps(3);
    save(&mut h, &dir, "24-image-underlays");

    // XCLIP: the survey xref clipped to a polygon, its frame showing.
    let attach_site = |h: &mut Harness<'_, OxidraftApp>| {
        h.state_mut().attach_file(dir.join("site-survey.dxf"));
        h.run_steps(2);
        h.get_by_label("OK").click();
        h.run_steps(2);
        click(h, Point::new(0.0, 0.0));
    };
    let mut h = harness();
    attach_site(&mut h);
    h.state_mut().type_command("xclip");
    h.run_steps(1);
    click(&mut h, Point::new(130.0, 30.0));
    for input in ["", "", "p", "-10,-25", "120,-25", "140,40", "60,95", "-10,95", ""] {
        h.state_mut().type_command(input);
        h.run_steps(1);
    }
    hover(&mut h, Point::new(50.0, 30.0));
    save(&mut h, &dir, "25-xclip");

    // REFEDIT: the survey open for editing in place, the plan faded, a
    // label being added.
    let mut h = harness();
    attach_site(&mut h);
    h.state_mut().type_command("refedit");
    h.run_steps(1);
    click(&mut h, Point::new(130.0, 30.0));
    h.state_mut().type_command("l");
    h.run_steps(1);
    click(&mut h, Point::new(130.0, 30.0));
    hover(&mut h, Point::new(150.0, 45.0));
    save(&mut h, &dir, "26-refedit");

    // A large orthophoto zoomed in: the part on screen read again at the
    // screen's resolution (fine lines a pixel wide stay crisp).
    let ortho = dir.join("ortho-large.tif");
    image::RgbImage::from_fn(6000, 4000, |x, y| {
        let line = x % 50 == 0 || y % 50 == 0 || (x + y) % 97 == 0;
        let field = (90.0 + 40.0 * ((f64::from(x) / 300.0).sin() + (f64::from(y) / 210.0).cos())) as u8;
        if line { image::Rgb([240, 240, 230]) } else { image::Rgb([field / 2, field, field / 3]) }
    })
    .save(&ortho)
    .expect("save the orthophoto");
    std::fs::write(dir.join("ortho-large.tfw"), "0.05\n0\n0\n-0.05\n-49.975\n109.975\n").expect("its world file");
    let mut h = harness();
    h.state_mut().attach_images_unloaded(&[ortho]);
    h.run_steps(2);
    let l = h.state_mut().document_mut().layer("ortho-large").cloned().expect("its layer");
    h.state_mut().document_mut().update_layer(Layer { on: true, ..l });
    h.run_steps(1);
    for input in ["z", "20,20", "35,30"] {
        h.state_mut().type_command(input);
        h.run_steps(1);
    }
    for _ in 0..300 {
        h.run_steps(1);
        if !h.state().pictures_loading() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run_steps(2);
    save(&mut h, &dir, "27-image-detail");

    // The light color theme, first around the dark drawing area (AutoCAD's
    // default) with the Layer Properties Manager, then with a white one and
    // the Options dialog; both with a selection.
    for (name, background) in [("28-light-theme", Background::Dark), ("29-light-white-drawing", Background::White)] {
        let mut h = harness();
        let ctx = h.ctx.clone();
        h.state_mut()
            .apply_settings(&ctx, Settings { color_theme: ColorTheme::Light, background, ..Settings::default() });
        click(&mut h, Point::new(30.0, 0.0));
        click(&mut h, Point::new(80.0, 30.0));
        let dialog = if background == Background::White { "op" } else { "la" };
        h.state_mut().type_command(dialog);
        hover(&mut h, Point::new(45.0, 45.0));
        h.run_steps(2);
        save(&mut h, &dir, name);
        h.state_mut().apply_settings(&ctx, Settings::default());
    }

    // The Draw panel's slide-out, opened from its title.
    let mut h = harness();
    h.get_by_label("Draw panel").click();
    h.run_steps(3);
    save(&mut h, &dir, "30-draw-slideout");

    // The Blocks palette with three blocks, then one open in the Block Editor.
    let with_blocks = |h: &mut Harness<'_, OxidraftApp>| {
        let doc = h.state_mut().document_mut();
        let door = vec![
            BlockEntity::new("0", Color::ByLayer, Geometry::Line { start: Point::ORIGIN, end: Point::new(0.0, 9.0) }),
            BlockEntity::new(
                "0",
                Color::ByLayer,
                Geometry::Arc { center: Point::ORIGIN, radius: 9.0, start_angle: 0.0, end_angle: 90.0 },
            ),
        ];
        doc.define_block(Block { name: "DOOR".into(), base: Point::ORIGIN, entities: door });
        let column = vec![
            BlockEntity::new("0", Color::Index(1), Geometry::Circle { center: Point::ORIGIN, radius: 3.0 }),
            BlockEntity::new(
                "0",
                Color::Index(1),
                Geometry::Line { start: Point::new(-3.0, 0.0), end: Point::new(3.0, 0.0) },
            ),
            BlockEntity::new(
                "0",
                Color::Index(1),
                Geometry::Line { start: Point::new(0.0, -3.0), end: Point::new(0.0, 3.0) },
            ),
        ];
        doc.define_block(Block { name: "COLUMN".into(), base: Point::ORIGIN, entities: column });
        let desk = (0..4)
            .map(|i| {
                let (x, y) = (f64::from(i % 2) * 14.0, f64::from(i / 2) * 9.0);
                let v = |dx: f64, dy: f64| Vertex::at(Point::new(x + dx, y + dy));
                BlockEntity::new(
                    "0",
                    Color::Index(4),
                    Geometry::Polyline {
                        vertices: vec![v(0.0, 0.0), v(12.0, 0.0), v(12.0, 7.0), v(0.0, 7.0)],
                        closed: true,
                    },
                )
            })
            .collect();
        doc.define_block(Block { name: "DESKS".into(), base: Point::ORIGIN, entities: desk });
    };
    let mut h = harness();
    with_blocks(&mut h);
    h.state_mut().type_command("blockspalette");
    h.run_steps(3);
    save(&mut h, &dir, "31-blocks-palette");
    let mut h = harness();
    with_blocks(&mut h);
    h.state_mut().open_block_editor("DESKS");
    h.run_steps(3);
    hover(&mut h, Point::new(10.0, 4.0));
    save(&mut h, &dir, "32-block-editor");

    // A layout: an A3 sheet with a frame, a title and a viewport onto the
    // plan at 1:50 (paper millimeters, model units of 10 cm).
    let mut h = harness();
    {
        let doc = h.state_mut().document_mut();
        let paper = oxidraft_model::Paper {
            size_name: "ISO_A3_(420.00_x_297.00_MM)".into(),
            width: 420.0,
            height: 297.0,
            margins: [7.5, 20.0, 7.5, 20.0],
            rotation: 0,
            millimeters: true,
            raw: Vec::new(),
        };
        let layout =
            oxidraft_model::layout::Layout { id: oxidraft_model::MODEL, name: "A3 Plan".into(), tab_order: 1, paper };
        let space = doc.add_layout(layout);
        doc.set_space(space);
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Geometry::Polyline {
            vertices: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| Vertex::at(Point::new(x, y))).to_vec(),
            closed: true,
        };
        doc.add(rect(0.0, 0.0, 405.0, 257.0));
        doc.add(rect(285.0, 0.0, 405.0, 40.0));
        doc.add(Geometry::Text {
            look: Default::default(),
            position: Point::new(292.0, 22.0),
            height: 6.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "GROUND FLOOR".into(),
        });
        doc.add(Geometry::Text {
            look: Default::default(),
            position: Point::new(292.0, 9.0),
            height: 4.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "Scale 1:50".into(),
        });
        doc.add(Geometry::Viewport {
            center: Point::new(140.0, 135.0),
            width: 260.0,
            height: 220.0,
            view_center: Point::new(55.0, 32.0),
            view_height: 110.0,
            twist: 0.0,
            frozen: vec!["Hatch".into()],
            locked: false,
            on: true,
            clip: Vec::new(),
        });
        doc.set_space(oxidraft_model::MODEL);
    }
    let space = h.state_mut().document_mut().layouts()[0].id;
    h.state_mut().switch_space(space);
    h.run_steps(4);
    save(&mut h, &dir, "33-layout");
    // A second viewport, a pentagon twisted a quarter turn, over the title block's corner.
    {
        let doc = h.state_mut().document_mut();
        let pentagon: Vec<Point> =
            (0..5).map(|k| Point::new(350.0, 175.0).polar(42.0, 90.0 + 72.0 * f64::from(k))).collect();
        let twisted = Geometry::Viewport {
            center: Point::new(350.0, 175.0),
            width: 84.0,
            height: 84.0,
            view_center: Point::new(30.0, 30.0),
            view_height: 60.0,
            twist: 90.0,
            frozen: vec!["Hatch".into()],
            locked: false,
            on: true,
            clip: Vec::new(),
        }
        .with_clip(pentagon);
        doc.add(twisted);
    }
    h.run_steps(3);
    save(&mut h, &dir, "33b-polygonal-viewport");
    // Working in model space through the viewport (MSPACE), the sheet around it.
    h.state_mut().type_command("ms");
    h.run_steps(3);
    hover(&mut h, Point::new(40.0, 25.0));
    save(&mut h, &dir, "34-viewport");
    // Back on the sheet, the viewport's properties: its scale among them.
    h.state_mut().type_command("ps");
    h.run_steps(3);
    click(&mut h, Point::new(10.0, 135.0));
    h.state_mut().type_command("pr");
    h.run_steps(4);
    save(&mut h, &dir, "35-viewport-properties");
    // The layout's plot preview: its sheet at 1:1, the model cut to the viewport.
    h.state_mut().type_command("preview");
    h.run_steps(4);
    save(&mut h, &dir, "36-layout-preview");
    // The Layout ribbon tab, and the layout tabs' shortcut menu.
    h.key_press(eframe::egui::Key::Escape);
    h.run_steps(3);
    h.get_by_role_and_label(eframe::egui::accesskit::Role::Tab, "A3 Plan").click_secondary();
    h.run_steps(4);
    save(&mut h, &dir, "37-layout-tab-menu");
    h.key_press(eframe::egui::Key::Escape);
    h.run_steps(3);
    h.state_mut().type_command("pagesetup");
    h.run_steps(4);
    save(&mut h, &dir, "38-page-setup");

    // A crossing lasso being dragged round the columns, leftwards.
    let mut h = harness();
    let path = [
        (48.0, 12.0),
        (30.0, 10.0),
        (12.0, 14.0),
        (10.0, 32.0),
        (16.0, 48.0),
        (34.0, 50.0),
        (46.0, 36.0),
        (44.0, 22.0),
    ];
    let at = |h: &Harness<'_, OxidraftApp>, (x, y): (f64, f64)| h.state().screen_pos(Point::new(x, y));
    let start = at(&h, path[0]);
    h.hover_at(start);
    h.run_steps(1);
    h.event(Event::PointerButton {
        pos: start,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    h.run_steps(1);
    for pair in path.windows(2) {
        let (a, b) = (at(&h, pair[0]), at(&h, pair[1]));
        for i in 1..=5 {
            h.hover_at(a + (b - a) * (i as f32 / 5.0));
            h.run_steps(1);
        }
    }
    save(&mut h, &dir, "39-lasso");

    // Selection cycling: a polyline drawn over a wall, clicked where they overlap.
    let mut h = harness();
    h.state_mut().document_mut().add_on_layer(
        Geometry::Polyline {
            vertices: vec![Vertex::at(Point::new(60.0, 0.0)), Vertex::at(Point::new(60.0, 60.0))],
            closed: false,
        },
        "Doors",
        Color::ByLayer,
    );
    h.run_steps(2);
    click(&mut h, Point::new(60.0, 30.0));
    h.run_steps(2);
    hover(&mut h, Point::new(64.0, 27.0));
    save(&mut h, &dir, "40-selection-cycling");

    // The Annotate tab, and a table filled in.
    let mut h = harness();
    let mut table = oxidraft_model::table::new_table(Point::new(110.0, 60.0), 3, 3, 18.0, 6.0, 2.5);
    if let Geometry::Table { cells, .. } = &mut table {
        let rows = [
            ["Door schedule", "", ""],
            ["Mark", "Width", "Height"],
            ["D1", "900", "2100"],
            ["D2", "800", "2100"],
            ["D3", "1200", "2400"],
        ];
        for (r, row) in rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                cells[r][c] = text.to_string();
            }
        }
    }
    h.state_mut().document_mut().add(table);
    h.state_mut().type_command("ze");
    h.run_steps(3);
    h.get_by_role_and_label(eframe::egui::accesskit::Role::Tab, "Annotate").click();
    h.run_steps(3);
    save(&mut h, &dir, "41-annotate-table");

    // Four model viewports, one zoomed in, and the View tab.
    let mut h = harness();
    for input in ["vports", "4", "z", "c", "20,20", "25"] {
        h.state_mut().type_command(input);
        h.run_steps(1);
    }
    h.key_press_modifiers(eframe::egui::Modifiers::CTRL, eframe::egui::Key::R);
    h.run_steps(2);
    h.get_by_role_and_label(eframe::egui::accesskit::Role::Tab, "View").click();
    h.run_steps(3);
    save(&mut h, &dir, "42-model-viewports");

    // DWG Compare: an earlier version with a column moved and a wall missing.
    let mut h = harness();
    let mut earlier = sample();
    let moved: Vec<_> = earlier
        .entities()
        .filter(|e| matches!(&e.geometry, Geometry::Circle { center, .. } if center.x == 80.0 && center.y == 30.0))
        .map(|e| e.id)
        .collect();
    for id in moved {
        earlier.modify(id, Geometry::Circle { center: Point::new(80.0, 15.0), radius: 3.0 });
    }
    let wall: Vec<_> = earlier
        .entities()
        .filter(|e| matches!(&e.geometry, Geometry::Line { start, end } if start.x == 60.0 && end.x == 60.0))
        .map(|e| e.id)
        .collect();
    for id in wall {
        earlier.remove(id);
    }
    let earlier_path = dir.join("earlier.dxf");
    oxidraft_io_dwg::save_dxf(&earlier, &earlier_path).expect("write the earlier drawing");
    // The current one from a file too, as drawings compared are.
    let current_path = dir.join("current.dxf");
    oxidraft_io_dwg::save_dxf(&sample(), &current_path).expect("write the current drawing");
    h.state_mut().open_path(current_path.clone());
    h.run_steps(3);
    h.state_mut().compare_with(&earlier_path);
    h.get_by_role_and_label(eframe::egui::accesskit::Role::Tab, "Collaborate").click();
    h.run_steps(3);
    save(&mut h, &dir, "43-compare");
    std::fs::remove_file(&earlier_path).ok();
    std::fs::remove_file(&current_path).ok();

    // Text styles: a narrow, slanted one in the drawing and its dialog.
    let mut h = harness();
    {
        let doc = h.state_mut().document_mut();
        let narrow = oxidraft_model::TextStyle {
            name: "Narrow".into(),
            font: "romans.shx".into(),
            height: 0.0,
            width_factor: 0.6,
            oblique: 20.0,
        };
        doc.set_text_style(narrow.clone()).unwrap();
        doc.set_current_text_style("Narrow").unwrap();
        doc.add(Geometry::Text {
            position: Point::new(70.0, 4.0),
            height: 3.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "NARROW SLANTED".into(),
            look: oxidraft_model::TextLook::of(&narrow),
        });
    }
    h.state_mut().type_command("style");
    h.run_steps(3);
    save(&mut h, &dir, "44-text-style");

    // A tolerance frame, and dimensions jogged, slanted and with text moved.
    let mut h = harness();
    {
        let doc = h.state_mut().document_mut();
        doc.add(Geometry::Tolerance {
            position: Point::new(5.0, 50.0),
            height: 2.5,
            rotation: 0.0,
            content: "{\\Fgdt;j}%%v{\\Fgdt;n}0.05{\\Fgdt;m}%%vA%%vB%%vC^J{\\Fgdt;c}%%v0.02".into(),
        });
        let dim = |y: f64, extra: oxidraft_model::annotation::DimExtra| Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(60.0, 0.0),
            line: Point::new(30.0, y),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Box::new(extra),
        };
        doc.add(dim(
            -8.0,
            oxidraft_model::annotation::DimExtra { jog: Some(Point::new(15.0, -8.0)), ..Default::default() },
        ));
        doc.add(dim(
            -16.0,
            oxidraft_model::annotation::DimExtra {
                oblique: Some(60.0),
                text_at: Some(Point::new(48.0, -16.0)),
                ..Default::default()
            },
        ));
    }
    h.state_mut().type_command("ze");
    h.run_steps(3);
    save(&mut h, &dir, "45-tolerance-dims");

    // AutoComplete on the command line.
    let mut h = harness();
    h.state_mut().set_command_text("c");
    hover(&mut h, Point::new(70.0, 45.0));
    save(&mut h, &dir, "6-autocomplete");
}

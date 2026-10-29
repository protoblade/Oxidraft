//! Drives the real app through egui pointer events, the way a user would.

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use oxidraft_app::OxidraftApp;
use oxidraft_geom::Point;
use oxidraft_model::{Document, Geometry};

fn app_with_line() -> Harness<'static, OxidraftApp> {
    app_with_line_at(1.0 / 4.0)
}

/// The test app, with `step_dt` seconds between frames.
fn app_with_line_at(step_dt: f32) -> Harness<'static, OxidraftApp> {
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).with_step_dt(step_dt).build_eframe(|cc| {
        let mut app = OxidraftApp::new(cc);
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 0.0) });
        // A second line so zoom extents leaves room around the first one.
        doc.add(Geometry::Line { start: Point::new(0.0, 60.0), end: Point::new(100.0, 60.0) });
        app.set_document(doc);
        app
    });
    h.run_steps(3);
    h
}

/// Turns a layer on, as the Layer Properties Manager does.
fn turn_on(h: &mut Harness<'_, OxidraftApp>, layer: &str) {
    let doc = h.state_mut().document_mut();
    let l = doc.layer(layer).cloned().expect("the layer");
    doc.update_layer(oxidraft_model::Layer { on: true, ..l });
    h.run_steps(2);
}

fn screen(h: &Harness<'_, OxidraftApp>, x: f64, y: f64) -> Pos2 {
    h.state().screen_pos(Point::new(x, y))
}

fn button(h: &mut Harness<'_, OxidraftApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

fn click(h: &mut Harness<'_, OxidraftApp>, pos: Pos2) {
    h.hover_at(pos);
    h.run_steps(1);
    button(h, pos, true);
    button(h, pos, false);
}

fn right_click(h: &mut Harness<'_, OxidraftApp>, pos: Pos2) {
    h.hover_at(pos);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    // The menu opens on the release; its items appear a frame later.
    h.run_steps(2);
}

/// Clicks a menu item by its accessible label (which includes any shortcut
/// text, e.g. "Select All Cmd+A").
fn choose(h: &mut Harness<'_, OxidraftApp>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
}

fn drag(h: &mut Harness<'_, OxidraftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    button(h, from, true);
    for i in 1..=10 {
        h.hover_at(from + (to - from) * (i as f32 / 10.0));
        h.run_steps(1);
    }
    button(h, to, false);
}

fn the_line(h: &mut Harness<'_, OxidraftApp>) -> (Point, Point) {
    match &h.state_mut().document_mut().entities().next().unwrap().geometry {
        Geometry::Line { start, end } => (*start, *end),
        g => panic!("expected a line, got {g:?}"),
    }
}

#[test]
fn dragging_an_end_grip_stretches_the_line() {
    let mut h = app_with_line();
    let on_line = screen(&h, 50.0, 0.0);
    click(&mut h, on_line);
    assert_eq!(h.state_mut().engine_mut().selection().len(), 1);

    // Drag the end grip somewhere empty (well away from any snap point).
    let (grip, target) = (screen(&h, 100.0, 0.0), screen(&h, 73.0, 41.0));
    drag(&mut h, grip, target);

    let (start, end) = the_line(&mut h);
    assert_eq!(start, Point::new(0.0, 0.0));
    let expected = h.state().screen_pos(end);
    assert!(expected.distance(target) < 1.0, "end {end:?} should land under the cursor");
    assert!(!h.state_mut().engine_mut().is_active(), "grip edit ends on release");
}

#[test]
fn dragging_the_midpoint_grip_moves_the_line() {
    let mut h = app_with_line();
    let on_line = screen(&h, 30.0, 0.0);
    click(&mut h, on_line);
    let (grip, target) = (screen(&h, 50.0, 0.0), screen(&h, 50.0, 23.0));
    drag(&mut h, grip, target);
    let (start, end) = the_line(&mut h);
    // Polar tracking locks the drag to 90°, so the line moves straight up.
    assert!((start.x - 0.0).abs() < 1e-6 && (end.x - 100.0).abs() < 1e-6, "{start:?} {end:?}");
    assert!((start.y - end.y).abs() < 1e-6 && start.y > 20.0);
}

#[test]
fn click_click_grip_editing_still_works() {
    let mut h = app_with_line();
    let (on_line, grip) = (screen(&h, 50.0, 0.0), screen(&h, 0.0, 0.0));
    click(&mut h, on_line);
    click(&mut h, grip);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("STRETCH"));
    let target = screen(&h, 20.0, 37.0);
    click(&mut h, target);
    let (start, _) = the_line(&mut h);
    assert!(h.state().screen_pos(start).distance(target) < 1.0);
}

#[test]
fn dragging_on_empty_space_selects_with_a_window() {
    let mut h = app_with_line();
    let (from, to) = (screen(&h, -10.0, 5.0), screen(&h, 110.0, -3.0));
    drag(&mut h, from, to);
    assert_eq!(h.state_mut().engine_mut().selection().len(), 1);
}

#[test]
fn right_click_menu_erases_the_selection() {
    let mut h = app_with_line();
    let on_line = screen(&h, 50.0, 0.0);
    click(&mut h, on_line);
    right_click(&mut h, on_line);
    choose(&mut h, "Erase Del");
    assert_eq!(h.state_mut().document_mut().len(), 1, "only the other line is left");
}

#[test]
fn right_click_menu_selects_all_when_idle() {
    let mut h = app_with_line();
    let empty = screen(&h, 50.0, 30.0);
    right_click(&mut h, empty);
    choose(&mut h, "Select All Cmd+A");
    assert_eq!(h.state_mut().engine_mut().selection().len(), 2);
}

#[test]
fn right_click_menu_enter_ends_a_command() {
    let mut h = app_with_line();
    h.state_mut().type_command("l");
    let (a, b) = (screen(&h, 20.0, 20.0), screen(&h, 40.0, 30.0));
    click(&mut h, a);
    click(&mut h, b);
    right_click(&mut h, b);
    choose(&mut h, "Enter");
    assert!(!h.state_mut().engine_mut().is_active());
    assert_eq!(h.state_mut().document_mut().len(), 3);
}

#[test]
fn snap_override_from_the_menu_picks_the_midpoint() {
    let mut h = app_with_line();
    h.state_mut().type_command("l");
    let start = screen(&h, 20.0, 30.0);
    click(&mut h, start);
    right_click(&mut h, start);
    choose(&mut h, "Snap Overrides");
    choose(&mut h, "Midpoint");
    // Touch the bottom line near its end: the override takes its midpoint.
    let near_end = screen(&h, 95.0, 0.2);
    click(&mut h, near_end);
    // The first point went through screen pixels; the snapped end is exact.
    let doc = h.state_mut().document_mut();
    let end = doc.entities().filter_map(|e| match &e.geometry {
        Geometry::Line { start, end } if start.distance(Point::new(20.0, 30.0)) < 0.5 => Some(*end),
        _ => None,
    });
    let end: Vec<Point> = end.collect();
    assert_eq!(end.len(), 1);
    assert!(end[0].approx_eq(Point::new(50.0, 0.0)), "ended at {:?}", end[0]);
}

#[test]
fn zoom_window_then_previous() {
    let mut h = app_with_line();
    let (before_a, before_b) = (screen(&h, 0.0, 0.0), screen(&h, 100.0, 0.0));
    h.state_mut().type_command("z");
    let (a, b) = (screen(&h, 40.0, 20.0), screen(&h, 60.0, 40.0));
    click(&mut h, a);
    click(&mut h, b);
    h.run_steps(2);
    // The 20-unit square fills the view: at least twice the pixels per unit.
    let per_unit_before = (before_b.x - before_a.x) / 100.0;
    let per_unit = (screen(&h, 60.0, 0.0).x - screen(&h, 40.0, 0.0).x) / 20.0;
    assert!(per_unit > per_unit_before * 2.0, "{per_unit_before} -> {per_unit} px per unit");

    h.state_mut().type_command("z");
    h.state_mut().type_command("p");
    h.run_steps(2);
    assert!(screen(&h, 0.0, 0.0).distance(before_a) < 0.5 && screen(&h, 100.0, 0.0).distance(before_b) < 0.5);
}

#[test]
fn pan_drags_the_view_with_the_left_button_until_escape() {
    let mut h = app_with_line();
    h.state_mut().type_command("p");
    h.run_steps(1);
    let before = screen(&h, 50.0, 30.0);
    drag(&mut h, before, before + vec2(120.0, 0.0));
    h.run_steps(1);
    let after = screen(&h, 50.0, 30.0);
    assert!((after.x - before.x - 120.0).abs() < 1.0 && (after.y - before.y).abs() < 1.0, "{before:?} -> {after:?}");
    assert!(h.state_mut().engine_mut().selection().is_empty(), "panning doesn't select");

    // Esc ends it: a click selects again.
    h.key_press(Key::Escape);
    h.run_steps(1);
    let on_line = screen(&h, 50.0, 0.0);
    click(&mut h, on_line);
    assert_eq!(h.state_mut().engine_mut().selection().len(), 1);
}

#[test]
fn select_similar_from_the_right_click_menu() {
    let mut h = app_with_line();
    let on_line = screen(&h, 50.0, 0.0);
    click(&mut h, on_line);
    assert_eq!(h.state_mut().engine_mut().selection().len(), 1);
    right_click(&mut h, on_line);
    choose(&mut h, "Select Similar");
    assert_eq!(h.state_mut().engine_mut().selection().len(), 2, "the other line on layer 0 too");
}

#[test]
fn quick_select_dialog_selects_by_layer() {
    let mut h = app_with_line();
    h.state_mut().type_command("qselect");
    h.run_steps(2);
    // Defaults: the entire drawing, any type, Layer = 0.
    choose(&mut h, "OK");
    assert_eq!(h.state_mut().engine_mut().selection().len(), 2);
    assert!(h.state_mut().engine_mut().history().iter().any(|l| l == "2 items selected."));
}

fn layer_control(h: &Harness<'_, OxidraftApp>) -> Option<String> {
    h.get_by_label("Layer control").accesskit_node().value()
}

#[test]
fn layer_control_shows_and_changes_the_selected_objects_layer() {
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    doc.add_layer(oxidraft_model::Layer::new("WALLS", 1));
    let first = doc.entities().next().unwrap().id;
    let second = doc.entities().nth(1).unwrap().id;
    doc.update(second, |e| e.layer = "WALLS".into());
    h.run_steps(2);
    assert_eq!(layer_control(&h).as_deref(), Some("0"), "the current layer");

    let (a, b) = (screen(&h, 50.0, 60.0), screen(&h, 50.0, 0.0));
    click(&mut h, a);
    h.run_steps(1);
    assert_eq!(layer_control(&h).as_deref(), Some("WALLS"), "the selected line's layer");
    click(&mut h, b);
    h.run_steps(1);
    assert_eq!(layer_control(&h), Some(String::new()), "blank: the lines are on different layers");

    // Picking a layer moves the selected objects; the current layer stays.
    h.get_by_label("Layer control").click();
    h.run_steps(2);
    choose(&mut h, "WALLS");
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.entity(first).unwrap().layer, "WALLS");
    assert_eq!(doc.current_layer(), "0");
    assert_eq!(h.state_mut().engine_mut().selection().len(), 2, "still selected");

    // With nothing selected it shows the current layer again.
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(layer_control(&h).as_deref(), Some("0"));
}

#[test]
fn zoom_object_uses_the_objects_already_selected() {
    let mut h = app_with_line();
    // Zoom out first, so zooming to the line changes the view.
    h.state_mut().type_command("z");
    h.state_mut().type_command(".25x");
    h.run_steps(2);
    let on_line = screen(&h, 50.0, 0.0);
    click(&mut h, on_line);
    assert_eq!(h.state_mut().engine_mut().selection().len(), 1);
    let before = screen(&h, 100.0, 0.0).x - screen(&h, 0.0, 0.0).x;
    h.state_mut().type_command("z");
    h.state_mut().type_command("o");
    h.run_steps(2);
    let after = screen(&h, 100.0, 0.0).x - screen(&h, 0.0, 0.0).x;
    assert!(after > before * 2.0, "zoomed to the line: {before} -> {after}");
    assert_eq!(h.state_mut().engine_mut().selection().len(), 1, "still selected");
}

/// Types like a person: one key per frame, without waiting for the app to settle.
fn type_keys(h: &mut Harness<'_, OxidraftApp>, text: &str) {
    for c in text.chars() {
        h.event(Event::Text(c.to_string()));
        h.run_steps(1);
    }
}

#[test]
fn no_keystroke_is_lost_while_the_command_line_changes() {
    let mut h = app_with_line();
    // AutoComplete appears with the first letter, then the ZOOM prompt and
    // its options replace it; every key still lands.
    type_keys(&mut h, "zoom o ");
    let history = h.state_mut().engine_mut().history().to_vec();
    assert!(history.iter().any(|l| l == "Command: ZOOM"), "{history:?}");
    assert!(history.last().unwrap().ends_with("[All/Center/Extents/Previous/Scale/Window/Object] <real time>: o"));
    assert_eq!(h.state_mut().engine_mut().current_prompt().unwrap().text, "Select objects");
}

#[test]
fn the_layer_manager_stays_above_the_command_line() {
    let mut h = app_with_line();
    // Enough layers to fill a tall window.
    for i in 0..40 {
        h.state_mut().document_mut().add_layer(oxidraft_model::Layer::new(format!("L{i}"), 7));
    }
    h.state_mut().type_command("la");
    h.run_steps(3);
    let id = eframe::egui::Id::unique(oxidraft_app::LAYER_MANAGER);
    let window = |h: &Harness<'_, OxidraftApp>| h.ctx.memory(|m| m.area_rect(id)).expect("layer manager shown");
    let top = h.state().command_line_top();
    let check = |h: &Harness<'_, OxidraftApp>, what: &str| {
        let w = window(h);
        assert!(w.bottom() <= top, "{what}: window {w:?} reaches below the command line at {top}");
    };
    // Resize it by its corner, then move it by its title bar, to the bottom of the screen.
    let before = window(&h).height();
    let corner = window(&h).right_bottom() - vec2(3.0, 3.0);
    drag(&mut h, corner, Pos2::new(corner.x, 795.0));
    h.run_steps(3);
    check(&h, "resized");
    assert!(window(&h).height() > before + 100.0, "it grew down to the command line");
    let title = window(&h).center_top() + vec2(0.0, 12.0);
    drag(&mut h, title, Pos2::new(title.x, 790.0));
    h.run_steps(3);
    check(&h, "moved");
}

#[test]
fn double_clicking_a_block_edits_its_attributes() {
    use oxidraft_model::{Attach, Attribute, Block, BlockEntity, Color, Placement};
    // 60 frames a second, so two clicks fall within egui's double-click time.
    let mut h = app_with_line_at(1.0 / 60.0);
    let doc = h.state_mut().document_mut();
    let circle = Geometry::Circle { center: Point::new(0.0, 0.0), radius: 3.0 };
    doc.define_block(Block {
        name: "KW".into(),
        base: Point::new(0.0, 0.0),
        entities: vec![BlockEntity::new("0", Color::ByBlock, circle)],
    });
    let number = Attribute {
        tag: "KWNOM".into(),
        value: "9".into(),
        layer: "0".into(),
        color: Color::ByBlock,
        position: Point::new(54.0, 30.0),
        height: 2.0,
        rotation: 0.0,
        attach: Attach::BottomLeft,
        invisible: false,
    };
    let block = doc.add(Geometry::Insert {
        block: "KW".into(),
        placement: Placement::at(Point::new(50.0, 30.0)),
        attributes: vec![number],
        clip: None,
    });
    h.run_steps(2);
    let edge = screen(&h, 53.0, 30.0);
    click(&mut h, edge);
    click(&mut h, edge);
    h.run_steps(2);
    let lines_before = h.state_mut().engine_mut().history().len();

    let field = h.get_by_role_and_label(Role::TextInput, "KWNOM");
    field.focus();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "KWNOM").type_text("12");
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(3);
    let value = match &h.state_mut().document_mut().entity(block).unwrap().geometry {
        Geometry::Insert { attributes, .. } => attributes[0].value.clone(),
        g => panic!("{g:?}"),
    };
    assert_eq!(value, "12");
    assert!(h.query_by_role_and_label(Role::TextInput, "KWNOM").is_none(), "Enter closed the editor");
    let engine = h.state_mut().engine_mut();
    assert!(!engine.is_active() && engine.history().len() == lines_before, "Enter didn't reach the command line");
}

#[test]
fn copy_and_paste_through_the_clipboard() {
    use eframe::egui::OutputCommand;
    let mut h = app_with_line();
    let on_line = screen(&h, 50.0, 0.0);
    click(&mut h, on_line);
    h.event(Event::Copy);
    h.run_steps(1);
    let copied = h.output().platform_output.commands.iter().find_map(|c| match c {
        OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    let text = copied.expect("Cmd+C put the line on the clipboard");
    assert!(text.contains("ENTITIES") && text.contains("LINE"));

    // Cmd+V: the line follows the cursor until a click places it.
    h.event(Event::Paste(text));
    h.run_steps(2);
    let prompt = h.state_mut().engine_mut().current_prompt().map(|p| p.text);
    assert_eq!(prompt.as_deref(), Some("Specify insertion point"));
    let target = screen(&h, 20.0, 30.0);
    click(&mut h, target);
    h.run_steps(1);
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.entities().count(), 3, "the copy, besides the two lines");
    let pasted = doc.entities().last().unwrap().geometry.clone();
    let Geometry::Line { start, end } = pasted else { panic!("{pasted:?}") };
    assert!(start.distance(Point::new(20.0, 30.0)) < 0.5 && (end - start).length() > 99.9, "{start:?} {end:?}");
}

#[test]
fn plot_a_window_to_pdf() {
    let mut h = app_with_line();
    h.state_mut().type_command("plot");
    h.run_steps(3);
    assert_eq!(h.state().plot_open(), (true, false));
    // Picking a window: the dialog steps aside for two clicks, then comes back.
    h.state_mut().plot_settings_mut().unwrap().area =
        oxidraft_plot::PlotArea::Window(oxidraft_geom::Aabb::from_points(Point::ORIGIN, Point::new(1.0, 1.0)));
    h.run_steps(2);
    h.get_by_label("Window<").click();
    h.run_steps(2);
    assert_eq!(h.state().plot_open(), (false, false), "hidden while picking");
    let (a, b) = (screen(&h, 10.0, 10.0), screen(&h, 60.0, 50.0));
    click(&mut h, a);
    click(&mut h, b);
    h.run_steps(2);
    assert_eq!(h.state().plot_open(), (true, false));
    let oxidraft_plot::PlotArea::Window(w) = h.state_mut().plot_settings_mut().unwrap().area else { panic!() };
    assert!((w.min.x - 10.0).abs() < 1.0 && (w.max.y - 50.0).abs() < 1.0, "{w:?}");
    // The preview opens from the dialog and Esc goes back to it.
    h.get_by_label("Preview...").click();
    h.run_steps(2);
    assert_eq!(h.state().plot_open(), (false, true));
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().plot_open(), (true, false));
    let path = std::env::temp_dir().join(format!("oxidraft-plot-test-{}.pdf", std::process::id()));
    h.state_mut().plot_to(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert_eq!(h.state().plot_open(), (false, false));
}

#[test]
fn attach_reopen_and_detach_an_external_reference() {
    use oxidraft_model::{Color, Layer, XrefStatus};
    let dir = std::env::temp_dir().join(format!("oxidraft-xref-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut site = Document::new();
    site.add_layer(Layer::new("Boundary", 1));
    site.add_on_layer(Geometry::Line { start: Point::ORIGIN, end: Point::new(20.0, 0.0) }, "Boundary", Color::ByLayer);
    oxidraft_io_dxf::save_file(&site, dir.join("site.dxf")).unwrap();
    let mut plan = Document::new();
    plan.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(100.0, 60.0) });
    let host = dir.join("plan.dxf");
    oxidraft_io_dxf::save_file(&plan, &host).unwrap();

    let mut h = app_with_line();
    h.state_mut().open_path(host.clone());
    h.run_steps(3);
    h.state_mut().attach_file(dir.join("site.dxf"));
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let at = screen(&h, 50.0, 30.0);
    click(&mut h, at);
    {
        let doc = h.state_mut().document_mut();
        let x = doc.xref("site").expect("attached").clone();
        assert_eq!((x.path.as_str(), x.status), ("site.dxf", XrefStatus::Loaded), "saved relative to the drawing");
        assert_eq!(doc.xref_references("site"), 1);
        assert!(doc.layer("site|Boundary").is_some());
        let reference =
            doc.entities().find(|e| matches!(&e.geometry, Geometry::Insert { block, .. } if block == "site"));
        assert_eq!(reference.map(|e| e.layer.as_str()), Some("site"), "on a layer of its own");
    }
    // Saved as a path; reopening reads the file again.
    h.state_mut().type_command("save");
    h.run_steps(1);
    h.state_mut().open_path(host.clone());
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.xref("site").map(|x| x.status), Some(XrefStatus::Loaded));
    assert_eq!(doc.block("site").map(|b| b.entities.len()), Some(1));
    // The palette lists it; Detach removes it.
    h.state_mut().type_command("xr");
    h.run_steps(2);
    h.get_by_label("   site").click();
    h.run_steps(2);
    h.get_by_label("Detach").click();
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    assert!(doc.xref("site").is_none() && doc.xref_references("site") == 0 && doc.len() == 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn many_georeferenced_tiles_attach_unloaded_and_load_in_view() {
    use oxidraft_model::XrefStatus;
    let dir = std::env::temp_dir().join(format!("oxidraft-tiles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Two 40 x 20 pixel tiles side by side at half a unit a pixel, and one
    // image with nothing to say where it goes.
    for (name, x) in [("tile_1", 0.0), ("tile_2", 20.0)] {
        let png = dir.join(format!("{name}.png"));
        image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 120, 40, 255])).save(&png).unwrap();
        std::fs::write(png.with_extension("pgw"), format!("0.5\n0\n0\n-0.5\n{}\n9.75\n", x + 0.25)).unwrap();
    }
    image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255])).save(dir.join("scan.png")).unwrap();
    let files: Vec<_> = ["tile_1.png", "tile_2.png", "scan.png"].iter().map(|f| dir.join(f)).collect();

    let mut h = app_with_line();
    h.state_mut().attach_images_unloaded(&files);
    h.run_steps(3);
    {
        let doc = h.state_mut().document_mut();
        assert_eq!(doc.image_defs().count(), 2, "the scan has no georeferencing");
        assert!(doc.image_defs().all(|d| d.status == XrefStatus::Unloaded));
        assert!(doc.layer("tile_1").is_some_and(|l| !l.on) && doc.layer("tile_2").is_some_and(|l| !l.on));
        // Each on a layer of its own name, where its world file puts it.
        let tile =
            doc.entities().find(|e| matches!(&e.geometry, Geometry::Image { image, .. } if image == "tile_2")).unwrap();
        assert_eq!(tile.layer, "tile_2");
        let Geometry::Image { origin, u, v, .. } = &tile.geometry else { panic!() };
        assert!(
            origin.approx_eq(Point::new(20.0, 0.0))
                && u.approx_eq(Point::new(20.0, 0.0))
                && v.approx_eq(Point::new(0.0, 10.0))
        );
    }
    assert!(!h.state().pictures_loading(), "nothing read yet");
    // Turning a layer on loads its image; the other stays as it was.
    turn_on(&mut h, "tile_1");
    {
        let doc = h.state_mut().document_mut();
        assert_eq!(doc.image_def("tile_1").map(|d| d.status), Some(XrefStatus::Loaded));
        assert_eq!(doc.image_def("tile_2").map(|d| d.status), Some(XrefStatus::Unloaded));
    }
    turn_on(&mut h, "tile_2");
    for _ in 0..100 {
        h.run_steps(1);
        if !h.state().pictures_loading() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(h.state_mut().document_mut().image_defs().all(|d| d.status == XrefStatus::Loaded));
    // The plot shows them.
    h.state_mut().type_command("plot");
    h.run_steps(2);
    let pdf = dir.join("tiles.pdf");
    h.state_mut().plot_to(&pdf).unwrap();
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/Subtype /Image"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn attach_a_pdf_page_as_an_underlay() {
    let dir = std::env::temp_dir().join(format!("oxidraft-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // An A4 landscape plot, written by Oxidraft itself.
    let mut plan = Document::new();
    plan.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(100.0, 50.0) });
    let sheet = oxidraft_plot::render(&plan, &oxidraft_plot::PlotSettings::default()).unwrap();
    let pdf = dir.join("plan.pdf");
    std::fs::write(&pdf, oxidraft_plot::pdf(&sheet, "plan")).unwrap();

    let mut h = app_with_line();
    h.state_mut().open_image_dialog(pdf);
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let at = screen(&h, 10.0, 10.0);
    click(&mut h, at);
    for _ in 0..100 {
        h.run_steps(1);
        if !h.state().pictures_loading() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let doc = h.state_mut().document_mut();
    let def = doc.image_def("plan - 1").expect("the page is attached").clone();
    assert_eq!((def.page, def.width.round(), def.height.round()), (Some(0), 842.0, 595.0));
    let page = doc.entities().find(|e| matches!(e.geometry, Geometry::Image { .. })).unwrap();
    assert_eq!(page.layer, "plan - 1");
    // At scale 1 a page is its size in millimeters.
    let Geometry::Image { u, v, .. } = &page.geometry else { panic!() };
    assert!((u.length() - 297.0).abs() < 0.1 && (v.length() - 210.0).abs() < 0.1, "{u:?} {v:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn edit_an_xref_in_place_and_save_it_to_its_file() {
    use oxidraft_model::{Color, Layer};
    let dir = std::env::temp_dir().join(format!("oxidraft-refedit-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut site = Document::new();
    site.add_layer(Layer::new("Boundary", 1));
    site.add_on_layer(Geometry::Line { start: Point::ORIGIN, end: Point::new(20.0, 0.0) }, "Boundary", Color::ByLayer);
    oxidraft_io_dxf::save_file(&site, dir.join("site.dxf")).unwrap();
    let mut plan = Document::new();
    plan.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(100.0, 60.0) });
    let host = dir.join("plan.dxf");
    oxidraft_io_dxf::save_file(&plan, &host).unwrap();

    let mut h = app_with_line();
    h.state_mut().open_path(host.clone());
    h.run_steps(3);
    h.state_mut().attach_file(dir.join("site.dxf"));
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let at = screen(&h, 50.0, 30.0);
    click(&mut h, at);
    // REFEDIT on the reference, then a circle drawn into it.
    h.state_mut().type_command("refedit");
    h.run_steps(1);
    let on_line = screen(&h, 60.0, 30.0);
    click(&mut h, on_line);
    assert!(h.state_mut().document_mut().refedit().is_some(), "{:?}", h.state_mut().engine_mut().history());
    for input in ["circle", "55,35", "2"] {
        h.state_mut().type_command(input);
        h.run_steps(1);
    }
    // Saving the drawing waits for the reference edit to end.
    h.state_mut().type_command("save");
    h.run_steps(1);
    assert!(h.state_mut().engine_mut().history().iter().any(|l| l.contains("Save or discard the reference edits")));
    h.get_by_label("Save Changes").click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let history = h.state_mut().engine_mut().history().to_vec();
    let doc = h.state_mut().document_mut();
    assert!(doc.refedit().is_none(), "{history:?}");
    assert_eq!(doc.block("site").map(|b| b.entities.len()), Some(2));
    // Written to the xref's own file, in its coordinates, with a backup.
    let saved = oxidraft_io_dxf::load_file(dir.join("site.dxf")).unwrap().document;
    assert_eq!(saved.len(), 2);
    assert!(
        saved
            .entities()
            .any(|e| matches!(e.geometry, Geometry::Circle { center, .. } if center.approx_eq(Point::new(5.0, 5.0))))
    );
    assert!(saved.entities().any(|e| e.layer == "Boundary"));
    assert!(dir.join("site.bak").is_file());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn zooming_into_a_large_image_reads_the_part_on_screen_sharper() {
    let dir = std::env::temp_dir().join(format!("oxidraft-detail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // 6000 x 4000 pixels at a tenth of a unit each: 600 x 400 units.
    let tif = dir.join("ortho.tif");
    image::RgbImage::from_fn(6000, 4000, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])).save(&tif).unwrap();
    std::fs::write(dir.join("ortho.tfw"), "0.1\n0\n0\n-0.1\n0.05\n399.95\n").unwrap();
    let mut h = app_with_line();
    h.state_mut().attach_images_unloaded(&[tif]);
    h.run_steps(2);
    turn_on(&mut h, "ortho");
    let settle = |h: &mut Harness<'_, OxidraftApp>| {
        for _ in 0..300 {
            h.run_steps(1);
            if !h.state().pictures_loading() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    settle(&mut h);
    // The whole picture in view: its overview is enough.
    assert!(h.state().picture_details().is_empty());
    // Zoomed to a 30-unit window, 300 of its pixels across: read sharper.
    for input in ["z", "100,100", "130,120"] {
        h.state_mut().type_command(input);
        h.run_steps(1);
    }
    settle(&mut h);
    h.run_steps(2);
    let details = h.state().picture_details();
    assert_eq!(details.len(), 1, "{details:?}");
    let (region, scale) = details[0];
    assert!(scale > 0.9, "read at the screen's resolution: {scale}");
    // Around the window: x 1000..1300, rows 2800..3000 from the top.
    assert!(region.x0 <= 1000 && region.x1 >= 1300 && region.y0 <= 2800 && region.y1 >= 3000, "{region:?}");
    assert!(region.width() < 1000, "only around the view: {region:?}");
    // Zoomed back out, it's let go.
    h.state_mut().type_command("ze");
    h.run_steps(3);
    assert!(h.state().picture_details().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn light_theme_and_white_drawing_area() {
    use oxidraft_app::{
        Settings,
        theme::{self, Background, ColorTheme},
    };
    let mut h = app_with_line();
    h.state_mut().type_command("colortheme");
    h.run_steps(1);
    h.state_mut().type_command("1");
    h.run_steps(2);
    assert_eq!(theme::color_theme(), ColorTheme::Light);
    assert_eq!(h.ctx.theme(), eframe::egui::Theme::Light);
    // Color 7 stays white on the dark drawing area, and turns black on a white one.
    assert_eq!(theme::aci(7), eframe::egui::Color32::WHITE);
    let ctx = h.ctx.clone();
    h.state_mut().apply_settings(&ctx, Settings { color_theme: ColorTheme::Light, background: Background::White });
    h.run_steps(1);
    assert_eq!(theme::aci(7), eframe::egui::Color32::BLACK);
    assert_eq!(theme::swatch(7), eframe::egui::Color32::WHITE, "the layer swatch still says white");
    assert_eq!(theme::model_bg(), eframe::egui::Color32::WHITE);
    // OPTIONS shows the choices.
    h.state_mut().type_command("op");
    h.run_steps(2);
    assert!(h.query_by_label("Color theme:").is_some() && h.query_by_label("Drawing background:").is_some());
    h.state_mut().apply_settings(&ctx, Settings::default());
    assert_eq!(theme::color_theme(), ColorTheme::Dark);
}

#[test]
fn turning_on_a_missing_images_layer_lets_you_find_its_file() {
    use oxidraft_model::XrefStatus;
    let dir = std::env::temp_dir().join(format!("oxidraft-moved-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("moved")).unwrap();
    for (name, x) in [("a", 0.0), ("b", 20.0), ("c", 40.0)] {
        let png = dir.join(format!("{name}.png"));
        image::RgbaImage::from_pixel(40, 20, image::Rgba([90, 160, 60, 255])).save(&png).unwrap();
        std::fs::write(png.with_extension("pgw"), format!("0.5\n0\n0\n-0.5\n{}\n9.75\n", x + 0.25)).unwrap();
    }
    let files: Vec<_> = ["a.png", "b.png", "c.png"].iter().map(|f| dir.join(f)).collect();
    let mut h = app_with_line();
    h.state_mut().attach_images_unloaded(&files);
    h.run_steps(2);
    // The tiles move to another folder.
    for f in ["a", "b", "c"] {
        std::fs::rename(dir.join(format!("{f}.png")), dir.join("moved").join(format!("{f}.png"))).unwrap();
    }
    turn_on(&mut h, "b");
    assert_eq!(h.state_mut().document_mut().image_def("b").map(|d| d.status), Some(XrefStatus::NotFound));
    // The palette opens on it, with Browse... to pick where it is now.
    assert!(h.query_by_label("Browse...").is_some(), "the palette offers Browse...");
    h.state_mut().set_image_file("b", &dir.join("moved").join("b.png"));
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    // B loads; A and C, also missing, are found in the same folder.
    assert_eq!(doc.image_def("b").map(|d| d.status), Some(XrefStatus::Loaded));
    for n in ["a", "c"] {
        assert!(doc.image_def(n).is_some_and(|d| d.path.contains("moved")), "{n}");
    }
    assert!(h.state_mut().engine_mut().history().iter().any(|l| l.starts_with("Found 2 more missing image(s)")));
    std::fs::remove_dir_all(&dir).ok();
}

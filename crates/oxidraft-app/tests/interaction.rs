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
    app_sized(step_dt, 1200.0)
}

/// The test app in a window `width` wide.
fn app_sized(step_dt: f32, width: f32) -> Harness<'static, OxidraftApp> {
    let mut h = Harness::builder().with_size(vec2(width, 800.0)).with_step_dt(step_dt).build_eframe(|cc| {
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
fn clicking_two_corners_selects_with_a_window() {
    let mut h = app_with_line();
    let (from, to) = (screen(&h, -10.0, 5.0), screen(&h, 110.0, -3.0));
    click(&mut h, from);
    click(&mut h, to);
    assert_eq!(h.state_mut().engine_mut().selection().len(), 1);
}

/// Presses at the first point, drags through the others, and lets go at the last.
fn drag_through(h: &mut Harness<'_, OxidraftApp>, path: &[(f64, f64)]) {
    let points: Vec<Pos2> = path.iter().map(|&(x, y)| screen(h, x, y)).collect();
    h.hover_at(points[0]);
    h.run_steps(1);
    button(h, points[0], true);
    for pair in points.windows(2) {
        for i in 1..=6 {
            h.hover_at(pair[0] + (pair[1] - pair[0]) * (i as f32 / 6.0));
            h.run_steps(1);
        }
    }
    button(h, *points.last().unwrap(), false);
}

#[test]
fn dragging_on_empty_space_draws_a_lasso() {
    let mut h = app_with_line();
    // Off to the right first: a window lasso, round the lower line only.
    drag_through(&mut h, &[(-10.0, 10.0), (110.0, 10.0), (110.0, -10.0), (-10.0, -10.0), (-10.0, 8.0)]);
    let lower = h.state_mut().engine_mut().selection().to_vec();
    assert_eq!(lower.len(), 1);
    // Off to the left first: a crossing lasso, which only has to cut the upper line.
    h.key_press(Key::Escape);
    h.run_steps(2);
    drag_through(&mut h, &[(70.0, 50.0), (30.0, 50.0), (30.0, 62.0), (70.0, 62.0), (70.0, 52.0)]);
    let upper = h.state_mut().engine_mut().selection().to_vec();
    assert_eq!(upper.len(), 1);
    assert_ne!(upper, lower);
}

#[test]
fn clicking_objects_on_top_of_each_other_lists_them_to_choose_from() {
    let mut h = app_with_line();
    // A polyline right over the lower line.
    let over = h.state_mut().document_mut().add(Geometry::Polyline {
        vertices: vec![
            oxidraft_model::Vertex::at(Point::new(0.0, 0.0)),
            oxidraft_model::Vertex::at(Point::new(100.0, 0.0)),
        ],
        closed: false,
    });
    h.run_steps(2);
    let at = screen(&h, 50.0, 0.0);
    click(&mut h, at);
    h.run_steps(2);
    assert!(h.query_by_label("Selection").is_some(), "the list is up");
    assert!(h.state_mut().engine_mut().selection().is_empty(), "nothing picked yet");
    // The list's Polyline, not the ribbon's: the one below where it was clicked.
    let item = h.get_all_by_label("Polyline").find(|n| n.rect().min.y > at.y).expect("listed");
    item.click();
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().selection(), &[over]);
    assert!(h.query_by_label("Selection").is_none(), "the list closed");
    // With selection cycling off (Ctrl+W), a click picks one straight away.
    h.key_press(Key::Escape);
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::CTRL, Key::W);
    h.run_steps(1);
    click(&mut h, at);
    h.run_steps(2);
    assert!(h.query_by_label("Selection").is_none());
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

    // Esc while a value is being typed closes the editor, as Cancel.
    let field = h.get_by_role_and_label(Role::TextInput, "KWNOM");
    field.focus();
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, "KWNOM").type_text("77");
    h.run_steps(1);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::TextInput, "KWNOM").is_none(), "Esc closes the editor");
    match &h.state_mut().document_mut().entity(block).unwrap().geometry {
        Geometry::Insert { attributes, .. } => assert_eq!(attributes[0].value, "9", "nothing applied"),
        g => panic!("{g:?}"),
    }
    // Half a second on, so the next two clicks are a double-click of their own.
    h.run_steps(30);
    click(&mut h, edge);
    click(&mut h, edge);
    h.run_steps(2);

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
    let sheet = oxidraft_plot::render(&plan, oxidraft_model::MODEL, &oxidraft_plot::PlotSettings::default()).unwrap();
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
    h.state_mut().apply_settings(
        &ctx,
        Settings { color_theme: ColorTheme::Light, background: Background::White, ..Settings::default() },
    );
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

#[test]
fn layer_manager_fits_long_names_and_changes_several_layers_at_once() {
    use oxidraft_model::Layer;
    let long = "CAD_BORDER_Half_Mass_Fence_common_with_the_neighbour";
    let names = [long, "Short", "CarevaLivada_VARBANOVO1_modifiedG"];
    let mut h = app_with_line();
    for n in names {
        h.state_mut().document_mut().add_layer(Layer::new(n, 3));
    }
    h.state_mut().type_command("la");
    h.run_steps(3);
    // The name column is as wide as the longest name: no name runs under
    // the On icon beside it.
    for n in names {
        let name = h.get_by_label(n).rect();
        let bulb = h.get_by_label(&format!("Turn off: {n}")).rect();
        assert!(name.max.x <= bulb.min.x, "{n}: {name:?} runs into {bulb:?}");
    }
    let layer = |h: &mut Harness<'_, OxidraftApp>, n: &str| h.state_mut().document_mut().layer(n).cloned().unwrap();
    // Click the first, Shift-click the third: all three are selected, and
    // turning one off turns them all off, as one undo step.
    h.get_by_label(names[0]).click();
    h.run_steps(1);
    h.get_by_label(names[2]).click_modifiers(eframe::egui::Modifiers::SHIFT);
    h.run_steps(1);
    h.get_by_label(&format!("Turn off: {}", names[1])).click();
    h.run_steps(2);
    assert!(names.iter().all(|n| !layer(&mut h, n).on));
    assert!(layer(&mut h, "0").on, "not selected");
    h.state_mut().type_command("u");
    h.run_steps(2);
    assert!(names.iter().all(|n| layer(&mut h, n).on), "undone in one step");
    // Cmd-click takes one out; freezing then leaves it alone.
    h.get_by_label(names[1]).click_modifiers(eframe::egui::Modifiers::COMMAND);
    h.run_steps(1);
    h.get_by_label(&format!("Freeze: {}", names[0])).click();
    h.run_steps(2);
    assert!(layer(&mut h, names[0]).frozen && layer(&mut h, names[2]).frozen && !layer(&mut h, names[1]).frozen);
    // A click on a row not selected changes it alone (the ribbon's layer
    // control has a Lock for layer 0 too; the manager's comes after it).
    h.get_all_by_label("Lock: 0").last().unwrap().click();
    h.run_steps(2);
    assert!(layer(&mut h, "0").locked && !layer(&mut h, names[0]).locked);
}

#[test]
fn properties_palette_lines_up_and_docks_at_either_side_or_floats() {
    let mut h = app_with_line();
    let wall = screen(&h, 50.0, 0.0);
    click(&mut h, wall);
    h.state_mut().type_command("pr");
    h.run_steps(3);
    // Its dropdowns and fields are one width, edge to edge.
    let right = |h: &Harness<'_, OxidraftApp>, label: &str| h.get_by_role_and_label(Role::ComboBox, label).rect().max.x;
    let edge = right(&h, "Color");
    for label in ["Linetype", "Lineweight"] {
        assert!((right(&h, label) - edge).abs() < 1.0, "{label} ends at {} not {edge}", right(&h, label));
    }
    let title = |h: &Harness<'_, OxidraftApp>| h.get_by_label("PROPERTIES").rect();
    assert!(title(&h).min.x < 100.0, "docked at the left: {:?}", title(&h));
    // The Dock menu moves it to the right.
    h.get_by_label("Dock").click();
    h.run_steps(2);
    h.get_by_label("Dock Right").click();
    h.run_steps(3);
    assert!(title(&h).min.x > 800.0, "docked at the right: {:?}", title(&h));
    // Dragged by its title into the drawing, it floats there...
    let from = title(&h).center();
    drag(&mut h, from, Pos2::new(600.0, 400.0));
    h.run_steps(3);
    let t = title(&h);
    assert!(t.min.x > 300.0 && t.max.x < 1000.0, "floating: {t:?}");
    // ...and dropped at the left side, it docks there.
    drag(&mut h, t.center(), Pos2::new(5.0, 400.0));
    h.run_steps(3);
    assert!(title(&h).min.x < 100.0, "docked at the left again: {:?}", title(&h));
}

#[test]
fn escape_closes_the_properties_palette_once_no_command_runs() {
    let mut h = app_with_line();
    let wall = screen(&h, 50.0, 0.0);
    click(&mut h, wall);
    h.state_mut().type_command("pr");
    h.run_steps(3);
    let open = |h: &Harness<'_, OxidraftApp>| h.query_by_label("PROPERTIES").is_some();
    assert!(open(&h));
    // Esc in a field leaves the value as it was, and the palette open.
    let line = |h: &mut Harness<'_, OxidraftApp>| the_line(h);
    let before = line(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Start X").focus();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Start X").type_text("999");
    h.run_steps(1);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(line(&mut h), before);
    assert!(open(&h), "the first Esc only leaves the field");
    // A command running: Esc ends it, and the palette stays.
    h.state_mut().type_command("l");
    h.run_steps(1);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state_mut().engine_mut().is_active() && open(&h));
    // Nothing running: Esc clears the selection and closes the palette.
    click(&mut h, wall);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!open(&h) && h.state_mut().engine_mut().selection().is_empty());
}

/// A drawing with block B (a 10-unit line from its base point) placed at
/// (100, 0) scaled 2, beside the test lines.
fn app_with_block() -> (Harness<'static, OxidraftApp>, oxidraft_model::EntityId) {
    use oxidraft_model::{Block, BlockEntity, Color, Placement};
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    let line = Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) };
    doc.define_block(Block {
        name: "B".into(),
        base: Point::ORIGIN,
        entities: vec![BlockEntity::new("0", Color::ByLayer, line)],
    });
    let placement = Placement { position: Point::new(100.0, 30.0), scale: 2.0, rotation: 0.0, mirror: false };
    let reference = doc.add(Geometry::Insert { block: "B".into(), placement, attributes: Vec::new(), clip: None });
    h.run_steps(2);
    (h, reference)
}

#[test]
fn block_editor_edits_a_block_and_asks_before_closing_with_changes() {
    let (mut h, reference) = app_with_block();
    h.state_mut().type_command("be");
    h.run_steps(2);
    assert!(h.query_by_label("Edit Block Definition").is_some());
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert!(h.state().in_block_editor());
    // Its base point is at the origin; draw a line up from it.
    for input in ["l", "0,0", "0,5", ""] {
        h.state_mut().type_command(input);
        h.run_steps(1);
    }
    // SAVE waits for the editor to close.
    h.state_mut().type_command("save");
    h.run_steps(1);
    assert!(h.state_mut().engine_mut().history().iter().any(|l| l.contains("Close the Block Editor first")));
    h.get_by_label("Close Block Editor").click();
    h.run_steps(2);
    h.get_by_label("Save the changes to B").click();
    h.run_steps(3);
    assert!(!h.state().in_block_editor());
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.block("B").map(|b| b.entities.len()), Some(2));
    assert!(doc.entity(reference).is_some());
    assert_eq!(doc.entities().count(), 3, "the two test lines and the reference, nothing left over");
}

#[test]
fn blocks_palette_inserts_from_the_drawing_and_from_a_library() {
    use oxidraft_model::{Block, BlockEntity, Color, Layer};
    let (mut h, _) = app_with_block();
    h.state_mut().type_command("blockspalette");
    h.run_steps(3);
    // This drawing's block: click it, then place it.
    h.get_by_label("B").click();
    h.run_steps(2);
    assert!(h.state_mut().engine_mut().is_active(), "INSERT asks where");
    let at = screen(&h, 40.0, 40.0);
    click(&mut h, at);
    h.run_steps(1);
    let count = |h: &mut Harness<'_, OxidraftApp>, name: &str| {
        h.state_mut()
            .document_mut()
            .entities()
            .filter(|e| matches!(&e.geometry, Geometry::Insert { block, .. } if block == name))
            .count()
    };
    assert_eq!(count(&mut h, "B"), 2);

    // A library drawing in a folder, with a TREE block on its own layer.
    let dir = std::env::temp_dir().join(format!("oxidraft-blocklib-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut lib = Document::new();
    lib.add_layer(Layer::new("Trees", 3));
    let circle = Geometry::Circle { center: Point::ORIGIN, radius: 2.0 };
    lib.define_block(Block {
        name: "TREE".into(),
        base: Point::ORIGIN,
        entities: vec![BlockEntity::new("Trees", Color::ByLayer, circle)],
    });
    lib.add(Geometry::Line { start: Point::ORIGIN, end: Point::new(5.0, 5.0) });
    let file = dir.join("landscape.dxf");
    oxidraft_io_dxf::save_file(&lib, &file).unwrap();
    let ctx = h.ctx.clone();
    h.state_mut().open_library_drawing(dir.clone(), file, &ctx);
    h.run_steps(3);
    assert!(h.query_by_label("landscape (whole drawing)").is_some());
    h.get_by_label("TREE").click();
    h.run_steps(2);
    assert!(h.state_mut().engine_mut().is_active(), "{:?}", h.state_mut().engine_mut().history());
    // Beside the palette, which covers the middle of the drawing.
    let at = screen(&h, 125.0, 50.0);
    click(&mut h, at);
    h.run_steps(1);
    assert_eq!(count(&mut h, "TREE"), 1);
    assert!(h.state_mut().document_mut().layer("Trees").is_some(), "its layer came along");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn layout_tabs_switch_spaces_and_drawing_goes_into_the_layout() {
    let mut h = app_with_line();
    // A drawing without layouts offers Layout1, made when it's opened.
    h.get_by_role_and_label(Role::Tab, "Layout1").click();
    h.run_steps(3);
    let layout = {
        let doc = h.state_mut().document_mut();
        assert_eq!(doc.layouts().len(), 1);
        assert_eq!(doc.space(), doc.layouts()[0].id);
        assert!(doc.is_empty(), "the layout has nothing yet; model space is apart");
        doc.space()
    };
    // What's drawn now is on the sheet, not in model space.
    for input in ["l", "10,10", "100,10", ""] {
        h.state_mut().type_command(input);
        h.run_steps(1);
    }
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.len(), 1);
    assert!(doc.entities().all(|e| e.space == layout));
    h.get_by_role_and_label(Role::Tab, "Model").click();
    h.run_steps(3);
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.space(), oxidraft_model::MODEL);
    assert_eq!(doc.len(), 2, "the two test lines");
}

fn the_viewport(h: &mut Harness<'_, OxidraftApp>) -> (oxidraft_model::EntityId, Point, f64) {
    let doc = h.state_mut().document_mut();
    doc.all_entities()
        .find_map(|e| match e.geometry {
            Geometry::Viewport { view_center, view_height, .. } => Some((e.id, view_center, view_height)),
            _ => None,
        })
        .expect("a viewport")
}

fn type_all(h: &mut Harness<'_, OxidraftApp>, inputs: &[&str]) {
    for input in inputs {
        h.state_mut().type_command(input);
        h.run_steps(1);
    }
}

#[test]
fn working_in_a_viewport_and_back_on_the_sheet() {
    // 60 frames a second, so two clicks fall within egui's double-click time.
    let mut h = app_with_line_at(1.0 / 60.0);
    h.get_by_role_and_label(Role::Tab, "Layout1").click();
    h.run_steps(30);
    type_all(&mut h, &["mv", "f"]);
    let layout = h.state_mut().document_mut().space();
    let (id, center, height) = the_viewport(&mut h);
    // The A4 sheet's printable area is 282 x 170; the viewport fills it.
    let (inside, outside) = (screen(&h, 100.0, 85.0), screen(&h, -3.0, -10.0));

    // Double-clicking inside it: model space, through the viewport.
    click(&mut h, inside);
    click(&mut h, inside);
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().space(), oxidraft_model::MODEL);
    let tab = h.get_by_role_and_label(Role::Tab, "Layout1").accesskit_node().toggled();
    assert_eq!(tab, Some(eframe::egui::accesskit::Toggled::True), "still on the layout's tab");
    type_all(&mut h, &["l", "20,30", "80,30", "", "z", "2x"]);
    {
        let doc = h.state_mut().document_mut();
        assert_eq!(doc.len(), 3, "the new line is in model space");
    }

    // Double-clicking the sheet outside it: back on paper; the viewport
    // keeps the view it was zoomed to.
    h.run_steps(30);
    click(&mut h, outside);
    click(&mut h, outside);
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().space(), layout);
    let (_, now_center, now_height) = the_viewport(&mut h);
    assert!((now_height - height / 2.0).abs() < 1e-6 * height, "{now_height} vs {height}");
    assert!(now_center.distance(center) < 1e-6 * height);

    // VPMAX fills the drawing area with it; VPMIN goes back.
    type_all(&mut h, &["vpmax"]);
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().space(), oxidraft_model::MODEL);
    type_all(&mut h, &["vpmin"]);
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().space(), layout);
    let (_, after_max, height_after_max) = the_viewport(&mut h);
    assert!(after_max.distance(now_center) < 1e-6 * height && (height_after_max - now_height).abs() < 1e-6 * height);

    // A locked viewport's view stays as it was.
    h.state_mut().document_mut().update(id, |e| {
        if let Geometry::Viewport { locked, .. } = &mut e.geometry {
            *locked = true;
        }
    });
    type_all(&mut h, &["ms", "z", "2x", "ps"]);
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().space(), layout);
    assert_eq!(the_viewport(&mut h).2, now_height);

    // MVIEW from inside a viewport makes the next one on the sheet.
    type_all(&mut h, &["ms", "mv", "f"]);
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.space(), layout);
    assert_eq!(doc.entities().filter(|e| matches!(e.geometry, Geometry::Viewport { .. })).count(), 2);
}

#[test]
fn plotting_a_layout_plots_its_sheet_with_model_space_through_the_viewport() {
    let mut h = app_with_line();
    h.get_by_role_and_label(Role::Tab, "Layout1").click();
    h.run_steps(3);
    type_all(&mut h, &["mv", "f"]);
    // Even from inside the viewport, PLOT plots the layout.
    type_all(&mut h, &["ms", "plot"]);
    h.run_steps(2);
    assert!(h.query_by_label("Plot - Layout1").is_some(), "the dialog names the layout");
    assert_eq!(h.state_mut().plot_settings_mut().unwrap().area, oxidraft_plot::PlotArea::Layout);
    let path = std::env::temp_dir().join(format!("oxidraft-layout-plot-{}.pdf", std::process::id()));
    h.state_mut().plot_to(&path).unwrap();
    let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    std::fs::remove_file(&path).ok();
    // Layout1 is A4 landscape; the model is cut to the viewport's frame.
    assert!(text.contains("/MediaBox [0 0 841.8898 595.2756]"), "A4 landscape");
    let history = h.state_mut().engine_mut().history().join("\n");
    assert!(history.contains("Effective plotting area: 282.00 wide by 170.00 high"), "{history}");
}

fn layout_names(h: &mut Harness<'_, OxidraftApp>) -> Vec<String> {
    h.state_mut().document_mut().layouts().iter().map(|l| l.name.clone()).collect()
}

/// Right-clicks a layout tab and picks an item of its menu.
fn tab_menu(h: &mut Harness<'_, OxidraftApp>, tab: &str, item: &str) {
    h.get_by_role_and_label(Role::Tab, tab).click_secondary();
    h.run_steps(3);
    h.get_by_label(item).click();
    h.run_steps(3);
}

#[test]
fn layout_tabs_menu_makes_renames_copies_and_deletes_layouts() {
    let mut h = app_with_line_at(1.0 / 60.0);
    h.get_by_role_and_label(Role::Tab, "Layout1").click();
    h.run_steps(3);
    tab_menu(&mut h, "Layout1", "New Layout");
    assert_eq!(layout_names(&mut h), ["Layout1", "Layout2"]);

    // Rename puts a field in the tab's place.
    tab_menu(&mut h, "Layout2", "Rename");
    let field = h.get_by_role_and_label(Role::TextInput, "Layout name");
    field.focus();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Layout name").type_text("Site plan");
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(layout_names(&mut h), ["Layout1", "Site plan"]);

    tab_menu(&mut h, "Layout1", "Copy");
    assert_eq!(layout_names(&mut h), ["Layout1", "Layout1 (2)", "Site plan"]);

    // Delete asks first.
    tab_menu(&mut h, "Layout1", "Delete");
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert_eq!(layout_names(&mut h), ["Layout1 (2)", "Site plan"]);
    assert_eq!(h.state_mut().document_mut().space(), oxidraft_model::MODEL, "it was the current one");
    let tab = h.get_by_role_and_label(Role::Tab, "Model").accesskit_node().toggled();
    assert_eq!(tab, Some(eframe::egui::accesskit::Toggled::True));
}

#[test]
fn layout_set_switches_the_view_and_the_ribbon_shows_the_layout_tab() {
    let mut h = app_with_line();
    type_all(&mut h, &["layout", "n", "Plan", "lo", "s", "plan"]);
    h.run_steps(3);
    let space = h.state_mut().document_mut().space();
    assert_eq!(h.state_mut().document_mut().layout(space).unwrap().name, "Plan");
    let tab = h.get_by_role_and_label(Role::Tab, "Plan").accesskit_node().toggled();
    assert_eq!(tab, Some(eframe::egui::accesskit::Toggled::True));
    // Its Layout ribbon tab came up, with the viewport tools.
    assert!(h.query_by_label("Rectangular").is_some());
    h.get_by_label("Rectangular").click();
    h.run_steps(2);
    type_all(&mut h, &["f"]);
    let doc = h.state_mut().document_mut();
    let viewport = doc.entities().find(|e| matches!(e.geometry, Geometry::Viewport { .. })).map(|e| e.id).unwrap();
    // Lock on the viewport selected first, from the ribbon.
    h.state_mut().engine_mut().set_selection(vec![viewport]);
    h.get_by_label("Lock").click();
    h.run_steps(2);
    let locked = matches!(
        h.state_mut().document_mut().entity(viewport).unwrap().geometry,
        Geometry::Viewport { locked: true, .. }
    );
    assert!(locked);
    // Back on Model the ribbon has no Layout tab.
    h.get_by_role_and_label(Role::Tab, "Model").click();
    h.run_steps(3);
    assert!(h.query_by_label("Rectangular").is_none());
}

#[test]
fn page_setup_changes_a_layouts_paper() {
    let mut h = app_with_line();
    h.get_by_role_and_label(Role::Tab, "Layout1").click();
    h.run_steps(3);
    type_all(&mut h, &["pagesetup"]);
    h.run_steps(2);
    assert!(h.query_by_label("Page Setup - Layout1").is_some());
    h.get_by_role(Role::ComboBox).click();
    h.run_steps(2);
    h.get_by_label("ISO A3 (297.00 x 420.00 MM)").click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    let paper = doc.layout(doc.space()).unwrap().paper.clone();
    assert_eq!((paper.width, paper.height, paper.rotation), (420.0, 297.0, 0), "A3, landscape as it was");
    assert_eq!(paper.size_name, "ISO_A3_(420.00_x_297.00_MM)");
    h.state_mut().document_mut().undo();
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.layout(doc.space()).unwrap().paper.width, 297.0);
}

#[test]
fn snap_mode_puts_clicked_points_on_the_snap_spacing() {
    let mut h = app_with_line();
    // SNAP with a spacing turns it on; F9 turns it off and on.
    type_all(&mut h, &["snap", "5"]);
    h.key_press(Key::F9);
    h.run_steps(1);
    assert!(!h.state_mut().engine_mut().vars.snap);
    h.key_press(Key::F9);
    h.run_steps(1);
    type_all(&mut h, &["l"]);
    let (a, b) = (screen(&h, 12.3, 31.6), screen(&h, 27.9, 31.2));
    click(&mut h, a);
    click(&mut h, b);
    type_all(&mut h, &[""]);
    let doc = h.state_mut().document_mut();
    let drawn = doc.entities().filter_map(|e| match e.geometry {
        Geometry::Line { start, end } if start.y > 1.0 && start.y < 59.0 => Some((start, end)),
        _ => None,
    });
    assert_eq!(drawn.collect::<Vec<_>>(), [(Point::new(10.0, 30.0), Point::new(30.0, 30.0))]);
}

#[test]
fn page_setup_on_the_model_tab_keeps_the_plot_settings() {
    let mut h = app_with_line();
    type_all(&mut h, &["pagesetup"]);
    h.run_steps(2);
    assert!(h.query_by_label("Page Setup - Model").is_some());
    h.state_mut().plot_settings_mut().unwrap().landscape = false;
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.query_by_label("Page Setup - Model").is_none(), "closed without plotting");
    // The next plot starts from it.
    type_all(&mut h, &["plot"]);
    h.run_steps(1);
    assert!(!h.state_mut().plot_settings_mut().unwrap().landscape);
}

#[test]
fn the_selection_list_picks_for_one_object_prompts_too() {
    let mut h = app_with_line();
    let over = h.state_mut().document_mut().add(Geometry::Polyline {
        vertices: vec![
            oxidraft_model::Vertex::at(Point::new(0.0, 0.0)),
            oxidraft_model::Vertex::at(Point::new(100.0, 0.0)),
        ],
        closed: false,
    });
    h.run_steps(2);
    // OFFSET by 5 the polyline, not the line beneath it.
    type_all(&mut h, &["o", "5"]);
    let at = screen(&h, 50.0, 0.0);
    click(&mut h, at);
    h.run_steps(2);
    let item = h.get_all_by_label("Polyline").find(|n| n.rect().min.y > at.y).expect("listed");
    item.click();
    h.run_steps(2);
    let side = screen(&h, 50.0, 10.0);
    click(&mut h, side);
    type_all(&mut h, &[""]);
    let doc = h.state_mut().document_mut();
    let offset: Vec<_> =
        doc.entities().filter(|e| e.id != over && matches!(e.geometry, Geometry::Polyline { .. })).collect();
    assert_eq!(offset.len(), 1, "the polyline was offset");
    assert_eq!(doc.entities().filter(|e| matches!(e.geometry, Geometry::Line { .. })).count(), 2, "the lines weren't");
}

#[test]
fn space_turns_a_lasso_into_a_fence() {
    let mut h = app_with_line();
    let points: Vec<Pos2> = [(50.0, 50.0), (60.0, 50.0), (60.0, 62.0)].iter().map(|&(x, y)| screen(&h, x, y)).collect();
    h.hover_at(points[0]);
    h.run_steps(1);
    button(&mut h, points[0], true);
    for i in 1..=6 {
        h.hover_at(points[0] + (points[1] - points[0]) * (i as f32 / 6.0));
        h.run_steps(1);
    }
    // Window (it went right), then Crossing, then Fence.
    for _ in 0..2 {
        h.key_press(Key::Space);
        h.run_steps(1);
    }
    for i in 1..=6 {
        h.hover_at(points[1] + (points[2] - points[1]) * (i as f32 / 6.0));
        h.run_steps(1);
    }
    button(&mut h, points[2], false);
    // The fence crossed the upper line; Space didn't reach the command line.
    assert_eq!(h.state_mut().engine_mut().selection().len(), 1);
    assert!(!h.state_mut().engine_mut().is_active());
}

#[test]
fn move_or_copy_reorders_layout_tabs() {
    let mut h = app_with_line();
    type_all(&mut h, &["layout", "n", "First", "layout", "n", "Second"]);
    h.run_steps(2);
    tab_menu(&mut h, "Second", "Move or Copy...");
    // The dialog's First, not the tab.
    h.get_all_by_label("First").find(|n| n.accesskit_node().role() != Role::Tab).unwrap().click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(layout_names(&mut h), ["Second", "First"]);
    // A copy, to the end.
    tab_menu(&mut h, "Second", "Move or Copy...");
    h.get_by_label("(move to end)").click();
    h.get_by_label("Create a copy").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(layout_names(&mut h), ["Second", "First", "Second (2)"]);
}

#[test]
fn layouts_come_in_from_a_template_with_what_is_on_them() {
    let dir = std::env::temp_dir().join(format!("oxidraft-template-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut template = Document::new();
    template.add_layer(oxidraft_model::Layer::new("Border", 5));
    let paper = oxidraft_model::Paper::a4_landscape();
    let sheet =
        template.add_layout(oxidraft_model::layout::Layout { id: 0, name: "A4 Title".into(), tab_order: 1, paper });
    template.set_space(sheet);
    template.add_on_layer(
        Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(280.0, 0.0) },
        "Border",
        oxidraft_model::Color::ByLayer,
    );
    template.set_space(oxidraft_model::MODEL);
    let path = dir.join("title.dxf");
    oxidraft_io_dwg::save_dxf(&template, &path).unwrap();

    let mut h = app_with_line();
    h.state_mut().open_template(&path);
    h.run_steps(2);
    assert!(h.query_by_label("Insert Layout(s)").is_some());
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(layout_names(&mut h), ["A4 Title"]);
    let doc = h.state_mut().document_mut();
    let space = doc.layouts()[0].id;
    let on_it: Vec<_> = doc.all_entities().filter(|e| e.space == space).collect();
    assert_eq!(on_it.len(), 1);
    assert_eq!(on_it[0].layer, "Border");
    assert_eq!(doc.layer("Border").map(|l| l.color), Some(5), "its layer came too");
    assert_eq!(doc.space(), oxidraft_model::MODEL, "still in model space");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn table_is_inserted_then_filled_in_and_edited_again() {
    let mut h = app_with_line_at(1.0 / 60.0);
    type_all(&mut h, &["table"]);
    h.run_steps(2);
    assert!(h.query_by_label("Insert Table").is_some());
    h.get_by_label("OK").click();
    h.run_steps(2);
    let at = screen(&h, 20.0, 50.0);
    click(&mut h, at);
    h.run_steps(3);
    // It opens to be filled in.
    assert!(h.query_by_label("Edit Table").is_some());
    let title = h.get_by_role_and_label(Role::TextInput, "Title");
    title.focus();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Title").type_text("Doors");
    h.get_by_role_and_label(Role::TextInput, "A2").focus();
    h.get_by_role_and_label(Role::TextInput, "A2").type_text("Mark");
    h.run_steps(1);
    let add_row = h.get_by_label("Add Row").rect().center();
    click(&mut h, add_row);
    h.run_steps(1);
    assert!(h.query_by_label("Row 2").is_some());
    h.get_by_label("OK").click();
    h.run_steps(2);
    let table = {
        let doc = h.state_mut().document_mut();
        let e = doc.entities().find(|e| matches!(e.geometry, Geometry::Table { .. })).expect("a table");
        let Geometry::Table { position, cells, widths, heights, .. } = &e.geometry else { unreachable!() };
        assert_eq!((widths.len(), heights.len()), (5, 4), "five columns; title, header, two data rows");
        assert_eq!((cells[0][0].as_str(), cells[1][0].as_str()), ("Doors", "Mark"));
        assert!(position.distance(Point::new(20.0, 50.0)) < 1.0);
        e.id
    };
    // Double-clicking it opens it again.
    h.run_steps(30);
    let inside = screen(&h, 21.0, 45.0);
    click(&mut h, inside);
    click(&mut h, inside);
    h.run_steps(2);
    assert!(h.query_by_label("Edit Table").is_some());
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.query_by_label("Edit Table").is_none());
    assert!(h.state_mut().document_mut().entity(table).is_some());
}

#[test]
fn model_viewports_split_the_drawing_area_and_each_keeps_its_view() {
    let mut h = app_with_line();
    let whole = h.state().screen_pos(Point::new(50.0, 30.0));
    type_all(&mut h, &["vports", "2", "v"]);
    h.run_steps(2);
    // The left pane is current: the same point now shows further left.
    let left = h.state().screen_pos(Point::new(50.0, 30.0));
    assert!(left.x < whole.x - 100.0, "{left:?} vs {whole:?}");
    // Zooming changes the current pane only; clicking the other makes it current.
    type_all(&mut h, &["z", "2x"]);
    let zoomed = h.state().screen_pos(Point::new(60.0, 30.0)) - h.state().screen_pos(Point::new(50.0, 30.0));
    let other = Pos2::new(1100.0, 400.0);
    click(&mut h, other);
    h.run_steps(2);
    let there = h.state().screen_pos(Point::new(60.0, 30.0)) - h.state().screen_pos(Point::new(50.0, 30.0));
    assert!((zoomed.x - 2.0 * there.x).abs() < 1.0, "the right pane kept its own scale: {zoomed:?} vs {there:?}");
    assert!(h.state().screen_pos(Point::new(50.0, 30.0)).x > 600.0, "now the right one is current");
    // Ctrl+R goes back; SIngle joins them.
    h.key_press_modifiers(Modifiers::CTRL, Key::R);
    h.run_steps(1);
    assert!(h.state().screen_pos(Point::new(50.0, 30.0)).x < 600.0);
    type_all(&mut h, &["vports", "si"]);
    h.run_steps(2);
    assert!(h.state().screen_pos(Point::new(50.0, 30.0)).x > 400.0);
}

#[test]
fn named_views_are_saved_and_restored() {
    let mut h = app_with_line();
    type_all(&mut h, &["-view", "s", "Whole"]);
    let whole = h.state().screen_pos(Point::new(50.0, 30.0));
    type_all(&mut h, &["z", "c", "0,0", "10"]);
    h.run_steps(1);
    assert!(h.state().screen_pos(Point::new(50.0, 30.0)).distance(whole) > 100.0);
    // The View Manager restores it.
    type_all(&mut h, &["view"]);
    h.run_steps(2);
    assert!(h.query_by_label("View Manager").is_some());
    let row = h.get_all_by_label_contains("Whole").next().expect("listed");
    row.click();
    h.run_steps(1);
    h.get_by_label("Set Current").click();
    h.run_steps(3);
    assert!(h.state().screen_pos(Point::new(50.0, 30.0)).distance(whole) < 1.0);
    // -VIEW Delete.
    type_all(&mut h, &["-view", "d", "whole"]);
    assert!(h.state_mut().document_mut().views().is_empty());
}

#[test]
fn ucs_icon_navigation_bar_and_file_tabs_can_be_hidden() {
    let mut h = app_with_line();
    type_all(&mut h, &["ucsicon", "off", "navbar", "off", "filetabclose"]);
    h.run_steps(2);
    let vars = &h.state_mut().engine_mut().vars;
    assert!(!vars.ucs_icon && !vars.nav_bar);
    assert!(h.query_by_label("Pan (P)").is_none(), "no navigation bar");
    type_all(&mut h, &["filetab", "navbar", "on"]);
    h.run_steps(2);
    assert!(h.state_mut().engine_mut().vars.nav_bar);
}

#[test]
fn purge_dialog_takes_out_unused_layers() {
    let mut h = app_with_line();
    h.state_mut().document_mut().add_layer(oxidraft_model::Layer::new("Idle", 3));
    h.run_steps(1);
    type_all(&mut h, &["purge"]);
    h.run_steps(2);
    assert!(h.query_by_label("Idle").is_some(), "listed");
    h.get_by_label("Purge All").click();
    h.run_steps(2);
    assert!(h.state_mut().document_mut().layer("Idle").is_none());
    assert!(h.query_by_label("Nothing unused to purge.").is_some());
}

#[test]
fn edit_aliases_adds_a_shortcut_of_your_own() {
    let mut h = app_with_line();
    type_all(&mut h, &["aliasedit"]);
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Alias").focus();
    h.get_by_role_and_label(Role::TextInput, "Alias").type_text("qq");
    h.get_by_role_and_label(Role::TextInput, "Command").focus();
    h.get_by_role_and_label(Role::TextInput, "Command").type_text("circle");
    h.run_steps(1);
    let add = h.get_by_label("Add").rect().center();
    click(&mut h, add);
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    type_all(&mut h, &["qq"]);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("CIRCLE"));
}

#[test]
fn compare_shows_what_is_only_here_only_there_or_in_both() {
    let dir = std::env::temp_dir().join(format!("oxidraft-compare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The other drawing: the first test line, and a circle.
    let mut other = Document::new();
    other.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 0.0) });
    other.add(Geometry::Circle { center: Point::new(50.0, 30.0), radius: 5.0 });
    let path = dir.join("other.dxf");
    oxidraft_io_dwg::save_dxf(&other, &path).unwrap();

    let mut h = app_with_line();
    h.state_mut().compare_with(&path);
    h.run_steps(2);
    let history = h.state_mut().engine_mut().history().join("\n");
    assert!(history.contains("Compared with other.dxf: 1 only here, 1 only there, 1 the same."), "{history}");
    assert!(h.query_by_label("Exit Compare").is_some());
    // Drawing the circle here too: now it's in both.
    h.state_mut().document_mut().add(Geometry::Circle { center: Point::new(50.0, 30.0), radius: 5.0 });
    h.run_steps(2);
    assert!(h.query_by_label("0 only there").is_some());
    h.get_all_by_label("Exit Compare").next().unwrap().click();
    h.run_steps(2);
    assert!(h.query_by_label("0 only there").is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn count_selects_a_blocks_references() {
    use oxidraft_model::{Block, Placement};
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    doc.define_block(Block { name: "TREE".into(), base: Point::ORIGIN, entities: vec![] });
    for x in [10.0, 20.0, 30.0] {
        doc.add(Geometry::Insert {
            block: "TREE".into(),
            placement: Placement::at(Point::new(x, 30.0)),
            attributes: vec![],
            clip: None,
        });
    }
    h.run_steps(1);
    type_all(&mut h, &["count"]);
    let at = screen(&h, 20.0, 30.0);
    click(&mut h, at);
    h.run_steps(1);
    assert_eq!(h.state_mut().engine_mut().selection().len(), 3);
    let history = h.state_mut().engine_mut().history().join("\n");
    assert!(history.contains("3 reference(s) of block \"TREE\"."));
}

#[test]
fn chspace_moves_objects_onto_the_sheet_looking_the_same() {
    let mut h = app_with_line();
    h.get_by_role_and_label(Role::Tab, "Layout1").click();
    h.run_steps(3);
    type_all(&mut h, &["mv", "f", "ms"]);
    h.run_steps(2);
    let (id, before) = {
        let doc = h.state_mut().document_mut();
        let e = doc.entities().next().unwrap();
        (e.id, e.geometry.clone())
    };
    let Geometry::Line { start, end } = before else { panic!() };
    let on_screen = (h.state().screen_pos(start), h.state().screen_pos(end));
    h.state_mut().engine_mut().set_selection(vec![id]);
    type_all(&mut h, &["chspace"]);
    h.run_steps(2);
    // Out of model space, onto the sheet, where it showed.
    type_all(&mut h, &["ps"]);
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    let e = doc.entity(id).unwrap();
    assert_eq!(e.space, doc.space());
    let Geometry::Line { start, end } = e.geometry.clone() else { panic!() };
    let now = (h.state().screen_pos(start), h.state().screen_pos(end));
    assert!((now.0 - on_screen.0).length() < 1.0 && (now.1 - on_screen.1).length() < 1.0, "{now:?} vs {on_screen:?}");
}

#[test]
fn dimension_style_and_find_and_replace_dialogs() {
    let mut h = app_with_line();
    type_all(&mut h, &["dimstyle"]);
    h.run_steps(2);
    let field = h.get_by_role_and_label(Role::TextInput, "Text height:");
    field.focus();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Text height:").type_text("5");
    h.key_press(Key::Enter);
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().dim_style().text_height, 5.0);

    // FIND: text replaced everywhere.
    h.state_mut().document_mut().add(Geometry::Text {
        look: Default::default(),
        position: Point::new(10.0, 30.0),
        height: 2.5,
        rotation: 0.0,
        width: 0.0,
        attach: oxidraft_model::Attach::BottomLeft,
        content: "Door D1".into(),
    });
    type_all(&mut h, &["find"]);
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Find what").focus();
    h.get_by_role_and_label(Role::TextInput, "Find what").type_text("door");
    h.get_by_role_and_label(Role::TextInput, "Replace with").focus();
    h.get_by_role_and_label(Role::TextInput, "Replace with").type_text("Gate");
    h.run_steps(1);
    let at = h.get_by_label("Replace All").rect().center();
    click(&mut h, at);
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    assert!(doc.entities().any(|e| matches!(&e.geometry, Geometry::Text { content, .. } if content == "Gate D1")));
}

#[test]
fn layer_walk_shows_only_the_layers_picked() {
    let mut h = app_with_line();
    {
        let doc = h.state_mut().document_mut();
        for (name, color) in [("Walls", 1), ("Doors", 2)] {
            doc.add_layer(oxidraft_model::Layer::new(name, color));
        }
        doc.add_on_layer(
            Geometry::Circle { center: Point::new(50.0, 30.0), radius: 5.0 },
            "Doors",
            oxidraft_model::Color::ByLayer,
        );
    }
    // From the Layers panel's slide-out.
    h.get_by_role_and_label(Role::Button, "Layers panel").click();
    h.run_steps(2);
    h.get_by_label("Layer Walk").click();
    h.run_steps(2);
    assert!(h.state().shows_layer("Walls") && h.state().shows_layer("Doors"), "starts as the drawing shows");
    h.get_by_label("Doors").click();
    h.run_steps(2);
    assert!(h.state().shows_layer("Doors") && !h.state().shows_layer("Walls") && !h.state().shows_layer("0"));
    // Closing restores the layers.
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(h.state().shows_layer("Walls") && h.state().shows_layer("0"));
    // Unless Restore on exit is off: then what showed stays.
    type_all(&mut h, &["laywalk"]);
    h.run_steps(2);
    h.get_by_label("Doors").click();
    h.run_steps(1);
    h.get_by_label("Restore on exit").click();
    h.run_steps(1);
    h.get_by_label("Close").click();
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    assert!(!doc.is_shown("Walls") && doc.is_shown("Doors"));
    assert!(doc.is_shown("0"), "the current layer stays on");
}

#[test]
fn export_pdf_and_publish_write_a_page_per_sheet() {
    let dir = std::env::temp_dir().join(format!("oxidraft-publish-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pages = |path: &std::path::Path| {
        let bytes = std::fs::read(path).unwrap();
        String::from_utf8_lossy(&bytes).matches("/Type /Page ").count()
    };
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    while doc.layouts().len() < 2 {
        let name = doc.next_layout_name();
        doc.new_layout(&name).unwrap();
    }
    let layouts = doc.layouts().len();
    // From the Output tab's Export button: the dialog.
    h.get_by_role_and_label(Role::Tab, "Output").click();
    h.run_steps(2);
    h.get_by_label("Export").click();
    h.run_steps(2);
    assert!(h.query_by_label("All Layouts (2)").is_some());
    h.get_by_label("Cancel").click();
    h.run_steps(1);
    let all = dir.join("all.pdf");
    assert_eq!(h.state_mut().export_pdf_to(&all, true), Ok(layouts));
    assert_eq!(pages(&all), layouts);
    let one = dir.join("one.pdf");
    assert_eq!(h.state_mut().export_pdf_to(&one, false), Ok(1), "model space, as PLOT would");

    // PUBLISH: this drawing's layouts, and another drawing's.
    let mut other = Document::new();
    other.new_layout("Layout1").unwrap();
    other.add(Geometry::Circle { center: Point::new(0.0, 0.0), radius: 5.0 });
    let other_path = dir.join("other.dxf");
    oxidraft_io_dwg::save_dxf(&other, &other_path).unwrap();
    type_all(&mut h, &["publish"]);
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::CheckBox, "Layout1").is_some());
    assert!(h.query_by_role_and_label(Role::CheckBox, "Model").is_some());
    h.state_mut().publish_add_drawing(&other_path).unwrap();
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::CheckBox, "other-Layout1").is_some());
    let other_layouts = other.layouts().len();
    let batch = dir.join("batch.pdf");
    assert_eq!(h.state_mut().publish_to(&batch), Ok(layouts + other_layouts));
    // Model ticked too.
    h.get_by_role_and_label(Role::CheckBox, "Model").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().publish_to(&batch), Ok(layouts + other_layouts + 1));
    assert_eq!(pages(&batch), layouts + other_layouts + 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn drawing_units_dialog_sets_feet_and_inches() {
    let mut h = app_with_line();
    type_all(&mut h, &["units"]);
    h.run_steps(2);
    assert!(h.query_by_label("1.5000,2.0039,0.0000").is_some(), "sample output");
    h.get_by_role_and_label(Role::ComboBox, "Length type").click();
    h.run_steps(2);
    h.get_by_label("Architectural").click();
    h.run_steps(2);
    assert!(h.query_by_label("0'-1 1/2\",0'-2\",0'-0\"").is_some(), "the sample follows");
    h.get_by_label("OK").click();
    h.run_steps(2);
    let units = h.state_mut().document_mut().units();
    assert_eq!((units.length, units.length_precision), (oxidraft_model::LengthType::Architectural, 4));
    // DIST answers in feet and inches, and takes them typed.
    type_all(&mut h, &["dist", "0,0", "@2'6\"<0"]);
    let history = h.state_mut().engine_mut().history().join("\n");
    assert!(history.contains("Distance = 2'-6\""), "{history}");
    // One undo step.
    h.state_mut().document_mut().undo();
    assert_eq!(h.state_mut().document_mut().units().length, oxidraft_model::LengthType::Decimal);
}

#[test]
fn rename_dialog_renames_a_layer() {
    let mut h = app_with_line();
    h.state_mut().document_mut().add_layer(oxidraft_model::Layer::new("Walls", 1));
    type_all(&mut h, &["ren"]);
    h.run_steps(2);
    h.get_by_label("Layers").click();
    h.run_steps(2);
    h.get_by_label("Walls").click();
    h.run_steps(2);
    let field = h.get_by_role_and_label(Role::TextInput, "New name").rect().center();
    click(&mut h, field);
    h.get_by_role_and_label(Role::TextInput, "New name").type_text(" outer");
    h.run_steps(1);
    let button = h.get_by_label("Rename To:").rect().center();
    click(&mut h, button);
    h.run_steps(2);
    assert!(h.state_mut().document_mut().layer("Walls outer").is_some());
    assert_eq!(h.query_all_by_label("Walls outer").count(), 2, "listed by its new name, and the old name shown");
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.query_by_label("Rename To:").is_none());
}

#[test]
fn write_block_writes_blocks_objects_and_the_drawing() {
    use oxidraft_app::{Afterwards, WblockSource};
    let dir = std::env::temp_dir().join(format!("oxidraft-wblock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let read = |path: &std::path::Path| match oxidraft_io_dwg::is_dwg(path) {
        true => oxidraft_io_dwg::load_file(path).unwrap().imported.document,
        false => oxidraft_io_dxf::load_file(path).unwrap().document,
    };
    let mut h = app_with_line();
    let circle = Geometry::Circle { center: Point::new(5.0, 5.0), radius: 2.0 };
    let block = oxidraft_model::Block {
        name: "Bolt".into(),
        base: Point::new(5.0, 5.0),
        entities: vec![oxidraft_model::BlockEntity::new("0", oxidraft_model::Color::ByLayer, circle)],
    };
    h.state_mut().document_mut().define_block(block);
    // A block: its contents, its base point as the drawing's.
    let bolt = dir.join("bolt.dwg");
    let app = h.state_mut();
    app.write_block(&WblockSource::Block("Bolt".into()), Point::ORIGIN, Afterwards::Retain, 4, &bolt).unwrap();
    let back = read(&bolt);
    assert_eq!(back.entities().count(), 1);
    assert_eq!(back.insbase(), Point::new(5.0, 5.0));
    assert_eq!(back.units().insertion, 4);
    // Objects, then made a block of that name in their place.
    let lines: Vec<_> = h.state_mut().document_mut().entities().map(|e| e.id).collect();
    let part = dir.join("part.dxf");
    let source = WblockSource::Objects(lines.clone());
    h.state_mut().write_block(&source, Point::ORIGIN, Afterwards::ConvertToBlock, 1, &part).unwrap();
    assert_eq!(read(&part).entities().count(), 2);
    assert_eq!(read(&part).units().insertion, 1, "inches");
    let doc = h.state_mut().document_mut();
    assert!(doc.block("part").is_some() && lines.iter().all(|id| doc.entity(*id).is_none()));
    assert!(doc.entities().any(|e| matches!(&e.geometry, Geometry::Insert { block, .. } if block == "part")));

    // The dialog: Objects, picked in the drawing with SELECT.
    h.state_mut().document_mut().undo();
    h.run_steps(2);
    type_all(&mut h, &["w"]);
    h.run_steps(2);
    h.get_by_label("Objects").click();
    h.run_steps(2);
    h.get_by_label("Select objects").click();
    h.run_steps(2);
    assert!(h.query_by_label("Write Block").is_none(), "out of the way while selecting");
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("SELECT"));
    let wall = screen(&h, 50.0, 0.0);
    click(&mut h, wall);
    type_all(&mut h, &[""]);
    h.run_steps(2);
    assert!(h.query_by_label("1 objects selected").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn quickcalc_works_out_and_pastes_to_the_command_line() {
    let mut h = app_with_line();
    h.get_by_role_and_label(Role::Tab, "View").click();
    h.run_steps(2);
    h.get_by_label("QuickCalc").click();
    h.run_steps(2);
    for key in ["7", "*", "6"] {
        h.get_by_role_and_label(Role::Button, key).click();
        h.run_steps(1);
    }
    h.get_by_role_and_label(Role::Button, "=").click();
    h.run_steps(2);
    assert!(h.query_by_label("7*6 = 42").is_some(), "in the history");
    h.get_by_label("Paste value to command line").click();
    h.run_steps(2);
    assert_eq!(h.state().command_text(), "42");
}

#[test]
fn current_color_linetype_and_lineweight_from_the_ribbon_and_dialogs() {
    use oxidraft_model::{Color, Lineweight};
    // Wide enough for the Home tab's Properties panel.
    let mut h = app_sized(1.0 / 4.0, 1600.0);
    let current = |h: &mut Harness<'_, OxidraftApp>| h.state_mut().document_mut().current_props().clone();
    // Nothing selected: the Properties panel sets what new objects get.
    h.get_by_role_and_label(Role::ComboBox, "Object color").click();
    h.run_steps(2);
    h.get_by_label("Red").click();
    h.run_steps(2);
    assert_eq!(current(&mut h).color, Color::Index(1));
    type_all(&mut h, &["line", "0,30", "40,30", ""]);
    let line = h.state_mut().document_mut().entities().last().map(|e| e.color);
    assert_eq!(line, Some(Color::Index(1)), "new objects get it");

    // COLOR: any of the 255.
    type_all(&mut h, &["col"]);
    h.run_steps(2);
    h.get_by_label("Color 30").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(current(&mut h).color, Color::Index(30));

    // LINETYPE: load CENTER, make it current.
    type_all(&mut h, &["lt"]);
    h.run_steps(2);
    h.get_by_label("Load...").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "CENTER").click();
    h.run_steps(1);
    h.get_all_by_label("OK").last().unwrap().click();
    h.run_steps(2);
    assert!(h.state_mut().document_mut().own_linetypes().iter().any(|n| n == "CENTER"));
    h.get_by_label("CENTER").click();
    h.run_steps(1);
    h.get_by_label("Current").click();
    h.run_steps(2);
    assert_eq!(current(&mut h).linetype, "CENTER");
    h.get_by_label("OK").click();
    h.run_steps(2);

    // LWEIGHT: 0.13 mm, shown.
    type_all(&mut h, &["lw"]);
    h.run_steps(2);
    h.get_by_label("0.13 mm").click();
    h.run_steps(1);
    h.get_by_label("Display Lineweight").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(current(&mut h).lineweight, Lineweight::Hundredths(13));
}

#[test]
fn text_style_dialog_makes_a_style_that_new_text_gets() {
    let mut h = app_with_line();
    type_all(&mut h, &["st"]);
    h.run_steps(2);
    assert!(h.query_by_label("Current text style: Standard").is_some());
    h.get_by_label("New...").click();
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::TextInput, "Style Name").is_some(), "the name asked for");
    let ok = h.get_by_role_and_label(Role::Button, "OK").rect().center();
    click(&mut h, ok);
    h.run_steps(2);
    assert!(h.state_mut().document_mut().text_style("Style1").is_some());
    h.get_by_label("Set Current").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().current_text_style().name, "Style1");
    h.get_by_label("Close").click();
    h.run_steps(2);
    type_all(&mut h, &["text", "0,30", "2.5", "0", "HELLO", ""]);
    let look = h.state_mut().document_mut().entities().find_map(|e| match &e.geometry {
        Geometry::Text { look, .. } => Some(look.clone()),
        _ => None,
    });
    assert_eq!(look.map(|l| l.style), Some("Style1".to_string()));
    // Annotate's dropdown shows it, and can switch back.
    h.get_by_role_and_label(Role::Tab, "Annotate").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::ComboBox, "Text style").click();
    h.run_steps(2);
    h.get_by_label("Standard").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().current_text_style().name, "Standard");
}

#[test]
fn drafting_settings_and_point_style() {
    let mut h = app_with_line();
    type_all(&mut h, &["os"]);
    h.run_steps(2);
    // The Object Snap tab: Clear All, then Endpoint only.
    h.get_by_label("Clear All").click();
    h.run_steps(1);
    h.get_by_role_and_label(Role::CheckBox, "Endpoint").click();
    h.run_steps(1);
    h.get_by_label("Polar Tracking").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::ComboBox, "Increment angle").click();
    h.run_steps(2);
    h.get_by_label("30").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().polar_increment, 30.0);
    type_all(&mut h, &["ptype"]);
    h.run_steps(2);
    h.get_by_label("PDMODE 35").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().pdmode(), 35);
    // SELECTALL selects both lines.
    type_all(&mut h, &["selectall"]);
    assert_eq!(h.state_mut().engine_mut().selection().len(), 2);
}

#[test]
fn geometric_tolerance_dialog_places_a_frame() {
    let mut h = app_with_line();
    type_all(&mut h, &["tol"]);
    h.run_steps(2);
    h.get_by_role_and_label(Role::ComboBox, "Symbol 1").click();
    h.run_steps(2);
    h.get_by_label("Position").click();
    h.run_steps(1);
    for (field, text) in [("Tolerance 1 row 1", "0.05"), ("Datum 1 row 1", "A")] {
        let at = h.get_by_role_and_label(Role::TextInput, field).rect().center();
        click(&mut h, at);
        h.get_by_role_and_label(Role::TextInput, field).type_text(text);
        h.run_steps(1);
    }
    let ok = h.get_by_label("OK").rect().center();
    click(&mut h, ok);
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("TOLERANCE"));
    let at = screen(&h, 40.0, 30.0);
    click(&mut h, at);
    h.run_steps(2);
    let content = h.state_mut().document_mut().entities().find_map(|e| match &e.geometry {
        Geometry::Tolerance { content, .. } => Some(content.clone()),
        _ => None,
    });
    assert_eq!(content.as_deref(), Some("{\\Fgdt;j}%%v0.05%%v%%vA"));
}

#[test]
fn multileader_and_table_styles() {
    let mut h = app_with_line();
    type_all(&mut h, &["mls"]);
    h.run_steps(2);
    h.get_by_role_and_label(Role::ComboBox, "Symbol").click();
    h.run_steps(2);
    h.get_by_label("Dot").click();
    h.run_steps(1);
    h.get_by_label("Automatically include landing").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let style = h.state_mut().document_mut().leader_style();
    assert_eq!((style.arrowhead, style.landing_on), (oxidraft_model::annotation::LeaderArrow::Dot, false));
    // One undo step.
    h.state_mut().document_mut().undo();
    assert!(h.state_mut().document_mut().leader_style().landing_on);
    type_all(&mut h, &["ts"]);
    h.run_steps(2);
    assert!(h.query_by_label("Modify Table Style: Standard").is_some());
}

#[test]
fn block_attribute_manager_reorders_and_syncs() {
    use oxidraft_model::{Attach, Block, BlockEntity, Color, Placement};
    let mut h = app_with_line();
    {
        let doc = h.state_mut().document_mut();
        let attdef = |tag: &str, y: f64| {
            BlockEntity::new(
                "0",
                Color::ByLayer,
                Geometry::AttributeDefinition {
                    tag: tag.into(),
                    prompt: format!("{tag}?"),
                    default: format!("{tag}1"),
                    position: Point::new(0.0, y),
                    height: 2.5,
                    rotation: 0.0,
                    attach: Attach::BottomLeft,
                    invisible: false,
                    constant: false,
                },
            )
        };
        doc.define_block(Block {
            name: "Door".into(),
            base: Point::ORIGIN,
            entities: vec![attdef("NUMBER", 0.0), attdef("WIDTH", 5.0)],
        });
        doc.add(Geometry::Insert {
            block: "Door".into(),
            placement: Placement::at(Point::new(50.0, 30.0)),
            attributes: Vec::new(),
            clip: None,
        });
    }
    type_all(&mut h, &["battman"]);
    h.run_steps(2);
    h.get_by_label("WIDTH").click();
    h.run_steps(1);
    h.get_by_label("Move Up").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    let tags: Vec<String> = doc
        .block("Door")
        .unwrap()
        .entities
        .iter()
        .filter_map(|be| match &be.geometry {
            Geometry::AttributeDefinition { tag, .. } => Some(tag.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(tags, ["WIDTH", "NUMBER"]);
    let on_reference: Vec<String> = doc
        .entities()
        .find_map(|e| match &e.geometry {
            Geometry::Insert { attributes, .. } => Some(attributes.iter().map(|a| a.tag.clone()).collect()),
            _ => None,
        })
        .unwrap();
    assert_eq!(on_reference, ["WIDTH", "NUMBER"], "the reference was synced");
}

#[test]
fn transparency_from_the_properties_palette() {
    let mut h = app_with_line();
    let wall = screen(&h, 50.0, 0.0);
    click(&mut h, wall);
    h.state_mut().type_command("pr");
    h.run_steps(3);
    h.get_by_role_and_label(Role::ComboBox, "Transparency").click();
    h.run_steps(2);
    h.get_by_label("ByBlock").click();
    h.run_steps(2);
    let t = h
        .state_mut()
        .document_mut()
        .entities()
        .find(|e| matches!(e.geometry, Geometry::Line { start, .. } if start.y == 0.0))
        .map(|e| e.transparency);
    assert_eq!(t, Some(oxidraft_model::Transparency::ByBlock));
}

#[test]
fn layer_states_manager_saves_and_restores() {
    let mut h = app_with_line();
    h.state_mut().document_mut().add_layer(oxidraft_model::Layer::new("Walls", 1));
    type_all(&mut h, &["las"]);
    h.run_steps(2);
    h.get_by_label("New...").click();
    h.run_steps(2);
    let at = h.get_by_role_and_label(Role::TextInput, "New layer state name:").rect().center();
    click(&mut h, at);
    h.get_by_role_and_label(Role::TextInput, "New layer state name:").type_text("Plan");
    h.run_steps(1);
    let save = h.get_by_label("Save New").rect().center();
    click(&mut h, save);
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().layer_states().len(), 1);
    // Walls off, then the state brings it back on.
    let doc = h.state_mut().document_mut();
    let walls = doc.layer("Walls").cloned().unwrap();
    doc.update_layer(oxidraft_model::Layer { on: false, ..walls });
    h.run_steps(1);
    h.get_by_label("Restore").click();
    h.run_steps(2);
    assert!(h.state_mut().document_mut().layer("Walls").unwrap().on);
}

#[test]
fn new_asks_to_save_changes_and_scripts_run() {
    let mut h = app_with_line();
    // A change, then NEW: asked first; Cancel keeps the drawing.
    type_all(&mut h, &["circle", "50,30", "5"]);
    let before = h.state_mut().document_mut().len();
    type_all(&mut h, &["new"]);
    h.run_steps(2);
    assert!(h.query_by_label("Save changes to Drawing1.dxf?").is_some());
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().len(), before);
    // No: a fresh drawing.
    type_all(&mut h, &["new"]);
    h.run_steps(2);
    h.get_by_label("No").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().len(), 0);
    // A script: each space or line end is Enter.
    h.state_mut().run_script("line 0,0 10,0 10,10 \ncircle 5,5 2\n");
    h.run_steps(1);
    let kinds: Vec<&str> = h.state_mut().document_mut().entities().map(|e| e.geometry.name()).collect();
    assert_eq!(kinds, ["Line", "Line", "Circle"]);
    // ISO on the status bar: isometric drafting on the top plane.
    h.get_by_label("ISO").click();
    h.run_steps(1);
    assert_eq!(h.state_mut().engine_mut().vars.isoplane, Some(oxidraft_commands::IsoPlane::Top));
}

#[test]
fn fields_go_in_from_the_field_dialog_and_the_text_editor() {
    use oxidraft_model::{Attach, DrawingProperties, field};
    // 60 frames a second, so two clicks fall within egui's double-click time.
    let mut h = app_with_line_at(1.0 / 60.0);
    let doc = h.state_mut().document_mut();
    doc.set_properties(DrawingProperties { title: "Ground floor".into(), ..Default::default() });
    let text = doc.add(Geometry::Text {
        look: Default::default(),
        position: Point::new(30.0, 30.0),
        height: 5.0,
        rotation: 0.0,
        width: 0.0,
        attach: Attach::BottomLeft,
        content: "Plan".into(),
    });
    h.run_steps(2);

    // FIELD: Title, then where its text goes.
    type_all(&mut h, &["field"]);
    h.run_steps(2);
    let title = h.get_by_label("Title").rect().center();
    click(&mut h, title);
    let upper = h.get_by_label("Uppercase").rect().center();
    click(&mut h, upper);
    h.run_steps(1);
    assert!(h.query_by_label("GROUND FLOOR").is_some(), "the field's value shows");
    let ok = h.get_by_label("OK").rect().center();
    click(&mut h, ok);
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("FIELD"));
    let at = screen(&h, 60.0, 20.0);
    click(&mut h, at);
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    let placed = doc.entities().find_map(|e| match &e.geometry {
        Geometry::Text { content, .. } if e.id != text => Some(content.clone()),
        _ => None,
    });
    assert_eq!(placed.as_deref(), Some(r#"%<\AcVar Title \f "%tc1">%"#));

    // Double-click the text: the text editor, and Insert Field.
    h.run_steps(30);
    let on_text = screen(&h, 32.0, 31.0);
    click(&mut h, on_text);
    click(&mut h, on_text);
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::TextInput, "Text").is_some(), "the text editor opens");
    let insert = h.get_by_label("Insert Field...").rect().center();
    click(&mut h, insert);
    h.run_steps(2);
    let name = h.get_by_label("Filename").rect().center();
    click(&mut h, name);
    // The Field dialog's OK, over the text editor's.
    let ok = h.get_all_by_label("OK").last().unwrap().rect().center();
    click(&mut h, ok);
    h.run_steps(2);
    let ok = h.get_by_label("OK").rect().center();
    click(&mut h, ok);
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    match &doc.entity(text).unwrap().geometry {
        Geometry::Text { content, .. } => {
            assert_eq!(content, r#"Plan%<\AcVar Filename \f "%fn6">%"#);
            assert_eq!(field::evaluate(content, doc), "PlanDrawing1.dxf");
        }
        g => panic!("{g:?}"),
    }
}

#[test]
fn spell_changes_words_not_in_the_dictionary() {
    use oxidraft_model::Attach;
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    let text = doc.add(Geometry::Text {
        look: Default::default(),
        position: Point::new(30.0, 30.0),
        height: 5.0,
        rotation: 0.0,
        width: 0.0,
        attach: Attach::BottomLeft,
        content: r"Qwzx\PQwzx %%c20".into(),
    });
    h.run_steps(2);
    type_all(&mut h, &["spell"]);
    h.run_steps(2);
    let start = h.get_by_label("Start").rect().center();
    click(&mut h, start);
    h.run_steps(2);
    let field = h.get_by_role_and_label(Role::TextInput, "Change to");
    field.focus();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Change to").type_text("Beam");
    h.run_steps(1);
    let all = h.get_by_label("Change All").rect().center();
    click(&mut h, all);
    h.run_steps(2);
    match &h.state_mut().document_mut().entity(text).unwrap().geometry {
        Geometry::Text { content, .. } => assert_eq!(content, r"Beam\PBeam %%c20"),
        g => panic!("{g:?}"),
    }
    assert!(h.query_all_by_label("Spelling check complete.").next().is_some());
}

#[test]
fn system_variables_typed_and_set() {
    let mut h = app_with_line();
    type_all(&mut h, &["orthomode"]);
    assert_eq!(h.state_mut().engine_mut().prompt(), "Enter new value for ORTHOMODE <0>:");
    type_all(&mut h, &["1"]);
    h.run_steps(1);
    assert!(h.state_mut().engine_mut().ortho());
    // SETVAR with a name, a read-only one, and a bad value.
    type_all(&mut h, &["setvar", "ltscale", "2.5"]);
    h.run_steps(1);
    assert_eq!(h.state_mut().document_mut().ltscale(), 2.5);
    type_all(&mut h, &["dwgname"]);
    type_all(&mut h, &["clayer", "nolayer"]);
    h.run_steps(1);
    type_all(&mut h, &["fill", "off", "limits", "0,0", "200,100", "status"]);
    h.run_steps(1);
    let history = h.state_mut().engine_mut().history().to_vec();
    assert!(history.iter().any(|l| l.starts_with("DWGNAME = \"Drawing1.dxf\"")), "{history:?}");
    assert!(history.iter().any(|l| l == "No such layer."), "{history:?}");
    assert!(history.iter().any(|l| l.starts_with("Fill Off  Grid")), "{history:?}");
    assert!(!h.state_mut().document_mut().fill());
    // Clean screen hides the ribbon; Ctrl+0 brings it back.
    assert!(h.query_by_role_and_label(Role::Tab, "Annotate").is_some());
    type_all(&mut h, &["cleanscreenon"]);
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::Tab, "Annotate").is_none());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num0);
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::Tab, "Annotate").is_some());
}

#[test]
fn pngout_saves_the_drawing_area() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-pngout-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("view.png");
    h.state_mut().start_image_out(path.clone());
    for _ in 0..20 {
        h.run_steps(1);
        if path.exists() && !h.state().capturing() {
            break;
        }
    }
    let history = h.state_mut().engine_mut().history().to_vec();
    assert!(history.iter().any(|l| l.starts_with("Saved ")), "{history:?}");
    let image = image::open(&path).unwrap();
    assert!(image.width() > 100 && image.height() > 100);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn plot_style_tables_are_added_and_edited() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-styles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    h.state_mut().keep_plot_styles_in(dir.clone());
    type_all(&mut h, &["stylesmanager"]);
    h.run_steps(2);
    let field = h.get_by_role_and_label(Role::TextInput, "New table name").rect().center();
    click(&mut h, field);
    h.get_by_role_and_label(Role::TextInput, "New table name").type_text("pens");
    h.run_steps(1);
    let add = h.get_by_label("Add").rect().center();
    click(&mut h, add);
    h.run_steps(2);
    // The editor: color 1 plotted black, then saved.
    h.get_by_label("Use object color").click();
    h.run_steps(2);
    assert!(h.query_by_label("Black").is_some(), "a color of its own to set");
    let save = h.get_by_label("Save & Close").rect().center();
    click(&mut h, save);
    h.run_steps(2);
    let table = oxidraft_plot::ctb::PenTable::read("pens.ctb", &std::fs::read(dir.join("pens.ctb")).unwrap()).unwrap();
    assert_eq!(table.pens[0].color, Some([0, 0, 0]));
    let styles: Vec<String> = oxidraft_plot::PlotStyle::all().into_iter().map(|s| s.name()).collect();
    assert!(styles.contains(&"pens.ctb".to_string()), "{styles:?}");
    assert_eq!(oxidraft_plot::PlotStyle::Table(0).rgb(1), [0, 0, 0]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn etransmit_gathers_the_drawing_and_its_files() {
    use oxidraft_model::{ImageDef, Xref, XrefStatus};
    let dir = std::env::temp_dir().join(format!("oxidraft-etransmit-{}", std::process::id()));
    let (source, dest) = (dir.join("project"), dir.join("out"));
    std::fs::create_dir_all(source.join("refs")).unwrap();
    std::fs::write(source.join("refs/site.png"), [0u8; 4]).unwrap();
    oxidraft_io_dxf::save_file(&Document::new(), source.join("refs/base.dxf")).unwrap();
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    doc.define_block(oxidraft_model::Block { name: "base".into(), base: Point::ORIGIN, entities: Vec::new() });
    doc.set_xref(Xref {
        name: "base".into(),
        path: source.join("refs/base.dxf").display().to_string(),
        overlay: false,
        status: XrefStatus::Loaded,
    });
    doc.set_image_def(ImageDef {
        name: "site".into(),
        path: source.join("refs/site.png").display().to_string(),
        page: None,
        width: 2.0,
        height: 2.0,
        status: XrefStatus::Loaded,
    });
    doc.add(Geometry::Image {
        image: "site".into(),
        origin: Point::ORIGIN,
        u: Point::new(10.0, 0.0),
        v: Point::new(0.0, 10.0),
        clip: Vec::new(),
        adjust: Default::default(),
    });
    doc.set_image_def(ImageDef {
        name: "gone".into(),
        path: "/nowhere/gone.png".into(),
        page: None,
        width: 2.0,
        height: 2.0,
        status: XrefStatus::Loaded,
    });
    let n = h.state_mut().transmit_to(&dest).unwrap();
    assert_eq!(n, 3, "the drawing, the xref and the image");
    for f in ["Drawing1.dxf", "base.dxf", "site.png", "Drawing1.txt"] {
        assert!(dest.join(f).exists(), "{f}");
    }
    let copy = oxidraft_io_dxf::load_file(dest.join("Drawing1.dxf")).unwrap().document;
    assert_eq!(copy.xref("base").map(|x| x.path.as_str()), Some("base.dxf"));
    assert_eq!(copy.image_def("site").map(|d| d.path.as_str()), Some("site.png"));
    let report = std::fs::read_to_string(dest.join("Drawing1.txt")).unwrap();
    assert!(report.contains("Not found, left out:") && report.contains("gone.png"), "{report}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn design_center_brings_layers_and_blocks_in() {
    use oxidraft_model::{Block, BlockEntity, Color, Layer};
    let dir = std::env::temp_dir().join(format!("oxidraft-adc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut lib = Document::new();
    lib.add_layer(Layer::new("Trees", 3));
    lib.define_block(Block {
        name: "LEAF".into(),
        base: Point::ORIGIN,
        entities: vec![BlockEntity::new(
            "Trees",
            Color::ByLayer,
            Geometry::Circle { center: Point::ORIGIN, radius: 1.0 },
        )],
    });
    lib.add(Geometry::Insert {
        block: "LEAF".into(),
        placement: oxidraft_model::Placement::at(Point::ORIGIN),
        attributes: Vec::new(),
        clip: None,
    });
    oxidraft_io_dxf::save_file(&lib, dir.join("lib.dxf")).unwrap();

    // 60 frames a second, so two clicks fall within egui's double-click time.
    let mut h = app_with_line_at(1.0 / 60.0);
    h.state_mut().design_center_in(dir.clone());
    h.run_steps(2);
    h.get_by_label("lib.dxf").click();
    h.run_steps(2);
    h.get_by_label_contains("Layers (").click();
    h.run_steps(2);
    h.get_by_label("Trees").click();
    h.run_steps(1);
    h.get_by_label("Add to Drawing").click();
    h.run_steps(2);
    assert!(h.state_mut().document_mut().layer("Trees").is_some_and(|l| l.color == 3));
    h.get_by_label_contains("Blocks (").click();
    h.run_steps(2);
    h.get_by_label("LEAF").click();
    h.run_steps(1);
    // DesignCenter's Insert, over the ribbon's.
    h.get_all_by_label("Insert").last().unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("INSERT"));
    assert!(h.state_mut().document_mut().block("LEAF").is_some());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn tool_palettes_run_tools_and_take_new_ones() {
    let mut h = app_with_line();
    type_all(&mut h, &["toolpalettes"]);
    h.run_steps(2);
    // "Circle" is also on the ribbon: the palette's comes last.
    h.get_all_by_label("Circle").last().unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("CIRCLE"));
    h.key_press(Key::Escape);
    h.run_steps(1);
    // A tool from the selected line: LINE on its layer.
    let first = h.state_mut().document_mut().entities().next().unwrap().id;
    h.state_mut().engine_mut().set_selection(vec![first]);
    h.run_steps(1);
    h.get_by_label("Add Selected Objects").click();
    h.run_steps(2);
    let tools = h.state_mut().palettes()[0].tools.len();
    assert_eq!(tools, 10, "the nine draw tools and the new one");
    h.get_by_label("Hatches").click();
    h.run_steps(2);
    h.get_by_label("ANSI37").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("HATCH"));
    assert_eq!(h.state_mut().engine_mut().vars.hatch_pattern, oxidraft_model::HatchPattern::Ansi37);
}

#[test]
fn annotation_scale_from_the_status_bar() {
    use oxidraft_model::Attach;
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    let text = doc.add(Geometry::Text {
        look: Default::default(),
        position: Point::new(20.0, 20.0),
        height: 2.5,
        rotation: 0.0,
        width: 0.0,
        attach: Attach::BottomLeft,
        content: "ROOM".into(),
    });
    doc.set_annotative(text, true);
    h.run_steps(2);
    type_all(&mut h, &["annoautoscale", "4"]);
    h.get_by_role_and_label(Role::Button, "Annotation Scale").click();
    h.run_steps(2);
    h.get_by_label("1:50").click();
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.anno_scale(), 50.0);
    assert!(
        matches!(doc.entity(text).unwrap().geometry, Geometry::Text { height, .. } if (height - 125.0).abs() < 1e-9)
    );
    // One undo puts the scale's sizes back.
    type_all(&mut h, &["cannoscale", "1:1"]);
    h.run_steps(1);
    let doc = h.state_mut().document_mut();
    assert!(matches!(doc.entity(text).unwrap().geometry, Geometry::Text { height, .. } if (height - 2.5).abs() < 1e-9));
}

#[test]
fn scale_list_takes_scales_of_your_own() {
    let mut h = app_with_line();
    type_all(&mut h, &["scalelistedit"]);
    h.run_steps(2);
    let field = h.get_by_role_and_label(Role::TextInput, "New scale").rect().center();
    click(&mut h, field);
    h.get_by_role_and_label(Role::TextInput, "New scale").type_text("1:75");
    h.run_steps(1);
    h.get_all_by_label("Add").last().unwrap().click();
    h.run_steps(2);
    assert!(h.query_by_label("1:75  (yours)").is_some());
    type_all(&mut h, &["cannoscale", "1:75"]);
    h.run_steps(1);
    assert_eq!(h.state_mut().document_mut().anno_scale(), 75.0);
}

#[test]
fn classic_array_group_and_layout_wizard() {
    let mut h = app_with_line();
    let first = h.state_mut().document_mut().entities().next().unwrap().id;
    h.state_mut().engine_mut().set_selection(vec![first]);
    h.run_steps(1);
    // ARRAYCLASSIC: 4 x 4 of the selected line, 15 copies.
    type_all(&mut h, &["arrayclassic"]);
    h.run_steps(2);
    let before = h.state_mut().document_mut().len();
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().len(), before + 15);
    // CLASSICGROUP: a group of the selection, named.
    h.state_mut().engine_mut().set_selection(vec![first]);
    type_all(&mut h, &["classicgroup"]);
    h.run_steps(2);
    let field = h.get_by_role_and_label(Role::TextInput, "Group Name:").rect().center();
    click(&mut h, field);
    h.get_by_role_and_label(Role::TextInput, "Group Name:").type_text("WALLS");
    h.run_steps(1);
    h.get_by_label("New").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().groups().iter().map(|g| g.name.clone()).collect::<Vec<_>>(), ["WALLS"]);
    h.get_by_label("OK").click();
    h.run_steps(2);
    // LAYOUTWIZARD: an A3 landscape layout with a viewport.
    h.state_mut().finish_layout_wizard("Plans", 1, true, true).unwrap();
    h.run_steps(2);
    let doc = h.state_mut().document_mut();
    let layout = doc.layouts().into_iter().find(|l| l.name == "Plans").cloned().unwrap();
    assert_eq!((layout.paper.width, layout.paper.height), (420.0, 297.0));
    assert!(doc.space_in_draw_order(layout.id).iter().any(|e| matches!(e.geometry, Geometry::Viewport { .. })));
    assert_eq!(doc.space(), layout.id);
}

#[test]
fn pdfimport_brings_a_plots_lines_and_text_back() {
    use oxidraft_model::Attach;
    // A drawing plotted to PDF: a line and a text.
    let mut drawing = Document::new();
    drawing.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 0.0) });
    drawing.add(Geometry::Text {
        look: Default::default(),
        position: Point::new(10.0, 20.0),
        height: 5.0,
        rotation: 0.0,
        width: 0.0,
        attach: Attach::BottomLeft,
        content: "PLAN".into(),
    });
    let settings = oxidraft_plot::PlotSettings::default();
    let sheet = oxidraft_plot::render(&drawing, oxidraft_model::MODEL, &settings).unwrap();
    let dir = std::env::temp_dir().join(format!("oxidraft-pdfimport-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("plan.pdf");
    std::fs::write(&file, oxidraft_plot::pdf(&sheet, "plan")).unwrap();

    let mut h = app_with_line();
    let n = h.state_mut().import_pdf_file(&file, 0, 25.4);
    assert!(n >= 2, "{n}");
    let doc = h.state_mut().document_mut();
    let lines: Vec<f64> = doc.entities().filter(|e| e.layer == "PDF_Geometry").map(|e| e.geometry.length()).collect();
    assert!(!lines.is_empty());
    let texts: Vec<String> = doc
        .entities()
        .filter_map(|e| match &e.geometry {
            Geometry::Text { content, .. } if e.layer == "PDF_Text" => Some(content.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t.contains("PLAN")), "{texts:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A one-page PDF with a line on layer Walls and one on Doors.
fn layered_pdf() -> Vec<u8> {
    let content = "/OC /L1 BDC 0 0 m 100 0 l S EMC /OC /L2 BDC 0 50 m 100 50 l S EMC";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R] /D << >> >> >>".to_string(),
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
fn underlay_layers_hide_from_view_and_import() {
    use oxidraft_model::{ImageDef, XrefStatus};
    let dir = std::env::temp_dir().join(format!("oxidraft-ulayers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("plan.pdf");
    std::fs::write(&file, layered_pdf()).unwrap();
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    doc.set_image_def(ImageDef {
        name: "plan".into(),
        path: file.display().to_string(),
        page: Some(0),
        width: 200.0,
        height: 200.0,
        status: XrefStatus::Loaded,
    });
    let underlay = doc.add(Geometry::Image {
        image: "plan".into(),
        origin: Point::new(0.0, 0.0),
        u: Point::new(200.0, 0.0),
        v: Point::new(0.0, 200.0),
        clip: Vec::new(),
        adjust: Default::default(),
    });
    h.state_mut().set_underlay_layers_off("plan", vec!["Doors".into()]);
    assert_eq!(h.state_mut().document_mut().pdf_layers_off("plan"), ["Doors".to_string()]);
    h.state_mut().engine_mut().set_selection(vec![underlay]);
    type_all(&mut h, &["pdfimport"]);
    h.run_steps(1);
    let doc = h.state_mut().document_mut();
    let imported: Vec<f64> = doc
        .entities()
        .filter(|e| e.layer == "PDF_Geometry")
        .filter_map(|e| match e.geometry {
            Geometry::Line { start, .. } => Some(start.y),
            _ => None,
        })
        .collect();
    assert_eq!(imported.len(), 1, "only the wall's line: {imported:?}");
    assert!(imported[0].abs() < 1e-6);
    // Kept with the drawing.
    let bytes = oxidraft_io_dwg::write(doc, oxidraft_io_dwg::Version::R2018).unwrap();
    let back = oxidraft_io_dwg::read(&bytes).unwrap().imported.document;
    assert_eq!(back.pdf_layers_off("plan"), ["Doors".to_string()]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_associative_array_shows_and_changes_in_properties() {
    use oxidraft_model::assoc_array::ArrayLayout;
    let mut h = app_with_line();
    let wall = screen(&h, 50.0, 0.0);
    click(&mut h, wall);
    type_all(&mut h, &["arrayrect", "2", "3", "10", "200"]);
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.len(), 2, "the array and the other line");
    let id = doc.entities().find(|e| e.array.is_some()).unwrap().id;
    click(&mut h, wall);
    h.state_mut().type_command("pr");
    h.run_steps(3);
    assert!(h.query_by_label_contains("Array (Rectangular) (1)").is_some());
    h.get_by_role_and_label(Role::TextInput, "Columns").focus();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(Role::TextInput, "Columns").type_text("5");
    h.key_press(Key::Enter);
    h.run_steps(2);
    let layout = h.state_mut().document_mut().array(id).map(|a| a.layout.clone());
    assert!(matches!(layout, Some(ArrayLayout::Rect { rows: 2, columns: 5, .. })), "{layout:?}");
}

#[test]
fn plotters_wmfout_and_paper_sizes_of_your_own() {
    use oxidraft_plot::Plotter;
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-plotters-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // PLOT to the PublishToWeb PNG plotter: an image of the sheet.
    h.state_mut().type_command("plot");
    h.run_steps(2);
    h.get_by_role_and_label(Role::ComboBox, "Printer/plotter").click();
    h.run_steps(2);
    h.get_by_label("PublishToWeb PNG.pc3").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().plot_settings_mut().unwrap().plotter, Plotter::Png);
    let png = dir.join("plot.png");
    h.state_mut().plot_to(&png).unwrap();
    let img = image::open(&png).unwrap();
    assert!(img.width() > 1000 && img.height() > 500, "{}x{}", img.width(), img.height());
    // EXPORTDWFX: a DWFx (zip) package.
    let dwfx = dir.join("sheets.dwfx");
    assert_eq!(h.state_mut().export_to_with(&dwfx, false, Plotter::Dwfx), Ok(1));
    assert!(std::fs::read(&dwfx).unwrap().starts_with(b"PK\x03\x04"));
    // WMFOUT: a placeable metafile of the objects.
    let ids: Vec<_> = h.state_mut().document_mut().entities().map(|e| e.id).collect();
    let wmf = dir.join("objects.wmf");
    h.state_mut().wmf_out_to(&wmf, &ids).unwrap();
    assert!(std::fs::read(&wmf).unwrap().starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]));
    // PLOTTERMANAGER: a paper size of your own, which the plotters offer.
    h.key_press(Key::Escape);
    h.state_mut().type_command("plottermanager");
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Paper name").click();
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, "Paper name").type_text("Banner 1000");
    h.run_steps(1);
    h.get_by_label("Add").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(oxidraft_plot::papers().iter().any(|p| p.name == "Banner 1000"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn several_drawings_in_file_tabs() {
    use oxidraft_app::Drawings;
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).with_step_dt(0.25).build_eframe(|cc| {
        let mut app = OxidraftApp::new(cc);
        let mut doc = Document::new();
        doc.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(100.0, 0.0) });
        app.set_document(doc);
        Drawings::new(app)
    });
    h.run_steps(3);
    let type_in = |h: &mut Harness<'_, Drawings>, text: &str| {
        h.state_mut().current().type_command(text);
        h.run_steps(2);
    };
    // NEW: a second drawing in a tab of its own, current.
    type_in(&mut h, "new");
    assert_eq!((h.state().len(), h.state().active()), (2, 1));
    assert_eq!(h.state_mut().current().document_mut().len(), 0);
    h.state_mut().current().document_mut().add(Geometry::Circle { center: Point::ORIGIN, radius: 5.0 });
    h.run_steps(1);
    assert!(h.query_by_label("Drawing2.dxf*").is_some(), "both tabs show, the changed one marked");
    // Ctrl+Tab and SWITCHWINDOWS go round them.
    h.key_press_modifiers(Modifiers::CTRL, Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().active(), 0);
    type_in(&mut h, "switchwindows");
    assert_eq!(h.state().active(), 1);
    // A drawing opened gets a tab; one already open is gone back to.
    let dir = std::env::temp_dir().join(format!("oxidraft-tabs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("site.dxf");
    let mut site = Document::new();
    site.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(5.0, 5.0) });
    oxidraft_io_dxf::save_file(&site, &file).unwrap();
    for _ in 0..2 {
        let ctx = h.ctx.clone();
        h.state_mut().handle(&ctx, oxidraft_app::drawings::TabRequest::Open(file.clone()));
        h.run_steps(1);
    }
    assert_eq!(h.state().names(), ["Drawing1.dxf", "Drawing2.dxf", "site.dxf"]);
    assert_eq!(h.state().active(), 2);
    // SYSWINDOWS, tiled: the others side by side, each in a window of its own to work in.
    type_in(&mut h, "syswindows");
    type_in(&mut h, "v");
    assert_eq!(h.state().in_windows(), [0, 1]);
    let other = h.state_mut().drawing(0).unwrap();
    other.type_command("LINE");
    other.type_command("0,0");
    other.type_command("10,10");
    other.type_command("");
    h.run_steps(2);
    assert_eq!(h.state_mut().drawing(0).unwrap().document_mut().len(), 2, "drawn in its window");
    assert_eq!(h.state().active(), 2, "the main window's still site.dxf");
    // QVDRAWINGCLOSE: back in the tabs; then the first one picked.
    type_in(&mut h, "qvdrawingclose");
    assert!(h.state().in_windows().is_empty());
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, oxidraft_app::drawings::TabRequest::Switch(0));
    h.run_steps(2);
    assert_eq!(h.state().active(), 0);
    // CLOSE: its tab goes (asking about its change first).
    type_in(&mut h, "close");
    h.state_mut().current().document_mut();
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, oxidraft_app::drawings::TabRequest::Close(0));
    h.run_steps(1);
    assert_eq!(h.state().names(), ["Drawing2.dxf", "site.dxf"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn workspaces_and_cui() {
    let mut h = app_with_line();
    let props = |h: &Harness<'_, OxidraftApp>| h.query_by_label("PROPERTIES").is_some();
    // The Properties palette open, saved as workspace Review.
    type_all(&mut h, &["pr"]);
    h.run_steps(2);
    assert!(props(&h));
    type_all(&mut h, &["wssave", "Review"]);
    assert_eq!(h.state().current_workspace(), "Review");
    // AutoCAD's own: the palette closes; Review again (WSCURRENT) opens it.
    type_all(&mut h, &["workspace", "", "Drafting & Annotation"]);
    h.run_steps(2);
    assert!(!props(&h));
    type_all(&mut h, &["wscurrent", "Review"]);
    h.run_steps(2);
    assert!(props(&h));
    // CUI: a shortcut of your own, and a command on the Quick Access Toolbar.
    type_all(&mut h, &["cui"]);
    h.run_steps(2);
    let field = |h: &mut Harness<'_, OxidraftApp>, label: &str, text: &str| {
        h.get_by_role_and_label(Role::TextInput, label).click();
        h.run_steps(1);
        h.get_by_role_and_label(Role::TextInput, label).type_text(text);
        h.run_steps(1);
    };
    field(&mut h, "Shortcut keys", "Ctrl+Shift+J");
    field(&mut h, "Shortcut command", "circle");
    h.get_by_label("Add Shortcut").click();
    h.run_steps(1);
    field(&mut h, "Toolbar command", "matchprop");
    h.get_by_label("Add to Toolbar").click();
    h.run_steps(1);
    // A ribbon panel on a tab of its own, and a command on the shortcut menu.
    field(&mut h, "Panel tab", "Tools");
    field(&mut h, "Panel name", "Mine");
    field(&mut h, "Panel commands", "line, circle|arc");
    h.get_by_label("Add Panel").click();
    h.run_steps(1);
    field(&mut h, "Menu command", "zoom");
    h.get_by_label("Add to Menu").click();
    h.run_steps(1);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.query_by_label("matchprop").is_some(), "on the Quick Access Toolbar");
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::J);
    h.run_steps(2);
    assert!(h.state_mut().engine_mut().is_active(), "the shortcut started CIRCLE");
    h.key_press(Key::Escape);
    h.run_steps(2);
    // The Tools tab: LINE as the ribbon's own tool, CIRCLE with ARC in its drop-down.
    h.get_by_label("Tools").click();
    h.run_steps(2);
    h.get_by_label("Line").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("LINE"));
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.get_by_label("More like CIRCLE").click();
    h.run_steps(2);
    h.get_by_label("ARC").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("ARC"));
}

#[test]
fn customization_files_and_macros() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-cuix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    h.state_mut().keep_settings_in(dir.join("settings.json"));
    // A partial customization: a ribbon tab with a panel, a shortcut, and
    // a button image of its own.
    let menu = r#"<?xml version="1.0" encoding="utf-8"?>
<CustSection>
  <MenuGroup Name="SITE">
    <MacroGroup Name="SITE">
      <MenuMacro UID="MM_TREE"><Macro type="Any"><Name>Tree</Name><Command>^C^C_CIRCLE;\2;</Command>
        <SmallImage Name="tree.png"/></Macro></MenuMacro>
      <MenuMacro UID="MM_BOX"><Macro type="Any"><Name>Box</Name><Command>^C^C_LINE;200,200;220,200;220,220;;</Command></Macro></MenuMacro>
      <MenuMacro UID="MM_KERB"><Macro type="Any"><Name>Kerb</Name><Command>^C^C_PLINE</Command></Macro></MenuMacro>
    </MacroGroup>
    <MenuRoot><AcceleratorRoot>
      <Accelerator UID="A1" MenuMacroID="MM_KERB"><AccelShortcutKey>CTRL+SHIFT+K</AccelShortcutKey></Accelerator>
    </AcceleratorRoot></MenuRoot>
  </MenuGroup>
  <RibbonRoot>
    <RibbonPanelSourceCollection>
      <RibbonPanelSource UID="P1" Text="Site Works"><RibbonRow>
        <RibbonCommandButton MenuMacroID="MM_TREE"/><RibbonCommandButton MenuMacroID="MM_BOX"/>
      </RibbonRow></RibbonPanelSource>
    </RibbonPanelSourceCollection>
    <RibbonTabSourceCollection>
      <RibbonTabSource UID="T1" Text="Landscape"><RibbonPanelSourceReference PanelId="P1"/></RibbonTabSource>
    </RibbonTabSourceCollection>
  </RibbonRoot>
</CustSection>"#;
    let png = oxidraft_plot::png::png(
        &oxidraft_plot::Picture { width: 2, height: 2, rgba: [0, 128, 0, 255].repeat(4) },
        false,
    );
    let cuix = dir.join("site.cuix");
    std::fs::write(
        &cuix,
        oxidraft_plot::zip::zip(&[("MenuGroup.cui".into(), menu.as_bytes().to_vec()), ("images/tree.png".into(), png)]),
    )
    .unwrap();
    assert_eq!(h.state_mut().load_customization(&cuix).unwrap(), "SITE");
    assert!(dir.join("cui/SITE/tree.png").is_file(), "its image kept with the settings");
    h.run_steps(2);
    // Its tab, and its buttons running their macros.
    h.get_by_label("Landscape").click();
    h.run_steps(2);
    h.get_by_label("Tree").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("CIRCLE"), "waiting at the macro's pause");
    // The center given, the macro goes on: a radius of 2.
    type_all(&mut h, &["150,150"]);
    h.run_steps(2);
    assert!(!h.state_mut().engine_mut().is_active());
    let circle = h.state_mut().document_mut().entities().find_map(|e| match e.geometry {
        Geometry::Circle { center, radius } => Some((center, radius)),
        _ => None,
    });
    assert_eq!(circle, Some((Point::new(150.0, 150.0), 2.0)));
    let before = h.state_mut().document_mut().len();
    h.get_by_label("Box").click();
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().len(), before + 2, "two lines, and Enter ended LINE");
    assert!(!h.state_mut().engine_mut().is_active());
    let ends: Vec<Point> = h
        .state_mut()
        .document_mut()
        .entities()
        .filter_map(|e| match e.geometry {
            Geometry::Line { end, .. } => Some(end),
            _ => None,
        })
        .collect();
    assert!(ends.contains(&Point::new(220.0, 220.0)), "the points as written: {ends:?}");
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::K);
    h.run_steps(2);
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("PLINE"), "its shortcut");
    h.key_press(Key::Escape);
    h.run_steps(2);
    // CUILOAD's dialog: the group listed, and unloaded.
    type_all(&mut h, &["cuiload"]);
    h.run_steps(2);
    assert!(h.query_by_label("Load/Unload Customizations").is_some());
    h.get_by_label("Unload").click();
    h.run_steps(2);
    assert!(h.query_by_label("Landscape").is_none(), "its tab gone");
    assert!(h.state().customization_groups().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sheet_sets_share_and_markup() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-sheetset-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("plans")).unwrap();
    // A drawing with two layouts, beside the set.
    let mut plan = Document::new();
    plan.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(50.0, 20.0) });
    let paper = oxidraft_model::Paper { width: 420.0, height: 297.0, millimeters: true, ..Default::default() };
    for (i, name) in ["Plan", "Details"].iter().enumerate() {
        let layout = oxidraft_model::layout::Layout {
            id: 0,
            name: (*name).into(),
            tab_order: i as i32 + 1,
            paper: paper.clone(),
        };
        let space = plan.add_layout(layout);
        plan.set_space(space);
        plan.add(Geometry::Circle { center: Point::new(100.0, 100.0), radius: 20.0 });
        // A named view on the Details sheet: a sheet view.
        if *name == "Details" {
            let view = oxidraft_model::NamedView {
                name: "Detail A".into(),
                center: Point::new(100.0, 100.0),
                height: 50.0,
                width: 80.0,
                space,
            };
            plan.save_view(view).unwrap();
        }
        plan.set_space(oxidraft_model::MODEL);
    }
    let drawing = dir.join("plans").join("plan.dxf");
    oxidraft_io_dwg::save_dxf(&plan, &drawing).unwrap();
    // SHEETSET: a new set, the drawing's layouts its sheets.
    let set = dir.join("Project.oxss");
    let app = h.state_mut();
    app.new_sheet_set(&set, "Project").unwrap();
    assert_eq!(app.add_sheets_from(&drawing), Ok(2));
    let saved = std::fs::read_to_string(&set).unwrap();
    assert!(saved.contains("plans/plan.dxf") && saved.contains("Details"), "{saved}");
    // Published: a page a sheet.
    let pdf = dir.join("Project.pdf");
    assert_eq!(app.publish_sheet_set(&pdf), Ok(2));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
    // A sheet selection publishes its sheets, and is kept in the set's file.
    app.tick_sheets(&[1]);
    assert_eq!(app.publish_sheet_set(&pdf), Ok(1));
    app.save_sheet_selection("Details only").unwrap();
    app.tick_sheets(&[]);
    assert_eq!(app.publish_sheet_set(&pdf), Ok(2), "none ticked: all");
    app.choose_sheet_selection("Details only");
    assert_eq!(app.publish_sheet_set(&pdf), Ok(1));
    app.tick_sheets(&[]);
    assert!(std::fs::read_to_string(&set).unwrap().contains("Details only"));
    // Sheet views: the named views on the sheets' layouts.
    let views = app.sheet_views().unwrap();
    assert_eq!(views.iter().map(|v| (v.sheet, v.name.as_str())).collect::<Vec<_>>(), [(1, "Detail A")]);
    // A subset, kept with the sheet.
    app.sheet_set_mut().unwrap().sheets[1].subset = "Details".into();
    app.save_sheet_selection("Details only").unwrap();
    assert!(std::fs::read_to_string(&set).unwrap().contains("\"subset\": \"Details\""));
    // A custom property, in a field.
    app.sheet_set_mut().unwrap().properties.push(("Client".into(), "Ada".into()));
    let note = app.document_mut().add(Geometry::Text {
        look: Default::default(),
        position: Point::new(0.0, -20.0),
        height: 2.5,
        rotation: 0.0,
        width: 0.0,
        attach: oxidraft_model::annotation::Attach::BottomLeft,
        content: "For %<\\AcSm SheetSet.Client>% (%<\\AcSm SheetSet.Name>%)".into(),
    });
    h.run();
    let doc = h.state_mut().document_mut();
    let shown = doc.expanded(doc.entity(note).unwrap());
    assert!(
        shown.iter().any(|g| matches!(g, Geometry::Text { content, .. } if content == "For Ada (Project)")),
        "{shown:?}"
    );
    // The Sheet List Table, placed.
    let app = h.state_mut();
    app.insert_sheet_list().unwrap();
    app.type_command("200,200");
    let table = app.document_mut().entities().find_map(|e| match &e.geometry {
        Geometry::Table { cells, .. } => Some(cells.clone()),
        _ => None,
    });
    let cells = table.expect("a table");
    assert!(
        cells.iter().any(|r| r.iter().any(|c| c == "Sheet Number"))
            && cells.iter().any(|r| r.contains(&"Details".to_string())),
        "{cells:?}"
    );
    let app = h.state_mut();
    // ARCHIVE: the set, its drawing and a report.
    let archive = dir.join("archive");
    let archived = app.archive_sheet_set(&archive);
    let report = std::fs::read_to_string(archive.join("archive report.txt")).unwrap_or_default();
    assert_eq!(archived, Ok(2), "{report}");
    assert!(archive.join("Project.oxss").is_file() && archive.join("plans/plan.dxf").is_file());
    assert!(archive.join("archive report.txt").is_file());
    // SHARE: the drawing as DWG and a PDF of its sheets, in one zip.
    let package = dir.join("shared.zip");
    assert_eq!(app.share_to(&package), Ok(2));
    let zip = std::fs::read(&package).unwrap();
    assert!(zip.starts_with(b"PK\x03\x04"));
    let text = String::from_utf8_lossy(&zip);
    assert!(text.contains("Drawing1.dwg") && text.contains("Drawing1.pdf"));
    // MARKUP: a plot laid back over its sheet, on layer MARKUP.
    let plotted = dir.join("plot.pdf");
    type_all(&mut h, &["plot"]);
    h.run_steps(2);
    h.state_mut().plot_to(&plotted).unwrap();
    h.state_mut().markup_import_file(&plotted).unwrap();
    let doc = h.state_mut().document_mut();
    let markup = doc.entities().find(|e| e.layer == "MARKUP").expect("the markup");
    let Geometry::Image { origin, u, v, .. } = &markup.geometry else { panic!() };
    assert!(u.length() > 0.0 && v.length() > 0.0 && origin.x.is_finite());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn isolation_addselected_and_more_lt_commands() {
    let mut h = app_with_line();
    let ids: Vec<_> = h.state_mut().document_mut().entities().map(|e| e.id).collect();
    // ISOLATEOBJECTS: only the first line shows (and can be picked).
    h.state_mut().engine_mut().set_selection(vec![ids[0]]);
    type_all(&mut h, &["isolateobjects"]);
    assert_eq!(h.state_mut().document_mut().hidden_count(), 1);
    let other = screen(&h, 50.0, 60.0);
    click(&mut h, other);
    assert!(h.state_mut().engine_mut().selection().is_empty(), "a hidden object isn't picked");
    type_all(&mut h, &["unisolateobjects"]);
    assert_eq!(h.state_mut().document_mut().hidden_count(), 0);
    // ADDSELECTED: LINE starts, on the picked line's layer.
    h.state_mut().document_mut().add_layer(oxidraft_model::Layer::new("Walls", 3));
    let wall = ids[1];
    h.state_mut().document_mut().update(wall, |e| e.layer = "Walls".into());
    type_all(&mut h, &["addselected"]);
    let at = screen(&h, 50.0, 60.0);
    click(&mut h, at);
    h.run_steps(2);
    assert_eq!(h.state_mut().document_mut().current_layer(), "Walls");
    assert!(h.state_mut().engine_mut().is_active(), "LINE is running");
    h.key_press(Key::Escape);
    h.run_steps(1);
    // LEADER: three points, then two lines of text.
    type_all(&mut h, &["leader", "0,-20", "10,-10", "20,-10", "", "FIRST", "SECOND", ""]);
    let leader = h.state_mut().document_mut().entities().find_map(|e| match &e.geometry {
        Geometry::Leader { points, content, .. } => Some((points.len(), content.clone())),
        _ => None,
    });
    assert_eq!(leader, Some((3, "FIRST\\PSECOND".to_string())));
    // RIBBONCLOSE and RIBBON.
    type_all(&mut h, &["ribbonclose"]);
    h.run_steps(1);
    assert!(h.query_by_label("Home").is_none());
    type_all(&mut h, &["ribbon"]);
    h.run_steps(1);
    assert!(h.query_by_label("Home").is_some());
}

#[test]
fn urls_views_web_pages_and_customization_files() {
    let mut h = app_with_line();
    let first = h.state_mut().document_mut().entities().next().unwrap().id;
    // ATTACHURL, then DETACHURL.
    h.state_mut().engine_mut().set_selection(vec![first]);
    type_all(&mut h, &["attachurl", "https://example.com/plan"]);
    let link = |h: &mut Harness<'_, OxidraftApp>| {
        h.state_mut().document_mut().entity(first).and_then(|e| e.hyperlink.as_ref().map(|l| l.url.clone()))
    };
    assert_eq!(link(&mut h).as_deref(), Some("https://example.com/plan"));
    h.state_mut().engine_mut().set_selection(vec![first]);
    type_all(&mut h, &["detachurl"]);
    assert_eq!(link(&mut h), None);
    // APERTURE: snaps reach 20 pixels.
    type_all(&mut h, &["aperture", "20"]);
    type_all(&mut h, &["aperture"]);
    assert!(
        h.state_mut().engine_mut().history().iter().any(|l| l.contains("APERTURE") || l.contains("Object snap target"))
    );
    // PUBLISHTOWEB: a page with an image of each sheet.
    let dir = std::env::temp_dir().join(format!("oxidraft-web-{}", std::process::id()));
    let n = h.state_mut().publish_to_web_in(&dir).unwrap();
    assert!(n >= 1);
    let html = std::fs::read_to_string(dir.join("index.html")).unwrap();
    assert!(html.contains("sheet1.png") && dir.join("sheet1.png").is_file());
    // WMFIN: a metafile written by WMFOUT comes back in as objects.
    let ids: Vec<_> = h.state_mut().document_mut().entities().map(|e| e.id).collect();
    let wmf = dir.join("lines.wmf");
    h.state_mut().wmf_out_to(&wmf, &ids).unwrap();
    let before = h.state_mut().document_mut().len();
    assert!(h.state_mut().wmf_in_file(&wmf).unwrap() >= 2);
    assert!(h.state_mut().document_mut().len() >= before + 2);
    // CUIIMPORT: shortcuts from a file.
    let cui = dir.join("mine.cui.json");
    std::fs::write(&cui, r#"{"shortcuts":[["Ctrl+Shift+K","CIRCLE"]],"quick_access":["LINE"]}"#).unwrap();
    h.state_mut().cui_import_from(&cui).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("line").is_some(), "LINE on the Quick Access Toolbar");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn automatic_saves_and_drawing_recovery() {
    let mut h = app_with_line();
    let path = h.state_mut().autosave_now().expect("an automatic save");
    assert!(path.is_file() && path.extension().is_some_and(|e| e == "sv$"));
    type_all(&mut h, &["drawingrecovery"]);
    h.run_steps(2);
    assert!(h.query_by_label("Drawing Recovery Manager").is_some());
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(h.get_all_by_label_contains(&name).next().is_some(), "the autosave is listed");
    std::fs::remove_file(&path).ok();
}

#[test]
fn slides_made_and_shown() {
    let mut h = app_with_line();
    let slide = h.state().view_slide();
    assert!(slide.vectors.len() >= 2, "the two lines");
    let path = std::env::temp_dir().join(format!("oxidraft-{}.sld", std::process::id()));
    std::fs::write(&path, oxidraft_plot::slide::write_sld(&slide)).unwrap();
    h.state_mut().show_slide(&path).unwrap();
    h.run_steps(1);
    assert!(h.state().showing_slide());
    type_all(&mut h, &["redraw"]);
    assert!(!h.state().showing_slide(), "REDRAW puts the drawing back");
    std::fs::remove_file(&path).ok();
}

#[test]
fn data_linked_tables_place_and_update() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-datalink-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("Doors.csv");
    std::fs::write(&csv, "Mark,Width\nD1,900\n").unwrap();
    h.state_mut().data_link_file(&csv).unwrap();
    h.run_steps(1);
    let at = screen(&h, 20.0, 40.0);
    click(&mut h, at);
    h.run_steps(2);
    let table = |h: &mut Harness<'_, OxidraftApp>| {
        h.state_mut().document_mut().entities().find_map(|e| match &e.geometry {
            Geometry::Table { cells, .. } => Some((cells.clone(), e.hyperlink.as_ref().map(|l| l.description.clone()))),
            _ => None,
        })
    };
    let (cells, link) = table(&mut h).expect("the table was placed");
    assert_eq!(cells[0][0], "Doors");
    assert_eq!(cells[2], ["D1", "900"]);
    assert_eq!(link.as_deref(), Some("Data link"));
    // The spreadsheet changes; DATALINKUPDATE brings it in.
    std::fs::write(&csv, "Mark,Width\nD1,900\nD2,1000\n").unwrap();
    assert_eq!(h.state_mut().data_link_update(), 1);
    let (cells, _) = table(&mut h).unwrap();
    assert_eq!(cells.len(), 4);
    assert_eq!(cells[3], ["D2", "1000"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dwfx_sheets_attach_as_blocks() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-dwfattach-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dwfx = dir.join("Plan.dwfx");
    assert_eq!(h.state_mut().export_to_with(&dwfx, false, oxidraft_plot::Plotter::Dwfx), Ok(1));
    let before = h.state_mut().document_mut().len();
    let n = h.state_mut().dwf_attach_file(&dwfx).unwrap();
    assert!(n >= 2, "the two lines at least");
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.len(), before + 1);
    assert!(doc.block("DWF-Plan").is_some_and(|b| b.entities.len() == n));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dgn_files_import_and_attach() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-dgn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut site = Document::new();
    site.add(Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(30.0, 40.0) });
    site.add(Geometry::Circle { center: Point::new(5.0, 5.0), radius: 2.0 });
    let file = dir.join("site.dgn");
    std::fs::write(&file, oxidraft_app_dgn(&site)).unwrap();
    let before = h.state_mut().document_mut().len();
    assert_eq!(h.state_mut().dgn_import_file(&file), Ok(2));
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.len(), before + 2);
    assert!(doc.layer("Level 1").is_some(), "a layer for the level");
    std::fs::remove_dir_all(&dir).ok();
}

fn oxidraft_app_dgn(doc: &Document) -> Vec<u8> {
    oxidraft_app::dgn_bytes(doc)
}

#[test]
fn autolisp_at_the_command_line_and_from_a_file() {
    let mut h = app_with_line();
    let history = |h: &mut Harness<'_, OxidraftApp>| h.state_mut().engine_mut().history().to_vec();
    // Spaces inside parentheses are spaces; once they're closed, Space is Enter.
    type_keys(&mut h, "(setq p (list 10 20))");
    assert_eq!(h.state_mut().command_text(), "(setq p (list 10 20))");
    type_keys(&mut h, " ");
    h.run();
    assert!(history(&mut h).iter().any(|l| l == "(10 20)"), "its value is shown");
    // !p at a prompt gives the point.
    let before = h.state_mut().document_mut().len();
    h.state_mut().type_command("LINE");
    h.state_mut().type_command("!p");
    h.state_mut().type_command("(list 40 60)");
    h.state_mut().type_command("");
    h.run();
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.len(), before + 1);
    let last = doc.entities().last().unwrap();
    assert!(
        matches!(last.geometry, Geometry::Line { start, end } if start == Point::new(10.0, 20.0) && end == Point::new(40.0, 60.0))
    );

    // A file defining a command that runs Oxidraft's, with "_." names.
    let dir = std::env::temp_dir().join(format!("oxidraft-lisp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("square.lsp");
    std::fs::write(
        &file,
        "; a square\n(defun c:SQUARE (/ s)\n  (setq s (getvar \"FILLETRAD\"))\n  (command \"_.PLINE\" '(0 0) (list s 0) (list s s) (list 0 s) \"_c\")\n  (princ))\n",
    )
    .unwrap();
    h.state_mut().load_lisp(&file);
    assert!(history(&mut h).iter().any(|l| l == "square.lsp successfully loaded."));
    h.state_mut().type_command("(setvar \"FILLETRAD\" 5)");
    let before = h.state_mut().document_mut().len();
    h.state_mut().type_command("square");
    h.run();
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.len(), before + 1, "SQUARE drew a polyline");
    match &doc.entities().last().unwrap().geometry {
        Geometry::Polyline { vertices, closed, .. } => {
            assert!(*closed);
            assert_eq!(vertices.len(), 4);
            assert_eq!(vertices[2].point, Point::new(5.0, 5.0));
        }
        g => panic!("not a polyline: {g:?}"),
    }
    // Errors are reported, and cancel what the program left running.
    h.state_mut().type_command("(car 5)");
    assert!(history(&mut h).iter().any(|l| l.starts_with("; error: bad argument type")));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn autolisp_programs_ask_the_user_and_change_objects() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-lisp-ask-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tools.lsp");
    std::fs::write(
        &file,
        r#"(defun c:RING (/ c r)
  (setq c (getpoint "Center: "))
  (setq r (getdist c "Radius: "))
  (command "_.CIRCLE" c r)
  (princ))
(defun c:REDDEN (/ ss i e)
  (if (setq ss (ssget '((0 . "LINE"))))
    (repeat (setq i (sslength ss))
      (setq e (entget (ssname ss (setq i (1- i)))))
      (entmod (subst (cons 8 "Red") (assoc 8 e) e))))
  (princ))
(defun c:TWOPT () (command "_.LINE" pause pause "") (princ))
"#,
    )
    .unwrap();
    h.state_mut().load_lisp(&file);
    let circles = |h: &mut Harness<'_, OxidraftApp>| {
        h.state_mut()
            .document_mut()
            .entities()
            .filter_map(|e| match e.geometry {
                Geometry::Circle { center, radius } => Some((center, radius, e.layer.clone())),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // getpoint and getdist wait for what's typed.
    h.state_mut().type_command("ring");
    assert_eq!(h.state_mut().engine_mut().prompt(), "Center:");
    h.state_mut().type_command("50,50");
    assert_eq!(h.state_mut().engine_mut().prompt(), "Radius:");
    h.state_mut().type_command("5");
    h.run();
    assert_eq!(circles(&mut h), [(Point::new(50.0, 50.0), 5.0, "0".to_string())]);

    // ssget with a filter, then entget, subst and entmod.
    h.state_mut().type_command("redden");
    h.state_mut().type_command("all");
    h.state_mut().type_command("");
    h.run();
    let doc = h.state_mut().document_mut();
    assert!(doc.entities().filter(|e| matches!(e.geometry, Geometry::Line { .. })).all(|e| e.layer == "Red"));
    assert!(doc.layer("Red").is_some(), "entmod made the layer");
    assert_eq!(circles(&mut h)[0].2, "0", "the filter left the circle out");

    // PAUSE waits for each point.
    let before = h.state_mut().document_mut().len();
    h.state_mut().type_command("twopt");
    h.state_mut().type_command("0,0");
    h.state_mut().type_command("10,10");
    h.run();
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.len(), before + 1);
    assert!(
        matches!(doc.entities().last().unwrap().geometry, Geometry::Line { end, .. } if end == Point::new(10.0, 10.0))
    );
    assert!(!h.state_mut().engine_mut().is_active(), "the empty string ended LINE");

    // entmake, and ssget "X" with a filter.
    h.state_mut().type_command("(entmake '((0 . \"CIRCLE\") (8 . \"Holes\") (10 1 1 0) (40 . 2.0)))");
    h.state_mut().type_command("(setq n (sslength (ssget \"X\" '((0 . \"CIRCLE\")))))");
    h.run();
    let history = h.state_mut().engine_mut().history().to_vec();
    assert_eq!(history.last().map(String::as_str), Some("2"));
    assert!(circles(&mut h).iter().any(|c| c.2 == "Holes"));

    // Esc at a prompt cancels the program.
    h.state_mut().type_command("ring");
    h.key_press(Key::Escape);
    h.run();
    let history = h.state_mut().engine_mut().history().to_vec();
    assert!(history.iter().any(|l| l == "; error: Function cancelled"), "{history:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mtext_with_inline_fonts_and_colors_draws() {
    let mut h = app_with_line();
    let doc = h.state_mut().document_mut();
    doc.add(Geometry::Text {
        look: Default::default(),
        position: Point::new(10.0, 30.0),
        height: 5.0,
        rotation: 0.0,
        width: 0.0,
        attach: oxidraft_model::annotation::Attach::TopLeft,
        content: "{\\fArial|b1|i0;Bold} {\\fNo Such Font|i1;\\H2x;\\C1;\\Lbig\\l} \\Kgone\\k".into(),
    });
    // The inline fonts load over a frame or two; drawing meanwhile is fine.
    for _ in 0..4 {
        h.run_steps(1);
    }
    h.run();
}

#[test]
fn text_boxes_are_measured_in_the_font() {
    let mut h = app_with_line();
    h.run_steps(1);
    let doc = h.state_mut().document_mut();
    let text = |content: &str| Geometry::Text {
        look: Default::default(),
        position: Point::new(0.0, 100.0),
        height: 10.0,
        rotation: 0.0,
        width: 0.0,
        attach: oxidraft_model::annotation::Attach::BottomLeft,
        content: content.into(),
    };
    let wide = doc.add(text("WWWW"));
    let narrow = doc.add(text("iiii"));
    let mut width = |id| {
        let doc = h.state_mut().document_mut();
        let b = doc.entity_bounds(doc.entity(id).unwrap());
        b.max.x - b.min.x
    };
    let (w, i) = (width(wide), width(narrow));
    assert!(w > 2.0 * i, "W is wider than i: {w} and {i}");
}

#[test]
fn autolisp_asks_within_a_command_and_reads_the_mouse() {
    let mut h = app_with_line();
    let history = |h: &mut Harness<'_, OxidraftApp>| h.state_mut().engine_mut().history().to_vec();
    // (getpoint) at LINE's prompt: LINE goes on with the point.
    let before = h.state_mut().document_mut().len();
    h.state_mut().type_command("LINE");
    h.state_mut().type_command("0,0");
    h.state_mut().type_command("(getpoint)");
    h.state_mut().type_command("10,5");
    assert_eq!(h.state_mut().engine_mut().active_name(), Some("LINE"), "{:?}", history(&mut h));
    h.state_mut().type_command("");
    h.run();
    let doc = h.state_mut().document_mut();
    assert_eq!(doc.len(), before + 1);
    assert!(
        matches!(doc.entities().last().unwrap().geometry, Geometry::Line { end, .. } if end == Point::new(10.0, 5.0))
    );
    // grread: a click (here typed), a key.
    h.state_mut().type_command("(setq g (grread))");
    h.state_mut().type_command("5,6");
    h.state_mut().type_command("(setq k (grread))");
    h.state_mut().type_command("a");
    h.state_mut().type_command("(list g k)");
    h.run();
    assert!(history(&mut h).iter().any(|l| l == "((3 (5 6 0)) (2 97))"), "{:?}", history(&mut h));
    // Tracking: the cursor's move answers it.
    h.state_mut().type_command("(setq m (car (grread T)))");
    h.event(Event::PointerMoved(Pos2::new(600.0, 400.0)));
    h.run_steps(2);
    h.event(Event::PointerMoved(Pos2::new(620.0, 410.0)));
    h.run_steps(2);
    h.state_mut().type_command("!m");
    h.run();
    assert_eq!(history(&mut h).last().map(String::as_str), Some("5"), "{:?}", history(&mut h));
}

#[test]
fn autolisp_dcl_dialogs() {
    let mut h = app_with_line();
    let dir = std::env::temp_dir().join(format!("oxidraft-dcl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dcl = dir.join("hello.dcl");
    std::fs::write(
        &dcl,
        r#"hello : dialog { label = "Hello";
            : edit_box { key = "name"; label = "Name:"; value = "World"; }
            : toggle { key = "loud"; label = "Loud"; }
            : popup_list { key = "size"; label = "Size"; }
            : button { key = "greet"; label = "Greet"; }
            : image { key = "pic"; width = 10; height = 2; }
            ok_cancel;
        }"#,
    )
    .unwrap();
    // A slide for the image: one red diagonal.
    let slide = oxidraft_plot::slide::Slide { width: 100, height: 50, vectors: vec![(1, (0, 0), (100, 50))] };
    std::fs::write(dir.join("logo.sld"), oxidraft_plot::slide::write_sld(&slide)).unwrap();
    let lsp = dir.join("hello.lsp");
    std::fs::write(
        &lsp,
        format!(
            r#"(defun c:HELLO (/ id r)
  (setq id (load_dialog "{}"))
  (new_dialog "hello" id)
  (start_list "size") (mapcar 'add_list '("Small" "Large")) (end_list)
  (set_tile "size" "1")
  (start_image "pic") (fill_image 0 0 (setq w (dimx_tile "pic")) (setq ht (dimy_tile "pic")) 1) (vector_image 0 0 w ht 7) (slide_image 0 0 w ht "{}") (end_image)
  (action_tile "greet" "(setq greeted (get_tile \"name\")) (set_tile \"name\" \"Done\")")
  (action_tile "accept" "(setq loud (get_tile \"loud\") size (get_tile \"size\")) (done_dialog 1)")
  (setq r (start_dialog))
  (unload_dialog id)
  (princ (strcat "status " (itoa r) " " greeted " loud " loud " size " size))
  (princ))"#,
            dcl.display().to_string().replace('\\', "/"),
            dir.join("logo.sld").display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    h.state_mut().load_lisp(&lsp);
    h.state_mut().type_command("hello");
    h.run();
    h.get_by_label("Greet").click();
    h.run();
    let name = h.get_by_role_and_label(Role::TextInput, "Name:").accesskit_node().value();
    assert_eq!(name.as_deref(), Some("Done"), "the action set it");
    h.get_by_label("Loud").click();
    h.run();
    h.get_by_label("OK").click();
    h.run();
    let history = h.state_mut().engine_mut().history().to_vec();
    assert!(history.iter().any(|l| l == "status 1 World loud 1 size 1"), "{history:?}");
    h.state_mut().type_command("(list w ht)");
    assert_eq!(h.state_mut().engine_mut().history().last().map(String::as_str), Some("(80 32)"), "the image's pixels");
    assert!(h.query_by_label("Greet").is_none(), "the dialog closed");
    // Cancel: status 0.
    let id_src = format!("(setq id (load_dialog \"{}\"))", dcl.display().to_string().replace('\\', "/"));
    h.state_mut().type_command(&id_src);
    h.state_mut().type_command("(progn (new_dialog \"hello\" id) (setq s (start_dialog)))");
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    h.state_mut().type_command("!s");
    assert_eq!(h.state_mut().engine_mut().history().last().map(String::as_str), Some("0"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn traces_markup_over_the_drawing() {
    let mut h = app_with_line();
    type_all(&mut h, &["traces"]);
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Trace name").click();
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, "Trace name").type_text("Review");
    h.run_steps(1);
    h.get_by_label("Create Trace").click();
    h.run_steps(2);
    assert_eq!(h.state().traces(), ["Review"]);
    assert_eq!(h.state().trace_open(), Some("Review"));
    // Drawn while it's open: in the trace.
    type_all(&mut h, &["line", "0,30", "50,30", ""]);
    let on_trace = |h: &mut Harness<'_, OxidraftApp>| {
        h.state_mut().document_mut().entities().filter(|e| e.layer == "TRACE$Review").count()
    };
    assert_eq!(on_trace(&mut h), 1);
    h.get_by_label("Close Trace").click();
    h.run_steps(2);
    assert_eq!(h.state().trace_open(), None);
    assert_eq!(h.state_mut().document_mut().current_layer(), "0", "back on the layer that was current");
    // Copied into the drawing; hidden; TRACEPAPERCTL.
    assert_eq!(h.state_mut().copy_trace_to_drawing("Review", "0"), 1);
    assert_eq!(h.state_mut().document_mut().entities().filter(|e| e.layer == "0").count(), 3);
    h.state_mut().set_trace_shown("Review", false);
    assert!(h.state_mut().document_mut().layer("TRACE$Review").is_some_and(|l| !l.on));
    type_all(&mut h, &["tracepaperctl", "80"]);
    h.run_steps(1);
    h.state_mut().delete_trace("Review");
    assert!(h.state().traces().is_empty() && on_trace(&mut h) == 0);
}

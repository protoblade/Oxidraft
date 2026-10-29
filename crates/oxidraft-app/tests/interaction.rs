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

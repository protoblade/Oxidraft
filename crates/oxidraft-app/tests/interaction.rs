//! Drives the real app through egui pointer events, the way a user would.

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use oxidraft_app::OxidraftApp;
use oxidraft_geom::Point;
use oxidraft_model::{Document, Geometry};

fn app_with_line() -> Harness<'static, OxidraftApp> {
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_eframe(|cc| {
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

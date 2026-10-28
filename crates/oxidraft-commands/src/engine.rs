use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Document, EntityId, Geometry, snap};

use crate::{
    Command, Ctx, Input, Prompt, Selection, Step, Vars, arc::Arc, block::DefineBlock, block::InsertBlock,
    circle::Circle, erase::Erase, explode::Explode, fillet::Fillet, grip::GripStretch, line::Line,
    matchprop::MatchProp, mirror::Mirror, move_copy::MoveCopy, offset::Offset, parse_input, pline::Pline,
    rectang::Rectang, rotate::Rotate, scale::Scale, stretch::Stretch, trim_extend::TrimExtend,
};

/// A command the user can type, with its aliases.
pub struct CommandInfo {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    /// `None` for commands that run immediately (UNDO, REDO).
    make: Option<fn() -> Box<dyn Command>>,
}

/// Every command Oxidraft knows, in the order `HELP` lists them.
pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        name: "LINE",
        aliases: &["L"],
        description: "Draw line segments",
        make: Some(|| Box::new(Line::default())),
    },
    CommandInfo {
        name: "PLINE",
        aliases: &["PL"],
        description: "Draw a polyline of line and arc segments",
        make: Some(|| Box::new(Pline::default())),
    },
    CommandInfo {
        name: "CIRCLE",
        aliases: &["C"],
        description: "Draw a circle from center and radius",
        make: Some(|| Box::new(Circle::default())),
    },
    CommandInfo {
        name: "ARC",
        aliases: &["A"],
        description: "Draw an arc through three points or from its center",
        make: Some(|| Box::new(Arc::default())),
    },
    CommandInfo {
        name: "RECTANG",
        aliases: &["REC", "RECTANGLE"],
        description: "Draw a rectangular polyline",
        make: Some(|| Box::new(Rectang::default())),
    },
    CommandInfo {
        name: "MOVE",
        aliases: &["M"],
        description: "Move objects by a displacement",
        make: Some(|| Box::new(MoveCopy::moving())),
    },
    CommandInfo {
        name: "COPY",
        aliases: &["CO", "CP"],
        description: "Copy objects by a displacement",
        make: Some(|| Box::new(MoveCopy::copying())),
    },
    CommandInfo {
        name: "ROTATE",
        aliases: &["RO"],
        description: "Rotate objects around a base point",
        make: Some(|| Box::new(Rotate::default())),
    },
    CommandInfo {
        name: "SCALE",
        aliases: &["SC"],
        description: "Enlarge or reduce objects",
        make: Some(|| Box::new(Scale::default())),
    },
    CommandInfo {
        name: "MIRROR",
        aliases: &["MI"],
        description: "Create a mirrored copy of objects",
        make: Some(|| Box::new(Mirror::default())),
    },
    CommandInfo {
        name: "STRETCH",
        aliases: &["S"],
        description: "Stretch objects crossed by a selection window",
        make: Some(|| Box::new(Stretch::default())),
    },
    CommandInfo {
        name: "OFFSET",
        aliases: &["O"],
        description: "Create parallel copies at a distance or through a point",
        make: Some(|| Box::new(Offset::default())),
    },
    CommandInfo {
        name: "TRIM",
        aliases: &["TR"],
        description: "Trim objects at the edges of other objects",
        make: Some(|| Box::new(TrimExtend::trim())),
    },
    CommandInfo {
        name: "EXTEND",
        aliases: &["EX"],
        description: "Extend objects to meet other objects",
        make: Some(|| Box::new(TrimExtend::extend())),
    },
    CommandInfo {
        name: "FILLET",
        aliases: &["F"],
        description: "Round or square the corner between two lines",
        make: Some(|| Box::new(Fillet::default())),
    },
    CommandInfo {
        name: "EXPLODE",
        aliases: &["X"],
        description: "Break polylines into lines and arcs",
        make: Some(|| Box::new(Explode)),
    },
    CommandInfo { name: "ERASE", aliases: &["E"], description: "Delete objects", make: Some(|| Box::new(Erase)) },
    CommandInfo {
        name: "MATCHPROP",
        aliases: &["MA"],
        description: "Copy layer and color from one object to others",
        make: Some(|| Box::new(MatchProp::default())),
    },
    CommandInfo {
        name: "BLOCK",
        aliases: &["B"],
        description: "Define a block from selected objects",
        make: Some(|| Box::new(DefineBlock::default())),
    },
    CommandInfo {
        name: "INSERT",
        aliases: &["I"],
        description: "Insert a block reference",
        make: Some(|| Box::new(InsertBlock::default())),
    },
    // Handled by the app, which owns the windows and palettes.
    CommandInfo { name: "LAYER", aliases: &["LA"], description: "Open the Layer Properties Manager", make: None },
    CommandInfo {
        name: "PROPERTIES",
        aliases: &["PR", "PROPS", "MO", "CH"],
        description: "Show the Properties palette",
        make: None,
    },
    CommandInfo { name: "UNDO", aliases: &["U"], description: "Undo the last command", make: None },
    CommandInfo { name: "REDO", aliases: &[], description: "Redo the last undone command", make: None },
    CommandInfo { name: "HELP", aliases: &["?"], description: "List commands", make: None },
];

/// Grip editing runs as a command but can't be typed.
static GRIP_STRETCH: CommandInfo =
    CommandInfo { name: "STRETCH", aliases: &[], description: "Grip stretch", make: None };

fn find(name: &str) -> Option<&'static CommandInfo> {
    COMMANDS
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name) || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
}

/// Cursor position after ortho/polar tracking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracked {
    pub point: Point,
    /// Base point and angle (degrees) when polar tracking locked on.
    pub polar: Option<(Point, f64)>,
}

struct Active {
    info: &'static CommandInfo,
    cmd: Box<dyn Command>,
}

/// Runs commands against a document, owns the selection set, and keeps the
/// command-line history.
pub struct Engine {
    active: Option<Active>,
    last_command: Option<&'static CommandInfo>,
    last_point: Option<Point>,
    history: Vec<String>,
    /// Commands and inputs typed so far, for Up-arrow recall.
    typed: Vec<String>,
    /// Selected objects: the pickfirst set when idle, or the set being built
    /// at a "Select objects:" prompt.
    selection: Vec<EntityId>,
    /// Windows used to build `selection` (for STRETCH).
    selection_windows: Vec<Aabb>,
    /// First corner of a selection window in progress.
    window_start: Option<Point>,
    ortho: bool,
    polar: bool,
    /// POLARANG: polar tracking increment in degrees.
    pub polar_increment: f64,
    /// DYNMODE: typed coordinates are relative to the last point unless `#` is used.
    pub dynamic_input: bool,
    /// Settings remembered between commands (OFFSETDIST, FILLETRAD, …).
    pub vars: Vars,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// A new engine with AutoCAD's defaults: polar tracking and dynamic input on, ortho off.
    pub fn new() -> Self {
        Self {
            active: None,
            last_command: None,
            last_point: None,
            history: Vec::new(),
            typed: Vec::new(),
            selection: Vec::new(),
            selection_windows: Vec::new(),
            window_start: None,
            ortho: false,
            polar: true,
            polar_increment: 90.0,
            dynamic_input: true,
            vars: Vars::default(),
        }
    }

    // ---- state queries ------------------------------------------------------

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Previously typed inputs, oldest first.
    pub fn typed(&self) -> &[String] {
        &self.typed
    }

    pub fn log(&mut self, line: impl Into<String>) {
        self.history.push(line.into());
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn active_name(&self) -> Option<&'static str> {
        self.active.as_ref().map(|a| a.info.name)
    }

    /// The active command's prompt, or `None` at the `Command:` prompt.
    pub fn current_prompt(&self) -> Option<Prompt> {
        self.active.as_ref().map(|a| a.cmd.prompt())
    }

    /// Text to show before the command-line cursor.
    pub fn prompt(&self) -> String {
        match &self.active {
            Some(a) => a.cmd.prompt().to_string(),
            None => "Command:".into(),
        }
    }

    pub fn preview(&self, cursor: Point) -> Vec<Geometry> {
        self.active.as_ref().map(|a| a.cmd.preview(cursor)).unwrap_or_default()
    }

    /// Objects to draw highlighted: the selection set plus whatever the command adds.
    pub fn highlighted(&self) -> Vec<EntityId> {
        let mut ids = self.selection.clone();
        if let Some(a) = &self.active {
            ids.extend(a.cmd.highlighted());
        }
        ids
    }

    pub fn selection(&self) -> &[EntityId] {
        &self.selection
    }

    /// True when clicks select objects (idle, or at a "Select objects:" prompt)
    /// rather than specify points.
    pub fn is_selecting(&self) -> bool {
        self.active.as_ref().is_none_or(|a| a.cmd.wants_selection())
    }

    /// True at a single-object prompt like "Select object to trim:".
    pub fn is_picking(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.cmd.wants_pick())
    }

    pub fn window_start(&self) -> Option<Point> {
        self.window_start
    }

    /// Point the rubber band starts from, if any.
    pub fn base_point(&self) -> Option<Point> {
        self.active.as_ref().and_then(|a| a.cmd.base_point())
    }

    /// Grips to draw: those of selected objects while no command runs.
    pub fn grips(&self, doc: &Document) -> Vec<Point> {
        if self.active.is_some() {
            return Vec::new();
        }
        self.selection.iter().filter_map(|&id| doc.entity(id)).flat_map(|e| e.geometry.grips()).collect()
    }

    // ---- drafting settings ----------------------------------------------------

    pub fn ortho(&self) -> bool {
        self.ortho
    }

    pub fn polar(&self) -> bool {
        self.polar
    }

    /// ORTHO and POLAR exclude each other, as in AutoCAD.
    pub fn set_ortho(&mut self, on: bool) {
        self.ortho = on;
        if on {
            self.polar = false;
        }
    }

    pub fn set_polar(&mut self, on: bool) {
        self.polar = on;
        if on {
            self.ortho = false;
        }
    }

    /// Applies ORTHO or polar tracking to a free cursor position.
    /// `tolerance` is how far (drawing units) the cursor may be from a polar angle.
    /// Snapped points skip this.
    pub fn track(&self, cursor: Point, tolerance: f64) -> Tracked {
        let free = Tracked { point: cursor, polar: None };
        let Some(base) = self.base_point() else { return free };
        let d = cursor - base;
        if self.ortho {
            let point =
                if d.x.abs() >= d.y.abs() { Point::new(cursor.x, base.y) } else { Point::new(base.x, cursor.y) };
            return Tracked { point, polar: None };
        }
        if !self.polar || d.length() <= tolerance || self.polar_increment <= 0.0 {
            return free;
        }
        let step = self.polar_increment;
        let angle = ((base.angle_to(cursor) / step).round() * step).rem_euclid(360.0);
        let dir = Point::ORIGIN.polar(1.0, angle);
        let along = d.dot(dir);
        let off = (d - dir * along).length();
        if along > 0.0 && off <= tolerance {
            Tracked { point: base + dir * along, polar: Some((base, angle)) }
        } else {
            free
        }
    }

    /// Commands whose name or alias starts with `prefix`, best match first (AutoComplete).
    pub fn suggest(prefix: &str) -> Vec<&'static CommandInfo> {
        let p = prefix.trim().to_ascii_uppercase();
        if p.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<&'static CommandInfo> = Vec::new();
        let exact = COMMANDS.iter().filter(|c| c.name == p || c.aliases.contains(&p.as_str()));
        let by_name = COMMANDS.iter().filter(|c| c.name.starts_with(&p));
        let by_alias = COMMANDS.iter().filter(|c| c.aliases.iter().any(|a| a.starts_with(&p)));
        for c in exact.chain(by_name).chain(by_alias) {
            if !out.iter().any(|o| o.name == c.name) {
                out.push(c);
            }
        }
        out
    }

    // ---- input ------------------------------------------------------------------

    /// Handles text submitted on the command line (Enter or Space).
    pub fn submit(&mut self, text: &str, doc: &mut Document, cursor: Point, pick_tolerance: f64) {
        let text = text.trim();
        if !text.is_empty() {
            self.typed.push(text.to_string());
        }
        let Some(active) = &self.active else {
            self.window_start = None;
            self.log(format!("Command: {text}"));
            let info = if text.is_empty() {
                match self.last_command {
                    Some(c) => c,
                    None => return,
                }
            } else {
                match find(text) {
                    Some(c) => c,
                    None => {
                        self.log(format!("Unknown command \"{}\". Press F1 for help.", text.to_uppercase()));
                        return;
                    }
                }
            };
            self.start(info, doc, cursor, pick_tolerance);
            return;
        };

        let prompt = active.cmd.prompt();
        let selecting = active.cmd.wants_selection();
        if active.cmd.wants_text() {
            self.log(format!("{prompt} {text}"));
            let input = if text.is_empty() { Input::Enter } else { Input::Text(text.to_string()) };
            self.feed(input, doc, cursor, pick_tolerance);
            return;
        }
        let dyn_relative = self.dynamic_input && active.cmd.base_point().is_some();
        self.log(format!("{prompt} {text}"));
        if selecting && text.is_empty() {
            self.window_start = None;
            let selection = self.take_selection();
            self.feed(Input::Selection(selection), doc, cursor, pick_tolerance);
            return;
        }
        match parse_input(text, self.last_point, &prompt.keywords, dyn_relative) {
            Ok(input) => self.feed(input, doc, cursor, pick_tolerance),
            Err(e) => self.log(e),
        }
    }

    /// Handles a click in the drawing area.
    ///
    /// `point` is the snapped/tracked point used when a command wants a point;
    /// `raw` is the bare cursor, used for picking objects and selection windows.
    pub fn click(&mut self, point: Point, raw: Point, shift: bool, doc: &mut Document, pick_tolerance: f64) {
        if self.is_selecting() {
            self.select_click(raw, shift, doc, pick_tolerance);
            return;
        }
        if self.is_picking() {
            self.feed(Input::Pick { at: raw, shift }, doc, raw, pick_tolerance);
            return;
        }
        let Some(active) = &self.active else { return };
        let prompt = active.cmd.prompt();
        self.log(format!("{prompt} {:.4},{:.4}", point.x, point.y));
        self.feed(Input::Point(point), doc, point, pick_tolerance);
    }

    fn select_click(&mut self, p: Point, shift: bool, doc: &mut Document, tolerance: f64) {
        if let Some(first) = self.window_start.take() {
            let ids = snap::window_select(doc, first, p);
            if !shift {
                self.selection_windows.push(Aabb::from_points(first, p));
            }
            self.add_to_selection(ids, shift);
            return;
        }
        if self.active.is_none() && !shift && self.start_grip_edit(p, doc, tolerance) {
            return;
        }
        match snap::pick(doc, p, tolerance) {
            Some(id) => self.add_to_selection(vec![id], shift),
            None => self.window_start = Some(p),
        }
    }

    /// Shift+pick removes from the selection, as in AutoCAD.
    fn add_to_selection(&mut self, ids: Vec<EntityId>, remove: bool) {
        let count = ids.len();
        if remove {
            self.selection.retain(|id| !ids.contains(id));
        } else {
            for id in ids {
                if !self.selection.contains(&id) {
                    self.selection.push(id);
                }
            }
        }
        if self.active.is_some() {
            let verb = if remove { "removed" } else { "found" };
            self.log(format!("{count} {verb}, {} total", self.selection.len()));
        }
    }

    /// Clicking a grip of a selected object makes it hot and starts STRETCH.
    fn start_grip_edit(&mut self, p: Point, doc: &mut Document, tolerance: f64) -> bool {
        let mut hot = None;
        let mut targets = Vec::new();
        for &id in &self.selection {
            let Some(e) = doc.entity(id) else { continue };
            for (i, g) in e.geometry.grips().into_iter().enumerate() {
                let near = hot.map_or(g.distance(p) <= tolerance, |h: Point| g.approx_eq(h));
                if near {
                    hot.get_or_insert(g);
                    targets.push((id, i, e.geometry.clone()));
                    break;
                }
            }
        }
        let Some(base) = hot else { return false };
        // Only the grips at exactly the hot spot move together.
        targets.retain(|(_, i, g)| g.grips()[*i].approx_eq(base));
        doc.begin(GRIP_STRETCH.name);
        self.log("** STRETCH **");
        self.last_point = Some(base);
        self.active = Some(Active { info: &GRIP_STRETCH, cmd: Box::new(GripStretch::new(targets, base)) });
        true
    }

    /// Escape: ends the active command (keeping what it drew, as AutoCAD does),
    /// or drops the selection window, or clears the selection.
    pub fn cancel(&mut self, doc: &mut Document) {
        if self.window_start.take().is_some() {
            return;
        }
        match self.active.take() {
            Some(a) => {
                self.log("*Cancel*");
                doc.commit();
                if !std::ptr::eq(a.info, &GRIP_STRETCH) {
                    self.clear_selection();
                }
            }
            None => self.clear_selection(),
        }
    }

    fn clear_selection(&mut self) {
        self.selection.clear();
        self.selection_windows.clear();
    }

    fn take_selection(&mut self) -> Selection {
        Selection { ids: std::mem::take(&mut self.selection), windows: std::mem::take(&mut self.selection_windows) }
    }

    /// Starts a command by name, as if typed. Returns false if unknown.
    pub fn run(&mut self, name: &str, doc: &mut Document) -> bool {
        let Some(info) = find(name) else { return false };
        if self.active.is_some() {
            self.cancel(doc);
        }
        self.window_start = None;
        self.log(format!("Command: {}", info.name));
        self.start(info, doc, Point::ORIGIN, 0.0);
        true
    }

    fn start(&mut self, info: &'static CommandInfo, doc: &mut Document, cursor: Point, pick_tolerance: f64) {
        self.last_command = Some(info);
        let Some(make) = info.make else {
            self.run_immediate(info.name, doc);
            return;
        };
        doc.begin(info.name);
        let mut cmd = make();
        let wants_selection = cmd.wants_selection();
        if let Some(intro) = cmd.start(&self.vars) {
            self.log(intro);
        }
        self.active = Some(Active { info, cmd });
        if wants_selection {
            // Noun-verb: objects selected before the command are used right away.
            if !self.selection.is_empty() {
                let selection = self.take_selection();
                self.log(format!("{} found", selection.ids.len()));
                self.feed(Input::Selection(selection), doc, cursor, pick_tolerance);
            }
        } else {
            self.clear_selection();
        }
    }

    fn run_immediate(&mut self, name: &str, doc: &mut Document) {
        let msg = match name {
            "UNDO" => doc.undo().unwrap_or_else(|| "Nothing to undo.".into()),
            "REDO" => doc.redo().unwrap_or_else(|| "Nothing to redo.".into()),
            "HELP" => COMMANDS
                .iter()
                .map(|c| {
                    let aliases =
                        if c.aliases.is_empty() { String::new() } else { format!(" ({})", c.aliases.join(", ")) };
                    format!("  {}{} - {}", c.name, aliases, c.description)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => return,
        };
        self.clear_selection();
        for line in msg.lines() {
            self.log(line);
        }
    }

    fn feed(&mut self, input: Input, doc: &mut Document, cursor: Point, pick_tolerance: f64) {
        let Some(active) = &mut self.active else { return };
        if let Input::Point(p) = input {
            self.last_point = Some(p);
        }
        let mut ctx = Ctx { doc, vars: &mut self.vars, cursor, pick_tolerance, messages: Vec::new() };
        let result = active.cmd.input(input, &mut ctx);
        let messages = std::mem::take(&mut ctx.messages);
        if let Some(b) = active.cmd.base_point() {
            self.last_point = Some(b);
        }
        self.history.extend(messages);
        match result {
            Ok(Step::Continue) => {}
            Ok(Step::Done) => {
                self.active = None;
                doc.commit();
            }
            Err(e) => self.log(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_all(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    fn engine() -> Engine {
        // Plain absolute coordinates make the tests easier to read.
        Engine { dynamic_input: false, ..Engine::new() }
    }

    fn click(e: &mut Engine, d: &mut Document, x: f64, y: f64) {
        let p = Point::new(x, y);
        e.click(p, p, false, d, 0.5);
    }

    fn lines(d: &Document) -> Vec<(Point, Point)> {
        d.entities()
            .filter_map(|e| match e.geometry {
                Geometry::Line { start, end } => Some((start, end)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn line_draws_closed_square_and_undoes_as_one_step() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "@0,10", "@-10,0", "c"]);
        assert!(!e.is_active());
        let l = lines(&d);
        assert_eq!(l.len(), 4);
        assert_eq!(l[3], (Point::new(0.0, 10.0), Point::new(0.0, 0.0)));

        type_all(&mut e, &mut d, &["u"]);
        assert!(d.is_empty());
        type_all(&mut e, &mut d, &["redo"]);
        assert_eq!(d.len(), 4);
    }

    #[test]
    fn dynamic_input_makes_next_points_relative() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        type_all(&mut e, &mut d, &["l", "5,5", "10,0", "0,10", "#0,0", ""]);
        assert_eq!(
            lines(&d),
            vec![
                (Point::new(5.0, 5.0), Point::new(15.0, 5.0)),
                (Point::new(15.0, 5.0), Point::new(15.0, 15.0)),
                (Point::new(15.0, 15.0), Point::new(0.0, 0.0)),
            ]
        );
    }

    #[test]
    fn line_undo_option_removes_last_segment() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["LINE", "0,0", "5,0", "5,5", "u", "0,5", ""]);
        assert_eq!(
            lines(&d),
            vec![(Point::new(0.0, 0.0), Point::new(5.0, 0.0)), (Point::new(5.0, 0.0), Point::new(0.0, 5.0))]
        );
    }

    #[test]
    fn direct_distance_entry_follows_cursor() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "1,1"]);
        e.submit("4", &mut d, Point::new(1.0, 100.0), 0.5);
        let (_, end) = lines(&d)[0];
        assert!(end.approx_eq(Point::new(1.0, 5.0)));
    }

    #[test]
    fn enter_repeats_last_command_and_circle_takes_diameter() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["c", "0,0", "3", "", "10,0", "d", "4"]);
        let radii: Vec<f64> = d
            .entities()
            .filter_map(|e| match e.geometry {
                Geometry::Circle { radius, .. } => Some(radius),
                _ => None,
            })
            .collect();
        assert_eq!(radii, vec![3.0, 2.0]);
    }

    #[test]
    fn erase_by_pick_inside_command() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["e"]);
        click(&mut e, &mut d, 5.0, 0.2);
        assert_eq!(e.highlighted().len(), 1);
        type_all(&mut e, &mut d, &[""]);
        assert!(d.is_empty());
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn noun_verb_selection_then_erase() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "0,5", "10,5", ""]);
        // Window from left to right around the first line only.
        click(&mut e, &mut d, -1.0, -1.0);
        assert!(e.window_start().is_some());
        click(&mut e, &mut d, 11.0, 1.0);
        assert_eq!(e.selection().len(), 1);
        type_all(&mut e, &mut d, &["erase"]);
        assert!(!e.is_active(), "pickfirst selection is used immediately");
        assert_eq!(lines(&d), vec![(Point::new(0.0, 5.0), Point::new(10.0, 5.0))]);
    }

    #[test]
    fn crossing_window_selects_touching_objects() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "0,5", "10,5", ""]);
        click(&mut e, &mut d, 6.0, 6.0);
        click(&mut e, &mut d, 4.0, -1.0);
        assert_eq!(e.selection().len(), 2);
        e.cancel(&mut d);
        assert!(e.selection().is_empty());
    }

    #[test]
    fn move_and_copy() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["m"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["", "0,0", "0,10"]);
        assert_eq!(lines(&d), vec![(Point::new(0.0, 10.0), Point::new(10.0, 10.0))]);

        type_all(&mut e, &mut d, &["co"]);
        click(&mut e, &mut d, 5.0, 10.0);
        type_all(&mut e, &mut d, &["", "0,0", "20,0", "40,0", "u", "x"]);
        assert_eq!(d.len(), 2);
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(d.len(), 1, "COPY undoes as one step");
    }

    fn circles(d: &Document) -> Vec<(Point, f64)> {
        d.entities()
            .filter_map(|e| match e.geometry {
                Geometry::Circle { center, radius } => Some((center, radius)),
                _ => None,
            })
            .collect()
    }

    fn approx(a: (Point, Point), b: ((f64, f64), (f64, f64))) -> bool {
        a.0.approx_eq(Point::new(b.0.0, b.0.1)) && a.1.approx_eq(Point::new(b.1.0, b.1.1))
    }

    #[test]
    fn rotate_by_angle_copy_and_reference() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["ro"]);
        assert!(e.history().iter().any(|l| l.contains("ANGDIR=counterclockwise")));
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["", "0,0", "90"]);
        assert!(approx(lines(&d)[0], ((0.0, 0.0), (0.0, 10.0))));

        // Copy keeps the original; Reference rotates by new - reference.
        click(&mut e, &mut d, 0.0, 5.0);
        type_all(&mut e, &mut d, &["ro", "0,0", "c", "r", "30", "0"]);
        let l = lines(&d);
        assert_eq!(l.len(), 2);
        assert!(approx(l[0], ((0.0, 0.0), (0.0, 10.0))));
        assert!(l[1].1.approx_eq(Point::new(0.0, 10.0).rotated_about(Point::ORIGIN, -30.0)));

        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(lines(&d).len(), 1, "ROTATE Copy undoes as one step");
    }

    #[test]
    fn scale_by_factor_and_reference() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["c", "10,0", "2"]);
        type_all(&mut e, &mut d, &["sc"]);
        click(&mut e, &mut d, 12.0, 0.0);
        type_all(&mut e, &mut d, &["", "0,0", "2"]);
        assert_eq!(circles(&d), vec![(Point::new(20.0, 0.0), 4.0)]);

        type_all(&mut e, &mut d, &["sc"]);
        click(&mut e, &mut d, 24.0, 0.0);
        type_all(&mut e, &mut d, &["", "0,0", "r", "4", "2"]);
        assert_eq!(circles(&d), vec![(Point::new(10.0, 0.0), 2.0)]);

        type_all(&mut e, &mut d, &["sc"]);
        click(&mut e, &mut d, 12.0, 0.0);
        type_all(&mut e, &mut d, &["", "0,0", "-1"]);
        assert!(e.is_active(), "negative factors are rejected and re-prompted");
        assert!(e.history().last().unwrap().contains("positive"));
    }

    #[test]
    fn mirror_keeps_or_erases_source() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "1,1", "5,3", ""]);
        type_all(&mut e, &mut d, &["mi"]);
        click(&mut e, &mut d, 3.0, 2.0);
        type_all(&mut e, &mut d, &["", "0,0", "10,0", ""]);
        let l = lines(&d);
        assert_eq!(l.len(), 2);
        assert!(approx(l[1], ((1.0, -1.0), (5.0, -3.0))));

        type_all(&mut e, &mut d, &["mi"]);
        click(&mut e, &mut d, 3.0, -2.0);
        type_all(&mut e, &mut d, &["", "0,0", "0,10", "y"]);
        let l = lines(&d);
        assert_eq!(l.len(), 2, "Yes erases the source");
        assert!(approx(l[1], ((-1.0, -1.0), (-5.0, -3.0))));
    }

    #[test]
    fn stretch_moves_points_inside_crossing_window() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "10,10", ""]);
        type_all(&mut e, &mut d, &["s"]);
        assert_eq!(e.active_name(), Some("STRETCH"));
        // Crossing window (right to left) around the shared corner only.
        click(&mut e, &mut d, 12.0, -2.0);
        click(&mut e, &mut d, 8.0, 2.0);
        assert_eq!(e.selection().len(), 2);
        type_all(&mut e, &mut d, &["", "0,0", "5,0"]);
        assert_eq!(
            lines(&d),
            vec![(Point::new(0.0, 0.0), Point::new(15.0, 0.0)), (Point::new(15.0, 0.0), Point::new(10.0, 10.0))]
        );
    }

    #[test]
    fn stretch_without_a_window_changes_nothing() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["s"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &[""]);
        assert!(!e.is_active());
        assert_eq!(lines(&d), vec![(Point::new(0.0, 0.0), Point::new(10.0, 0.0))]);
    }

    #[test]
    fn stretch_uses_preselected_crossing_window() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        click(&mut e, &mut d, 12.0, -2.0);
        click(&mut e, &mut d, 8.0, 2.0);
        type_all(&mut e, &mut d, &["stretch", "0,0", "0,5"]);
        assert_eq!(lines(&d), vec![(Point::new(0.0, 0.0), Point::new(10.0, 5.0))]);
    }

    fn shift_click(e: &mut Engine, d: &mut Document, x: f64, y: f64) {
        let p = Point::new(x, y);
        e.click(p, p, true, d, 0.5);
    }

    fn only(d: &Document) -> Geometry {
        assert_eq!(d.len(), 1, "expected exactly one object");
        d.entities().next().unwrap().geometry.clone()
    }

    fn points(g: &Geometry) -> Vec<Point> {
        match g {
            Geometry::Polyline { vertices, .. } => vertices.iter().map(|v| v.point).collect(),
            _ => panic!("expected a polyline, got {g:?}"),
        }
    }

    #[test]
    fn pline_with_tangent_arc_and_close() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["pl", "0,0", "10,0", "a", "10,10", "l", "0,10", "c"]);
        let Geometry::Polyline { vertices, closed } = only(&d) else { panic!() };
        assert!(closed);
        assert_eq!(vertices.len(), 4);
        // Heading east then turning back west: a counterclockwise half circle (bulge 1).
        assert!((vertices[1].bulge - 1.0).abs() < 1e-9);
        assert!(vertices.iter().enumerate().all(|(i, v)| i == 1 || v.bulge == 0.0));
        type_all(&mut e, &mut d, &["u"]);
        assert!(d.is_empty(), "PLINE undoes as one step");
    }

    #[test]
    fn pline_undo_and_escape_keep_drawn_segments() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["pl", "0,0", "5,0", "5,5", "u"]);
        assert_eq!(points(&only(&d)), vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0)]);
        type_all(&mut e, &mut d, &["0,5"]);
        e.cancel(&mut d);
        assert_eq!(points(&only(&d)).len(), 3, "Esc keeps the polyline drawn so far");
    }

    #[test]
    fn arc_three_points_center_and_angle() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["a", "1,0", "0,1", "-1,0"]);
        let Geometry::Arc { center, radius, start_angle, end_angle } = only(&d) else { panic!() };
        assert!(center.approx_eq(Point::ORIGIN) && (radius - 1.0).abs() < 1e-9);
        assert!(start_angle.abs() < 1e-9 && (end_angle - 180.0).abs() < 1e-9);

        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["arc", "c", "0,0", "2,0", "a", "-90"]);
        let Geometry::Arc { start_angle, end_angle, .. } = only(&d) else { panic!() };
        assert!((start_angle - 270.0).abs() < 1e-9 && end_angle.abs() < 1e-9, "negative angle is clockwise");
    }

    #[test]
    fn rectang_by_corners_and_dimensions() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["rec", "0,0", "10,5"]);
        assert_eq!(
            points(&only(&d)),
            vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 5.0), Point::new(0.0, 5.0)]
        );
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["rectang", "0,0", "d", "4", "3"]);
        click(&mut e, &mut d, -1.0, -1.0);
        assert!(points(&only(&d)).contains(&Point::new(-4.0, -3.0)), "the click picks the quadrant");
    }

    #[test]
    fn offset_by_distance_is_remembered_and_through_point() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["o", "2"]);
        assert!(e.is_picking());
        click(&mut e, &mut d, 5.0, 0.0);
        click(&mut e, &mut d, 5.0, -1.0);
        type_all(&mut e, &mut d, &[""]);
        assert!(lines(&d).contains(&(Point::new(0.0, -2.0), Point::new(10.0, -2.0))));

        type_all(&mut e, &mut d, &["o"]);
        assert!(e.prompt().contains("<2.0000>"), "OFFSETDIST is remembered: {}", e.prompt());
        type_all(&mut e, &mut d, &["t"]);
        click(&mut e, &mut d, 5.0, 0.0);
        click(&mut e, &mut d, 5.0, 7.0);
        assert!(lines(&d).contains(&(Point::new(0.0, 7.0), Point::new(10.0, 7.0))));
        type_all(&mut e, &mut d, &["u", ""]);
        assert_eq!(d.len(), 2, "Undo removes the last offset");
    }

    #[test]
    fn trim_quick_mode_with_undo_and_shift_extend() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "5,-5", "5,5", "", "l", "0,3", "3,3", ""]);
        type_all(&mut e, &mut d, &["tr"]);
        click(&mut e, &mut d, 8.0, 0.0);
        assert_eq!(lines(&d)[0], (Point::new(0.0, 0.0), Point::new(5.0, 0.0)));
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(lines(&d)[0], (Point::new(0.0, 0.0), Point::new(10.0, 0.0)), "Undo restores the last trim");
        // Shift-click extends instead.
        shift_click(&mut e, &mut d, 2.5, 3.0);
        assert_eq!(lines(&d)[2], (Point::new(0.0, 3.0), Point::new(5.0, 3.0)));
        // It now only touches the vertical line at its own end, so nothing
        // bounds the picked part and Quick mode erases it.
        click(&mut e, &mut d, 2.0, 3.0);
        assert_eq!(lines(&d).len(), 2);
        type_all(&mut e, &mut d, &[""]);
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(lines(&d).len(), 3);
        assert_eq!(lines(&d)[2], (Point::new(0.0, 3.0), Point::new(3.0, 3.0)), "TRIM undoes as one step");
    }

    #[test]
    fn trim_splits_a_circle_into_an_arc() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["c", "0,0", "5", "l", "-10,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["tr"]);
        click(&mut e, &mut d, 0.0, 5.0);
        type_all(&mut e, &mut d, &[""]);
        let arc = d.entities().find_map(|e| match e.geometry {
            Geometry::Arc { start_angle, end_angle, .. } => Some((start_angle, end_angle)),
            _ => None,
        });
        let (s, en) = arc.expect("the circle became an arc");
        assert!((s - 180.0).abs() < 1e-9 && en.abs() < 1e-9, "lower half kept: {s} -> {en}");
    }

    #[test]
    fn extend_to_nearest_boundary() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "2,0", "", "l", "5,-5", "5,5", "", "l", "9,-5", "9,5", ""]);
        type_all(&mut e, &mut d, &["ex"]);
        click(&mut e, &mut d, 1.8, 0.0);
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(lines(&d)[0], (Point::new(0.0, 0.0), Point::new(5.0, 0.0)));
    }

    #[test]
    fn fillet_lines_with_radius_and_polyline_option() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "12,2", "12,10", ""]);
        type_all(&mut e, &mut d, &["f", "r", "2"]);
        click(&mut e, &mut d, 5.0, 0.0);
        click(&mut e, &mut d, 12.0, 6.0);
        assert!(!e.is_active());
        assert_eq!(d.len(), 3, "two trimmed lines plus the arc");
        assert!(circles(&d).is_empty());
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(d.len(), 2);

        type_all(&mut e, &mut d, &["f"]);
        assert!(e.history().last().unwrap().contains("Radius = 2.0000"), "FILLETRAD is remembered");

        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["rec", "0,0", "10,10", "f", "r", "1", "p"]);
        click(&mut e, &mut d, 5.0, 0.0);
        assert_eq!(points(&only(&d)).len(), 8);
        assert!(e.history().iter().any(|l| l == "4 lines were filleted"));
    }

    #[test]
    fn explode_polyline_into_lines() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["rec", "0,0", "10,10", "x"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(lines(&d).len(), 4);
    }

    #[test]
    fn matchprop_copies_layer_and_color() {
        let (mut e, mut d) = (engine(), Document::new());
        d.add_layer(oxidraft_model::Layer::new("Red", 1));
        d.set_current_layer("Red");
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        d.set_current_layer("0");
        type_all(&mut e, &mut d, &["l", "0,5", "10,5", "", "l", "0,9", "10,9", ""]);
        type_all(&mut e, &mut d, &["ma"]);
        click(&mut e, &mut d, 5.0, 0.0);
        click(&mut e, &mut d, 5.0, 5.0);
        click(&mut e, &mut d, 5.0, 9.0);
        type_all(&mut e, &mut d, &[""]);
        assert!(d.entities().all(|x| x.layer == "Red"));
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(d.entities().filter(|x| x.layer == "Red").count(), 1, "MATCHPROP undoes as one step");
    }

    #[test]
    fn locked_layers_cannot_be_picked() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        let mut zero = d.layer("0").unwrap().clone();
        zero.locked = true;
        d.update_layer(zero);
        click(&mut e, &mut d, 5.0, 0.0);
        assert!(e.selection().is_empty());
        assert!(e.window_start().is_some(), "the click starts a window instead");
    }

    #[test]
    fn block_define_insert_and_explode() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["rec", "0,0", "2,1", "c", "1,0.5", "0.25"]);
        type_all(&mut e, &mut d, &["block", "TABLE", "0,0"]);
        click(&mut e, &mut d, 3.0, 3.0);
        click(&mut e, &mut d, -1.0, -1.0);
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(d.block("TABLE").unwrap().entities.len(), 2);
        assert!(matches!(only(&d), Geometry::Insert { .. }), "the objects became one reference");

        // A name with spaces or bad characters is refused.
        type_all(&mut e, &mut d, &["block", "a/b"]);
        assert!(e.history().last().unwrap().contains("isn't a valid name"));
        e.cancel(&mut d);

        type_all(&mut e, &mut d, &["insert", "TABLE", "s", "2", "r", "90", "10,0"]);
        let refs: Vec<_> = d.entities().filter(|x| matches!(x.geometry, Geometry::Insert { .. })).collect();
        assert_eq!(refs.len(), 2);
        // The second copy is scaled by 2 and turned 90°: its rectangle runs from x 8 to 10.
        let b = d.entity_bounds(refs[1]);
        assert!(b.min.approx_eq(Point::new(8.0, 0.0)) && b.max.approx_eq(Point::new(10.0, 4.0)), "{b:?}");

        // INSERT offers the last block by default.
        type_all(&mut e, &mut d, &["i"]);
        assert!(e.prompt().contains("<TABLE>"));
        type_all(&mut e, &mut d, &["", "20,0"]);
        assert_eq!(d.len(), 3);

        type_all(&mut e, &mut d, &["x"]);
        click(&mut e, &mut d, 9.0, 0.0);
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(d.len(), 4, "exploding one reference gives back its rectangle and circle");
        assert!(circles(&d).iter().any(|&(c, r)| c.approx_eq(Point::new(9.0, 2.0)) && (r - 0.5).abs() < 1e-9));
    }

    #[test]
    fn trimming_a_block_reference_is_refused() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["b", "LINE1", "0,0"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["", "l", "5,-5", "5,5", "", "tr"]);
        click(&mut e, &mut d, 8.0, 0.0);
        assert!(e.history().last().unwrap().contains("explode"));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn grip_stretch_moves_shared_endpoint() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "10,10", ""]);
        click(&mut e, &mut d, 5.0, 0.0);
        click(&mut e, &mut d, 10.0, 5.0);
        assert_eq!(e.grips(&d).len(), 6);
        // Click the shared corner grip, then the new location.
        click(&mut e, &mut d, 10.0, 0.0);
        assert_eq!(e.active_name(), Some("STRETCH"));
        click(&mut e, &mut d, 20.0, 0.0);
        assert_eq!(
            lines(&d),
            vec![(Point::new(0.0, 0.0), Point::new(20.0, 0.0)), (Point::new(20.0, 0.0), Point::new(10.0, 10.0))]
        );
        assert_eq!(e.selection().len(), 2, "selection survives grip editing");
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(lines(&d)[0].1, Point::new(10.0, 0.0));
    }

    #[test]
    fn ortho_and_polar_tracking() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0"]);
        // Polar (default on) locks near 0° and 90°, but not at 45°.
        let t = e.track(Point::new(10.0, 0.3), 0.5);
        assert_eq!(t.point, Point::new(10.0, 0.0));
        assert_eq!(t.polar.map(|p| p.1), Some(0.0));
        assert!(e.track(Point::new(5.0, 5.0), 0.5).polar.is_none());

        e.set_ortho(true);
        assert!(!e.polar());
        assert_eq!(e.track(Point::new(10.0, 3.0), 0.5).point, Point::new(10.0, 0.0));
        assert_eq!(e.track(Point::new(2.0, -8.0), 0.5).point, Point::new(0.0, -8.0));
    }

    #[test]
    fn autocomplete_prefers_exact_alias() {
        let names: Vec<_> = Engine::suggest("c").iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["CIRCLE", "COPY", "PROPERTIES"], "PROPERTIES via its CH alias");
        let names: Vec<_> = Engine::suggest("co").iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["COPY"]);
        let names: Vec<_> = Engine::suggest("s").iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["STRETCH", "SCALE"]);
    }

    #[test]
    fn unknown_command_is_reported() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["frobnicate"]);
        assert!(e.history().last().unwrap().contains("Unknown command"));
    }
}

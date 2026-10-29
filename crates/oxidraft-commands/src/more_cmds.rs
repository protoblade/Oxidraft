//! Smaller AutoCAD LT commands: object isolation (ISOLATEOBJECTS,
//! HIDEOBJECTS), ADDSELECTED, MINSERT, XPLODE, MREDO, the classic LEADER
//! (and QLEADER), on/off modes (ORTHO, REGENAUTO, DRAGMODE), ELEV,
//! VIEWRES, TABLEEXPORT, BROWSER and GOTOURL.

use oxidraft_geom::Point;
use oxidraft_model::assoc_array::ArrayLayout;
use oxidraft_model::{CurrentProps, EntityId, Geometry, Placement};

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest};

/// ISOLATEOBJECTS (only the objects selected show) and HIDEOBJECTS
/// (they don't); UNISOLATEOBJECTS shows them all again. For now only:
/// nothing is saved or undone.
pub struct Isolate {
    hide: bool,
}

impl Isolate {
    pub fn new(hide: bool) -> Self {
        Self { hide }
    }
}

impl Command for Isolate {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else { return Err("Select objects, then press Enter.".into()) };
        if s.ids.is_empty() {
            return Ok(Step::Done);
        }
        let hidden: Vec<EntityId> = if self.hide {
            s.ids
        } else {
            let space = ctx.doc.space();
            ctx.doc.entities().filter(|e| e.space == space && !s.ids.contains(&e.id)).map(|e| e.id).collect()
        };
        let n = hidden.len();
        ctx.doc.hide_objects(hidden);
        ctx.messages.push(format!("{n} object(s) hidden; UNISOLATEOBJECTS shows them again."));
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

/// The command that draws what `g` is.
fn command_for(g: &Geometry) -> &'static str {
    match g {
        Geometry::Point { .. } => "POINT",
        Geometry::Line { .. } => "LINE",
        Geometry::Circle { .. } => "CIRCLE",
        Geometry::Arc { .. } => "ARC",
        Geometry::Polyline { .. } => "PLINE",
        Geometry::Ellipse(_) => "ELLIPSE",
        Geometry::Spline(_) => "SPLINE",
        Geometry::Xline { .. } => "XLINE",
        Geometry::Ray { .. } => "RAY",
        Geometry::Text { .. } => "MTEXT",
        Geometry::Dimension { aligned: true, .. } => "DIMALIGNED",
        Geometry::Dimension { .. } => "DIMLINEAR",
        Geometry::RadialDimension { diameter: true, .. } => "DIMDIAMETER",
        Geometry::RadialDimension { .. } => "DIMRADIUS",
        Geometry::AngularDimension { .. } => "DIMANGULAR",
        Geometry::OrdinateDimension { .. } => "DIMORDINATE",
        Geometry::ArcLengthDimension { .. } => "DIMARC",
        Geometry::JoggedDimension { .. } => "DIMJOGGED",
        Geometry::Leader { .. } => "MLEADER",
        Geometry::Hatch { .. } => "HATCH",
        Geometry::Insert { .. } => "INSERT",
        Geometry::Table { .. } => "TABLE",
        Geometry::Tolerance { .. } => "TOLERANCE",
        Geometry::Wipeout { .. } => "WIPEOUT",
        Geometry::Image { .. } => "IMAGEATTACH",
        Geometry::AttributeDefinition { .. } => "ATTDEF",
        _ => "LINE",
    }
}

/// ADDSELECTED: the command that draws an object like the one picked, its
/// layer, color, linetype and lineweight current.
#[derive(Default)]
pub struct AddSelected;

impl Command for AddSelected {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select object")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let at = match input {
            Input::Pick { at, .. } => at,
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Select an object.".into()),
        };
        let e = ctx.pick(at).and_then(|id| ctx.doc.entity(id)).cloned().ok_or("No object found.")?;
        ctx.doc.begin("ADDSELECTED");
        ctx.doc.set_current_layer(&e.layer);
        let props = CurrentProps {
            color: e.color,
            linetype: e.linetype.clone(),
            lineweight: e.lineweight,
            transparency: e.transparency,
        };
        ctx.doc.set_current_props(props);
        ctx.doc.commit();
        ctx.ui = Some(UiRequest::RunCommand(command_for(&e.geometry).into()));
        Ok(Step::Done)
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, PartialEq)]
enum MinsertStage {
    Name,
    Point,
    Scale,
    Rotation,
    Rows,
    Columns,
    RowSpacing,
    ColumnSpacing,
}

/// MINSERT: a block inserted in rows and columns, as one object: an
/// associative rectangular array of the reference.
pub struct MInsert {
    stage: MinsertStage,
    name: String,
    at: Point,
    scale: f64,
    rotation: f64,
    rows: usize,
    columns: usize,
    row_spacing: f64,
}

impl Default for MInsert {
    fn default() -> Self {
        Self {
            stage: MinsertStage::Name,
            name: String::new(),
            at: Point::ORIGIN,
            scale: 1.0,
            rotation: 0.0,
            rows: 1,
            columns: 1,
            row_spacing: 1.0,
        }
    }
}

impl MInsert {
    fn place(&self, column_spacing: f64, ctx: &mut Ctx) -> Step {
        let block = ctx.doc.block(&self.name).cloned();
        let Some(block) = block else { return Step::Done };
        let placement = Placement { position: self.at, scale: self.scale, rotation: self.rotation, mirror: false };
        let attributes = block.attributes_for(&placement, |_| None);
        let id = ctx.doc.add(Geometry::Insert { block: block.name.clone(), placement, attributes, clip: None });
        if self.rows * self.columns > 1 {
            let layout = ArrayLayout::Rect {
                rows: self.rows,
                columns: self.columns,
                row_spacing: self.row_spacing,
                column_spacing,
            };
            ctx.doc.make_array(&[id], layout);
        }
        Step::Done
    }
}

impl Command for MInsert {
    fn prompt(&self) -> Prompt {
        match self.stage {
            MinsertStage::Name => Prompt::new("Enter block name"),
            MinsertStage::Point => Prompt::new("Specify insertion point"),
            MinsertStage::Scale => Prompt::new("Enter scale factor").with_default("1"),
            MinsertStage::Rotation => Prompt::new("Specify rotation angle").with_default("0"),
            MinsertStage::Rows => Prompt::new("Enter number of rows (---)").with_default("1"),
            MinsertStage::Columns => Prompt::new("Enter number of columns (|||)").with_default("1"),
            MinsertStage::RowSpacing => Prompt::new("Enter distance between rows"),
            MinsertStage::ColumnSpacing => Prompt::new("Specify distance between columns (|||)"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let count = |v: f64| {
            (v >= 1.0 && v.fract() == 0.0 && v <= 32767.0)
                .then_some(v as usize)
                .ok_or("Requires a whole number of 1 or more.")
        };
        match (self.stage, input) {
            (MinsertStage::Name, Input::Text(t)) => {
                let block = ctx.doc.block(t.trim()).ok_or_else(|| format!("Block \"{}\" not found.", t.trim()))?;
                self.name = block.name.clone();
                self.stage = MinsertStage::Point;
            }
            (MinsertStage::Point, Input::Point(p)) => {
                self.at = p;
                self.stage = MinsertStage::Scale;
            }
            (MinsertStage::Scale, input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(v) = input {
                    self.scale = if v > 0.0 { v } else { return Err("The scale must be more than 0.".into()) };
                }
                self.stage = MinsertStage::Rotation;
            }
            (MinsertStage::Rotation, input @ (Input::Value(_) | Input::Enter)) => {
                if let Input::Value(v) = input {
                    self.rotation = v;
                }
                self.stage = MinsertStage::Rows;
            }
            (MinsertStage::Rows, input @ (Input::Value(_) | Input::Enter)) => {
                self.rows = if let Input::Value(v) = input { count(v)? } else { 1 };
                self.stage = MinsertStage::Columns;
            }
            (MinsertStage::Columns, input @ (Input::Value(_) | Input::Enter)) => {
                self.columns = if let Input::Value(v) = input { count(v)? } else { 1 };
                self.stage = match (self.rows, self.columns) {
                    (1, 1) => return Ok(self.place(0.0, ctx)),
                    (1, _) => MinsertStage::ColumnSpacing,
                    _ => MinsertStage::RowSpacing,
                };
            }
            (MinsertStage::RowSpacing, Input::Value(d)) => {
                self.row_spacing = d;
                if self.columns == 1 {
                    return Ok(self.place(0.0, ctx));
                }
                self.stage = MinsertStage::ColumnSpacing;
            }
            (MinsertStage::ColumnSpacing, Input::Value(d)) => return Ok(self.place(d, ctx)),
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_text(&self) -> bool {
        self.stage == MinsertStage::Name
    }
}

#[derive(Clone, PartialEq)]
enum XplodeStage {
    Select,
    Option(Vec<EntityId>),
    Layer(Vec<EntityId>),
    Color(Vec<EntityId>),
}

/// XPLODE: exploded, the parts given a layer or a color of your choosing,
/// or taking the reference's (Inherit), or as EXPLODE leaves them.
pub struct XPlode {
    stage: XplodeStage,
}

impl Default for XPlode {
    fn default() -> Self {
        Self { stage: XplodeStage::Select }
    }
}

impl XPlode {
    /// Explodes `ids`, then gives what came of each `f`.
    fn explode(ctx: &mut Ctx, ids: &[EntityId], f: impl Fn(&mut oxidraft_model::Entity, &oxidraft_model::Entity)) {
        let before: std::collections::HashSet<EntityId> = ctx.doc.entities().map(|e| e.id).collect();
        let originals: Vec<oxidraft_model::Entity> = ids.iter().filter_map(|id| ctx.doc.entity(*id).cloned()).collect();
        let mut explode = crate::explode::Explode;
        let selection = crate::Selection { ids: ids.to_vec(), ..Default::default() };
        let _ = explode.input(Input::Selection(selection), ctx);
        let new: Vec<EntityId> = ctx.doc.entities().map(|e| e.id).filter(|id| !before.contains(id)).collect();
        if let Some(parent) = originals.first() {
            for id in new {
                ctx.doc.update(id, |e| f(e, parent));
            }
        }
    }
}

impl Command for XPlode {
    fn prompt(&self) -> Prompt {
        match self.stage {
            XplodeStage::Select => Prompt::new("Select objects to XPlode"),
            XplodeStage::Option(_) => Prompt::new("Enter an option")
                .with_keywords(&["LAyer", "Color", "Inherit from parent block", "Explode"])
                .with_default("Explode"),
            XplodeStage::Layer(_) => Prompt::new("Enter new layer name for exploded objects"),
            XplodeStage::Color(_) => Prompt::new("Enter new color for exploded objects (1-255 or BYLAYER)"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (XplodeStage::Select, Input::Selection(s)) if s.ids.is_empty() => return Ok(Step::Done),
            (XplodeStage::Select, Input::Selection(s)) => self.stage = XplodeStage::Option(s.ids),
            (XplodeStage::Option(ids), Input::Keyword("LAyer")) => self.stage = XplodeStage::Layer(ids),
            (XplodeStage::Option(ids), Input::Keyword("Color")) => self.stage = XplodeStage::Color(ids),
            (XplodeStage::Option(ids), Input::Keyword("Inherit from parent block")) => {
                Self::explode(ctx, &ids, |e, parent| {
                    e.layer = parent.layer.clone();
                    e.color = parent.color;
                    e.linetype = parent.linetype.clone();
                    e.lineweight = parent.lineweight;
                });
                return Ok(Step::Done);
            }
            (XplodeStage::Option(ids), Input::Keyword(_) | Input::Enter) => {
                Self::explode(ctx, &ids, |_, _| {});
                return Ok(Step::Done);
            }
            (XplodeStage::Layer(ids), Input::Text(t)) => {
                let layer = t.trim().to_string();
                if ctx.doc.layer(&layer).is_none() {
                    return Err(format!("Layer \"{layer}\" not found."));
                }
                Self::explode(ctx, &ids, |e, _| e.layer = layer.clone());
                return Ok(Step::Done);
            }
            (XplodeStage::Color(ids), input @ (Input::Text(_) | Input::Value(_))) => {
                let text = match input {
                    Input::Text(t) => t,
                    Input::Value(v) => v.to_string(),
                    _ => unreachable!(),
                };
                let color = crate::current_props::parse_color(&text).ok_or("Invalid color.")?;
                Self::explode(ctx, &ids, |e, _| e.color = color);
                return Ok(Step::Done);
            }
            (XplodeStage::Select, _) => return Err("Select objects, then press Enter.".into()),
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Invalid input.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        self.stage == XplodeStage::Select
    }

    fn wants_text(&self) -> bool {
        matches!(self.stage, XplodeStage::Layer(_) | XplodeStage::Color(_))
    }

    fn text_with_spaces(&self) -> bool {
        matches!(self.stage, XplodeStage::Layer(_))
    }
}

/// MREDO: several undone actions done again, all of them, or the last.
#[derive(Default)]
pub struct MRedo;

impl Command for MRedo {
    fn prompt(&self) -> Prompt {
        Prompt::new("Enter number of actions").with_keywords(&["All", "Last"])
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let n = match input {
            Input::Value(v) if v >= 1.0 => v as usize,
            Input::Keyword("All") => usize::MAX,
            Input::Keyword(_) | Input::Enter => 1,
            _ => return Err("Requires a positive number, All or Last.".into()),
        };
        let mut done = Vec::new();
        for _ in 0..n {
            match ctx.doc.redo() {
                Some(m) => done.push(m),
                None => break,
            }
        }
        ctx.messages.push(if done.is_empty() { "Nothing to redo.".into() } else { done.join(", ") });
        Ok(Step::Done)
    }
}

/// LEADER and QLEADER, the classic leaders: points from the arrowhead,
/// then the annotation's lines of text.
pub struct ClassicLeader {
    points: Vec<Point>,
    text: Vec<String>,
    annotating: bool,
    scale: f64,
}

impl Default for ClassicLeader {
    fn default() -> Self {
        Self { points: Vec::new(), text: Vec::new(), annotating: false, scale: 1.0 }
    }
}

impl ClassicLeader {
    fn finish(&self, ctx: &mut Ctx) -> Step {
        if self.points.len() >= 2 {
            ctx.doc.add(Geometry::Leader {
                points: self.points.clone(),
                content: self.text.join("\\P"),
                scale: self.scale,
                more: Vec::new(),
                blocks: None,
            });
        }
        Step::Done
    }
}

impl Command for ClassicLeader {
    fn start(&mut self, vars: &crate::Vars) -> Option<String> {
        self.scale = vars.dim_scale;
        None
    }

    fn prompt(&self) -> Prompt {
        match (self.annotating, self.points.len()) {
            (true, _) if self.text.is_empty() => Prompt::new("Enter first line of annotation text"),
            (true, _) => Prompt::new("Enter next line of annotation text"),
            (false, 0) => Prompt::new("Specify leader start point"),
            (false, 1) => Prompt::new("Specify next point"),
            (false, _) => Prompt::new("Specify next point or [Annotation/Undo]").with_keywords(&["Annotation", "Undo"]),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.annotating, input) {
            (false, Input::Point(p)) => self.points.push(p),
            (false, Input::Keyword("Undo")) => {
                self.points.pop();
            }
            (false, Input::Keyword(_) | Input::Enter) if self.points.len() >= 2 => self.annotating = true,
            (false, Input::Enter) => return Ok(Step::Done),
            (true, Input::Text(t)) if !t.is_empty() => self.text.push(t),
            (true, _) => return Ok(self.finish(ctx)),
            _ => return Err("Point required.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        if self.annotating || self.points.is_empty() {
            return Vec::new();
        }
        let points: Vec<Point> = self.points.iter().copied().chain([cursor]).collect();
        vec![Geometry::Leader { points, content: String::new(), scale: self.scale, more: Vec::new(), blocks: None }]
    }

    fn base_point(&self) -> Option<Point> {
        if self.annotating { None } else { self.points.last().copied() }
    }

    fn wants_text(&self) -> bool {
        self.annotating
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// ORTHO, REGENAUTO and DRAGMODE: a mode ON or OFF (DRAGMODE also Auto),
/// for the app to set.
pub struct OnOff {
    var: &'static str,
    auto: bool,
}

impl OnOff {
    pub fn new(var: &'static str, auto: bool) -> Self {
        Self { var, auto }
    }
}

impl Command for OnOff {
    fn prompt(&self) -> Prompt {
        let keywords: &[&str] = if self.auto { &["ON", "OFF", "Auto"] } else { &["ON", "OFF"] };
        Prompt::new("Enter mode").with_keywords(keywords).with_default(if self.auto { "Auto" } else { "ON" })
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let value = match input {
            Input::Keyword("OFF") => "0",
            Input::Keyword("Auto") => "2",
            Input::Keyword(_) | Input::Enter => {
                if self.auto && input == Input::Enter {
                    "2"
                } else {
                    "1"
                }
            }
            _ => return Err("Enter ON or OFF.".into()),
        };
        ctx.ui = Some(UiRequest::SetVar(self.var.into(), value.into()));
        Ok(Step::Done)
    }
}

/// ELEV: the elevation and thickness new objects get (kept, though a 2D
/// drawing has no use for them).
#[derive(Default)]
pub struct Elev {
    elevation_set: bool,
}

impl Command for Elev {
    fn prompt(&self) -> Prompt {
        if self.elevation_set {
            Prompt::new("Specify new default thickness").with_default("0")
        } else {
            Prompt::new("Specify new default elevation").with_default("0")
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let v = match input {
            Input::Value(v) => v,
            Input::Enter => 0.0,
            _ => return Err("Requires a number.".into()),
        };
        if self.elevation_set {
            ctx.vars.thickness = v;
            return Ok(Step::Done);
        }
        ctx.vars.elevation = v;
        self.elevation_set = true;
        Ok(Step::Continue)
    }
}

/// VIEWRES: fast zooms, and how smooth circles and arcs show (in Oxidraft
/// they're always smooth; the percent is kept).
#[derive(Default)]
pub struct ViewRes {
    asked_fast: bool,
}

impl Command for ViewRes {
    fn prompt(&self) -> Prompt {
        if self.asked_fast {
            Prompt::new("Enter circle zoom percent (1-20000)").with_default("1000")
        } else {
            Prompt::new("Do you want fast zooms?").with_keywords(&["Yes", "No"]).with_default("Yes")
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        if !self.asked_fast {
            self.asked_fast = true;
            return Ok(Step::Continue);
        }
        match input {
            Input::Value(v) if (1.0..=20000.0).contains(&v) => ctx.vars.view_res = v.round() as u32,
            Input::Enter => {}
            _ => return Err("Requires an integer between 1 and 20000.".into()),
        }
        Ok(Step::Done)
    }
}

/// TABLEEXPORT: a table, for the app to write as CSV.
#[derive(Default)]
pub struct TableExport;

impl Command for TableExport {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select a table")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Pick { at, .. } = input else { return Ok(Step::Done) };
        let id = ctx
            .pick(at)
            .filter(|id| matches!(ctx.doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Table { .. })))
            .ok_or("Select a table.")?;
        ctx.ui = Some(UiRequest::TableExport(id));
        Ok(Step::Done)
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

/// BROWSER: a web address, for the app to open; GOTOURL: an object's
/// hyperlink, opened.
pub struct Browser {
    object: bool,
}

impl Browser {
    pub fn new(object: bool) -> Self {
        Self { object }
    }
}

impl Command for Browser {
    fn prompt(&self) -> Prompt {
        if self.object {
            Prompt::new("Select an object with a hyperlink")
        } else {
            Prompt::new("Enter Web location (URL)").with_default("https://github.com/protoblade/Oxidraft")
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let url = match input {
            Input::Pick { at, .. } if self.object => ctx
                .pick(at)
                .and_then(|id| ctx.doc.entity(id))
                .and_then(|e| e.hyperlink.as_ref())
                .map(|h| h.url.clone())
                .ok_or("That object has no hyperlink.")?,
            Input::Text(t) if !t.trim().is_empty() => t.trim().to_string(),
            Input::Enter if !self.object => "https://github.com/protoblade/Oxidraft".into(),
            _ => return Ok(Step::Done),
        };
        ctx.ui = Some(UiRequest::OpenUrl(url));
        Ok(Step::Done)
    }

    fn wants_pick(&self) -> bool {
        self.object
    }

    fn wants_text(&self) -> bool {
        !self.object
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Block, BlockEntity, Color, Document, Geometry};

    fn run(e: &mut Engine, d: &mut Document, inputs: &[&str]) {
        e.dynamic_input = false;
        for t in inputs {
            e.submit(t, d, Point::ORIGIN, 0.5);
        }
    }

    #[test]
    fn minsert_xplode_and_mredo() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        let dot = BlockEntity::new("0", Color::ByBlock, Geometry::Circle { center: Point::ORIGIN, radius: 1.0 });
        d.define_block(Block { name: "DOT".into(), base: Point::ORIGIN, entities: vec![dot] });
        // MINSERT: 2 rows of 3, one object.
        run(&mut e, &mut d, &["minsert", "DOT", "0,0", "", "", "2", "3", "10", "20"]);
        assert_eq!(d.len(), 1);
        let circles = |d: &Document| {
            d.entities()
                .map(|x| d.expanded(x).iter().filter(|g| matches!(g, Geometry::Circle { .. })).count())
                .sum::<usize>()
        };
        assert_eq!(circles(&d), 6);
        // XPLODE onto a layer: the reference's parts come apart (one level).
        d.add_layer(oxidraft_model::Layer::new("Parts", 2));
        let id = d.entities().next().unwrap().id;
        e.set_selection(vec![id]);
        run(&mut e, &mut d, &["xplode", "la", "Parts"]);
        assert!(d.len() > 1 && d.entities().all(|x| x.layer == "Parts"), "{}", d.len());
        // MREDO: XPLODE, the new layer and MINSERT undone, all redone.
        run(&mut e, &mut d, &["u", "u", "u"]);
        assert_eq!(d.len(), 0);
        run(&mut e, &mut d, &["mredo", "a"]);
        assert!(d.len() > 1);
    }
}

/// ATTACHURL (a web address on the objects selected), DETACHURL (their
/// hyperlinks taken off) and PASTEASHYPERLINK (the clipboard's text on
/// them, which the app fills in).
pub struct Url {
    mode: UrlMode,
    ids: Vec<EntityId>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum UrlMode {
    Attach,
    Detach,
    Paste,
}

impl Url {
    pub fn new(mode: UrlMode) -> Self {
        Self { mode, ids: Vec::new() }
    }
}

impl Command for Url {
    fn prompt(&self) -> Prompt {
        if self.ids.is_empty() { Prompt::new("Select objects") } else { Prompt::new("Enter hyperlink") }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Selection(s) if self.ids.is_empty() => {
                if s.ids.is_empty() {
                    return Ok(Step::Done);
                }
                match self.mode {
                    UrlMode::Attach => self.ids = s.ids,
                    UrlMode::Detach => {
                        let mut n = 0;
                        for id in s.ids {
                            if ctx.doc.entity(id).is_some_and(|e| e.hyperlink.is_some()) {
                                ctx.doc.update(id, |e| e.hyperlink = None);
                                n += 1;
                            }
                        }
                        ctx.messages.push(format!("{n} hyperlink(s) deleted."));
                        return Ok(Step::Done);
                    }
                    UrlMode::Paste => {
                        ctx.ui = Some(UiRequest::PasteHyperlink(s.ids));
                        return Ok(Step::Done);
                    }
                }
            }
            Input::Text(t) if !t.trim().is_empty() => {
                let link = oxidraft_model::Hyperlink { url: t.trim().to_string(), description: String::new() };
                for id in &self.ids {
                    ctx.doc.update(*id, |e| e.hyperlink = Some(Box::new(link.clone())));
                }
                return Ok(Step::Done);
            }
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Select objects, then press Enter.".into()),
        }
        Ok(Step::Continue)
    }

    fn wants_selection(&self) -> bool {
        self.ids.is_empty()
    }

    fn wants_text(&self) -> bool {
        !self.ids.is_empty()
    }
}

/// VIEWGO and VIEWPLAY: a named view, by name, made current.
#[derive(Default)]
pub struct ViewGo;

impl Command for ViewGo {
    fn prompt(&self) -> Prompt {
        Prompt::new("Enter view name")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Text(name) = input else { return Ok(Step::Done) };
        if !ctx.doc.views().iter().any(|v| v.name.eq_ignore_ascii_case(name.trim())) {
            return Err(format!("View \"{}\" not found.", name.trim()));
        }
        ctx.ui = Some(UiRequest::RestoreView(name.trim().to_string()));
        Ok(Step::Done)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// XBIND: an attached drawing's layer or block (XREF|NAME) made this
/// drawing's own, as XREF$0$NAME, as AutoCAD binds them.
#[derive(Default)]
pub struct XBind;

impl Command for XBind {
    fn prompt(&self) -> Prompt {
        Prompt::new("Enter the attached drawing's layer or block to bind (XREF|NAME)")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Text(t) = input else { return Ok(Step::Done) };
        let name = t.trim();
        let Some((xref, own)) = name.split_once('|') else {
            return Err("Name one of an attached drawing's: XREF|NAME.".into());
        };
        let bound = format!("{xref}$0${own}");
        if let Some(layer) = ctx.doc.layer(name).cloned() {
            ctx.doc.rename_layer(&layer.name, &bound)?;
        } else if ctx.doc.block(name).is_some() {
            ctx.doc.rename_block(name, &bound)?;
        } else {
            return Err(format!("\"{name}\" isn't an attached drawing's layer or block."));
        }
        ctx.messages.push(format!("{name} bound as {bound}."));
        Ok(Step::Done)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_with_spaces(&self) -> bool {
        true
    }
}

/// APERTURE: how many pixels round the cursor object snaps reach.
#[derive(Default)]
pub struct Aperture;

impl Command for Aperture {
    fn prompt(&self) -> Prompt {
        Prompt::new("Object snap target height (1-50 pixels)").with_default("10")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let v = match input {
            Input::Value(v) if (1.0..=50.0).contains(&v) => v,
            Input::Enter => 10.0,
            _ => return Err("Requires an integer between 1 and 50.".into()),
        };
        ctx.ui = Some(UiRequest::SetVar("APERTURE".into(), format!("{}", v.round())));
        Ok(Step::Done)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum CenterAction {
    Disassociate,
    Reassociate,
    Reset,
}

/// CENTERDISASSOCIATE, CENTERREASSOCIATE (then the circle or arc) and
/// CENTERRESET: on center marks picked.
pub struct CenterCmd {
    action: CenterAction,
    mark: Option<EntityId>,
}

impl CenterCmd {
    pub fn new(action: CenterAction) -> Self {
        Self { action, mark: None }
    }
}

impl Command for CenterCmd {
    fn prompt(&self) -> Prompt {
        match (self.action, self.mark) {
            (CenterAction::Reassociate, Some(_)) => Prompt::new("Select circle or arc"),
            _ => Prompt::new("Select center mark"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let at = match input {
            Input::Pick { at, .. } => at,
            Input::Enter => return Ok(Step::Done),
            _ => return Err("Pick a center mark.".into()),
        };
        let id = ctx.pick(at).ok_or("No object found.")?;
        match (self.action, self.mark) {
            (CenterAction::Reassociate, Some(mark)) => {
                if !ctx.doc.reassociate_center_mark(mark, id) {
                    return Err("Select a circle or arc.".into());
                }
                return Ok(Step::Done);
            }
            (CenterAction::Reassociate, None) => {
                if ctx.doc.group_of(id).is_none_or(|g| !g.description.starts_with("Center mark")) {
                    return Err("Select a center mark.".into());
                }
                self.mark = Some(id);
            }
            (CenterAction::Disassociate, _) => {
                if !ctx.doc.set_center_mark_follows(id, false) {
                    return Err("Select a center mark.".into());
                }
                ctx.messages.push("The center mark no longer follows its circle.".into());
            }
            (CenterAction::Reset, _) => {
                if !ctx.doc.reset_center_mark(id) {
                    return Err("Select a center mark on a circle or arc.".into());
                }
            }
        }
        Ok(Step::Continue)
    }

    fn wants_pick(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod center_tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry};

    #[test]
    fn center_marks_follow_reset_and_let_go() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        e.dynamic_input = false;
        let run = |e: &mut Engine, d: &mut Document, t: &str| e.submit(t, d, Point::ORIGIN, 0.5);
        run(&mut e, &mut d, "c");
        run(&mut e, &mut d, "0,0");
        run(&mut e, &mut d, "10");
        let circle = d.entities().next().unwrap().id;
        run(&mut e, &mut d, "centermark");
        e.click(Point::new(10.0, 0.0), Point::new(10.0, 0.0), false, &mut d, 0.5);
        run(&mut e, &mut d, "");
        let lines = |d: &Document| {
            d.entities()
                .filter_map(|x| match x.geometry {
                    Geometry::Line { start, end } => Some((start, end)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(lines(&d).len(), 2);
        // MOVE the circle: the mark comes along.
        e.set_selection(vec![circle]);
        run(&mut e, &mut d, "m");
        run(&mut e, &mut d, "0,0");
        run(&mut e, &mut d, "20,0");
        assert!(lines(&d).iter().all(|(s, t)| s.midpoint(*t).approx_eq(Point::new(20.0, 0.0))), "{:?}", lines(&d));
    }
}

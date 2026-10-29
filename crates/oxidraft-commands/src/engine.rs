use oxidraft_geom::{Aabb, Point};
use oxidraft_model::{Document, EntityId, Geometry, snap};

use crate::{
    Command, Ctx, Input, Prompt, Selection, Step, UiRequest, Vars, ViewRequest,
    align::{Align, ArrayPath, ChangeSpace, Reverse},
    annotate::Dimension,
    annotate::MLeader,
    annotate::MText,
    annotate::SingleText,
    arc::Arc,
    array::Array,
    array::ArrayKind,
    attdef::{AttDef, AttSync},
    attribute::AttDisp,
    attribute::EattEdit,
    blend::{Blend, Boundary},
    block::DefineBlock,
    block::InsertBlock,
    block_tools::{AttEdit, AttEditGlobal, HyperlinkCmd, NCopy, SetByLayer},
    break_join::Break,
    break_join::Join,
    calc::Cal,
    chamfer::Chamfer,
    circle::Circle,
    cleanup::{Audit, Count, Overkill, Purge},
    clipboard::CopyClip,
    clipboard::Paste,
    current_props::{ColorCmd, LinetypeCmd, LweightCmd},
    dimedit::{DimBreak, DimEdit, DimJogLine, DimOverride, DimTEdit, DimUpdate, Tolerance},
    dims::DimAngular,
    dims::DimChain,
    dims::DimRadial,
    dims_more::{CenterLine, CenterMark, DimArc, DimJogged, DimOrdinate, QDim},
    donut::Donut,
    draworder::DrawOrder,
    draworder::TextToFront,
    ellipse::EllipseCmd,
    erase::Erase,
    explode::Explode,
    fillet::Fillet,
    grip::GripStretch,
    groups::{GroupCommand, GroupEdit, Ungroup},
    hatch::Hatch,
    hatchedit::HatchEdit,
    inquiry::Area,
    inquiry::Dist,
    inquiry::Id,
    inquiry::{ColorTheme, Ltscale, Snap},
    layer::LayIso,
    layer::LayMcur,
    layer::LayerPick,
    layer_tools::{LayCur, LayDel, LayMrg, LayVpi, ToLayer},
    layouts::LayoutCommand,
    leader_edit::{MLeaderAlign, MLeaderEdit},
    lengthen::Lengthen,
    line::Line,
    list::List,
    matchprop::MatchProp,
    measuregeom::{Base, MeasureGeom},
    mirror::Mirror,
    move_copy::MoveCopy,
    mview::{Mview, VpClip, VpMax},
    offset::Offset,
    parse_input,
    pedit::Pedit,
    pline::Pline,
    plot::PlotWindow,
    point::Marks,
    point::PointCmd,
    point::PointStyle,
    polygon::Polygon,
    rectang::Rectang,
    refedit::{RefClose, RefEdit, RefSet},
    rename::Rename,
    revcloud::RevCloud,
    rotate::Rotate,
    scale::Scale,
    select::SelectCmd,
    select::SelectSimilar,
    smartdim::{DimSpace, SmartDim},
    spline::SplineCmd,
    splinedit::SplinEdit,
    stretch::Stretch,
    table::{TableEdit, TableInsert},
    text_edit::{JustifyText, ScaleText},
    text_style::StyleCmd,
    trim_extend::TrimExtend,
    underlay::AttachImage,
    underlay::ImageClip,
    units_cmd::UnitsCmd,
    views::{Toggle, ViewCommand, Vports},
    wipeout::Wipeout,
    xclip::{Clip, XClipCommand},
    xline::ConstructionLine,
    xref::AttachXref,
    zoom::Zoom,
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
        name: "POLYGON",
        aliases: &["POL"],
        description: "Draw a regular polygon as a closed polyline",
        make: Some(|| Box::new(Polygon::default())),
    },
    CommandInfo {
        name: "ELLIPSE",
        aliases: &["EL"],
        description: "Draw an ellipse or elliptical arc",
        make: Some(|| Box::new(EllipseCmd::full())),
    },
    CommandInfo {
        name: "SPLINE",
        aliases: &["SPL"],
        description: "Draw a smooth curve through or near points",
        make: Some(|| Box::new(SplineCmd::default())),
    },
    CommandInfo {
        name: "SPLINEDIT",
        aliases: &["SPE"],
        description: "Edit a spline: close, join, fit points, control vertices, order, reverse",
        make: Some(|| Box::new(SplinEdit::default())),
    },
    CommandInfo {
        name: "XLINE",
        aliases: &["XL"],
        description: "Draw a construction line, infinite both ways",
        make: Some(|| Box::new(ConstructionLine::xline())),
    },
    CommandInfo {
        name: "RAY",
        aliases: &[],
        description: "Draw a ray, infinite one way",
        make: Some(|| Box::new(ConstructionLine::ray())),
    },
    CommandInfo {
        name: "DONUT",
        aliases: &["DO", "DOUGHNUT"],
        description: "Draw filled rings and dots",
        make: Some(|| Box::new(Donut::default())),
    },
    CommandInfo {
        name: "REVCLOUD",
        aliases: &[],
        description: "Draw a revision cloud",
        make: Some(|| Box::new(RevCloud::default())),
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
        name: "CHAMFER",
        aliases: &["CHA"],
        description: "Bevel the corner between two lines",
        make: Some(|| Box::new(Chamfer::default())),
    },
    CommandInfo {
        name: "BREAK",
        aliases: &["BR"],
        description: "Remove the part of an object between two points",
        make: Some(|| Box::new(Break::two_points())),
    },
    CommandInfo {
        name: "BREAKATPOINT",
        aliases: &[],
        description: "Split an object in two at a point",
        make: Some(|| Box::new(Break::at_point())),
    },
    CommandInfo {
        name: "JOIN",
        aliases: &["J"],
        description: "Join lines, arcs and polylines that meet end to end",
        make: Some(|| Box::new(Join)),
    },
    CommandInfo {
        name: "LENGTHEN",
        aliases: &["LEN"],
        description: "Change the length of lines, arcs and open polylines",
        make: Some(|| Box::new(Lengthen::default())),
    },
    CommandInfo {
        name: "ARRAY",
        aliases: &["AR"],
        description: "Copy objects in rows and columns or around a center",
        make: Some(|| Box::new(Array::new(None))),
    },
    CommandInfo {
        name: "ARRAYRECT",
        aliases: &[],
        description: "Copy objects in rows and columns",
        make: Some(|| Box::new(Array::new(Some(ArrayKind::Rectangular)))),
    },
    CommandInfo {
        name: "ARRAYPOLAR",
        aliases: &[],
        description: "Copy objects around a center point",
        make: Some(|| Box::new(Array::new(Some(ArrayKind::Polar)))),
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
    CommandInfo {
        name: "MTEXT",
        aliases: &["T", "MT"],
        description: "Create paragraph text",
        make: Some(|| Box::new(MText::default())),
    },
    CommandInfo {
        name: "TEXT",
        aliases: &["DT", "DTEXT"],
        description: "Create single-line text",
        make: Some(|| Box::new(SingleText::default())),
    },
    CommandInfo {
        name: "DIM",
        aliases: &[],
        description: "Dimension what you pick: lines, circles, arcs, angles between lines, or two points",
        make: Some(|| Box::new(SmartDim::default())),
    },
    CommandInfo {
        name: "DIMEDIT",
        aliases: &["DED", "DIMED"],
        description: "Put dimension text back, give it new text, turn it, or slant extension lines",
        make: Some(|| Box::new(DimEdit::default())),
    },
    CommandInfo {
        name: "DIMTEDIT",
        aliases: &["DIMTED"],
        description: "Move or turn a dimension's text",
        make: Some(|| Box::new(DimTEdit::default())),
    },
    CommandInfo {
        name: "DIMBREAK",
        aliases: &[],
        description: "Cut gaps in dimensions where objects cross them",
        make: Some(|| Box::new(DimBreak::default())),
    },
    CommandInfo {
        name: "TOLERANCE",
        aliases: &["TOL"],
        description: "Place a geometric tolerance (feature control frame)",
        make: Some(|| Box::new(Tolerance::default())),
    },
    CommandInfo {
        name: "DIMJOGLINE",
        aliases: &["DJL"],
        description: "Add a jog to a linear dimension's line, or Remove it",
        make: Some(|| Box::new(DimJogLine::default())),
    },
    CommandInfo {
        name: "DIMOVERRIDE",
        aliases: &["DOV"],
        description: "Set dimension variables on selected dimensions alone, or clear them",
        make: Some(|| Box::new(DimOverride::default())),
    },
    CommandInfo {
        name: "DIMUPDATE",
        aliases: &[],
        description: "Put selected dimensions back to the current dimension style",
        make: Some(|| Box::new(DimUpdate)),
    },
    CommandInfo {
        name: "MLEADEREDIT",
        aliases: &["MLE"],
        description: "Add arrowheads to a multileader, or remove added ones",
        make: Some(|| Box::new(MLeaderEdit::default())),
    },
    CommandInfo {
        name: "MLEADERALIGN",
        aliases: &["MLA"],
        description: "Line up multileaders' text along a direction",
        make: Some(|| Box::new(MLeaderAlign::default())),
    },
    CommandInfo {
        name: "MLEADERSTYLE",
        aliases: &["MLS"],
        description: "Set how multileaders look: arrowhead, text height, landing",
        make: None,
    },
    CommandInfo {
        name: "TABLESTYLE",
        aliases: &["TS"],
        description: "Set how new tables look: text height and cell margins",
        make: None,
    },
    CommandInfo {
        name: "DIMSPACE",
        aliases: &[],
        description: "Space parallel linear dimensions evenly",
        make: Some(|| Box::new(DimSpace::default())),
    },
    CommandInfo {
        name: "DIMLINEAR",
        aliases: &["DLI"],
        description: "Create a horizontal or vertical dimension",
        make: Some(|| Box::new(Dimension::linear())),
    },
    CommandInfo {
        name: "DIMALIGNED",
        aliases: &["DAL"],
        description: "Create a dimension aligned to two points",
        make: Some(|| Box::new(Dimension::aligned())),
    },
    CommandInfo {
        name: "DIMRADIUS",
        aliases: &["DRA"],
        description: "Dimension the radius of an arc or circle",
        make: Some(|| Box::new(DimRadial::radius())),
    },
    CommandInfo {
        name: "DIMDIAMETER",
        aliases: &["DDI"],
        description: "Dimension the diameter of an arc or circle",
        make: Some(|| Box::new(DimRadial::diameter())),
    },
    CommandInfo {
        name: "DIMANGULAR",
        aliases: &["DAN"],
        description: "Dimension the angle between lines, of an arc, or at a vertex",
        make: Some(|| Box::new(DimAngular::default())),
    },
    CommandInfo {
        name: "DIMCONTINUE",
        aliases: &["DCO"],
        description: "Continue a chain of linear dimensions",
        make: Some(|| Box::new(DimChain::continued())),
    },
    CommandInfo {
        name: "DIMBASELINE",
        aliases: &["DBA"],
        description: "Add linear dimensions from the same baseline",
        make: Some(|| Box::new(DimChain::baseline())),
    },
    CommandInfo {
        name: "ATTDEF",
        aliases: &["ATT"],
        description: "Define an attribute: what a block's references ask for and show",
        make: Some(|| Box::new(AttDef::default())),
    },
    CommandInfo {
        name: "ATTSYNC",
        aliases: &[],
        description: "Give a block's references the attributes its definitions now have",
        make: Some(|| Box::new(AttSync)),
    },
    CommandInfo {
        name: "LAYCUR",
        aliases: &[],
        description: "Put the selected objects on the current layer",
        make: Some(|| Box::new(LayCur)),
    },
    CommandInfo {
        name: "LAYMCH",
        aliases: &[],
        description: "Put the selected objects on the layer of an object picked",
        make: Some(|| Box::new(ToLayer::matching())),
    },
    CommandInfo {
        name: "COPYTOLAYER",
        aliases: &[],
        description: "Copy the selected objects onto another layer",
        make: Some(|| Box::new(ToLayer::copying())),
    },
    CommandInfo {
        name: "LAYMRG",
        aliases: &[],
        description: "Merge layers into another: their objects move and they go",
        make: Some(|| Box::new(LayMrg::default())),
    },
    CommandInfo {
        name: "LAYDEL",
        aliases: &[],
        description: "Delete a layer and everything on it",
        make: Some(|| Box::new(LayDel::default())),
    },
    CommandInfo {
        name: "LAYERP",
        aliases: &["LAYP"],
        description: "Put the layer settings back as they were before the last change",
        make: None,
    },
    CommandInfo {
        name: "UNITS",
        aliases: &["UN"],
        description: "Set how lengths and angles show, and what a drawing unit is",
        make: None,
    },
    CommandInfo {
        name: "-UNITS",
        aliases: &["-UN"],
        description: "Set the drawing units at the command line",
        make: Some(|| Box::new(UnitsCmd::default())),
    },
    CommandInfo {
        name: "RENAME",
        aliases: &["REN"],
        description: "Rename blocks, layers, linetypes and views",
        make: None,
    },
    CommandInfo {
        name: "-RENAME",
        aliases: &["-REN"],
        description: "Rename a block, layer, linetype or view at the command line",
        make: Some(|| Box::new(Rename::default())),
    },
    CommandInfo {
        name: "SELECT",
        aliases: &[],
        description: "Select objects, kept as the Previous selection",
        make: Some(|| Box::new(SelectCmd)),
    },
    CommandInfo {
        name: "WBLOCK",
        aliases: &["W"],
        description: "Write a block, the selected objects or the whole drawing to a drawing file",
        make: None,
    },
    CommandInfo {
        name: "CAL",
        aliases: &[],
        description: "Work out an expression at the command line",
        make: Some(|| Box::new(Cal)),
    },
    CommandInfo { name: "QUICKCALC", aliases: &["QC"], description: "Show the QuickCalc calculator", make: None },
    CommandInfo {
        name: "COLOR",
        aliases: &["COL", "COLOUR"],
        description: "Choose the color new objects get",
        make: None,
    },
    CommandInfo {
        name: "-COLOR",
        aliases: &["-COL"],
        description: "Set the color new objects get at the command line",
        make: Some(|| Box::new(ColorCmd::default())),
    },
    CommandInfo {
        name: "LINETYPE",
        aliases: &["LT", "LTYPE"],
        description: "Load, set current and delete linetypes (Linetype Manager)",
        make: None,
    },
    CommandInfo {
        name: "-LINETYPE",
        aliases: &["-LT"],
        description: "List, load, create or set the current linetype at the command line",
        make: Some(|| Box::new(LinetypeCmd::default())),
    },
    CommandInfo {
        name: "LWEIGHT",
        aliases: &["LW", "LINEWEIGHT"],
        description: "Choose the lineweight new objects get, and whether lineweights show",
        make: None,
    },
    CommandInfo {
        name: "-LWEIGHT",
        aliases: &["-LW"],
        description: "Set the lineweight new objects get at the command line",
        make: Some(|| Box::new(LweightCmd::default())),
    },
    CommandInfo {
        name: "STYLE",
        aliases: &["ST", "DDSTYLE"],
        description: "Make, change and set current text styles",
        make: None,
    },
    CommandInfo {
        name: "-STYLE",
        aliases: &["-ST"],
        description: "Make or change a text style at the command line",
        make: Some(|| Box::new(StyleCmd::default())),
    },
    CommandInfo {
        name: "MEASUREGEOM",
        aliases: &["MEA"],
        description: "Measure distances, radii, angles, areas and volumes",
        make: Some(|| Box::new(MeasureGeom::default())),
    },
    CommandInfo {
        name: "BASE",
        aliases: &[],
        description: "Set the point the drawing is inserted by (INSBASE)",
        make: Some(|| Box::new(Base::default())),
    },
    CommandInfo { name: "OOPS", aliases: &[], description: "Bring back what the last ERASE erased", make: None },
    CommandInfo { name: "SETVAR", aliases: &["SET"], description: "Show or set a system variable", make: None },
    CommandInfo {
        name: "STATUS",
        aliases: &[],
        description: "Show the drawing's counts, limits, extents and modes",
        make: None,
    },
    CommandInfo { name: "TIME", aliases: &[], description: "Show when the drawing was made and saved", make: None },
    CommandInfo {
        name: "ATTACH",
        aliases: &[],
        description: "Attach a drawing, image or PDF, by the file picked",
        make: None,
    },
    CommandInfo {
        name: "XOPEN",
        aliases: &[],
        description: "Open an attached drawing (xref) in its own window",
        make: Some(|| Box::new(crate::field_cmd::XOpen)),
    },
    CommandInfo { name: "EXPORT", aliases: &["EXP"], description: "Save a copy as DXF, DWG or PDF", make: None },
    CommandInfo { name: "DXFOUT", aliases: &[], description: "Save a copy as DXF", make: None },
    CommandInfo { name: "DXFIN", aliases: &[], description: "Open a DXF file", make: None },
    CommandInfo { name: "QNEW", aliases: &[], description: "Start a new drawing", make: None },
    CommandInfo { name: "SAVEALL", aliases: &[], description: "Save the drawing", make: None },
    CommandInfo {
        name: "CLEANSCREENON",
        aliases: &[],
        description: "Hide the ribbon and palettes (Ctrl+0)",
        make: None,
    },
    CommandInfo { name: "CLEANSCREENOFF", aliases: &[], description: "Show the ribbon and palettes again", make: None },
    CommandInfo {
        name: "LIMITS",
        aliases: &[],
        description: "Set the drawing's limits; ON keeps points inside them (LIMCHECK)",
        make: Some(|| Box::new(crate::sysvar::Limits::default())),
    },
    CommandInfo {
        name: "FILL",
        aliases: &[],
        description: "Show hatches, wide polylines and solids filled, or as outlines (FILLMODE)",
        make: Some(|| Box::new(crate::sysvar::Mode::fill())),
    },
    CommandInfo {
        name: "QTEXT",
        aliases: &[],
        description: "Show text as boxes for speed (QTEXTMODE)",
        make: Some(|| Box::new(crate::sysvar::Mode::quick_text())),
    },
    CommandInfo {
        name: "GRID",
        aliases: &[],
        description: "Set the grid's spacing, or turn it on or off (F7)",
        make: Some(|| Box::new(crate::sysvar::Grid::default())),
    },
    CommandInfo { name: "REGEN", aliases: &["RE"], description: "Regenerate the drawing", make: None },
    CommandInfo { name: "REGENALL", aliases: &["REA"], description: "Regenerate every viewport", make: None },
    CommandInfo { name: "REDRAW", aliases: &["R"], description: "Redraw the drawing", make: None },
    CommandInfo { name: "REDRAWALL", aliases: &["RA"], description: "Redraw every viewport", make: None },
    CommandInfo {
        name: "MULTIPLE",
        aliases: &[],
        description: "Repeat the next command until Esc",
        make: Some(|| Box::new(Multiple)),
    },
    CommandInfo {
        name: "CHPROP",
        aliases: &[],
        description: "Change objects' color, layer, linetype, lineweight or transparency",
        make: Some(|| Box::new(crate::change::ChProp::default())),
    },
    CommandInfo {
        name: "CHANGE",
        aliases: &["-CH"],
        description: "Move line ends or resize circles to a point, edit texts, or change properties",
        make: Some(|| Box::new(crate::change::Change::default())),
    },
    CommandInfo { name: "DBLIST", aliases: &[], description: "List every object in the drawing", make: None },
    CommandInfo {
        name: "SOLID",
        aliases: &["SO"],
        description: "Draw filled triangles and quadrilaterals",
        make: Some(|| Box::new(crate::change::Solid::default())),
    },
    CommandInfo {
        name: "TXT2MTXT",
        aliases: &[],
        description: "Combine single-line texts into one multiline text",
        make: Some(|| Box::new(crate::change::Txt2Mtxt)),
    },
    CommandInfo { name: "SELECTALL", aliases: &["AI_SELALL"], description: "Select every object", make: None },
    CommandInfo {
        name: "DSETTINGS",
        aliases: &["DS", "SE", "DDRMODES"],
        description: "Snap and grid, polar tracking, object snaps and dynamic input (Drafting Settings)",
        make: None,
    },
    CommandInfo {
        name: "OSNAP",
        aliases: &["OS", "DDOSNAP"],
        description: "Choose the running object snaps (Drafting Settings' Object Snap tab)",
        make: None,
    },
    CommandInfo {
        name: "PTYPE",
        aliases: &["DDPTYPE"],
        description: "Choose how points look, and their size (Point Style)",
        make: None,
    },
    CommandInfo {
        name: "SETBYLAYER",
        aliases: &[],
        description: "Make the selected objects' color, linetype and lineweight ByLayer",
        make: Some(|| Box::new(SetByLayer::default())),
    },
    CommandInfo {
        name: "NCOPY",
        aliases: &[],
        description: "Copy objects from inside block references and external references",
        make: Some(|| Box::new(NCopy::default())),
    },
    CommandInfo {
        name: "ATTEDIT",
        aliases: &["ATE"],
        description: "Edit a block reference's attribute values",
        make: Some(|| Box::new(AttEdit)),
    },
    CommandInfo {
        name: "-ATTEDIT",
        aliases: &["-ATE"],
        description: "Change attribute values across block references",
        make: Some(|| Box::new(AttEditGlobal::default())),
    },
    CommandInfo { name: "PASTEBLOCK", aliases: &[], description: "Paste the clipboard as a block", make: None },
    CommandInfo {
        name: "BATTMAN",
        aliases: &[],
        description: "Manage a block's attribute definitions (Block Attribute Manager)",
        make: None,
    },
    CommandInfo {
        name: "BOUNDARY",
        aliases: &["BO", "BPOLY"],
        description: "Draw closed polylines around an area picked inside",
        make: Some(|| Box::new(Boundary::default())),
    },
    CommandInfo {
        name: "BLEND",
        aliases: &["BLENDCURVES"],
        description: "Join two objects' ends with a smooth spline",
        make: Some(|| Box::new(Blend::default())),
    },
    CommandInfo {
        name: "CLOSE",
        aliases: &["CLOSEALL"],
        description: "Close the drawing (asks to save changes)",
        make: None,
    },
    CommandInfo { name: "QUIT", aliases: &["EXIT"], description: "Quit Oxidraft (asks to save changes)", make: None },
    CommandInfo {
        name: "RECOVER",
        aliases: &[],
        description: "Open a damaged drawing and fix what AUDIT finds",
        make: None,
    },
    CommandInfo {
        name: "SCRIPT",
        aliases: &["SCR"],
        description: "Run the commands in a script (.scr) file",
        make: None,
    },
    CommandInfo {
        name: "DWGPROPS",
        aliases: &[],
        description: "The drawing's title, subject, author, keywords and comments",
        make: None,
    },
    CommandInfo {
        name: "ISODRAFT",
        aliases: &[],
        description: "Draft isometrically on a plane, or orthographically",
        make: Some(|| Box::new(crate::views::IsoDraft)),
    },
    CommandInfo {
        name: "ISOPLANE",
        aliases: &[],
        description: "Choose the isometric plane: Left, Top or Right (F5)",
        make: Some(|| Box::new(crate::views::IsoPlaneCmd::default())),
    },
    CommandInfo {
        name: "VPLAYER",
        aliases: &[],
        description: "Freeze or thaw layers in layout viewports by name: all, selected, current or all but current",
        make: Some(|| Box::new(crate::layer_tools::VpLayer::default())),
    },
    CommandInfo {
        name: "ADJUST",
        aliases: &[],
        description: "Adjust an image's brightness, contrast and fade, or a PDF underlay's fade and monochrome",
        make: Some(|| Box::new(crate::picture_tools::Adjust::default())),
    },
    CommandInfo {
        name: "IMAGEFRAME",
        aliases: &[],
        description: "Image frames: 0 hidden, 1 shown and plotted, 2 shown but not plotted",
        make: Some(|| Box::new(crate::picture_tools::FrameVar::new(crate::picture_tools::Frames::Image))),
    },
    CommandInfo {
        name: "PDFFRAME",
        aliases: &[],
        description: "PDF underlay frames: 0 hidden, 1 shown and plotted, 2 shown but not plotted",
        make: Some(|| Box::new(crate::picture_tools::FrameVar::new(crate::picture_tools::Frames::Pdf))),
    },
    CommandInfo {
        name: "FRAME",
        aliases: &[],
        description: "Frames of images, PDF underlays and wipeouts together (IMAGEFRAME, PDFFRAME, WIPEOUTFRAME)",
        make: Some(|| Box::new(crate::picture_tools::FrameVar::new(crate::picture_tools::Frames::All))),
    },
    CommandInfo {
        name: "DIMINSPECT",
        aliases: &["-DIMINSPECT"],
        description: "Add inspection data (a shape, label and inspection rate) to dimensions, or remove it",
        make: Some(|| Box::new(crate::dimedit::DimInspect::default())),
    },
    CommandInfo {
        name: "FIELD",
        aliases: &[],
        description: "Insert a field: text showing the date, the file name, the drawing's properties or an object's area",
        make: Some(|| Box::new(crate::field_cmd::FieldText::default())),
    },
    CommandInfo {
        name: "UPDATEFIELD",
        aliases: &[],
        description: "Update the fields in the selected objects",
        make: Some(|| Box::new(crate::field_cmd::UpdateField)),
    },
    CommandInfo {
        name: "TEXTEDIT",
        aliases: &["ED", "DDEDIT"],
        description: "Edit text, attribute values or a table's cells (or double-click them)",
        make: Some(|| Box::new(crate::field_cmd::TextEdit::default())),
    },
    CommandInfo {
        name: "HYPERLINK",
        aliases: &["-HYPERLINK"],
        description: "Link objects to a web page or file (Cmd/Ctrl-click to follow), or remove links",
        make: Some(|| Box::new(HyperlinkCmd::default())),
    },
    CommandInfo {
        name: "EXPORTPDF",
        aliases: &[],
        description: "Save the current layout, or all of them, as a PDF",
        make: None,
    },
    CommandInfo {
        name: "PUBLISH",
        aliases: &[],
        description: "Plot several sheets, from this drawing and others, to one PDF (Batch Plot)",
        make: None,
    },
    CommandInfo {
        name: "LAYVPI",
        aliases: &[],
        description: "Freeze the selected objects' layers in every viewport but one",
        make: Some(|| Box::new(LayVpi::default())),
    },
    CommandInfo {
        name: "LAYERSTATE",
        aliases: &["LAS"],
        description: "Save and restore named layer settings (Layer States Manager)",
        make: None,
    },
    CommandInfo {
        name: "LAYWALK",
        aliases: &[],
        description: "Show layers one at a time, or a few, to see what's on them",
        make: None,
    },
    CommandInfo {
        name: "GROUP",
        aliases: &["G"],
        description: "Make the selected objects a group, picked together",
        make: Some(|| Box::new(GroupCommand::default())),
    },
    CommandInfo {
        name: "UNGROUP",
        aliases: &["UNG"],
        description: "Take a group apart",
        make: Some(|| Box::new(Ungroup::default())),
    },
    CommandInfo {
        name: "GROUPEDIT",
        aliases: &[],
        description: "Add objects to a group, remove them, or rename it",
        make: Some(|| Box::new(GroupEdit::default())),
    },
    CommandInfo {
        name: "ALIGN",
        aliases: &["AL"],
        description: "Move, turn and scale objects so two points land on two others",
        make: Some(|| Box::new(Align::default())),
    },
    CommandInfo {
        name: "REVERSE",
        aliases: &[],
        description: "Reverse the direction of lines, polylines and splines",
        make: Some(|| Box::new(Reverse)),
    },
    CommandInfo {
        name: "ARRAYPATH",
        aliases: &[],
        description: "Copy objects evenly along a path curve",
        make: Some(|| Box::new(ArrayPath::default())),
    },
    CommandInfo {
        name: "CHSPACE",
        aliases: &[],
        description: "Move objects between a layout's paper space and model space through a viewport",
        make: Some(|| Box::new(ChangeSpace)),
    },
    CommandInfo {
        name: "WIPEOUT",
        aliases: &[],
        description: "Hide what's under a polygon of points or a closed polyline; Frames shows or hides frames",
        make: Some(|| Box::new(Wipeout::default())),
    },
    CommandInfo {
        name: "TABLE",
        aliases: &["TB"],
        description: "Insert a table of rows and columns, then fill it in",
        make: None,
    },
    CommandInfo {
        name: "TABLEDIT",
        aliases: &[],
        description: "Edit the cells of a table",
        make: Some(|| Box::new(TableEdit)),
    },
    CommandInfo {
        name: "DIMSTYLE",
        aliases: &["D", "DST"],
        description: "Set the dimension style: text height, arrows, extension lines, gaps, precision, scale",
        make: None,
    },
    CommandInfo { name: "FIND", aliases: &[], description: "Find and replace text", make: None },
    CommandInfo {
        name: "SPELL",
        aliases: &["SP"],
        description: "Check the spelling of text in the drawing",
        make: None,
    },
    CommandInfo {
        name: "SCALETEXT",
        aliases: &[],
        description: "Give text a new height, or scale it, each about its own point",
        make: Some(|| Box::new(ScaleText::default())),
    },
    CommandInfo {
        name: "JUSTIFYTEXT",
        aliases: &[],
        description: "Hang text from another of its points without moving it",
        make: Some(|| Box::new(JustifyText::default())),
    },
    CommandInfo {
        name: "DIMORDINATE",
        aliases: &["DOR"],
        description: "Dimension a feature's X or Y with a leader",
        make: Some(|| Box::new(DimOrdinate::default())),
    },
    CommandInfo {
        name: "DIMARC",
        aliases: &["DAR"],
        description: "Dimension the length of an arc",
        make: Some(|| Box::new(DimArc::default())),
    },
    CommandInfo {
        name: "DIMJOGGED",
        aliases: &["DJO", "JOG"],
        description: "Dimension a large arc's radius from a stand-in center, with a jog",
        make: Some(|| Box::new(DimJogged::default())),
    },
    CommandInfo {
        name: "QDIM",
        aliases: &[],
        description: "Dimension the points of the selected objects at once, continued or from a baseline",
        make: Some(|| Box::new(QDim::default())),
    },
    CommandInfo {
        name: "CENTERMARK",
        aliases: &["CM", "DIMCENTER", "DCE"],
        description: "Mark circles' and arcs' centers with center lines",
        make: Some(|| Box::new(CenterMark)),
    },
    CommandInfo {
        name: "CENTERLINE",
        aliases: &["CL"],
        description: "Draw the center line between two lines",
        make: Some(|| Box::new(CenterLine::default())),
    },
    CommandInfo {
        name: "MLEADER",
        aliases: &["MLD"],
        description: "Create a leader with text",
        make: Some(|| Box::new(MLeader::default())),
    },
    CommandInfo {
        name: "HATCH",
        aliases: &["H", "BH"],
        description: "Fill an enclosed area with a pattern",
        make: Some(|| Box::new(Hatch::default())),
    },
    CommandInfo {
        name: "GRADIENT",
        aliases: &["GD"],
        description: "Fill an enclosed area with a gradient (Pattern: LINEAR, SPHERICAL, …)",
        make: Some(|| Box::new(Hatch::gradient())),
    },
    CommandInfo {
        name: "HATCHEDIT",
        aliases: &["HE", "-HATCHEDIT"],
        description: "Change a hatch's pattern, draw order or boundaries",
        make: Some(|| Box::new(HatchEdit::default())),
    },
    CommandInfo {
        name: "DIST",
        aliases: &["DI"],
        description: "Measure the distance and angle between two points",
        make: Some(|| Box::new(Dist::default())),
    },
    CommandInfo {
        name: "AREA",
        aliases: &["AA"],
        description: "Measure the area and perimeter of points or an object",
        make: Some(|| Box::new(Area::default())),
    },
    CommandInfo {
        name: "ID",
        aliases: &[],
        description: "Show the coordinates of a point",
        make: Some(|| Box::new(Id)),
    },
    CommandInfo {
        name: "SNAP",
        aliases: &["SN"],
        description: "Set the snap spacing, or turn snap mode on or off (F9)",
        make: Some(|| Box::new(Snap::default())),
    },
    CommandInfo {
        name: "LTSCALE",
        aliases: &["LTS"],
        description: "Set the global linetype scale",
        make: Some(|| Box::new(Ltscale::default())),
    },
    CommandInfo {
        name: "COLORTHEME",
        aliases: &[],
        description: "Set the color theme: 0 dark, 1 light",
        make: Some(|| Box::new(ColorTheme::default())),
    },
    CommandInfo {
        name: "ZOOM",
        aliases: &["Z"],
        description: "Zoom to a window, a scale, the extents, the previous view, …",
        make: Some(|| Box::new(Zoom::default())),
    },
    CommandInfo { name: "PAN", aliases: &["P"], description: "Pan the view by dragging", make: None },
    CommandInfo {
        name: "SELECTSIMILAR",
        aliases: &[],
        description: "Select all objects like the selected ones",
        make: Some(|| Box::new(SelectSimilar::default())),
    },
    CommandInfo {
        name: "QSELECT",
        aliases: &[],
        description: "Select objects by type and property (Quick Select)",
        make: None,
    },
    CommandInfo {
        name: "COPYCLIP",
        aliases: &[],
        description: "Copy objects to the clipboard (Cmd+C)",
        make: Some(|| Box::new(CopyClip::copy())),
    },
    CommandInfo {
        name: "CUTCLIP",
        aliases: &[],
        description: "Cut objects to the clipboard (Cmd+X)",
        make: Some(|| Box::new(CopyClip::cut())),
    },
    CommandInfo {
        name: "COPYBASE",
        aliases: &[],
        description: "Copy objects to the clipboard with a base point (Cmd+Shift+C)",
        make: Some(|| Box::new(CopyClip::with_base())),
    },
    // Handled by the app, which reads the clipboard.
    CommandInfo {
        name: "PASTECLIP",
        aliases: &[],
        description: "Paste objects from the clipboard (Cmd+V)",
        make: None,
    },
    CommandInfo {
        name: "PASTEORIG",
        aliases: &[],
        description: "Paste objects at the coordinates they were copied from",
        make: None,
    },
    CommandInfo {
        name: "PEDIT",
        aliases: &["PE"],
        description: "Edit polylines: close, open, join, decurve, reverse",
        make: Some(|| Box::new(Pedit::default())),
    },
    CommandInfo {
        name: "POINT",
        aliases: &["PO"],
        description: "Place a point object",
        make: Some(|| Box::new(PointCmd)),
    },
    CommandInfo {
        name: "DIVIDE",
        aliases: &["DIV"],
        description: "Place points or blocks that divide an object into equal parts",
        make: Some(|| Box::new(Marks::divide())),
    },
    CommandInfo {
        name: "MEASURE",
        aliases: &["ME"],
        description: "Place points or blocks at set distances along an object",
        make: Some(|| Box::new(Marks::measure())),
    },
    CommandInfo {
        name: "PDMODE",
        aliases: &[],
        description: "Set how points are drawn",
        make: Some(|| Box::new(PointStyle::mode())),
    },
    CommandInfo {
        name: "PDSIZE",
        aliases: &[],
        description: "Set the size points are drawn at",
        make: Some(|| Box::new(PointStyle::size())),
    },
    CommandInfo {
        name: "LIST",
        aliases: &["LI", "LS"],
        description: "List the properties of selected objects",
        make: Some(|| Box::new(List)),
    },
    CommandInfo {
        name: "EATTEDIT",
        aliases: &[],
        description: "Edit the attribute values of a block",
        make: Some(|| Box::new(EattEdit)),
    },
    CommandInfo {
        name: "ATTDISP",
        aliases: &[],
        description: "Show attributes as set, all of them, or none",
        make: Some(|| Box::new(AttDisp::default())),
    },
    CommandInfo {
        name: "LAYISO",
        aliases: &[],
        description: "Isolate the layers of selected objects",
        make: Some(|| Box::new(LayIso::default())),
    },
    CommandInfo { name: "LAYUNISO", aliases: &[], description: "Restore the layers LAYISO changed", make: None },
    CommandInfo {
        name: "LAYOFF",
        aliases: &[],
        description: "Turn off the layer of a selected object",
        make: Some(|| Box::new(LayerPick::off())),
    },
    CommandInfo {
        name: "LAYFRZ",
        aliases: &[],
        description: "Freeze the layer of a selected object",
        make: Some(|| Box::new(LayerPick::freeze())),
    },
    CommandInfo {
        name: "LAYLCK",
        aliases: &[],
        description: "Lock the layer of a selected object",
        make: Some(|| Box::new(LayerPick::lock())),
    },
    CommandInfo {
        name: "LAYULK",
        aliases: &[],
        description: "Unlock the layer of a selected object",
        make: Some(|| Box::new(LayerPick::unlock())),
    },
    CommandInfo {
        name: "DRAWORDER",
        aliases: &["DR"],
        description: "Bring objects to the front, send them to the back, or put them above or under others",
        make: Some(|| Box::new(DrawOrder::new(None))),
    },
    CommandInfo {
        name: "TEXTTOFRONT",
        aliases: &[],
        description: "Bring text, dimensions and leaders to the front",
        make: Some(|| Box::new(TextToFront)),
    },
    CommandInfo { name: "HATCHTOBACK", aliases: &[], description: "Send every hatch to the back", make: None },
    CommandInfo { name: "LAYON", aliases: &[], description: "Turn on all layers", make: None },
    CommandInfo { name: "LAYTHW", aliases: &[], description: "Thaw all layers", make: None },
    CommandInfo {
        name: "LAYMCUR",
        aliases: &[],
        description: "Make the layer of a selected object current",
        make: Some(|| Box::new(LayMcur)),
    },
    // Handled by the app, which owns the windows and palettes.
    CommandInfo { name: "LAYER", aliases: &["LA"], description: "Open the Layer Properties Manager", make: None },
    CommandInfo { name: "PLOT", aliases: &["PRINT"], description: "Plot the drawing to a PDF file", make: None },
    CommandInfo {
        name: "EXTERNALREFERENCES",
        aliases: &["ER", "XREF", "XR"],
        description: "Open the External References palette",
        make: None,
    },
    CommandInfo {
        name: "XATTACH",
        aliases: &["XA"],
        description: "Attach a drawing as an external reference",
        make: None,
    },
    CommandInfo {
        name: "IMAGEATTACH",
        aliases: &["IAT"],
        description: "Attach images: one placed by hand or by its georeferencing, or many GeoTIFFs at once",
        make: None,
    },
    CommandInfo { name: "PDFATTACH", aliases: &[], description: "Attach a page of a PDF as an underlay", make: None },
    CommandInfo {
        name: "IMAGECLIP",
        aliases: &["ICL", "PDFCLIP"],
        description: "Clip an image or PDF underlay to a rectangle or polygon",
        make: Some(|| Box::new(ImageClip::default())),
    },
    CommandInfo {
        name: "LAYOUT",
        aliases: &["LO"],
        description: "Make, copy, rename, delete or switch to a layout",
        make: Some(|| Box::new(LayoutCommand::default())),
    },
    CommandInfo {
        name: "MVIEW",
        aliases: &["MV"],
        description: "Make viewports onto model space in a layout, or turn them on, off or locked",
        make: Some(|| Box::new(Mview::default())),
    },
    CommandInfo {
        name: "VPCLIP",
        aliases: &[],
        description: "Clip a layout viewport to a closed shape or points, or make it rectangular again",
        make: Some(|| Box::new(VpClip::default())),
    },
    CommandInfo {
        name: "VPMAX",
        aliases: &[],
        description: "Work in model space through a layout viewport filling the drawing area",
        make: Some(|| Box::new(VpMax::default())),
    },
    CommandInfo {
        name: "XCLIP",
        aliases: &["XC"],
        description: "Clip a block reference or xref to a rectangle, polygon or polyline",
        make: Some(|| Box::new(XClipCommand::default())),
    },
    // Handled by the app, which owns the dialogs and palettes.
    CommandInfo {
        name: "BEDIT",
        aliases: &["BE"],
        description: "Edit a block definition in the Block Editor",
        make: None,
    },
    CommandInfo { name: "BSAVE", aliases: &[], description: "Save the block in the Block Editor", make: None },
    CommandInfo { name: "BCLOSE", aliases: &[], description: "Close the Block Editor", make: None },
    CommandInfo {
        name: "BLOCKSPALETTE",
        aliases: &[],
        description: "Open the Blocks palette: insert blocks from this drawing or a library",
        make: None,
    },
    CommandInfo {
        name: "REFEDIT",
        aliases: &[],
        description: "Edit a block reference or xref in place",
        make: Some(|| Box::new(RefEdit)),
    },
    CommandInfo {
        name: "REFSET",
        aliases: &[],
        description: "Add objects to or remove them from the reference being edited",
        make: Some(|| Box::new(RefSet::new(None))),
    },
    CommandInfo {
        name: "REFCLOSE",
        aliases: &[],
        description: "Save or discard the edits to the reference being edited",
        make: Some(|| Box::new(RefClose::new(None))),
    },
    CommandInfo {
        name: "CLIP",
        aliases: &[],
        description: "Clip an xref, block reference, image or PDF underlay",
        make: Some(|| Box::new(Clip::default())),
    },
    CommandInfo {
        name: "OPTIONS",
        aliases: &["OP"],
        description: "Open the Options dialog: color theme and drawing background",
        make: None,
    },
    CommandInfo {
        name: "MSPACE",
        aliases: &["MS"],
        description: "Work in model space through a layout viewport",
        make: None,
    },
    CommandInfo { name: "PSPACE", aliases: &["PS"], description: "Go back to a layout's paper space", make: None },
    CommandInfo { name: "VPMIN", aliases: &[], description: "Go back to the layout from VPMAX", make: None },
    CommandInfo {
        name: "PURGE",
        aliases: &["PU"],
        description: "Remove unused layers, blocks and linetypes, and empty objects",
        make: None,
    },
    CommandInfo {
        name: "-PURGE",
        aliases: &[],
        description: "Purge on the command line: Blocks, LAyers, LTypes, Zero-length geometry, Empty text, All",
        make: Some(|| Box::new(Purge)),
    },
    CommandInfo {
        name: "OVERKILL",
        aliases: &["OV"],
        description: "Delete duplicate objects and join lines lying along each other",
        make: Some(|| Box::new(Overkill)),
    },
    CommandInfo {
        name: "AUDIT",
        aliases: &[],
        description: "Check the drawing for broken objects, and mend them",
        make: Some(|| Box::new(Audit)),
    },
    CommandInfo {
        name: "COUNT",
        aliases: &[],
        description: "Count a block's references, or list every block's count",
        make: Some(|| Box::new(Count)),
    },
    CommandInfo {
        name: "COMPARE",
        aliases: &[],
        description: "Compare this drawing with another: what's only here, only there, or the same",
        make: None,
    },
    CommandInfo { name: "COMPAREEXIT", aliases: &[], description: "Stop comparing drawings", make: None },
    CommandInfo { name: "ALIASEDIT", aliases: &[], description: "Edit your own command aliases", make: None },
    CommandInfo {
        name: "VPORTS",
        aliases: &["-VPORTS"],
        description: "Split model space's drawing area into 1, 2, 3 or 4 viewports",
        make: Some(|| Box::new(Vports::default())),
    },
    CommandInfo {
        name: "VIEW",
        aliases: &["V"],
        description: "Open the View Manager: save, restore and delete named views",
        make: None,
    },
    CommandInfo {
        name: "-VIEW",
        aliases: &[],
        description: "Save, restore, delete or list named views on the command line",
        make: Some(|| Box::new(ViewCommand::default())),
    },
    CommandInfo {
        name: "UCSICON",
        aliases: &[],
        description: "Show or hide the UCS icon",
        make: Some(|| Box::new(Toggle::new("UCS icon", |v| &mut v.ucs_icon))),
    },
    CommandInfo {
        name: "NAVBAR",
        aliases: &[],
        description: "Show or hide the navigation bar",
        make: Some(|| Box::new(Toggle::new("navigation bar", |v| &mut v.nav_bar))),
    },
    CommandInfo { name: "FILETAB", aliases: &[], description: "Show the file tabs", make: None },
    CommandInfo { name: "FILETABCLOSE", aliases: &[], description: "Hide the file tabs", make: None },
    CommandInfo { name: "PREVIEW", aliases: &["PRE"], description: "Show how the drawing will plot", make: None },
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

/// True if a command (or alias) has this name.
pub fn is_command(name: &str) -> bool {
    find(name).is_some()
}

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

/// What can be typed at "Select objects:" besides picking, as in AutoCAD:
/// W, L, C, ALL, F, WP, CP, A, R, P and U.
const SELECT_OPTIONS: &[&str] =
    &["Window", "Last", "Crossing", "ALL", "Fence", "WPolygon", "CPolygon", "Add", "Remove", "Previous", "Undo"];

/// A "Select objects:" option being given its points.
#[derive(Debug, Clone, PartialEq)]
enum Gathering {
    /// Window or Crossing, whichever way it's dragged: the first corner
    /// once given.
    Window { crossing: bool, first: Option<Point> },
    /// WPolygon or CPolygon: Enter closes it.
    Polygon { crossing: bool, points: Vec<Point> },
    /// Fence: Enter ends it.
    Fence { points: Vec<Point> },
}

/// Runs commands against a document, owns the selection set, and keeps the
/// command-line history.
pub struct Engine {
    active: Option<Active>,
    /// MULTIPLE: the command started again each time it ends.
    repeat: Option<&'static CommandInfo>,
    last_command: Option<&'static CommandInfo>,
    last_point: Option<Point>,
    history: Vec<String>,
    /// Commands and inputs typed so far, for Up-arrow recall.
    typed: Vec<String>,
    /// Commands started, and inputs typed at command prompts, for the
    /// shortcut menu's Recent Input.
    started: Vec<&'static str>,
    inputs: Vec<String>,
    /// Selected objects: the pickfirst set when idle, or the set being built
    /// at a "Select objects:" prompt.
    selection: Vec<EntityId>,
    /// Windows used to build `selection` (for STRETCH).
    selection_windows: Vec<Aabb>,
    /// The object picked from the Selection list, for the next pick.
    chosen: Option<EntityId>,
    /// At "Select objects:": an option's points being given, Remove mode
    /// (picks take objects out), what each pick added (for Undo), and the
    /// last selection a command got (for Previous).
    gathering: Option<Gathering>,
    removing: bool,
    added: Vec<Vec<EntityId>>,
    previous: Vec<EntityId>,
    /// Lassos used to build `selection` (for STRETCH too).
    selection_lassos: Vec<Vec<Point>>,
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
    /// The user's own command aliases (ALIASEDIT), as (alias, command);
    /// they come before the built-in ones.
    pub aliases: Vec<(String, String)>,
    /// View changes asked for since the app last took them.
    view_requests: Vec<ViewRequest>,
    ui_requests: Vec<UiRequest>,
    /// What was last copied, until the app puts it on the clipboard.
    copied: Option<Document>,
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
            repeat: None,
            active: None,
            last_command: None,
            last_point: None,
            history: Vec::new(),
            typed: Vec::new(),
            started: Vec::new(),
            inputs: Vec::new(),
            selection: Vec::new(),
            selection_windows: Vec::new(),
            selection_lassos: Vec::new(),
            chosen: None,
            gathering: None,
            removing: false,
            added: Vec::new(),
            previous: Vec::new(),
            window_start: None,
            ortho: false,
            polar: true,
            polar_increment: 90.0,
            dynamic_input: true,
            vars: Vars::default(),
            aliases: Vec::new(),
            view_requests: Vec::new(),
            ui_requests: Vec::new(),
            copied: None,
        }
    }

    // ---- state queries ------------------------------------------------------

    /// View changes commands asked for since the last call, oldest first.
    pub fn take_view_requests(&mut self) -> Vec<ViewRequest> {
        std::mem::take(&mut self.view_requests)
    }

    /// What COPYCLIP, CUTCLIP or COPYBASE just copied: a drawing for the
    /// app to put on the clipboard.
    pub fn take_copied(&mut self) -> Option<Document> {
        self.copied.take()
    }

    /// PASTECLIP (or PASTEORIG, at the copied place) with a clipboard drawing.
    /// XATTACH, once its dialog is filled in: places the attached drawing at
    /// `at`, or asks for the insertion point with it on the cursor.
    pub fn attach_xref(
        &mut self,
        xref: oxidraft_model::Xref,
        content: Document,
        scale: f64,
        rotation: f64,
        at: Option<Point>,
        doc: &mut Document,
    ) {
        if self.active.is_some() {
            self.cancel(doc);
        }
        let info = find("XATTACH").expect("XATTACH is a command");
        self.log(format!("Command: {}", info.name));
        let cmd = AttachXref::new(xref.clone(), content, scale, rotation);
        match at {
            Some(p) => {
                doc.begin(info.name);
                cmd.attach(doc, p);
                doc.commit();
                self.log(format!("Attach Xref \"{}\": {}", xref.name, xref.path));
                self.log(format!("\"{}\" loaded.", xref.name));
            }
            None => self.launch(info, Box::new(cmd), doc, Point::ORIGIN, 0.0),
        }
    }

    /// IMAGEATTACH and PDFATTACH, once their dialog is filled in: places the
    /// picture with its corner at `at`, or asks for it with the frame on
    /// the cursor.
    pub fn attach_image(
        &mut self,
        def: oxidraft_model::ImageDef,
        u: Point,
        v: Point,
        at: Option<Point>,
        doc: &mut Document,
    ) {
        if self.active.is_some() {
            self.cancel(doc);
        }
        let name = if def.is_pdf() { "PDFATTACH" } else { "IMAGEATTACH" };
        let info = find(name).expect("attaching pictures is a command");
        self.log(format!("Command: {}", info.name));
        let cmd = AttachImage::new(def, u, v);
        match at {
            Some(p) => {
                doc.begin(info.name);
                cmd.attach(doc, p);
                doc.commit();
            }
            None => self.launch(info, Box::new(cmd), doc, Point::ORIGIN, 0.0),
        }
    }

    /// TABLE once the Insert Table dialog has set the table up: it's
    /// placed at the point picked.
    pub fn insert_table(&mut self, table: Geometry, doc: &mut Document) {
        if self.active.is_some() {
            self.cancel(doc);
        }
        let info = find("TABLE").expect("TABLE is a command");
        self.log(format!("Command: {}", info.name));
        self.started.push(info.name);
        self.launch(info, Box::new(TableInsert::new(table)), doc, Point::ORIGIN, 0.0);
    }

    /// DRAWORDER with its option chosen already (the ribbon's and the
    /// shortcut menu's Bring to Front, Send to Back, Above and Under):
    /// "Front", "Back", "Above objects" or "Under objects".
    pub fn draw_order(&mut self, option: &'static str, doc: &mut Document) {
        if self.active.is_some() {
            self.cancel(doc);
        }
        let info = find("DRAWORDER").expect("DRAWORDER is a command");
        self.log(format!("Command: {}", info.name));
        self.started.push(info.name);
        self.launch(info, Box::new(DrawOrder::new(Some(option))), doc, Point::ORIGIN, 0.0);
    }

    /// The shortcut menu's Edit Xref In-place (or Edit Block In-place):
    /// REFEDIT on the selected reference.
    pub fn refedit_selected(&mut self, doc: &mut Document) {
        let reference = self
            .selection
            .iter()
            .copied()
            .find(|id| matches!(doc.entity(*id).map(|e| &e.geometry), Some(Geometry::Insert { .. })));
        let Some(id) = reference else { return };
        self.cancel(doc);
        let info = find("REFEDIT").expect("REFEDIT is a command");
        self.log(format!("Command: {}", info.name));
        self.started.push(info.name);
        self.last_command = Some(info);
        doc.begin(info.name);
        match crate::refedit::open(doc, id) {
            Ok(lines) => lines.into_iter().for_each(|l| self.log(l)),
            Err(e) => self.log(e),
        }
        doc.commit();
    }

    /// The Blocks palette: INSERT with block `name` chosen, its preview on
    /// the cursor until the insertion point is picked.
    pub fn insert_block(&mut self, name: &str, doc: &mut Document) {
        self.vars.insert_name = Some(name.to_string());
        self.run("INSERT", doc);
        self.feed(Input::Enter, doc, Point::ORIGIN, 0.0);
    }

    /// The Edit Reference panel's buttons: REFCLOSE's "Save" or
    /// "Discard", or REFSET's "Add" or "Remove".
    pub fn edit_reference(&mut self, action: &'static str, doc: &mut Document) {
        if self.active.is_some() {
            self.cancel(doc);
        }
        let (name, cmd): (&str, Box<dyn Command>) = match action {
            "Save" | "Discard" => ("REFCLOSE", Box::new(RefClose::new(Some(action == "Save")))),
            _ => ("REFSET", Box::new(RefSet::new(Some(action == "Add")))),
        };
        let info = find(name).expect("REFCLOSE and REFSET are commands");
        self.log(format!("Command: {}", info.name));
        self.started.push(info.name);
        self.launch(info, cmd, doc, Point::ORIGIN, 0.0);
        if name == "REFCLOSE" && self.active.is_some() {
            self.feed(Input::Enter, doc, Point::ORIGIN, 0.0);
        }
    }

    /// The Plot dialog's Window< button: asks for the window's corners,
    /// then hands it back as [`UiRequest::PlotWindow`].
    pub fn pick_plot_window(&mut self, doc: &mut Document) {
        if self.active.is_some() {
            self.cancel(doc);
        }
        let info = find("PLOT").expect("PLOT is a command");
        self.launch(info, Box::new(PlotWindow::default()), doc, Point::ORIGIN, 0.0);
    }

    /// PASTEBLOCK: the clipboard's objects as a block reference to place.
    pub fn paste_block(&mut self, clip: Document, doc: &mut Document) {
        if self.active.is_some() {
            self.cancel(doc);
        }
        self.log("Command: PASTEBLOCK".to_string());
        let info = find("PASTEBLOCK").expect("PASTEBLOCK is a command");
        self.started.push(info.name);
        doc.begin(info.name);
        let one = crate::block_tools::as_block(doc, &clip);
        doc.commit();
        self.launch(
            find("PASTECLIP").expect("PASTECLIP is a command"),
            Box::new(Paste::new(one)),
            doc,
            Point::ORIGIN,
            0.0,
        );
    }

    pub fn paste(&mut self, clip: Document, original: bool, doc: &mut Document) {
        if self.active.is_some() {
            self.cancel(doc);
        }
        let name = if original { "PASTEORIG" } else { "PASTECLIP" };
        self.log(format!("Command: {name}"));
        if original {
            doc.begin(name);
            crate::clipboard::paste(doc, &clip, Point::ORIGIN);
            doc.commit();
            return;
        }
        let info = find(name).expect("PASTECLIP is a command");
        self.started.push(info.name);
        self.launch(info, Box::new(Paste::new(clip)), doc, Point::ORIGIN, 0.0);
    }

    /// Dialogs and windows commands asked the app to show.
    pub fn take_ui_requests(&mut self) -> Vec<UiRequest> {
        std::mem::take(&mut self.ui_requests)
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Puts a line in the history before the one at `index`, as the
    /// "Command:" line of a command the app ran after it printed.
    pub fn log_at(&mut self, index: usize, line: impl Into<String>) {
        self.history.insert(index.min(self.history.len()), line.into());
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
        self.selection_prompt().or_else(|| self.active.as_ref().map(|a| a.cmd.prompt()))
    }

    /// At "Select objects:", the prompt of an option being given its
    /// points, or Remove mode's.
    fn selection_prompt(&self) -> Option<Prompt> {
        if self.active.is_none() || !self.is_selecting() {
            return None;
        }
        Some(match &self.gathering {
            Some(Gathering::Window { first: None, .. }) => Prompt::new("Specify first corner"),
            Some(Gathering::Window { first: Some(_), .. }) => Prompt::new("Specify opposite corner"),
            Some(Gathering::Polygon { points, .. }) if points.is_empty() => Prompt::new("First polygon point"),
            Some(Gathering::Polygon { .. }) => Prompt::new("Specify endpoint of line").with_keywords(&["Undo"]),
            Some(Gathering::Fence { points }) if points.is_empty() => Prompt::new("Specify first fence point"),
            Some(Gathering::Fence { .. }) => Prompt::new("Specify next fence point").with_keywords(&["Undo"]),
            None if self.removing => Prompt::new("Remove objects"),
            None => return None,
        })
    }

    /// Text to show before the command-line cursor.
    pub fn prompt(&self) -> String {
        match self.current_prompt() {
            Some(p) => p.to_string(),
            None => "Command:".into(),
        }
    }

    pub fn preview(&self, cursor: Point) -> Vec<Geometry> {
        // A selection option's window, polygon or fence, rubber-banded.
        let ring = |points: &[Point], closed: bool| {
            let vertices = points.iter().chain([&cursor]).map(|p| oxidraft_model::Vertex::at(*p)).collect();
            vec![Geometry::Polyline { vertices, closed }]
        };
        match &self.gathering {
            Some(_) if self.active.is_none() => {}
            Some(Gathering::Window { first: Some(a), .. }) => {
                return ring(&[*a, Point::new(cursor.x, a.y)], false)
                    .into_iter()
                    .chain(ring(&[Point::new(a.x, cursor.y), *a], false))
                    .collect();
            }
            Some(Gathering::Polygon { points, .. }) if !points.is_empty() => return ring(points, true),
            Some(Gathering::Fence { points }) if !points.is_empty() => return ring(points, false),
            _ => {}
        }
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

    /// The command Enter would repeat.
    pub fn last_command(&self) -> Option<&'static str> {
        self.last_command.map(|c| c.name)
    }

    /// Recently typed commands, newest first, without repeats.
    pub fn recent_commands(&self, max: usize) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for &name in self.started.iter().rev() {
            if !out.contains(&name) {
                out.push(name);
            }
        }
        out.truncate(max);
        out
    }

    /// Recently typed inputs that aren't command names (points, values), newest first.
    pub fn recent_inputs(&self, max: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in self.inputs.iter().rev() {
            if !out.contains(t) {
                out.push(t.clone());
            }
        }
        out.truncate(max);
        out
    }

    /// Selects every object that can be selected (Ctrl/Cmd+A).
    pub fn select_all(&mut self, doc: &Document) {
        if self.active.is_some() {
            return;
        }
        self.window_start = None;
        self.selection = doc.entities().filter(|e| doc.is_selectable(e)).map(|e| e.id).collect();
        self.selection_windows.clear();
        self.selection_lassos.clear();
    }

    /// Replaces the selection (QSELECT, SELECTSIMILAR), when no command runs.
    pub fn set_selection(&mut self, ids: Vec<EntityId>) {
        if self.active.is_some() {
            return;
        }
        self.window_start = None;
        self.selection = ids;
        self.selection_windows.clear();
        self.selection_lassos.clear();
    }

    pub fn selection(&self) -> &[EntityId] {
        &self.selection
    }

    /// True when clicks select objects (idle, or at a "Select objects:" prompt)
    /// rather than specify points.
    pub fn is_selecting(&self) -> bool {
        self.active.as_ref().is_none_or(|a| a.cmd.wants_selection())
    }

    /// True while typing text in which Space is a character, not Enter.
    pub fn accepts_spaces(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.cmd.wants_text() && a.cmd.text_with_spaces())
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
        // Isometric: along whichever of the plane's two axes is nearer.
        if self.ortho
            && let Some(plane) = self.vars.isoplane
        {
            let along = |a: f64| {
                let u = Point::ORIGIN.polar(1.0, a);
                base + u * d.dot(u)
            };
            let [a, b] = plane.axes();
            let (pa, pb) = (along(a), along(b));
            let point = if pa.distance(cursor) <= pb.distance(cursor) { pa } else { pb };
            return Tracked { point, polar: None };
        }
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

    /// The command a user alias stands for, or `text` as it is.
    pub fn expand_alias(&self, text: &str) -> String {
        let t = text.trim();
        self.aliases
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(t))
            .map_or_else(|| t.to_string(), |(_, command)| command.clone())
    }

    /// Handles text submitted on the command line (Enter or Space).
    pub fn submit(&mut self, text: &str, doc: &mut Document, cursor: Point, pick_tolerance: f64) {
        oxidraft_model::units::set_current(doc.units());
        let text = text.trim();
        if !text.is_empty() {
            self.typed.push(text.to_string());
            if self.active.is_some() {
                self.inputs.push(text.to_string());
            }
        }
        let expanded = if self.active.is_none() { self.expand_alias(text) } else { text.to_string() };
        let text = expanded.as_str();
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

        let prompt = self.current_prompt().unwrap_or_else(|| active.cmd.prompt());
        let selecting = active.cmd.wants_selection();
        if active.cmd.wants_text() {
            self.log(format!("{prompt} {text}"));
            let input = if text.is_empty() { Input::Enter } else { Input::Text(text.to_string()) };
            self.feed(input, doc, cursor, pick_tolerance);
            return;
        }
        let dyn_relative = self.dynamic_input && active.cmd.base_point().is_some();
        self.log(format!("{prompt} {text}"));
        if selecting && self.select_typed(text, &prompt.keywords, doc, cursor, pick_tolerance) {
            return;
        }
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
        oxidraft_model::units::set_current(doc.units());
        if self.is_selecting() && self.active.is_some() && self.gathering.is_some() {
            self.gather(raw, doc, pick_tolerance);
            return;
        }
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
        self.log(format!("{prompt} {}", oxidraft_model::units::current().point(point.x, point.y)));
        self.feed(Input::Point(point), doc, point, pick_tolerance);
    }

    /// Text typed at "Select objects:": an option, its points, or a point
    /// picking or starting a window there. Returns false for what the
    /// command takes itself (Enter, its own options).
    fn select_typed(
        &mut self,
        text: &str,
        keywords: &[&'static str],
        doc: &mut Document,
        cursor: Point,
        tolerance: f64,
    ) -> bool {
        if self.gathering.is_some() {
            match parse_input(text, self.last_point, &["Undo"], false) {
                Ok(Input::Enter) => self.finish_gathering(doc, cursor, tolerance),
                Ok(Input::Point(p)) => {
                    self.last_point = Some(p);
                    self.gather(p, doc, tolerance);
                }
                Ok(Input::Keyword(_)) => {
                    if let Some(Gathering::Polygon { points, .. } | Gathering::Fence { points }) = &mut self.gathering {
                        points.pop();
                    }
                }
                Ok(_) => self.log("Point or option keyword required."),
                Err(e) => self.log(e),
            }
            return true;
        }
        if text.is_empty() || matches!(parse_input(text, None, keywords, false), Ok(Input::Keyword(_))) {
            return false;
        }
        match parse_input(text, self.last_point, SELECT_OPTIONS, false) {
            Ok(Input::Keyword(option)) => self.select_option(option, doc, cursor, tolerance),
            Ok(Input::Point(p)) => {
                self.last_point = Some(p);
                self.select_click(p, false, doc, tolerance);
            }
            _ => {
                self.log("*Invalid selection*");
                self.log(
                    "Expects a point or Window/Last/Crossing/ALL/Fence/WPolygon/CPolygon/Add/Remove/Previous/Undo",
                );
            }
        }
        true
    }

    fn select_option(&mut self, option: &'static str, doc: &mut Document, cursor: Point, tolerance: f64) {
        let selectable = |doc: &Document| doc.entities().filter(|e| doc.is_selectable(e)).map(|e| e.id).collect();
        let found: Vec<EntityId> = match option {
            "Window" | "Crossing" => {
                self.gathering = Some(Gathering::Window { crossing: option == "Crossing", first: None });
                return;
            }
            "WPolygon" | "CPolygon" => {
                self.gathering = Some(Gathering::Polygon { crossing: option == "CPolygon", points: Vec::new() });
                return;
            }
            "Fence" => {
                self.gathering = Some(Gathering::Fence { points: Vec::new() });
                return;
            }
            "Add" | "Remove" => {
                self.removing = option == "Remove";
                return;
            }
            "Undo" => {
                let undone = self.added.pop().unwrap_or_default();
                self.selection.retain(|id| !undone.contains(id));
                return;
            }
            "ALL" => selectable(doc),
            // The newest object that can be selected.
            "Last" => {
                let all: Vec<EntityId> = selectable(doc);
                all.into_iter().max().into_iter().collect()
            }
            _ => {
                let all: Vec<EntityId> = selectable(doc);
                self.previous.iter().copied().filter(|id| all.contains(id)).collect()
            }
        };
        self.add_to_selection(doc, found, self.removing);
        self.end_single_selection(doc, cursor, tolerance);
    }

    /// A point for the option being given its points.
    fn gather(&mut self, p: Point, doc: &mut Document, tolerance: f64) {
        match &mut self.gathering {
            Some(Gathering::Window { first: first @ None, .. }) => *first = Some(p),
            Some(Gathering::Window { crossing, first: Some(a) }) => {
                // Corners as a window or a crossing window would have them.
                let b = Aabb::from_points(*a, p);
                let (from, to) = if *crossing { (b.max, b.min) } else { (b.min, b.max) };
                let ids = snap::window_select(doc, from, to);
                self.gathering = None;
                if !self.removing {
                    self.selection_windows.push(b);
                }
                self.add_to_selection(doc, ids, self.removing);
                self.end_single_selection(doc, p, tolerance);
            }
            Some(Gathering::Polygon { points, .. } | Gathering::Fence { points }) => points.push(p),
            None => {}
        }
    }

    /// Enter after an option's points: the polygon closes, the fence ends.
    fn finish_gathering(&mut self, doc: &mut Document, cursor: Point, tolerance: f64) {
        let ids = match self.gathering.take() {
            Some(Gathering::Polygon { crossing, points }) if points.len() >= 3 => {
                let ids = snap::polygon_select(doc, &points, crossing);
                if !self.removing {
                    self.selection_lassos.push(points);
                }
                ids
            }
            Some(Gathering::Fence { points }) if points.len() >= 2 => snap::fence_select(doc, &points),
            _ => return,
        };
        self.add_to_selection(doc, ids, self.removing);
        self.end_single_selection(doc, cursor, tolerance);
    }

    /// A fence dragged at a selection prompt (the lasso's Fence): the
    /// objects it crosses join the selection (Shift: leave it).
    pub fn select_fence(&mut self, path: Vec<Point>, shift: bool, doc: &mut Document, tolerance: f64) {
        if !self.is_selecting() {
            return;
        }
        self.window_start = None;
        let ids = snap::fence_select(doc, &path);
        let at = path.last().copied().unwrap_or(Point::ORIGIN);
        self.add_to_selection(doc, ids, shift != self.removing);
        self.end_single_selection(doc, at, tolerance);
    }

    fn select_click(&mut self, p: Point, shift: bool, doc: &mut Document, tolerance: f64) {
        // In Remove mode picks take objects out, and Shift puts them back.
        let shift = shift != self.removing;
        if let Some(first) = self.window_start.take() {
            let ids = snap::window_select(doc, first, p);
            if !shift {
                self.selection_windows.push(Aabb::from_points(first, p));
            }
            self.add_to_selection(doc, ids, shift);
            self.end_single_selection(doc, p, tolerance);
            return;
        }
        if self.active.is_none() && !shift && self.start_grip_edit(p, doc, tolerance) {
            return;
        }
        match snap::pick(doc, p, tolerance) {
            Some(id) => self.add_to_selection(doc, vec![id], shift),
            None => self.window_start = Some(p),
        }
        self.end_single_selection(doc, p, tolerance);
    }

    /// A lasso dragged at a selection prompt (or with no command): the
    /// objects inside it, or with `crossing` inside or crossing it, join
    /// the selection (Shift: leave it).
    pub fn select_lasso(&mut self, ring: Vec<Point>, crossing: bool, shift: bool, doc: &mut Document, tolerance: f64) {
        if !self.is_selecting() {
            return;
        }
        self.window_start = None;
        let ids = snap::polygon_select(doc, &ring, crossing);
        let at = ring.last().copied().unwrap_or(Point::ORIGIN);
        let shift = shift != self.removing;
        if !shift {
            self.selection_lassos.push(ring);
        }
        self.add_to_selection(doc, ids, shift);
        self.end_single_selection(doc, at, tolerance);
    }

    /// One object picked at a selection prompt, or a one-object prompt,
    /// as clicking it alone would (the Selection list of overlapping
    /// objects).
    pub fn select_object(&mut self, id: EntityId, at: Point, shift: bool, doc: &mut Document, tolerance: f64) {
        if self.is_picking() {
            self.chosen = Some(id);
            self.feed(Input::Pick { at, shift }, doc, at, tolerance);
            self.chosen = None;
            return;
        }
        if !self.is_selecting() {
            return;
        }
        self.window_start = None;
        self.add_to_selection(doc, vec![id], shift != self.removing);
        self.end_single_selection(doc, at, tolerance);
    }

    /// At a one-object prompt, the first object found ends the selection.
    fn end_single_selection(&mut self, doc: &mut Document, cursor: Point, tolerance: f64) {
        let single = self.active.as_ref().is_some_and(|a| a.cmd.single_selection());
        if single && !self.selection.is_empty() {
            let selection = self.take_selection();
            self.feed(Input::Selection(selection), doc, cursor, tolerance);
        }
    }

    /// Shift+pick removes from the selection, as in AutoCAD.
    fn add_to_selection(&mut self, doc: &Document, ids: Vec<EntityId>, remove: bool) {
        let count = ids.len();
        // Group selection (PICKSTYLE 1): a group's objects come together.
        let ids = if self.vars.group_select {
            let mut all = ids.clone();
            for g in doc.groups().into_iter().filter(|g| g.members.iter().any(|m| ids.contains(m))) {
                all.extend(g.members.into_iter().filter(|m| !ids.contains(m)));
            }
            all
        } else {
            ids
        };
        if !remove {
            self.added.push(ids.clone());
        }
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

    /// Drops a selection window in progress (e.g. when a menu opens).
    pub fn cancel_window(&mut self) {
        self.window_start = None;
    }

    /// Escape: ends the active command (keeping what it drew, as AutoCAD does),
    /// or drops the selection window, or clears the selection.
    pub fn cancel(&mut self, doc: &mut Document) {
        if self.window_start.take().is_some() || self.gathering.take().is_some() {
            return;
        }
        self.repeat = None;
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
        (self.gathering, self.removing) = (None, false);
        self.added.clear();
        self.selection.clear();
        self.selection_windows.clear();
        self.selection_lassos.clear();
    }

    /// The objects last selected for a command ("Previous").
    pub fn previous_selection(&self) -> &[EntityId] {
        &self.previous
    }

    fn take_selection(&mut self) -> Selection {
        (self.gathering, self.removing) = (None, false);
        self.added.clear();
        self.previous = self.selection.clone();
        Selection {
            ids: std::mem::take(&mut self.selection),
            windows: std::mem::take(&mut self.selection_windows),
            lassos: std::mem::take(&mut self.selection_lassos),
        }
    }

    /// Starts a command by name, as if typed. Returns false if unknown.
    pub fn run(&mut self, name: &str, doc: &mut Document) -> bool {
        oxidraft_model::units::set_current(doc.units());
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
        self.started.push(info.name);
        let Some(make) = info.make else {
            self.run_immediate(info.name, doc);
            return;
        };
        self.launch(info, make(), doc, cursor, pick_tolerance);
    }

    /// Runs a command that has been made.
    fn launch(
        &mut self,
        info: &'static CommandInfo,
        mut cmd: Box<dyn Command>,
        doc: &mut Document,
        cursor: Point,
        pick_tolerance: f64,
    ) {
        doc.begin(info.name);
        let wants_selection = cmd.wants_selection();
        if let Some(intro) = cmd.start(&self.vars) {
            self.log(intro);
        }
        if let Some(line) = cmd.prepare(doc) {
            self.log(line);
        }
        if cmd.is_finished() {
            doc.commit();
            return;
        }
        if !wants_selection {
            cmd.preselected(&self.selection);
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
            // Real-time pan: the app takes over the mouse until Esc or Enter.
            "PAN" => {
                self.view_requests.push(ViewRequest::RealtimePan);
                "Press Esc or Enter to exit, or right-click to exit.".into()
            }
            "REDO" => doc.redo().unwrap_or_else(|| "Nothing to redo.".into()),
            "HATCHTOBACK" => {
                doc.begin(name);
                let msg = crate::draworder::hatch_to_back(doc);
                doc.commit();
                msg
            }
            "LAYON" | "LAYTHW" | "LAYUNISO" => {
                doc.begin(name);
                let msg = match name {
                    "LAYUNISO" => crate::layer::unisolate(doc, &mut self.vars),
                    _ => crate::layer::all_layers(doc, name == "LAYTHW"),
                };
                doc.commit();
                msg
            }
            "OOPS" => {
                doc.begin(name);
                let msg = crate::measuregeom::oops(doc, &mut self.vars);
                doc.commit();
                msg
            }
            "LAYERP" => {
                doc.begin(name);
                let restored = doc.layer_previous();
                doc.commit();
                if restored { "Restored previous layer states.".into() } else { "*No previous layer states*".into() }
            }
            "REGEN" => "Regenerating model.".into(),
            "REGENALL" => "Regenerating model - caching viewports.".into(),
            "REDRAW" | "REDRAWALL" => String::new(),
            "DBLIST" => {
                let lines = crate::change::DbList::lines(doc);
                if lines.len() > 40 {
                    self.ui_requests.push(UiRequest::TextWindow);
                }
                lines.join("\n")
            }
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
        // MULTIPLE: the command named starts, and starts again each time it ends.
        if std::ptr::eq(active.info, find("MULTIPLE").unwrap_or(active.info))
            && let Input::Text(name) = &input
        {
            let Some(info) = find(name.trim()).filter(|i| i.make.is_some() && i.name != "MULTIPLE") else {
                self.log("Unknown command.");
                return;
            };
            self.active = None;
            doc.commit();
            self.repeat = Some(info);
            self.log(format!("Command: {}", info.name));
            self.start(info, doc, cursor, pick_tolerance);
            return;
        }
        // LIMCHECK: no points outside the limits.
        if let Input::Point(p) = input
            && doc.outside_limits(p)
        {
            self.log("**Outside limits");
            return;
        }
        if let Input::Point(p) = input {
            self.last_point = Some(p);
        }
        let mut ctx = Ctx {
            doc,
            vars: &mut self.vars,
            cursor,
            pick_tolerance,
            messages: Vec::new(),
            view: None,
            select: None,
            ui: None,
            copied: None,
            chosen: self.chosen.take(),
        };
        let result = active.cmd.input(input, &mut ctx);
        let messages = std::mem::take(&mut ctx.messages);
        self.view_requests.extend(ctx.view);
        self.ui_requests.extend(ctx.ui);
        if let Some(clip) = ctx.copied.take() {
            self.copied = Some(clip);
        }
        let select = ctx.select.take();
        if let Some(b) = active.cmd.base_point() {
            self.last_point = Some(b);
        }
        self.history.extend(messages);
        match result {
            Ok(Step::Continue) => {}
            Ok(Step::Done) => {
                let info = self.active.take().map(|a| a.info);
                doc.commit();
                if let Some(ids) = select {
                    self.set_selection(ids);
                }
                if let (Some(again), Some(info)) = (self.repeat, info)
                    && std::ptr::eq(again, info)
                {
                    self.log(format!("Command: {}", info.name));
                    self.start(info, doc, cursor, pick_tolerance);
                }
            }
            Err(e) => self.log(e),
        }
    }

    /// Runs a command the app made, under `info_name`'s entry (`label` is
    /// what the history shows).
    pub fn run_with(&mut self, info_name: &str, label: &str, cmd: Box<dyn Command>, doc: &mut Document) -> bool {
        let Some(info) = find(info_name) else { return false };
        if self.active.is_some() {
            self.cancel(doc);
        }
        self.window_start = None;
        if !label.is_empty() {
            self.log(format!("Command: {label}"));
        }
        self.launch(info, cmd, doc, Point::ORIGIN, 0.0);
        true
    }
}

/// MULTIPLE: asks for the command to repeat (the engine starts it).
struct Multiple;

impl Command for Multiple {
    fn prompt(&self) -> Prompt {
        Prompt::new("Enter command name to repeat")
    }

    fn input(&mut self, input: Input, _ctx: &mut Ctx) -> Result<Step, String> {
        match input {
            Input::Enter => Ok(Step::Done),
            _ => Err("Enter a command name.".into()),
        }
    }

    fn wants_text(&self) -> bool {
        true
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
    fn inquiry_commands_print_measurements_and_leave_no_undo_step() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["di", "0,0", "30,40"]);
        assert!(e.history().iter().any(|l| l.starts_with("Distance = 50.0000,  Angle in XY Plane = 53,")));
        type_all(&mut e, &mut d, &["id", "3,4"]);
        assert!(e.history().iter().any(|l| l == "X = 3.0000     Y = 4.0000     Z = 0.0000"));
        type_all(&mut e, &mut d, &["aa", "0,0", "10,0", "10,5", "0,5", ""]);
        assert!(e.history().iter().any(|l| l == "Area = 50.0000, Perimeter = 30.0000"));

        d.add(Geometry::Circle { center: Point::ORIGIN, radius: 1.0 });
        type_all(&mut e, &mut d, &["area", "o"]);
        click(&mut e, &mut d, 1.0, 0.0);
        assert!(e.history().iter().any(|l| l == "Area = 3.1416, Circumference = 6.2832"));
        assert!(!d.can_undo() || d.undo_name() != Some("AREA"));

        // In feet and inches, with bearings; and typed that way too.
        d.set_units(oxidraft_model::Units {
            length: oxidraft_model::LengthType::Architectural,
            angle: oxidraft_model::AngleType::Surveyor,
            ..Default::default()
        });
        type_all(&mut e, &mut d, &["di", "0,0", "@15'6-1/2\"<N45dE"]);
        let last = e.history().iter().rev().find(|l| l.starts_with("Distance")).unwrap().clone();
        assert_eq!(last, "Distance = 15'-6 1/2\",  Angle in XY Plane = N 45d E,  Angle from XY Plane = 0d");
    }

    #[test]
    fn colortheme_asks_the_app_to_switch() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["colortheme"]);
        assert_eq!(e.prompt(), "Enter new value for COLORTHEME <0>:");
        type_all(&mut e, &mut d, &["2"]);
        assert!(e.is_active() && e.history().iter().any(|l| l == "Requires an integer between 0 and 1."));
        type_all(&mut e, &mut d, &["1"]);
        assert_eq!(e.take_ui_requests(), vec![UiRequest::ColorTheme(1)]);
        type_all(&mut e, &mut d, &["colortheme"]);
        assert_eq!(e.prompt(), "Enter new value for COLORTHEME <1>:");
    }

    #[test]
    fn ltscale_sets_the_drawing_scale() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["lts"]);
        assert_eq!(e.prompt(), "Enter new linetype scale factor <1.0000>:");
        type_all(&mut e, &mut d, &["0.5"]);
        assert_eq!(d.ltscale(), 0.5);
        type_all(&mut e, &mut d, &["lts", "-1"]);
        assert!(e.is_active(), "rejects a negative scale");
    }

    #[test]
    fn select_similar_leaves_the_matches_selected() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "0,5", "10,5", "", "c", "50,50", "2"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["selectsimilar"]);
        assert!(!e.is_active());
        assert_eq!(e.selection().len(), 2, "both lines, not the circle");
        assert!(e.history().iter().any(|l| l == "2 found"));

        // Settings: also match color; a red line no longer matches the others.
        let red = d.entities().next().unwrap().id;
        d.update(red, |x| x.color = oxidraft_model::Color::Index(1));
        e.cancel(&mut d);
        type_all(&mut e, &mut d, &["selectsimilar", "se", "c", "x"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(e.selection(), &[red]);
    }

    fn point_objects(d: &Document) -> Vec<Point> {
        d.entities()
            .filter_map(|x| match x.geometry {
                Geometry::Point { position } => Some(position),
                _ => None,
            })
            .collect()
    }

    fn has_point(d: &Document, x: f64, y: f64) -> bool {
        point_objects(d).iter().any(|p| p.distance(Point::new(x, y)) < 1e-9)
    }

    #[test]
    fn points_divide_and_measure() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["po", "3,4"]);
        assert!(e.history().iter().any(|l| l == "Current point modes:  PDMODE=0  PDSIZE=0.0000"));
        assert!(has_point(&d, 3.0, 4.0));
        type_all(&mut e, &mut d, &["pdmode", "5"]);
        assert!(e.is_active(), "5 isn't a point style");
        type_all(&mut e, &mut d, &["35", "pdsize", "-2"]);
        assert_eq!((d.pdmode(), d.pdsize()), (35, -2.0));

        // DIVIDE a line into 5: 4 points; a circle into 4: one at 0° too.
        type_all(&mut e, &mut d, &["l", "0,10", "10,10", "", "div"]);
        click(&mut e, &mut d, 2.0, 10.0);
        type_all(&mut e, &mut d, &["5"]);
        assert!([2.0, 4.0, 6.0, 8.0].iter().all(|&x| has_point(&d, x, 10.0)));
        assert_eq!(point_objects(&d).len(), 5);
        type_all(&mut e, &mut d, &["c", "50,50", "10", "div"]);
        click(&mut e, &mut d, 40.0, 50.0);
        type_all(&mut e, &mut d, &["4"]);
        assert!(has_point(&d, 60.0, 50.0) && has_point(&d, 50.0, 60.0) && has_point(&d, 50.0, 40.0));

        // MEASURE from the end nearest the pick.
        type_all(&mut e, &mut d, &["l", "0,20", "10,20", "", "me"]);
        click(&mut e, &mut d, 9.0, 20.0);
        type_all(&mut e, &mut d, &["3"]);
        assert!([7.0, 4.0, 1.0].iter().all(|&x| has_point(&d, x, 20.0)));
        assert!(!has_point(&d, 3.0, 20.0));
    }

    fn polyline(d: &Document) -> (Vec<Point>, bool) {
        let mut lines = d.entities().filter_map(|x| match &x.geometry {
            Geometry::Polyline { vertices, closed } => Some((vertices.iter().map(|v| v.point).collect(), *closed)),
            _ => None,
        });
        lines.next().expect("a polyline")
    }

    #[test]
    fn pedit_turns_lines_into_a_closed_polyline_and_reverses_it() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "10,0", "10,10", "", "l", "10,10", "0,10", ""]);
        type_all(&mut e, &mut d, &["pe"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["", "j"]);
        click(&mut e, &mut d, 10.0, 5.0);
        click(&mut e, &mut d, 5.0, 10.0);
        type_all(&mut e, &mut d, &["", "c", ""]);
        assert!(!e.is_active());
        assert!(e.history().iter().any(|l| l == "2 segments added to polyline"));
        assert_eq!(d.entities().count(), 1);
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].map(|(x, y)| Point::new(x, y));
        assert_eq!(polyline(&d), (square.to_vec(), true));

        type_all(&mut e, &mut d, &["pe"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["r", "o", ""]);
        let (pts, closed) = polyline(&d);
        assert!(!closed);
        // Same start, the other way round.
        assert_eq!(pts, [square[0], square[3], square[2], square[1]]);
        d.undo();
        assert_eq!(polyline(&d), (square.to_vec(), true), "one undo step per PEDIT");
    }

    #[test]
    fn pedit_multiple_joins_across_small_gaps() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["pl", "0,0", "10,0", "", "pl", "10.05,0", "20,0", "", "pe", "m"]);
        click(&mut e, &mut d, 5.0, 0.0);
        click(&mut e, &mut d, 15.0, 0.0);
        type_all(&mut e, &mut d, &["", "j", "0.1", ""]);
        assert_eq!(d.entities().count(), 1);
        let (pts, closed) = polyline(&d);
        assert_eq!(pts.len(), 4, "the gap is bridged: {pts:?}");
        assert!(!closed);
    }

    /// A survey point block with a visible number and an invisible elevation.
    fn survey_point(d: &mut Document) -> EntityId {
        use oxidraft_model::{Attach, Attribute, Block, BlockEntity, Color, Placement};
        let circle = Geometry::Circle { center: Point::ORIGIN, radius: 0.5 };
        d.define_block(Block {
            name: "KW".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByBlock, circle)],
        });
        let attr = |tag: &str, value: &str, invisible| Attribute {
            tag: tag.into(),
            value: value.into(),
            layer: "0".into(),
            color: Color::ByBlock,
            position: Point::new(21.0, 0.0),
            height: 1.0,
            rotation: 0.0,
            attach: Attach::BottomLeft,
            invisible,
        };
        d.add(Geometry::Insert {
            block: "KW".into(),
            placement: Placement::at(Point::new(20.0, 0.0)),
            attributes: vec![attr("KWNOM", "9", false), attr("ELEV", "101.5", true)],
            clip: None,
        })
    }

    #[test]
    fn list_eattedit_and_attdisp() {
        let (mut e, mut d) = (engine(), Document::new());
        let block = survey_point(&mut d);
        type_all(&mut e, &mut d, &["l", "0,0", "3,4", ""]);
        click(&mut e, &mut d, 1.5, 2.0);
        click(&mut e, &mut d, 20.0, 0.5);
        type_all(&mut e, &mut d, &["list"]);
        let history = e.history().join("\n");
        assert!(history.contains("LINE  Layer: \"0\""), "{history}");
        assert!(history.contains("Length =  5.0000"), "{history}");
        assert!(history.contains("Block Name:  \"KW\""), "{history}");
        assert!(history.contains("Tag:  ELEV") && history.contains("Invisible"), "{history}");
        assert_eq!(e.take_ui_requests(), vec![UiRequest::TextWindow]);

        // EATTEDIT asks the app for its editor; a line isn't a block.
        type_all(&mut e, &mut d, &["eattedit"]);
        click(&mut e, &mut d, 1.5, 2.0);
        assert!(e.is_active(), "still asking for a block");
        click(&mut e, &mut d, 20.0, 0.5);
        assert!(!e.is_active());
        assert_eq!(e.take_ui_requests(), vec![UiRequest::EditAttributes(block)]);

        let shown = |d: &Document| d.expanded_styled(d.entity(block).unwrap()).len();
        assert_eq!(shown(&d), 2, "circle and the visible number");
        type_all(&mut e, &mut d, &["attdisp", "on"]);
        assert_eq!(shown(&d), 3);
        type_all(&mut e, &mut d, &["attdisp", "off"]);
        assert_eq!(shown(&d), 1);
    }

    #[test]
    fn layer_isolate_off_freeze_lock_and_back() {
        use oxidraft_model::{Color, Layer};
        let (mut e, mut d) = (engine(), Document::new());
        for (name, color) in [("WALLS", 1), ("DOORS", 2), ("TEXT", 3)] {
            d.add_layer(Layer::new(name, color));
        }
        let line = |y: f64| Geometry::Line { start: Point::new(0.0, y), end: Point::new(10.0, y) };
        d.add_on_layer(line(0.0), "WALLS", Color::ByLayer);
        d.add_on_layer(line(5.0), "DOORS", Color::ByLayer);
        d.add_on_layer(line(10.0), "TEXT", Color::ByLayer);
        let layer = |d: &Document, n: &str| d.layer(n).unwrap().clone();

        // LAYISO locks the others by default and makes the isolated layer current.
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["layiso"]);
        assert!(e.history().iter().any(|l| l == "Layer \"WALLS\" has been isolated."));
        assert!(layer(&d, "DOORS").locked && layer(&d, "TEXT").locked && !layer(&d, "WALLS").locked);
        assert_eq!(d.current_layer(), "WALLS");
        type_all(&mut e, &mut d, &["layuniso"]);
        assert!(!layer(&d, "DOORS").locked && !layer(&d, "TEXT").locked);
        assert_eq!(d.current_layer(), "0");

        // LAYOFF repeats until Enter; Undo turns the last one back on.
        type_all(&mut e, &mut d, &["layoff"]);
        click(&mut e, &mut d, 5.0, 5.0);
        click(&mut e, &mut d, 5.0, 10.0);
        type_all(&mut e, &mut d, &["u", ""]);
        assert!(!layer(&d, "DOORS").on && layer(&d, "TEXT").on);
        type_all(&mut e, &mut d, &["layon"]);
        assert!(layer(&d, "DOORS").on);

        // The current layer can't be frozen.
        d.set_current_layer("WALLS");
        type_all(&mut e, &mut d, &["layfrz"]);
        click(&mut e, &mut d, 5.0, 0.0);
        assert!(!layer(&d, "WALLS").frozen);
        click(&mut e, &mut d, 5.0, 10.0);
        type_all(&mut e, &mut d, &[""]);
        assert!(layer(&d, "TEXT").frozen);
        type_all(&mut e, &mut d, &["laythw"]);
        assert!(!layer(&d, "TEXT").frozen);

        // LAYLCK, then LAYULK picks an object on the locked layer.
        type_all(&mut e, &mut d, &["laylck"]);
        click(&mut e, &mut d, 5.0, 5.0);
        assert!(layer(&d, "DOORS").locked && !e.is_active());
        type_all(&mut e, &mut d, &["layulk"]);
        click(&mut e, &mut d, 5.0, 5.0);
        assert!(!layer(&d, "DOORS").locked);
    }

    #[test]
    fn copy_cut_and_paste_with_layers() {
        use oxidraft_model::{Color, Layer};
        let (mut e, mut d) = (engine(), Document::new());
        d.add_layer(Layer::new("WALLS", 1));
        d.add_on_layer(
            Geometry::Line { start: Point::new(10.0, 10.0), end: Point::new(20.0, 10.0) },
            "WALLS",
            Color::ByLayer,
        );
        let line = |d: &Document| match d.entities().last().map(|x| (x.geometry.clone(), x.layer.clone())) {
            Some((Geometry::Line { start, end }, layer)) => (start, end, layer),
            other => panic!("{other:?}"),
        };

        // COPYCLIP: the base point is the lower-left corner.
        click(&mut e, &mut d, 15.0, 10.0);
        type_all(&mut e, &mut d, &["copyclip"]);
        let clip = e.take_copied().expect("copied");
        assert_eq!(clip.insbase(), Point::new(10.0, 10.0));
        assert_eq!(d.entities().count(), 1);

        // Pasting into another drawing brings the layer along.
        let mut other = Document::new();
        e.paste(clip.clone(), false, &mut other);
        assert_eq!(e.current_prompt().unwrap().text, "Specify insertion point");
        type_all(&mut e, &mut other, &["100,100"]);
        assert_eq!(line(&other), (Point::new(100.0, 100.0), Point::new(110.0, 100.0), "WALLS".to_string()));
        assert_eq!(other.layer("WALLS").unwrap().color, 1);
        e.paste(clip, true, &mut other);
        assert_eq!(line(&other).0, Point::new(10.0, 10.0), "PASTEORIG keeps the coordinates");

        // CUTCLIP removes the objects; COPYBASE takes its base point first.
        click(&mut e, &mut d, 15.0, 10.0);
        type_all(&mut e, &mut d, &["cutclip"]);
        assert_eq!(d.entities().count(), 0);
        assert!(e.take_copied().is_some());
        d.undo();
        click(&mut e, &mut d, 15.0, 10.0);
        type_all(&mut e, &mut d, &["copybase", "20,10"]);
        assert!(!e.is_active());
        assert_eq!(e.take_copied().unwrap().insbase(), Point::new(20.0, 10.0));
    }

    #[test]
    fn laymcur_makes_the_picked_objects_layer_current() {
        let (mut e, mut d) = (engine(), Document::new());
        d.add_layer(oxidraft_model::Layer::new("WALLS", 1));
        d.add_layer(oxidraft_model::Layer::new("DOORS", 2));
        let line = |y: f64| Geometry::Line { start: Point::new(0.0, y), end: Point::new(10.0, y) };
        d.add_on_layer(line(0.0), "WALLS", oxidraft_model::Color::ByLayer);
        d.add_on_layer(line(5.0), "DOORS", oxidraft_model::Color::ByLayer);

        // Selected first: used right away.
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["laymcur"]);
        assert!(!e.is_active());
        assert_eq!(d.current_layer(), "WALLS");
        assert!(e.history().iter().any(|l| l == "WALLS is now the current layer."));

        // Picked at the prompt: one click is enough.
        type_all(&mut e, &mut d, &["laymcur"]);
        click(&mut e, &mut d, 5.0, 5.0);
        assert!(!e.is_active());
        assert_eq!(d.current_layer(), "DOORS");
        d.undo();
        assert_eq!(d.current_layer(), "WALLS");
    }

    #[test]
    fn zoom_asks_the_app_for_view_changes() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["z"]);
        click(&mut e, &mut d, 0.0, 0.0);
        click(&mut e, &mut d, 10.0, 5.0);
        let window = ViewRequest::Window(Aabb::from_points(Point::ORIGIN, Point::new(10.0, 5.0)));
        assert_eq!(e.take_view_requests(), vec![window]);
        type_all(&mut e, &mut d, &["z", "2x", "zoom", "e", "z", "p", "z", "s", ".5", "z", "c", "3,4", "20"]);
        assert_eq!(
            e.take_view_requests(),
            vec![
                ViewRequest::Magnify(2.0),
                ViewRequest::Extents,
                ViewRequest::Previous,
                ViewRequest::OfExtents(0.5),
                ViewRequest::Center { center: Point::new(3.0, 4.0), height: 20.0 },
            ]
        );
        assert!(e.take_view_requests().is_empty());
        type_all(&mut e, &mut d, &["z", "", "p"]);
        assert_eq!(e.take_view_requests(), vec![ViewRequest::RealtimeZoom, ViewRequest::RealtimePan]);

        // Object: the bounds of what's selected.
        d.add(Geometry::Circle { center: Point::new(50.0, 50.0), radius: 2.0 });
        type_all(&mut e, &mut d, &["z", "o"]);
        click(&mut e, &mut d, 52.0, 50.0);
        type_all(&mut e, &mut d, &[""]);
        let b = Aabb::from_points(Point::new(48.0, 48.0), Point::new(52.0, 52.0));
        assert_eq!(e.take_view_requests(), vec![ViewRequest::Window(b)]);
        assert!(e.selection().is_empty());

        // Objects selected first: Object zooms to them at once, and they stay selected.
        click(&mut e, &mut d, 52.0, 50.0);
        let circle = e.selection().to_vec();
        type_all(&mut e, &mut d, &["z"]);
        assert_eq!(e.highlighted(), circle, "still shown selected");
        type_all(&mut e, &mut d, &["o"]);
        assert!(!e.is_active());
        assert_eq!(e.take_view_requests(), vec![ViewRequest::Window(b)]);
        assert_eq!(e.selection(), circle.as_slice());
        // Any other zoom keeps them selected too.
        type_all(&mut e, &mut d, &["z", "e"]);
        assert_eq!(e.selection(), circle.as_slice());
        e.take_view_requests();
        assert!(!d.can_undo() || d.undo_name() != Some("ZOOM"), "zooming doesn't change the drawing");
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
    fn array_rectangular_and_polar() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["c", "0,0", "1"]);
        type_all(&mut e, &mut d, &["ar"]);
        click(&mut e, &mut d, 1.0, 0.0);
        type_all(&mut e, &mut d, &["", "r", "2", "3", "10", "20"]);
        assert!(!e.is_active());
        assert_eq!(d.len(), 6);
        assert!(d.entities().any(|x| x.geometry == Geometry::Circle { center: Point::new(40.0, 10.0), radius: 1.0 }));
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(d.len(), 1, "one undo step");

        // Four items around (0,10): the circle at (0,0) and three copies a quarter turn apart.
        type_all(&mut e, &mut d, &["arraypolar"]);
        click(&mut e, &mut d, 1.0, 0.0);
        type_all(&mut e, &mut d, &["", "0,10", "4", "", ""]);
        let mut centers: Vec<Point> = d
            .entities()
            .filter_map(|x| if let Geometry::Circle { center, .. } = x.geometry { Some(center) } else { None })
            .collect();
        centers.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        let expected = [(-10.0, 10.0), (0.0, 0.0), (0.0, 20.0), (10.0, 10.0)];
        assert_eq!(centers.len(), 4);
        for (c, (x, y)) in centers.iter().zip(expected) {
            assert!(c.approx_eq(Point::new(x, y)), "{centers:?}");
        }
    }

    #[test]
    fn chamfer_break_join_and_lengthen() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["l", "12,2", "12,10", ""]);
        type_all(&mut e, &mut d, &["cha", "d", "2", ""]);
        click(&mut e, &mut d, 5.0, 0.0);
        click(&mut e, &mut d, 12.0, 6.0);
        assert!(lines(&d).contains(&(Point::new(10.0, 0.0), Point::new(12.0, 2.0))), "{:?}", lines(&d));
        assert_eq!((e.vars.chamfer_a, e.vars.chamfer_b), (2.0, 2.0));

        // JOIN the three into one polyline, then BREAK a gap in it.
        type_all(&mut e, &mut d, &["j"]);
        for (x, y) in [(5.0, 0.0), (11.0, 1.0), (12.0, 6.0)] {
            click(&mut e, &mut d, x, y);
        }
        type_all(&mut e, &mut d, &[""]);
        assert!(e.history().iter().any(|l| l == "3 objects converted to 1 polyline"));
        assert_eq!(d.len(), 1);
        type_all(&mut e, &mut d, &["br"]);
        click(&mut e, &mut d, 4.0, 0.0);
        type_all(&mut e, &mut d, &["6,0"]);
        assert_eq!(d.len(), 2);
        assert!(lines(&d).is_empty(), "both pieces stay polylines");

        // LENGTHEN a line by a delta at the picked end.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["len", "de", "5"]);
        click(&mut e, &mut d, 9.0, 0.0);
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(lines(&d), vec![(Point::new(0.0, 0.0), Point::new(15.0, 0.0))]);
    }

    #[test]
    fn radial_angular_and_chained_dimensions() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["c", "0,0", "5"]);
        type_all(&mut e, &mut d, &["dra"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["10,10"]);
        assert!(e.history().iter().any(|l| l == "Dimension text = 5.00"));
        let radial = d.entities().last().unwrap().geometry.clone();
        let Geometry::RadialDimension { point, diameter: false, .. } = radial else { panic!("{radial:?}") };
        assert!(point.approx_eq(Point::new(5.0, 5.0) * (1.0 / 2f64.sqrt())));

        // Two lines meeting at the origin at 90°; the arc in the first quadrant measures it.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["l", "-10,0", "10,0", ""]);
        type_all(&mut e, &mut d, &["l", "0,-10", "0,10", ""]);
        type_all(&mut e, &mut d, &["dan"]);
        click(&mut e, &mut d, 6.0, 0.0);
        click(&mut e, &mut d, 0.0, 6.0);
        type_all(&mut e, &mut d, &["-4,4"]);
        let Some(Geometry::AngularDimension { vertex, p1, p2, .. }) = d.entities().last().map(|x| x.geometry.clone())
        else {
            panic!()
        };
        assert!(vertex.approx_eq(Point::ORIGIN));
        assert!(p1.approx_eq(Point::new(0.0, 10.0)) && p2.approx_eq(Point::new(-10.0, 0.0)), "{p1:?} {p2:?}");
        assert!(e.history().iter().any(|l| l == "Dimension text = 90°"));

        // A linear dimension, two continued from it, then one from its baseline.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["dli", "0,0", "10,0", "5,5", "dco", "25,0", "40,0", ""]);
        type_all(&mut e, &mut d, &["dba", "50,0", ""]);
        let dims: Vec<(Point, Point, Point)> = d
            .entities()
            .filter_map(|x| match x.geometry {
                Geometry::Dimension { p1, p2, line, .. } => Some((p1, p2, line)),
                _ => None,
            })
            .collect();
        assert_eq!(dims.len(), 4);
        assert_eq!(
            (dims[1].0, dims[1].1, dims[1].2),
            (Point::new(10.0, 0.0), Point::new(25.0, 0.0), Point::new(5.0, 5.0))
        );
        assert_eq!((dims[2].0, dims[2].1), (Point::new(25.0, 0.0), Point::new(40.0, 0.0)));
        // The baseline builds on the last (25→40), from its first extension line, one DIMDLI up.
        assert_eq!(
            (dims[3].0, dims[3].1, dims[3].2),
            (Point::new(25.0, 0.0), Point::new(50.0, 0.0), Point::new(5.0, 8.75))
        );
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

    fn ring(points: &[(f64, f64)]) -> Vec<Point> {
        points.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    #[test]
    fn a_lasso_selects_and_stretch_moves_the_points_inside_it() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "10,10", ""]);
        // A crossing lasso round the shared corner, at STRETCH's prompt.
        type_all(&mut e, &mut d, &["s"]);
        e.select_lasso(ring(&[(8.0, -2.0), (12.0, -2.0), (12.0, 2.0), (8.0, 2.0)]), true, false, &mut d, 0.5);
        assert_eq!(e.selection().len(), 2);
        type_all(&mut e, &mut d, &["", "0,0", "5,0"]);
        assert_eq!(
            lines(&d),
            vec![(Point::new(0.0, 0.0), Point::new(15.0, 0.0)), (Point::new(15.0, 0.0), Point::new(10.0, 10.0))]
        );
        // A window lasso takes only what's wholly inside; Shift takes it out again.
        e.select_lasso(ring(&[(-1.0, -1.0), (16.0, -1.0), (16.0, 1.0), (-1.0, 1.0)]), false, false, &mut d, 0.5);
        assert_eq!(e.selection().len(), 1);
        e.select_lasso(ring(&[(-1.0, -1.0), (16.0, -1.0), (16.0, 1.0), (-1.0, 1.0)]), false, true, &mut d, 0.5);
        assert!(e.selection().is_empty());
    }

    #[test]
    fn user_aliases_run_their_commands() {
        let (mut e, mut d) = (engine(), Document::new());
        e.aliases = vec![("QQ".into(), "CIRCLE".into())];
        type_all(&mut e, &mut d, &["qq", "0,0", "5"]);
        assert!(matches!(only(&d), Geometry::Circle { radius, .. } if radius == 5.0));
    }

    #[test]
    fn select_objects_takes_autocads_typed_options() {
        let (mut e, mut d) = (engine(), Document::new());
        // Three vertical lines at x = 0, 10 and 20, then a circle.
        type_all(&mut e, &mut d, &["l", "0,0", "0,10", "", "l", "10,0", "10,10", "", "l", "20,0", "20,10", ""]);
        type_all(&mut e, &mut d, &["c", "50,50", "1"]);
        let count = |e: &Engine| e.selection().len();
        type_all(&mut e, &mut d, &["move"]);
        // Window by typed corners: the first line only.
        type_all(&mut e, &mut d, &["w", "-1,-1", "1,11"]);
        assert_eq!(count(&e), 1);
        assert_eq!(e.prompt(), "Select objects:");
        // Crossing: touching is enough.
        type_all(&mut e, &mut d, &["c", "9,5", "21,6"]);
        assert_eq!(count(&e), 3);
        // Undo takes the last pick's objects out again; Remove mode takes picks out.
        type_all(&mut e, &mut d, &["u"]);
        assert_eq!(count(&e), 1);
        type_all(&mut e, &mut d, &["r"]);
        assert_eq!(e.prompt(), "Remove objects:");
        type_all(&mut e, &mut d, &["0,5"]);
        assert_eq!(count(&e), 0);
        type_all(&mut e, &mut d, &["a", "f", "-5,5", "15,5", ""]);
        assert_eq!(count(&e), 2, "the fence crossed two lines");
        type_all(&mut e, &mut d, &["l"]);
        assert_eq!(count(&e), 3, "Last: the circle");
        type_all(&mut e, &mut d, &["", "0,0", "0,0"]);
        assert!(!e.is_active());

        // Previous: what MOVE got; ALL; a CPolygon round the middle line's top.
        type_all(&mut e, &mut d, &["copy", "p"]);
        assert_eq!(count(&e), 3);
        e.cancel(&mut d);
        e.cancel(&mut d);
        type_all(&mut e, &mut d, &["erase", "all"]);
        assert_eq!(count(&e), 4);
        e.cancel(&mut d);
        e.cancel(&mut d);
        type_all(&mut e, &mut d, &["erase", "cp", "8,8", "12,8", "12,12", "8,12", ""]);
        assert_eq!(count(&e), 1);
        type_all(&mut e, &mut d, &["wp", "8,-1", "12,-1", "12,11", "8,11", ""]);
        assert_eq!(count(&e), 1, "the same line, already in");
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(d.len(), 3);
    }

    #[test]
    fn an_object_picked_from_the_selection_list_joins_the_selection() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "0,0", "10,0", ""]);
        let under = d.entities().map(|x| x.id).min().unwrap();
        type_all(&mut e, &mut d, &["e"]);
        e.select_object(under, Point::new(5.0, 0.0), false, &mut d, 0.5);
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(d.len(), 1);
        assert!(d.entity(under).is_none(), "the one chosen went");
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
    fn ellipse_by_axis_center_rotation_and_arc() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["el", "0,0", "10,0", "2"]);
        let Geometry::Ellipse(el) = only(&d) else { panic!() };
        assert!(el.center.approx_eq(Point::new(5.0, 0.0)) && (el.a() - 5.0).abs() < 1e-9);
        assert!((el.ratio - 0.4).abs() < 1e-9 && el.is_full());
        // From the center; the second axis is longer, so it's the major one.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["el", "c", "0,0", "2,0", "0,6"]);
        let Geometry::Ellipse(el) = only(&d) else { panic!() };
        assert!(el.major.approx_eq(Point::new(0.0, 6.0)) && (el.ratio - 1.0 / 3.0).abs() < 1e-9);
        // A circle turned 60° round its axis looks half as tall.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["el", "0,0", "8,0", "r", "60"]);
        let Geometry::Ellipse(el) = only(&d) else { panic!() };
        assert!((el.ratio - 0.5).abs() < 1e-9);
        // An elliptical arc: the upper half.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["el", "a", "-4,0", "4,0", "2", "0", "180"]);
        let Geometry::Ellipse(el) = only(&d) else { panic!() };
        assert!(!el.is_full() && el.point_at(el.start + el.sweep() / 2.0).approx_eq(Point::new(0.0, 2.0)));
    }

    #[test]
    fn trimming_an_ellipse_and_a_construction_line() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["el", "c", "0,0", "10,0", "5", "xl", "0,0", "0,10", ""]);
        assert!(d.extents().is_some_and(|b| b.max.y < 6.0), "construction lines don't count in the extents");
        type_all(&mut e, &mut d, &["tr"]);
        click(&mut e, &mut d, -10.0, 0.0);
        click(&mut e, &mut d, 0.0, 20.0);
        type_all(&mut e, &mut d, &[""]);
        let mut ellipse = None;
        let mut ray = None;
        for x in d.entities() {
            match &x.geometry {
                Geometry::Ellipse(el) => ellipse = Some(*el),
                Geometry::Ray { base, direction } => ray = Some((*base, *direction)),
                g => panic!("unexpected {g:?}"),
            }
        }
        // The left half is gone; the ends sit where the line crossed.
        let el = ellipse.expect("an elliptical arc");
        assert!(!el.is_full() && el.covers(0.0) && !el.covers(180.0));
        assert!(el.start_point().distance(Point::new(0.0, -5.0)) < 1e-5);
        assert!(el.end_point().distance(Point::new(0.0, 5.0)) < 1e-5);
        // Trimming the line above the arc leaves a ray running down from it.
        let (base, direction) = ray.expect("a ray");
        assert!(base.distance(Point::new(0.0, 5.0)) < 1e-5 && direction.y < 0.0);
    }

    #[test]
    fn spline_by_fit_points_and_control_vertices() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["spl", "0,0", "10,5", "20,0", ""]);
        let Geometry::Spline(s) = only(&d) else { panic!() };
        assert_eq!(s.fit().len(), 3);
        assert!(s.start_point().approx_eq(Point::ORIGIN) && s.end_point().approx_eq(Point::new(20.0, 0.0)));
        // The CV method, closed; the method is remembered.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["spl", "m", "cv", "0,0", "10,0", "10,10", "0,10", "c"]);
        let Geometry::Spline(s) = only(&d) else { panic!() };
        assert!(s.is_closed() && s.fit().is_empty() && s.grip_points().len() == 4);
        type_all(&mut e, &mut d, &["spl", ""]);
        assert!(e.history().iter().any(|l| l.contains("Method=CV")));
    }

    #[test]
    fn xline_options_and_rays() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["xl", "h", "0,5", "0,8", ""]);
        type_all(&mut e, &mut d, &["xl", "b", "0,0", "10,0", "0,10", ""]);
        type_all(&mut e, &mut d, &["ray", "1,1", "2,1", "1,2", ""]);
        type_all(&mut e, &mut d, &["xl", "o", "2"]);
        click(&mut e, &mut d, 30.0, 5.0);
        type_all(&mut e, &mut d, &["30,6", ""]);
        let xlines: Vec<(Point, Point)> = d
            .entities()
            .filter_map(|x| match x.geometry {
                Geometry::Xline { base, direction } => Some((base, direction)),
                _ => None,
            })
            .collect();
        let rays = d.entities().filter(|x| matches!(x.geometry, Geometry::Ray { .. })).count();
        assert_eq!((xlines.len(), rays), (4, 2));
        assert!((Point::ORIGIN.angle_to(xlines[2].1) - 45.0).abs() < 1e-9, "bisects the angle");
        // Offset 2 from the line at y=5, on the side of y=6.
        let (base, direction) = xlines[3];
        assert!((base.y - 7.0).abs() < 1e-9 && direction.y.abs() < 1e-9);
        assert!(d.extents().is_none());
    }

    #[test]
    fn polygon_inscribed_circumscribed_and_by_edge() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["pol", "6", "0,0", "i", "10,0"]);
        let pts = points(&only(&d));
        assert!(pts.len() == 6 && pts[0].approx_eq(Point::new(10.0, 0.0)));
        // Circumscribed with a typed radius: the bottom edge is level, 5 below the center.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["pol", "4", "0,0", "c", "5"]);
        let pts = points(&only(&d));
        assert!(pts.iter().any(|p| p.approx_eq(Point::new(-5.0, -5.0))));
        assert!(pts.iter().any(|p| p.approx_eq(Point::new(5.0, -5.0))));
        // By an edge, on its left; the number of sides is remembered.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["pol", "", "e", "0,0", "4,0"]);
        let pts = points(&only(&d));
        assert!(pts.len() == 4 && pts.iter().any(|p| p.approx_eq(Point::new(4.0, 4.0))));
    }

    #[test]
    fn donuts_and_wide_polylines() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["do", "2", "4", "10,10", "20,10", ""]);
        assert_eq!(d.len(), 2);
        let donut = d.entities().next().unwrap().geometry.clone();
        let Geometry::Polyline { vertices, closed: true } = &donut else { panic!() };
        assert_eq!(oxidraft_model::constant_width(vertices), Some(1.0));
        // Anywhere on the ring picks it, the hole doesn't.
        assert!(donut.distance_to(Point::new(10.0, 11.9)) < 1e-9 && donut.distance_to(Point::new(10.0, 10.0)) > 0.9);

        // A tapered first segment, then the ending width carries on.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["pl", "0,0", "w", "0", "2", "10,0", "10,10", ""]);
        let Geometry::Polyline { vertices, .. } = only(&d) else { panic!() };
        assert_eq!((vertices[0].start_width, vertices[0].end_width), (0.0, 2.0));
        assert_eq!((vertices[1].start_width, vertices[1].end_width), (2.0, 2.0));
        type_all(&mut e, &mut d, &["pe"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &["w", "0.5", ""]);
        let Geometry::Polyline { vertices, .. } = only(&d) else { panic!() };
        assert_eq!(oxidraft_model::constant_width(&vertices), Some(0.5));
        // Moving and scaling keep the width in proportion.
        let Geometry::Polyline { vertices, .. } = only(&d).scaled(Point::ORIGIN, 2.0) else { panic!() };
        assert_eq!(oxidraft_model::constant_width(&vertices), Some(1.0));
    }

    #[test]
    fn revision_clouds_around_a_rectangle_and_from_an_object() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["revcloud", "a", "2", "0,0", "10,6"]);
        let cloud = only(&d);
        let Geometry::Polyline { vertices, closed: true } = &cloud else { panic!() };
        assert_eq!(vertices.len(), 16, "32 units round at 2 per arc");
        // Every arc bulges outward.
        for seg in cloud.segments() {
            let m = seg.point_at(0.5);
            assert!(!(m.x > 0.0 && m.x < 10.0 && m.y > 0.0 && m.y < 6.0), "{m:?} is inside");
        }
        // A circle becomes a cloud in its place.
        let mut d = Document::new();
        type_all(&mut e, &mut d, &["c", "0,0", "5", "revcloud", "o"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &[""]);
        assert!(matches!(only(&d), Geometry::Polyline { closed: true, .. }));
    }

    #[test]
    fn draworder_moves_objects_and_new_hatches_go_behind() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "", "l", "0,5", "10,5", "", "l", "0,9", "10,9", ""]);
        let order = |d: &Document| -> Vec<u64> { d.entities_in_draw_order().iter().map(|x| x.id.0).collect() };
        // The middle line to the front, then the top one under it.
        type_all(&mut e, &mut d, &["dr"]);
        click(&mut e, &mut d, 5.0, 5.0);
        type_all(&mut e, &mut d, &["", "f"]);
        assert_eq!(order(&d), vec![1, 3, 2]);
        type_all(&mut e, &mut d, &["dr"]);
        click(&mut e, &mut d, 5.0, 9.0);
        type_all(&mut e, &mut d, &["", "u"]);
        click(&mut e, &mut d, 5.0, 0.0);
        type_all(&mut e, &mut d, &[""]);
        assert_eq!(order(&d), vec![3, 1, 2]);
        // Preselected objects and a preset option, as the ribbon does it.
        click(&mut e, &mut d, 5.0, 9.0);
        e.draw_order("Front", &mut d);
        assert_eq!(order(&d), vec![1, 2, 3]);
        // A hatch goes behind the lines around it; HATCHTOBACK and
        // TEXTTOFRONT reorder whole kinds of objects.
        type_all(&mut e, &mut d, &["rec", "20,0", "30,10", "h"]);
        click(&mut e, &mut d, 25.0, 5.0);
        type_all(&mut e, &mut d, &[""]);
        let hatch = d.entities().find(|x| matches!(x.geometry, Geometry::Hatch { .. })).unwrap().id;
        assert_eq!(order(&d)[0], hatch.0);
        type_all(&mut e, &mut d, &["mtext", "40,0", "50,5", "Note", ""]);
        type_all(&mut e, &mut d, &["dr"]);
        click(&mut e, &mut d, 25.0, 0.0);
        type_all(&mut e, &mut d, &["", "f", "texttofront", ""]);
        let text = d.entities().find(|x| matches!(x.geometry, Geometry::Text { .. })).map(|x| x.id.0);
        assert_eq!(order(&d).last().copied(), text);
        type_all(&mut e, &mut d, &["hatchtoback"]);
        assert_eq!(order(&d)[0], hatch.0);
        assert!(e.history().iter().any(|l| l == "1 hatch object(s) sent to back."));
    }

    #[test]
    fn refedit_edits_a_block_in_place() {
        use oxidraft_model::{Block, BlockEntity, Color, Placement};
        let (mut e, mut d) = (engine(), Document::new());
        let line = Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) };
        d.define_block(Block {
            name: "B".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByLayer, line)],
        });
        let reference = d.add(Geometry::Insert {
            block: "B".into(),
            placement: Placement::at(Point::new(100.0, 0.0)),
            attributes: Vec::new(),
            clip: None,
        });
        let host = d.add(Geometry::Line { start: Point::new(0.0, 50.0), end: Point::new(10.0, 50.0) });
        type_all(&mut e, &mut d, &["refedit"]);
        click(&mut e, &mut d, 105.0, 0.0);
        assert!(d.refedit().is_some(), "{:?}", e.history());
        // The drawing's own objects can't be picked meanwhile.
        type_all(&mut e, &mut d, &["erase"]);
        click(&mut e, &mut d, 5.0, 50.0);
        type_all(&mut e, &mut d, &[""]);
        assert!(d.entity(host).is_some());
        // New objects join the working set; REFSET brings in the line.
        type_all(&mut e, &mut d, &["l", "100,5", "110,5", ""]);
        type_all(&mut e, &mut d, &["refset", "a"]);
        click(&mut e, &mut d, 5.0, 50.0);
        type_all(&mut e, &mut d, &[""]);
        assert!(e.history().iter().any(|l| l == "1 object(s) added to the working set."));
        type_all(&mut e, &mut d, &["refclose", ""]);
        assert!(d.refedit().is_none());
        assert_eq!(d.block("B").unwrap().entities.len(), 3);
        assert!(d.entity(host).is_none() && d.entity(reference).is_some());
        assert_eq!(d.entities().count(), 1);
        // Undo takes the edits back out of the block.
        type_all(&mut e, &mut d, &["u"]);
        assert!(d.refedit().is_some());
        // Discard from the panel, then REFCLOSE with nothing being edited.
        e.edit_reference("Discard", &mut d);
        assert!(d.refedit().is_none() && d.entity(host).is_some());
        assert_eq!(d.block("B").unwrap().entities.len(), 1);
        type_all(&mut e, &mut d, &["refclose"]);
        assert!(!e.is_active());
        assert!(e.history().iter().any(|l| l.starts_with("** Command not allowed unless a reference")));
    }

    #[test]
    fn xclip_clips_turns_off_inverts_and_traces() {
        use oxidraft_model::{Block, BlockEntity, Color, Placement};
        let (mut e, mut d) = (engine(), Document::new());
        let line = Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) };
        d.define_block(Block {
            name: "B".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByLayer, line)],
        });
        let placement = Placement { position: Point::new(100.0, 0.0), scale: 2.0, rotation: 0.0, mirror: false };
        let id = d.add(Geometry::Insert { block: "B".into(), placement, attributes: Vec::new(), clip: None });
        let shown = |d: &Document| -> f64 { d.expanded(d.entity(id).unwrap()).iter().map(Geometry::length).sum() };
        let clip = |d: &Document| match &d.entity(id).unwrap().geometry {
            Geometry::Insert { clip, .. } => clip.as_deref().cloned(),
            _ => None,
        };
        // The reference runs from 100 to 120: a rectangle keeps 100..110.
        type_all(&mut e, &mut d, &["xclip"]);
        click(&mut e, &mut d, 101.0, 0.0);
        type_all(&mut e, &mut d, &["", "", "", "95,-5", "110,5"]);
        assert!((shown(&d) - 10.0).abs() < 1e-9, "{}", shown(&d));
        // Kept in the block's own coordinates.
        assert!(clip(&d).unwrap().boundary.iter().any(|p| p.approx_eq(Point::new(5.0, 2.5))));
        for (option, expected) in [("off", 20.0), ("on", 10.0), ("d", 20.0)] {
            type_all(&mut e, &mut d, &["xc"]);
            click(&mut e, &mut d, 101.0, 0.0);
            type_all(&mut e, &mut d, &["", option]);
            assert!((shown(&d) - expected).abs() < 1e-9, "{option}: {}", shown(&d));
        }
        assert!(clip(&d).is_none());
        // An inverted triangle hides the 2.5 of the line under it.
        type_all(&mut e, &mut d, &["xclip"]);
        click(&mut e, &mut d, 101.0, 0.0);
        type_all(&mut e, &mut d, &["", "n", "i", "p", "105,-5", "110,-5", "110,5", ""]);
        assert!((shown(&d) - 17.5).abs() < 1e-9, "{}", shown(&d));
        assert!(clip(&d).unwrap().inverted);
        // A new boundary asks before replacing the old one; then a polyline's.
        type_all(&mut e, &mut d, &["rec", "90,-1", "115,1"]);
        type_all(&mut e, &mut d, &["xclip"]);
        click(&mut e, &mut d, 101.0, 0.0);
        type_all(&mut e, &mut d, &["", "n"]);
        assert!(e.prompt().starts_with("Delete old boundary(s)?"), "{}", e.prompt());
        type_all(&mut e, &mut d, &["y", "s"]);
        click(&mut e, &mut d, 90.0, 0.0);
        assert!((shown(&d) - 15.0).abs() < 1e-9, "{}", shown(&d));
        // CLIP does the same for a picked reference.
        type_all(&mut e, &mut d, &["clip"]);
        click(&mut e, &mut d, 101.0, 0.0);
        type_all(&mut e, &mut d, &["off"]);
        assert!(!clip(&d).unwrap().on);
        type_all(&mut e, &mut d, &["clip"]);
        click(&mut e, &mut d, 101.0, 0.0);
        type_all(&mut e, &mut d, &["on"]);
        // generate Polyline traces the boundary.
        let before = d.entities().count();
        type_all(&mut e, &mut d, &["xclip"]);
        click(&mut e, &mut d, 101.0, 0.0);
        type_all(&mut e, &mut d, &["", "p"]);
        assert_eq!(d.entities().count(), before + 1);
    }

    #[test]
    fn imageclip_clips_and_unclips_a_picture() {
        let (mut e, mut d) = (engine(), Document::new());
        d.add(Geometry::Image {
            adjust: Default::default(),
            image: "ortho".into(),
            origin: Point::ORIGIN,
            u: Point::new(100.0, 0.0),
            v: Point::new(0.0, 50.0),
            clip: Vec::new(),
        });
        type_all(&mut e, &mut d, &["icl"]);
        click(&mut e, &mut d, 50.0, 0.0);
        type_all(&mut e, &mut d, &["", "", "10,10", "60,40"]);
        let Geometry::Image { clip, .. } = only(&d) else { panic!() };
        assert_eq!(clip.len(), 4);
        assert!(clip.contains(&Point::new(10.0, 10.0)) && clip.contains(&Point::new(60.0, 40.0)));
        // The clipped picture is picked by its boundary now.
        type_all(&mut e, &mut d, &["icl"]);
        click(&mut e, &mut d, 35.0, 10.0);
        type_all(&mut e, &mut d, &["d"]);
        let Geometry::Image { clip, .. } = only(&d) else { panic!() };
        assert!(clip.is_empty());
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

    fn texts(d: &Document) -> Vec<(Point, f64, String)> {
        d.entities()
            .filter_map(|e| match &e.geometry {
                Geometry::Text { position, height, content, .. } => Some((*position, *height, content.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn single_line_text_stacks_lines() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["dt", "1,1", "3", "0", "Hello world", "Second line", ""]);
        let t = texts(&d);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], (Point::new(1.0, 1.0), 3.0, "Hello world".to_string()));
        assert!(t[1].0.approx_eq(Point::new(1.0, 1.0 - 5.0)), "next line 5/3 of the height below");
        assert!(!e.accepts_spaces() && !e.is_active());
        type_all(&mut e, &mut d, &["t"]);
        assert!(e.history().last().unwrap().contains("Text height:  3.0000"), "TEXTSIZE is remembered");
    }

    #[test]
    fn mtext_box_sets_position_and_width() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["mtext", "20,0", "0,10"]);
        assert!(e.accepts_spaces());
        type_all(&mut e, &mut d, &["General notes\\PSecond paragraph"]);
        let Geometry::Text { position, width, attach, .. } = only(&d) else { panic!() };
        assert_eq!((position, width, attach), (Point::new(0.0, 10.0), 20.0, oxidraft_model::Attach::TopLeft));
    }

    #[test]
    fn dimlinear_picks_horizontal_or_vertical_from_the_cursor() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["dli", "0,0", "10,3", "5,8"]);
        assert_eq!(e.history().last().unwrap(), "Dimension text = 10.00");
        type_all(&mut e, &mut d, &["dli", "0,0", "10,3", "15,1"]);
        assert_eq!(e.history().last().unwrap(), "Dimension text = 3.00");
        type_all(&mut e, &mut d, &["dli", "0,0", "10,0", "t", "<> TYP", "5,5"]);
        assert_eq!(e.history().last().unwrap(), "Dimension text = 10.00 TYP");
        type_all(&mut e, &mut d, &["dal", "0,0", "3,4", "0,5"]);
        assert_eq!(e.history().last().unwrap(), "Dimension text = 5.00");
        assert_eq!(d.len(), 4);
    }

    #[test]
    fn dimension_by_selecting_a_line() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "0,7", "", "dli", ""]);
        assert!(e.is_picking());
        click(&mut e, &mut d, 0.0, 3.0);
        type_all(&mut e, &mut d, &["-4,3"]);
        assert_eq!(e.history().last().unwrap(), "Dimension text = 7.00");
    }

    #[test]
    fn mleader_with_text() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["mld", "0,0", "10,10", "Check this corner"]);
        let Geometry::Leader { points, content, .. } = only(&d) else { panic!() };
        assert_eq!(points, vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)]);
        assert_eq!(content, "Check this corner");
    }

    #[test]
    fn hatch_a_room_with_a_column_and_pick_it() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "10,0", "10,10", "0,10", "c", "c", "7,7", "1"]);
        type_all(&mut e, &mut d, &["h"]);
        assert!(e.is_picking());
        click(&mut e, &mut d, 3.0, 5.0);
        type_all(&mut e, &mut d, &[""]);
        let hatch = d.entities().find_map(|x| match &x.geometry {
            Geometry::Hatch { loops, pattern, .. } => Some((x.id, loops.len(), *pattern)),
            _ => None,
        });
        let (id, loops, pattern) = hatch.expect("a hatch was made");
        assert_eq!((loops, pattern), (2, oxidraft_model::HatchPattern::Ansi31), "room plus the column island");

        // Clicking inside it (away from walls) selects the hatch.
        click(&mut e, &mut d, 3.0, 5.0);
        assert_eq!(e.selection(), &[id]);
        e.cancel(&mut d);

        // Pattern changes are remembered; Undo removes the last hatch.
        type_all(&mut e, &mut d, &["h", "p", "solid"]);
        click(&mut e, &mut d, 7.0, 7.0);
        type_all(&mut e, &mut d, &["u", ""]);
        assert_eq!(d.entities().filter(|x| matches!(x.geometry, Geometry::Hatch { .. })).count(), 1);
        type_all(&mut e, &mut d, &["h"]);
        assert!(e.history().last().unwrap().contains("SOLID"));

        click(&mut e, &mut d, 30.0, 30.0);
        assert_eq!(e.history().last().unwrap(), "Valid hatch boundary not found.");
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
        let names = |prefix: &str| Engine::suggest(prefix).iter().map(|c| c.name).collect::<Vec<_>>();
        // An exact alias first (C is CIRCLE, CO COPY, S STRETCH), then names
        // starting so, then other aliases starting so (CH: PROPERTIES).
        let c = names("c");
        assert_eq!(c[..3], ["CIRCLE", "COPY", "CHAMFER"]);
        assert_eq!(c.last(), Some(&"PROPERTIES"), "via its CH alias, after the names");
        assert!(c.iter().all(|n| n.starts_with('C') || *n == "PROPERTIES"));
        let co = names("co");
        assert_eq!(co[0], "COPY");
        assert!(co.iter().all(|n| n.starts_with("CO")));
        let s = names("s");
        assert_eq!(s[..2], ["STRETCH", "SPLINE"]);
        assert!(s.contains(&"SCALE") && s.contains(&"SNAP") && s.contains(&"SELECTSIMILAR"));
    }

    #[test]
    fn recent_commands_and_inputs_for_the_shortcut_menu() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["l", "0,0", "5,0", "5,5", "c", "c", "1,1", "2", "LINE", "3,3", "4,4", ""]);
        // The "c" that closed the line was an option, not CIRCLE.
        assert_eq!(e.recent_commands(5), vec!["LINE", "CIRCLE"]);
        assert_eq!(e.recent_inputs(3), vec!["4,4".to_string(), "3,3".to_string(), "2".to_string()]);
        assert_eq!(e.last_command(), Some("LINE"));
        e.select_all(&d);
        assert_eq!(e.selection().len(), 5, "three closed-line segments, the circle and the last line");
    }

    #[test]
    fn unknown_command_is_reported() {
        let (mut e, mut d) = (engine(), Document::new());
        type_all(&mut e, &mut d, &["frobnicate"]);
        assert!(e.history().last().unwrap().contains("Unknown command"));
    }
}

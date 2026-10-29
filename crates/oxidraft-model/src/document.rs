use std::collections::BTreeMap;

use oxidraft_geom::{Aabb, Point};

use crate::entity::{Attribute, Color, Entity, EntityId, Geometry, Vertex};
use crate::linetype::{self, CONTINUOUS, Linetype, Lineweight};
use crate::refedit::RefEdit;
use crate::underlay::ImageDef;
use crate::xref::{Xref, is_dependent};

/// ATTDISP: which block attributes are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttributeDisplay {
    /// Visible ones only.
    #[default]
    Normal,
    /// All of them, invisible ones too.
    On,
    /// None.
    Off,
}

/// A named layer. Layer "0" always exists, as in DXF.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    /// AutoCAD Color Index of entities drawn `ByLayer`.
    pub color: u8,
    /// Off layers aren't drawn or selectable.
    pub on: bool,
    /// Frozen layers behave like off layers (AutoCAD also skips them in regens).
    pub frozen: bool,
    /// Locked layers are drawn faded and snap, but can't be selected or edited.
    pub locked: bool,
    /// Linetype of objects drawn ByLayer.
    pub linetype: String,
    /// Lineweight of objects drawn ByLayer (Default, or a weight).
    pub lineweight: Lineweight,
    /// Transparency of objects drawn ByLayer, 0 to 90 percent.
    pub transparency: u8,
}

impl Layer {
    pub fn new(name: impl Into<String>, color: u8) -> Self {
        Self {
            name: name.into(),
            color,
            on: true,
            frozen: false,
            locked: false,
            linetype: CONTINUOUS.into(),
            lineweight: Lineweight::Default,
            transparency: 0,
        }
    }

    pub fn is_shown(&self) -> bool {
        self.on && !self.frozen
    }
}

/// Characters AutoCAD doesn't allow in layer and block names.
const BAD_NAME_CHARS: &[char] = &['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`'];

/// Checks a layer or block name the way AutoCAD does.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err("The name can't be empty.".into())
    } else if name.len() > 255 || name.contains(BAD_NAME_CHARS) {
        Err(format!("\"{name}\" isn't a valid name (it can't contain {}).", BAD_NAME_CHARS.iter().collect::<String>()))
    } else {
        Ok(())
    }
}

/// `name` made into a valid layer or block name: characters AutoCAD
/// doesn't allow become underscores ("plan, sheet 2" → "plan_ sheet 2").
pub fn sanitize_name(name: &str) -> String {
    let clean: String = name.trim().chars().map(|c| if BAD_NAME_CHARS.contains(&c) { '_' } else { c }).collect();
    let clean: String = clean.chars().take(255).collect();
    if clean.trim().is_empty() { "Unnamed".into() } else { clean }
}

/// An object inside a block definition.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntity {
    pub layer: String,
    pub color: Color,
    pub linetype: String,
    pub lineweight: Lineweight,
    pub geometry: Geometry,
}

impl BlockEntity {
    /// An object with ByLayer linetype and lineweight.
    pub fn new(layer: impl Into<String>, color: Color, geometry: Geometry) -> Self {
        Self {
            layer: layer.into(),
            color,
            linetype: linetype::BYLAYER.into(),
            lineweight: Lineweight::ByLayer,
            geometry,
        }
    }
}

/// One curve, text or fill of an expanded object, with how it's drawn:
/// its ACI color, layer, linetype name and lineweight, all resolved (no
/// ByLayer or ByBlock left).
#[derive(Debug, Clone, PartialEq)]
pub struct Styled {
    pub geometry: Geometry,
    pub color: u8,
    pub layer: String,
    pub linetype: String,
    pub lineweight: Lineweight,
}

/// What ByBlock means inside a block reference: the reference's own
/// resolved color, linetype and lineweight.
#[derive(Clone)]
struct Inherited {
    color: u8,
    linetype: String,
    lineweight: Lineweight,
}

impl Default for Inherited {
    fn default() -> Self {
        Self { color: 7, linetype: CONTINUOUS.into(), lineweight: Lineweight::Default }
    }
}

/// A named block definition; block references place copies of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub name: String,
    /// The point that lands on a reference's insertion point.
    pub base: Point,
    pub entities: Vec<BlockEntity>,
}

impl Block {
    /// What a new reference asks for: the block's attribute definitions
    /// that aren't constant, as (tag, prompt, default).
    pub fn attribute_prompts(&self) -> Vec<(String, String, String)> {
        self.entities
            .iter()
            .filter_map(|be| match &be.geometry {
                Geometry::AttributeDefinition { tag, prompt, default, constant: false, .. } => {
                    Some((tag.clone(), prompt.clone(), default.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// A reference's attributes, placed as it is: each definition's value
    /// is `value_of` its tag (or its default).
    pub fn attributes_for(
        &self,
        placement: &crate::entity::Placement,
        value_of: impl Fn(&str) -> Option<String>,
    ) -> Vec<crate::entity::Attribute> {
        self.entities
            .iter()
            .filter_map(|be| {
                let Geometry::AttributeDefinition {
                    tag,
                    default,
                    position,
                    height,
                    rotation,
                    attach,
                    invisible,
                    constant: false,
                    ..
                } = &be.geometry
                else {
                    return None;
                };
                let text = Geometry::Text {
                    look: Default::default(),
                    position: *position,
                    height: *height,
                    rotation: *rotation,
                    width: 0.0,
                    attach: *attach,
                    content: String::new(),
                };
                let Geometry::Text { position, height, rotation, .. } = placement.place(self.base, &text) else {
                    return None;
                };
                Some(crate::entity::Attribute {
                    tag: tag.clone(),
                    value: value_of(tag).unwrap_or_else(|| default.clone()),
                    layer: be.layer.clone(),
                    color: be.color,
                    position,
                    height,
                    rotation,
                    attach: *attach,
                    invisible: *invisible,
                })
            })
            .collect()
    }
}

/// The layer table and the current layer, recorded whole for undo.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayerTable {
    layers: Vec<Layer>,
    current: String,
}

/// One edit, recorded with what it replaced so it can be undone.
#[derive(Debug, Clone)]
pub(crate) enum Change {
    /// `before` is `None` for an add, `after` is `None` for a removal.
    Entity {
        id: EntityId,
        before: Option<Entity>,
        after: Option<Entity>,
    },
    Layers {
        before: LayerTable,
        after: LayerTable,
    },
    Block {
        name: String,
        before: Option<Block>,
        after: Option<Block>,
    },
    /// An attached drawing's record, by upper-case name.
    Xref {
        key: String,
        before: Option<Xref>,
        after: Option<Xref>,
    },
    /// An image or PDF underlay's definition, by upper-case name.
    Image {
        key: String,
        before: Option<ImageDef>,
        after: Option<ImageDef>,
    },
    /// An object's place in the draw order (`None`: where it was made).
    Order {
        id: EntityId,
        before: Option<f64>,
        after: Option<f64>,
    },
    /// The layouts (their names, order and sheets).
    Layouts {
        before: Vec<crate::layout::Layout>,
        after: Vec<crate::layout::Layout>,
    },
    Views {
        before: Vec<NamedView>,
        after: Vec<NamedView>,
    },
    Linetype {
        key: String,
        before: Option<Linetype>,
        after: Option<Linetype>,
    },
    Groups {
        before: Vec<Group>,
        after: Vec<Group>,
    },
    DimStyle {
        before: crate::annotation::DimStyle,
        after: crate::annotation::DimStyle,
    },
    Units {
        before: crate::units::Units,
        after: crate::units::Units,
    },
    Current {
        before: CurrentProps,
        after: CurrentProps,
    },
    LayerStates {
        before: Vec<LayerState>,
        after: Vec<LayerState>,
    },
    Properties {
        before: DrawingProperties,
        after: DrawingProperties,
    },
    /// A dimension's associations (DIMREASSOCIATE, DIMDISASSOCIATE).
    DimAssoc {
        id: EntityId,
        before: Option<crate::dimassoc::DimAssoc>,
        after: Option<crate::dimassoc::DimAssoc>,
    },
    /// The multileader and table styles.
    AnnotationStyles {
        before: (crate::annotation::LeaderStyle, crate::table::TableStyle),
        after: (crate::annotation::LeaderStyle, crate::table::TableStyle),
    },
    /// The text styles and the current one.
    TextStyles {
        before: (Vec<crate::annotation::TextStyle>, String),
        after: (Vec<crate::annotation::TextStyle>, String),
    },
    /// REFEDIT's session: started, its working set changed, or ended.
    RefEdit {
        before: Option<Box<RefEdit>>,
        after: Option<Box<RefEdit>>,
    },
}

#[derive(Debug, Clone)]
struct Transaction {
    name: String,
    changes: Vec<Change>,
}

/// Nesting limit when expanding block references (guards against cycles).
const MAX_BLOCK_DEPTH: usize = 16;

/// A drawing: entities, layers, block definitions and undo/redo history.
///
/// Edits made between [`Document::begin`] and [`Document::commit`] undo as a
/// single step. Edits made outside a transaction each get their own step.
#[derive(Debug, Clone)]
pub struct Document {
    entities: BTreeMap<EntityId, Entity>,
    table: LayerTable,
    blocks: BTreeMap<String, Block>,
    next_id: u64,
    open: Option<Transaction>,
    undo_stack: Vec<Transaction>,
    redo_stack: Vec<Transaction>,
    pub(crate) revision: u64,
    /// Linetypes the drawing defines, by upper-case name, besides the standard ones.
    linetypes: BTreeMap<String, Linetype>,
    /// LTSCALE: global scale of linetype patterns.
    ltscale: f64,
    /// PDMODE and PDSIZE: how points are drawn.
    pdmode: i32,
    pdsize: f64,
    attdisp: AttributeDisplay,
    /// INSBASE: the point that lands where the drawing is inserted or pasted.
    insbase: Point,
    /// The current UCS, the ones before it (UCS Previous), and the named
    /// ones.
    ucs: crate::ucs::Ucs,
    ucs_previous: Vec<crate::ucs::Ucs>,
    named_ucs: Vec<crate::ucs::NamedUcs>,
    /// Associative dimensions: what each one's definition points follow.
    pub(crate) dim_assoc: BTreeMap<EntityId, crate::dimassoc::DimAssoc>,
    /// ULAYERS: each PDF underlay's layers turned off, by its name.
    pdf_layers_off: BTreeMap<String, Vec<String>>,
    /// CANNOSCALE: drawing units per paper unit; ANNOALLVISIBLE.
    pub(crate) anno_scale: f64,
    pub(crate) anno_all_visible: bool,
    /// LIMITS: LIMMIN and LIMMAX, and LIMCHECK (no points outside them).
    limits: (Point, Point),
    limits_check: bool,
    /// FILLMODE (fills, wide polylines and solids filled) and QTEXTMODE
    /// (text shown as boxes).
    fill: bool,
    quick_text: bool,
    /// Attached drawings (XREF), by upper-case name.
    xrefs: BTreeMap<String, Xref>,
    /// Attached images and PDF pages, by upper-case name.
    images: BTreeMap<String, ImageDef>,
    /// Draw order: objects draw from the lowest key to the highest. Each
    /// object's key is its id unless DRAWORDER has moved it.
    order: BTreeMap<EntityId, f64>,
    /// The highest key given out, so new objects still go on top.
    top: f64,
    /// The block reference or xref being edited in place (REFEDIT).
    refedit: Option<RefEdit>,
    /// The layouts, and the space being drawn in: model space, or one of
    /// them. [`Document::entities`] and what's added are that space's.
    layouts: Vec<crate::layout::Layout>,
    space: crate::layout::Space,
    /// Named views (VIEW), by name.
    views: Vec<NamedView>,
    /// WIPEOUTFRAME: wipeouts' frames shown and plotted (1), shown but
    /// not plotted (2), or neither (0).
    wipeout_frame: u8,
    /// IMAGEFRAME and PDFFRAME, the same for images and PDF underlays.
    image_frame: u8,
    pdf_frame: u8,
    /// Groups (GROUP), by name.
    groups: Vec<Group>,
    /// The dimension style (DIMSTYLE) dimensions are drawn in.
    dim_style: crate::annotation::DimStyle,
    /// UNITS: how lengths and angles show, and what a unit is.
    units: crate::units::Units,
    /// New objects' color, linetype and lineweight.
    current: CurrentProps,
    /// LAYERSTATE: named layer settings.
    layer_states: Vec<LayerState>,
    /// DWGPROPS.
    properties: DrawingProperties,
    /// MLEADERSTYLE and TABLESTYLE.
    leader_style: crate::annotation::LeaderStyle,
    table_style: crate::table::TableStyle,
    /// Text styles (STYLE), Standard first, and the one new text gets.
    text_styles: Vec<crate::annotation::TextStyle>,
    current_text_style: String,
    /// The layer settings before each change, latest last (LAYERP).
    previous_layers: Vec<LayerTable>,
}

/// A group (GROUP): objects picked together. Unnamed groups are called
/// *A1, *A2, …
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub description: String,
    pub members: Vec<EntityId>,
}

impl Group {
    pub fn is_unnamed(&self) -> bool {
        self.name.starts_with('*')
    }
}

/// DWGPROPS: the drawing's summary properties.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DrawingProperties {
    pub title: String,
    pub subject: String,
    pub author: String,
    pub keywords: String,
    pub comments: String,
}

/// A named snapshot of the layers' settings (LAYERSTATE): each layer's on,
/// frozen and locked states, color, linetype, lineweight and transparency,
/// and which was current.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerState {
    pub name: String,
    pub description: String,
    pub layers: Vec<Layer>,
    pub current: String,
}

/// What new objects get besides the current layer: CECOLOR, CELTYPE and
/// CELWEIGHT (ByLayer unless set).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CurrentProps {
    pub color: Color,
    pub linetype: String,
    pub lineweight: Lineweight,
    /// CETRANSPARENCY.
    pub transparency: crate::linetype::Transparency,
}

impl CurrentProps {
    pub fn by_layer() -> Self {
        Self {
            color: Color::ByLayer,
            linetype: linetype::BYLAYER.into(),
            lineweight: Lineweight::ByLayer,
            transparency: crate::linetype::Transparency::ByLayer,
        }
    }
}

/// A view saved by name (VIEW): what's shown, `height` tall and `width`
/// wide around `center`, in model space or a layout.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedView {
    pub name: String,
    pub center: Point,
    pub height: f64,
    pub width: f64,
    pub space: crate::layout::Space,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            table: LayerTable { layers: vec![Layer::new("0", 7)], current: "0".into() },
            blocks: BTreeMap::new(),
            next_id: 1,
            open: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            revision: 0,
            linetypes: BTreeMap::new(),
            ltscale: 1.0,
            pdmode: 0,
            pdsize: 0.0,
            attdisp: AttributeDisplay::Normal,
            insbase: Point::ORIGIN,
            ucs: crate::ucs::WORLD,
            ucs_previous: Vec::new(),
            named_ucs: Vec::new(),
            pdf_layers_off: BTreeMap::new(),
            dim_assoc: BTreeMap::new(),
            anno_scale: 1.0,
            anno_all_visible: true,
            limits: (Point::ORIGIN, Point::new(420.0, 297.0)),
            limits_check: false,
            fill: true,
            quick_text: false,
            xrefs: BTreeMap::new(),
            images: BTreeMap::new(),
            order: BTreeMap::new(),
            top: 0.0,
            refedit: None,
            layouts: Vec::new(),
            space: crate::layout::MODEL,
            views: Vec::new(),
            wipeout_frame: 1,
            image_frame: 1,
            pdf_frame: 1,
            groups: Vec::new(),
            dim_style: crate::annotation::DEFAULT_DIM_STYLE,
            units: crate::units::DEFAULT_UNITS,
            current: CurrentProps::by_layer(),
            layer_states: Vec::new(),
            properties: DrawingProperties::default(),
            leader_style: crate::annotation::DEFAULT_LEADER_STYLE,
            table_style: crate::table::DEFAULT_TABLE_STYLE,
            text_styles: vec![crate::annotation::TextStyle::standard()],
            current_text_style: crate::annotation::STANDARD.into(),
            previous_layers: Vec::new(),
        }
    }

    /// Increases on every change, including undo and redo.
    /// Compare against a saved value to show the unsaved-changes marker.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    // ---- queries -------------------------------------------------------

    /// The objects in the space being drawn in (model space, or a layout).
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values().filter(move |e| e.space == self.space)
    }

    /// Every object, in every space.
    pub fn all_entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    /// The objects of `space`, in draw order.
    pub fn space_in_draw_order(&self, space: crate::layout::Space) -> Vec<&Entity> {
        let mut all: Vec<&Entity> = self.entities.values().filter(|e| e.space == space).collect();
        if !self.order.is_empty() {
            all.sort_by(|a, b| self.draw_key(a.id).total_cmp(&self.draw_key(b.id)));
        }
        all
    }

    // ---- layouts ----------------------------------------------------------------

    /// The layouts, in tab order.
    pub fn layouts(&self) -> Vec<&crate::layout::Layout> {
        let mut all: Vec<&crate::layout::Layout> = self.layouts.iter().collect();
        all.sort_by_key(|l| l.tab_order);
        all
    }

    pub fn layout(&self, space: crate::layout::Space) -> Option<&crate::layout::Layout> {
        self.layouts.iter().find(|l| l.id == space)
    }

    /// Adds a layout (its `id` is given here) and returns its space.
    pub fn add_layout(&mut self, mut layout: crate::layout::Layout) -> crate::layout::Space {
        layout.id = self.layouts.iter().map(|l| l.id).max().unwrap_or(crate::layout::MODEL) + 1;
        let id = layout.id;
        let mut after = self.layouts.clone();
        after.push(layout);
        self.set_layouts(after);
        id
    }

    /// DIMSTYLE: what dimensions are drawn with.
    pub fn dim_style(&self) -> crate::annotation::DimStyle {
        self.dim_style
    }

    pub fn set_dim_style(&mut self, style: crate::annotation::DimStyle) {
        if style != self.dim_style {
            let before = std::mem::replace(&mut self.dim_style, style);
            self.record(Change::DimStyle { before, after: style });
        }
    }

    /// UNITS: how lengths and angles show, and what a drawing unit is.
    pub fn units(&self) -> crate::units::Units {
        self.units
    }

    pub fn set_units(&mut self, units: crate::units::Units) {
        if units != self.units {
            let before = std::mem::replace(&mut self.units, units);
            self.record(Change::Units { before, after: units });
        }
    }

    /// CECOLOR, CELTYPE and CELWEIGHT: what new objects get.
    pub fn current_props(&self) -> &CurrentProps {
        &self.current
    }

    pub fn set_current_props(&mut self, props: CurrentProps) {
        if props != self.current {
            let before = std::mem::replace(&mut self.current, props.clone());
            self.record(Change::Current { before, after: props });
        }
    }

    /// DWGPROPS: title, subject, author, keywords and comments.
    pub fn properties(&self) -> &DrawingProperties {
        &self.properties
    }

    pub fn set_properties(&mut self, props: DrawingProperties) {
        if props != self.properties {
            let before = std::mem::replace(&mut self.properties, props.clone());
            self.record(Change::Properties { before, after: props });
        }
    }

    /// LAYERSTATE: the saved layer states, by name.
    pub fn layer_states(&self) -> &[LayerState] {
        &self.layer_states
    }

    fn set_layer_states(&mut self, after: Vec<LayerState>) {
        if after != self.layer_states {
            let before = std::mem::replace(&mut self.layer_states, after.clone());
            self.record(Change::LayerStates { before, after });
        }
    }

    /// Saves the layers' settings now as `name` (replacing one so named).
    pub fn save_layer_state(&mut self, name: &str, description: &str) -> Result<(), String> {
        check_name(name.trim())?;
        let state = LayerState {
            name: name.trim().into(),
            description: description.into(),
            layers: self.table.layers.clone(),
            current: self.table.current.clone(),
        };
        self.add_layer_state(state);
        Ok(())
    }

    /// Adds a layer state (from a file), replacing one with its name.
    pub fn add_layer_state(&mut self, state: LayerState) {
        let mut states: Vec<LayerState> =
            self.layer_states.iter().filter(|s| !s.name.eq_ignore_ascii_case(&state.name)).cloned().collect();
        states.push(state);
        states.sort_by_key(|s| s.name.to_lowercase());
        self.set_layer_states(states);
    }

    /// Puts the layers back as state `name` had them; layers it doesn't
    /// know stay as they are.
    pub fn restore_layer_state(&mut self, name: &str) -> Result<(), String> {
        let state = self.layer_states.iter().find(|s| s.name.eq_ignore_ascii_case(name.trim())).cloned();
        let state = state.ok_or_else(|| format!("Layer state \"{}\" not found.", name.trim()))?;
        let mut table = self.table.clone();
        for l in &mut table.layers {
            if let Some(saved) = state.layers.iter().find(|s| s.name.eq_ignore_ascii_case(&l.name)) {
                *l = Layer { name: l.name.clone(), ..saved.clone() };
            }
        }
        if table.layers.iter().any(|l| l.name.eq_ignore_ascii_case(&state.current)) {
            table.current = table
                .layers
                .iter()
                .find(|l| l.name.eq_ignore_ascii_case(&state.current))
                .map(|l| l.name.clone())
                .unwrap_or(table.current);
        }
        self.set_table(table);
        Ok(())
    }

    pub fn delete_layer_state(&mut self, name: &str) -> bool {
        let states: Vec<LayerState> =
            self.layer_states.iter().filter(|s| !s.name.eq_ignore_ascii_case(name.trim())).cloned().collect();
        let found = states.len() != self.layer_states.len();
        self.set_layer_states(states);
        found
    }

    pub fn rename_layer_state(&mut self, old: &str, new: &str) -> Result<(), String> {
        check_name(new.trim())?;
        if !old.eq_ignore_ascii_case(new.trim())
            && self.layer_states.iter().any(|s| s.name.eq_ignore_ascii_case(new.trim()))
        {
            return Err(format!("A layer state named \"{}\" already exists.", new.trim()));
        }
        let mut states = self.layer_states.clone();
        let s = states.iter_mut().find(|s| s.name.eq_ignore_ascii_case(old.trim())).ok_or("Layer state not found.")?;
        s.name = new.trim().into();
        self.set_layer_states(states);
        Ok(())
    }

    /// MLEADERSTYLE: how multileaders look.
    pub fn leader_style(&self) -> crate::annotation::LeaderStyle {
        self.leader_style
    }

    /// TABLESTYLE: how new tables look.
    pub fn table_style(&self) -> crate::table::TableStyle {
        self.table_style
    }

    pub fn set_annotation_styles(&mut self, leader: crate::annotation::LeaderStyle, table: crate::table::TableStyle) {
        let after = (leader, table);
        if after != (self.leader_style, self.table_style) {
            let before = (self.leader_style, self.table_style);
            (self.leader_style, self.table_style) = after;
            self.record(Change::AnnotationStyles { before, after });
        }
    }

    /// The text styles, Standard first.
    pub fn text_styles(&self) -> &[crate::annotation::TextStyle] {
        &self.text_styles
    }

    pub fn text_style(&self, name: &str) -> Option<&crate::annotation::TextStyle> {
        self.text_styles.iter().find(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }

    /// The style new text gets (TEXTSTYLE).
    pub fn current_text_style(&self) -> crate::annotation::TextStyle {
        self.text_style(&self.current_text_style).cloned().unwrap_or_else(crate::annotation::TextStyle::standard)
    }

    fn set_text_styles(&mut self, styles: Vec<crate::annotation::TextStyle>, current: String) {
        let after = (styles, current);
        if after.0 != self.text_styles || after.1 != self.current_text_style {
            let before = (
                std::mem::replace(&mut self.text_styles, after.0.clone()),
                std::mem::replace(&mut self.current_text_style, after.1.clone()),
            );
            self.record(Change::TextStyles { before, after });
        }
    }

    /// Adds a text style, or changes the one with its name (ignoring case).
    pub fn set_text_style(&mut self, style: crate::annotation::TextStyle) -> Result<(), String> {
        check_name(&style.name)?;
        let valid = style.width_factor > 0.0 && style.height >= 0.0 && style.oblique.abs() <= 85.0;
        if !valid {
            return Err("Width factor must be above 0, height 0 or more, and oblique within 85 degrees.".into());
        }
        let mut styles = self.text_styles.clone();
        match styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&style.name)) {
            Some(s) => *s = crate::annotation::TextStyle { name: s.name.clone(), ..style },
            None => styles.push(style),
        }
        self.set_text_styles(styles, self.current_text_style.clone());
        Ok(())
    }

    /// Makes a text style the one new text gets.
    pub fn set_current_text_style(&mut self, name: &str) -> Result<(), String> {
        let style = self.text_style(name).ok_or_else(|| format!("Text style \"{}\" not found.", name.trim()))?;
        let name = style.name.clone();
        self.set_text_styles(self.text_styles.clone(), name);
        Ok(())
    }

    /// True if some text (in a block too) uses the style.
    pub fn text_style_in_use(&self, name: &str) -> bool {
        let uses = |g: &Geometry| matches!(g, Geometry::Text { look, .. } if look.style.eq_ignore_ascii_case(name));
        self.entities.values().any(|e| uses(&e.geometry))
            || self.blocks.values().any(|b| b.entities.iter().any(|be| uses(&be.geometry)))
    }

    /// Deletes a text style nothing uses. Standard and the current one stay.
    pub fn delete_text_style(&mut self, name: &str) -> Result<(), String> {
        let style =
            self.text_style(name).cloned().ok_or_else(|| format!("Text style \"{}\" not found.", name.trim()))?;
        if style.name.eq_ignore_ascii_case(crate::annotation::STANDARD) {
            return Err("The Standard text style can't be deleted.".into());
        }
        if style.name.eq_ignore_ascii_case(&self.current_text_style) {
            return Err("The current text style can't be deleted.".into());
        }
        if self.text_style_in_use(&style.name) {
            return Err(format!("Text style \"{}\" is in use.", style.name));
        }
        let styles = self.text_styles.iter().filter(|s| s.name != style.name).cloned().collect();
        self.set_text_styles(styles, self.current_text_style.clone());
        Ok(())
    }

    /// RENAME a text style, and the text that uses it. Standard can't be.
    pub fn rename_text_style(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        check_name(new)?;
        let style = self.text_style(old).cloned().ok_or_else(|| format!("Text style \"{}\" not found.", old.trim()))?;
        if style.name.eq_ignore_ascii_case(crate::annotation::STANDARD) {
            return Err("The Standard text style can't be renamed.".into());
        }
        if !new.eq_ignore_ascii_case(&style.name) && self.text_style(new).is_some() {
            return Err(format!("A text style named \"{new}\" already exists."));
        }
        let old = style.name.clone();
        let styles =
            self.text_styles
                .iter()
                .map(|s| {
                    if s.name == old {
                        crate::annotation::TextStyle { name: new.into(), ..s.clone() }
                    } else {
                        s.clone()
                    }
                })
                .collect();
        let current = if self.current_text_style.eq_ignore_ascii_case(&old) {
            new.into()
        } else {
            self.current_text_style.clone()
        };
        self.set_text_styles(styles, current);
        let restyle = |g: &mut Geometry| {
            if let Geometry::Text { look, .. } = g
                && look.style.eq_ignore_ascii_case(&old)
            {
                look.style = new.to_string();
            }
        };
        let ids: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| matches!(&e.geometry, Geometry::Text { look, .. } if look.style.eq_ignore_ascii_case(&old)))
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.update(id, |e| restyle(&mut e.geometry));
        }
        let blocks: Vec<Block> = self
            .blocks
            .values()
            .filter(|b| {
                b.entities.iter().any(
                    |be| matches!(&be.geometry, Geometry::Text { look, .. } if look.style.eq_ignore_ascii_case(&old)),
                )
            })
            .cloned()
            .collect();
        for mut b in blocks {
            b.entities.iter_mut().for_each(|be| restyle(&mut be.geometry));
            self.define_block(b);
        }
        Ok(())
    }

    /// How see-through `e` draws, 0 to 90 percent: its own, or its layer's
    /// (ByBlock: opaque at the top level).
    pub fn transparency_of(&self, e: &Entity) -> u8 {
        match e.transparency {
            crate::linetype::Transparency::Percent(p) => p.min(90),
            crate::linetype::Transparency::ByLayer => self.layer(&e.layer).map_or(0, |l| l.transparency.min(90)),
            crate::linetype::Transparency::ByBlock => 0,
        }
    }

    /// Loads a linetype into the drawing (LINETYPE's Load), undoably.
    pub fn load_linetype(&mut self, l: Linetype) {
        if l.name.eq_ignore_ascii_case(CONTINUOUS) {
            return;
        }
        let key = l.name.to_ascii_uppercase();
        let before = self.linetypes.insert(key.clone(), l.clone());
        if before.as_ref() != Some(&l) {
            self.record(Change::Linetype { key, before, after: Some(l) });
        }
    }

    /// The groups, their members that still exist.
    pub fn groups(&self) -> Vec<Group> {
        self.groups
            .iter()
            .map(|g| Group {
                members: g.members.iter().copied().filter(|id| self.entities.contains_key(id)).collect(),
                ..g.clone()
            })
            .filter(|g| !g.members.is_empty())
            .collect()
    }

    /// The group `id` is in, if any.
    pub fn group_of(&self, id: EntityId) -> Option<Group> {
        self.groups().into_iter().find(|g| g.members.contains(&id))
    }

    /// Makes a group of `members`, called `name`, or unnamed (*A1, …)
    /// if `name` is empty. Returns its name.
    pub fn add_group(&mut self, name: &str, description: &str, members: Vec<EntityId>) -> Result<String, String> {
        let name = if name.trim().is_empty() {
            (1..).map(|n| format!("*A{n}")).find(|n| !self.groups.iter().any(|g| &g.name == n)).unwrap_or_default()
        } else {
            check_name(name)?;
            if self.groups.iter().any(|g| g.name.eq_ignore_ascii_case(name.trim())) {
                return Err(format!("A group named \"{}\" already exists.", name.trim()));
            }
            name.trim().to_string()
        };
        let mut after = self.groups.clone();
        after.push(Group { name: name.clone(), description: description.into(), members });
        self.set_groups(after);
        Ok(name)
    }

    /// Replaces the groups (ungrouping, editing a group).
    pub fn set_groups(&mut self, groups: Vec<Group>) {
        let before = std::mem::replace(&mut self.groups, groups.clone());
        if before != groups {
            self.record(Change::Groups { before, after: groups });
        }
    }

    /// The groups as kept, with members that may have been erased (for
    /// undoable edits that keep them).
    pub fn all_groups(&self) -> &[Group] {
        &self.groups
    }

    /// WIPEOUTFRAME.
    pub fn wipeout_frame(&self) -> u8 {
        self.wipeout_frame
    }

    /// IMAGEFRAME, or PDFFRAME for PDF underlays.
    pub fn picture_frame(&self, pdf: bool) -> u8 {
        if pdf { self.pdf_frame } else { self.image_frame }
    }

    /// The frame setting of the picture `image` shows.
    pub fn frame_of(&self, image: &str) -> u8 {
        self.picture_frame(self.image_def(image).is_some_and(ImageDef::is_pdf))
    }

    pub fn set_picture_frame(&mut self, pdf: bool, mode: u8) {
        let frame = if pdf { &mut self.pdf_frame } else { &mut self.image_frame };
        if mode <= 2 && mode != *frame {
            *frame = mode;
            self.revision += 1;
        }
    }

    pub fn set_wipeout_frame(&mut self, mode: u8) {
        if mode <= 2 && mode != self.wipeout_frame {
            self.wipeout_frame = mode;
            self.revision += 1;
        }
    }

    /// The named views, by name.
    pub fn views(&self) -> &[NamedView] {
        &self.views
    }

    /// Saves a named view, replacing one with its name (ignoring case).
    pub fn save_view(&mut self, view: NamedView) -> Result<(), String> {
        check_name(&view.name)?;
        let mut after: Vec<NamedView> =
            self.views.iter().filter(|v| !v.name.eq_ignore_ascii_case(view.name.trim())).cloned().collect();
        after.push(NamedView { name: view.name.trim().into(), ..view });
        after.sort_by_key(|v| v.name.to_lowercase());
        self.set_views(after);
        Ok(())
    }

    pub fn delete_view(&mut self, name: &str) -> bool {
        let after: Vec<NamedView> = self.views.iter().filter(|v| !v.name.eq_ignore_ascii_case(name)).cloned().collect();
        let found = after.len() != self.views.len();
        self.set_views(after);
        found
    }

    /// RENAME a named view.
    pub fn rename_view(&mut self, old: &str, new: &str) -> Result<(), String> {
        check_name(new)?;
        let view = self.views.iter().find(|v| v.name.eq_ignore_ascii_case(old.trim())).cloned();
        let view = view.ok_or_else(|| format!("View \"{old}\" not found."))?;
        let taken = self.views.iter().any(|v| v.name.eq_ignore_ascii_case(new.trim()) && v.name != view.name);
        if taken {
            return Err(format!("A view named \"{}\" already exists.", new.trim()));
        }
        let mut after: Vec<NamedView> = self.views.iter().filter(|v| v.name != view.name).cloned().collect();
        after.push(NamedView { name: new.trim().into(), ..view });
        after.sort_by_key(|v| v.name.to_lowercase());
        self.set_views(after);
        Ok(())
    }

    fn set_views(&mut self, views: Vec<NamedView>) {
        let before = std::mem::replace(&mut self.views, views.clone());
        if before != views {
            self.record(Change::Views { before, after: views });
        }
    }

    /// A name for a new layout: the first of Layout1, Layout2, … not taken.
    pub fn next_layout_name(&self) -> String {
        (1..).map(|n| format!("Layout{n}")).find(|n| self.layout_named(n).is_none()).unwrap_or_default()
    }

    /// The layout called `name`, ignoring case.
    pub fn layout_named(&self, name: &str) -> Option<&crate::layout::Layout> {
        self.layouts.iter().find(|l| l.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Whether `name` can name a layout (but `except`): valid, not
    /// "Model", and not another layout's, ignoring case.
    pub fn check_layout_name(&self, name: &str, except: Option<crate::layout::Space>) -> Result<(), String> {
        check_name(name)?;
        if name.trim().eq_ignore_ascii_case("Model") {
            return Err("A layout can't be called Model.".into());
        }
        match self.layout_named(name) {
            Some(l) if Some(l.id) != except => Err(format!("A layout named \"{}\" already exists.", l.name)),
            _ => Ok(()),
        }
    }

    /// A new layout after the others, on an A4 sheet. Returns its space.
    pub fn new_layout(&mut self, name: &str) -> Result<crate::layout::Space, String> {
        self.check_layout_name(name, None)?;
        let tab_order = self.layouts.iter().map(|l| l.tab_order).max().unwrap_or(0) + 1;
        let paper = crate::layout::Paper::a4_landscape();
        let layout = crate::layout::Layout { id: crate::layout::MODEL, name: name.trim().into(), tab_order, paper };
        Ok(self.add_layout(layout))
    }

    pub fn rename_layout(&mut self, space: crate::layout::Space, name: &str) -> Result<(), String> {
        self.check_layout_name(name, Some(space))?;
        let mut after = self.layouts.clone();
        let layout = after.iter_mut().find(|l| l.id == space).ok_or("There's no such layout.")?;
        layout.name = name.trim().into();
        self.set_layouts(after);
        Ok(())
    }

    /// Changes a layout's page setup.
    pub fn set_paper(&mut self, space: crate::layout::Space, paper: crate::layout::Paper) -> bool {
        let mut after = self.layouts.clone();
        let Some(layout) = after.iter_mut().find(|l| l.id == space) else { return false };
        layout.paper = paper;
        self.set_layouts(after);
        true
    }

    /// Deletes a layout and everything on its sheet. Drawing in it, the
    /// drawing goes back to model space.
    pub fn delete_layout(&mut self, space: crate::layout::Space) -> bool {
        if self.layout(space).is_none() {
            return false;
        }
        let ids: Vec<EntityId> = self.entities.values().filter(|e| e.space == space).map(|e| e.id).collect();
        for id in ids {
            self.remove(id);
        }
        let after = self.layouts.iter().filter(|l| l.id != space).cloned().collect();
        self.set_layouts(after);
        if self.space == space {
            self.space = crate::layout::MODEL;
        }
        true
    }

    /// Moves a layout's tab before `before`'s, or to the end.
    pub fn move_layout(&mut self, space: crate::layout::Space, before: Option<crate::layout::Space>) {
        let mut order: Vec<crate::layout::Space> =
            self.layouts().iter().map(|l| l.id).filter(|id| *id != space).collect();
        let at = before.and_then(|b| order.iter().position(|id| *id == b)).unwrap_or(order.len());
        order.insert(at, space);
        let mut after = self.layouts.clone();
        for l in &mut after {
            l.tab_order = order.iter().position(|id| *id == l.id).map_or(l.tab_order, |i| i as i32 + 1);
        }
        self.set_layouts(after);
    }

    /// A copy of a layout, with its page setup and everything on its
    /// sheet, called `name` and placed just after it. Returns its space.
    pub fn copy_layout(&mut self, space: crate::layout::Space, name: &str) -> Result<crate::layout::Space, String> {
        self.check_layout_name(name, None)?;
        let source = self.layout(space).cloned().ok_or("There's no such layout.")?;
        // The tabs after it move along one.
        let mut after = self.layouts.clone();
        for l in after.iter_mut().filter(|l| l.tab_order > source.tab_order) {
            l.tab_order += 1;
        }
        self.set_layouts(after);
        let copy = crate::layout::Layout { name: name.trim().into(), tab_order: source.tab_order + 1, ..source };
        let id = self.add_layout(copy);
        let objects: Vec<Entity> = self.space_in_draw_order(space).into_iter().cloned().collect();
        let current = std::mem::replace(&mut self.space, id);
        for e in &objects {
            self.add_like(e.geometry.clone(), e);
        }
        self.space = current;
        Ok(id)
    }

    pub fn set_layouts(&mut self, layouts: Vec<crate::layout::Layout>) {
        let before = std::mem::replace(&mut self.layouts, layouts.clone());
        if before != layouts {
            self.record(Change::Layouts { before, after: layouts });
        }
    }

    /// The space being drawn in: model space, or a layout.
    pub fn space(&self) -> crate::layout::Space {
        self.space
    }

    /// Switches to model space or a layout, as the tabs do. Not a step to
    /// undo.
    pub fn set_space(&mut self, space: crate::layout::Space) {
        if space == crate::layout::MODEL || self.layout(space).is_some() {
            self.space = space;
        }
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    /// How many objects the space being drawn in has.
    pub fn len(&self) -> usize {
        self.entities().count()
    }

    pub fn is_empty(&self) -> bool {
        self.entities().next().is_none()
    }

    pub fn layers(&self) -> &[Layer] {
        &self.table.layers
    }

    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.table.layers.iter().find(|l| l.name.eq_ignore_ascii_case(name))
    }

    pub fn current_layer(&self) -> &str {
        &self.table.current
    }

    /// True if objects on `layer` are drawn (layer on and thawed).
    pub fn is_shown(&self, layer: &str) -> bool {
        self.layer(layer).is_none_or(Layer::is_shown)
    }

    /// True if objects on `layer` can be selected and edited.
    pub fn is_editable(&self, layer: &str) -> bool {
        self.layer(layer).is_none_or(|l| l.is_shown() && !l.locked)
    }

    /// True if the object can be selected and edited: its layer allows it
    /// and, while a reference is edited in place, it's in the working set.
    pub fn is_selectable(&self, e: &Entity) -> bool {
        self.is_editable(&e.layer) && self.refedit.as_ref().is_none_or(|r| r.in_working_set(e.id))
    }

    /// The block reference or xref being edited in place, if any.
    pub fn refedit(&self) -> Option<&RefEdit> {
        self.refedit.as_ref()
    }

    pub(crate) fn set_refedit(&mut self, refedit: Option<RefEdit>) {
        let before = std::mem::replace(&mut self.refedit, refedit.clone());
        self.record(Change::RefEdit { before: before.map(Box::new), after: refedit.map(Box::new) });
    }

    /// Number of objects on `layer`.
    pub fn layer_usage(&self, layer: &str) -> usize {
        self.entities.values().filter(|e| e.layer.eq_ignore_ascii_case(layer)).count()
    }

    /// The ACI color `color` resolves to on `layer`, where ByBlock means
    /// `by_block` (the enclosing reference's color; white outside blocks).
    fn color_on(&self, layer: &str, color: Color, by_block: u8) -> u8 {
        match color {
            Color::Index(i) => i,
            Color::ByLayer => self.layer(layer).map_or(7, |l| l.color),
            Color::ByBlock => by_block,
        }
    }

    /// The linetype name `name` resolves to on `layer`.
    fn linetype_on(&self, layer: &str, name: &str, by_block: &str) -> String {
        if linetype::is_by_layer(name) {
            self.layer(layer).map_or(CONTINUOUS, |l| &l.linetype).to_string()
        } else if linetype::is_by_block(name) {
            by_block.to_string()
        } else {
            name.to_string()
        }
    }

    /// The lineweight `lw` resolves to on `layer`.
    fn lineweight_on(&self, layer: &str, lw: Lineweight, by_block: Lineweight) -> Lineweight {
        match lw {
            Lineweight::ByLayer => self.layer(layer).map_or(Lineweight::Default, |l| l.lineweight),
            Lineweight::ByBlock => by_block,
            lw => lw,
        }
    }

    /// The definition of a linetype: the drawing's own, else a standard
    /// one. `None` for unknown names.
    pub fn linetype(&self, name: &str) -> Option<Linetype> {
        if name.eq_ignore_ascii_case(CONTINUOUS) {
            return Some(Linetype::continuous());
        }
        self.linetypes
            .get(&name.to_ascii_uppercase())
            .cloned()
            .or_else(|| linetype::standard().into_iter().find(|l| l.name.eq_ignore_ascii_case(name)))
    }

    /// Every linetype that can be used: Continuous, the drawing's own and
    /// the standard ones.
    pub fn linetypes(&self) -> Vec<Linetype> {
        let mut out = vec![Linetype::continuous()];
        out.extend(self.linetypes.values().cloned());
        for l in linetype::standard() {
            if !out.iter().any(|o| o.name.eq_ignore_ascii_case(&l.name)) {
                out.push(l);
            }
        }
        out
    }

    /// Adds or replaces a linetype definition (from a drawing file).
    pub fn define_linetype(&mut self, l: Linetype) {
        if !l.name.eq_ignore_ascii_case(CONTINUOUS) {
            self.linetypes.insert(l.name.to_ascii_uppercase(), l);
        }
    }

    /// The drawing's own linetypes (not Continuous or the standard ones it
    /// hasn't loaded).
    pub fn own_linetypes(&self) -> Vec<String> {
        self.linetypes.values().map(|l| l.name.clone()).collect()
    }

    /// RENAME a linetype: its definition, and the objects, layers and
    /// block contents that use it. Continuous, ByLayer and ByBlock can't
    /// be.
    pub fn rename_linetype(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        check_name(new)?;
        let special = |n: &str| [CONTINUOUS, "ByLayer", "ByBlock"].iter().any(|s| s.eq_ignore_ascii_case(n.trim()));
        if special(old) || special(new) {
            return Err(format!("\"{}\" can't be renamed.", old.trim()));
        }
        let lt = self.linetype(old.trim()).ok_or_else(|| format!("Linetype \"{}\" not found.", old.trim()))?;
        if !new.eq_ignore_ascii_case(&lt.name) && self.linetypes.contains_key(&new.to_ascii_uppercase()) {
            return Err(format!("A linetype named \"{new}\" already exists."));
        }
        let old = lt.name.clone();
        let old_key = old.to_ascii_uppercase();
        if let Some(before) = self.linetypes.remove(&old_key) {
            self.record(Change::Linetype { key: old_key, before: Some(before), after: None });
        }
        let renamed = Linetype { name: new.into(), ..lt };
        let key = new.to_ascii_uppercase();
        let before = self.linetypes.insert(key.clone(), renamed.clone());
        self.record(Change::Linetype { key, before, after: Some(renamed) });
        let is_old = |n: &str| n.eq_ignore_ascii_case(&old);
        let ids: Vec<EntityId> = self.entities.values().filter(|e| is_old(&e.linetype)).map(|e| e.id).collect();
        for id in ids {
            self.update(id, |e| e.linetype = new.to_string());
        }
        self.edit_layers(|layers| {
            layers.iter_mut().filter(|l| is_old(&l.linetype)).for_each(|l| l.linetype = new.into())
        });
        let blocks: Vec<Block> =
            self.blocks.values().filter(|b| b.entities.iter().any(|be| is_old(&be.linetype))).cloned().collect();
        for mut b in blocks {
            b.entities.iter_mut().filter(|be| is_old(&be.linetype)).for_each(|be| be.linetype = new.to_string());
            self.define_block(b);
        }
        Ok(())
    }

    /// Removes one of the drawing's linetypes (PURGE), undoably.
    pub fn remove_linetype(&mut self, name: &str) -> bool {
        let key = name.to_ascii_uppercase();
        let Some(before) = self.linetypes.remove(&key) else { return false };
        self.record(Change::Linetype { key, before: Some(before), after: None });
        true
    }

    /// LTSCALE.
    pub fn ltscale(&self) -> f64 {
        self.ltscale
    }

    pub fn set_ltscale(&mut self, scale: f64) {
        if scale > 0.0 && scale != self.ltscale {
            self.ltscale = scale;
            self.revision += 1;
        }
    }

    /// PDMODE: the point style. 0 is a dot, 1 nothing, 2 a plus, 3 an X,
    /// 4 a tick; add 32 for a circle around it and 64 for a square.
    pub fn pdmode(&self) -> i32 {
        self.pdmode
    }

    /// PDSIZE: point size in drawing units, or a percentage of the view
    /// height when negative; 0 is 5% of the view height.
    pub fn pdsize(&self) -> f64 {
        self.pdsize
    }

    pub fn set_point_style(&mut self, pdmode: i32, pdsize: f64) {
        if (pdmode, pdsize) != (self.pdmode, self.pdsize) {
            (self.pdmode, self.pdsize) = (pdmode, pdsize);
            self.revision += 1;
        }
    }

    /// INSBASE.
    pub fn insbase(&self) -> Point {
        self.insbase
    }

    pub fn set_insbase(&mut self, p: Point) {
        if p != self.insbase {
            self.insbase = p;
            self.revision += 1;
        }
    }

    /// ULAYERS: the layers of PDF underlay `name` turned off.
    pub fn pdf_layers_off(&self, name: &str) -> &[String] {
        self.pdf_layers_off.get(&name.to_ascii_uppercase()).map_or(&[], Vec::as_slice)
    }

    /// Every PDF underlay's layers turned off, by underlay name.
    pub fn all_pdf_layers_off(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.pdf_layers_off.iter()
    }

    pub fn set_pdf_layers_off(&mut self, name: &str, off: Vec<String>) {
        let key = name.to_ascii_uppercase();
        if off.is_empty() {
            self.pdf_layers_off.remove(&key);
        } else {
            self.pdf_layers_off.insert(key, off);
        }
        self.revision += 1;
    }

    /// The current UCS.
    pub fn ucs(&self) -> crate::ucs::Ucs {
        self.ucs
    }

    /// Makes `ucs` current; the one it replaces is UCS Previous's.
    pub fn set_ucs(&mut self, ucs: crate::ucs::Ucs) {
        if ucs != self.ucs {
            self.ucs_previous.push(self.ucs);
            if self.ucs_previous.len() > 10 {
                self.ucs_previous.remove(0);
            }
            self.ucs = ucs;
            self.revision += 1;
        }
    }

    /// UCS Previous: back to the UCS before; false if there's none.
    pub fn previous_ucs(&mut self) -> bool {
        let Some(ucs) = self.ucs_previous.pop() else { return false };
        self.ucs = ucs;
        self.revision += 1;
        true
    }

    /// The named UCSs, in the order saved.
    pub fn named_ucs(&self) -> &[crate::ucs::NamedUcs] {
        &self.named_ucs
    }

    /// Saves `ucs` as `name` (replacing one of that name).
    pub fn save_ucs(&mut self, name: &str, ucs: crate::ucs::Ucs) -> Result<(), String> {
        check_name(name)?;
        let name = name.trim().to_string();
        match self.named_ucs.iter_mut().find(|n| n.name.eq_ignore_ascii_case(&name)) {
            Some(n) => n.ucs = ucs,
            None => self.named_ucs.push(crate::ucs::NamedUcs { name, ucs }),
        }
        self.revision += 1;
        Ok(())
    }

    pub fn delete_ucs(&mut self, name: &str) -> bool {
        let before = self.named_ucs.len();
        self.named_ucs.retain(|n| !n.name.eq_ignore_ascii_case(name));
        self.revision += 1;
        self.named_ucs.len() != before
    }

    pub fn rename_ucs(&mut self, old: &str, new: &str) -> Result<(), String> {
        check_name(new)?;
        let new = new.trim().to_string();
        if !old.eq_ignore_ascii_case(&new) && self.named_ucs.iter().any(|n| n.name.eq_ignore_ascii_case(&new)) {
            return Err(format!("A UCS named \"{new}\" already exists."));
        }
        let n = self.named_ucs.iter_mut().find(|n| n.name.eq_ignore_ascii_case(old)).ok_or("No such UCS.")?;
        n.name = new;
        self.revision += 1;
        Ok(())
    }

    /// LIMMIN and LIMMAX.
    pub fn limits(&self) -> (Point, Point) {
        self.limits
    }

    pub fn set_limits(&mut self, min: Point, max: Point) {
        let limits = (Point::new(min.x.min(max.x), min.y.min(max.y)), Point::new(min.x.max(max.x), min.y.max(max.y)));
        if limits != self.limits {
            self.limits = limits;
            self.revision += 1;
        }
    }

    /// LIMCHECK.
    pub fn limits_check(&self) -> bool {
        self.limits_check
    }

    pub fn set_limits_check(&mut self, on: bool) {
        if on != self.limits_check {
            self.limits_check = on;
            self.revision += 1;
        }
    }

    /// True if LIMCHECK is on and `p` is outside the limits.
    pub fn outside_limits(&self, p: Point) -> bool {
        let (min, max) = self.limits;
        self.limits_check && (p.x < min.x - 1e-9 || p.y < min.y - 1e-9 || p.x > max.x + 1e-9 || p.y > max.y + 1e-9)
    }

    /// FILLMODE.
    pub fn fill(&self) -> bool {
        self.fill
    }

    pub fn set_fill(&mut self, on: bool) {
        if on != self.fill {
            self.fill = on;
            self.revision += 1;
        }
    }

    /// QTEXTMODE.
    pub fn quick_text(&self) -> bool {
        self.quick_text
    }

    pub fn set_quick_text(&mut self, on: bool) {
        if on != self.quick_text {
            self.quick_text = on;
            self.revision += 1;
        }
    }

    /// ATTDISP.
    pub fn attdisp(&self) -> AttributeDisplay {
        self.attdisp
    }

    pub fn set_attdisp(&mut self, mode: AttributeDisplay) {
        if mode != self.attdisp {
            self.attdisp = mode;
            self.revision += 1;
        }
    }

    /// True if ATTDISP draws this attribute.
    pub fn shows_attribute(&self, a: &Attribute) -> bool {
        let wanted = match self.attdisp {
            AttributeDisplay::Normal => !a.invisible,
            AttributeDisplay::On => true,
            AttributeDisplay::Off => false,
        };
        wanted && !a.value.is_empty() && self.is_shown(&a.layer)
    }

    /// Resolved ACI color of an entity (layer color when `ByLayer`).
    pub fn display_color(&self, e: &Entity) -> u8 {
        self.color_on(&e.layer, e.color, 7)
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.blocks.values()
    }

    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks.get(name).or_else(|| self.blocks.values().find(|b| b.name.eq_ignore_ascii_case(name)))
    }

    /// The entity's curves in world coordinates, with how each is drawn.
    /// Block references are expanded recursively; objects on layer "0"
    /// inside a block take the reference's layer, as in AutoCAD.
    pub fn expanded_styled(&self, e: &Entity) -> Vec<Styled> {
        let mut out = Vec::new();
        let style = (e.color, e.linetype.as_str(), e.lineweight);
        self.expand_into(&e.geometry, &e.layer, style, &Inherited::default(), 0, &mut out);
        out
    }

    /// The entity's curves in world coordinates (block references expanded).
    pub fn expanded(&self, e: &Entity) -> Vec<Geometry> {
        if !matches!(
            e.geometry,
            Geometry::Insert { .. }
                | Geometry::Dimension { .. }
                | Geometry::RadialDimension { .. }
                | Geometry::AngularDimension { .. }
                | Geometry::Leader { .. }
        ) {
            return vec![self.shown(&e.geometry)];
        }
        self.expanded_styled(e).into_iter().map(|s| s.geometry).collect()
    }

    /// `g` as it shows: text with its fields' values.
    pub fn shown(&self, g: &Geometry) -> Geometry {
        let mut g = g.clone();
        if let Geometry::Text { content, .. } = &mut g
            && crate::field::has_fields(content)
        {
            *content = crate::field::evaluate(content, self);
        }
        g
    }

    fn resolve(&self, layer: &str, style: (Color, &str, Lineweight), by_block: &Inherited) -> Inherited {
        Inherited {
            color: self.color_on(layer, style.0, by_block.color),
            linetype: self.linetype_on(layer, style.1, &by_block.linetype),
            lineweight: self.lineweight_on(layer, style.2, by_block.lineweight),
        }
    }

    fn expand_into(
        &self,
        g: &Geometry,
        layer: &str,
        style: (Color, &str, Lineweight),
        by_block: &Inherited,
        depth: usize,
        out: &mut Vec<Styled>,
    ) {
        let primitives = || crate::annotation::with_dim_style(self.dim_style, || g.primitives());
        if let Some(parts) = crate::annotation::with_leader_style(self.leader_style, primitives) {
            for part in &parts {
                self.expand_into(part, layer, style, by_block, depth + 1, out);
            }
            return;
        }
        // What ByBlock means for everything inside this reference.
        let own = self.resolve(layer, style, by_block);
        let Geometry::Insert { block, placement, attributes, clip } = g else {
            let Inherited { color, linetype, lineweight } = own;
            out.push(Styled { geometry: self.shown(g), color, layer: layer.to_string(), linetype, lineweight });
            return;
        };
        let start = out.len();
        if let Some(def) = self.block(block).filter(|_| depth < MAX_BLOCK_DEPTH) {
            for be in &def.entities {
                let inner_layer = if be.layer == "0" { layer } else { &be.layer };
                // Attribute definitions are the references' attributes; a
                // constant one shows its value as text.
                let geometry = match &be.geometry {
                    Geometry::AttributeDefinition { constant: false, .. } => continue,
                    Geometry::AttributeDefinition {
                        default,
                        position,
                        height,
                        rotation,
                        attach,
                        invisible: false,
                        ..
                    } => Geometry::Text {
                        look: Default::default(),
                        position: *position,
                        height: *height,
                        rotation: *rotation,
                        width: 0.0,
                        attach: *attach,
                        content: default.clone(),
                    },
                    Geometry::AttributeDefinition { .. } => continue,
                    g => g.clone(),
                };
                if self.is_shown(inner_layer) {
                    let placed = placement.place(def.base, &geometry);
                    let inner = (be.color, be.linetype.as_str(), be.lineweight);
                    self.expand_into(&placed, inner_layer, inner, &own, depth + 1, out);
                }
            }
        }
        for a in attributes.iter().filter(|a| self.shows_attribute(a)) {
            out.push(Styled {
                geometry: self.shown(&a.text()),
                color: self.color_on(&a.layer, a.color, own.color),
                layer: a.layer.clone(),
                linetype: CONTINUOUS.into(),
                lineweight: own.lineweight,
            });
        }
        // XCLIP: only what's inside the boundary (or outside it) shows.
        if let Some(c) = clip.as_deref().filter(|c| c.on)
            && let Some(boundary) = self.clip_boundary(g).filter(|b| b.len() >= 3)
        {
            let shown = out.split_off(start);
            out.extend(crate::clip::clip_styled(shown, &boundary, c.inverted));
        }
    }

    /// XCLIP's boundary of a clipped block reference, in world coordinates.
    pub fn clip_boundary(&self, g: &Geometry) -> Option<Vec<Point>> {
        let Geometry::Insert { block, placement, clip: Some(c), .. } = g else { return None };
        let base = self.block(block)?.base;
        Some(c.boundary.iter().map(|p| placement.apply(base, *p)).collect())
    }

    /// The extents of a block's contents in its own coordinates, ignoring
    /// layers that are off.
    pub fn block_extents(&self, name: &str) -> Option<Aabb> {
        self.block_contents(name).iter().map(|s| s.geometry.bounds()).reduce(Aabb::union)
    }

    /// A block's contents in its own coordinates, nested blocks expanded,
    /// as a reference on layer 0 would draw them (for previews).
    pub fn block_contents(&self, name: &str) -> Vec<Styled> {
        let Some(def) = self.block(name) else { return Vec::new() };
        let g = Geometry::Insert {
            block: def.name.clone(),
            placement: crate::Placement::at(def.base),
            attributes: Vec::new(),
            clip: None,
        };
        let mut out = Vec::new();
        let style = (Color::ByLayer, linetype::BYLAYER, Lineweight::ByLayer);
        self.expand_into(&g, "0", style, &Inherited::default(), 0, &mut out);
        out
    }

    /// The frame drawn around a clipped reference (XCLIPFRAME 2: on screen
    /// and pickable, but not plotted), while its clipping is on.
    pub fn clip_frame(&self, g: &Geometry) -> Option<Geometry> {
        let Geometry::Insert { clip: Some(c), .. } = g else { return None };
        let boundary = self.clip_boundary(g).filter(|b| c.on && b.len() >= 3)?;
        Some(Geometry::Polyline { vertices: boundary.into_iter().map(Vertex::at).collect(), closed: true })
    }

    /// Bounding box of an entity, block contents included.
    pub fn entity_bounds(&self, e: &Entity) -> Aabb {
        let mut parts = self.expanded(e);
        parts.extend(self.clip_frame(&e.geometry));
        parts.iter().map(Geometry::bounds).reduce(Aabb::union).unwrap_or_else(|| e.geometry.bounds())
    }

    /// Distance from `p` to the entity's curves (a block reference also
    /// counts its insertion point, so an empty block can still be picked).
    pub fn entity_distance(&self, e: &Entity, p: Point) -> f64 {
        let curves = self.expanded(e).iter().map(|g| g.distance_to(p)).fold(f64::INFINITY, f64::min);
        match &e.geometry {
            Geometry::Insert { placement, .. } => {
                let frame = self.clip_frame(&e.geometry).map_or(f64::INFINITY, |f| f.distance_to(p));
                curves.min(frame).min(placement.position.distance(p))
            }
            _ => curves,
        }
    }

    /// True if any of the entity's curves lies in the box (crossing selection).
    pub fn entity_touches(&self, e: &Entity, b: &Aabb) -> bool {
        self.expanded(e).iter().chain(&self.clip_frame(&e.geometry)).any(|g| g.touches(b))
    }

    /// Bounding box of all entities, or `None` for an empty drawing. Like
    /// AutoCAD's ZOOM Extents, objects on frozen layers don't count, and
    /// neither do construction lines and rays, which go on forever.
    pub fn extents(&self) -> Option<Aabb> {
        self.space_extents(self.space)
    }

    /// Like [`Document::extents`], for model space or a layout.
    pub fn space_extents(&self, space: crate::layout::Space) -> Option<Aabb> {
        self.entities
            .values()
            .filter(|e| e.space == space)
            .filter(|e| !self.layer(&e.layer).is_some_and(|l| l.frozen) && !e.geometry.is_infinite())
            .map(|e| self.entity_bounds(e))
            .reduce(Aabb::union)
    }

    // ---- layers ----------------------------------------------------------

    /// Replaces the layer table (one undoable change).
    fn set_table(&mut self, after: LayerTable) {
        if after == self.table {
            return;
        }
        let before = std::mem::replace(&mut self.table, after.clone());
        self.previous_layers.push(before.clone());
        if self.previous_layers.len() > 50 {
            self.previous_layers.remove(0);
        }
        self.record(Change::Layers { before, after });
    }

    /// LAYERP: the layer settings as they were before the last change to
    /// them (undoable). False if there's none to go back to.
    pub fn layer_previous(&mut self) -> bool {
        let Some(before) = self.previous_layers.pop() else { return false };
        let after = before;
        let now = std::mem::replace(&mut self.table, after.clone());
        self.record(Change::Layers { before: now, after });
        true
    }

    /// LAYMRG: everything on the `from` layers (blocks' contents and
    /// attributes too) goes onto `into`, and the `from` layers go. Returns
    /// how many objects moved.
    pub fn merge_layers(&mut self, from: &[String], into: &str) -> Result<usize, String> {
        if self.layer(into).is_none() {
            return Err(format!("Layer \"{into}\" not found."));
        }
        let is_from =
            |name: &str| from.iter().any(|f| f.eq_ignore_ascii_case(name)) && !name.eq_ignore_ascii_case(into);
        let ids: Vec<EntityId> = self.entities.values().map(|e| e.id).collect();
        let mut moved = 0;
        for id in ids {
            let into = into.to_string();
            let mut touched = false;
            self.update(id, |e| {
                if is_from(&e.layer) {
                    e.layer = into.clone();
                    touched = true;
                }
                if let Geometry::Insert { attributes, .. } = &mut e.geometry {
                    attributes.iter_mut().filter(|a| is_from(&a.layer)).for_each(|a| a.layer = into.clone());
                }
            });
            moved += usize::from(touched);
        }
        let blocks: Vec<Block> =
            self.blocks.values().filter(|b| b.entities.iter().any(|be| is_from(&be.layer))).cloned().collect();
        for mut b in blocks {
            b.entities.iter_mut().filter(|be| is_from(&be.layer)).for_each(|be| be.layer = into.to_string());
            self.define_block(b);
        }
        if from.iter().any(|f| f.eq_ignore_ascii_case(&self.table.current)) {
            self.set_current_layer(into);
        }
        for f in from.iter().filter(|f| !f.eq_ignore_ascii_case(into)) {
            let _ = self.delete_layer(f);
        }
        Ok(moved)
    }

    /// LAYDEL: a layer and everything on it (in blocks too) go. Layer 0,
    /// the current layer and external references' layers stay.
    pub fn delete_layer_and_objects(&mut self, name: &str) -> Result<usize, String> {
        let layer = self.layer(name).cloned().ok_or_else(|| format!("Layer \"{name}\" not found."))?;
        if layer.name == "0" || layer.name.eq_ignore_ascii_case(&self.table.current) || is_dependent(&layer.name) {
            return Err(format!("Layer \"{}\" can't be deleted.", layer.name));
        }
        let on = |n: &str| n.eq_ignore_ascii_case(&layer.name);
        let ids: Vec<EntityId> = self.entities.values().filter(|e| on(&e.layer)).map(|e| e.id).collect();
        for id in &ids {
            self.remove(*id);
        }
        let blocks: Vec<Block> =
            self.blocks.values().filter(|b| b.entities.iter().any(|be| on(&be.layer))).cloned().collect();
        for mut b in blocks {
            b.entities.retain(|be| !on(&be.layer));
            self.define_block(b);
        }
        let others: Vec<EntityId> = self.entities.values().map(|e| e.id).collect();
        for id in others {
            self.update(id, |e| {
                if let Geometry::Insert { attributes, .. } = &mut e.geometry {
                    attributes.retain(|a| !on(&a.layer));
                }
            });
        }
        self.delete_layer(&layer.name)?;
        Ok(ids.len())
    }

    /// Adds a layer, or updates the color of an existing one with the same name.
    pub fn add_layer(&mut self, layer: Layer) {
        let mut table = self.table.clone();
        match table.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&layer.name)) {
            Some(existing) => existing.color = layer.color,
            None => table.layers.push(layer),
        }
        self.set_table(table);
    }

    /// Updates a layer's color and states (not its name; see [`Document::rename_layer`]).
    pub fn update_layer(&mut self, layer: Layer) -> bool {
        let mut table = self.table.clone();
        let Some(existing) = table.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&layer.name)) else {
            return false;
        };
        *existing = Layer { name: existing.name.clone(), ..layer };
        self.set_table(table);
        true
    }

    /// Makes `name` the layer new entities go on. Returns false if it doesn't exist.
    pub fn set_current_layer(&mut self, name: &str) -> bool {
        // An xref's layers can't take new objects.
        let Some(l) = self.layer(name).filter(|l| !is_dependent(&l.name)) else { return false };
        let table = LayerTable { current: l.name.clone(), ..self.table.clone() };
        self.set_table(table);
        true
    }

    /// Renames a layer and moves its objects with it. Layer "0" can't be renamed.
    pub fn rename_layer(&mut self, old: &str, new: &str) -> Result<(), String> {
        check_name(new)?;
        let Some(layer) = self.layer(old).cloned() else { return Err(format!("Layer \"{old}\" not found.")) };
        if layer.name == "0" {
            return Err("Layer 0 can't be renamed.".into());
        }
        if is_dependent(&layer.name) {
            return Err(format!("\"{}\" comes from an external reference and can't be renamed.", layer.name));
        }
        if !new.eq_ignore_ascii_case(old) && self.layer(new).is_some() {
            return Err(format!("A layer named \"{new}\" already exists."));
        }
        let mut table = self.table.clone();
        for l in &mut table.layers {
            if l.name == layer.name {
                l.name = new.to_string();
            }
        }
        if table.current == layer.name {
            table.current = new.to_string();
        }
        self.set_table(table);
        let ids: Vec<EntityId> = self.entities.values().filter(|e| e.layer == layer.name).map(|e| e.id).collect();
        for id in ids {
            self.update(id, |e| e.layer = new.to_string());
        }
        Ok(())
    }

    /// Deletes an empty layer. Layer "0" and the current layer can't be deleted.
    pub fn delete_layer(&mut self, name: &str) -> Result<(), String> {
        let Some(layer) = self.layer(name).cloned() else { return Err(format!("Layer \"{name}\" not found.")) };
        if layer.name == "0" {
            return Err("Layer 0 can't be deleted.".into());
        }
        if is_dependent(&layer.name) {
            return Err(format!("\"{}\" comes from an external reference; detach it instead.", layer.name));
        }
        if layer.name == self.table.current {
            return Err("The current layer can't be deleted.".into());
        }
        let used = self.layer_usage(&layer.name)
            + self.blocks.values().flat_map(|b| &b.entities).filter(|be| be.layer == layer.name).count();
        if used > 0 {
            return Err(format!("Layer \"{}\" has {used} objects on it.", layer.name));
        }
        let mut table = self.table.clone();
        table.layers.retain(|l| l.name != layer.name);
        self.set_table(table);
        Ok(())
    }

    /// A layer name like "Layer1" that isn't taken yet.
    pub fn unused_layer_name(&self) -> String {
        (1..).map(|i| format!("Layer{i}")).find(|n| self.layer(n).is_none()).unwrap_or_default()
    }

    // ---- blocks ----------------------------------------------------------

    /// Adds or replaces a block definition.
    pub fn define_block(&mut self, block: Block) {
        let name = block.name.clone();
        let before = self.blocks.insert(name.clone(), block.clone());
        self.record(Change::Block { name, before, after: Some(block) });
    }

    /// RENAME a block: its definition and every reference to it, nested
    /// ones too. Anonymous blocks and external references' can't be.
    pub fn rename_block(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        check_name(new)?;
        let block = self.block(old.trim()).cloned().ok_or_else(|| format!("Block \"{}\" not found.", old.trim()))?;
        if block.name.starts_with('*') || is_dependent(&block.name) || self.xref(&block.name).is_some() {
            return Err(format!("\"{}\" can't be renamed.", block.name));
        }
        if !new.eq_ignore_ascii_case(&block.name) && self.block(new).is_some() {
            return Err(format!("A block named \"{new}\" already exists."));
        }
        let old = block.name.clone();
        let retarget = |g: &mut Geometry| {
            if let Geometry::Insert { block, .. } = g
                && block.eq_ignore_ascii_case(&old)
            {
                *block = new.to_string();
            }
        };
        self.remove_block(&old);
        self.define_block(Block { name: new.into(), ..block });
        let ids: Vec<EntityId> = self
            .entities
            .values()
            .filter(|e| matches!(&e.geometry, Geometry::Insert { block, .. } if block.eq_ignore_ascii_case(&old)))
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.update(id, |e| retarget(&mut e.geometry));
        }
        let nesting: Vec<Block> = self
            .blocks
            .values()
            .filter(|b| {
                b.entities.iter().any(
                    |be| matches!(&be.geometry, Geometry::Insert { block, .. } if block.eq_ignore_ascii_case(&old)),
                )
            })
            .cloned()
            .collect();
        for mut b in nesting {
            b.entities.iter_mut().for_each(|be| retarget(&mut be.geometry));
            self.define_block(b);
        }
        Ok(())
    }

    /// Removes a block definition (whatever still references it draws nothing).
    pub fn remove_block(&mut self, name: &str) -> Option<Block> {
        let key = self.block(name)?.name.clone();
        let before = self.blocks.remove(&key)?;
        self.record(Change::Block { name: key, before: Some(before.clone()), after: None });
        Some(before)
    }

    /// Changes the layer table in one undoable step (for xrefs, which bring
    /// many layers along).
    pub(crate) fn edit_layers(&mut self, f: impl FnOnce(&mut Vec<Layer>)) {
        let mut table = self.table.clone();
        f(&mut table.layers);
        self.set_table(table);
    }

    // ---- external references -----------------------------------------------

    /// The drawings attached to this one.
    pub fn xrefs(&self) -> impl Iterator<Item = &Xref> {
        self.xrefs.values()
    }

    pub fn xref(&self, name: &str) -> Option<&Xref> {
        self.xrefs.get(&name.to_ascii_uppercase())
    }

    /// Adds or replaces an attached drawing's record.
    pub fn set_xref(&mut self, xref: Xref) {
        let key = xref.name.to_ascii_uppercase();
        let before = self.xrefs.insert(key.clone(), xref.clone());
        if before.as_ref() != Some(&xref) {
            self.record(Change::Xref { key, before, after: Some(xref) });
        }
    }

    /// The images and PDF pages attached to this drawing.
    pub fn image_defs(&self) -> impl Iterator<Item = &ImageDef> {
        self.images.values()
    }

    pub fn image_def(&self, name: &str) -> Option<&ImageDef> {
        self.images.get(&name.to_ascii_uppercase())
    }

    /// Adds or replaces an image or PDF underlay's definition.
    pub fn set_image_def(&mut self, def: ImageDef) {
        let key = def.name.to_ascii_uppercase();
        let before = self.images.insert(key.clone(), def.clone());
        if before.as_ref() != Some(&def) {
            self.record(Change::Image { key, before, after: Some(def) });
        }
    }

    /// Marks an image or PDF underlay loaded, unloaded or not found. As in
    /// AutoCAD, loading isn't a step to undo, but the drawing has changed:
    /// its file keeps which images are loaded.
    pub fn set_image_status(&mut self, name: &str, status: crate::XrefStatus) {
        if let Some(def) = self.images.get_mut(&name.to_ascii_uppercase())
            && def.status != status
        {
            def.status = status;
            self.revision += 1;
        }
    }

    pub(crate) fn remove_image_def(&mut self, name: &str) -> Option<ImageDef> {
        let key = name.to_ascii_uppercase();
        let before = self.images.remove(&key)?;
        self.record(Change::Image { key, before: Some(before.clone()), after: None });
        Some(before)
    }

    pub(crate) fn remove_xref(&mut self, name: &str) -> Option<Xref> {
        let key = name.to_ascii_uppercase();
        let before = self.xrefs.remove(&key)?;
        self.record(Change::Xref { key, before: Some(before.clone()), after: None });
        Some(before)
    }

    // ---- edits ---------------------------------------------------------

    /// Adds an entity on the current layer with the current color,
    /// linetype, lineweight and transparency.
    pub fn add(&mut self, geometry: Geometry) -> EntityId {
        let layer = self.table.current.clone();
        let CurrentProps { color, linetype, lineweight, transparency } = self.current.clone();
        self.add_with(geometry, &layer, color, &linetype, lineweight, transparency)
    }

    /// Adds an entity on a specific layer (created with color 7 if missing).
    pub fn add_on_layer(&mut self, geometry: Geometry, layer: &str, color: Color) -> EntityId {
        self.add_styled(geometry, layer, color, linetype::BYLAYER, Lineweight::ByLayer)
    }

    /// Adds an entity with all its properties (the layer is created with
    /// color 7 if missing).
    pub fn add_styled(
        &mut self,
        geometry: Geometry,
        layer: &str,
        color: Color,
        linetype: &str,
        lineweight: Lineweight,
    ) -> EntityId {
        self.add_with(geometry, layer, color, linetype, lineweight, crate::linetype::Transparency::ByLayer)
    }

    /// [`Document::add_styled`], with a transparency too.
    pub fn add_with(
        &mut self,
        geometry: Geometry,
        layer: &str,
        color: Color,
        linetype: &str,
        lineweight: Lineweight,
        transparency: crate::linetype::Transparency,
    ) -> EntityId {
        let layer = match self.layer(layer) {
            Some(l) => l.name.clone(),
            None => {
                self.add_layer(Layer::new(layer, 7));
                layer.to_string()
            }
        };
        let entity = Entity {
            annotative: None,
            id: EntityId(0),
            space: self.space,
            layer,
            color,
            linetype: linetype.to_string(),
            lineweight,
            transparency,
            hyperlink: None,
            array: None,
            geometry,
        };
        self.insert_entity(entity)
    }

    /// Adds `entity` as a new object (its id is given here), in the space
    /// being drawn in.
    fn insert_entity(&mut self, mut entity: Entity) -> EntityId {
        let id = EntityId(self.next_id);
        self.next_id += 1;
        (entity.id, entity.space) = (id, self.space);
        // Something was brought to the front: new objects go above it, in
        // the same undo step.
        let over_top = self.top >= id.0 as f64;
        let own = over_top && self.open.is_none();
        if own {
            self.begin("Add");
        }
        self.entities.insert(id, entity.clone());
        self.record(Change::Entity { id, before: None, after: Some(entity) });
        if over_top {
            self.set_draw_key(id, self.top + 1.0);
        }
        if own {
            self.commit();
        }
        id
    }

    // ---- draw order ------------------------------------------------------

    /// Where an object is in the draw order: higher draws over lower.
    pub fn draw_key(&self, id: EntityId) -> f64 {
        self.order.get(&id).copied().unwrap_or(id.0 as f64)
    }

    /// The objects from the back to the front.
    /// The objects of the space being drawn in, in draw order.
    pub fn entities_in_draw_order(&self) -> Vec<&Entity> {
        self.space_in_draw_order(self.space)
    }

    fn set_draw_key(&mut self, id: EntityId, key: f64) {
        let before = self.order.insert(id, key);
        self.top = self.top.max(key);
        self.record(Change::Order { id, before, after: Some(key) });
    }

    /// `ids` from the back to the front, as they draw now.
    fn in_draw_order(&self, ids: &[EntityId]) -> Vec<EntityId> {
        let mut ids: Vec<EntityId> = ids.iter().copied().filter(|id| self.entities.contains_key(id)).collect();
        ids.sort_by(|a, b| self.draw_key(*a).total_cmp(&self.draw_key(*b)));
        ids.dedup();
        ids
    }

    /// Gives `ids` keys spread evenly between `low` and `high`, keeping
    /// their order among themselves.
    fn spread(&mut self, ids: &[EntityId], low: f64, high: f64) {
        let ids = self.in_draw_order(ids);
        let step = (high - low) / (ids.len() + 1) as f64;
        for (i, id) in ids.into_iter().enumerate() {
            self.set_draw_key(id, low + step * (i + 1) as f64);
        }
    }

    /// The lowest and highest keys among objects other than `except`.
    fn key_range(&self, except: &[EntityId]) -> Option<(f64, f64)> {
        let keys = self.entities.keys().filter(|id| !except.contains(id)).map(|id| self.draw_key(*id));
        keys.fold(None, |r, k| Some(r.map_or((k, k), |(lo, hi): (f64, f64)| (lo.min(k), hi.max(k)))))
    }

    /// DRAWORDER Front: `ids` over everything else.
    pub fn bring_to_front(&mut self, ids: &[EntityId]) {
        let top = self.key_range(ids).map_or(0.0, |r| r.1).max(self.top);
        self.spread(ids, top, top + ids.len() as f64 + 1.0);
    }

    /// DRAWORDER Back: `ids` under everything else.
    pub fn send_to_back(&mut self, ids: &[EntityId]) {
        let bottom = self.key_range(ids).map_or(0.0, |r| r.0);
        self.spread(ids, bottom - ids.len() as f64 - 1.0, bottom);
    }

    /// DRAWORDER Above objects: `ids` just over the topmost of `reference`.
    pub fn place_above(&mut self, ids: &[EntityId], reference: &[EntityId]) {
        let Some(low) = reference.iter().map(|id| self.draw_key(*id)).reduce(f64::max) else { return };
        let high = self
            .entities
            .keys()
            .filter(|id| !ids.contains(id))
            .map(|id| self.draw_key(*id))
            .filter(|k| *k > low)
            .reduce(f64::min)
            .unwrap_or(low + ids.len() as f64 + 1.0);
        self.spread(ids, low, high);
    }

    /// DRAWORDER Under objects: `ids` just under the lowest of `reference`.
    pub fn place_under(&mut self, ids: &[EntityId], reference: &[EntityId]) {
        let Some(high) = reference.iter().map(|id| self.draw_key(*id)).reduce(f64::min) else { return };
        let low = self
            .entities
            .keys()
            .filter(|id| !ids.contains(id))
            .map(|id| self.draw_key(*id))
            .filter(|k| *k < high)
            .reduce(f64::max)
            .unwrap_or(high - ids.len() as f64 - 1.0);
        self.spread(ids, low, high);
    }

    /// New hatches go behind the drawing they fill, though not behind
    /// images (AutoCAD's HPDRAWORDER "Behind boundary", near enough).
    pub fn send_behind_drawing(&mut self, ids: &[EntityId]) {
        let others = || self.entities.values().filter(|e| !ids.contains(&e.id));
        let images_top = others()
            .filter(|e| matches!(e.geometry, Geometry::Image { .. }))
            .map(|e| self.draw_key(e.id))
            .reduce(f64::max);
        let bottom = others()
            .filter(|e| !matches!(e.geometry, Geometry::Image { .. }))
            .map(|e| self.draw_key(e.id))
            .reduce(f64::min);
        match (images_top, bottom) {
            (_, None) => {}
            (Some(low), Some(high)) if low < high => self.spread(ids, low, high),
            (_, Some(high)) => self.spread(ids, high - ids.len() as f64 - 1.0, high),
        }
    }

    /// Puts the objects in this draw order, back to front (a file's).
    pub fn set_draw_order(&mut self, back_to_front: &[EntityId]) {
        let in_id_order = back_to_front.windows(2).all(|w| w[0] < w[1]);
        if in_id_order && self.order.is_empty() {
            return;
        }
        for (i, id) in back_to_front.iter().enumerate() {
            self.set_draw_key(*id, i as f64 + 1.0);
        }
    }

    /// Adds an entity with the layer, color, linetype and lineweight of
    /// `like` (copies, offsets, pieces of a broken object).
    pub fn add_like(&mut self, geometry: Geometry, like: &Entity) -> EntityId {
        if self.layer(&like.layer).is_none() {
            self.add_layer(Layer::new(like.layer.clone(), 7));
        }
        self.insert_entity(Entity { geometry, ..like.clone() })
    }

    /// Removes an entity. Returns false if it doesn't exist.
    pub fn remove(&mut self, id: EntityId) -> bool {
        match self.entities.remove(&id) {
            Some(e) => {
                self.record(Change::Entity { id, before: Some(e), after: None });
                true
            }
            None => false,
        }
    }

    /// Changes an entity in place (geometry, layer or color). Returns false if it doesn't exist.
    pub fn update(&mut self, id: EntityId, f: impl FnOnce(&mut Entity)) -> bool {
        let Some(e) = self.entities.get_mut(&id) else { return false };
        let before = e.clone();
        f(e);
        e.id = id;
        if *e == before {
            return true;
        }
        let after = e.clone();
        self.record(Change::Entity { id, before: Some(before), after: Some(after) });
        true
    }

    /// Replaces an entity's geometry. Returns false if it doesn't exist.
    pub fn modify(&mut self, id: EntityId, geometry: Geometry) -> bool {
        self.update(id, |e| e.geometry = geometry)
    }

    // ---- transactions & undo ------------------------------------------

    /// Starts grouping edits into one undo step. Commits any transaction already open.
    pub fn begin(&mut self, name: impl Into<String>) {
        self.commit();
        self.open = Some(Transaction { name: name.into(), changes: Vec::new() });
    }

    /// Ends the open transaction. Empty transactions leave no undo step.
    pub fn commit(&mut self) {
        // Dimensions follow the objects they measure, in the same step.
        if !self.dim_assoc.is_empty()
            && let Some(t) = &self.open
        {
            let changed: std::collections::HashSet<EntityId> = t
                .changes
                .iter()
                .filter_map(|c| if let Change::Entity { id, .. } = c { Some(*id) } else { None })
                .collect();
            if !changed.is_empty() {
                self.follow_associations(&changed);
            }
        }
        if let Some(t) = self.open.take()
            && !t.changes.is_empty()
        {
            self.undo_stack.push(t);
            self.redo_stack.clear();
        }
    }

    /// Reverts every edit made since [`Document::begin`].
    pub fn rollback(&mut self) {
        if let Some(t) = self.open.take() {
            self.revert(&t);
        }
    }

    /// The objects the open transaction added.
    pub fn added_in_transaction(&self) -> Vec<EntityId> {
        self.open.as_ref().map_or_else(Vec::new, |t| {
            t.changes
                .iter()
                .filter_map(|c| match c {
                    Change::Entity { id, before: None, after: Some(_) } => Some(*id),
                    _ => None,
                })
                .collect()
        })
    }

    /// How many changes the open transaction holds: a command's own Undo
    /// takes back to a count with [`Document::undo_last_in_transaction`].
    pub fn changes_in_transaction(&self) -> usize {
        self.open.as_ref().map_or(0, |t| t.changes.len())
    }

    /// Reverts the last edit in the open transaction but keeps it open, so
    /// later edits still group with it.
    pub fn undo_last_in_transaction(&mut self) -> bool {
        let Some(change) = self.open.as_mut().and_then(|t| t.changes.pop()) else {
            return false;
        };
        self.apply(&change, false);
        // An object's place in the draw order goes with the object.
        if let Change::Order { id, .. } = change
            && let Some(t) = self.open.as_mut()
            && matches!(t.changes.last(), Some(Change::Entity { id: added, before: None, .. }) if *added == id)
            && let Some(add) = t.changes.pop()
        {
            self.apply(&add, false);
        }
        true
    }

    /// Undoes the last committed step and returns its name.
    pub fn undo(&mut self) -> Option<String> {
        self.commit();
        let t = self.undo_stack.pop()?;
        self.revert(&t);
        let name = t.name.clone();
        self.redo_stack.push(t);
        Some(name)
    }

    /// Redoes the last undone step and returns its name.
    pub fn redo(&mut self) -> Option<String> {
        self.commit();
        let t = self.redo_stack.pop()?;
        for c in &t.changes {
            self.apply(c, true);
        }
        let name = t.name.clone();
        self.undo_stack.push(t);
        Some(name)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty() || self.open.as_ref().is_some_and(|t| !t.changes.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Name of the step UNDO would revert ("LINE", "Erase", …).
    pub fn undo_name(&self) -> Option<&str> {
        self.undo_stack.last().map(|t| t.name.as_str())
    }

    pub fn redo_name(&self) -> Option<&str> {
        self.redo_stack.last().map(|t| t.name.as_str())
    }

    /// Drops all undo/redo history, e.g. right after opening a file.
    pub fn forget_history(&mut self) {
        self.open = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub(crate) fn record(&mut self, change: Change) {
        self.revision += 1;
        match &mut self.open {
            Some(t) => t.changes.push(change),
            None => {
                let name = match &change {
                    Change::Entity { before: None, .. } => "Add",
                    Change::Entity { after: None, .. } => "Erase",
                    Change::Entity { .. } => "Modify",
                    Change::Layers { .. } => "Layer",
                    Change::Block { .. } => "Block",
                    Change::Xref { .. } => "Xref",
                    Change::Image { .. } => "Image",
                    Change::Order { .. } => "Draw order",
                    Change::RefEdit { .. } => "Reference edit",
                    Change::Layouts { .. } => "Layout",
                    Change::Views { .. } => "View",
                    Change::Linetype { .. } => "Linetype",
                    Change::Groups { .. } => "Group",
                    Change::DimStyle { .. } => "Dimension style",
                    Change::Units { .. } => "Units",
                    Change::Current { .. } => "Current properties",
                    Change::TextStyles { .. } => "Text style",
                    Change::AnnotationStyles { .. } => "Style",
                    Change::LayerStates { .. } => "Layer state",
                    Change::Properties { .. } => "Drawing properties",
                    Change::DimAssoc { .. } => "Dimension association",
                };
                self.undo_stack.push(Transaction { name: name.into(), changes: vec![change] });
                self.redo_stack.clear();
            }
        }
    }

    fn revert(&mut self, t: &Transaction) {
        for c in t.changes.iter().rev() {
            self.apply(c, false);
        }
    }

    /// Applies a change forwards (redo) or backwards (undo).
    fn apply(&mut self, c: &Change, forward: bool) {
        self.revision += 1;
        match c {
            Change::Entity { id, before, after } => {
                match if forward { after } else { before } {
                    Some(e) => self.entities.insert(*id, e.clone()),
                    None => self.entities.remove(id),
                };
            }
            Change::Layers { before, after } => self.table = if forward { after } else { before }.clone(),
            Change::Block { name, before, after } => {
                match if forward { after } else { before } {
                    Some(b) => self.blocks.insert(name.clone(), b.clone()),
                    None => self.blocks.remove(name),
                };
            }
            Change::Xref { key, before, after } => {
                match if forward { after } else { before } {
                    Some(x) => self.xrefs.insert(key.clone(), x.clone()),
                    None => self.xrefs.remove(key),
                };
            }
            Change::Image { key, before, after } => {
                match if forward { after } else { before } {
                    Some(d) => self.images.insert(key.clone(), d.clone()),
                    None => self.images.remove(key),
                };
            }
            Change::Order { id, before, after } => {
                match if forward { after } else { before } {
                    Some(k) => self.order.insert(*id, *k),
                    None => self.order.remove(id),
                };
            }
            Change::RefEdit { before, after } => {
                self.refedit = if forward { after } else { before }.as_deref().cloned()
            }
            Change::Views { before, after } => self.views = if forward { after } else { before }.clone(),
            Change::Groups { before, after } => self.groups = if forward { after } else { before }.clone(),
            Change::DimStyle { before, after } => self.dim_style = if forward { *after } else { *before },
            Change::Units { before, after } => self.units = if forward { *after } else { *before },
            Change::Current { before, after } => self.current = if forward { after } else { before }.clone(),
            Change::Properties { before, after } => self.properties = if forward { after } else { before }.clone(),
            Change::DimAssoc { id, before, after } => {
                match if forward { after } else { before } {
                    Some(a) => self.dim_assoc.insert(*id, a.clone()),
                    None => self.dim_assoc.remove(id),
                };
            }
            Change::LayerStates { before, after } => {
                self.layer_states = if forward { after } else { before }.clone();
            }
            Change::AnnotationStyles { before, after } => {
                (self.leader_style, self.table_style) = if forward { *after } else { *before };
            }
            Change::TextStyles { before, after } => {
                (self.text_styles, self.current_text_style) = if forward { after } else { before }.clone();
            }
            Change::Linetype { key, before, after } => {
                match if forward { after } else { before } {
                    Some(l) => self.linetypes.insert(key.clone(), l.clone()),
                    None => self.linetypes.remove(key),
                };
            }
            Change::Layouts { before, after } => {
                self.layouts = if forward { after } else { before }.clone();
                // Undoing a new layout while drawing in it: back to model space.
                if self.layout(self.space).is_none() {
                    self.space = crate::layout::MODEL;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Placement;
    use crate::layout::{MODEL, Paper};

    fn line(x: f64) -> Geometry {
        Geometry::Line { start: Point::new(x, 0.0), end: Point::new(x, 1.0) }
    }

    fn draw_order(d: &Document) -> Vec<u64> {
        d.entities_in_draw_order().iter().map(|e| e.id.0).collect()
    }

    #[test]
    fn layouts_are_made_renamed_copied_and_deleted_with_their_sheets() {
        let mut d = Document::new();
        d.add(line(0.0));
        assert_eq!(d.next_layout_name(), "Layout1");
        d.begin("LAYOUT");
        let one = d.new_layout("Layout1").unwrap();
        d.commit();
        let two = d.new_layout("Plan").unwrap();
        assert_eq!(d.next_layout_name(), "Layout2");
        assert!(d.new_layout("plan").is_err(), "names are unique, ignoring case");
        assert!(d.new_layout("Model").is_err() && d.new_layout("a:b").is_err());
        d.set_space(one);
        d.add(line(1.0));
        d.add(line(2.0));

        // A copy goes just after its layout, with what's on its sheet.
        let copy = d.copy_layout(one, "Layout1 (2)").unwrap();
        let tabs: Vec<&str> = d.layouts().iter().map(|l| l.name.as_str()).collect();
        assert_eq!(tabs, ["Layout1", "Layout1 (2)", "Plan"]);
        assert_eq!(d.space(), one, "copying doesn't switch");
        assert_eq!(d.all_entities().filter(|e| e.space == copy).count(), 2);

        // Moved before the first tab, then to the end.
        d.move_layout(two, Some(one));
        let tabs: Vec<&str> = d.layouts().iter().map(|l| l.name.as_str()).collect();
        assert_eq!(tabs, ["Plan", "Layout1", "Layout1 (2)"]);
        d.move_layout(two, None);

        assert!(d.rename_layout(two, "Layout1").is_err());
        d.rename_layout(two, "Site").unwrap();
        assert_eq!(d.layout(two).unwrap().name, "Site");

        // Deleting the one drawn in: its objects go, and so back to model space.
        d.begin("LAYOUT");
        assert!(d.delete_layout(one));
        d.commit();
        assert_eq!(d.space(), MODEL);
        assert!(d.layout(one).is_none() && d.all_entities().all(|e| e.space != one));
        d.undo();
        assert_eq!(d.all_entities().filter(|e| e.space == one).count(), 2);
        assert!(d.layout(one).is_some());
    }

    #[test]
    fn a_new_paper_changes_a_page_setup_from_a_file_too() {
        let raw = vec![(4, "Letter".to_string()), (44, "215.9".into()), (45, "279.4".into()), (73, "1".into())];
        let p = Paper { raw, ..Paper::a4_landscape() }.resized("ISO A3", 420.0, 297.0, [5.0; 4], 0);
        assert_eq!((p.width, p.height, p.rotation), (420.0, 297.0, 0));
        let value = |code: i32| p.raw.iter().find(|r| r.0 == code).unwrap().1.clone();
        assert_eq!((value(4), value(44), value(73)), ("ISO A3".into(), "420.000000".into(), "0".into()));
    }

    #[test]
    fn draw_order_moves_objects_and_undoes() {
        let mut d = Document::new();
        let ids: Vec<EntityId> = (0..4).map(|i| d.add(line(f64::from(i)))).collect();
        assert_eq!(draw_order(&d), vec![1, 2, 3, 4]);
        d.send_to_back(&[ids[2]]);
        assert_eq!(draw_order(&d), vec![3, 1, 2, 4]);
        d.bring_to_front(&[ids[0], ids[1]]);
        assert_eq!(draw_order(&d), vec![3, 4, 1, 2], "moved together, keeping their order");
        d.place_under(&[ids[3]], &[ids[2]]);
        assert_eq!(draw_order(&d), vec![4, 3, 1, 2]);
        d.place_above(&[ids[3]], &[ids[0]]);
        assert_eq!(draw_order(&d), vec![3, 1, 4, 2]);
        // Something new goes on top, even of what was brought to the front.
        let new = d.add(line(9.0));
        assert_eq!(*draw_order(&d).last().unwrap(), new.0);
        d.undo();
        d.undo();
        assert_eq!(draw_order(&d), vec![4, 3, 1, 2]);
        // A new hatch goes behind lines, but over images.
        let image = d.add(Geometry::Image {
            adjust: Default::default(),
            image: "ortho".into(),
            origin: Point::ORIGIN,
            u: Point::new(1.0, 0.0),
            v: Point::new(0.0, 1.0),
            clip: Vec::new(),
        });
        d.send_to_back(&[image]);
        let hatch = d.add(line(5.0));
        d.send_behind_drawing(&[hatch]);
        assert_eq!(draw_order(&d)[..2], [image.0, hatch.0]);
    }

    #[test]
    fn transaction_undoes_as_one_step() {
        let mut d = Document::new();
        d.begin("LINE");
        d.add(line(0.0));
        d.add(line(1.0));
        d.commit();
        assert_eq!(d.len(), 2);
        assert_eq!(d.undo().as_deref(), Some("LINE"));
        assert!(d.is_empty());
        assert_eq!(d.redo().as_deref(), Some("LINE"));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn rollback_discards_open_edits() {
        let mut d = Document::new();
        d.add(line(0.0));
        d.begin("LINE");
        d.add(line(1.0));
        d.rollback();
        assert_eq!(d.len(), 1);
        assert!(!d.can_redo());
    }

    #[test]
    fn erase_is_undoable_and_new_edit_clears_redo() {
        let mut d = Document::new();
        let id = d.add(line(0.0));
        assert!(d.remove(id));
        assert!(d.is_empty());
        d.undo();
        assert_eq!(d.len(), 1);
        d.add(line(2.0));
        assert!(!d.can_redo());
    }

    #[test]
    fn modify_is_undoable() {
        let mut d = Document::new();
        let id = d.add(line(0.0));
        assert!(d.modify(id, line(5.0)));
        assert_eq!(d.entity(id).unwrap().geometry, line(5.0));
        d.undo();
        assert_eq!(d.entity(id).unwrap().geometry, line(0.0));
        d.redo();
        assert_eq!(d.entity(id).unwrap().geometry, line(5.0));
    }

    #[test]
    fn bylayer_color_resolves_from_layer() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Walls", 1));
        assert!(d.set_current_layer("walls"));
        let id = d.add(line(0.0));
        let e = d.entity(id).unwrap();
        assert_eq!(e.layer, "Walls");
        assert_eq!(d.display_color(e), 1);
    }

    #[test]
    fn layer_changes_are_undoable() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Walls", 1));
        d.update_layer(Layer { locked: true, ..Layer::new("walls", 3) });
        assert!(d.layer("Walls").unwrap().locked);
        assert_eq!(d.layer("Walls").unwrap().color, 3);
        d.undo();
        assert!(!d.layer("Walls").unwrap().locked);
        d.undo();
        assert!(d.layer("Walls").is_none());
    }

    #[test]
    fn rename_moves_objects_and_undoes_in_one_step() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Walls", 1));
        d.set_current_layer("Walls");
        let id = d.add(line(0.0));
        d.begin("LAYER");
        d.rename_layer("Walls", "A-Wall").unwrap();
        d.commit();
        assert_eq!(d.entity(id).unwrap().layer, "A-Wall");
        assert_eq!(d.current_layer(), "A-Wall");
        d.undo();
        assert_eq!(d.entity(id).unwrap().layer, "Walls");
        assert!(d.rename_layer("0", "Zero").is_err());
        assert!(d.rename_layer("Walls", "a/b").is_err());
    }

    #[test]
    fn extents_skip_frozen_layers() {
        let mut d = Document::new();
        d.add(line(0.0));
        d.add_layer(Layer::new("Far", 1));
        d.add_on_layer(line(1.0e6), "Far", Color::ByLayer);
        assert!(d.extents().unwrap().max.x > 1.0e5);
        d.update_layer(Layer { frozen: true, ..Layer::new("Far", 1) });
        assert_eq!(d.extents().unwrap().max.x, 0.0);
    }

    #[test]
    fn delete_layer_rules() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Empty", 1));
        d.add_layer(Layer::new("Used", 2));
        d.add_on_layer(line(0.0), "Used", Color::ByLayer);
        assert!(d.delete_layer("0").is_err());
        assert!(d.delete_layer("Used").is_err());
        assert!(d.delete_layer("Empty").is_ok());
        assert!(d.layer("Empty").is_none());
        assert_eq!(d.unused_layer_name(), "Layer1");
    }

    #[test]
    fn block_references_expand_with_layer_zero_inheriting() {
        let mut d = Document::new();
        d.add_layer(Layer::new("Doors", 2));
        d.add_layer(Layer::new("Red", 1));
        d.define_block(Block {
            name: "DOOR".into(),
            base: Point::ORIGIN,
            entities: vec![
                BlockEntity::new("0", Color::ByLayer, line(1.0)),
                BlockEntity::new("Red", Color::ByLayer, line(2.0)),
            ],
        });
        let insert = Geometry::Insert {
            block: "DOOR".into(),
            placement: Placement::at(Point::new(10.0, 0.0)),
            attributes: Vec::new(),
            clip: None,
        };
        let id = d.add_on_layer(insert, "Doors", Color::ByLayer);
        let e = d.entity(id).unwrap();
        let styled = d.expanded_styled(e);
        assert_eq!(styled.len(), 2);
        assert_eq!((styled[0].color, styled[0].layer.as_str()), (2, "Doors"), "layer 0 takes the reference's layer");
        assert_eq!((styled[1].color, styled[1].layer.as_str()), (1, "Red"));
        assert!((d.entity_distance(e, Point::new(11.0, 0.5)) - 0.0).abs() < 1e-9);
        assert_eq!(d.extents().unwrap().min, Point::new(11.0, 0.0));

        // Undo removes the reference, then the definition.
        assert!(d.undo().is_some() && d.undo().is_some());
        assert!(d.block("DOOR").is_none());
    }

    #[test]
    fn attributes_draw_with_the_reference_and_byblock_takes_its_color() {
        use crate::{Attach, entity::Attribute};
        let mut d = Document::new();
        d.add_layer(Layer::new("Points", 3));
        d.define_block(Block {
            name: "PT".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new(
                "0",
                Color::ByBlock,
                Geometry::Circle { center: Point::ORIGIN, radius: 0.5 },
            )],
        });
        let attr = |tag: &str, value: &str, invisible| Attribute {
            tag: tag.into(),
            value: value.into(),
            layer: "0".into(),
            color: Color::ByBlock,
            position: Point::new(11.0, 0.0),
            height: 2.0,
            rotation: 0.0,
            attach: Attach::BottomLeft,
            invisible,
        };
        let id = d.add_on_layer(
            Geometry::Insert {
                block: "PT".into(),
                placement: Placement::at(Point::new(10.0, 0.0)),
                attributes: vec![attr("NOMER", "17", false), attr("CREATOR", "x", true), attr("KOTA", "", false)],
                clip: None,
            },
            "Points",
            Color::ByLayer,
        );
        let styled = d.expanded_styled(d.entity(id).unwrap());
        assert_eq!(styled.len(), 2, "the circle and the one visible, non-empty attribute");
        assert!(styled.iter().all(|s| s.color == 3), "ByBlock takes the reference's (layer) color");
        assert!(matches!(&styled[1].geometry, Geometry::Text { content, .. } if content == "17"));

        // An attribute on an off layer isn't drawn; one with its own color keeps it.
        let mut off = Layer::new("Labels", 5);
        off.on = false;
        d.add_layer(off.clone());
        d.update_layer(off);
        let mut g = d.entity(id).unwrap().geometry.clone();
        if let Geometry::Insert { attributes, .. } = &mut g {
            attributes[0].layer = "Labels".into();
            attributes[2] = Attribute { value: "9.81".into(), color: Color::Index(150), ..attributes[2].clone() };
        }
        d.modify(id, g);
        let styled = d.expanded_styled(d.entity(id).unwrap());
        assert_eq!(styled.len(), 2);
        assert!(matches!(&styled[1].geometry, Geometry::Text { content, .. } if content == "9.81"));
        assert_eq!(styled[1].color, 150);
        d.undo();

        // Moving the reference moves its attributes.
        let moved = d.entity(id).unwrap().geometry.translated(Point::new(5.0, 5.0));
        let Geometry::Insert { attributes, .. } = moved else { panic!() };
        assert_eq!(attributes[0].position, Point::new(16.0, 5.0));
    }

    #[test]
    fn linetypes_and_lineweights_resolve_through_layers_and_blocks() {
        let mut d = Document::new();
        let mut hidden = Layer::new("Hidden", 1);
        (hidden.linetype, hidden.lineweight) = ("HIDDEN".into(), Lineweight::Hundredths(50));
        d.add_layer(hidden);
        let by_block = BlockEntity {
            linetype: linetype::BYBLOCK.into(),
            lineweight: Lineweight::ByBlock,
            ..BlockEntity::new("0", Color::ByLayer, line(1.0))
        };
        let block = Block {
            name: "B".into(),
            base: Point::ORIGIN,
            entities: vec![by_block, BlockEntity::new("0", Color::ByLayer, line(2.0))],
        };
        d.define_block(block);
        let insert = Geometry::Insert {
            block: "B".into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: Vec::new(),
            clip: None,
        };
        let id = d.add_on_layer(insert, "Hidden", Color::ByLayer);
        d.update(id, |e| e.linetype = "CENTER".into());
        let styled = d.expanded_styled(d.entity(id).unwrap());
        // ByBlock takes the reference's own linetype and its (ByLayer) lineweight.
        assert_eq!((styled[0].linetype.as_str(), styled[0].lineweight), ("CENTER", Lineweight::Hundredths(50)));
        // ByLayer on layer 0 inside a block means the reference's layer.
        assert_eq!(styled[1].linetype, "HIDDEN");
        assert_eq!(d.linetype("hidden").unwrap().pattern, vec![6.35, -3.175]);
        assert!(d.linetype("NOSUCH").is_none());
        assert_eq!(d.linetypes()[0].name, "Continuous");
    }

    #[test]
    fn self_referencing_blocks_stop_expanding() {
        let mut d = Document::new();
        let me = Geometry::Insert {
            block: "LOOP".into(),
            placement: Placement::at(Point::ORIGIN),
            attributes: Vec::new(),
            clip: None,
        };
        d.define_block(Block {
            name: "LOOP".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByLayer, me.clone())],
        });
        let id = d.add(me);
        assert!(d.expanded(d.entity(id).unwrap()).is_empty());
    }
}

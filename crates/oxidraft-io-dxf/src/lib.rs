//! Reading and writing DXF, Oxidraft's native exchange format.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;

use dxf::LwPolylineVertex;
use dxf::entities::{Entity as DxfEntity, EntityType, LwPolyline};
use dxf::enums::{AcadVersion, AttributeVisibility};
use dxf::enums::{AttachmentPoint, DimensionType, HorizontalTextJustification, VerticalTextJustification};
use dxf::tables::Layer as DxfLayer;
use dxf::{Color as DxfColor, Drawing, Point as DxfPoint};
use oxidraft_geom::Point;
use oxidraft_model::{
    Attach, Attribute, AttributeDisplay, Block, BlockEntity, Color, Document, Ellipse, EntityId, Geometry,
    HatchPattern, Layer, Linetype, Lineweight, MODEL, Placement, Space, Spline, Vertex, Xref, XrefStatus,
    annotation::{angle_around, decode_control_codes, two_line_angle},
    constant_width, hatch, is_dependent, linetype,
};

mod clip_dxf;
mod dim_xdata;
mod hatch_dxf;
mod layout_dxf;
mod underlay_dxf;

pub type Error = dxf::DxfError;

/// Result of an import, with what was skipped.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    /// Number of DXF entities of types Oxidraft can't represent yet.
    pub skipped: usize,
    /// The objects of model and paper space by their DXF handles.
    pub handles: HashMap<u64, EntityId>,
}

fn to_dxf(p: Point) -> DxfPoint {
    DxfPoint::new(p.x, p.y, 0.0)
}

fn from_dxf(p: &DxfPoint) -> Point {
    Point::new(p.x, p.y)
}

fn to_dxf_vector(v: Point) -> dxf::Vector {
    dxf::Vector::new(v.x, v.y, 0.0)
}

fn from_dxf_vector(v: &dxf::Vector) -> Point {
    Point::new(v.x, v.y)
}

fn to_dxf_color(c: Color) -> DxfColor {
    match c {
        Color::ByLayer => DxfColor::by_layer(),
        Color::ByBlock => DxfColor::by_block(),
        Color::Index(i) => DxfColor::from_index(i),
    }
}

fn from_dxf_color(c: &DxfColor) -> Color {
    match c.index() {
        _ if c.is_by_block() => Color::ByBlock,
        Some(i) if !c.is_by_layer() && i > 0 => Color::Index(i),
        _ => Color::ByLayer,
    }
}

fn linetype_from_dxf(name: &str) -> String {
    if name.is_empty() || linetype::is_by_layer(name) {
        linetype::BYLAYER.into()
    } else if linetype::is_by_block(name) {
        linetype::BYBLOCK.into()
    } else {
        name.to_string()
    }
}

fn linetype_to_dxf(name: &str) -> String {
    if linetype::is_by_layer(name) || linetype::is_by_block(name) {
        name.to_ascii_uppercase()
    } else {
        name.to_string()
    }
}

/// An object's lineweight. The dxf crate reads a missing group 370 as 0, so
/// 0 is taken as ByLayer (AutoCAD's default) rather than as 0.00 mm.
fn lineweight_from_dxf(v: i16) -> Lineweight {
    if v == 0 { Lineweight::ByLayer } else { Lineweight::from_dxf(v) }
}

/// A text style's name in DXF: Standard as the dxf crate (and AutoCAD's
/// readers) expect it, "STANDARD".
fn style_to_dxf(name: &str) -> String {
    if name.trim().is_empty() || name.eq_ignore_ascii_case(oxidraft_model::annotation::STANDARD) {
        "STANDARD".into()
    } else {
        name.to_string()
    }
}

/// An oblique angle as read: within 85° and without the rounding of a
/// trip through radians.
fn oblique_from_dxf(a: f64) -> f64 {
    ((a * 1e9).round() / 1e9).clamp(-85.0, 85.0)
}

fn style_from_dxf(name: &str) -> String {
    if name.trim().is_empty() || name.eq_ignore_ascii_case(oxidraft_model::annotation::STANDARD) {
        oxidraft_model::annotation::STANDARD.into()
    } else {
        name.to_string()
    }
}

fn to_dxf_attach(a: Attach) -> AttachmentPoint {
    match a {
        Attach::TopLeft => AttachmentPoint::TopLeft,
        Attach::TopCenter => AttachmentPoint::TopCenter,
        Attach::TopRight => AttachmentPoint::TopRight,
        Attach::MiddleLeft => AttachmentPoint::MiddleLeft,
        Attach::MiddleCenter => AttachmentPoint::MiddleCenter,
        Attach::MiddleRight => AttachmentPoint::MiddleRight,
        Attach::BottomLeft => AttachmentPoint::BottomLeft,
        Attach::BottomCenter => AttachmentPoint::BottomCenter,
        Attach::BottomRight => AttachmentPoint::BottomRight,
    }
}

fn from_dxf_attach(a: AttachmentPoint) -> Attach {
    match a {
        AttachmentPoint::TopLeft => Attach::TopLeft,
        AttachmentPoint::TopCenter => Attach::TopCenter,
        AttachmentPoint::TopRight => Attach::TopRight,
        AttachmentPoint::MiddleLeft => Attach::MiddleLeft,
        AttachmentPoint::MiddleCenter => Attach::MiddleCenter,
        AttachmentPoint::MiddleRight => Attach::MiddleRight,
        AttachmentPoint::BottomLeft => Attach::BottomLeft,
        AttachmentPoint::BottomCenter => Attach::BottomCenter,
        AttachmentPoint::BottomRight => Attach::BottomRight,
    }
}

/// Single-line text (TEXT, ATTRIB, ATTDEF) justification as an attachment
/// point, and whether the text hangs from the second alignment point (code
/// 11) rather than the first (code 10), which only left/baseline text uses.
fn text_justification(h: HorizontalTextJustification, v: VerticalTextJustification) -> (Attach, bool) {
    use HorizontalTextJustification as H;
    use VerticalTextJustification as V;
    match (h, v) {
        (H::Left, V::Baseline) | (H::Aligned | H::Fit, _) => (Attach::BottomLeft, false),
        (H::Middle, _) => (Attach::MiddleCenter, true),
        (h, v) => {
            let attach = match (v, h) {
                (V::Top, H::Left) => Attach::TopLeft,
                (V::Top, H::Center) => Attach::TopCenter,
                (V::Top, _) => Attach::TopRight,
                (V::Middle, H::Left) => Attach::MiddleLeft,
                (V::Middle, H::Center) => Attach::MiddleCenter,
                (V::Middle, _) => Attach::MiddleRight,
                (_, H::Left) => Attach::BottomLeft,
                (_, H::Center) => Attach::BottomCenter,
                (_, _) => Attach::BottomRight,
            };
            (attach, true)
        }
    }
}

fn to_text_justification(a: Attach) -> (HorizontalTextJustification, VerticalTextJustification) {
    use HorizontalTextJustification as H;
    use VerticalTextJustification as V;
    let (fx, fy) = a.fractions();
    let h = if fx == 0.0 {
        H::Left
    } else if fx == 0.5 {
        H::Center
    } else {
        H::Right
    };
    let v = if fy == 0.0 {
        V::Top
    } else if fy == 0.5 {
        V::Middle
    } else {
        V::Baseline
    };
    (h, v)
}

fn attribute_from_dxf(a: &dxf::entities::Attribute) -> Attribute {
    let (attach, second) = text_justification(a.horizontal_text_justification, a.vertical_text_justification);
    Attribute {
        tag: a.attribute_tag.clone(),
        value: decode_control_codes(&a.value),
        position: from_dxf(if second { &a.second_alignment_point } else { &a.location }),
        height: a.text_height,
        rotation: a.rotation,
        attach,
        invisible: a.is_invisible(),
        // Filled in from the file text by `from_drawing_styled`.
        layer: "0".into(),
        color: Color::ByBlock,
    }
}

fn attribute_to_dxf(a: &Attribute) -> dxf::entities::Attribute {
    let (horizontal_text_justification, vertical_text_justification) = to_text_justification(a.attach);
    let mut out = dxf::entities::Attribute {
        attribute_tag: a.tag.clone(),
        value: a.value.clone(),
        location: to_dxf(a.position),
        second_alignment_point: to_dxf(a.position),
        text_height: a.height,
        rotation: a.rotation,
        horizontal_text_justification,
        vertical_text_justification,
        ..Default::default()
    };
    out.set_is_invisible(a.invisible);
    out
}

/// Collects the extra blocks some objects need: a dimension's graphics, and
/// hatches (which the dxf crate can't write as HATCH entities yet).
#[derive(Default)]
struct Exporter {
    blocks: Vec<dxf::Block>,
    /// AcDbHatch data for each placeholder POINT, by its X coordinate.
    hatches: Vec<Vec<(i32, String)>>,
    /// The image and PDF definitions' handles, for their references.
    images: underlay_dxf::Handles,
}

impl Exporter {
    fn anonymous_block(&mut self, prefix: char, entities: Vec<EntityType>, layer: &str) -> String {
        let name = format!("*{prefix}{}", self.blocks.len() + 1);
        self.blocks.push(dxf::Block {
            name: name.clone(),
            entities: entities
                .into_iter()
                .map(|t| {
                    let mut e = DxfEntity::new(t);
                    e.common.layer = layer.to_string();
                    e.common.color = DxfColor::by_block();
                    e.common.line_type_name = "BYBLOCK".into();
                    e.common.lineweight_enum_value = Lineweight::ByBlock.to_dxf();
                    e
                })
                .collect(),
            ..Default::default()
        });
        name
    }

    /// A dimension's drawn parts as an anonymous `*D` block, which other
    /// programs show as-is, and where its text sits.
    fn dimension_block(&mut self, g: &Geometry, layer: &str) -> (String, Option<Point>) {
        let primitives = g.primitives().unwrap_or_default();
        // Arrowheads are SOLIDs, as in AutoCAD's own dimension blocks.
        let parts: Vec<EntityType> = primitives
            .iter()
            .flat_map(|p| match p {
                Geometry::Hatch { loops, pattern: HatchPattern::Solid, .. } => solids(loops),
                _ => self.entities(p, layer),
            })
            .collect();
        let text_mid = primitives.iter().find_map(|p| match p {
            Geometry::Text { position, .. } => Some(*position),
            _ => None,
        });
        (self.anonymous_block('D', parts, layer), text_mid)
    }

    /// DXF entities for one object.
    fn entities(&mut self, g: &Geometry, layer: &str) -> Vec<EntityType> {
        match g {
            Geometry::Point { position } => {
                vec![EntityType::ModelPoint(dxf::entities::ModelPoint::new(to_dxf(*position)))]
            }
            Geometry::Line { start, end } => {
                vec![EntityType::Line(dxf::entities::Line::new(to_dxf(*start), to_dxf(*end)))]
            }
            Geometry::Circle { center, radius } => {
                vec![EntityType::Circle(dxf::entities::Circle::new(to_dxf(*center), *radius))]
            }
            Geometry::Arc { center, radius, start_angle, end_angle } => vec![EntityType::Arc(dxf::entities::Arc {
                center: to_dxf(*center),
                radius: *radius,
                start_angle: *start_angle,
                end_angle: *end_angle,
                ..Default::default()
            })],
            Geometry::Polyline { vertices, closed } => {
                // One width for the whole polyline is written once, as AutoCAD does.
                let constant = constant_width(vertices).unwrap_or(0.0);
                let mut pl = LwPolyline {
                    vertices: vertices
                        .iter()
                        .map(|v| LwPolylineVertex {
                            x: v.point.x,
                            y: v.point.y,
                            bulge: v.bulge,
                            starting_width: if constant > 0.0 { 0.0 } else { v.start_width },
                            ending_width: if constant > 0.0 { 0.0 } else { v.end_width },
                            ..Default::default()
                        })
                        .collect(),
                    constant_width: constant,
                    ..Default::default()
                };
                pl.set_is_closed(*closed);
                vec![EntityType::LwPolyline(pl)]
            }
            Geometry::Ellipse(e) => {
                let (start, end) =
                    if e.is_full() { (0.0, std::f64::consts::TAU) } else { (e.start.to_radians(), e.end.to_radians()) };
                vec![EntityType::Ellipse(dxf::entities::Ellipse {
                    center: to_dxf(e.center),
                    major_axis: to_dxf_vector(e.major),
                    minor_axis_ratio: e.ratio,
                    start_parameter: start,
                    end_parameter: end,
                    ..Default::default()
                })]
            }
            Geometry::Spline(s) => {
                let mut sp = dxf::entities::Spline {
                    degree_of_curve: s.degree() as i32,
                    knot_values: s.knots().to_vec(),
                    weight_values: if s.is_rational() { s.weights() } else { Vec::new() },
                    control_points: s.control().iter().map(|p| to_dxf(*p)).collect(),
                    fit_points: s.fit().iter().map(|p| to_dxf(*p)).collect(),
                    ..Default::default()
                };
                sp.set_is_closed(s.is_closed());
                sp.set_is_periodic(s.is_periodic());
                sp.set_is_rational(s.is_rational());
                sp.set_is_planar(true);
                vec![EntityType::Spline(sp)]
            }
            Geometry::Xline { base, direction } => vec![EntityType::XLine(dxf::entities::XLine {
                first_point: to_dxf(*base),
                unit_direction_vector: to_dxf_vector(*direction * (1.0 / direction.length().max(f64::MIN_POSITIVE))),
            })],
            Geometry::Ray { base, direction } => vec![EntityType::Ray(dxf::entities::Ray {
                start_point: to_dxf(*base),
                unit_direction_vector: to_dxf_vector(*direction * (1.0 / direction.length().max(f64::MIN_POSITIVE))),
            })],
            // A mirrored reference is a negative X scale in DXF.
            Geometry::Insert { block, placement, .. } => vec![EntityType::Insert(dxf::entities::Insert {
                name: block.clone(),
                location: to_dxf(placement.position),
                x_scale_factor: if placement.mirror { -placement.scale } else { placement.scale },
                y_scale_factor: placement.scale,
                z_scale_factor: placement.scale,
                rotation: placement.rotation,
                ..Default::default()
            })],
            Geometry::Text { position, height, rotation, width, attach, content, look } => {
                if is_single_line(*attach, content) {
                    return vec![EntityType::Text(dxf::entities::Text {
                        location: to_dxf(*position),
                        text_height: *height,
                        value: content.clone(),
                        rotation: *rotation,
                        relative_x_scale_factor: look.width_factor,
                        oblique_angle: look.oblique,
                        text_style_name: style_to_dxf(&look.style),
                        ..Default::default()
                    })];
                }
                let r = rotation.to_radians();
                vec![EntityType::MText(dxf::entities::MText {
                    insertion_point: to_dxf(*position),
                    initial_text_height: *height,
                    reference_rectangle_width: *width,
                    attachment_point: to_dxf_attach(*attach),
                    text: content.replace('\n', "\\P"),
                    rotation_angle: r,
                    x_axis_direction: dxf::Vector::new(r.cos(), r.sin(), 0.0),
                    text_style_name: style_to_dxf(&look.style),
                    ..Default::default()
                })]
            }
            Geometry::Dimension { p1, p2, line, rotation, aligned, text, extra, .. } => {
                let (block_name, text_mid) = self.dimension_block(g, layer);
                let readable =
                    oxidraft_model::annotation::readable(if *aligned { p1.angle_to(*p2) } else { *rotation });
                let r = if *aligned { p1.angle_to(*p2) } else { *rotation };
                let u = Point::ORIGIN.polar(1.0, r);
                let on_line = *line + u * (*p2 - *line).dot(u);
                vec![EntityType::RotatedDimension(dxf::entities::RotatedDimension {
                    dimension_base: dxf::entities::DimensionBase {
                        block_name,
                        definition_point_1: to_dxf(on_line),
                        text_mid_point: to_dxf(text_mid.unwrap_or(*line)),
                        dimension_type: if *aligned {
                            DimensionType::Aligned
                        } else {
                            DimensionType::RotatedHorizontalOrVertical
                        },
                        actual_measurement: oxidraft_model::annotation::measurement(*p1, *p2, *rotation, *aligned),
                        text: text.clone().unwrap_or_default(),
                        // Text moved (DIMTEDIT) or turned from along the line.
                        is_at_user_defined_location: extra.text_at.is_some(),
                        text_rotation_angle: extra.text_angle.map_or(0.0, |a| a - readable),
                        ..Default::default()
                    },
                    definition_point_2: to_dxf(*p1),
                    definition_point_3: to_dxf(*p2),
                    rotation_angle: if *aligned { 0.0 } else { *rotation },
                    extension_line_angle: extra.oblique.unwrap_or(0.0),
                    ..Default::default()
                })]
            }
            // The text point is where the dimension line ends, so it reads back exactly.
            Geometry::RadialDimension { center, point, text_at, diameter, text, .. } => {
                let (block_name, _) = self.dimension_block(g, layer);
                let r = center.distance(*point);
                let base =
                    |definition_point_1: Point, dimension_type, actual_measurement| dxf::entities::DimensionBase {
                        block_name,
                        definition_point_1: to_dxf(definition_point_1),
                        text_mid_point: to_dxf(*text_at),
                        dimension_type,
                        actual_measurement,
                        text: text.clone().unwrap_or_default(),
                        ..Default::default()
                    };
                // A diameter runs from the point on the circle to the opposite one.
                vec![if *diameter {
                    EntityType::DiameterDimension(dxf::entities::DiameterDimension {
                        dimension_base: base(*center * 2.0 - *point, DimensionType::Diameter, 2.0 * r),
                        definition_point_2: to_dxf(*point),
                        ..Default::default()
                    })
                } else {
                    EntityType::RadialDimension(dxf::entities::RadialDimension {
                        dimension_base: base(*center, DimensionType::Radius, r),
                        definition_point_2: to_dxf(*point),
                        ..Default::default()
                    })
                }]
            }
            Geometry::AngularDimension { vertex, p1, p2, arc, text, .. } => {
                let (block_name, text_mid) = self.dimension_block(g, layer);
                let angle = oxidraft_model::annotation::angular_measurement(*vertex, *p1, *p2);
                vec![EntityType::AngularThreePointDimension(dxf::entities::AngularThreePointDimension {
                    dimension_base: dxf::entities::DimensionBase {
                        block_name,
                        definition_point_1: to_dxf(*arc),
                        text_mid_point: to_dxf(text_mid.unwrap_or(*arc)),
                        dimension_type: DimensionType::AngularThreePoint,
                        actual_measurement: angle.to_radians(),
                        text: text.clone().unwrap_or_default(),
                        ..Default::default()
                    },
                    definition_point_2: to_dxf(*p1),
                    definition_point_3: to_dxf(*p2),
                    definition_point_4: to_dxf(*vertex),
                    ..Default::default()
                })]
            }
            // A LEADER for the arrow and lines, plus its text as MTEXT.
            // Added arrowheads (MLEADEREDIT) as LEADERs of their own.
            Geometry::Leader { points, more, .. } => {
                let leader = |vertices: Vec<Point>| {
                    EntityType::Leader(dxf::entities::Leader {
                        vertices: vertices.iter().map(|p| to_dxf(*p)).collect(),
                        use_arrowheads: true,
                        ..Default::default()
                    })
                };
                let mut out = vec![leader(points.clone())];
                if let Some(end) = points.last() {
                    out.extend(more.iter().map(|tip| leader(vec![*tip, *end])));
                }
                for p in g.primitives().unwrap_or_default() {
                    if matches!(p, Geometry::Text { .. }) {
                        out.extend(self.entities(&p, layer));
                    }
                }
                out
            }
            Geometry::Image { .. } => underlay_dxf::entity(g, &self.images).into_iter().collect(),
            // The dxf crate has no VIEWPORT: they're added with their layouts.
            Geometry::Viewport { .. } => Vec::new(),
            Geometry::Wipeout { points } => wipeout_to_dxf(points).into_iter().collect(),
            Geometry::Tolerance { position, rotation, content, .. } => {
                let r = rotation.to_radians();
                vec![EntityType::Tolerance(dxf::entities::Tolerance {
                    dimension_style_name: "STANDARD".into(),
                    insertion_point: to_dxf(*position),
                    display_text: content.clone(),
                    direction_vector: dxf::Vector::new(r.cos(), r.sin(), 0.0),
                    ..Default::default()
                })]
            }
            Geometry::OrdinateDimension { feature, leader_end, x_datum, text, .. } => {
                let (block_name, text_mid) = self.dimension_block(g, layer);
                let mut dimension_base = dxf::entities::DimensionBase {
                    block_name,
                    definition_point_1: to_dxf(Point::ORIGIN),
                    text_mid_point: to_dxf(text_mid.unwrap_or(*leader_end)),
                    dimension_type: DimensionType::Ordinate,
                    actual_measurement: if *x_datum { feature.x } else { feature.y },
                    text: text.clone().unwrap_or_default(),
                    ..Default::default()
                };
                dimension_base.is_ordinate_x_type = *x_datum;
                vec![EntityType::OrdinateDimension(dxf::entities::OrdinateDimension {
                    dimension_base,
                    definition_point_2: to_dxf(*feature),
                    definition_point_3: to_dxf(*leader_end),
                })]
            }
            // The dxf crate has no arc length or jogged dimensions: they go
            // out as what they draw, a block.
            Geometry::ArcLengthDimension { .. } | Geometry::JoggedDimension { .. } => {
                let (name, _) = self.dimension_block(g, layer);
                vec![EntityType::Insert(dxf::entities::Insert { name, ..Default::default() })]
            }
            Geometry::AttributeDefinition {
                tag,
                prompt,
                default,
                position,
                height,
                rotation,
                attach,
                invisible,
                constant,
            } => {
                let (horizontal_text_justification, vertical_text_justification) = to_text_justification(*attach);
                let mut a = dxf::entities::AttributeDefinition {
                    text_tag: tag.clone(),
                    prompt: prompt.clone(),
                    value: default.clone(),
                    location: to_dxf(*position),
                    second_alignment_point: to_dxf(*position),
                    text_height: *height,
                    rotation: *rotation,
                    horizontal_text_justification,
                    vertical_text_justification,
                    ..Default::default()
                };
                a.set_is_invisible(*invisible);
                a.set_is_constant(*constant);
                vec![EntityType::AttributeDefinition(a)]
            }
            // A table goes out as its lines and text.
            Geometry::Table { .. } => {
                g.primitives().unwrap_or_default().iter().flat_map(|p| self.entities(p, layer)).collect()
            }
            // A placeholder the crate can write; `write` turns it into a HATCH.
            Geometry::Hatch { .. } => {
                self.hatches.push(hatch_dxf::hatch_data(g));
                let index = (self.hatches.len() - 1) as f64;
                vec![EntityType::ModelPoint(dxf::entities::ModelPoint {
                    location: DxfPoint::new(index, 0.0, hatch_dxf::PLACEHOLDER_Z),
                    ..Default::default()
                })]
            }
        }
    }

    fn entity(
        &mut self,
        geometry: &Geometry,
        layer: &str,
        color: Color,
        linetype: &str,
        lineweight: Lineweight,
    ) -> Vec<DxfEntity> {
        self.entities(geometry, layer)
            .into_iter()
            .map(|t| {
                let mut de = DxfEntity::new(t);
                de.common.layer = layer.to_string();
                de.common.color = to_dxf_color(color);
                de.common.line_type_name = linetype_to_dxf(linetype);
                de.common.lineweight_enum_value = lineweight.to_dxf();
                de
            })
            .collect()
    }
}

/// A solid fill as SOLID triangles.
fn solids(loops: &[Vec<Vertex>]) -> Vec<EntityType> {
    hatch::triangles(&hatch::polygons(loops))
        .into_iter()
        .map(|[a, b, c]| {
            EntityType::Solid(dxf::entities::Solid {
                first_corner: to_dxf(a),
                second_corner: to_dxf(b),
                third_corner: to_dxf(c),
                fourth_corner: to_dxf(c),
                ..Default::default()
            })
        })
        .collect()
}

/// Builds a DXF drawing from a document. The dxf crate has no HATCH
/// entity, so hatches are POINT placeholders here; [`write`] turns them into
/// HATCHes.
pub fn to_drawing(doc: &Document) -> Drawing {
    build(doc).0
}

/// What [`build`] makes: the drawing, the AcDbHatch data of each hatch
/// placeholder, and each ATTRIB's layer and color in the order they're in
/// the file.
/// What `build` makes: the drawing, the hatches and attribute styles to
/// put in afterwards, and the handles each object was written with.
type Built = (Drawing, Vec<Vec<(i32, String)>>, Vec<(String, Color)>, HashMap<EntityId, Vec<u64>>);

/// Adds an INSERT's attributes, noting their styles for `fix_attributes`.
fn add_attributes(drawing: &mut Drawing, de: &mut DxfEntity, g: &Geometry, styles: &mut Vec<(String, Color)>) {
    if let (EntityType::Insert(ins), Geometry::Insert { attributes, .. }) = (&mut de.specific, g) {
        for a in attributes {
            ins.add_attribute(drawing, attribute_to_dxf(a));
            styles.push((a.layer.clone(), a.color));
        }
    }
}

/// $INSUNITS values in order.
const DXF_UNITS: [dxf::enums::Units; 25] = {
    use dxf::enums::Units as U;
    [
        U::Unitless,
        U::Inches,
        U::Feet,
        U::Miles,
        U::Millimeters,
        U::Centimeters,
        U::Meters,
        U::Kilometers,
        U::Microinches,
        U::Mils,
        U::Yards,
        U::Angstroms,
        U::Nanometers,
        U::Microns,
        U::Decimeters,
        U::Decameters,
        U::Hectometers,
        U::Gigameters,
        U::AstronomicalUnits,
        U::LightYears,
        U::Parsecs,
        U::USSurveyFeet,
        U::USSurveyInch,
        U::USSurveyYard,
        U::USSurveyMile,
    ]
};

/// UNITS into the header: $LUNITS, $LUPREC, $AUNITS, $AUPREC, $ANGDIR,
/// $ANGBASE (degrees), $INSUNITS and $MEASUREMENT.
fn write_units(h: &mut dxf::Header, u: oxidraft_model::Units) {
    use dxf::enums::{AngleDirection, AngleFormat, DrawingUnits, UnitFormat};
    use oxidraft_model::{AngleType, LengthType};
    h.unit_format = match u.length {
        LengthType::Scientific => UnitFormat::Scientific,
        LengthType::Decimal => UnitFormat::Decimal,
        LengthType::Engineering => UnitFormat::Engineering,
        LengthType::Architectural => UnitFormat::ArchitecturalStacked,
        LengthType::Fractional => UnitFormat::FractionalStacked,
    };
    h.unit_precision = i16::from(u.length_precision);
    h.angle_unit_format = match u.angle {
        AngleType::DecimalDegrees => AngleFormat::DecimalDegrees,
        AngleType::DegMinSec => AngleFormat::DegreesMinutesSeconds,
        AngleType::Grads => AngleFormat::Gradians,
        AngleType::Radians => AngleFormat::Radians,
        AngleType::Surveyor => AngleFormat::SurveyorsUnits,
    };
    h.angle_unit_precision = i16::from(u.angle_precision);
    h.angle_direction = if u.clockwise { AngleDirection::Clockwise } else { AngleDirection::CounterClockwise };
    h.angle_zero_direction = u.base_angle;
    h.default_drawing_units = DXF_UNITS.get(u.insertion as usize).copied().unwrap_or(dxf::enums::Units::Unitless);
    let english = matches!(u.insertion, 1 | 2 | 3 | 8 | 9 | 10 | 21..=24);
    h.drawing_units = if english { DrawingUnits::English } else { DrawingUnits::Metric };
}

/// True if a text is written as TEXT rather than MTEXT.
fn is_single_line(attach: Attach, content: &str) -> bool {
    attach == Attach::BottomLeft && !content.contains("\\P") && !content.contains('\n')
}

/// The STYLE table and $TEXTSTYLE. A font named without an extension is a
/// TrueType font's.
fn write_text_styles(drawing: &mut Drawing, doc: &Document) {
    for s in doc.text_styles() {
        let name = style_to_dxf(&s.name);
        let font = if s.font.contains('.') { s.font.clone() } else { format!("{}.ttf", s.font.to_lowercase()) };
        let set = |x: &mut dxf::tables::Style| {
            x.text_height = s.height;
            x.width_factor = s.width_factor;
            x.oblique_angle = s.oblique;
            x.primary_font_file_name = font.clone();
        };
        if let Some(x) = drawing.styles_mut().find(|x| x.name.eq_ignore_ascii_case(&name)) {
            set(x);
            continue;
        }
        let mut x = dxf::tables::Style { name, ..Default::default() };
        set(&mut x);
        drawing.add_style(x);
    }
    drawing.header.text_style = style_to_dxf(&doc.current_text_style().name);
}

fn read_text_styles(doc: &mut Document, drawing: &Drawing) {
    for s in drawing.styles() {
        // Unnamed styles are shape files that linetypes use.
        if s.name.trim().is_empty() || s.text_generation_flags & 1 != 0 {
            continue;
        }
        let style = oxidraft_model::TextStyle {
            name: style_from_dxf(&s.name),
            font: if s.primary_font_file_name.is_empty() {
                "arial.ttf".into()
            } else {
                s.primary_font_file_name.clone()
            },
            height: s.text_height.max(0.0),
            width_factor: if s.width_factor > 0.0 { s.width_factor } else { 1.0 },
            oblique: oblique_from_dxf(s.oblique_angle),
        };
        let _ = doc.set_text_style(style);
    }
    let _ = doc.set_current_text_style(&style_from_dxf(&drawing.header.text_style));
}

/// A UCS's axes as DXF vectors.
fn ucs_axes(ucs: &oxidraft_model::ucs::Ucs) -> (dxf::Vector, dxf::Vector) {
    let (x, y) = (Point::ORIGIN.polar(1.0, ucs.angle), Point::ORIGIN.polar(1.0, ucs.angle + 90.0));
    (dxf::Vector::new(x.x, x.y, 0.0), dxf::Vector::new(y.x, y.y, 0.0))
}

/// The UCS an origin and X axis make (`None` for a UCS not in plan).
fn ucs_from(origin: &DxfPoint, x: &dxf::Vector) -> Option<oxidraft_model::ucs::Ucs> {
    (x.x.hypot(x.y) > 1e-9 && x.z.abs() < 1e-9)
        .then(|| oxidraft_model::ucs::Ucs { origin: from_dxf(origin), angle: x.y.atan2(x.x).to_degrees() })
}

/// The current UCS ($UCSORG, $UCSXDIR, $UCSYDIR) and the named ones.
fn write_ucs(drawing: &mut Drawing, doc: &Document) {
    let ucs = doc.ucs();
    drawing.header.ucs_origin = to_dxf(ucs.origin);
    (drawing.header.ucs_x_axis, drawing.header.ucs_y_axis) = ucs_axes(&ucs);
    for n in doc.named_ucs() {
        let (x_axis, y_axis) = ucs_axes(&n.ucs);
        drawing.add_ucs(dxf::tables::Ucs {
            name: n.name.clone(),
            origin: to_dxf(n.ucs.origin),
            x_axis,
            y_axis,
            ..Default::default()
        });
    }
}

fn read_ucs(doc: &mut Document, drawing: &Drawing) {
    for u in drawing.ucss() {
        if let Some(ucs) = ucs_from(&u.origin, &u.x_axis) {
            let _ = doc.save_ucs(&u.name, ucs);
        }
    }
    if let Some(ucs) = ucs_from(&drawing.header.ucs_origin, &drawing.header.ucs_x_axis).filter(|u| !u.is_world()) {
        doc.set_ucs(ucs);
    }
}

/// True if the drawing has what AutoCAD 2000's DXF can't hold: PDF
/// underlays, transparency or fields.
pub fn needs_newer_version(doc: &Document) -> bool {
    doc.image_defs().any(|d| d.is_pdf())
        || doc.all_entities().any(
            |e| matches!(&e.geometry, Geometry::Text { content, .. } if oxidraft_model::field::has_fields(content)),
        )
        || doc.layers().iter().any(|l| l.transparency > 0)
        || doc.all_entities().any(|e| e.transparency != oxidraft_model::Transparency::ByLayer)
}

/// An object's hyperlink as AutoCAD keeps it: PE_URL XDATA, the address
/// then the description in braces.
fn hyperlink_xdata(e: &oxidraft_model::Entity) -> Option<dxf::XData> {
    let link = e.hyperlink.as_ref()?;
    let items = vec![
        dxf::XDataItem::Str(link.url.clone()),
        dxf::XDataItem::ControlGroup(vec![dxf::XDataItem::Str(link.description.clone())]),
    ];
    Some(dxf::XData { application_name: "PE_URL".into(), items })
}

/// The XDATA application keeping an annotative object's scales.
const ANNO_APP: &str = "OXIDRAFT_ANNO";

/// An annotative object's scales as XDATA: the one it's sized for, then
/// all it supports.
fn anno_xdata(e: &oxidraft_model::Entity) -> Option<dxf::XData> {
    let a = e.annotative.as_ref()?;
    let items = std::iter::once(a.shown).chain(a.scales.iter().copied()).map(dxf::XDataItem::Real).collect();
    Some(dxf::XData { application_name: ANNO_APP.into(), items })
}

fn anno_from(x_data: &[dxf::XData]) -> Option<Box<oxidraft_model::Annotative>> {
    let x = x_data.iter().find(|x| x.application_name == ANNO_APP)?;
    let values: Vec<f64> = x
        .items
        .iter()
        .filter_map(|i| if let dxf::XDataItem::Real(v) = i { Some(*v) } else { None })
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    let (shown, scales) = values.split_first()?;
    (!scales.is_empty()).then(|| Box::new(oxidraft_model::Annotative { scales: scales.to_vec(), shown: *shown }))
}

fn hyperlink_from(x_data: &[dxf::XData]) -> Option<Box<oxidraft_model::Hyperlink>> {
    let x = x_data.iter().find(|x| x.application_name == "PE_URL")?;
    let url = x.items.iter().find_map(|i| if let dxf::XDataItem::Str(s) = i { Some(s.clone()) } else { None })?;
    let description = x
        .items
        .iter()
        .find_map(|i| match i {
            dxf::XDataItem::ControlGroup(g) => {
                g.iter().find_map(|i| if let dxf::XDataItem::Str(s) = i { Some(s.clone()) } else { None })
            }
            _ => None,
        })
        .unwrap_or_default();
    Some(Box::new(oxidraft_model::Hyperlink { url, description }))
}

/// The XDATA application a layer's transparency is kept under.
const TRANSPARENCY_APP: &str = "AcCmTransparency";

/// The XRECORD Oxidraft keeps its own settings in, in the root dictionary:
/// key (1) and value (40) pairs.
pub const SETTINGS_RECORD: &str = "OXIDRAFT_SETTINGS";

/// The XRECORD the drawing's layer states are kept in (see the DWG writer).
pub const LAYER_STATES_RECORD: &str = "OXIDRAFT_LAYER_STATES";

/// The XRECORD keeping the PDF underlays' layers turned off (ULAYERS).
pub const PDF_LAYERS_RECORD: &str = "OXIDRAFT_PDF_LAYERS";

fn read_pdf_layers(doc: &mut Document, raw: &layout_dxf::RawLayouts) {
    let Some(handle) = raw.dictionaries.values().flatten().find(|(n, _)| n == PDF_LAYERS_RECORD).map(|(_, h)| *h)
    else {
        return;
    };
    let Some(pairs) = raw.xrecords.get(&handle) else { return };
    let mut current: Option<(String, Vec<String>)> = None;
    for (code, value) in pairs.iter().chain(std::iter::once(&(1, String::new()))) {
        match code {
            1 => {
                if let Some((name, off)) = current.take().filter(|c| !c.0.is_empty()) {
                    doc.set_pdf_layers_off(&name, off);
                }
                current = Some((value.clone(), Vec::new()));
            }
            8 => current.iter_mut().for_each(|c| c.1.push(value.clone())),
            _ => {}
        }
    }
}

/// The settings worth keeping that DXF has no place for: the multileader
/// and table styles, as key and value pairs.
pub fn settings_of(doc: &Document) -> Vec<(&'static str, f64)> {
    let (l, t) = (doc.leader_style(), doc.table_style());
    vec![
        ("LEADER_ARROWHEAD", l.arrowhead.code()),
        ("LEADER_ARROW_SIZE", l.arrow_size),
        ("LEADER_TEXT_HEIGHT", l.text_height),
        ("LEADER_LANDING_ON", if l.landing_on { 1.0 } else { 0.0 }),
        ("LEADER_LANDING", l.landing),
        ("TABLE_TEXT_HEIGHT", t.text_height),
        ("TABLE_MARGIN", t.margin),
        ("IMAGEFRAME", f64::from(doc.picture_frame(false))),
        ("PDFFRAME", f64::from(doc.picture_frame(true))),
        ("WIPEOUTFRAME", f64::from(doc.wipeout_frame())),
        // acadrust's DXF leaves $LIMCHECK out.
        ("LIMCHECK", if doc.limits_check() { 1.0 } else { 0.0 }),
        ("CANNOSCALE", doc.anno_scale()),
        ("ANNOALLVISIBLE", if doc.anno_all_visible() { 1.0 } else { 0.0 }),
    ]
}

fn read_layer_states(doc: &mut Document, raw: &layout_dxf::RawLayouts) {
    let Some(handle) = raw.dictionaries.values().flatten().find(|(n, _)| n == LAYER_STATES_RECORD).map(|(_, h)| *h)
    else {
        return;
    };
    let Some(pairs) = raw.xrecords.get(&handle) else { return };
    let mut states: Vec<oxidraft_model::LayerState> = Vec::new();
    for (code, value) in pairs {
        let state = states.last_mut();
        let layer = state.and_then(|s| s.layers.last_mut());
        let int = || value.trim().parse::<i32>().unwrap_or(0);
        match (code, layer) {
            (1, _) => states.push(oxidraft_model::LayerState {
                name: value.clone(),
                description: String::new(),
                layers: Vec::new(),
                current: "0".into(),
            }),
            (3, _) => states.last_mut().into_iter().for_each(|s| s.description = value.clone()),
            (2, _) => states.last_mut().into_iter().for_each(|s| s.current = value.clone()),
            (8, _) => states.last_mut().into_iter().for_each(|s| s.layers.push(Layer::new(value.clone(), 7))),
            (90, Some(l)) => (l.on, l.frozen, l.locked) = (int() & 1 != 0, int() & 2 != 0, int() & 4 != 0),
            (62, Some(l)) => l.color = int().clamp(1, 255) as u8,
            (6, Some(l)) => l.linetype = value.clone(),
            (370, Some(l)) => l.lineweight = Lineweight::from_dxf(int() as i16),
            (440, Some(l)) => l.transparency = int().clamp(0, 90) as u8,
            _ => {}
        }
    }
    for s in states {
        doc.add_layer_state(s);
    }
}

/// DWGPROPS: the drawing's title, subject, author, comments and keywords.
fn read_properties(doc: &mut Document, raw: &layout_dxf::RawLayouts) {
    let Some(handle) = raw.dictionaries.values().flatten().find(|(n, _)| n == "DWGPROPS").map(|(_, h)| *h) else {
        return;
    };
    let Some(pairs) = raw.xrecords.get(&handle) else { return };
    let mut p = oxidraft_model::DrawingProperties::default();
    for (code, value) in pairs {
        match code {
            2 => p.title = value.clone(),
            3 => p.subject = value.clone(),
            4 => p.author = value.clone(),
            6 => p.comments = value.clone(),
            7 => p.keywords = value.clone(),
            _ => {}
        }
    }
    doc.set_properties(p);
}

fn read_settings(doc: &mut Document, raw: &layout_dxf::RawLayouts) {
    read_layer_states(doc, raw);
    read_pdf_layers(doc, raw);
    read_properties(doc, raw);
    let Some(handle) = raw.dictionaries.values().flatten().find(|(n, _)| n == SETTINGS_RECORD).map(|(_, h)| *h) else {
        return;
    };
    let Some(pairs) = raw.xrecords.get(&handle) else { return };
    let (mut l, mut t) = (doc.leader_style(), doc.table_style());
    let mut key = "";
    for (code, value) in pairs {
        match code {
            1 => key = value,
            40 => {
                let Ok(v) = value.parse::<f64>() else { continue };
                let positive = |v: f64, d: f64| if v > 0.0 && v.is_finite() { v } else { d };
                match key {
                    "LEADER_ARROWHEAD" => l.arrowhead = oxidraft_model::annotation::LeaderArrow::from_code(v),
                    "LEADER_ARROW_SIZE" => l.arrow_size = positive(v, l.arrow_size),
                    "LEADER_TEXT_HEIGHT" => l.text_height = positive(v, l.text_height),
                    "LEADER_LANDING_ON" => l.landing_on = v != 0.0,
                    "LEADER_LANDING" => l.landing = v.max(0.0),
                    "TABLE_TEXT_HEIGHT" => t.text_height = positive(v, t.text_height),
                    "TABLE_MARGIN" => t.margin = v.max(0.0),
                    "IMAGEFRAME" => doc.set_picture_frame(false, v as u8),
                    "PDFFRAME" => doc.set_picture_frame(true, v as u8),
                    "WIPEOUTFRAME" => doc.set_wipeout_frame(v as u8),
                    "LIMCHECK" => doc.set_limits_check(v != 0.0),
                    // The objects were saved sized for it already.
                    "CANNOSCALE" if v > 0.0 => doc.load_anno_scale(v),
                    "ANNOALLVISIBLE" => doc.set_anno_all_visible(v != 0.0),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    doc.set_annotation_styles(l, t);
}

/// MTEXT has no width factor or slant of its own: its style's.
fn restyle_mtext(doc: &mut Document) {
    let ids: Vec<_> = doc
        .all_entities()
        .filter(|e| matches!(&e.geometry, Geometry::Text { attach, content, .. } if !is_single_line(*attach, content)))
        .map(|e| e.id)
        .collect();
    for id in ids {
        let Some(Geometry::Text { look, .. }) = doc.entity(id).map(|e| &e.geometry) else { continue };
        let Some(style) = doc.text_style(&look.style).cloned() else { continue };
        doc.update(id, |e| {
            if let Geometry::Text { look, .. } = &mut e.geometry {
                (look.width_factor, look.oblique) = (style.width_factor, style.oblique);
            }
        });
    }
}

fn read_units(h: &dxf::Header) -> oxidraft_model::Units {
    use oxidraft_model::{AngleType, LengthType};
    oxidraft_model::Units {
        length: LengthType::from_code(h.unit_format as i16),
        length_precision: h.unit_precision.clamp(0, 8) as u8,
        angle: AngleType::from_code(h.angle_unit_format as i16),
        angle_precision: h.angle_unit_precision.clamp(0, 8) as u8,
        clockwise: h.angle_direction == dxf::enums::AngleDirection::Clockwise,
        base_angle: if h.angle_zero_direction.is_finite() { h.angle_zero_direction } else { 0.0 },
        insertion: h.default_drawing_units as i16,
    }
}

fn build(doc: &Document) -> Built {
    let mut drawing = Drawing::new();
    // R2000 is the oldest version with lightweight polylines; PDF underlays
    // need R2007 or later, and transparency R2004.
    let pdfs = needs_newer_version(doc);
    drawing.header.version = if pdfs { AcadVersion::R2010 } else { AcadVersion::R2000 };
    drawing.header.line_type_scale = doc.ltscale();
    let dim = doc.dim_style();
    drawing.header.dimensioning_text_height = dim.text_height;
    drawing.header.dimensioning_arrow_size = dim.arrow_size;
    drawing.header.dimension_extension_line_offset = dim.ext_offset;
    drawing.header.dimension_extension_line_extension = dim.ext_beyond;
    drawing.header.dimension_line_gap = dim.gap;
    drawing.header.dimension_unit_tolerance_decimal_places = dim.decimals as i16;
    drawing.header.insertion_base = to_dxf(doc.insbase());
    let (min, max) = doc.limits();
    drawing.header.minimum_drawing_limits = to_dxf(min);
    drawing.header.maximum_drawing_limits = to_dxf(max);
    drawing.header.use_limits_checking = doc.limits_check();
    drawing.header.fill_mode_on = doc.fill();
    drawing.header.use_quick_text_mode = doc.quick_text();
    write_ucs(&mut drawing, doc);
    write_units(&mut drawing.header, doc.units());
    write_text_styles(&mut drawing, doc);
    // The XDATA applications used: registered, as AutoCAD (and DWG) need.
    let mut apps = Vec::new();
    if doc.all_entities().any(|e| e.hyperlink.is_some()) {
        apps.push("PE_URL");
    }
    if doc.layers().iter().any(|l| l.transparency > 0) {
        apps.push(TRANSPARENCY_APP);
    }
    if doc.all_entities().any(|e| e.annotative.is_some()) {
        apps.push(ANNO_APP);
    }
    let dims: Vec<_> = doc.all_entities().flat_map(|e| dim_xdata::xdata(&e.geometry)).collect();
    for x in &dims {
        if !apps.contains(&x.application_name.as_str()) {
            apps.push(if x.application_name == "ACAD" { "ACAD" } else { dim_xdata::APP });
        }
    }
    for name in apps {
        if !drawing.app_ids().any(|a| a.name == name) {
            drawing.add_app_id(dxf::tables::AppId { name: name.into(), ..Default::default() });
        }
    }
    let current = doc.current_props();
    drawing.header.current_entity_color = to_dxf_color(current.color);
    drawing.header.current_entity_line_type = linetype_to_dxf(&current.linetype);
    drawing.header.point_display_mode = doc.pdmode();
    drawing.header.point_display_size = doc.pdsize();
    drawing.header.attribute_visibility = match doc.attdisp() {
        AttributeDisplay::Off => AttributeVisibility::None,
        AttributeDisplay::Normal => AttributeVisibility::Normal,
        AttributeDisplay::On => AttributeVisibility::All,
    };
    // Define every linetype loaded or in use, in any space (Drawing::new()
    // has ByLayer, ByBlock, Continuous).
    let used = doc
        .own_linetypes()
        .into_iter()
        .chain(std::iter::once(doc.current_props().linetype.clone()))
        .chain(doc.layers().iter().map(|l| l.linetype.clone()))
        .chain(doc.all_entities().map(|e| e.linetype.clone()))
        .chain(doc.blocks().flat_map(|b| b.entities.iter().map(|e| e.linetype.clone())));
    let mut defined: Vec<String> = Vec::new();
    // What came from xrefs is read from them again, so it isn't saved.
    for name in used.filter(|n| !is_dependent(n)) {
        let Some(lt) = doc.linetype(&name) else { continue };
        if lt.pattern.is_empty() || defined.iter().any(|d| d.eq_ignore_ascii_case(&lt.name)) {
            continue;
        }
        defined.push(lt.name.clone());
        drawing.add_line_type(dxf::tables::LineType {
            name: lt.name.clone(),
            description: lt.description.clone(),
            alignment_code: i32::from(b'A'),
            element_count: lt.pattern.len() as i32,
            total_pattern_length: lt.length(),
            dash_dot_space_lengths: lt.pattern.clone(),
            ..Default::default()
        });
    }
    // An attached drawing's layers are kept, marked as coming from it, so
    // turning them off or freezing them here lasts (AutoCAD's VISRETAIN).
    let from_xref = |name: &str| name.split_once('|').is_some_and(|(x, _)| doc.xref(x).is_some());
    for l in doc.layers().iter().filter(|l| !is_dependent(&l.name) || from_xref(&l.name)) {
        let known = doc.linetype(&l.linetype).is_some() && !is_dependent(&l.linetype);
        let line_type_name = if known { l.linetype.clone() } else { "Continuous".into() };
        // A layer's transparency is XDATA, as AutoCAD keeps it.
        let x_data = if l.transparency > 0 {
            let value = oxidraft_model::Transparency::Percent(l.transparency).to_dxf();
            vec![dxf::XData { application_name: TRANSPARENCY_APP.into(), items: vec![dxf::XDataItem::Long(value)] }]
        } else {
            Vec::new()
        };
        if l.name == "0" {
            // Drawing::new() already has layer 0.
            if let Some(zero) = drawing.layers_mut().find(|z| z.name == "0") {
                zero.line_type_name = line_type_name;
                zero.x_data = x_data;
            }
            continue;
        }
        drawing.add_layer(DxfLayer {
            name: l.name.clone(),
            color: DxfColor::from_index(l.color),
            line_type_name,
            is_layer_on: l.on,
            x_data,
            ..Default::default()
        });
    }
    for v in doc.views().iter().filter(|v| v.space == oxidraft_model::MODEL) {
        drawing.add_view(dxf::tables::View {
            name: v.name.clone(),
            view_center_point: DxfPoint::new(v.center.x, v.center.y, 0.0),
            view_height: v.height,
            view_width: v.width,
            ..Default::default()
        });
    }
    let mut ex = Exporter { images: underlay_dxf::add_definitions(&mut drawing, doc), ..Default::default() };
    let mut blocks = Vec::new();
    // Clipped references in blocks: (block, index), for their filters once
    // they have handles.
    let mut block_clips: Vec<(String, usize, &Geometry)> = Vec::new();
    for b in doc.blocks().filter(|b| !is_dependent(&b.name)) {
        // An attached drawing is saved as its path; its contents stay in its own file.
        if let Some(x) = doc.xref(&b.name) {
            let mut block = dxf::Block {
                name: b.name.clone(),
                base_point: to_dxf(b.base),
                xref_path_name: x.path.clone(),
                ..Default::default()
            };
            block.set_is_xref(true);
            block.set_is_xref_overlay(x.overlay);
            blocks.push(block);
            continue;
        }
        let mut entities = Vec::new();
        for be in &b.entities {
            for de in ex.entity(&be.geometry, &be.layer, be.color, &be.linetype, be.lineweight) {
                if is_clipped(&de, &be.geometry) {
                    block_clips.push((b.name.clone(), entities.len(), &be.geometry));
                }
                entities.push(de);
            }
        }
        blocks.push(dxf::Block { name: b.name.clone(), base_point: to_dxf(b.base), entities, ..Default::default() });
    }
    // Objects are written in draw order, which is the order AutoCAD draws
    // them in when there's no SORTENTSTABLE.
    let mut entities: Vec<(DxfEntity, &Geometry, EntityId)> = Vec::new();
    let (mut styles, mut block_styles) = (Vec::new(), Vec::new());
    // Where objects in *Paper_SpaceN blocks went: (block, index, object).
    let mut in_blocks: Vec<(String, usize, EntityId)> = Vec::new();
    for e in doc.space_in_draw_order(MODEL) {
        for mut de in ex.entity(&e.geometry, &e.layer, e.color, &e.linetype, e.lineweight) {
            de.common.transparency = e.transparency.to_dxf();
            de.common.x_data.extend(hyperlink_xdata(e));
            de.common.x_data.extend(dim_xdata::xdata(&e.geometry));
            de.common.x_data.extend(anno_xdata(e));
            add_attributes(&mut drawing, &mut de, &e.geometry, &mut styles);
            entities.push((de, &e.geometry, e.id));
        }
    }
    // Layouts, as AutoCAD keeps them: the first one's objects in ENTITIES,
    // marked paper space; the others' in their *Paper_SpaceN blocks. Their
    // LAYOUT objects and viewports are added where the file is finished.
    for (i, layout) in doc.layouts().into_iter().enumerate() {
        let name = format!("*Paper_Space{}", i.saturating_sub(1));
        let mut in_block = Vec::new();
        for e in doc.space_in_draw_order(layout.id) {
            for mut de in ex.entity(&e.geometry, &e.layer, e.color, &e.linetype, e.lineweight) {
                de.common.transparency = e.transparency.to_dxf();
                de.common.x_data.extend(hyperlink_xdata(e));
                de.common.x_data.extend(dim_xdata::xdata(&e.geometry));
                de.common.x_data.extend(anno_xdata(e));
                if i == 0 {
                    de.common.is_in_paper_space = true;
                    add_attributes(&mut drawing, &mut de, &e.geometry, &mut styles);
                    entities.push((de, &e.geometry, e.id));
                } else {
                    add_attributes(&mut drawing, &mut de, &e.geometry, &mut block_styles);
                    if is_clipped(&de, &e.geometry) {
                        block_clips.push((name.clone(), in_block.len(), &e.geometry));
                    }
                    in_blocks.push((name.clone(), in_block.len(), e.id));
                    in_block.push(de);
                }
            }
        }
        if i > 0 {
            blocks.push(dxf::Block { name, entities: in_block, ..Default::default() });
        }
    }
    for b in blocks.into_iter().chain(ex.blocks) {
        drawing.add_block(b);
    }
    for (name, index, g) in block_clips {
        let handle = drawing.blocks().find(|b| b.name == name).and_then(|b| b.entities.get(index));
        let Some(handle) = handle.map(|e| e.common.handle) else { continue };
        let group = clip_filter(&mut drawing, doc, handle, g);
        let entity = drawing.blocks_mut().find(|b| b.name == name).and_then(|b| b.entities.get_mut(index));
        if let (Some(e), Some(group)) = (entity, group) {
            e.common.extension_data_groups.push(group);
        }
    }
    let mut handles: HashMap<EntityId, Vec<u64>> = HashMap::new();
    for (name, index, id) in in_blocks {
        let handle =
            drawing.blocks().find(|b| b.name == name).and_then(|b| b.entities.get(index)).map(|e| e.common.handle);
        if let Some(h) = handle {
            handles.entry(id).or_default().push(h.0);
        }
    }
    for (e, g, id) in entities {
        let clipped = is_clipped(&e, g);
        let handle = drawing.add_entity(e).common.handle;
        handles.entry(id).or_default().push(handle.0);
        if clipped
            && let Some(group) = clip_filter(&mut drawing, doc, handle, g)
            && let Some(added) = drawing.entities_mut().find(|x| x.common.handle == handle)
        {
            added.common.extension_data_groups.push(group);
        }
    }
    // The ATTRIBs in the BLOCKS section come before those in ENTITIES.
    block_styles.extend(styles);
    (drawing, ex.hatches, block_styles, handles)
}

fn is_clipped(de: &DxfEntity, g: &Geometry) -> bool {
    matches!((&de.specific, g), (EntityType::Insert(_), Geometry::Insert { clip: Some(_), .. }))
}

/// Adds the SPATIAL_FILTER of the clipped INSERT `handle` (drawn from `g`),
/// and returns the group that points the INSERT at it.
fn clip_filter(
    drawing: &mut Drawing,
    doc: &Document,
    handle: dxf::Handle,
    g: &Geometry,
) -> Option<dxf::ExtensionGroup> {
    let Geometry::Insert { block, clip: Some(c), .. } = g else { return None };
    Some(clip_dxf::add_filter(drawing, handle, c, doc.block_extents(block)))
}

/// Converts one DXF entity, or `None` for types Oxidraft doesn't support yet.
/// A WIPEOUT: its boundary, given in the unit image's pixels (from its
/// center, y down) and placed by its corner and vectors.
fn wipeout_from_dxf(w: &dxf::entities::Wipeout) -> Option<Geometry> {
    let (o, u, v) =
        (from_dxf(&w.location), Point::new(w.u_vector.x, w.u_vector.y), Point::new(w.v_vector.x, w.v_vector.y));
    let (sx, sy) = (w.image_size.x.max(1.0), w.image_size.y.max(1.0));
    let place = |p: &DxfPoint| o + u * (p.x + 0.5) + v * (sy - (p.y + 0.5));
    let mut points: Vec<Point> = w.clipping_vertices.iter().map(place).collect();
    // A rectangle is given by two corners.
    if points.len() == 2 {
        let (a, b) = (points[0], points[1]);
        points = vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)];
    }
    if points.len() > 3 && points[0].distance(points[points.len() - 1]) < 1e-9 {
        points.pop();
    }
    if points.len() < 3 {
        // No boundary: the whole unit image.
        points = vec![o, o + u * sx, o + u * sx + v * sy, o + v * sy];
    }
    Some(Geometry::Wipeout { points })
}

/// A wipeout as a DXF WIPEOUT: a unit image over its bounds, clipped to
/// its boundary.
fn wipeout_to_dxf(points: &[Point]) -> Option<EntityType> {
    let &first = points.first()?;
    let b = points
        .iter()
        .fold(oxidraft_geom::Aabb::from_points(first, first), |b, p| b.union(oxidraft_geom::Aabb::from_points(*p, *p)));
    let (w, h) = (b.width().max(1e-9), b.height().max(1e-9));
    let local = |p: &Point| DxfPoint::new((p.x - b.min.x) / w - 0.5, 0.5 - (p.y - b.min.y) / h, 0.0);
    let mut vertices: Vec<DxfPoint> = points.iter().map(local).collect();
    vertices.push(local(&first));
    Some(EntityType::Wipeout(dxf::entities::Wipeout {
        location: DxfPoint::new(b.min.x, b.min.y, 0.0),
        u_vector: dxf::Vector::new(w, 0.0, 0.0),
        v_vector: dxf::Vector::new(0.0, h, 0.0),
        image_size: dxf::Vector::new(1.0, 1.0, 0.0),
        display_options_flags: 7,
        use_clipping: true,
        clipping_type: dxf::enums::ImageClippingBoundaryType::Polygonal,
        clipping_vertex_count: vertices.len() as i32,
        clipping_vertices: vertices,
        ..Default::default()
    }))
}

fn geometry_from_dxf(specific: &EntityType) -> Option<Geometry> {
    Some(match specific {
        EntityType::Wipeout(w) => return wipeout_from_dxf(w),
        EntityType::ModelPoint(p) => Geometry::Point { position: from_dxf(&p.location) },
        EntityType::Line(l) => Geometry::Line { start: from_dxf(&l.p1), end: from_dxf(&l.p2) },
        EntityType::Circle(c) => Geometry::Circle { center: from_dxf(&c.center), radius: c.radius },
        EntityType::Arc(a) => Geometry::Arc {
            center: from_dxf(&a.center),
            radius: a.radius,
            start_angle: a.start_angle,
            end_angle: a.end_angle,
        },
        // Vertices without widths of their own take the polyline's.
        EntityType::LwPolyline(pl) => Geometry::Polyline {
            vertices: pl
                .vertices
                .iter()
                .map(|v| {
                    let (s, e) = if v.starting_width == 0.0 && v.ending_width == 0.0 {
                        (pl.constant_width, pl.constant_width)
                    } else {
                        (v.starting_width, v.ending_width)
                    };
                    Vertex::new(Point::new(v.x, v.y), v.bulge).with_widths(s, e)
                })
                .collect(),
            closed: pl.is_closed(),
        },
        // Old-style (R12) 2D polylines.
        EntityType::Polyline(pl) => Geometry::Polyline {
            vertices: pl
                .vertices()
                .map(|v| {
                    let (s, e) = if v.starting_width == 0.0 && v.ending_width == 0.0 {
                        (pl.default_starting_width, pl.default_ending_width)
                    } else {
                        (v.starting_width, v.ending_width)
                    };
                    Vertex::new(from_dxf(&v.location), v.bulge).with_widths(s, e)
                })
                .collect(),
            closed: pl.is_closed(),
        },
        EntityType::Ellipse(el) => {
            let full = (el.end_parameter - el.start_parameter).abs() >= std::f64::consts::TAU - 1e-9
                || el.end_parameter == el.start_parameter;
            let (mut start, mut end) =
                if full { (0.0, 360.0) } else { (el.start_parameter.to_degrees(), el.end_parameter.to_degrees()) };
            // Seen from below (extrusion -Z), the parameter runs clockwise.
            if el.normal.z < 0.0 && !full {
                (start, end) = (-end, -start);
            }
            let e = Ellipse {
                center: from_dxf(&el.center),
                major: from_dxf_vector(&el.major_axis),
                ratio: el.minor_axis_ratio,
                start: if full { 0.0 } else { start.rem_euclid(360.0) },
                end: if full { 360.0 } else { end.rem_euclid(360.0) },
            };
            if e.a() <= 0.0 || e.ratio <= 0.0 {
                return None;
            }
            // A ratio over 1 isn't valid DXF; read it as the other axis being the major one.
            Geometry::Ellipse(if e.ratio > 1.0 { e.with_axes(e.a(), e.b()) } else { e })
        }
        // Splines without control points are rebuilt from their fit points.
        EntityType::Spline(sp) => {
            let control: Vec<Point> = sp.control_points.iter().map(from_dxf).collect();
            let fit: Vec<Point> = sp.fit_points.iter().map(from_dxf).collect();
            let weights = if sp.is_rational() && sp.weight_values.len() == control.len() {
                sp.weight_values.clone()
            } else {
                Vec::new()
            };
            let degree = usize::try_from(sp.degree_of_curve).unwrap_or(0);
            let spline = Spline::new(degree, control, sp.knot_values.clone(), weights, false)
                .map(|s| s.with_fit(fit.clone()))
                .or_else(|| Spline::through(&fit, sp.is_closed()))?;
            let ends_meet =
                spline.start_point().distance(spline.end_point()) <= 1e-9_f64.max(1e-9 * spline.end_point().length());
            Geometry::Spline(spline.with_closed(sp.is_closed() && ends_meet))
        }
        EntityType::XLine(x) => {
            Geometry::Xline { base: from_dxf(&x.first_point), direction: from_dxf_vector(&x.unit_direction_vector) }
        }
        EntityType::Ray(r) => {
            Geometry::Ray { base: from_dxf(&r.start_point), direction: from_dxf_vector(&r.unit_direction_vector) }
        }
        EntityType::Text(t) => {
            let (attach, second) = text_justification(t.horizontal_text_justification, t.vertical_text_justification);
            Geometry::Text {
                look: oxidraft_model::TextLook {
                    style: style_from_dxf(&t.text_style_name),
                    width_factor: if t.relative_x_scale_factor > 0.0 { t.relative_x_scale_factor } else { 1.0 },
                    oblique: oblique_from_dxf(t.oblique_angle),
                },
                position: from_dxf(if second { &t.second_alignment_point } else { &t.location }),
                height: t.text_height,
                rotation: t.rotation,
                width: 0.0,
                attach,
                content: decode_control_codes(&t.value),
            }
        }
        // Attribute definitions: in blocks, what references ask for (or,
        // constant, show); elsewhere drawn as their tags.
        EntityType::AttributeDefinition(a) => {
            let (attach, second) = text_justification(a.horizontal_text_justification, a.vertical_text_justification);
            Geometry::AttributeDefinition {
                tag: a.text_tag.clone(),
                prompt: a.prompt.clone(),
                default: decode_control_codes(&a.value),
                position: from_dxf(if second { &a.second_alignment_point } else { &a.location }),
                height: a.text_height,
                rotation: a.rotation,
                attach,
                invisible: a.is_invisible(),
                constant: a.is_constant(),
            }
        }
        EntityType::MText(m) => {
            let x = &m.x_axis_direction;
            let rotation =
                if x.x.hypot(x.y) > 1e-9 { x.y.atan2(x.x).to_degrees() } else { m.rotation_angle.to_degrees() };
            // Its width factor and slant are its style's (see `restyle_mtext`).
            Geometry::Text {
                look: oxidraft_model::TextLook { style: style_from_dxf(&m.text_style_name), ..Default::default() },
                position: from_dxf(&m.insertion_point),
                height: m.initial_text_height,
                rotation,
                width: m.reference_rectangle_width,
                attach: from_dxf_attach(m.attachment_point),
                content: decode_control_codes(&(m.extended_text.concat() + &m.text)),
            }
        }
        EntityType::RotatedDimension(d) => {
            let base = &d.dimension_base;
            let aligned = matches!(base.dimension_type, DimensionType::Aligned);
            let (p1, p2) = (from_dxf(&d.definition_point_2), from_dxf(&d.definition_point_3));
            let readable =
                oxidraft_model::annotation::readable(if aligned { p1.angle_to(p2) } else { d.rotation_angle });
            let some = |a: f64| (a.abs() > 1e-9).then_some(a);
            Geometry::Dimension {
                extra: Box::new(oxidraft_model::annotation::DimExtra {
                    text_at: base.is_at_user_defined_location.then(|| from_dxf(&base.text_mid_point)),
                    text_angle: some(base.text_rotation_angle).map(|a| readable + a),
                    oblique: some(d.extension_line_angle),
                    ..Default::default()
                }),
                p1,
                p2,
                line: from_dxf(&base.definition_point_1),
                rotation: d.rotation_angle,
                aligned,
                text: dimension_override(base),
                scale: 1.0,
            }
        }
        // Its height is the dimension style's text height.
        EntityType::Tolerance(t) => {
            let v = &t.direction_vector;
            Geometry::Tolerance {
                position: from_dxf(&t.insertion_point),
                height: oxidraft_model::annotation::dim_style().text_height,
                rotation: if v.x.hypot(v.y) > 1e-9 { v.y.atan2(v.x).to_degrees() } else { 0.0 },
                content: t.display_text.clone(),
            }
        }
        EntityType::OrdinateDimension(d) => Geometry::OrdinateDimension {
            feature: from_dxf(&d.definition_point_2),
            leader_end: from_dxf(&d.definition_point_3),
            x_datum: d.dimension_base.is_ordinate_x_type,
            text: dimension_override(&d.dimension_base),
            scale: 1.0,
        },
        EntityType::RadialDimension(d) => Geometry::RadialDimension {
            center: from_dxf(&d.dimension_base.definition_point_1),
            point: from_dxf(&d.definition_point_2),
            text_at: from_dxf(&d.dimension_base.text_mid_point),
            diameter: false,
            text: dimension_override(&d.dimension_base),
            scale: 1.0,
        },
        EntityType::DiameterDimension(d) => {
            let point = from_dxf(&d.definition_point_2);
            Geometry::RadialDimension {
                center: point.midpoint(from_dxf(&d.dimension_base.definition_point_1)),
                point,
                text_at: from_dxf(&d.dimension_base.text_mid_point),
                diameter: true,
                text: dimension_override(&d.dimension_base),
                scale: 1.0,
            }
        }
        EntityType::AngularThreePointDimension(d) => {
            let base = &d.dimension_base;
            let (p2, p3, p4) =
                (from_dxf(&d.definition_point_2), from_dxf(&d.definition_point_3), from_dxf(&d.definition_point_4));
            // Two-line angular dimensions give both lines and the arc location;
            // three-point ones the two ends, the vertex and the arc location.
            let (vertex, p1, p2, arc) = if matches!(base.dimension_type, DimensionType::Angular) {
                let arc = from_dxf(&d.definition_point_5);
                let (vertex, p1, p2) = two_line_angle((p2, p3), (p4, from_dxf(&base.definition_point_1)), arc)?;
                (vertex, p1, p2, arc)
            } else {
                let arc = from_dxf(&base.definition_point_1);
                let (p1, p2) = angle_around(p4, p2, p3, arc);
                (p4, p1, p2, arc)
            };
            Geometry::AngularDimension { vertex, p1, p2, arc, text: dimension_override(base), scale: 1.0 }
        }
        // A 2D SOLID: its corners go 1-2-4-3 around; a triangle repeats the third.
        EntityType::Solid(s) => {
            let (a, b, c, d) = (
                from_dxf(&s.first_corner),
                from_dxf(&s.second_corner),
                from_dxf(&s.third_corner),
                from_dxf(&s.fourth_corner),
            );
            let corners = if c.approx_eq(d) { vec![a, b, c] } else { vec![a, b, d, c] };
            let loops = vec![corners.into_iter().map(Vertex::at).collect()];
            Geometry::Hatch { origin: Default::default(), loops, pattern: HatchPattern::Solid, scale: 1.0, angle: 0.0 }
        }
        EntityType::Leader(l) if l.vertices.len() >= 2 => Geometry::Leader {
            more: Vec::new(),
            points: l.vertices.iter().map(from_dxf).collect(),
            content: String::new(),
            scale: 1.0,
        },
        // Non-uniform scales are approximated by the Y scale.
        EntityType::Insert(i) => Geometry::Insert {
            block: i.name.clone(),
            placement: Placement {
                position: from_dxf(&i.location),
                scale: if i.y_scale_factor != 0.0 { i.y_scale_factor.abs() } else { i.x_scale_factor.abs() },
                rotation: i.rotation,
                mirror: (i.x_scale_factor < 0.0) != (i.y_scale_factor < 0.0),
            },
            attributes: i.attributes().map(attribute_from_dxf).collect(),
            clip: None,
        },
        _ => return None,
    })
}

/// A dimension's text override; empty and `<>` mean the measurement.
fn dimension_override(base: &dxf::entities::DimensionBase) -> Option<String> {
    Some(base.text.clone()).filter(|t| !t.is_empty() && t != "<>")
}

/// Nothing read is only a template now: attribute definitions are
/// objects of their own.
fn is_template(_e: &EntityType) -> bool {
    false
}

/// Builds a document from a DXF drawing. Attributes take their reference's
/// layer; [`read`] and [`load_file`] also recover their own layer and color.
pub fn from_drawing(drawing: &Drawing) -> Imported {
    from_drawing_styled(drawing, Raw::default())
}

/// What the dxf crate doesn't read, scanned from the file's text.
#[derive(Default)]
struct Raw {
    /// Each ATTRIB's (layer, color), in file order.
    attributes: Option<Vec<(String, Color)>>,
    /// Each layer's flags (group 70).
    layer_flags: Vec<(String, i16)>,
    hatches: Vec<hatch_dxf::RawHatch>,
    /// XCLIP boundaries, by the extension dictionary of their INSERT.
    clips: HashMap<u64, oxidraft_model::XClip>,
    /// Layouts' page setups, and viewports.
    layouts: layout_dxf::RawLayouts,
}

/// WIPEOUT boundaries as the file has them, to put right what the dxf
/// crate read: by handle, or else in file order.
struct RawWipeouts<'a> {
    all: &'a [(u64, Vec<Point>)],
    next: usize,
}

impl RawWipeouts<'_> {
    fn mend(&mut self, g: &mut Geometry, handle: u64) {
        let Geometry::Wipeout { points } = g else { return };
        let found = self.all.iter().position(|(h, _)| *h == handle && handle != 0).unwrap_or(self.next);
        if let Some((_, raw)) = self.all.get(found).filter(|(_, p)| p.len() >= 3) {
            *points = raw.clone();
        }
        self.next = found + 1;
    }
}

/// An INSERT's clip from the file, if it has one.
fn attach_clip(g: &mut Geometry, e: &DxfEntity, clips: &HashMap<u64, oxidraft_model::XClip>) {
    if let Geometry::Insert { clip, .. } = g
        && let Some(c) = clip_dxf::xdictionary_of(&e.common.extension_data_groups).and_then(|h| clips.get(&h))
    {
        *clip = Some(Box::new(c.clone()));
    }
}

/// Like [`from_drawing`], with what the crate doesn't read.
fn from_drawing_styled(drawing: &Drawing, raw: Raw) -> Imported {
    let mut doc = Document::new();
    for lt in drawing.line_types() {
        let special = ["BYLAYER", "BYBLOCK", "CONTINUOUS"].iter().any(|n| lt.name.eq_ignore_ascii_case(n));
        if !special && !lt.dash_dot_space_lengths.is_empty() {
            doc.define_linetype(Linetype {
                name: lt.name.clone(),
                description: lt.description.clone(),
                pattern: lt.dash_dot_space_lengths.clone(),
            });
        }
    }
    if drawing.header.line_type_scale > 0.0 {
        doc.set_ltscale(drawing.header.line_type_scale);
    }
    doc.set_insbase(from_dxf(&drawing.header.insertion_base));
    let h = &drawing.header;
    doc.set_limits(from_dxf(&h.minimum_drawing_limits), from_dxf(&h.maximum_drawing_limits));
    doc.set_limits_check(h.use_limits_checking);
    doc.set_fill(h.fill_mode_on);
    doc.set_quick_text(h.use_quick_text_mode);
    read_ucs(&mut doc, drawing);
    doc.set_units(read_units(&drawing.header));
    read_text_styles(&mut doc, drawing);
    doc.set_current_props(oxidraft_model::CurrentProps {
        color: from_dxf_color(&drawing.header.current_entity_color),
        linetype: linetype_from_dxf(&drawing.header.current_entity_line_type),
        lineweight: lineweight_from_dxf(drawing.header.new_object_line_weight.raw_value()),
        transparency: oxidraft_model::Transparency::ByLayer,
    });
    let h = &drawing.header;
    let positive = |v: f64, default: f64| if v > 0.0 && v.is_finite() { v } else { default };
    let d = oxidraft_model::annotation::DEFAULT_DIM_STYLE;
    doc.set_dim_style(oxidraft_model::annotation::DimStyle {
        text_height: positive(h.dimensioning_text_height, d.text_height),
        arrow_size: positive(h.dimensioning_arrow_size, d.arrow_size),
        ext_offset: h.dimension_extension_line_offset.max(0.0),
        ext_beyond: h.dimension_extension_line_extension.max(0.0),
        gap: h.dimension_line_gap.abs(),
        decimals: h.dimension_unit_tolerance_decimal_places.clamp(0, 8) as usize,
    });
    doc.set_point_style(drawing.header.point_display_mode, drawing.header.point_display_size);
    doc.set_attdisp(match drawing.header.attribute_visibility {
        AttributeVisibility::None => AttributeDisplay::Off,
        AttributeVisibility::Normal => AttributeDisplay::Normal,
        AttributeVisibility::All => AttributeDisplay::On,
    });
    // Named views (model space's: the dxf crate doesn't say which space).
    for v in drawing.views() {
        let view = oxidraft_model::NamedView {
            name: v.name.clone(),
            center: Point::new(v.view_center_point.x, v.view_center_point.y),
            height: v.view_height,
            width: v.view_width,
            space: oxidraft_model::MODEL,
        };
        let _ = doc.save_view(view);
    }
    for l in drawing.layers() {
        let color = l.color.index().filter(|&i| i > 0).unwrap_or(7);
        let mut layer = Layer::new(l.name.clone(), color);
        layer.on = l.is_layer_on;
        if !l.line_type_name.is_empty() {
            layer.linetype = l.line_type_name.clone();
        }
        let weight = l.line_weight.raw_value();
        layer.lineweight = if weight >= 0 { Lineweight::Hundredths(weight as u16) } else { Lineweight::Default };
        let transparency = l.x_data.iter().filter(|x| x.application_name == TRANSPARENCY_APP).flat_map(|x| &x.items);
        for item in transparency {
            if let dxf::XDataItem::Long(v) = item
                && let oxidraft_model::Transparency::Percent(p) = oxidraft_model::Transparency::from_dxf(*v)
            {
                layer.transparency = p;
            }
        }
        // Frozen (1) and locked (4) are flags the dxf crate doesn't read.
        if let Some((_, flags)) = raw.layer_flags.iter().find(|(name, _)| name.eq_ignore_ascii_case(&l.name)) {
            layer.frozen = flags & 1 != 0;
            layer.locked = flags & 4 != 0;
        }
        doc.add_layer(layer.clone());
        doc.update_layer(layer);
    }
    // Layouts, each a space of its own: their paper space objects are found
    // by their block records, "*Paper_Space" being the active layout's.
    let mut records: HashMap<u64, String> =
        drawing.block_records().map(|r| (r.handle.0, r.name.to_ascii_uppercase())).collect();
    records.extend(raw.layouts.records.iter().map(|(h, n)| (*h, n.clone())));
    let mut layouts: Vec<&dxf::objects::Layout> = drawing
        .objects()
        .filter_map(|o| match &o.specific {
            dxf::objects::ObjectType::Layout(l) if !l.layout_name.eq_ignore_ascii_case("Model") => Some(l),
            _ => None,
        })
        .collect();
    layouts.sort_by_key(|l| l.tab_order);
    let mut layout_spaces: HashMap<String, Space> = HashMap::new();
    let mut first_layout = None;
    for l in layouts {
        let paper = raw.layouts.papers.get(&l.layout_name.to_ascii_uppercase()).cloned().unwrap_or_default();
        let layout =
            oxidraft_model::layout::Layout { id: MODEL, name: l.layout_name.clone(), tab_order: l.tab_order, paper };
        let space = doc.add_layout(layout);
        first_layout.get_or_insert(space);
        let handle = raw.layouts.layout_records.get(&l.layout_name.to_ascii_uppercase()).copied();
        if let Some(record) = records.get(&handle.unwrap_or(l.__table_record_handle.0)) {
            layout_spaces.insert(record.clone(), space);
        }
    }
    // Paper space objects in a file without layouts (R12) get one.
    let has_paper =
        drawing.entities().any(|e| e.common.is_in_paper_space) || raw.layouts.viewports.iter().any(|v| v.id != 1);
    if first_layout.is_none() && has_paper {
        let layout = oxidraft_model::layout::Layout {
            id: MODEL,
            name: "Layout1".into(),
            tab_order: 1,
            paper: oxidraft_model::Paper::default(),
        };
        first_layout = Some(doc.add_layout(layout));
    }
    let space_of_record = |name: &str| layout_spaces.get(&name.to_ascii_uppercase()).copied();
    let active = space_of_record("*PAPER_SPACE").or(first_layout);
    let space_of_owner = |owner: u64| records.get(&owner).and_then(|n| layout_spaces.get(n)).copied();
    let mut skipped = 0;
    // "*Model_Space" and "*Paper_Space" hold the drawing itself; other "*"
    // blocks (exploded hatches, dimension graphics) are kept so references
    // to them still draw.
    let is_space = |n: &str| {
        let n = n.to_ascii_lowercase();
        n.starts_with("*model_space") || n.starts_with("*paper_space")
    };
    for b in drawing.blocks().filter(|b| !is_space(&b.name)) {
        // Attached drawings keep their path; whoever opens this reads them.
        // The blocks they bring along come from them too.
        if b.is_xref() || b.is_xref_overlay() {
            doc.define_block(Block { name: b.name.clone(), base: from_dxf(&b.base_point), entities: Vec::new() });
            doc.set_xref(Xref {
                name: b.name.clone(),
                path: b.xref_path_name.clone(),
                overlay: b.is_xref_overlay(),
                status: XrefStatus::NotFound,
            });
            continue;
        }
        if is_dependent(&b.name) {
            continue;
        }
        let mut entities = Vec::new();
        for e in &b.entities {
            if is_template(&e.specific) {
                continue;
            }
            match geometry_from_dxf(&e.specific) {
                Some(mut geometry) => {
                    style_attributes(&mut geometry, None, &e.common.layer);
                    attach_clip(&mut geometry, e, &raw.clips);
                    entities.push(BlockEntity {
                        layer: e.common.layer.clone(),
                        color: from_dxf_color(&e.common.color),
                        linetype: linetype_from_dxf(&e.common.line_type_name),
                        lineweight: lineweight_from_dxf(e.common.lineweight_enum_value),
                        geometry,
                    });
                }
                None => skipped += 1,
            }
        }
        for h in raw.hatches.iter().filter(|h| h.block.as_deref() == Some(b.name.as_str())) {
            entities.push(BlockEntity {
                layer: h.layer.clone(),
                color: h.color,
                linetype: h.linetype.clone(),
                lineweight: h.lineweight,
                geometry: h.geometry.clone(),
            });
        }
        doc.define_block(Block { name: b.name.clone(), base: from_dxf(&b.base_point), entities });
    }
    // ATTRIBs follow their INSERT in the file, so the styles line up with
    // the references' attributes in order, if the counts agree.
    let attribute_count = |e: &DxfEntity| match &e.specific {
        EntityType::Insert(i) => i.attributes().count(),
        _ => 0,
    };
    let pictures = underlay_dxf::Definitions::read(drawing);
    // Draw order: by SORTENTSTABLE where it says, else by handle (else, in
    // files without handles, by place in the file).
    let mut sort_handles: HashMap<u64, u64> = HashMap::new();
    for o in drawing.objects() {
        if let dxf::objects::ObjectType::SortentsTable(t) = &o.specific {
            for (e, s) in t.__entities_handle.iter().zip(&t.__sort_items_handle) {
                sort_handles.insert(e.0, s.0);
            }
        }
    }
    let mut placed: Vec<(u64, f64, EntityId)> = Vec::new();
    let total: usize = drawing.entities().map(attribute_count).sum();
    let styles = raw.attributes.filter(|s| s.len() == total);
    let mut wipeouts = RawWipeouts { all: &raw.layouts.wipeouts, next: 0 };
    let mut next = 0;
    for (position, e) in drawing.entities().enumerate() {
        let n = attribute_count(e);
        let style = styles.as_ref().map(|s| &s[next..next + n]);
        next += n;
        // Viewports' boundaries are drawn by their viewports.
        if is_template(&e.specific) || raw.layouts.boundaries.contains(&e.common.handle.0) {
            continue;
        }
        // The active layout's objects are here too, marked paper space.
        let space = match e.common.is_in_paper_space {
            false => MODEL,
            true => match space_of_owner(e.common.__owner_handle.0).or(active) {
                Some(space) => space,
                None => continue,
            },
        };
        doc.set_space(space);
        let picture = match &e.specific {
            EntityType::Image(_) | EntityType::PdfUnderlay(_) => pictures.geometry(&e.specific, &mut doc),
            _ => None,
        };
        match picture.or_else(|| geometry_from_dxf(&e.specific)) {
            Some(mut g) => {
                wipeouts.mend(&mut g, e.common.handle.0);
                style_attributes(&mut g, style, &e.common.layer);
                attach_clip(&mut g, e, &raw.clips);
                if let Geometry::Dimension { extra, .. } = &mut g {
                    dim_xdata::read(&e.common.x_data, doc.dim_style(), extra);
                }
                let (color, linetype) = (from_dxf_color(&e.common.color), linetype_from_dxf(&e.common.line_type_name));
                let id = doc.add_with(
                    g,
                    &e.common.layer,
                    color,
                    &linetype,
                    lineweight_from_dxf(e.common.lineweight_enum_value),
                    oxidraft_model::Transparency::from_dxf(e.common.transparency),
                );
                if let Some(link) = hyperlink_from(&e.common.x_data) {
                    doc.update(id, |x| x.hyperlink = Some(link));
                }
                if let Some(a) = anno_from(&e.common.x_data) {
                    doc.update(id, |x| x.annotative = Some(a));
                }
                placed.push((e.common.handle.0, position as f64, id));
            }
            None => skipped += 1,
        }
        doc.set_space(MODEL);
    }
    // The other layouts' objects, in their *Paper_SpaceN blocks.
    for b in drawing.blocks().filter(|b| b.name.to_ascii_lowercase().starts_with("*paper_space")) {
        let Some(space) = space_of_record(&b.name) else { continue };
        doc.set_space(space);
        for e in b.entities.iter().filter(|e| !is_template(&e.specific)) {
            if raw.layouts.boundaries.contains(&e.common.handle.0) {
                continue;
            }
            let picture = match &e.specific {
                EntityType::Image(_) | EntityType::PdfUnderlay(_) => pictures.geometry(&e.specific, &mut doc),
                _ => None,
            };
            match picture.or_else(|| geometry_from_dxf(&e.specific)) {
                Some(mut g) => {
                    wipeouts.mend(&mut g, e.common.handle.0);
                    style_attributes(&mut g, None, &e.common.layer);
                    attach_clip(&mut g, e, &raw.clips);
                    if let Geometry::Dimension { extra, .. } = &mut g {
                        dim_xdata::read(&e.common.x_data, doc.dim_style(), extra);
                    }
                    let id = doc.add_with(
                        g,
                        &e.common.layer,
                        from_dxf_color(&e.common.color),
                        &linetype_from_dxf(&e.common.line_type_name),
                        lineweight_from_dxf(e.common.lineweight_enum_value),
                        oxidraft_model::Transparency::from_dxf(e.common.transparency),
                    );
                    if let Some(link) = hyperlink_from(&e.common.x_data) {
                        doc.update(id, |x| x.hyperlink = Some(link));
                    }
                    if let Some(a) = anno_from(&e.common.x_data) {
                        doc.update(id, |x| x.annotative = Some(a));
                    }
                    placed.push((e.common.handle.0, f64::from(u32::MAX), id));
                }
                None => skipped += 1,
            }
        }
        for h in raw.hatches.iter().filter(|h| h.block.as_deref() == Some(b.name.as_str())) {
            let id = doc.add_styled(h.geometry.clone(), &h.layer, h.color, &h.linetype, h.lineweight);
            placed.push((h.handle, f64::from(u32::MAX), id));
        }
        doc.set_space(MODEL);
    }
    // Viewports onto model space, in their layouts (not each layout's own,
    // which only frames the sheet).
    let layer_names: HashMap<u64, String> = drawing.layers().map(|l| (l.handle.0, l.name.clone())).collect();
    for v in raw.layouts.viewports.iter().filter(|v| v.id != 1) {
        let space = match &v.block {
            Some(block) => space_of_record(block),
            None => space_of_owner(v.owner).or(active),
        };
        let Some(space) = space else { continue };
        doc.set_space(space);
        let g = Geometry::Viewport {
            center: v.center,
            width: v.width,
            height: v.height,
            view_center: v.view_center,
            view_height: v.view_height,
            twist: v.twist,
            frozen: v.frozen.iter().filter_map(|h| layer_names.get(h).cloned()).collect(),
            locked: v.locked,
            on: v.on,
            clip: v.clip.clone(),
        };
        let id = doc.add_styled(g, &v.layer, Color::ByLayer, linetype::BYLAYER, Lineweight::ByLayer);
        placed.push((v.handle, f64::from(u32::MAX), id));
        // Its boundary is drawn by it, not as a polyline of its own.
        if v.clip.len() >= 3 {
            let boundary = doc
                .entities()
                .find(|e| match &e.geometry {
                    Geometry::Polyline { vertices, .. } => {
                        vertices.len() == v.clip.len()
                            && vertices.iter().zip(&v.clip).all(|(a, b)| a.point.distance(*b) < 1e-6)
                    }
                    Geometry::Circle { center, radius } => {
                        v.clip.len() == 64 && v.clip.iter().all(|p| (p.distance(*center) - radius).abs() < 1e-6)
                    }
                    _ => false,
                })
                .map(|e| e.id);
            if let Some(b) = boundary {
                doc.remove(b);
            }
        }
        doc.set_space(MODEL);
    }
    // Hatches are read last, then put in their place in the draw order.
    for h in raw.hatches.into_iter().filter(|h| h.block.is_none()) {
        let space = if h.paper_space { active } else { Some(MODEL) };
        let Some(space) = space else { continue };
        doc.set_space(space);
        let id = doc.add_styled(h.geometry, &h.layer, h.color, &h.linetype, h.lineweight);
        placed.push((h.handle, h.position as f64 - 0.5, id));
        doc.set_space(MODEL);
    }
    let by_handle = placed.iter().all(|p| p.0 != 0);
    // Groups: the ACAD_GROUP dictionary names them; members by handle.
    let ids_by_handle: HashMap<u64, EntityId> = placed.iter().map(|(h, _, id)| (*h, *id)).collect();
    let group_dict = raw.layouts.dictionaries.values().flatten().find(|(n, _)| n == "ACAD_GROUP").map(|(_, h)| *h);
    let names: HashMap<u64, String> = group_dict
        .and_then(|d| raw.layouts.dictionaries.get(&d))
        .map(|entries| entries.iter().map(|(n, h)| (*h, n.clone())).collect())
        .unwrap_or_default();
    let mut groups: Vec<oxidraft_model::Group> = Vec::new();
    for (handle, description, members) in &raw.layouts.groups {
        let mut ids: Vec<EntityId> = members.iter().filter_map(|h| ids_by_handle.get(h).copied()).collect();
        ids.dedup();
        if ids.is_empty() {
            continue;
        }
        let name = names.get(handle).cloned().unwrap_or_else(|| format!("*A{}", groups.len() + 1));
        groups.push(oxidraft_model::Group { name, description: description.clone(), members: ids });
    }
    doc.set_groups(groups);
    let mut order: Vec<(f64, EntityId)> = placed
        .iter()
        .map(|&(handle, position, id)| {
            let key = if by_handle { sort_handles.get(&handle).copied().unwrap_or(handle) as f64 } else { position };
            (key, id)
        })
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    doc.set_draw_order(&order.into_iter().map(|(_, id)| id).collect::<Vec<_>>());
    restyle_mtext(&mut doc);
    read_settings(&mut doc, &raw.layouts);
    // A freshly opened file starts with an empty undo history.
    doc.forget_history();
    Imported { document: doc, skipped, handles: ids_by_handle }
}

/// Sets imported attributes' layer and color: from the file when known,
/// otherwise the reference's layer with ByBlock color.
fn style_attributes(g: &mut Geometry, style: Option<&[(String, Color)]>, reference_layer: &str) {
    let Geometry::Insert { attributes, .. } = g else { return };
    for (i, a) in attributes.iter_mut().enumerate() {
        (a.layer, a.color) = match style.and_then(|s| s.get(i)) {
            Some((layer, color)) => (layer.clone(), *color),
            None => (reference_layer.to_string(), Color::ByBlock),
        };
    }
}

fn color_from_raw(v: i16) -> Color {
    match v {
        0 => Color::ByBlock,
        1..=255 => Color::Index(v as u8),
        -255..=-1 => Color::Index(v.unsigned_abs() as u8),
        _ => Color::ByLayer,
    }
}

fn color_to_raw(c: Color) -> i16 {
    match c {
        Color::ByBlock => 0,
        Color::ByLayer => 256,
        Color::Index(i) => i16::from(i),
    }
}

/// Each ATTRIB's layer (code 8) and color (code 62) in the ENTITIES section,
/// in file order. The dxf crate doesn't keep them, so they're read from the
/// text; `None` for binary DXF.
fn attribute_styles(dxf: &[u8]) -> Option<Vec<(String, Color)>> {
    if dxf.starts_with(b"AutoCAD Binary DXF") {
        return None;
    }
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let (mut section_name_next, mut in_entities) = (false, false);
    let mut current: Option<(String, Color)> = None;
    for pair in lines.chunks(2) {
        let (code, value) = (pair[0].trim(), pair.get(1).map_or("", |v| v.trim()));
        if code == "0" {
            out.extend(current.take());
            match value {
                "SECTION" => section_name_next = true,
                "ENDSEC" => in_entities = false,
                "ATTRIB" if in_entities => current = Some(("0".into(), Color::ByLayer)),
                _ => {}
            }
        } else if section_name_next && code == "2" {
            in_entities = value == "ENTITIES";
            section_name_next = false;
        } else if let Some((layer, color)) = &mut current {
            match code {
                "8" => *layer = value.to_string(),
                "62" => *color = color_from_raw(value.parse().unwrap_or(256)),
                _ => {}
            }
        }
    }
    out.extend(current);
    Some(out)
}

pub fn write<W: Write>(doc: &Document, writer: &mut W) -> Result<(), Error> {
    write_with_handles(doc, writer).map(|_| ())
}

/// Like [`write`], returning the handles each object was written with
/// (some objects are written as several entities).
pub fn write_with_handles<W: Write>(doc: &Document, writer: &mut W) -> Result<HashMap<EntityId, Vec<u64>>, Error> {
    let mut buf = Vec::new();
    let (drawing, hatches, styles, handles) = build(doc);
    drawing.save(&mut buf)?;
    // Layer flags: 1 frozen, 4 locked; 16 from an xref, 32 that xref found.
    let layers: Vec<(String, i16, i16)> = doc
        .layers()
        .iter()
        .map(|l| {
            let xref = if is_dependent(&l.name) { 48 } else { 0 };
            (l.name.clone(), i16::from(l.frozen) | (i16::from(l.locked) << 2) | xref, l.lineweight.to_dxf())
        })
        .collect();
    let fixed = fix_tables(&fix_attributes(&buf, &styles), &layers);
    // The dxf crate can only write ByLayer or ByBlock here.
    let celweight = doc.current_props().lineweight.to_dxf().to_string();
    let fixed = set_header_value(&fixed, "$CELWEIGHT", &celweight);
    let fixed = underlay_dxf::rename_type(&fixed, "UNDERLAYDEFINITION", "PDFDEFINITION");
    let fixed = clip_dxf::drop_unused_planes(&fixed);
    writer.write_all(&hatch_dxf::insert_hatches(&fixed, &hatches)).map_err(Error::IoError)?;
    Ok(handles)
}

/// The DXF with header variable `name`'s value replaced (its first value
/// line after the name).
fn set_header_value(dxf: &[u8], name: &str, value: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(at) = lines.chunks(2).position(|p| p[0].trim() == "9" && p.get(1).is_some_and(|v| v.trim() == name))
    else {
        return dxf.to_vec();
    };
    let value_line = at * 2 + 3;
    let mut out = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        if i == value_line {
            out.push_str(value);
            out.push_str(eol(line));
        } else {
            out.push_str(line);
        }
    }
    out.into_bytes()
}

/// The line ending of a code or value line.
fn eol(line: &str) -> &'static str {
    if line.ends_with("\r\n") { "\r\n" } else { "\n" }
}

/// Fills in table data the dxf crate can't write: each layer's flags
/// (frozen 1, locked 4) and lineweight, from `(name, flags, lineweight)`;
/// and a type (74, simple) after each linetype dash length, as AutoCAD writes them.
fn fix_tables(dxf: &[u8], layers: &[(String, i16, i16)]) -> Vec<u8> {
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::with_capacity(text.len() + 1024);
    let mut record = String::new();
    let mut layer: Option<&(String, i16, i16)> = None;
    for pair in lines.chunks(2) {
        let (code, value) = (pair[0].trim(), pair.get(1).map_or("", |v| v.trim()));
        let end = eol(pair[0]);
        if code == "0" {
            record = value.to_string();
            layer = None;
        }
        match (record.as_str(), code, layer) {
            ("LAYER", "2", _) => layer = layers.iter().find(|l| l.0.eq_ignore_ascii_case(value)),
            ("LAYER", "70", Some(l)) => {
                out.push_str(&format!("{}{}{end}", pair[0], l.1));
                continue;
            }
            ("LAYER", "370", Some(l)) => {
                out.push_str(&format!("{}{}{end}", pair[0], l.2));
                continue;
            }
            ("LTYPE", "49", _) => {
                pair.iter().for_each(|l| out.push_str(l));
                out.push_str(&format!(" 74{end}0{end}"));
                continue;
            }
            _ => {}
        }
        pair.iter().for_each(|l| out.push_str(l));
    }
    out.into_bytes()
}

/// Each layer's flags (group 70) from the LAYER table.
fn layer_flags(dxf: &[u8]) -> Vec<(String, i16)> {
    if dxf.starts_with(b"AutoCAD Binary DXF") {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let (mut in_layer, mut name, mut flags) = (false, None, 0);
    for pair in lines.chunks(2) {
        let (code, value) = (pair[0].trim(), pair.get(1).map_or("", |v| v.trim()));
        match code {
            "0" => {
                if let Some(n) = name.take() {
                    out.push((n, flags));
                }
                (in_layer, flags) = (value == "LAYER", 0);
            }
            "2" if in_layer => name = Some(value.to_string()),
            "70" if in_layer => flags = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    out
}

/// Repairs the dxf crate's ATTRIB output: it writes every attribute on
/// layer 0 with no color, and follows each with an empty embedded MTEXT that
/// only multi-line attributes should have (readers then see stray MTEXTs at
/// the origin). Oxidraft never writes an MTEXT right after an ATTRIB itself,
/// so any such MTEXT entity is dropped.
fn fix_attributes(dxf: &[u8], styles: &[(String, Color)]) -> Vec<u8> {
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::with_capacity(text.len());
    let (mut previous, mut skipping) = ("", false);
    let (mut attribute, mut next) = (None, 0);
    for pair in lines.chunks(2) {
        if pair[0].trim() == "0" {
            let kind = pair.get(1).map_or("", |v| v.trim());
            skipping = kind == "MTEXT" && previous == "ATTRIB";
            if !skipping {
                previous = kind;
                attribute = (kind == "ATTRIB").then(|| {
                    next += 1;
                    next - 1
                });
            }
        }
        if skipping {
            continue;
        }
        if let Some((layer, color)) = attribute.and_then(|i| styles.get(i))
            && pair[0].trim() == "8"
        {
            let eol = if pair[0].ends_with("\r\n") { "\r\n" } else { "\n" };
            out.push_str(pair[0]);
            out.push_str(&format!("{layer}{eol}"));
            if *color != Color::ByLayer {
                out.push_str(&format!(" 62{eol}{}{eol}", color_to_raw(*color)));
            }
            continue;
        }
        pair.iter().for_each(|l| out.push_str(l));
    }
    out.into_bytes()
}

pub fn read<R: BufRead>(reader: &mut R) -> Result<Imported, Error> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(Error::IoError)?;
    let bytes = underlay_dxf::rename_type(&bytes, "PDFDEFINITION", "UNDERLAYDEFINITION");
    let drawing = Drawing::load(&mut bytes.as_slice())?;
    let raw = Raw {
        attributes: attribute_styles(&bytes),
        layer_flags: layer_flags(&bytes),
        hatches: hatch_dxf::read_hatches(&bytes),
        clips: clip_dxf::read_clips(&bytes),
        layouts: layout_dxf::read(&bytes),
    };
    Ok(from_drawing_styled(&drawing, raw))
}

pub fn save_file(doc: &Document, path: impl AsRef<Path>) -> Result<(), Error> {
    let mut file = std::io::BufWriter::new(std::fs::File::create(path.as_ref()).map_err(Error::IoError)?);
    write(doc, &mut file)?;
    file.flush().map_err(Error::IoError)
}

pub fn load_file(path: impl AsRef<Path>) -> Result<Imported, Error> {
    let bytes = std::fs::read(path.as_ref()).map_err(Error::IoError)?;
    read(&mut bytes.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_and_pdf_pages_save_with_their_definitions() {
        use oxidraft_model::ImageDef;
        let mut doc = Document::new();
        let def = |name: &str, path: &str, page, width, height| ImageDef {
            name: name.into(),
            path: path.into(),
            page,
            width,
            height,
            status: XrefStatus::Loaded,
        };
        doc.set_image_def(def("ortho", "tiles\\ortho.tif", None, 400.0, 200.0));
        doc.set_image_def(def("plan - 2", "plan.pdf", Some(1), 842.0, 595.0));
        let clip = vec![Point::new(110.0, 205.0), Point::new(130.0, 205.0), Point::new(120.0, 215.0)];
        let adjust = oxidraft_model::PictureAdjust {
            brightness: 70,
            contrast: 40,
            fade: 20,
            monochrome: false,
            transparent: true,
        };
        let ortho = Geometry::Image {
            adjust,
            image: "ortho".into(),
            origin: Point::new(100.0, 200.0),
            u: Point::new(40.0, 0.0),
            v: Point::new(0.0, 20.0),
            clip: clip.clone(),
        };
        doc.add(ortho.clone());
        let pdf_adjust =
            oxidraft_model::PictureAdjust { contrast: 20, fade: 25, monochrome: true, ..Default::default() };
        let page = Geometry::Image {
            adjust: pdf_adjust,
            image: "plan - 2".into(),
            origin: Point::new(0.0, 0.0),
            u: Point::new(0.0, 297.0),
            v: Point::new(-210.0, 0.0),
            clip: Vec::new(),
        };
        doc.add(page.clone());

        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.contains("PDFDEFINITION") && !text.contains("UNDERLAYDEFINITION"));
        assert!(text.contains("ACAD_IMAGE_DICT") && text.contains("ACAD_PDFDEFINITIONS"));
        let imported = read(&mut buf.as_slice()).unwrap();
        assert_eq!(imported.skipped, 0);
        let mut back = imported.document;
        let d = back.image_def("ortho").unwrap();
        assert_eq!((d.path.as_str(), d.page, d.width, d.height), ("tiles\\ortho.tif", None, 400.0, 200.0));
        let shapes: Vec<Geometry> = back.entities().map(|e| e.geometry.clone()).collect();
        let Geometry::Image { origin, u, v, clip: c, adjust: a, .. } = &shapes[0] else { panic!() };
        assert_eq!(*a, adjust);
        assert!(matches!(&shapes[1], Geometry::Image { adjust: a, .. } if *a == pdf_adjust));
        assert!(
            origin.approx_eq(Point::new(100.0, 200.0))
                && u.approx_eq(Point::new(40.0, 0.0))
                && v.approx_eq(Point::new(0.0, 20.0))
        );
        assert!(c.len() == 3 && c.iter().zip(&clip).all(|(a, b)| a.distance(*b) < 1e-9), "{c:?}");
        // A PDF page's size comes from the PDF; until it's read, its scale stands in.
        assert_eq!(back.image_def("plan - 2").map(|d| d.page), Some(Some(1)));
        back.size_pdf("plan - 2", 842.0, 595.0);
        let Geometry::Image { u, v, .. } = &back.entities().nth(1).unwrap().geometry else { panic!() };
        assert!(u.distance(Point::new(0.0, 297.0)) < 1e-6 && v.distance(Point::new(-210.0, 0.0)) < 1e-6, "{u:?} {v:?}");
    }

    fn drawn(doc: &Document) -> Vec<Geometry> {
        doc.entities_in_draw_order().iter().map(|e| e.geometry.clone()).collect()
    }

    #[test]
    fn draw_order_round_trips_and_follows_sortentstable() {
        let mut doc = Document::new();
        let line = |x: f64| Geometry::Line { start: Point::new(x, 0.0), end: Point::new(x, 1.0) };
        let a = doc.add(line(0.0));
        doc.add(line(1.0));
        let square = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(x, y)| Vertex::at(Point::new(x, y)));
        let hatch = doc.add(Geometry::Hatch {
            origin: Default::default(),
            loops: vec![square.to_vec()],
            pattern: HatchPattern::Solid,
            scale: 1.0,
            angle: 0.0,
        });
        doc.bring_to_front(&[a]);
        doc.send_to_back(&[hatch]);
        let before = drawn(&doc);
        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        assert_eq!(drawn(&read(&mut buf.as_slice()).unwrap().document), before);

        // AutoCAD's SORTENTSTABLE: the first line sorts after the second.
        let mut drawing = Drawing::new();
        let h1 = drawing
            .add_entity(DxfEntity::new(EntityType::Line(dxf::entities::Line::new(
                to_dxf(Point::ORIGIN),
                to_dxf(Point::new(0.0, 1.0)),
            ))))
            .common
            .handle;
        let h2 = drawing
            .add_entity(DxfEntity::new(EntityType::Line(dxf::entities::Line::new(
                to_dxf(Point::new(1.0, 0.0)),
                to_dxf(Point::new(1.0, 1.0)),
            ))))
            .common
            .handle;
        let table = dxf::objects::SortentsTable {
            __entities_handle: vec![h1],
            __sort_items_handle: vec![dxf::Handle(h2.0 + 100)],
        };
        drawing.add_object(dxf::objects::Object::new(dxf::objects::ObjectType::SortentsTable(table)));
        let back = from_drawing(&drawing).document;
        let xs: Vec<f64> = back
            .entities_in_draw_order()
            .iter()
            .map(|e| match e.geometry {
                Geometry::Line { start, .. } => start.x,
                _ => f64::NAN,
            })
            .collect();
        assert_eq!(xs, vec![1.0, 0.0]);
    }

    #[test]
    fn xclip_boundaries_round_trip() {
        use oxidraft_model::{Block, BlockEntity, XClip};
        let mut doc = Document::new();
        let line = Geometry::Line { start: Point::ORIGIN, end: Point::new(10.0, 0.0) };
        doc.define_block(Block {
            name: "B".into(),
            base: Point::new(1.0, 0.0),
            entities: vec![BlockEntity::new("0", Color::ByLayer, line)],
        });
        let square = vec![Point::new(2.0, -1.0), Point::new(6.0, -1.0), Point::new(6.0, 1.0), Point::new(2.0, 1.0)];
        let clips = [
            XClip { boundary: square.clone(), inverted: false, on: true },
            XClip { boundary: square.clone(), inverted: true, on: true },
            XClip { boundary: square.clone(), inverted: false, on: false },
        ];
        let reference = |clip: Option<&XClip>, y: f64| Geometry::Insert {
            block: "B".into(),
            placement: Placement { position: Point::new(0.0, y), scale: 2.0, rotation: 30.0, mirror: false },
            attributes: Vec::new(),
            clip: clip.cloned().map(Box::new),
        };
        for (i, c) in clips.iter().enumerate() {
            doc.add(reference(Some(c), i as f64 * 10.0));
        }
        // A clipped reference inside another block.
        doc.define_block(Block {
            name: "OUTER".into(),
            base: Point::ORIGIN,
            entities: vec![BlockEntity::new("0", Color::ByLayer, reference(Some(&clips[0]), 0.0))],
        });
        doc.add(Geometry::Insert {
            block: "OUTER".into(),
            placement: Placement::at(Point::new(100.0, 0.0)),
            attributes: Vec::new(),
            clip: None,
        });
        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let back = read(&mut buf.as_slice()).unwrap().document;
        let clip_of = |g: &Geometry| match g {
            Geometry::Insert { clip, .. } => clip.as_deref().cloned(),
            _ => None,
        };
        let read: Vec<Option<XClip>> = back.entities_in_draw_order().iter().map(|e| clip_of(&e.geometry)).collect();
        assert_eq!(read[..3], clips.iter().cloned().map(Some).collect::<Vec<_>>()[..]);
        assert_eq!(read[3], None);
        let nested = &back.block("OUTER").unwrap().entities[0].geometry;
        assert_eq!(clip_of(nested), Some(clips[0].clone()));
        // What shows is the same.
        let shown = |d: &Document| -> Vec<f64> {
            d.entities_in_draw_order().iter().map(|e| d.expanded(e).iter().map(Geometry::length).sum()).collect()
        };
        assert_eq!(shown(&back), shown(&doc));
        assert!((shown(&doc)[0] - 8.0).abs() < 1e-9 && (shown(&doc)[1] - 12.0).abs() < 1e-9);
    }

    #[test]
    fn xrefs_save_as_their_path() {
        let mut doc = Document::new();
        doc.set_xref(Xref {
            name: "SITE".into(),
            path: "..\\survey\\site.dwg".into(),
            overlay: true,
            status: XrefStatus::NotFound,
        });
        let mut site = Document::new();
        site.add_layer(Layer::new("Boundary", 1));
        site.add_on_layer(
            Geometry::Line { start: Point::ORIGIN, end: Point::new(5.0, 0.0) },
            "Boundary",
            Color::ByLayer,
        );
        doc.load_xref("SITE", &site);
        doc.add(Geometry::Insert {
            block: "SITE".into(),
            placement: Placement::at(Point::new(10.0, 10.0)),
            attributes: Vec::new(),
            clip: None,
        });
        assert!(doc.layer("SITE|Boundary").is_some());

        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(!text.contains("SITE|Boundary\r\n  6\r\nSITE|"), "its linetypes aren't saved");
        let back = read(&mut buf.as_slice()).unwrap().document;
        // Its layers are, so what's set on them here lasts.
        assert!(back.layer("SITE|Boundary").is_some_and(|l| l.color == 1));
        let x = back.xref("site").unwrap();
        assert_eq!((x.path.as_str(), x.overlay, x.status), ("..\\survey\\site.dwg", true, XrefStatus::NotFound));
        assert!(back.block("SITE").is_some_and(|b| b.entities.is_empty()));
        assert_eq!(back.len(), 1);
    }

    #[test]
    fn roundtrip_ellipses_splines_construction_lines_and_widths() {
        use oxidraft_model::{Ellipse, Spline};
        let mut doc = Document::new();
        let arc =
            Ellipse { center: Point::new(1.0, 2.0), major: Point::new(3.0, 4.0), ratio: 0.5, start: 30.0, end: 300.0 };
        let fit = [(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 8.0)].map(|(x, y)| Point::new(x, y));
        let shapes = vec![
            Geometry::Ellipse(Ellipse::full(Point::ORIGIN, Point::new(5.0, 0.0), 0.25)),
            Geometry::Ellipse(arc),
            Geometry::Spline(Spline::through(&fit, false).unwrap()),
            Geometry::Spline(Spline::from_control(&fit, true).unwrap()),
            Geometry::Xline { base: Point::new(1.0, 1.0), direction: Point::new(0.6, 0.8) },
            Geometry::Ray { base: Point::new(2.0, 1.0), direction: Point::new(0.0, -1.0) },
            Geometry::Polyline {
                vertices: vec![
                    Vertex::new(Point::ORIGIN, 0.5).with_widths(1.5, 1.5),
                    Vertex::at(Point::new(4.0, 0.0)).with_widths(1.5, 1.5),
                ],
                closed: false,
            },
            Geometry::Polyline {
                vertices: vec![Vertex::at(Point::ORIGIN).with_widths(0.0, 2.0), Vertex::at(Point::new(4.0, 0.0))],
                closed: false,
            },
        ];
        for g in &shapes {
            doc.add(g.clone());
        }
        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let imported = read(&mut buf.as_slice()).unwrap();
        assert_eq!(imported.skipped, 0);
        let back: Vec<Geometry> = imported.document.entities().map(|e| e.geometry.clone()).collect();
        assert_eq!(back.len(), shapes.len());
        for (b, g) in back.iter().zip(&shapes) {
            match (b, g) {
                (Geometry::Ellipse(x), Geometry::Ellipse(y)) => {
                    assert!(
                        x.center.approx_eq(y.center) && x.major.approx_eq(y.major) && (x.ratio - y.ratio).abs() < 1e-12
                    );
                    assert!((x.start - y.start).abs() < 1e-9 && (x.sweep() - y.sweep()).abs() < 1e-9, "{x:?} vs {y:?}");
                }
                // DXF keeps 12 decimals.
                (Geometry::Spline(x), Geometry::Spline(y)) => {
                    assert_eq!(
                        (x.degree(), x.knots(), x.is_closed(), x.is_periodic()),
                        (y.degree(), y.knots(), y.is_closed(), y.is_periodic())
                    );
                    assert!(x.control().iter().zip(y.control()).all(|(p, q)| p.distance(*q) < 1e-9));
                    assert_eq!(x.fit(), y.fit());
                }
                _ => assert_eq!(b, g),
            }
        }
    }

    #[test]
    fn roundtrip_ordinate_dimensions_and_arc_ones_as_their_graphics() {
        let mut doc = Document::new();
        let ordinate = Geometry::OrdinateDimension {
            feature: Point::new(30.0, 20.0),
            leader_end: Point::new(32.0, 40.0),
            x_datum: true,
            text: None,
            scale: 1.0,
        };
        doc.add(ordinate.clone());
        doc.add(Geometry::ArcLengthDimension {
            center: Point::ORIGIN,
            radius: 10.0,
            start_angle: 0.0,
            end_angle: 90.0,
            dim_radius: 15.0,
            text: None,
            scale: 1.0,
        });
        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let back = read(&mut buf.as_slice()).unwrap().document;
        let shapes: Vec<&Geometry> = back.entities().map(|e| &e.geometry).collect();
        assert!(shapes.contains(&&ordinate), "{shapes:?}");
        // The arc length dimension's look, as a block of its parts.
        assert!(shapes.iter().any(|g| matches!(g, Geometry::Insert { .. })));
    }

    #[test]
    fn roundtrip_named_views_and_tables_as_lines_and_text() {
        let mut doc = Document::new();
        let view = oxidraft_model::NamedView {
            name: "Kitchen".into(),
            center: Point::new(12.0, 34.0),
            height: 50.0,
            width: 80.0,
            space: oxidraft_model::MODEL,
        };
        doc.save_view(view.clone()).unwrap();
        doc.add(oxidraft_model::table::new_table(Point::new(0.0, 0.0), 2, 1, 10.0, 5.0, 2.5));
        let triangle = vec![Point::new(100.0, 0.0), Point::new(140.0, 0.0), Point::new(120.0, 30.0)];
        doc.add(Geometry::Wipeout { points: triangle.clone() });
        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let back = read(&mut buf.as_slice()).unwrap().document;
        assert_eq!(back.views(), &[view]);
        let wipeout = back.entities().find_map(|e| match &e.geometry {
            Geometry::Wipeout { points } => Some(points.clone()),
            _ => None,
        });
        let wipeout = wipeout.expect("the wipeout came back");
        assert!(wipeout.len() == 3 && wipeout.iter().zip(&triangle).all(|(a, b)| a.distance(*b) < 1e-6), "{wipeout:?}");
        // Three rows: four lines across, three down; and the title.
        let lines = back.entities().filter(|e| matches!(e.geometry, Geometry::Line { .. })).count();
        let texts = back.entities().filter(|e| matches!(e.geometry, Geometry::Text { .. })).count();
        assert_eq!((lines, texts), (7, 1));
    }

    #[test]
    fn roundtrip_points_next_to_hatches_and_the_point_style() {
        let mut doc = Document::new();
        doc.set_point_style(35, -2.5);
        doc.set_attdisp(AttributeDisplay::On);
        doc.set_insbase(Point::new(7.0, 8.0));
        doc.add(Geometry::Point { position: Point::new(3.0, 4.0) });
        let square = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(x, y)| Vertex::at(Point::new(x, y)));
        let hatch = Geometry::Hatch {
            origin: Default::default(),
            loops: vec![square.to_vec()],
            pattern: HatchPattern::Solid,
            scale: 1.0,
            angle: 0.0,
        };
        doc.add(hatch.clone());
        // A real point where the first hatch placeholder would be.
        doc.add(Geometry::Point { position: Point::new(0.0, 0.0) });

        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let back = read(&mut buf.as_slice()).unwrap().document;
        // Hatches are read in a second pass, so they come back last.
        let shapes: Vec<Geometry> = back.entities().map(|e| e.geometry.clone()).collect();
        assert_eq!(
            shapes,
            vec![
                Geometry::Point { position: Point::new(3.0, 4.0) },
                Geometry::Point { position: Point::new(0.0, 0.0) },
                hatch
            ]
        );
        assert_eq!((back.pdmode(), back.pdsize(), back.attdisp()), (35, -2.5, AttributeDisplay::On));
        assert_eq!(back.insbase(), Point::new(7.0, 8.0));
    }

    #[test]
    fn roundtrip_linetypes_lineweights_and_layer_states() {
        let mut doc = Document::new();
        let mut axes = Layer::new("Axes", 1);
        (axes.linetype, axes.lineweight, axes.frozen) = ("CENTER".into(), Lineweight::Hundredths(50), true);
        doc.add_layer(axes);
        let mut fixed = Layer::new("Fixed", 2);
        fixed.locked = true;
        doc.add_layer(fixed);
        doc.define_linetype(Linetype::new("FENCE", "Fence ---x---x", &[5.0, -1.0, 0.0, -1.0]));
        doc.set_ltscale(0.5);
        let line = |y: f64| Geometry::Line { start: Point::new(0.0, y), end: Point::new(10.0, y) };
        doc.add_styled(line(0.0), "Fixed", Color::ByLayer, "FENCE", Lineweight::Hundredths(35));
        doc.add_on_layer(line(1.0), "Axes", Color::ByLayer);

        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let text = String::from_utf8_lossy(&buf).replace("\r\n", "\n");
        assert!(text.contains(" 49\n5.0\n 74\n0\n"), "each dash length has its type");
        let back = read(&mut buf.as_slice()).unwrap().document;

        let axes = back.layer("Axes").unwrap();
        assert_eq!((axes.linetype.as_str(), axes.lineweight), ("CENTER", Lineweight::Hundredths(50)));
        assert!(axes.frozen && !axes.locked);
        assert!(back.layer("Fixed").unwrap().locked);
        let styles: Vec<(String, Lineweight)> = back.entities().map(|e| (e.linetype.clone(), e.lineweight)).collect();
        assert_eq!(
            styles,
            vec![("FENCE".into(), Lineweight::Hundredths(35)), (linetype::BYLAYER.into(), Lineweight::ByLayer)]
        );
        assert_eq!(back.ltscale(), 0.5);
        assert_eq!(back.linetype("fence").unwrap().pattern, vec![5.0, -1.0, 0.0, -1.0]);
    }

    #[test]
    fn roundtrip_preserves_entities_layers_and_colors() {
        let mut doc = Document::new();
        doc.add_layer(Layer::new("Walls", 1));
        doc.add_on_layer(
            Geometry::Line { start: Point::new(0.0, 0.0), end: Point::new(10.0, 5.0) },
            "Walls",
            Color::ByLayer,
        );
        doc.add(Geometry::Circle { center: Point::new(3.0, 4.0), radius: 2.5 });
        let mut hidden = Layer::new("Hidden", 5);
        hidden.on = false;
        doc.add_layer(hidden.clone());
        doc.update_layer(hidden);
        doc.define_block(Block {
            name: "CHAIR".into(),
            base: Point::new(1.0, 1.0),
            entities: vec![BlockEntity::new(
                "0",
                Color::ByLayer,
                Geometry::Circle { center: Point::new(1.0, 1.0), radius: 0.5 },
            )],
        });
        doc.add(Geometry::Insert {
            block: "CHAIR".into(),
            placement: Placement { position: Point::new(20.0, 5.0), scale: 2.0, rotation: 30.0, mirror: true },
            attributes: vec![Attribute {
                tag: "SEAT".into(),
                value: "12".into(),
                layer: "Walls".into(),
                color: Color::Index(3),
                position: Point::new(22.0, 6.0),
                height: 1.5,
                rotation: 0.0,
                attach: Attach::MiddleCenter,
                invisible: false,
            }],
            clip: None,
        });
        doc.add(Geometry::Arc { center: Point::new(1.0, 2.0), radius: 3.0, start_angle: 30.0, end_angle: 300.0 });
        doc.add(Geometry::Polyline {
            vertices: vec![
                Vertex::at(Point::new(0.0, 0.0)),
                Vertex::new(Point::new(10.0, 0.0), 0.5),
                Vertex::at(Point::new(10.0, 10.0)),
            ],
            closed: true,
        });
        doc.add_on_layer(
            Geometry::Line { start: Point::new(1.0, 1.0), end: Point::new(2.0, 2.0) },
            "0",
            Color::Index(3),
        );

        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let imported = read(&mut buf.as_slice()).unwrap();
        let back = imported.document;

        assert_eq!(imported.skipped, 0);
        assert_eq!(back.layer("Walls").map(|l| l.color), Some(1));
        assert_eq!(back.block("CHAIR"), doc.block("CHAIR"));
        assert!(!back.layer("Hidden").unwrap().on);
        let got: Vec<_> = back.entities().map(|e| (e.layer.clone(), e.color, e.geometry.clone())).collect();
        let want: Vec<_> = doc.entities().map(|e| (e.layer.clone(), e.color, e.geometry.clone())).collect();
        assert_eq!(got, want);
        assert!(!back.can_undo(), "opening a file should not be undoable");
    }

    #[test]
    fn roundtrip_annotations_and_hatches() {
        let mut doc = Document::new();
        let mtext = Geometry::Text {
            look: Default::default(),
            position: Point::new(1.0, 2.0),
            height: 2.5,
            rotation: 30.0,
            width: 40.0,
            attach: Attach::TopLeft,
            content: "Line one\\PLine two".into(),
        };
        let single = Geometry::Text {
            look: Default::default(),
            position: Point::new(5.0, 5.0),
            height: 3.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "TITLE".into(),
        };
        let dim = Geometry::Dimension {
            extra: Default::default(),
            p1: Point::new(0.0, 0.0),
            p2: Point::new(10.0, 3.0),
            line: Point::new(5.0, 8.0),
            rotation: 0.0,
            aligned: false,
            text: Some("<> TYP".into()),
            scale: 1.0,
        };
        let aligned = Geometry::Dimension {
            extra: Default::default(),
            p1: Point::new(0.0, 0.0),
            p2: Point::new(3.0, 4.0),
            line: Point::new(-1.0, 2.0),
            rotation: 0.0,
            aligned: true,
            text: None,
            scale: 1.0,
        };
        let radius = Geometry::RadialDimension {
            center: Point::new(50.0, 50.0),
            point: Point::new(55.0, 50.0),
            text_at: Point::new(60.0, 50.0),
            diameter: false,
            text: None,
            scale: 1.0,
        };
        let diameter = Geometry::RadialDimension {
            center: Point::new(50.0, 50.0),
            point: Point::new(50.0, 55.0),
            text_at: Point::new(50.0, 52.0),
            diameter: true,
            text: Some("<> HOLE".into()),
            scale: 1.0,
        };
        let angular = Geometry::AngularDimension {
            vertex: Point::new(20.0, 0.0),
            p1: Point::new(30.0, 0.0),
            p2: Point::new(20.0, 10.0),
            arc: Point::new(25.0, 4.0),
            text: None,
            scale: 1.0,
        };
        for g in [&mtext, &single, &dim, &aligned, &radius, &diameter, &angular] {
            doc.add(g.clone());
        }
        doc.add(Geometry::Leader {
            more: Vec::new(),
            points: vec![Point::ORIGIN, Point::new(10.0, 10.0)],
            content: "NOTE".into(),
            scale: 1.0,
        });
        let square: Vec<Vertex> = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]
            .iter()
            .map(|&(x, y)| Vertex::at(Point::new(x, y)))
            .collect();
        let hatched = Geometry::Hatch {
            origin: Default::default(),
            loops: vec![square],
            pattern: HatchPattern::Ansi31,
            scale: 2.0,
            angle: 15.0,
        };
        doc.add(hatched.clone());

        let mut buf = Vec::new();
        write(&doc, &mut buf).unwrap();
        let back = read(&mut buf.as_slice()).unwrap().document;
        let got: Vec<Geometry> = back.entities().map(|e| e.geometry.clone()).collect();

        assert!(got.contains(&single));
        // DXF keeps a different point on the same dimension line, so compare
        // what the dimensions draw.
        let drawn = |g: &Geometry| format!("{:.6?}", g.primitives().unwrap());
        for want in [&dim, &aligned] {
            assert!(
                got.iter().any(|g| matches!(g, Geometry::Dimension { .. }) && drawn(g) == drawn(want)),
                "missing {want:?}"
            );
        }
        for want in [&radius, &diameter] {
            assert!(
                got.iter().any(|g| matches!(g, Geometry::RadialDimension { .. }) && drawn(g) == drawn(want)),
                "missing {want:?}"
            );
        }
        assert!(got.contains(&angular));
        let Some(Geometry::Text { rotation, width, content, .. }) =
            got.iter().find(|g| matches!(g, Geometry::Text { content, .. } if content.starts_with("Line one")))
        else {
            panic!("MTEXT lost")
        };
        assert!((rotation - 30.0).abs() < 1e-9 && *width == 40.0 && content == "Line one\\PLine two");
        // The leader keeps its arrow and lines; its text comes back as MTEXT beside it.
        assert!(got.iter().any(|g| matches!(g, Geometry::Leader { points, .. } if points.len() == 2)));
        assert!(got.iter().any(|g| matches!(g, Geometry::Text { content, .. } if content == "NOTE")));
        // Hatches come back as the same hatches, and no POINT placeholder is left.
        assert!(got.contains(&hatched), "hatch lost");
        assert!(!String::from_utf8_lossy(&buf).contains("\nPOINT"));
    }

    /// The structure of survey exports like the one reported: a block made
    /// only of attribute definitions, references whose attributes are centered
    /// on their second alignment point, ByBlock colors, and a paper-space object.
    #[test]
    fn imports_attributes_justification_and_byblock() {
        use dxf::entities::{AttributeDefinition, Circle as DxfCircle, Insert as DxfInsert};
        let mut drawing = Drawing::new();
        let mut template = AttributeDefinition { text_height: 3.0, ..Default::default() };
        template.text_tag = "NPARCEL".into();
        let mut symbol = DxfEntity::new(EntityType::Circle(DxfCircle::new(DxfPoint::new(0.0, 0.0, 0.0), 0.5)));
        symbol.common.color = DxfColor::by_block();
        drawing.add_block(dxf::Block {
            name: "PRPARC".into(),
            entities: vec![DxfEntity::new(EntityType::AttributeDefinition(template)), symbol],
            ..Default::default()
        });
        let mut insert =
            DxfInsert { name: "PRPARC".into(), location: DxfPoint::new(100.0, 200.0, 0.0), ..Default::default() };
        let mut number = dxf::entities::Attribute {
            attribute_tag: "NPARCEL".into(),
            value: "IV".into(),
            text_height: 3.0,
            location: DxfPoint::new(0.0, 0.0, 0.0),
            second_alignment_point: DxfPoint::new(100.0, 200.0, 0.0),
            horizontal_text_justification: HorizontalTextJustification::Middle,
            ..Default::default()
        };
        number.set_is_invisible(false);
        let mut creator =
            dxf::entities::Attribute { attribute_tag: "CREATOR".into(), value: "x".into(), ..Default::default() };
        creator.set_is_invisible(true);
        insert.add_attribute(&mut drawing, number);
        insert.add_attribute(&mut drawing, creator);
        let mut reference = DxfEntity::new(EntityType::Insert(insert));
        reference.common.layer = "RP_NPAR".into();
        drawing.add_entity(reference);
        let mut paper = DxfEntity::new(EntityType::Line(dxf::entities::Line::default()));
        paper.common.is_in_paper_space = true;
        drawing.add_entity(paper);
        let mut label = dxf::entities::Text { value: "45%%d".into(), text_height: 2.0, ..Default::default() };
        label.horizontal_text_justification = HorizontalTextJustification::Right;
        label.vertical_text_justification = VerticalTextJustification::Top;
        label.second_alignment_point = DxfPoint::new(5.0, 5.0, 0.0);
        drawing.add_entity(DxfEntity::new(EntityType::Text(label)));

        let imported = from_drawing(&drawing);
        assert_eq!(imported.skipped, 0, "attribute definitions and paper space aren't 'unsupported'");
        let doc = imported.document;
        assert_eq!(doc.len(), 2, "the reference and the text; the paper-space line stays out");
        // The symbol, and the attribute definition new references ask from.
        let content: Vec<&str> = doc.block("PRPARC").unwrap().entities.iter().map(|e| e.geometry.name()).collect();
        assert_eq!(content, ["Attribute Definition", "Circle"]);

        let reference = doc.entities().find(|e| matches!(e.geometry, Geometry::Insert { .. })).unwrap();
        let Geometry::Insert { attributes, .. } = &reference.geometry else { unreachable!() };
        assert_eq!(attributes.len(), 2);
        assert_eq!((attributes[0].position, attributes[0].attach), (Point::new(100.0, 200.0), Attach::MiddleCenter));
        assert!(attributes[1].invisible);
        // The ByBlock symbol and the attribute take the reference's layer color.
        let drawn = doc.expanded_styled(reference);
        assert_eq!(drawn.len(), 2, "symbol plus the visible attribute");
        assert!(matches!(&drawn[1].geometry, Geometry::Text { content, .. } if content == "IV"));

        let Some(Geometry::Text { position, attach, content, .. }) =
            doc.entities().map(|e| &e.geometry).find(|g| matches!(g, Geometry::Text { .. }))
        else {
            panic!()
        };
        assert_eq!((*position, *attach, content.as_str()), (Point::new(5.0, 5.0), Attach::TopRight, "45°"));
    }
}

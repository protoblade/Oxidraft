//! Reading and writing DXF, Oxidraft's native exchange format.

use std::io::{BufRead, Write};
use std::path::Path;

use dxf::LwPolylineVertex;
use dxf::entities::{Entity as DxfEntity, EntityType, LwPolyline};
use dxf::enums::AcadVersion;
use dxf::enums::{AttachmentPoint, DimensionType, HorizontalTextJustification, VerticalTextJustification};
use dxf::tables::Layer as DxfLayer;
use dxf::{Color as DxfColor, Drawing, Point as DxfPoint};
use oxidraft_geom::Point;
use oxidraft_model::{
    Attach, Attribute, Block, BlockEntity, Color, Document, Geometry, HatchPattern, Layer, Placement, Vertex,
    annotation::decode_control_codes, hatch,
};

pub type Error = dxf::DxfError;

/// Result of an import, with what was skipped.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    /// Number of DXF entities of types Oxidraft can't represent yet.
    pub skipped: usize,
}

fn to_dxf(p: Point) -> DxfPoint {
    DxfPoint::new(p.x, p.y, 0.0)
}

fn from_dxf(p: &DxfPoint) -> Point {
    Point::new(p.x, p.y)
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
                    e
                })
                .collect(),
            ..Default::default()
        });
        name
    }

    /// DXF entities for one object.
    fn entities(&mut self, g: &Geometry, layer: &str) -> Vec<EntityType> {
        match g {
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
                let mut pl = LwPolyline {
                    vertices: vertices
                        .iter()
                        .map(|v| LwPolylineVertex { x: v.point.x, y: v.point.y, bulge: v.bulge, ..Default::default() })
                        .collect(),
                    ..Default::default()
                };
                pl.set_is_closed(*closed);
                vec![EntityType::LwPolyline(pl)]
            }
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
            Geometry::Text { position, height, rotation, width, attach, content } => {
                let single_line = *attach == Attach::BottomLeft && !content.contains("\\P") && !content.contains('\n');
                if single_line {
                    return vec![EntityType::Text(dxf::entities::Text {
                        location: to_dxf(*position),
                        text_height: *height,
                        value: content.clone(),
                        rotation: *rotation,
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
                    ..Default::default()
                })]
            }
            Geometry::Dimension { p1, p2, line, rotation, aligned, text, .. } => {
                let parts: Vec<EntityType> =
                    g.primitives().unwrap_or_default().iter().flat_map(|p| self.entities(p, layer)).collect();
                let text_mid = g.primitives().unwrap_or_default().iter().find_map(|p| match p {
                    Geometry::Text { position, .. } => Some(*position),
                    _ => None,
                });
                let block_name = self.anonymous_block('D', parts, layer);
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
                        ..Default::default()
                    },
                    definition_point_2: to_dxf(*p1),
                    definition_point_3: to_dxf(*p2),
                    rotation_angle: if *aligned { 0.0 } else { *rotation },
                    ..Default::default()
                })]
            }
            // A LEADER for the arrow and lines, plus its text as MTEXT.
            Geometry::Leader { points, .. } => {
                let mut out = vec![EntityType::Leader(dxf::entities::Leader {
                    vertices: points.iter().map(|p| to_dxf(*p)).collect(),
                    use_arrowheads: true,
                    ..Default::default()
                })];
                for p in g.primitives().unwrap_or_default() {
                    if matches!(p, Geometry::Text { .. }) {
                        out.extend(self.entities(&p, layer));
                    }
                }
                out
            }
            Geometry::Hatch { loops, pattern, scale, angle } => {
                let polys = hatch::polygons(loops);
                let parts: Vec<EntityType> = if *pattern == HatchPattern::Solid {
                    hatch::triangles(&polys)
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
                } else {
                    hatch::pattern_lines(&polys, *pattern, *scale, *angle, 200_000)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(a, b)| EntityType::Line(dxf::entities::Line::new(to_dxf(a), to_dxf(b))))
                        .collect()
                };
                let name = self.anonymous_block('X', parts, layer);
                vec![EntityType::Insert(dxf::entities::Insert { name, ..Default::default() })]
            }
        }
    }

    fn entity(&mut self, geometry: &Geometry, layer: &str, color: Color) -> Vec<DxfEntity> {
        self.entities(geometry, layer)
            .into_iter()
            .map(|t| {
                let mut de = DxfEntity::new(t);
                de.common.layer = layer.to_string();
                de.common.color = to_dxf_color(color);
                de
            })
            .collect()
    }
}

/// Builds a DXF drawing from a document.
pub fn to_drawing(doc: &Document) -> Drawing {
    let mut drawing = Drawing::new();
    // R2000 is the oldest version with lightweight polylines.
    drawing.header.version = AcadVersion::R2000;
    for l in doc.layers() {
        if l.name == "0" {
            continue; // Drawing::new() already has layer 0
        }
        drawing.add_layer(DxfLayer {
            name: l.name.clone(),
            color: DxfColor::from_index(l.color),
            is_layer_on: l.on,
            ..Default::default()
        });
    }
    let mut ex = Exporter::default();
    let mut blocks = Vec::new();
    for b in doc.blocks() {
        let entities = b.entities.iter().flat_map(|be| ex.entity(&be.geometry, &be.layer, be.color)).collect();
        blocks.push(dxf::Block { name: b.name.clone(), base_point: to_dxf(b.base), entities, ..Default::default() });
    }
    let mut entities: Vec<DxfEntity> = Vec::new();
    for e in doc.entities() {
        for mut de in ex.entity(&e.geometry, &e.layer, e.color) {
            if let (EntityType::Insert(ins), Geometry::Insert { attributes, .. }) = (&mut de.specific, &e.geometry) {
                for a in attributes {
                    ins.add_attribute(&mut drawing, attribute_to_dxf(a));
                }
            }
            entities.push(de);
        }
    }
    for b in blocks.into_iter().chain(ex.blocks) {
        drawing.add_block(b);
    }
    for e in entities {
        drawing.add_entity(e);
    }
    drawing
}

/// Converts one DXF entity, or `None` for types Oxidraft doesn't support yet.
fn geometry_from_dxf(specific: &EntityType) -> Option<Geometry> {
    Some(match specific {
        EntityType::Line(l) => Geometry::Line { start: from_dxf(&l.p1), end: from_dxf(&l.p2) },
        EntityType::Circle(c) => Geometry::Circle { center: from_dxf(&c.center), radius: c.radius },
        EntityType::Arc(a) => Geometry::Arc {
            center: from_dxf(&a.center),
            radius: a.radius,
            start_angle: a.start_angle,
            end_angle: a.end_angle,
        },
        EntityType::LwPolyline(pl) => Geometry::Polyline {
            vertices: pl.vertices.iter().map(|v| Vertex::new(Point::new(v.x, v.y), v.bulge)).collect(),
            closed: pl.is_closed(),
        },
        // Old-style (R12) 2D polylines.
        EntityType::Polyline(pl) => Geometry::Polyline {
            vertices: pl.vertices().map(|v| Vertex::new(from_dxf(&v.location), v.bulge)).collect(),
            closed: pl.is_closed(),
        },
        EntityType::Text(t) => {
            let (attach, second) = text_justification(t.horizontal_text_justification, t.vertical_text_justification);
            Geometry::Text {
                position: from_dxf(if second { &t.second_alignment_point } else { &t.location }),
                height: t.text_height,
                rotation: t.rotation,
                width: 0.0,
                attach,
                content: decode_control_codes(&t.value),
            }
        }
        // Constant attribute definitions draw as text in every reference;
        // the others are templates for ATTRIBs and aren't drawn.
        EntityType::AttributeDefinition(a) if a.is_constant() => {
            let (attach, second) = text_justification(a.horizontal_text_justification, a.vertical_text_justification);
            Geometry::Text {
                position: from_dxf(if second { &a.second_alignment_point } else { &a.location }),
                height: a.text_height,
                rotation: a.rotation,
                width: 0.0,
                attach,
                content: decode_control_codes(&a.value),
            }
        }
        EntityType::MText(m) => {
            let x = &m.x_axis_direction;
            let rotation =
                if x.x.hypot(x.y) > 1e-9 { x.y.atan2(x.x).to_degrees() } else { m.rotation_angle.to_degrees() };
            Geometry::Text {
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
            Geometry::Dimension {
                p1: from_dxf(&d.definition_point_2),
                p2: from_dxf(&d.definition_point_3),
                line: from_dxf(&base.definition_point_1),
                rotation: d.rotation_angle,
                aligned,
                text: Some(base.text.clone()).filter(|t| !t.is_empty() && t != "<>"),
                scale: 1.0,
            }
        }
        EntityType::Leader(l) if l.vertices.len() >= 2 => {
            Geometry::Leader { points: l.vertices.iter().map(from_dxf).collect(), content: String::new(), scale: 1.0 }
        }
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
        },
        _ => return None,
    })
}

/// Attribute definitions other than constant ones only describe the
/// attributes of future references; they aren't unsupported content.
fn is_template(e: &EntityType) -> bool {
    matches!(e, EntityType::AttributeDefinition(a) if !a.is_constant())
}

/// Builds a document from a DXF drawing. Attributes take their reference's
/// layer; [`read`] and [`load_file`] also recover their own layer and color.
pub fn from_drawing(drawing: &Drawing) -> Imported {
    from_drawing_styled(drawing, None)
}

/// Like [`from_drawing`], with each ATTRIB's (layer, color) in file order.
fn from_drawing_styled(drawing: &Drawing, attribute_styles: Option<Vec<(String, Color)>>) -> Imported {
    let mut doc = Document::new();
    for l in drawing.layers() {
        let color = l.color.index().filter(|&i| i > 0).unwrap_or(7);
        let mut layer = Layer::new(l.name.clone(), color);
        layer.on = l.is_layer_on;
        doc.add_layer(layer.clone());
        doc.update_layer(layer);
    }
    let mut skipped = 0;
    // "*Model_Space" and "*Paper_Space" hold the drawing itself; other "*"
    // blocks (exploded hatches, dimension graphics) are kept so references
    // to them still draw.
    let is_space = |n: &str| {
        let n = n.to_ascii_lowercase();
        n.starts_with("*model_space") || n.starts_with("*paper_space")
    };
    for b in drawing.blocks().filter(|b| !is_space(&b.name)) {
        let mut entities = Vec::new();
        for e in &b.entities {
            if is_template(&e.specific) {
                continue;
            }
            match geometry_from_dxf(&e.specific) {
                Some(mut geometry) => {
                    style_attributes(&mut geometry, None, &e.common.layer);
                    entities.push(BlockEntity {
                        layer: e.common.layer.clone(),
                        color: from_dxf_color(&e.common.color),
                        geometry,
                    });
                }
                None => skipped += 1,
            }
        }
        doc.define_block(Block { name: b.name.clone(), base: from_dxf(&b.base_point), entities });
    }
    // ATTRIBs follow their INSERT in the file, so the styles line up with
    // the references' attributes in order, if the counts agree.
    let attribute_count = |e: &DxfEntity| match &e.specific {
        EntityType::Insert(i) => i.attributes().count(),
        _ => 0,
    };
    let total: usize = drawing.entities().map(attribute_count).sum();
    let styles = attribute_styles.filter(|s| s.len() == total);
    let mut next = 0;
    for e in drawing.entities() {
        let n = attribute_count(e);
        let style = styles.as_ref().map(|s| &s[next..next + n]);
        next += n;
        // Paper space (layouts) isn't supported yet; its objects aren't model space.
        if e.common.is_in_paper_space || is_template(&e.specific) {
            continue;
        }
        match geometry_from_dxf(&e.specific) {
            Some(mut g) => {
                style_attributes(&mut g, style, &e.common.layer);
                doc.add_on_layer(g, &e.common.layer, from_dxf_color(&e.common.color));
            }
            None => skipped += 1,
        }
    }
    // A freshly opened file starts with an empty undo history.
    doc.forget_history();
    Imported { document: doc, skipped }
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
    let mut buf = Vec::new();
    to_drawing(doc).save(&mut buf)?;
    // Attributes are written in document order, one INSERT after another.
    let styles: Vec<(String, Color)> = doc
        .entities()
        .flat_map(|e| match &e.geometry {
            Geometry::Insert { attributes, .. } => attributes.iter().map(|a| (a.layer.clone(), a.color)).collect(),
            _ => Vec::new(),
        })
        .collect();
    writer.write_all(&fix_attributes(&buf, &styles)).map_err(Error::IoError)
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
    let drawing = Drawing::load(&mut bytes.as_slice())?;
    Ok(from_drawing_styled(&drawing, attribute_styles(&bytes)))
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
            entities: vec![BlockEntity {
                layer: "0".into(),
                color: Color::ByLayer,
                geometry: Geometry::Circle { center: Point::new(1.0, 1.0), radius: 0.5 },
            }],
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
            position: Point::new(1.0, 2.0),
            height: 2.5,
            rotation: 30.0,
            width: 40.0,
            attach: Attach::TopLeft,
            content: "Line one\\PLine two".into(),
        };
        let single = Geometry::Text {
            position: Point::new(5.0, 5.0),
            height: 3.0,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "TITLE".into(),
        };
        let dim = Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(10.0, 3.0),
            line: Point::new(5.0, 8.0),
            rotation: 0.0,
            aligned: false,
            text: Some("<> TYP".into()),
            scale: 1.0,
        };
        let aligned = Geometry::Dimension {
            p1: Point::new(0.0, 0.0),
            p2: Point::new(3.0, 4.0),
            line: Point::new(-1.0, 2.0),
            rotation: 0.0,
            aligned: true,
            text: None,
            scale: 1.0,
        };
        for g in [&mtext, &single, &dim, &aligned] {
            doc.add(g.clone());
        }
        doc.add(Geometry::Leader {
            points: vec![Point::ORIGIN, Point::new(10.0, 10.0)],
            content: "NOTE".into(),
            scale: 1.0,
        });
        let square: Vec<Vertex> = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]
            .iter()
            .map(|&(x, y)| Vertex::at(Point::new(x, y)))
            .collect();
        doc.add(Geometry::Hatch { loops: vec![square], pattern: HatchPattern::Ansi31, scale: 1.0, angle: 0.0 });

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
        let Some(Geometry::Text { rotation, width, content, .. }) =
            got.iter().find(|g| matches!(g, Geometry::Text { content, .. } if content.starts_with("Line one")))
        else {
            panic!("MTEXT lost")
        };
        assert!((rotation - 30.0).abs() < 1e-9 && *width == 40.0 && content == "Line one\\PLine two");
        // The leader keeps its arrow and lines; its text comes back as MTEXT beside it.
        assert!(got.iter().any(|g| matches!(g, Geometry::Leader { points, .. } if points.len() == 2)));
        assert!(got.iter().any(|g| matches!(g, Geometry::Text { content, .. } if content == "NOTE")));
        // Hatches come back as a reference to an anonymous block of pattern lines.
        let Some(Geometry::Insert { block, .. }) = got.iter().find(|g| matches!(g, Geometry::Insert { .. })) else {
            panic!("hatch lost")
        };
        assert!(block.starts_with("*X") && !back.block(block).unwrap().entities.is_empty());
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
        assert_eq!(doc.block("PRPARC").unwrap().entities.len(), 1, "only the symbol is block content");

        let reference = doc.entities().find(|e| matches!(e.geometry, Geometry::Insert { .. })).unwrap();
        let Geometry::Insert { attributes, .. } = &reference.geometry else { unreachable!() };
        assert_eq!(attributes.len(), 2);
        assert_eq!((attributes[0].position, attributes[0].attach), (Point::new(100.0, 200.0), Attach::MiddleCenter));
        assert!(attributes[1].invisible);
        // The ByBlock symbol and the attribute take the reference's layer color.
        let drawn = doc.expanded_styled(reference);
        assert_eq!(drawn.len(), 2, "symbol plus the visible attribute");
        assert!(matches!(&drawn[1].0, Geometry::Text { content, .. } if content == "IV"));

        let Some(Geometry::Text { position, attach, content, .. }) =
            doc.entities().map(|e| &e.geometry).find(|g| matches!(g, Geometry::Text { .. }))
        else {
            panic!()
        };
        assert_eq!((*position, *attach, content.as_str()), (Point::new(5.0, 5.0), Attach::TopRight, "45°"));
    }
}

//! LIST: shows the properties of the selected objects in the command
//! history, as AutoCAD's text window does.

use oxidraft_geom::{Point, curve::Seg};
use oxidraft_model::{
    Color, Entity, Geometry,
    annotation::{angular_measurement, format_angle, format_measurement, measurement, radial_value},
    linetype,
};

use crate::{Command, Ctx, Input, Prompt, Step, UiRequest};

fn n(v: f64) -> String {
    format!("{v:>10.4}")
}

fn at(label: &str, p: Point) -> String {
    format!("{label:>20}  X={}  Y={}  Z={}", n(p.x), n(p.y), n(0.0))
}

fn value(label: &str, v: impl std::fmt::Display) -> String {
    format!("{label:>20}  {v}")
}

/// The lines LIST prints for one object.
pub(crate) fn describe(e: &Entity) -> Vec<String> {
    let g = &e.geometry;
    let mut out = vec![
        format!("{:>20}  Layer: \"{}\"", g.name().to_uppercase(), e.layer),
        value("", "Space: Model space"),
        value("Handle =", format!("{:x}", e.id.0)),
    ];
    if e.color != Color::ByLayer {
        let color = match e.color {
            Color::Index(i) => i.to_string(),
            _ => "BYBLOCK".into(),
        };
        out.push(value("Color:", color));
    }
    if !linetype::is_by_layer(&e.linetype) {
        out.push(value("Linetype:", e.linetype.to_uppercase()));
    }
    if e.lineweight != oxidraft_model::Lineweight::ByLayer {
        out.push(value("Lineweight:", e.lineweight.label()));
    }
    match g {
        Geometry::Point { position } => out.push(at("at point,", *position)),
        Geometry::Line { start, end } => {
            let d = *end - *start;
            out.push(at("from point,", *start));
            out.push(at("to point,", *end));
            out.push(value("Length =", n(d.length()).trim()));
            out.push(value("Angle in XY Plane =", format_angle(start.angle_to(*end))));
            out.push(value("Delta X =", format!("{}, Delta Y = {}", n(d.x).trim(), n(d.y).trim())));
        }
        Geometry::Circle { center, radius } => {
            out.push(at("center point,", *center));
            out.push(value("radius", n(*radius)));
            out.push(value("circumference", n(g.length())));
            out.push(value("area", n(g.area().unwrap_or(0.0))));
        }
        Geometry::Arc { center, radius, start_angle, end_angle } => {
            out.push(at("center point,", *center));
            out.push(value("radius", n(*radius)));
            out.push(value("start angle", format_angle(*start_angle)));
            out.push(value("end angle", format_angle(*end_angle)));
            out.push(value("length", n(g.length())));
        }
        Geometry::Polyline { vertices, closed } => {
            out.push(value("", if *closed { "Closed" } else { "Open" }));
            let constant = oxidraft_model::constant_width(vertices);
            if let Some(w) = constant {
                out.push(value("Constant width", n(w)));
            }
            if let Some(area) = g.area() {
                out.push(value("area", n(area)));
                out.push(value("perimeter", n(g.length())));
            } else {
                out.push(value("length", n(g.length())));
            }
            let segs = g.segments();
            for (i, v) in vertices.iter().enumerate() {
                out.push(at("at point", v.point));
                if constant.is_none() {
                    out.push(value("starting width", n(v.start_width)));
                    out.push(value("ending width", n(v.end_width)));
                }
                if let Some(Seg::Arc(a)) = segs.get(i).filter(|_| v.bulge != 0.0) {
                    out.push(value("bulge", n(v.bulge)));
                    out.push(at("center", a.center));
                    out.push(value("radius", n(a.radius)));
                }
            }
        }
        Geometry::Ellipse(e) => {
            out.push(at("Center:", e.center));
            out.push(at("Major Axis:", e.major));
            out.push(at("Minor Axis:", e.minor()));
            out.push(value("Radius Ratio:", n(e.ratio).trim()));
            if e.is_full() {
                out.push(value("Area:", n(e.area()).trim()));
                out.push(value("Circumference:", n(g.length()).trim()));
            } else {
                out.push(value("Start Angle:", format_angle(e.center.angle_to(e.start_point()))));
                out.push(value("End Angle:", format_angle(e.center.angle_to(e.end_point()))));
                out.push(value("Length:", n(g.length()).trim()));
            }
        }
        Geometry::Spline(s) => {
            out.push(value("Length:", n(g.length()).trim()));
            out.push(value("Order:", s.degree() + 1));
            let rational = if s.is_rational() { "Rational" } else { "Non-Rational" };
            let periodic = if s.is_periodic() { "Periodic" } else { "Non-Periodic" };
            let closed = if s.is_closed() { "Closed" } else { "Open" };
            out.push(value("Properties:", format!("Planar, {rational}, {periodic}, {closed}")));
            let (u0, u1) = s.domain();
            out.push(value("Parametric Range:", format!("Start {}  End {}", n(u0).trim(), n(u1).trim())));
            out.push(value("Number of control points:", s.control().len()));
            for p in s.control() {
                out.push(at("", *p));
            }
            if !s.fit().is_empty() {
                out.push(value("Number of fit points:", s.fit().len()));
                for p in s.fit() {
                    out.push(at("", *p));
                }
            }
        }
        Geometry::Viewport { center, width, height, view_center, view_height, .. } => {
            out.push(at("Center:", *center));
            out.push(value("Width:", n(*width).trim()));
            out.push(value("Height:", n(*height).trim()));
            out.push(at("View center:", *view_center));
            out.push(value("Scale:", format!("1:{}", n(view_height / height.max(1e-12)).trim())));
        }
        Geometry::Table { position, widths, heights, cells, .. } => {
            out.push(at("Insertion point:", *position));
            out.push(value("Columns:", widths.len().to_string()));
            out.push(value("Rows:", heights.len().to_string()));
            out.push(value("Width:", n(widths.iter().sum()).trim()));
            out.push(value("Height:", n(heights.iter().sum()).trim()));
            if let Some(title) = cells.first().and_then(|r| r.first()).filter(|t| !t.is_empty()) {
                out.push(value("Title:", title));
            }
        }
        Geometry::Image { image, origin, u, v, clip } => {
            out.push(value("Image name:", format!("\"{image}\"")));
            out.push(at("Insertion point:", *origin));
            out.push(value("Width:", n(u.length()).trim()));
            out.push(value("Height:", n(v.length()).trim()));
            out.push(value("Rotation:", format_angle(Point::ORIGIN.angle_to(*u))));
            if clip.len() >= 3 {
                out.push(value("", "Clipped"));
            }
        }
        Geometry::Xline { base, direction } => {
            out.push(at("basepoint,", *base));
            out.push(at("second point,", *base + *direction * (1.0 / direction.length().max(f64::MIN_POSITIVE))));
        }
        Geometry::Ray { base, direction } => {
            out.push(at("start point,", *base));
            out.push(at("second point,", *base + *direction * (1.0 / direction.length().max(f64::MIN_POSITIVE))));
        }
        Geometry::Insert { block, placement, attributes, .. } => {
            out.push(value("Block Name:", format!("\"{block}\"")));
            out.push(at("at point,", placement.position));
            out.push(value("X scale factor:", n(if placement.mirror { -placement.scale } else { placement.scale })));
            out.push(value("Y scale factor:", n(placement.scale)));
            out.push(value("rotation angle:", format_angle(placement.rotation)));
            for a in attributes {
                out.push(format!("{:>20}  Layer: \"{}\"", "ATTRIBUTE", a.layer));
                out.push(value("Tag:", &a.tag));
                out.push(value("Value:", &a.value));
                if a.invisible {
                    out.push(value("", "Invisible"));
                }
            }
        }
        Geometry::Text { position, height, rotation, content, .. } => {
            out.push(at("Location:", *position));
            out.push(value("Text height:", n(*height)));
            out.push(value("Rotation:", format_angle(*rotation)));
            out.push(value("Contents:", content.replace('\n', "\\P")));
        }
        Geometry::Dimension { p1, p2, line, rotation, aligned, text, .. } => {
            out.push(value("Measurement:", format_measurement(measurement(*p1, *p2, *rotation, *aligned))));
            if let Some(t) = text {
                out.push(value("Text override:", t));
            }
            out.push(at("defining point,", *p1));
            out.push(at("defining point,", *p2));
            out.push(at("dimension line,", *line));
        }
        Geometry::RadialDimension { center, point, diameter, text, .. } => {
            out.push(value("Measurement:", radial_value(*center, *point, *diameter)));
            if let Some(t) = text {
                out.push(value("Text override:", t));
            }
            out.push(at("center point,", *center));
        }
        Geometry::AngularDimension { vertex, p1, p2, text, .. } => {
            out.push(value("Measurement:", format_angle(angular_measurement(*vertex, *p1, *p2))));
            if let Some(t) = text {
                out.push(value("Text override:", t));
            }
            out.push(at("vertex,", *vertex));
        }
        Geometry::Leader { points, content, .. } => {
            for p in points {
                out.push(at("at point,", *p));
            }
            out.push(value("Contents:", content.replace('\n', "\\P")));
        }
        Geometry::Hatch { pattern, scale, angle, .. } => {
            out.push(value("Pattern name:", pattern.name()));
            out.push(value("Scale:", n(*scale)));
            out.push(value("Angle:", format_angle(*angle)));
            if let Some((area, _, _)) = crate::inquiry::object_area(g) {
                out.push(value("Area", n(area)));
            }
        }
    }
    out
}

/// LIST.
#[derive(Default)]
pub struct List;

impl Command for List {
    fn prompt(&self) -> Prompt {
        Prompt::new("Select objects")
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        let Input::Selection(s) = input else {
            return Err("Select objects, then press Enter.".into());
        };
        for e in s.ids.iter().filter_map(|&id| ctx.doc.entity(id)) {
            ctx.messages.extend(describe(e));
            ctx.messages.push(String::new());
        }
        if !s.ids.is_empty() {
            ctx.ui = Some(UiRequest::TextWindow);
        }
        Ok(Step::Done)
    }

    fn wants_selection(&self) -> bool {
        true
    }
}

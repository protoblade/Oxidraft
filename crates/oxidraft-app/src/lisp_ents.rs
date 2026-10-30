//! Objects as AutoLISP sees them: entget's DXF-coded lists, entmod and
//! entmake changing and making objects from them, and ssget's modes and
//! filters.

use oxidraft_geom::ellipse::Ellipse;
use oxidraft_geom::{Aabb, Point};
use oxidraft_model::annotation::{Attach, TextLook};
use oxidraft_model::{Color, Entity, EntityId, Geometry, Layer, Lineweight, Placement, Vertex};

use crate::app::OxidraftApp;
use crate::lisp::{Dxf, EntData, wcmatch};

fn pt(p: Point) -> Dxf {
    Dxf::Point([p.x, p.y, 0.0])
}

fn is_single_line(width: f64, attach: Attach, content: &str) -> bool {
    width == 0.0 && attach == Attach::BottomLeft && !content.contains('\n')
}

/// The object's type as DXF names it.
pub(crate) fn dxf_type(g: &Geometry) -> String {
    match g {
        Geometry::Point { .. } => "POINT",
        Geometry::Line { .. } => "LINE",
        Geometry::Circle { .. } => "CIRCLE",
        Geometry::Arc { .. } => "ARC",
        Geometry::Polyline { .. } => "LWPOLYLINE",
        Geometry::Insert { .. } => "INSERT",
        Geometry::Text { width, attach, content, .. } if is_single_line(*width, *attach, content) => "TEXT",
        Geometry::Text { .. } => "MTEXT",
        Geometry::Dimension { .. } | Geometry::RadialDimension { .. } | Geometry::AngularDimension { .. } => {
            "DIMENSION"
        }
        Geometry::Leader { .. } => "LEADER",
        Geometry::Hatch { .. } => "HATCH",
        Geometry::Ellipse(_) => "ELLIPSE",
        Geometry::Spline(_) => "SPLINE",
        Geometry::Xline { .. } => "XLINE",
        Geometry::Ray { .. } => "RAY",
        Geometry::Image { .. } => "IMAGE",
        Geometry::Viewport { .. } => "VIEWPORT",
        g => return g.name().to_uppercase().replace(' ', ""),
    }
    .to_string()
}

/// entget: the object's data.
pub(crate) fn ent_data(e: &Entity) -> EntData {
    let mut d: EntData =
        vec![(-1, Dxf::Ename(e.id.0)), (0, Dxf::Str(dxf_type(&e.geometry))), (8, Dxf::Str(e.layer.clone()))];
    if e.space != oxidraft_model::MODEL {
        d.push((67, Dxf::Int(1)));
    }
    match e.color {
        Color::ByLayer => {}
        Color::ByBlock => d.push((62, Dxf::Int(0))),
        Color::Index(n) => d.push((62, Dxf::Int(n as i64))),
    }
    if !e.linetype.eq_ignore_ascii_case(oxidraft_model::linetype::BYLAYER) {
        d.push((6, Dxf::Str(e.linetype.clone())));
    }
    match &e.geometry {
        Geometry::Point { position } => d.push((10, pt(*position))),
        Geometry::Line { start, end } => d.extend([(10, pt(*start)), (11, pt(*end))]),
        Geometry::Circle { center, radius } => d.extend([(10, pt(*center)), (40, Dxf::Real(*radius))]),
        Geometry::Arc { center, radius, start_angle, end_angle } => d.extend([
            (10, pt(*center)),
            (40, Dxf::Real(*radius)),
            (50, Dxf::Real(start_angle.to_radians())),
            (51, Dxf::Real(end_angle.to_radians())),
        ]),
        Geometry::Polyline { vertices, closed } => {
            d.extend([(90, Dxf::Int(vertices.len() as i64)), (70, Dxf::Int(*closed as i64))]);
            for v in vertices {
                d.extend([
                    (10, pt(v.point)),
                    (40, Dxf::Real(v.start_width)),
                    (41, Dxf::Real(v.end_width)),
                    (42, Dxf::Real(v.bulge)),
                ]);
            }
        }
        Geometry::Insert { block, placement, .. } => {
            let x = if placement.mirror { -placement.scale } else { placement.scale };
            d.extend([
                (2, Dxf::Str(block.clone())),
                (10, pt(placement.position)),
                (41, Dxf::Real(x)),
                (42, Dxf::Real(placement.scale)),
                (43, Dxf::Real(placement.scale)),
                (50, Dxf::Real(placement.rotation.to_radians())),
            ]);
        }
        Geometry::Text { position, height, rotation, width, attach, content, look } => {
            d.extend([
                (1, Dxf::Str(content.clone())),
                (7, Dxf::Str(look.style.clone())),
                (10, pt(*position)),
                (40, Dxf::Real(*height)),
                (50, Dxf::Real(rotation.to_radians())),
            ]);
            if is_single_line(*width, *attach, content) {
                d.push((41, Dxf::Real(look.width_factor)));
            } else {
                d.extend([(41, Dxf::Real(*width)), (71, Dxf::Int(*attach as i64 + 1))]);
            }
        }
        Geometry::Ellipse(el) => d.extend([
            (10, pt(el.center)),
            (11, pt(el.major)),
            (40, Dxf::Real(el.ratio)),
            (41, Dxf::Real(el.start.to_radians())),
            (42, Dxf::Real(el.end.to_radians())),
        ]),
        Geometry::Xline { base, direction } | Geometry::Ray { base, direction } => {
            d.extend([(10, pt(*base)), (11, pt(*direction))])
        }
        Geometry::Hatch { pattern, scale, angle, .. } => d.extend([
            (2, Dxf::Str(pattern.name().to_string())),
            (41, Dxf::Real(*scale)),
            (52, Dxf::Real(angle.to_radians())),
        ]),
        Geometry::Dimension { p1, p2, line, text, .. } => {
            d.extend([(1, Dxf::Str(text.clone().unwrap_or_default())), (10, pt(*line)), (13, pt(*p1)), (14, pt(*p2))])
        }
        Geometry::Leader { points, content, .. } => {
            d.push((3, Dxf::Str(content.clone())));
            d.extend(points.iter().map(|p| (10, pt(*p))));
        }
        _ => {}
    }
    d
}

fn same(d: &Dxf, wanted: &Dxf) -> bool {
    match (d, wanted) {
        (Dxf::Str(s), Dxf::Str(p)) => wcmatch(s, p),
        (Dxf::Point(a), Dxf::Point(b)) => a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9),
        (Dxf::Ename(a), Dxf::Ename(b)) => a == b,
        _ => match (number(d), number(wanted)) {
            (Some(x), Some(y)) => (x - y).abs() < 1e-9,
            _ => false,
        },
    }
}

fn number(d: &Dxf) -> Option<f64> {
    match d {
        Dxf::Int(i) => Some(*i as f64),
        Dxf::Real(r) => Some(*r),
        _ => None,
    }
}

/// Whether the object passes an ssget filter: every pair given matches
/// (text with wildcards; color 256 is ByLayer).
pub(crate) fn passes(e: &Entity, filter: &EntData) -> bool {
    let data = ent_data(e);
    filter.iter().filter(|(code, _)| *code != -4).all(|(code, wanted)| match (code, wanted) {
        (62, Dxf::Int(256)) => e.color == Color::ByLayer,
        (6, Dxf::Str(p)) if e.linetype.eq_ignore_ascii_case(oxidraft_model::linetype::BYLAYER) => wcmatch("BYLAYER", p),
        _ => data.iter().any(|(c, d)| c == code && same(d, wanted)),
    })
}

/// The data's value for `code`.
struct Fields<'a>(&'a EntData);

impl Fields<'_> {
    fn get(&self, code: i32) -> Option<&Dxf> {
        self.0.iter().find(|(c, _)| *c == code).map(|(_, v)| v)
    }

    fn text(&self, code: i32) -> Option<String> {
        match self.get(code)? {
            Dxf::Str(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn real(&self, code: i32) -> Option<f64> {
        self.get(code).and_then(number)
    }

    fn point(&self, code: i32) -> Option<Point> {
        match self.get(code)? {
            Dxf::Point([x, y, _]) => Some(Point::new(*x, *y)),
            _ => None,
        }
    }

    fn degrees(&self, code: i32) -> Option<f64> {
        self.real(code).map(f64::to_degrees)
    }

    /// LWPOLYLINE's vertices: each 10, with the 40, 41 and 42 after it.
    fn vertices(&self) -> Vec<Vertex> {
        let mut out: Vec<Vertex> = Vec::new();
        for (code, v) in self.0 {
            match (code, v, out.last_mut()) {
                (10, Dxf::Point([x, y, _]), _) => out.push(Vertex::at(Point::new(*x, *y))),
                (40, v, Some(last)) => last.start_width = number(v).unwrap_or(0.0),
                (41, v, Some(last)) => last.end_width = number(v).unwrap_or(0.0),
                (42, v, Some(last)) => last.bulge = number(v).unwrap_or(0.0),
                _ => {}
            }
        }
        out
    }
}

/// The geometry the data gives: `old` changed (entmod), or a new object
/// of the type in group 0 (entmake).
fn geometry(old: Option<&Geometry>, f: &Fields) -> Option<Geometry> {
    let kind = match old {
        Some(g) => dxf_type(g),
        None => f.text(0)?.to_uppercase(),
    };
    let p = |code: i32, was: Option<Point>| f.point(code).or(was);
    let r = |code: i32, was: Option<f64>| f.real(code).or(was);
    let a = |code: i32, was: Option<f64>| f.degrees(code).or(was);
    Some(match (kind.as_str(), old) {
        ("POINT", Some(Geometry::Point { position })) => Geometry::Point { position: p(10, Some(*position))? },
        ("POINT", _) => Geometry::Point { position: f.point(10)? },
        ("LINE", Some(Geometry::Line { start, end })) => {
            Geometry::Line { start: p(10, Some(*start))?, end: p(11, Some(*end))? }
        }
        ("LINE", _) => Geometry::Line { start: f.point(10)?, end: f.point(11)? },
        ("CIRCLE", Some(Geometry::Circle { center, radius })) => {
            Geometry::Circle { center: p(10, Some(*center))?, radius: r(40, Some(*radius))? }
        }
        ("CIRCLE", _) => Geometry::Circle { center: f.point(10)?, radius: f.real(40)? },
        ("ARC", Some(Geometry::Arc { center, radius, start_angle, end_angle })) => Geometry::Arc {
            center: p(10, Some(*center))?,
            radius: r(40, Some(*radius))?,
            start_angle: a(50, Some(*start_angle))?,
            end_angle: a(51, Some(*end_angle))?,
        },
        ("ARC", _) => Geometry::Arc {
            center: f.point(10)?,
            radius: f.real(40)?,
            start_angle: f.degrees(50)?,
            end_angle: f.degrees(51)?,
        },
        ("LWPOLYLINE", old) => {
            let (was, was_closed) = match old {
                Some(Geometry::Polyline { vertices, closed }) => (vertices.clone(), *closed),
                _ => (Vec::new(), false),
            };
            let vertices = f.vertices();
            let vertices = if vertices.is_empty() { was } else { vertices };
            if vertices.len() < 2 {
                return None;
            }
            let closed = f.real(70).map_or(was_closed, |flags| flags as i64 & 1 == 1);
            Geometry::Polyline { vertices, closed }
        }
        ("INSERT", old) => {
            let (block, placement, attributes, clip) = match old {
                Some(Geometry::Insert { block, placement, attributes, clip }) => {
                    (block.clone(), *placement, attributes.clone(), clip.clone())
                }
                _ => (
                    f.text(2)?,
                    Placement { position: f.point(10)?, scale: 1.0, rotation: 0.0, mirror: false },
                    Vec::new(),
                    None,
                ),
            };
            let x = r(41, Some(if placement.mirror { -placement.scale } else { placement.scale }))?;
            Geometry::Insert {
                block: f.text(2).unwrap_or(block),
                placement: Placement {
                    position: p(10, Some(placement.position))?,
                    scale: x.abs(),
                    rotation: a(50, Some(placement.rotation))?,
                    mirror: x < 0.0,
                },
                attributes,
                clip,
            }
        }
        ("TEXT" | "MTEXT", old) => {
            let mtext = kind == "MTEXT";
            let (position, height, rotation, width, attach, content, mut look) = match old {
                Some(Geometry::Text { position, height, rotation, width, attach, content, look }) => {
                    (Some(*position), Some(*height), *rotation, *width, *attach, Some(content.clone()), look.clone())
                }
                _ => (
                    None,
                    None,
                    0.0,
                    0.0,
                    if mtext { Attach::TopLeft } else { Attach::BottomLeft },
                    None,
                    TextLook::default(),
                ),
            };
            if let Some(style) = f.text(7) {
                look.style = style;
            }
            if !mtext && let Some(w) = f.real(41) {
                look.width_factor = w;
            }
            Geometry::Text {
                position: p(10, position)?,
                height: r(40, height)?,
                rotation: a(50, Some(rotation))?,
                width: if mtext { r(41, Some(width))? } else { width },
                attach,
                content: f.text(1).or(content)?,
                look,
            }
        }
        ("ELLIPSE", old) => {
            let was = match old {
                Some(Geometry::Ellipse(e)) => Some(*e),
                _ => None,
            };
            Geometry::Ellipse(Ellipse {
                center: p(10, was.map(|e| e.center))?,
                major: p(11, was.map(|e| e.major))?,
                ratio: r(40, was.map(|e| e.ratio))?,
                start: a(41, was.map(|e| e.start)).unwrap_or(0.0),
                end: a(42, was.map(|e| e.end)).unwrap_or(360.0),
            })
        }
        ("XLINE", Some(Geometry::Xline { base, direction })) => {
            Geometry::Xline { base: p(10, Some(*base))?, direction: p(11, Some(*direction))? }
        }
        ("XLINE", _) => Geometry::Xline { base: f.point(10)?, direction: f.point(11)? },
        ("RAY", Some(Geometry::Ray { base, direction })) => {
            Geometry::Ray { base: p(10, Some(*base))?, direction: p(11, Some(*direction))? }
        }
        ("RAY", _) => Geometry::Ray { base: f.point(10)?, direction: f.point(11)? },
        // Other objects keep their geometry; only their properties change.
        (_, Some(g)) => g.clone(),
        _ => return None,
    })
}

impl OxidraftApp {
    /// entmod (`id`) or entmake: the object changed or made, as one
    /// undoable step.
    pub(crate) fn ent_mod(&mut self, id: Option<EntityId>, data: &EntData) -> Option<EntityId> {
        let f = Fields(data);
        let old = match id {
            Some(id) => Some(self.doc.entity(id)?.geometry.clone()),
            None => None,
        };
        let g = geometry(old.as_ref(), &f)?;
        let layer = f.text(8);
        let color = match f.real(62).map(|c| c as i64) {
            Some(0) => Some(Color::ByBlock),
            Some(256) => Some(Color::ByLayer),
            Some(n @ 1..=255) => Some(Color::Index(n as u8)),
            _ => None,
        };
        let linetype = f.text(6);
        self.doc.begin(if id.is_some() { "ENTMOD" } else { "ENTMAKE" });
        if let Some(l) = &layer
            && self.doc.layer(l).is_none()
        {
            self.doc.add_layer(Layer::new(l, 7));
        }
        let id = match id {
            Some(id) => {
                self.doc.modify(id, g);
                self.doc.update(id, |e| {
                    if let Some(l) = layer {
                        e.layer = l;
                    }
                    if let Some(c) = color {
                        e.color = c;
                    }
                    if let Some(lt) = linetype {
                        e.linetype = lt;
                    }
                });
                id
            }
            None => {
                let layer = layer.unwrap_or_else(|| self.doc.current_layer().to_string());
                let linetype = linetype.unwrap_or_else(|| oxidraft_model::linetype::BYLAYER.to_string());
                self.doc.add_styled(g, &layer, color.unwrap_or(Color::ByLayer), &linetype, Lineweight::ByLayer)
            }
        };
        self.doc.commit();
        Some(id)
    }

    /// ssget's modes that don't ask: all (X), the last object (L), the
    /// previous selection (P), the selected objects (I), a window (W) or
    /// crossing (C) between two points, or the object at a point.
    pub(crate) fn ss_get(&self, mode: &str, points: &[[f64; 2]], filter: &EntData) -> Vec<u64> {
        let pts: Vec<Point> = points.iter().map(|[x, y]| Point::new(*x, *y)).collect();
        let space = self.doc.space();
        let here = || self.doc.entities().filter(move |e| e.space == space);
        let ids: Vec<EntityId> = match (mode, pts.as_slice()) {
            ("X" | "A", _) => self.doc.entities().map(|e| e.id).collect(),
            ("L", _) => self.doc.entities().last().map(|e| e.id).into_iter().collect(),
            ("P", _) => self.engine.previous_selection().to_vec(),
            ("I", _) => self.engine.selection().to_vec(),
            ("W" | "C", [a, b, ..]) => {
                let window = Aabb::from_points(*a, *b);
                here()
                    .filter(|e| {
                        window.contains_aabb(&self.doc.entity_bounds(e))
                            || mode == "C" && self.doc.entity_touches(e, &window)
                    })
                    .map(|e| e.id)
                    .collect()
            }
            ("PT", [p, ..]) => {
                let tolerance = self.pick_tolerance();
                here()
                    .map(|e| (e.id, self.doc.entity_distance(e, *p)))
                    .filter(|(_, d)| *d <= tolerance)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(id, _)| id)
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        };
        ids.into_iter().filter(|id| self.doc.entity(*id).is_some_and(|e| passes(e, filter))).map(|id| id.0).collect()
    }
}

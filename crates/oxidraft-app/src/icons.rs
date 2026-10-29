//! Vector icons for the ribbon and quick-access toolbar, drawn with the
//! painter so the app ships without image assets.
//!
//! Each icon is laid out on a 0..1 square and scaled to the target rect.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Line,
    Polyline,
    Circle,
    Arc,
    Rectangle,
    Hatch,
    Move,
    Copy,
    Rotate,
    Mirror,
    Stretch,
    Scale,
    Trim,
    Extend,
    Chamfer,
    Break,
    Join,
    Lengthen,
    Fillet,
    Array,
    Erase,
    Offset,
    Explode,
    Text,
    Dimension,
    Leader,
    Table,
    LayerProperties,
    MakeCurrent,
    LayerIsolate,
    LayerUnisolate,
    LayerOff,
    LayerFreeze,
    LayerLock,
    LayerUnlock,
    Point,
    Polygon,
    Ellipse,
    Spline,
    Xline,
    Ray,
    RevCloud,
    Donut,
    EditPolyline,
    Measure,
    Plot,
    PlotPreview,
    Attach,
    ExternalReferences,
    BringToFront,
    SendToBack,
    Image,
    Pdf,
    ImageClip,
    EditReference,
    SaveReference,
    DiscardReference,
    AddToWorkingSet,
    RemoveFromWorkingSet,
    InsertBlock,
    CreateBlock,
    MatchProperties,
    Properties,
    New,
    Open,
    Save,
    Undo,
    Redo,
    Pan,
    Zoom,
    NewLayout,
    PageSetup,
    Viewport,
    ViewportLock,
    ViewportUnlock,
    Maximize,
    SingleLineText,
    DimAngular,
    DimRadius,
    DimDiameter,
    DimContinue,
    DimBaseline,
}

/// Accent colors used inside icons, as in AutoCAD's icon set.
const BLUE: Color32 = Color32::from_rgb(70, 160, 255);
const YELLOW: Color32 = Color32::from_rgb(250, 200, 60);
const RED: Color32 = Color32::from_rgb(235, 80, 70);
const GREEN: Color32 = Color32::from_rgb(110, 200, 90);

struct Pen<'a> {
    painter: &'a Painter,
    rect: Rect,
    width: f32,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + vec2(x * self.rect.width(), y * self.rect.height())
    }

    fn s(&self, v: f32) -> f32 {
        v * self.rect.width()
    }

    fn stroke(&self, color: Color32) -> Stroke {
        Stroke::new(self.width, color)
    }

    fn line(&self, pts: &[(f32, f32)], color: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::line(pts, self.stroke(color)));
    }

    fn closed(&self, pts: &[(f32, f32)], color: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::closed_line(pts, self.stroke(color)));
    }

    fn filled(&self, pts: &[(f32, f32)], fill: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::convex_polygon(pts, fill, Stroke::NONE));
    }

    fn circle(&self, c: (f32, f32), r: f32, color: Color32) {
        self.painter.circle_stroke(self.p(c.0, c.1), self.s(r), self.stroke(color));
    }

    fn dot(&self, c: (f32, f32), r: f32, color: Color32) {
        self.painter.circle_filled(self.p(c.0, c.1), self.s(r), color);
    }

    /// Small filled square, like a grip or vertex marker.
    fn grip(&self, c: (f32, f32), color: Color32) {
        let half = self.s(0.07).max(1.5);
        self.painter.rect_filled(Rect::from_center_size(self.p(c.0, c.1), vec2(half, half) * 2.0), 0.0, color);
    }

    /// Arc from angle `a0` to `a1` (radians, y down), optionally with an arrowhead at the end.
    fn arc(&self, c: (f32, f32), r: f32, a0: f32, a1: f32, color: Color32, arrow: bool) {
        let n = 20;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect();
        self.line(&pts, color);
        if arrow {
            let end = pts[n];
            let dir = a1 - a0;
            let tangent = a1 + dir.signum() * FRAC_PI_2;
            let back = tangent + PI;
            let h = 0.16;
            self.filled(
                &[
                    (end.0 + h * 0.6 * tangent.cos(), end.1 + h * 0.6 * tangent.sin()),
                    (end.0 + h * (back + 0.5).cos(), end.1 + h * (back + 0.5).sin()),
                    (end.0 + h * (back - 0.5).cos(), end.1 + h * (back - 0.5).sin()),
                ],
                color,
            );
        }
    }

    fn arrow(&self, from: (f32, f32), to: (f32, f32), color: Color32) {
        self.line(&[from, to], color);
        let a = (to.1 - from.1).atan2(to.0 - from.0);
        let h = 0.14;
        self.filled(
            &[
                (to.0 + 0.04 * a.cos(), to.1 + 0.04 * a.sin()),
                (to.0 - h * (a - 0.45).cos(), to.1 - h * (a - 0.45).sin()),
                (to.0 - h * (a + 0.45).cos(), to.1 - h * (a + 0.45).sin()),
            ],
            color,
        );
    }
}

/// Paints `icon` into `rect`. `color` is the main line color (dimmed when disabled).
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32, enabled: bool) {
    let accent = |c: Color32| if enabled { c } else { color };
    let (blue, yellow, red, green) = (accent(BLUE), accent(YELLOW), accent(RED), accent(GREEN));
    let pen = Pen { painter, rect, width: (rect.width() / 18.0).clamp(1.0, 2.0) };
    let c = color;
    match icon {
        Icon::Line => {
            pen.line(&[(0.18, 0.82), (0.82, 0.18)], c);
            pen.grip((0.18, 0.82), blue);
            pen.grip((0.82, 0.18), blue);
        }
        Icon::Polyline => {
            pen.line(&[(0.12, 0.8), (0.35, 0.35), (0.6, 0.65)], c);
            pen.arc((0.72, 0.5), 0.17, PI * 0.75, -PI * 0.5, c, false);
            for p in [(0.12, 0.8), (0.35, 0.35), (0.6, 0.65)] {
                pen.grip(p, blue);
            }
        }
        Icon::Circle => {
            pen.circle((0.5, 0.5), 0.34, c);
            pen.line(&[(0.5, 0.5), (0.74, 0.26)], yellow);
            pen.grip((0.5, 0.5), blue);
        }
        Icon::Arc => {
            pen.arc((0.5, 0.62), 0.34, PI * 1.05, PI * 1.95, c, false);
            for a in [PI * 1.05, PI * 1.5, PI * 1.95] {
                pen.grip((0.5 + 0.34 * a.cos(), 0.62 + 0.34 * a.sin()), blue);
            }
        }
        Icon::Rectangle => {
            pen.closed(&[(0.15, 0.25), (0.85, 0.25), (0.85, 0.75), (0.15, 0.75)], c);
            pen.grip((0.15, 0.75), blue);
            pen.grip((0.85, 0.25), blue);
        }
        Icon::Hatch => {
            pen.closed(&[(0.15, 0.2), (0.85, 0.2), (0.85, 0.8), (0.15, 0.8)], c);
            for i in 0..4 {
                let o = 0.18 * i as f32;
                pen.line(&[(0.15 + o, 0.8), (0.15 + o + 0.25, 0.2 + 0.35)], yellow);
            }
        }
        Icon::Move => {
            for to in [(0.5, 0.1), (0.5, 0.9), (0.1, 0.5), (0.9, 0.5)] {
                pen.arrow((0.5, 0.5), to, c);
            }
            pen.grip((0.5, 0.5), blue);
        }
        Icon::Copy => {
            pen.closed(&[(0.15, 0.15), (0.6, 0.15), (0.6, 0.6), (0.15, 0.6)], c);
            pen.closed(&[(0.4, 0.4), (0.85, 0.4), (0.85, 0.85), (0.4, 0.85)], blue);
        }
        Icon::Rotate => {
            pen.arc((0.5, 0.5), 0.32, PI * 0.9, PI * 2.4, c, true);
            pen.grip((0.5, 0.5), blue);
        }
        Icon::Mirror => {
            pen.closed(&[(0.12, 0.8), (0.42, 0.8), (0.42, 0.25)], c);
            pen.closed(&[(0.88, 0.8), (0.58, 0.8), (0.58, 0.25)], blue);
            for i in 0..5 {
                let y = 0.1 + 0.18 * i as f32;
                pen.line(&[(0.5, y), (0.5, y + 0.09)], yellow);
            }
        }
        Icon::Stretch => {
            pen.line(&[(0.45, 0.25), (0.12, 0.25), (0.12, 0.75), (0.45, 0.75)], c);
            pen.arrow((0.42, 0.5), (0.9, 0.5), blue);
        }
        Icon::Scale => {
            pen.closed(&[(0.12, 0.55), (0.45, 0.55), (0.45, 0.88), (0.12, 0.88)], c);
            pen.closed(&[(0.12, 0.12), (0.88, 0.12), (0.88, 0.88), (0.12, 0.88)], blue);
            pen.arrow((0.45, 0.55), (0.78, 0.22), c);
        }
        Icon::Trim => {
            pen.line(&[(0.5, 0.1), (0.5, 0.9)], c);
            pen.line(&[(0.1, 0.5), (0.5, 0.5)], c);
            pen.line(&[(0.5, 0.5), (0.9, 0.5)], red);
            pen.line(&[(0.64, 0.38), (0.78, 0.62)], red);
            pen.line(&[(0.64, 0.62), (0.78, 0.38)], red);
        }
        Icon::Extend => {
            // A line reaching out to a boundary.
            pen.line(&[(0.85, 0.1), (0.85, 0.9)], c);
            pen.line(&[(0.1, 0.5), (0.45, 0.5)], c);
            pen.arrow((0.45, 0.5), (0.82, 0.5), blue);
        }
        Icon::Chamfer => {
            pen.line(&[(0.15, 0.85), (0.15, 0.45)], c);
            pen.line(&[(0.55, 0.15), (0.85, 0.15)], c);
            pen.line(&[(0.15, 0.45), (0.55, 0.15)], blue);
        }
        Icon::Break => {
            pen.line(&[(0.1, 0.65), (0.4, 0.45)], c);
            pen.line(&[(0.6, 0.35), (0.9, 0.15)], c);
            pen.grip((0.4, 0.45), blue);
            pen.grip((0.6, 0.35), blue);
        }
        Icon::Join => {
            pen.line(&[(0.1, 0.7), (0.5, 0.5)], c);
            pen.line(&[(0.5, 0.5), (0.9, 0.3)], blue);
            pen.grip((0.5, 0.5), blue);
        }
        Icon::Lengthen => {
            pen.line(&[(0.1, 0.8), (0.55, 0.45)], c);
            pen.arrow((0.55, 0.45), (0.9, 0.18), blue);
        }
        Icon::Fillet => {
            pen.line(&[(0.15, 0.85), (0.15, 0.5)], c);
            pen.arc((0.5, 0.5), 0.35, PI, PI * 1.5, blue, false);
            pen.line(&[(0.5, 0.15), (0.85, 0.15)], c);
        }
        Icon::Array => {
            for i in 0..3 {
                for j in 0..3 {
                    let (x, y) = (0.2 + 0.3 * i as f32, 0.2 + 0.3 * j as f32);
                    let col = if i == 0 && j == 2 { c } else { blue };
                    pen.closed(
                        &[(x - 0.09, y - 0.09), (x + 0.09, y - 0.09), (x + 0.09, y + 0.09), (x - 0.09, y + 0.09)],
                        col,
                    );
                }
            }
        }
        Icon::Erase => {
            pen.closed(&[(0.15, 0.6), (0.5, 0.25), (0.85, 0.6), (0.5, 0.95)], c);
            pen.filled(&[(0.5, 0.25), (0.85, 0.6), (0.68, 0.77), (0.33, 0.42)], red);
            pen.line(&[(0.1, 0.92), (0.9, 0.92)], c);
        }
        Icon::Offset => {
            pen.arc((0.75, 0.75), 0.55, PI, PI * 1.5, c, false);
            pen.arc((0.75, 0.75), 0.32, PI, PI * 1.5, blue, false);
        }
        Icon::Explode => {
            for a in 0..8 {
                let a = a as f32 * TAU / 8.0;
                pen.line(
                    &[(0.5 + 0.15 * a.cos(), 0.5 + 0.15 * a.sin()), (0.5 + 0.38 * a.cos(), 0.5 + 0.38 * a.sin())],
                    c,
                );
            }
            pen.dot((0.5, 0.5), 0.06, green);
        }
        Icon::Text => {
            pen.line(&[(0.15, 0.88), (0.5, 0.1), (0.85, 0.88)], c);
            pen.line(&[(0.3, 0.58), (0.7, 0.58)], c);
            pen.line(&[(0.1, 0.94), (0.9, 0.94)], blue);
        }
        Icon::Dimension => {
            pen.line(&[(0.12, 0.2), (0.12, 0.75)], c);
            pen.line(&[(0.88, 0.2), (0.88, 0.75)], c);
            pen.arrow((0.5, 0.6), (0.14, 0.6), blue);
            pen.arrow((0.5, 0.6), (0.86, 0.6), blue);
            pen.line(&[(0.38, 0.38), (0.62, 0.38)], yellow);
        }
        // A smaller A on its baseline, its start marked.
        Icon::SingleLineText => {
            pen.line(&[(0.25, 0.8), (0.5, 0.25), (0.75, 0.8)], c);
            pen.line(&[(0.35, 0.6), (0.65, 0.6)], c);
            pen.line(&[(0.1, 0.8), (0.9, 0.8)], blue);
            pen.grip((0.1, 0.8), yellow);
        }
        // Two lines from a vertex, the angle's arc between them.
        Icon::DimAngular => {
            pen.line(&[(0.12, 0.88), (0.9, 0.88)], c);
            pen.line(&[(0.12, 0.88), (0.7, 0.2)], c);
            pen.arc((0.12, 0.88), 0.55, -0.86, 0.0, blue, true);
        }
        Icon::DimRadius | Icon::DimDiameter => {
            pen.circle((0.5, 0.5), 0.36, c);
            if icon == Icon::DimRadius {
                pen.arrow((0.5, 0.5), (0.75, 0.25), blue);
                pen.dot((0.5, 0.5), 0.04, yellow);
            } else {
                pen.arrow((0.5, 0.5), (0.75, 0.25), blue);
                pen.arrow((0.5, 0.5), (0.25, 0.75), blue);
            }
        }
        // Dimensions end to end, or all from one baseline.
        Icon::DimContinue | Icon::DimBaseline => {
            for x in [0.1, 0.5, 0.9] {
                pen.line(&[(x, 0.9), (x, 0.3)], c);
            }
            if icon == Icon::DimContinue {
                pen.line(&[(0.1, 0.45), (0.5, 0.45)], blue);
                pen.line(&[(0.5, 0.45), (0.9, 0.45)], yellow);
            } else {
                pen.line(&[(0.1, 0.55), (0.5, 0.55)], blue);
                pen.line(&[(0.1, 0.35), (0.9, 0.35)], yellow);
            }
        }
        Icon::Leader => {
            pen.arrow((0.55, 0.45), (0.12, 0.88), c);
            pen.line(&[(0.55, 0.45), (0.9, 0.45)], c);
            pen.line(&[(0.55, 0.25), (0.9, 0.25)], yellow);
        }
        Icon::Table => {
            pen.closed(&[(0.1, 0.2), (0.9, 0.2), (0.9, 0.82), (0.1, 0.82)], c);
            pen.line(&[(0.1, 0.4), (0.9, 0.4)], c);
            pen.line(&[(0.1, 0.61), (0.9, 0.61)], c);
            pen.line(&[(0.5, 0.4), (0.5, 0.82)], c);
            pen.filled(&[(0.1, 0.2), (0.9, 0.2), (0.9, 0.4), (0.1, 0.4)], blue.gamma_multiply(0.6));
        }
        Icon::LayerProperties => {
            for (i, col) in [(0, c), (1, yellow), (2, blue)] {
                let y = 0.25 + 0.2 * i as f32;
                pen.closed(&[(0.3, y), (0.9, y), (0.7, y + 0.2), (0.1, y + 0.2)], col);
            }
        }
        Icon::MakeCurrent => {
            for (i, col) in [(0, c), (1, blue)] {
                let y = 0.3 + 0.24 * i as f32;
                pen.closed(&[(0.25, y), (0.75, y), (0.55, y + 0.2), (0.05, y + 0.2)], col);
            }
            pen.line(&[(0.58, 0.32), (0.72, 0.46), (0.95, 0.12)], green);
        }
        // One layer standing out from dimmed ones; all of them back.
        Icon::LayerIsolate | Icon::LayerUnisolate => {
            let isolated = icon == Icon::LayerIsolate;
            for (i, col) in [(0, c), (1, yellow), (2, blue)] {
                let y = 0.18 + 0.24 * i as f32;
                let col = if isolated && i != 1 { col.gamma_multiply(0.3) } else { col };
                pen.closed(&[(0.3, y), (0.9, y), (0.7, y + 0.2), (0.1, y + 0.2)], col);
            }
        }
        // A layer with its state symbol: bulb, snowflake, lock.
        Icon::LayerOff | Icon::LayerFreeze | Icon::LayerLock | Icon::LayerUnlock => {
            pen.closed(&[(0.2, 0.62), (0.75, 0.62), (0.55, 0.85), (0.0, 0.85)], c);
            match icon {
                Icon::LayerOff => {
                    pen.circle((0.68, 0.28), 0.17, c);
                    pen.line(&[(0.62, 0.45), (0.74, 0.45)], c);
                }
                Icon::LayerFreeze => {
                    for (dx, dy) in [(0.0, 0.2), (0.17, 0.1), (0.17, -0.1)] {
                        pen.line(&[(0.68 - dx, 0.3 - dy), (0.68 + dx, 0.3 + dy)], blue);
                    }
                }
                _ => {
                    pen.closed(&[(0.52, 0.3), (0.86, 0.3), (0.86, 0.55), (0.52, 0.55)], yellow);
                    let (x1, lift) = if icon == Icon::LayerLock { (0.79, 0.0) } else { (0.97, 0.06) };
                    pen.line(&[(0.59, 0.3), (0.59, 0.12 - lift), (x1 - 0.07, 0.12 - lift), (x1, 0.18 - lift)], yellow);
                }
            }
        }
        // A point in style 35: a dot in a circle with an X.
        Icon::Point => {
            pen.circle((0.5, 0.5), 0.3, c);
            pen.line(&[(0.3, 0.3), (0.7, 0.7)], c);
            pen.line(&[(0.3, 0.7), (0.7, 0.3)], c);
            pen.dot((0.5, 0.5), 0.07, blue);
        }
        Icon::Plot => {
            // A printer with a sheet coming out.
            pen.closed(&[(0.3, 0.12), (0.7, 0.12), (0.7, 0.38), (0.3, 0.38)], c);
            pen.closed(&[(0.12, 0.38), (0.88, 0.38), (0.88, 0.7), (0.12, 0.7)], c);
            pen.filled(&[(0.3, 0.62), (0.7, 0.62), (0.7, 0.9), (0.3, 0.9)], blue);
            pen.dot((0.78, 0.48), 0.035, green);
        }
        Icon::Attach => {
            // A sheet with a paper clip.
            pen.closed(&[(0.18, 0.2), (0.7, 0.2), (0.7, 0.88), (0.18, 0.88)], c);
            pen.line(&[(0.3, 0.45), (0.58, 0.45)], c);
            pen.line(&[(0.3, 0.6), (0.5, 0.6)], c);
            pen.line(&[(0.76, 0.5), (0.76, 0.12), (0.88, 0.12), (0.88, 0.62)], blue);
            pen.line(&[(0.82, 0.18), (0.82, 0.5)], blue);
        }
        Icon::BringToFront => {
            pen.closed(&[(0.1, 0.4), (0.6, 0.4), (0.6, 0.9), (0.1, 0.9)], c);
            pen.filled(&[(0.4, 0.1), (0.9, 0.1), (0.9, 0.6), (0.4, 0.6)], blue);
        }
        Icon::SendToBack => {
            pen.filled(&[(0.4, 0.1), (0.9, 0.1), (0.9, 0.6), (0.4, 0.6)], blue);
            pen.filled(&[(0.1, 0.4), (0.6, 0.4), (0.6, 0.9), (0.1, 0.9)], crate::theme::ribbon());
            pen.closed(&[(0.1, 0.4), (0.6, 0.4), (0.6, 0.9), (0.1, 0.9)], c);
        }
        Icon::Image => {
            // A picture: hills under a sun.
            pen.closed(&[(0.1, 0.18), (0.9, 0.18), (0.9, 0.82), (0.1, 0.82)], c);
            pen.line(&[(0.1, 0.78), (0.38, 0.45), (0.56, 0.64), (0.7, 0.5), (0.9, 0.72)], green);
            pen.dot((0.7, 0.32), 0.07, yellow);
        }
        Icon::Pdf => {
            pen.closed(&[(0.2, 0.08), (0.64, 0.08), (0.8, 0.24), (0.8, 0.92), (0.2, 0.92)], c);
            pen.filled(&[(0.12, 0.5), (0.88, 0.5), (0.88, 0.74), (0.12, 0.74)], red);
        }
        Icon::ImageClip => {
            pen.closed(&[(0.1, 0.18), (0.9, 0.18), (0.9, 0.82), (0.1, 0.82)], c);
            pen.closed(&[(0.3, 0.32), (0.75, 0.32), (0.75, 0.7), (0.3, 0.7)], blue);
            pen.grip((0.3, 0.32), blue);
            pen.grip((0.75, 0.7), blue);
        }
        Icon::ExternalReferences => {
            // A drawing with another one linked in.
            pen.closed(&[(0.1, 0.12), (0.62, 0.12), (0.62, 0.7), (0.1, 0.7)], c);
            pen.closed(&[(0.38, 0.38), (0.9, 0.38), (0.9, 0.9), (0.38, 0.9)], blue);
            pen.line(&[(0.5, 0.75), (0.78, 0.55)], blue);
        }
        Icon::PlotPreview => {
            pen.closed(&[(0.14, 0.1), (0.62, 0.1), (0.62, 0.78), (0.14, 0.78)], c);
            pen.line(&[(0.22, 0.25), (0.54, 0.25)], c);
            pen.line(&[(0.22, 0.38), (0.48, 0.38)], c);
            pen.circle((0.62, 0.62), 0.16, blue);
            pen.line(&[(0.73, 0.73), (0.9, 0.9)], blue);
        }
        Icon::Polygon => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|k| (0.5 + 0.36 * (PI / 3.0 * k as f32).cos(), 0.5 + 0.36 * (PI / 3.0 * k as f32).sin()))
                .collect();
            pen.closed(&pts, c);
            pen.grip((0.5, 0.5), blue);
            pen.grip(pts[0], blue);
        }
        Icon::Ellipse => {
            let pts: Vec<(f32, f32)> = (0..=32)
                .map(|k| {
                    let a = PI * 2.0 * k as f32 / 32.0;
                    (0.5 + 0.38 * a.cos(), 0.5 + 0.22 * a.sin())
                })
                .collect();
            pen.line(&pts, c);
            pen.grip((0.5, 0.5), blue);
            pen.grip((0.88, 0.5), blue);
            pen.grip((0.5, 0.28), blue);
        }
        Icon::Spline => {
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|k| {
                    let t = k as f32 / 24.0;
                    (0.1 + 0.8 * t, 0.5 - 0.28 * (t * PI * 2.0).sin())
                })
                .collect();
            pen.line(&pts, c);
            for p in [(0.1, 0.5), (0.3, 0.22), (0.7, 0.78), (0.9, 0.5)] {
                pen.grip(p, blue);
            }
        }
        Icon::Xline => {
            pen.line(&[(0.02, 0.86), (0.98, 0.14)], c);
            pen.line(&[(0.02, 0.6), (0.98, 0.6)], c);
            pen.grip((0.5, 0.5), blue);
        }
        Icon::Ray => {
            pen.line(&[(0.18, 0.82), (0.98, 0.1)], c);
            pen.line(&[(0.18, 0.82), (0.98, 0.62)], c);
            pen.grip((0.18, 0.82), blue);
        }
        Icon::RevCloud => {
            // Scallops round a rectangle.
            let (x0, y0, x1, y1) = (0.16, 0.26, 0.84, 0.74);
            let r = 0.085;
            for k in 0..4 {
                let x = x0 + (x1 - x0) * (k as f32 + 0.5) / 4.0;
                pen.arc((x, y0), r, PI, PI * 2.0, c, false);
                pen.arc((x, y1), r, 0.0, PI, c, false);
            }
            for k in 0..3 {
                let y = y0 + (y1 - y0) * (k as f32 + 0.5) / 3.0;
                pen.arc((x0, y), r, PI * 0.5, PI * 1.5, c, false);
                pen.arc((x1, y), r, -PI * 0.5, PI * 0.5, c, false);
            }
        }
        Icon::Donut => {
            painter.circle_stroke(pen.p(0.5, 0.5), pen.s(0.25), Stroke::new(pen.s(0.16), c));
            pen.grip((0.5, 0.5), blue);
        }
        Icon::EditPolyline => {
            pen.line(&[(0.1, 0.8), (0.35, 0.35), (0.6, 0.65), (0.9, 0.2)], c);
            for p in [(0.1, 0.8), (0.35, 0.35), (0.6, 0.65), (0.9, 0.2)] {
                pen.grip(p, blue);
            }
        }
        Icon::Pan => {
            for to in [(0.5, 0.08), (0.92, 0.5), (0.5, 0.92), (0.08, 0.5)] {
                pen.arrow((0.5, 0.5), to, c);
            }
            pen.dot((0.5, 0.5), 0.07, blue);
        }
        // A sheet with a tab, and a plus.
        Icon::NewLayout => {
            pen.closed(&[(0.08, 0.3), (0.78, 0.3), (0.78, 0.82), (0.08, 0.82)], c);
            pen.line(&[(0.08, 0.3), (0.12, 0.18), (0.38, 0.18), (0.42, 0.3)], c);
            pen.line(&[(0.78, 0.5), (0.78, 0.9)], green);
            pen.line(&[(0.58, 0.7), (0.98, 0.7)], green);
        }
        // A sheet with its printable area marked.
        Icon::PageSetup => {
            pen.closed(&[(0.1, 0.2), (0.9, 0.2), (0.9, 0.8), (0.1, 0.8)], c);
            for (a, b) in
                [((0.2, 0.3), (0.8, 0.3)), ((0.8, 0.3), (0.8, 0.7)), ((0.8, 0.7), (0.2, 0.7)), ((0.2, 0.7), (0.2, 0.3))]
            {
                for t in [0.0, 0.4, 0.8] {
                    let at = |k: f32| (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k);
                    pen.line(&[at(t), at(t + 0.2)], blue);
                }
            }
        }
        // A viewport on a sheet, model space showing in it.
        Icon::Viewport | Icon::ViewportLock | Icon::ViewportUnlock => {
            pen.closed(&[(0.06, 0.18), (0.94, 0.18), (0.94, 0.82), (0.06, 0.82)], c);
            pen.closed(&[(0.16, 0.28), (0.66, 0.28), (0.66, 0.72), (0.16, 0.72)], blue);
            pen.circle((0.41, 0.5), 0.12, c);
            if icon != Icon::Viewport {
                pen.filled(&[(0.62, 0.58), (0.92, 0.58), (0.92, 0.88), (0.62, 0.88)], yellow);
                let lift = if icon == Icon::ViewportLock { 0.0 } else { 0.08 };
                pen.line(&[(0.67, 0.58), (0.67, 0.44 - lift), (0.87, 0.44 - lift), (0.87, 0.58 - lift * 2.0)], yellow);
            }
        }
        // A frame with arrows out of its corners.
        Icon::Maximize => {
            pen.closed(&[(0.3, 0.3), (0.7, 0.3), (0.7, 0.7), (0.3, 0.7)], blue);
            for (from, to) in [
                ((0.3, 0.3), (0.06, 0.06)),
                ((0.7, 0.3), (0.94, 0.06)),
                ((0.7, 0.7), (0.94, 0.94)),
                ((0.3, 0.7), (0.06, 0.94)),
            ] {
                pen.arrow(from, to, c);
            }
        }
        Icon::Zoom => {
            pen.circle((0.42, 0.42), 0.27, c);
            pen.line(&[(0.62, 0.62), (0.9, 0.9)], c);
            pen.line(&[(0.3, 0.42), (0.54, 0.42)], blue);
            pen.line(&[(0.42, 0.3), (0.42, 0.54)], blue);
        }
        Icon::Measure => {
            pen.closed(&[(0.08, 0.38), (0.92, 0.38), (0.92, 0.66), (0.08, 0.66)], c);
            for i in 1..8 {
                let x = 0.08 + 0.105 * i as f32;
                let h = if i % 2 == 0 { 0.16 } else { 0.09 };
                pen.line(&[(x, 0.38), (x, 0.38 + h)], yellow);
            }
        }
        // A block (a square with its diagonal) and what's done to it.
        Icon::EditReference
        | Icon::SaveReference
        | Icon::DiscardReference
        | Icon::AddToWorkingSet
        | Icon::RemoveFromWorkingSet => {
            pen.closed(&[(0.1, 0.1), (0.62, 0.1), (0.62, 0.62), (0.1, 0.62)], c);
            pen.line(&[(0.1, 0.62), (0.62, 0.1)], c);
            match icon {
                Icon::EditReference => {
                    pen.line(&[(0.45, 0.92), (0.92, 0.45)], blue);
                    pen.line(&[(0.38, 0.99), (0.45, 0.92)], yellow);
                }
                Icon::SaveReference => pen.line(&[(0.48, 0.72), (0.62, 0.88), (0.94, 0.5)], green),
                Icon::DiscardReference => {
                    pen.line(&[(0.52, 0.52), (0.9, 0.9)], red);
                    pen.line(&[(0.9, 0.52), (0.52, 0.9)], red);
                }
                Icon::AddToWorkingSet => {
                    pen.line(&[(0.5, 0.72), (0.94, 0.72)], green);
                    pen.line(&[(0.72, 0.5), (0.72, 0.94)], green);
                }
                _ => pen.line(&[(0.5, 0.72), (0.94, 0.72)], red),
            }
        }
        Icon::InsertBlock => {
            pen.closed(&[(0.35, 0.1), (0.9, 0.1), (0.9, 0.65), (0.35, 0.65)], blue);
            pen.line(&[(0.35, 0.65), (0.9, 0.1)], blue);
            pen.arrow((0.08, 0.92), (0.42, 0.58), c);
        }
        Icon::CreateBlock => {
            pen.closed(&[(0.12, 0.25), (0.75, 0.25), (0.75, 0.88), (0.12, 0.88)], c);
            pen.line(&[(0.12, 0.88), (0.75, 0.25)], c);
            for a in 0..4 {
                let a = a as f32 * PI / 4.0;
                pen.line(
                    &[(0.78 + 0.16 * a.cos(), 0.22 + 0.16 * a.sin()), (0.78 - 0.16 * a.cos(), 0.22 - 0.16 * a.sin())],
                    yellow,
                );
            }
        }
        Icon::MatchProperties => {
            pen.closed(&[(0.15, 0.12), (0.7, 0.12), (0.7, 0.38), (0.15, 0.38)], c);
            pen.filled(&[(0.15, 0.12), (0.7, 0.12), (0.7, 0.38), (0.15, 0.38)], yellow.gamma_multiply(0.7));
            pen.line(&[(0.7, 0.25), (0.85, 0.25), (0.85, 0.5), (0.45, 0.5), (0.45, 0.62)], c);
            pen.closed(&[(0.38, 0.62), (0.52, 0.62), (0.52, 0.92), (0.38, 0.92)], c);
        }
        Icon::Properties => {
            pen.closed(&[(0.15, 0.08), (0.85, 0.08), (0.85, 0.92), (0.15, 0.92)], c);
            for i in 0..4 {
                let y = 0.25 + 0.18 * i as f32;
                pen.line(&[(0.25, y), (0.42, y)], blue);
                pen.line(&[(0.5, y), (0.75, y)], c);
            }
        }
        Icon::New => {
            pen.closed(&[(0.22, 0.1), (0.62, 0.1), (0.8, 0.28), (0.8, 0.9), (0.22, 0.9)], c);
            pen.line(&[(0.62, 0.1), (0.62, 0.28), (0.8, 0.28)], c);
        }
        Icon::Open => {
            pen.closed(&[(0.1, 0.25), (0.4, 0.25), (0.48, 0.35), (0.85, 0.35), (0.85, 0.82), (0.1, 0.82)], yellow);
        }
        Icon::Save => {
            pen.closed(&[(0.15, 0.15), (0.75, 0.15), (0.88, 0.28), (0.88, 0.88), (0.15, 0.88)], c);
            pen.closed(&[(0.3, 0.15), (0.68, 0.15), (0.68, 0.38), (0.3, 0.38)], c);
            pen.closed(&[(0.28, 0.58), (0.75, 0.58), (0.75, 0.88), (0.28, 0.88)], blue);
        }
        Icon::Undo => {
            pen.arc((0.55, 0.58), 0.3, 0.2, -PI * 1.05, c, true);
        }
        Icon::Redo => {
            pen.arc((0.45, 0.58), 0.3, PI - 0.2, PI * 2.05, c, true);
        }
    }
}

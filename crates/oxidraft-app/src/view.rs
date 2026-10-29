//! World <-> screen transform for the drawing area.

use eframe::egui::{Pos2, Rect, pos2};
use oxidraft_geom::{Aabb, Point};

/// Which part of the drawing is on screen. World Y points up, screen Y down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// World point shown at the center of the canvas.
    pub center: Point,
    /// Screen points per drawing unit.
    pub scale: f64,
    /// Degrees the drawing shows turned counterclockwise (a layout
    /// viewport's twist); 0 everywhere else.
    pub rotation: f64,
}

impl Default for View {
    fn default() -> Self {
        Self { center: Point::new(50.0, 30.0), scale: 8.0, rotation: 0.0 }
    }
}

impl View {
    /// A view without any turn.
    pub fn new(center: Point, scale: f64) -> View {
        View { center, scale, rotation: 0.0 }
    }

    pub fn to_screen(self, rect: Rect, p: Point) -> Pos2 {
        let c = rect.center();
        let d = (p - self.center).rotated_about(Point::ORIGIN, self.rotation);
        pos2(c.x + (d.x * self.scale) as f32, c.y - (d.y * self.scale) as f32)
    }

    pub fn to_world(self, rect: Rect, s: Pos2) -> Point {
        let c = rect.center();
        let d = Point::new(f64::from(s.x - c.x) / self.scale, -f64::from(s.y - c.y) / self.scale);
        self.center + d.rotated_about(Point::ORIGIN, -self.rotation)
    }

    /// Converts a screen distance (points) to drawing units.
    pub fn units(&self, screen: f32) -> f64 {
        f64::from(screen) / self.scale
    }

    /// Pans by a screen-space drag delta.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let d = Point::new(f64::from(dx), -f64::from(dy)) * (1.0 / self.scale);
        self.center = self.center - d.rotated_about(Point::ORIGIN, -self.rotation);
    }

    /// Zooms by `factor`, keeping the world point under `anchor` fixed.
    pub fn zoom_at(&mut self, rect: Rect, anchor: Pos2, factor: f64) {
        let before = self.to_world(rect, anchor);
        self.scale = (self.scale * factor).clamp(1e-6, 1e9);
        let after = self.to_world(rect, anchor);
        self.center = self.center + (before - after);
    }

    /// Fits `bounds` into the `fit` part of the canvas `rect` (e.g. leaving
    /// room for the floating command line).
    pub fn zoom_extents(&mut self, rect: Rect, fit: Rect, bounds: Aabb) {
        // The bounds as they lie on screen, turned with the view.
        let turned = bounds.corners().map(|p| (p - bounds.center()).rotated_about(Point::ORIGIN, self.rotation));
        let on_screen =
            turned.iter().fold(Aabb::from_points(turned[0], turned[0]), |b, p| b.union(Aabb::from_points(*p, *p)));
        let w = on_screen.width().max(1e-6);
        let h = on_screen.height().max(1e-6);
        self.scale = (f64::from(fit.width()) / w).min(f64::from(fit.height()) / h) * 0.95;
        let offset = fit.center() - rect.center();
        let shift = Point::new(f64::from(offset.x), -f64::from(offset.y)) * (1.0 / self.scale);
        self.center = bounds.center() - shift.rotated_about(Point::ORIGIN, -self.rotation);
    }

    /// Adaptive grid spacing in drawing units as `(minor, major)`: the minor
    /// step is the smallest of 1, 5, 10, 50, … that is at least `min_px` apart
    /// on screen, and major lines fall on the next power of ten above it
    /// (or every 5 minor lines when the minor step is itself a power of ten).
    pub fn grid_step(&self, min_px: f64) -> (f64, f64) {
        let units = min_px / self.scale;
        let decade = 10f64.powf(units.log10().floor());
        if decade >= units {
            (decade, decade * 5.0)
        } else if decade * 5.0 >= units {
            (decade * 5.0, decade * 10.0)
        } else {
            (decade * 10.0, decade * 50.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_world_roundtrip_and_zoom_anchor() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));
        let mut v = View::default();
        let p = Point::new(12.5, -3.0);
        assert!(v.to_world(rect, v.to_screen(rect, p)).distance(p) < 1e-4);

        let anchor = pos2(100.0, 450.0);
        let world = v.to_world(rect, anchor);
        v.zoom_at(rect, anchor, 2.5);
        assert!(v.to_world(rect, anchor).distance(world) < 1e-4);
        // Turned, a quarter turn: up the drawing is left on screen.
        let turned = View { rotation: 90.0, ..View::new(Point::ORIGIN, 1.0) };
        let up = turned.to_screen(rect, Point::new(0.0, 10.0));
        assert!((up - pos2(390.0, 300.0)).length() < 1e-3);
        assert!(turned.to_world(rect, up).distance(Point::new(0.0, 10.0)) < 1e-9);
    }

    #[test]
    fn grid_steps_follow_one_five_sequence() {
        let step = |scale: f64| View::new(Point::ORIGIN, scale).grid_step(10.0);
        assert_eq!(step(10.0), (1.0, 5.0));
        assert_eq!(step(8.0), (5.0, 10.0));
        assert_eq!(step(1.5), (10.0, 50.0));
        assert_eq!(step(0.3), (50.0, 100.0));
    }
}

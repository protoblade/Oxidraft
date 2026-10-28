//! World <-> screen transform for the drawing area.

use eframe::egui::{Pos2, Rect, pos2};
use oxidraft_geom::{Aabb, Point};

/// Which part of the drawing is on screen. World Y points up, screen Y down.
#[derive(Debug, Clone, Copy)]
pub struct View {
    /// World point shown at the center of the canvas.
    pub center: Point,
    /// Screen points per drawing unit.
    pub scale: f64,
}

impl Default for View {
    fn default() -> Self {
        Self { center: Point::new(50.0, 30.0), scale: 8.0 }
    }
}

impl View {
    pub fn to_screen(self, rect: Rect, p: Point) -> Pos2 {
        let c = rect.center();
        pos2(c.x + ((p.x - self.center.x) * self.scale) as f32, c.y - ((p.y - self.center.y) * self.scale) as f32)
    }

    pub fn to_world(self, rect: Rect, s: Pos2) -> Point {
        let c = rect.center();
        Point::new(self.center.x + f64::from(s.x - c.x) / self.scale, self.center.y - f64::from(s.y - c.y) / self.scale)
    }

    /// Converts a screen distance (points) to drawing units.
    pub fn units(&self, screen: f32) -> f64 {
        f64::from(screen) / self.scale
    }

    /// Pans by a screen-space drag delta.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.center.x -= f64::from(dx) / self.scale;
        self.center.y += f64::from(dy) / self.scale;
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
        let w = bounds.width().max(1e-6);
        let h = bounds.height().max(1e-6);
        self.scale = (f64::from(fit.width()) / w).min(f64::from(fit.height()) / h) * 0.95;
        let offset = fit.center() - rect.center();
        self.center = Point::new(
            bounds.center().x - f64::from(offset.x) / self.scale,
            bounds.center().y + f64::from(offset.y) / self.scale,
        );
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
    }

    #[test]
    fn grid_steps_follow_one_five_sequence() {
        let step = |scale: f64| View { center: Point::ORIGIN, scale }.grid_step(10.0);
        assert_eq!(step(10.0), (1.0, 5.0));
        assert_eq!(step(8.0), (5.0, 10.0));
        assert_eq!(step(1.5), (10.0, 50.0));
        assert_eq!(step(0.3), (50.0, 100.0));
    }
}

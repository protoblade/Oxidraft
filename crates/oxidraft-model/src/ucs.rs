//! User coordinate systems (UCS), in 2D: an origin and the angle of the
//! X axis. Typed coordinates, ortho, polar tracking, snap and the grid
//! follow the current one.

use std::cell::Cell;

use oxidraft_geom::Point;

/// A coordinate system: its origin and X axis's angle (degrees, from
/// the world's X axis), both in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ucs {
    pub origin: Point,
    pub angle: f64,
}

/// The world coordinate system (WCS).
pub const WORLD: Ucs = Ucs { origin: Point::ORIGIN, angle: 0.0 };

impl Default for Ucs {
    fn default() -> Self {
        WORLD
    }
}

impl Ucs {
    pub fn is_world(&self) -> bool {
        self.origin.approx_eq(Point::ORIGIN) && self.angle.rem_euclid(360.0).abs() < 1e-9
    }

    /// A point given in this UCS, in world coordinates.
    pub fn to_world(&self, p: Point) -> Point {
        self.origin + self.vector_to_world(p)
    }

    /// A world point in this UCS.
    pub fn to_ucs(&self, p: Point) -> Point {
        (p - self.origin).rotated_about(Point::ORIGIN, -self.angle)
    }

    /// A displacement given in this UCS, in world coordinates.
    pub fn vector_to_world(&self, v: Point) -> Point {
        v.rotated_about(Point::ORIGIN, self.angle)
    }

    /// The X axis's direction.
    pub fn x_axis(&self) -> Point {
        Point::ORIGIN.polar(1.0, self.angle)
    }

    /// From an origin and a point along the X axis.
    pub fn through(origin: Point, on_x: Point) -> Option<Ucs> {
        (origin.distance(on_x) > 1e-9).then(|| Ucs { origin, angle: origin.angle_to(on_x) })
    }
}

/// A UCS saved under a name (UCS NAmed Save, UCSMAN).
#[derive(Debug, Clone, PartialEq)]
pub struct NamedUcs {
    pub name: String,
    pub ucs: Ucs,
}

thread_local! {
    /// The UCS of the drawing being worked on.
    static CURRENT: Cell<Ucs> = const { Cell::new(WORLD) };
}

/// The UCS in effect.
pub fn current() -> Ucs {
    CURRENT.get()
}

/// Puts a drawing's UCS in effect (the app and the command engine do,
/// as they work on it).
pub fn set_current(ucs: Ucs) {
    CURRENT.set(ucs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_go_both_ways() {
        let ucs = Ucs { origin: Point::new(10.0, 5.0), angle: 90.0 };
        let w = ucs.to_world(Point::new(2.0, 0.0));
        assert!(w.approx_eq(Point::new(10.0, 7.0)), "{w:?}");
        assert!(ucs.to_ucs(w).approx_eq(Point::new(2.0, 0.0)));
        assert!(Ucs::through(Point::ORIGIN, Point::new(1.0, 1.0)).is_some_and(|u| (u.angle - 45.0).abs() < 1e-9));
        assert!(WORLD.is_world() && !ucs.is_world());
    }
}

//! WIPEOUT: a polygon that hides what's under it, from points picked or
//! a closed polyline; Frames sets whether wipeouts' frames show.

use oxidraft_geom::Point;
use oxidraft_model::{Geometry, Vertex};

use crate::{Command, Ctx, Input, Prompt, Step};

#[derive(Clone, PartialEq)]
enum Stage {
    First,
    Next(Vec<Point>),
    Frames,
    Polyline,
    /// The polyline's points, and whether to erase it.
    Erase(Vec<Point>, oxidraft_model::EntityId),
}

pub struct Wipeout {
    stage: Stage,
}

impl Default for Wipeout {
    fn default() -> Self {
        Self { stage: Stage::First }
    }
}

impl Command for Wipeout {
    fn prompt(&self) -> Prompt {
        match &self.stage {
            Stage::First => {
                Prompt::new("Specify first point").with_keywords(&["Frames", "Polyline"]).with_default("Polyline")
            }
            Stage::Next(points) if points.len() < 3 => Prompt::new("Specify next point").with_keywords(&["Undo"]),
            Stage::Next(_) => Prompt::new("Specify next point").with_keywords(&["Undo"]).with_default("close"),
            Stage::Frames => Prompt::new("Enter mode").with_keywords(&["ON", "OFF", "Display but not plot"]),
            Stage::Polyline => Prompt::new("Select a closed polyline"),
            Stage::Erase(..) => Prompt::new("Erase polyline?").with_keywords(&["Yes", "No"]).with_default("No"),
        }
    }

    fn input(&mut self, input: Input, ctx: &mut Ctx) -> Result<Step, String> {
        match (self.stage.clone(), input) {
            (Stage::First, Input::Point(p)) => self.stage = Stage::Next(vec![p]),
            (Stage::First, Input::Keyword("Frames")) => self.stage = Stage::Frames,
            (Stage::First, Input::Keyword(_) | Input::Enter) => self.stage = Stage::Polyline,
            (Stage::Next(mut points), Input::Point(p)) => {
                points.push(p);
                self.stage = Stage::Next(points);
            }
            (Stage::Next(mut points), Input::Keyword(_)) => {
                points.pop();
                self.stage = if points.is_empty() { Stage::First } else { Stage::Next(points) };
            }
            (Stage::Next(points), Input::Enter) if points.len() >= 3 => {
                ctx.doc.add(Geometry::Wipeout { points });
                return Ok(Step::Done);
            }
            (Stage::Next(_), Input::Enter) => return Err("A wipeout needs three points or more.".into()),
            (Stage::Frames, Input::Keyword(k)) => {
                ctx.doc.set_wipeout_frame(match k {
                    "ON" => 1,
                    "OFF" => 0,
                    _ => 2,
                });
                return Ok(Step::Done);
            }
            (Stage::Polyline, Input::Pick { at, .. }) => {
                let id = ctx.pick(at).ok_or("No object found.")?;
                let points = match ctx.doc.entity(id).map(|e| &e.geometry) {
                    Some(Geometry::Polyline { vertices, closed: true }) if vertices.len() >= 3 => {
                        if vertices.iter().any(|v| v.bulge != 0.0) {
                            return Err("The polyline must be made of lines only.".into());
                        }
                        vertices.iter().map(|v| v.point).collect()
                    }
                    _ => return Err("Choose a closed polyline.".into()),
                };
                self.stage = Stage::Erase(points, id);
            }
            (Stage::Erase(points, id), input) => {
                if input == Input::Keyword("Yes") {
                    ctx.doc.remove(id);
                }
                ctx.doc.add(Geometry::Wipeout { points });
                return Ok(Step::Done);
            }
            (_, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Specify a point, or choose an option.".into()),
        }
        Ok(Step::Continue)
    }

    fn preview(&self, cursor: Point) -> Vec<Geometry> {
        match &self.stage {
            Stage::Next(points) => {
                let vertices = points.iter().chain([&cursor]).map(|p| Vertex::at(*p)).collect();
                vec![Geometry::Polyline { vertices, closed: true }]
            }
            _ => Vec::new(),
        }
    }

    fn base_point(&self) -> Option<Point> {
        match &self.stage {
            Stage::Next(points) => points.last().copied(),
            _ => None,
        }
    }

    fn wants_pick(&self) -> bool {
        self.stage == Stage::Polyline
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use oxidraft_geom::Point;
    use oxidraft_model::{Document, Geometry};

    #[test]
    fn wipeout_from_points_and_from_a_polyline() {
        let (mut e, mut d) = (Engine::new(), Document::new());
        e.dynamic_input = false;
        for t in ["wipeout", "0,0", "10,0", "10,10", ""] {
            e.submit(t, &mut d, Point::ORIGIN, 0.5);
        }
        assert!(
            matches!(d.entities().next().map(|x| &x.geometry), Some(Geometry::Wipeout { points }) if points.len() == 3)
        );
        for t in ["rec", "20,0", "30,10", "wipeout", ""] {
            e.submit(t, &mut d, Point::ORIGIN, 0.5);
        }
        let edge = Point::new(25.0, 0.0);
        e.click(edge, edge, false, &mut d, 0.5);
        e.submit("y", &mut d, Point::ORIGIN, 0.5);
        let kinds: Vec<&str> = d.entities().map(|x| x.geometry.name()).collect();
        assert_eq!(kinds, ["Wipeout", "Wipeout"], "the polyline went");
        for t in ["wipeout", "f", "off"] {
            e.submit(t, &mut d, Point::ORIGIN, 0.5);
        }
        assert_eq!(d.wipeout_frame(), 0);
    }
}

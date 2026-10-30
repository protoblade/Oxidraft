//! Annotation scaling: annotative objects (text, dimensions, leaders,
//! tolerances, hatches, blocks) keep the scales they support, as
//! drawing units per paper unit (50 for 1:50). They're drawn sized for the
//! current annotation scale (CANNOSCALE) in model space, and for each
//! layout viewport's own scale there.

use std::borrow::Cow;

use oxidraft_geom::Point;

use crate::{Document, Entity, EntityId, Geometry};

/// The scales AutoCAD lists, as (name, drawing units per paper unit).
pub const SCALES: [(&str, f64); 17] = [
    ("1:1", 1.0),
    ("1:2", 2.0),
    ("1:4", 4.0),
    ("1:5", 5.0),
    ("1:8", 8.0),
    ("1:10", 10.0),
    ("1:16", 16.0),
    ("1:20", 20.0),
    ("1:30", 30.0),
    ("1:40", 40.0),
    ("1:50", 50.0),
    ("1:100", 100.0),
    ("1:200", 200.0),
    ("1:500", 500.0),
    ("1:1000", 1000.0),
    ("2:1", 0.5),
    ("4:1", 0.25),
];

/// A scale's name: "1:50", "2:1", or the ratio for others.
pub fn scale_name(units_per_paper: f64) -> String {
    if let Some((name, _)) = SCALES.iter().find(|(_, v)| (v - units_per_paper).abs() < 1e-9) {
        return name.to_string();
    }
    if units_per_paper >= 1.0 {
        format!("1:{}", trim(units_per_paper))
    } else {
        format!("{}:1", trim(1.0 / units_per_paper))
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A scale typed: "1:50", "2:1", "1/4\"=1'-0\"" isn't, a bare factor is.
pub fn parse_scale(text: &str) -> Option<f64> {
    let t = text.trim().trim_matches('"');
    let v = match t.split_once(':') {
        Some((paper, drawing)) => drawing.trim().parse::<f64>().ok()? / paper.trim().parse::<f64>().ok()?,
        None => t.parse::<f64>().ok()?,
    };
    (v.is_finite() && v > 0.0).then_some(v)
}

/// Whether `scales` has `s`.
pub fn supports(scales: &[f64], s: f64) -> bool {
    scales.iter().any(|v| (v - s).abs() < 1e-9 * s.max(1.0))
}

impl Geometry {
    /// Whether it can be annotative.
    pub fn can_be_annotative(&self) -> bool {
        matches!(
            self,
            Geometry::Text { .. }
                | Geometry::Dimension { .. }
                | Geometry::RadialDimension { .. }
                | Geometry::AngularDimension { .. }
                | Geometry::OrdinateDimension { .. }
                | Geometry::ArcLengthDimension { .. }
                | Geometry::JoggedDimension { .. }
                | Geometry::Leader { .. }
                | Geometry::Tolerance { .. }
                | Geometry::Hatch { .. }
                | Geometry::Insert { .. }
        )
    }

    /// The same annotation sized `factor` times: text taller about its
    /// point, dimensions and leaders with a larger DIMSCALE, a hatch's
    /// pattern larger, a block larger about its insertion point; what
    /// they measure or point at stays put.
    pub fn annotation_scaled(&self, factor: f64) -> Geometry {
        let mut g = self.clone();
        match &mut g {
            Geometry::Text { position, .. } => return self.scaled(*position, factor),
            Geometry::Insert { placement, .. } => return self.scaled(placement.position, factor),
            Geometry::Dimension { scale, .. }
            | Geometry::RadialDimension { scale, .. }
            | Geometry::AngularDimension { scale, .. }
            | Geometry::OrdinateDimension { scale, .. }
            | Geometry::ArcLengthDimension { scale, .. }
            | Geometry::JoggedDimension { scale, .. }
            | Geometry::Leader { scale, .. }
            | Geometry::Hatch { scale, .. } => *scale *= factor,
            Geometry::Tolerance { height, .. } => *height *= factor,
            _ => {}
        }
        g
    }
}

/// The commands whose new text, dimensions and multileaders take an
/// annotative style's way: made annotative at the current scale.
const MAKING: [&str; 9] =
    ["TEXT", "MTEXT", "DTEXT", "MLEADER", "QLEADER", "LEADER", "QDIM", "DIMCONTINUE", "DIMBASELINE"];

impl Document {
    /// What `command` drew in an annotative style is made annotative at
    /// the current scale: text's height (and a dimension's or
    /// multileader's sizes) are on paper, so they're scaled up to it.
    pub(crate) fn annotate_new(&mut self, command: &str, added: &[EntityId]) {
        let making = MAKING.contains(&command) || command.starts_with("DIM") && !command.starts_with("DIMEDIT");
        if !making {
            return;
        }
        let scale = self.anno_scale;
        let (dims, leaders) = (self.dim_style().annotative, self.leader_style().annotative);
        for &id in added {
            let Some(e) = self.entity(id) else { continue };
            if e.annotative.is_some() {
                continue;
            }
            let annotative = match &e.geometry {
                Geometry::Text { look, .. } => self.text_style(&look.style).is_some_and(|s| s.annotative),
                Geometry::Leader { .. } => leaders,
                g => dims && g.is_dimension(),
            };
            if annotative {
                self.update(id, |e| {
                    e.geometry = e.geometry.annotation_scaled(scale);
                    e.annotative = Some(Box::new(crate::entity::Annotative::at(scale)));
                });
            }
        }
    }

    /// ANNORESET: every scale's representation of `id` back where the
    /// current one is. Whether it had any moved on their own.
    pub fn anno_reset(&mut self, id: EntityId) -> bool {
        let moved = self.entity(id).and_then(|e| e.annotative.as_ref()).is_some_and(|a| !a.offsets.is_empty());
        if moved {
            self.update(id, |e| {
                if let Some(a) = e.annotative.as_mut() {
                    a.offsets.clear();
                }
            });
        }
        moved
    }

    /// Grip editing moved annotative `id`'s representation at the current
    /// scale by `d` on its own.
    pub fn move_scale_representation(&mut self, id: EntityId, d: Point) {
        self.update(id, |e| {
            if let Some(a) = e.annotative.as_mut() {
                let s = a.shown;
                a.move_offset(s, d);
            }
        });
    }

    /// CANNOSCALE: drawing units per paper unit.
    pub fn anno_scale(&self) -> f64 {
        self.anno_scale
    }

    /// Makes `scale` current: annotative objects that support it are
    /// resized for it (with `add`, ANNOAUTOSCALE, the others take it on
    /// too). How many changed.
    pub fn set_anno_scale(&mut self, scale: f64, add: bool) -> usize {
        if !(scale.is_finite() && scale > 0.0) {
            return 0;
        }
        let old = self.anno_scale;
        self.anno_scale = scale;
        self.revision += 1;
        let ids: Vec<EntityId> = self.all_entities().filter(|e| e.annotative.is_some()).map(|e| e.id).collect();
        let mut changed = 0;
        for id in ids {
            self.update(id, |e| {
                let Some(a) = e.annotative.as_mut() else { return };
                if add && !supports(&a.scales, scale) {
                    a.scales.push(scale);
                }
                // Sized for the scale it was shown at, now for this one.
                if supports(&a.scales, scale) && (a.shown - scale).abs() > 1e-12 {
                    e.geometry = a.for_scale(&e.geometry, scale);
                    a.shown = scale;
                    changed += 1;
                }
            });
        }
        let _ = old;
        changed
    }

    /// The current scale as read from a file, its objects sized for it
    /// already.
    pub fn load_anno_scale(&mut self, scale: f64) {
        self.anno_scale = scale;
        self.revision += 1;
    }

    /// ANNOALLVISIBLE: annotative objects show at scales they don't
    /// support (as they are).
    pub fn anno_all_visible(&self) -> bool {
        self.anno_all_visible
    }

    pub fn set_anno_all_visible(&mut self, on: bool) {
        if on != self.anno_all_visible {
            self.anno_all_visible = on;
            self.revision += 1;
        }
    }

    /// `e` as drawn at annotation scale `scale` (a viewport's), or at the
    /// current one (`None`): sized for it when it's annotative and
    /// supports it; `None` when it doesn't and ANNOALLVISIBLE is off.
    pub fn shown_at<'a>(&self, e: &'a Entity, scale: Option<f64>) -> Option<Cow<'a, Entity>> {
        let Some(a) = &e.annotative else { return Some(Cow::Borrowed(e)) };
        let s = scale.unwrap_or(self.anno_scale);
        if !supports(&a.scales, s) {
            return self.anno_all_visible.then_some(Cow::Borrowed(e));
        }
        if (s - a.shown).abs() < 1e-12 {
            return Some(Cow::Borrowed(e));
        }
        Some(Cow::Owned(Entity { geometry: a.for_scale(&e.geometry, s), ..e.clone() }))
    }

    /// Makes an object annotative at the current scale (its present size
    /// being that scale's), or not.
    pub fn set_annotative(&mut self, id: EntityId, on: bool) -> bool {
        let scale = self.anno_scale;
        let mut done = false;
        self.update(id, |e| {
            if !e.geometry.can_be_annotative() {
                return;
            }
            e.annotative = on.then(|| Box::new(crate::entity::Annotative::at(scale)));
            done = true;
        });
        done
    }
}

#[cfg(test)]
mod tests {
    use oxidraft_geom::Point;

    use super::*;
    use crate::Attach;

    #[test]
    fn scales_by_name() {
        assert_eq!(parse_scale("1:50"), Some(50.0));
        assert_eq!(parse_scale("2:1"), Some(0.5));
        assert_eq!(scale_name(50.0), "1:50");
        assert_eq!(scale_name(0.5), "2:1");
        assert_eq!(scale_name(75.0), "1:75");
        assert_eq!(parse_scale("x"), None);
    }

    #[test]
    fn annotative_text_follows_the_scale() {
        let mut doc = Document::new();
        let text = doc.add(Geometry::Text {
            look: Default::default(),
            position: Point::new(10.0, 10.0),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "ROOM".into(),
        });
        let plain = doc.add(Geometry::Text {
            look: Default::default(),
            position: Point::ORIGIN,
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "NOTE".into(),
        });
        assert!(doc.set_annotative(text, true));
        let height = |d: &Document, id| match d.entity(id).unwrap().geometry {
            Geometry::Text { height, .. } => height,
            _ => 0.0,
        };
        // Not supported at 1:50: it stays as it is.
        doc.set_anno_scale(50.0, false);
        assert_eq!(height(&doc, text), 2.5);
        // ANNOAUTOSCALE: it takes 1:50 on, and is sized for it.
        doc.set_anno_scale(1.0, false);
        doc.set_anno_scale(50.0, true);
        assert!((height(&doc, text) - 125.0).abs() < 1e-9);
        assert_eq!(height(&doc, plain), 2.5, "not annotative");
        assert!(
            matches!(doc.entity(text).unwrap().geometry, Geometry::Text { position, .. } if position == Point::new(10.0, 10.0))
        );
        doc.set_anno_scale(1.0, false);
        assert!((height(&doc, text) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn annotative_styles_make_annotative_objects() {
        let mut doc = Document::new();
        let mut style = crate::TextStyle::new("Notes");
        style.annotative = true;
        doc.set_text_style(style).unwrap();
        doc.set_dim_style(crate::annotation::DimStyle { annotative: true, ..Default::default() });
        doc.set_anno_scale(50.0, false);
        let text = |style: &str| Geometry::Text {
            look: crate::annotation::TextLook { style: style.into(), ..Default::default() },
            position: Point::new(10.0, 10.0),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "ROOM".into(),
        };
        doc.begin("TEXT");
        let note = doc.add(text("Notes"));
        let plain = doc.add(text("Standard"));
        doc.commit();
        let e = doc.entity(note).unwrap();
        assert!(
            matches!(e.geometry, Geometry::Text { height, .. } if (height - 125.0).abs() < 1e-9),
            "paper height at 1:50"
        );
        assert_eq!(e.annotative.as_ref().map(|a| a.scales.clone()), Some(vec![50.0]));
        assert!(doc.entity(plain).unwrap().annotative.is_none());
        doc.begin("DIMLINEAR");
        let dim = doc.add(Geometry::Dimension {
            p1: Point::ORIGIN,
            p2: Point::new(100.0, 0.0),
            line: Point::new(0.0, 20.0),
            rotation: 0.0,
            aligned: false,
            text: None,
            scale: 1.0,
            extra: Default::default(),
        });
        doc.commit();
        assert!(matches!(doc.entity(dim).unwrap().geometry, Geometry::Dimension { scale, .. } if scale == 50.0));
        // Copied, it stays as it is.
        doc.begin("COPY");
        let copy = doc.add(text("Notes"));
        doc.commit();
        assert!(doc.entity(copy).unwrap().annotative.is_none());
    }

    #[test]
    fn a_scales_representation_moves_on_its_own() {
        let mut doc = Document::new();
        let id = doc.add(Geometry::Text {
            look: Default::default(),
            position: Point::new(10.0, 10.0),
            height: 2.5,
            rotation: 0.0,
            width: 0.0,
            attach: Attach::BottomLeft,
            content: "ROOM".into(),
        });
        doc.set_annotative(id, true);
        doc.set_anno_scale(50.0, true);
        let at = |d: &Document| match d.entity(id).unwrap().geometry {
            Geometry::Text { position, .. } => position,
            _ => Point::ORIGIN,
        };
        // Moved by its grip at 1:50: there only.
        let d = Point::new(5.0, 0.0);
        doc.update(id, |e| e.geometry = e.geometry.translated(d));
        doc.move_scale_representation(id, d);
        assert_eq!(at(&doc), Point::new(15.0, 10.0));
        doc.set_anno_scale(1.0, false);
        assert_eq!(at(&doc), Point::new(10.0, 10.0), "1:1's didn't move");
        doc.set_anno_scale(50.0, false);
        assert_eq!(at(&doc), Point::new(15.0, 10.0));
        // ANNORESET: 1:1's comes here too.
        assert!(doc.anno_reset(id));
        doc.set_anno_scale(1.0, false);
        assert_eq!(at(&doc), Point::new(15.0, 10.0));
        assert!(!doc.anno_reset(id), "nothing moved on its own now");
    }
}

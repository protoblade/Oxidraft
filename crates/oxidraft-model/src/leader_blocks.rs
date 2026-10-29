//! Multileaders with block content: the ready-made tag blocks, and the
//! block references a multileader's blocks draw as.

use oxidraft_geom::Point;

use crate::annotation::{LeaderBlocks, TagShape, leader_block_places};
use crate::{Attach, Block, BlockEntity, Color, Document, Geometry};

impl Document {
    /// The tag block for `shape`, defined in the drawing if it isn't yet;
    /// its name.
    pub fn tag_block(&mut self, shape: TagShape) -> String {
        let name = shape.block_name();
        if self.block(name).is_none() {
            let tag = Geometry::AttributeDefinition {
                tag: "TAGNUMBER".into(),
                prompt: "Enter tag number".into(),
                default: "TAGNUMBER".into(),
                position: Point::ORIGIN,
                height: 2.5,
                rotation: 0.0,
                attach: Attach::MiddleCenter,
                invisible: false,
                constant: false,
            };
            let entities = vec![
                BlockEntity::new("0", Color::ByBlock, shape.outline()),
                BlockEntity::new("0", Color::ByBlock, tag),
            ];
            self.define_block(Block { name: name.into(), base: Point::ORIGIN, entities });
        }
        name.into()
    }

    /// The block references a multileader's blocks draw as, each with its
    /// value in its attributes.
    pub fn leader_inserts(&self, g: &Geometry) -> Vec<Geometry> {
        let Geometry::Leader { points, scale, blocks: Some(blocks), .. } = g else { return Vec::new() };
        self.place_leader_blocks(points, blocks, *scale)
    }

    pub(crate) fn place_leader_blocks(&self, points: &[Point], blocks: &LeaderBlocks, scale: f64) -> Vec<Geometry> {
        let places = crate::annotation::with_leader_style(self.leader_style(), || {
            leader_block_places(points, blocks, scale, |name| self.block_extents(name))
        });
        places
            .into_iter()
            .filter_map(|(i, placement)| {
                let item = &blocks.items[i];
                let def = self.block(&item.name)?;
                let attributes = def.attributes_for(&placement, |_| Some(item.value.clone()));
                Some(Geometry::Insert { block: def.name.clone(), placement, attributes, clip: None })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::LeaderBlock;

    #[test]
    fn blocks_sit_at_the_landing_in_rows() {
        let mut d = Document::new();
        let name = d.tag_block(TagShape::Circle);
        let mut blocks = LeaderBlocks::one(&name, "1");
        let leader = |blocks: &LeaderBlocks| Geometry::Leader {
            points: vec![Point::ORIGIN, Point::new(10.0, 10.0)],
            content: String::new(),
            scale: 1.0,
            more: Vec::new(),
            blocks: Some(Box::new(blocks.clone())),
        };
        let inserts = d.leader_inserts(&leader(&blocks));
        let Geometry::Insert { placement, attributes, .. } = &inserts[0] else { panic!() };
        // Landing 2.5 long, then the circle (radius 3) beside it.
        assert!(placement.position.approx_eq(Point::new(15.5, 10.0)), "{placement:?}");
        assert_eq!(attributes[0].value, "1");
        blocks.items.extend(["2", "3"].map(|v| LeaderBlock { name: name.clone(), value: v.into() }));
        blocks.per_row = 2;
        let at: Vec<Point> = d
            .leader_inserts(&leader(&blocks))
            .iter()
            .map(|g| match g {
                Geometry::Insert { placement, .. } => placement.position,
                _ => unreachable!(),
            })
            .collect();
        assert!(at[1].approx_eq(Point::new(21.5, 10.0)) && at[2].approx_eq(Point::new(15.5, 4.0)), "{at:?}");
        // Drawn: the leader's lines and arrow, the circles and their numbers.
        let e = d.add(leader(&blocks));
        let drawn = d.expanded(d.entity(e).unwrap());
        assert_eq!(drawn.iter().filter(|g| matches!(g, Geometry::Circle { .. })).count(), 3);
        assert!(drawn.iter().any(|g| matches!(g, Geometry::Text { content, .. } if content == "3")));
    }
}

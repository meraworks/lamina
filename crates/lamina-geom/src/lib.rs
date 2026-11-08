//! Copper geometry for Lamina: primitives, exact distances, extraction of
//! board items, and a spatial index.

pub mod board;
pub mod fill;
pub mod font;
pub mod prim;

pub use board::{
    BoardGeometry, CopperItem, DrillItem, Owner, build, courtyard, pad_prim, part_keepouts,
    placed_pads,
};
pub use prim::Prim;

use lamina_model::Rect;
use rstar::{AABB, RTree, RTreeObject};

struct Entry {
    idx: usize,
    env: AABB<[f64; 2]>,
}

impl RTreeObject for Entry {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

/// A bounding-box index for a list of rectangles.
pub struct Index {
    tree: RTree<Entry>,
}

impl Index {
    pub fn new(boxes: impl IntoIterator<Item = Rect>) -> Index {
        let entries = boxes
            .into_iter()
            .enumerate()
            .map(|(idx, r)| Entry {
                idx,
                env: AABB::from_corners([r.min.x, r.min.y], [r.max.x, r.max.y]),
            })
            .collect();
        Index {
            tree: RTree::bulk_load(entries),
        }
    }

    /// The indices of the boxes that come within `margin` of `r`.
    pub fn near(&self, r: &Rect, margin: f64) -> impl Iterator<Item = usize> + '_ {
        let env = AABB::from_corners(
            [r.min.x - margin, r.min.y - margin],
            [r.max.x + margin, r.max.y + margin],
        );
        self.tree
            .locate_in_envelope_intersecting(env)
            .map(|e| e.idx)
    }
}

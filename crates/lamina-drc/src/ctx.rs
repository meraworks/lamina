//! The data that all check families use: the board geometry with the pour
//! pieces, the connections between the copper items, and the pairs of items
//! of different nets that are near to each other. `Ctx::new` runs no check.

use std::collections::HashMap;

use lamina_geom::fill::ZoneFill;
use lamina_geom::prim::{IndexedPoly, Prim};
use lamina_geom::{BoardGeometry, CopperItem, Index, Owner};
use lamina_model::*;

/// Sets of connected items (union-find).
#[derive(Debug, Clone)]
pub(crate) struct Dsu(Vec<usize>);

impl Dsu {
    pub(crate) fn new(n: usize) -> Dsu {
        Dsu((0..n).collect())
    }
    pub(crate) fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut c = x;
        while self.0[c] != r {
            let n = self.0[c];
            self.0[c] = r;
            c = n;
        }
        r
    }
    pub(crate) fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

pub(crate) struct Ctx<'a> {
    pub d: &'a ProjectData,
    /// The geometry. `g.copper` has the pour pieces after the first `n_items` items.
    pub g: BoardGeometry,
    pub fills: Vec<ZoneFill>,
    /// The number of copper items that are not pour pieces.
    pub n_items: usize,
    pub layers: u8,
    pub index: Index,
    pub fill_index: Vec<IndexedPoly>,
    /// The island of each item of `g.copper`.
    pub roots: Vec<usize>,
    /// The connections without the joins that only touch.
    pub strong: Dsu,
    /// The joins that only touch.
    pub weak: Vec<(usize, usize)>,
    /// Pairs of items of different nets (no pour pieces) that are within the
    /// largest clearance of the board, with their distance.
    pub near: Vec<(usize, usize, f64)>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(d: &'a ProjectData) -> Ctx<'a> {
        let mut g = lamina_geom::build(d);
        let fills = crate::fills(d, &g);
        let layers = g.layers;
        let rules = &d.board.rules;
        // Pour fills are part of the connectivity check. They are not part of the
        // clearance check, because each fill already keeps the clearance.
        let n_items = g.copper.len();
        for f in &fills {
            let Some(c) = f.layer.copper() else { continue };
            for (k, p) in f.polygons.iter().enumerate() {
                g.copper.push(CopperItem {
                    layer: c.index(layers),
                    net: f.net.clone(),
                    prim: p.prim(),
                    owner: Owner::Zone {
                        id: f.zone.clone(),
                        piece: k,
                    },
                });
            }
        }
        let items = &g.copper;
        let fill_index: Vec<IndexedPoly> = items[n_items..]
            .iter()
            .map(|it| match &it.prim {
                Prim::Poly { outer, holes } => IndexedPoly::new(outer, holes),
                other => {
                    let b = other.bbox();
                    let r = [
                        b.min,
                        Point::new(b.max.x, b.min.y),
                        b.max,
                        Point::new(b.min.x, b.max.y),
                    ];
                    IndexedPoly::new(&r, &[])
                }
            })
            .collect();
        let boxes: Vec<Rect> = items.iter().map(|i| i.prim.bbox()).collect();
        let index = Index::new(boxes.iter().copied());
        let max_cl = rules
            .net_classes
            .values()
            .filter_map(|c| c.clearance)
            .chain(rules.custom.iter().filter_map(|r| r.clearance))
            .fold(rules.clearance, f64::max);

        let mut dsu = Dsu::new(items.len());
        let mut strong = Dsu::new(items.len());
        let mut weak: Vec<(usize, usize)> = Vec::new();
        let mut near: Vec<(usize, usize, f64)> = Vec::new();
        let mut by_body: HashMap<String, usize> = HashMap::new();
        for (i, it) in items.iter().enumerate() {
            if let Some(&j) = by_body.get(&it.owner.body()) {
                dsu.union(i, j);
                strong.union(i, j);
            } else {
                by_body.insert(it.owner.body(), i);
            }
        }
        // Pads of one part with the same number are one terminal. The part connects
        // them internally (for example, the paired legs of a switch or the shield
        // tabs of a connector).
        let mut by_pad: HashMap<(&str, &str), usize> = HashMap::new();
        for (i, it) in items.iter().enumerate() {
            if let Owner::Pad { part, pad, .. } = &it.owner {
                if pad.is_empty() {
                    continue;
                }
                match by_pad.get(&(part.as_str(), pad.as_str())) {
                    Some(&j) => {
                        dsu.union(i, j);
                        strong.union(i, j);
                    }
                    None => {
                        by_pad.insert((part.as_str(), pad.as_str()), i);
                    }
                }
            }
        }
        for (i, a) in items.iter().enumerate() {
            for j in index.near(&boxes[i], max_cl) {
                if j <= i {
                    continue;
                }
                let b = &items[j];
                if a.layer != b.layer || a.owner.body() == b.owner.body() {
                    continue;
                }
                // Two drawings with no net can touch: a logo, or the copper of a
                // board that came from Gerber files.
                if a.owner.is_art() && b.owner.is_art() {
                    continue;
                }
                let both_fill = i >= n_items && j >= n_items;
                let same_net = a.net.is_some() && a.net == b.net;
                // A pour fill keeps clear of other nets, so a pour piece is important
                // only for the connectivity of its own net. Skip the other pairs. This
                // prevents an exact distance to a polygon of thousands of edges for each pair.
                if (i >= n_items || j >= n_items) && !same_net {
                    continue;
                }
                // For a pour piece and a pad or track of its net, use an indexed touch
                // test. Do not calculate the exact distance to the full polygon.
                let dist = match (i >= n_items, j >= n_items) {
                    (true, false) | (false, true) if same_net => {
                        let (f, other) = if i >= n_items { (i, b) } else { (j, a) };
                        if fill_index[f - n_items].touches(&other.prim, 1e-6) {
                            0.0
                        } else {
                            1.0
                        }
                    }
                    (true, true) => {
                        // Pieces of one pour are always separate. Pieces of two pours
                        // of the same net can overlap.
                        let same_zone = matches!(
                            (&a.owner, &b.owner),
                            (Owner::Zone { id: x, .. }, Owner::Zone { id: y, .. }) if x == y
                        );
                        if !same_zone
                            && fill_index[i - n_items].touches_poly(&fill_index[j - n_items], 1e-6)
                        {
                            0.0
                        } else {
                            1.0
                        }
                    }
                    _ => a.prim.distance(&b.prim),
                };
                if dist <= 1e-6 {
                    dsu.union(i, j);
                    let plain = i < n_items && j < n_items;
                    if plain
                        && same_net
                        && crate::parity::is_weak(&a.prim, &b.prim, rules.min_connection)
                    {
                        weak.push((i, j));
                    } else {
                        strong.union(i, j);
                    }
                }
                if same_net || both_fill || i >= n_items || j >= n_items {
                    continue;
                }
                near.push((i, j, dist));
            }
        }
        let roots: Vec<usize> = (0..items.len()).map(|i| dsu.find(i)).collect();
        Ctx {
            d,
            g,
            fills,
            n_items,
            layers,
            index,
            fill_index,
            roots,
            strong,
            weak,
            near,
        }
    }

    /// The island of each fill polygon, for each fill.
    pub(crate) fn fill_islands(&self) -> Vec<Vec<usize>> {
        let mut k = self.n_items;
        self.fills
            .iter()
            .map(|f| {
                if f.layer.copper().is_none() {
                    return Vec::new();
                }
                let v: Vec<usize> = (k..k + f.polygons.len()).map(|i| self.roots[i]).collect();
                k += f.polygons.len();
                v
            })
            .collect()
    }
}

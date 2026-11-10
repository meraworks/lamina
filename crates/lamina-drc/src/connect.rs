//! Connections: the ratsnest, open connections, joins that only touch,
//! and parts with no placement.

use std::collections::BTreeMap;

use lamina_geom::Owner;
use lamina_model::*;

use crate::ctx::Ctx;
use crate::{Ratsnest, mid, parity};

/// The missing connections of each net. It adds one `unrouted` violation
/// for each net that has one.
pub(crate) fn ratsnest(ctx: &Ctx, v: &mut Vec<Violation>) -> Vec<Ratsnest> {
    let d = ctx.d;
    let items = &ctx.g.copper;
    // Unrouted connections and ratsnest.
    let mut ratsnest = Vec::new();
    for (name, net) in &d.nets.nets {
        // Islands of this net that contain at least one pad.
        let mut islands: BTreeMap<usize, Vec<(String, Point)>> = BTreeMap::new();
        for pr in &net.pads {
            let found = items.iter().enumerate().find(|(_, it)| {
                matches!(&it.owner, Owner::Pad { part, pad, .. } if *part == pr.part && *pad == pr.pad)
            });
            if let Some((i, it)) = found {
                islands
                    .entry(ctx.roots[i])
                    .or_default()
                    .push((pr.to_string(), it.prim.anchor()));
            }
        }
        if islands.len() < 2 {
            continue;
        }
        // Prim's MST of the islands. Each edge is the nearest pad pair between two islands.
        let groups: Vec<Vec<(String, Point)>> = islands.into_values().collect();
        let k = groups.len();
        let mut in_tree = vec![false; k];
        in_tree[0] = true;
        for _ in 1..k {
            let mut best: Option<(f64, usize, usize, usize, usize)> = None;
            for a in (0..k).filter(|&a| in_tree[a]) {
                for b in (0..k).filter(|&b| !in_tree[b]) {
                    for (ia, pa) in groups[a].iter().enumerate() {
                        for (ib, pb) in groups[b].iter().enumerate() {
                            let dd = pa.1.dist(pb.1);
                            if best.is_none_or(|x| dd < x.0) {
                                best = Some((dd, a, ia, b, ib));
                            }
                        }
                    }
                }
            }
            let (_, a, ia, b, ib) = best.unwrap();
            in_tree[b] = true;
            ratsnest.push(Ratsnest {
                net: name.clone(),
                a: groups[a][ia].1,
                b: groups[b][ib].1,
                from: groups[a][ia].0.clone(),
                to: groups[b][ib].0.clone(),
            });
        }
        v.push(
            Violation::warning(
                "unrouted",
                format!("The net {name} has {} unrouted connection(s)", k - 1),
            )
            .items([name.clone()]),
        );
    }

    ratsnest
}

pub(crate) fn weak(ctx: &Ctx, v: &mut Vec<Violation>) {
    let (rules, layers) = (&ctx.d.board.rules, ctx.layers);
    let items = &ctx.g.copper;
    // A join that only touches is a violation if nothing else connects the two sides.
    let mut strong = ctx.strong.clone();
    for &(i, j) in &ctx.weak {
        if strong.find(i) != strong.find(j) {
            let (a, b) = (&items[i], &items[j]);
            v.push(parity::weak_violation(
                &a.owner,
                &b.owner,
                mid(&a.prim, &b.prim),
                Layer::Copper(Copper::from_index(a.layer, layers)),
                rules.min_connection,
            ));
            strong.union(i, j);
        }
    }
}

pub(crate) fn unplaced(d: &ProjectData, v: &mut Vec<Violation>) {
    for r in d.parts.parts.keys() {
        if !d.placement.placements.contains_key(r) {
            v.push(Violation::warning("unplaced", format!("{r} is not placed")).items([r.clone()]));
        }
    }
}

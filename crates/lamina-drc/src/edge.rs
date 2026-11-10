//! Copper and holes against the board edge.

use std::collections::BTreeSet;

use lamina_geom::Index;
use lamina_geom::prim::{Prim, ring_contains};
use lamina_model::*;

use crate::ctx::Ctx;
use crate::r3;

pub(crate) fn run(ctx: &Ctx, v: &mut Vec<Violation>) {
    let (g, rules) = (&ctx.g, &ctx.d.board.rules);
    let items = &g.copper;
    let n_items = ctx.n_items;
    // Board edge.
    if let Some(outline) = &g.outline {
        let mut edges: Vec<Prim> = Vec::new();
        let mut push_ring = |r: &Vec<Point>| {
            for k in 0..r.len() {
                edges.push(Prim::Capsule {
                    a: r[k],
                    b: r[(k + 1) % r.len()],
                    r: 0.0,
                });
            }
        };
        push_ring(outline);
        for c in &g.cutouts {
            push_ring(c);
        }
        let eboxes: Vec<Rect> = edges.iter().map(|e| e.bbox()).collect();
        let eidx = Index::new(eboxes.iter().copied());
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for it in items.iter().take(n_items) {
            let label = it.owner.label();
            if reported.contains(&label) {
                continue;
            }
            let p = it.prim.anchor();
            let inside =
                ring_contains(outline, p) && !g.cutouts.iter().any(|c| ring_contains(c, p));
            if !inside {
                reported.insert(label.clone());
                v.push(
                    Violation::error(
                        "copper_outside_board",
                        format!("{label} is outside the board"),
                    )
                    .at(p)
                    .items([label]),
                );
                continue;
            }
            let mut near = f64::INFINITY;
            for k in eidx.near(&it.prim.bbox(), rules.copper_to_edge) {
                near = near.min(it.prim.distance(&edges[k]));
            }
            if near + 1e-6 < rules.copper_to_edge {
                reported.insert(label.clone());
                v.push(
                    Violation::error(
                        "copper_to_edge",
                        format!(
                            "{label} is {} mm from the board edge, but the minimum is {} mm",
                            r3(near),
                            r3(rules.copper_to_edge)
                        ),
                    )
                    .at(p)
                    .items([label]),
                );
            }
        }
    }
}

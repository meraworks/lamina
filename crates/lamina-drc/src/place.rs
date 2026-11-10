//! Parts: courtyards and keep-outs.

use std::collections::BTreeSet;

use lamina_geom::Owner;
use lamina_geom::prim::{Prim, ring_contains};
use lamina_model::*;

use crate::ctx::Ctx;
use crate::{centroid, polys_overlap, shrink};

pub(crate) fn run(ctx: &Ctx, v: &mut Vec<Violation>) {
    let (d, g, layers) = (ctx.d, &ctx.g, ctx.layers);
    let items = &g.copper;
    let index = &ctx.index;
    // Courtyards and keep-outs.
    let mut cys: Vec<(String, Side, Vec<Point>)> = Vec::new();
    let mut kos: Vec<(String, Vec<u8>, Vec<Point>, Forbid)> = Vec::new();
    for (refdes, pl) in &d.placement.placements {
        let Some(fp) = d
            .parts
            .parts
            .get(refdes)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            continue;
        };
        if let Some(c) = lamina_geom::courtyard(fp, pl) {
            cys.push((refdes.clone(), pl.side, c));
        }
        for (ls, pts, f) in lamina_geom::part_keepouts(fp, pl, layers) {
            kos.push((refdes.clone(), ls, pts, f));
        }
    }
    for k in &d.routing.keepouts {
        let ls: Vec<u8> = k
            .layers
            .iter()
            .filter_map(|l| l.copper().map(|c| c.index(layers)))
            .collect();
        kos.push((k.id.clone(), ls, k.outline.flatten(0.02), k.forbid));
    }
    for hole in g.drills.iter().filter(|hole| !hole.plated) {
        for (part, side, courtyard) in &cys {
            if matches!(&hole.owner, Owner::Pad { part: own, .. } if own == part) {
                continue;
            }
            let polygon = Prim::Poly {
                outer: courtyard.clone(),
                holes: vec![],
            };
            if polygon.distance(&hole.prim) <= 1e-6 {
                v.push(
                    Violation::warning(
                        "npth_inside_courtyard",
                        format!(
                            "The non-plated hole {} is inside the courtyard of {part}",
                            hole.owner.label()
                        ),
                    )
                    .at(hole.prim.anchor())
                    .on(Layer::Courtyard(*side))
                    .items([hole.owner.label(), part.clone()]),
                );
            }
        }
    }
    for i in 0..cys.len() {
        for j in (i + 1)..cys.len() {
            if cys[i].1 != cys[j].1 {
                continue;
            }
            if polys_overlap(&cys[i].2, &cys[j].2) {
                let pair = [cys[i].0.clone(), cys[j].0.clone()];
                v.push(
                    Violation::error(
                        "courtyard_overlap",
                        format!("The courtyards of {} and {} overlap", pair[0], pair[1]),
                    )
                    .at(centroid(&cys[i].2))
                    .items(pair),
                );
            }
        }
    }
    if let Some(outline) = &g.outline {
        for (r, _, c) in &cys {
            if c.iter().any(|p| !ring_contains(outline, *p)) {
                v.push(
                    Violation::warning(
                        "courtyard_outside_board",
                        format!("The courtyard of {r} is partly outside the board"),
                    )
                    .at(centroid(c))
                    .items([r.clone()]),
                );
            }
        }
    }
    for (kid, ls, pts, forbid) in &kos {
        let kp = Prim::Poly {
            outer: pts.clone(),
            holes: vec![],
        };
        let kb = kp.bbox();
        let mut hits: BTreeSet<String> = BTreeSet::new();
        for j in index.near(&kb, 0.0) {
            let it = &items[j];
            if !ls.contains(&it.layer) {
                continue;
            }
            let forbidden = match &it.owner {
                Owner::Track { .. } | Owner::Text { .. } | Owner::Graphic { .. } => forbid.tracks,
                Owner::Via { .. } => forbid.vias,
                Owner::Zone { .. } => false,
                Owner::Pad { part, .. } => forbid.pads && part != kid,
                Owner::Hole { .. } => forbid.pads,
            };
            if forbidden && it.prim.distance(&kp) <= 1e-6 {
                // An item can touch the edge, but it must not overlap it.
                let shrunk = shrink(&kp, 1e-3);
                if it.prim.distance(&shrunk) <= 1e-6 {
                    hits.insert(it.owner.label());
                }
            }
        }
        if forbid.parts {
            for (r, _, c) in &cys {
                if r != kid && polys_overlap(c, pts) {
                    hits.insert(r.clone());
                }
            }
        }
        for h in hits {
            v.push(
                Violation::error("keepout", format!("{h} is inside the keep-out {kid}"))
                    .at(centroid(pts))
                    .items([h, kid.clone()]),
            );
        }
    }
}

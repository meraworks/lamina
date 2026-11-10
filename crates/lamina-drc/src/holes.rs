//! Checks of drilled holes against copper.

use lamina_geom::{BoardGeometry, Index, Owner};
use lamina_model::*;

use crate::{mid, r3};

fn part_of(o: &Owner) -> Option<&str> {
    match o {
        Owner::Pad { part, .. } => Some(part),
        _ => None,
    }
}

/// A hole wall that is too near to copper. Copper of the net of a plated
/// hole can touch it. All copper must keep away from a non-plated hole.
/// The copper of the part that has the hole does not count: the footprint
/// puts it there.
pub(crate) fn hole_to_copper(d: &ProjectData, g: &BoardGeometry) -> Vec<Violation> {
    let mut out = Vec::new();
    if g.drills.is_empty() {
        return out;
    }
    let rules = &d.board.rules;
    // The largest limit that a rule can give, for the search.
    let reach = rules
        .custom
        .iter()
        .filter_map(|c| c.hole_to_copper)
        .fold(rules.hole_to_copper, f64::max);
    if reach <= 0.0 {
        return out;
    }
    let boxes: Vec<Rect> = g.copper.iter().map(|c| c.prim.bbox()).collect();
    let index = Index::new(boxes.iter().copied());
    let mut seen = std::collections::BTreeSet::new();
    for hole in &g.drills {
        let areas = if d.routing.areas.is_empty() {
            Vec::new()
        } else {
            d.routing.areas_at(hole.prim.anchor(), None)
        };
        let need = rules
            .resolve_for(&Subject {
                net: hole.net.as_deref(),
                class: hole
                    .net
                    .as_ref()
                    .and_then(|n| d.nets.nets.get(n))
                    .and_then(|n| n.class.as_deref()),
                layer: None,
                kind: Some(RuleKind::Hole),
                part: part_of(&hole.owner),
                areas: &areas,
            })
            .hole_to_copper;
        if need <= 0.0 {
            continue;
        }
        for j in index.near(&hole.prim.bbox(), need) {
            let c = &g.copper[j];
            if c.layer < hole.layers.0 || c.layer > hole.layers.1 {
                continue;
            }
            // The pour keeps this distance when it fills.
            if matches!(c.owner, Owner::Zone { .. }) || c.owner.body() == hole.owner.body() {
                continue;
            }
            if part_of(&c.owner).is_some() && part_of(&c.owner) == part_of(&hole.owner) {
                continue;
            }
            if hole.plated && hole.net.is_some() && c.net == hole.net {
                continue;
            }
            let dist = hole.prim.distance(&c.prim);
            if dist + 1e-6 >= need {
                continue;
            }
            let pair = [hole.owner.label(), c.owner.label()];
            if !seen.insert(pair.clone()) {
                continue;
            }
            let layer = Layer::Copper(Copper::from_index(c.layer, g.layers));
            out.push(
                Violation::error(
                    "hole_to_copper",
                    format!(
                        "The hole of {} is {} mm from {} on {layer}, but the minimum is {} mm",
                        pair[0],
                        r3(dist),
                        pair[1],
                        r3(need)
                    ),
                )
                .at(hole.prim.anchor())
                .on(layer)
                .items(pair)
                .measured(dist, need),
            );
        }
    }
    out
}

/// Two holes that are too near to each other.
pub(crate) fn hole_to_hole(ctx: &crate::ctx::Ctx, v: &mut Vec<Violation>) {
    let (g, rules) = (&ctx.g, &ctx.d.board.rules);
    // Hole to hole.
    let dboxes: Vec<Rect> = g.drills.iter().map(|x| x.prim.bbox()).collect();
    let didx = Index::new(dboxes.iter().copied());
    for (i, a) in g.drills.iter().enumerate() {
        for j in didx.near(&dboxes[i], rules.hole_to_hole) {
            if j <= i {
                continue;
            }
            let b = &g.drills[j];
            if a.owner.body() == b.owner.body() {
                continue;
            }
            let dist = a.prim.distance(&b.prim);
            if dist + 1e-6 < rules.hole_to_hole {
                let mut pair = [a.owner.label(), b.owner.label()];
                pair.sort();
                v.push(
                    Violation::error(
                        "hole_to_hole",
                        format!(
                            "The holes {} and {} are {} mm apart, but the minimum is {} mm",
                            pair[0],
                            pair[1],
                            r3(dist),
                            r3(rules.hole_to_hole)
                        ),
                    )
                    .at(mid(&a.prim, &b.prim))
                    .items(pair),
                );
            }
        }
    }
}

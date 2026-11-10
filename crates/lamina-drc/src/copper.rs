//! Copper of different nets: clearance and shorts.

use std::collections::{BTreeMap, BTreeSet};

use lamina_geom::Owner;
use lamina_model::*;

use crate::ctx::Ctx;
use crate::{item_limits, mid, r3};

pub(crate) fn run(ctx: &Ctx, v: &mut Vec<Violation>) {
    let (d, layers) = (ctx.d, ctx.layers);
    let items = &ctx.g.copper;
    let class_of = |net: &Option<String>| -> Option<String> {
        net.as_ref()
            .and_then(|n| d.nets.nets.get(n))
            .and_then(|n| n.class.clone())
    };
    // Clearance and shorts between copper of different nets.
    let mut seen_pairs: BTreeSet<(String, String)> = BTreeSet::new();
    for &(i, j, dist) in &ctx.near {
        let (a, b) = (&items[i], &items[j]);
        // If two pieces of one part have no net (for example, two pads that
        // touch in the footprint drawing), it is a footprint problem, not a short.
        let same_part = matches!((&a.owner, &b.owner), (Owner::Pad { part: p, .. }, Owner::Pad { part: q, .. }) if p == q);
        let layer = Layer::Copper(Copper::from_index(a.layer, layers));
        let need = item_limits(d, a, class_of(&a.net).as_deref(), layer)
            .clearance
            .max(item_limits(d, b, class_of(&b.net).as_deref(), layer).clearance);
        if dist + 1e-6 < need {
            let mut pair = [a.owner.label(), b.owner.label()];
            pair.sort();
            if !seen_pairs.insert((pair[0].clone(), pair[1].clone())) {
                continue;
            }
            if same_part && a.net.is_none() && b.net.is_none() {
                continue;
            }
            let layer = Layer::Copper(Copper::from_index(a.layer, layers));
            let nets = format!(
                "{} / {}",
                a.net.as_deref().unwrap_or("no net"),
                b.net.as_deref().unwrap_or("no net")
            );
            let viol = if dist <= 1e-6 {
                Violation::error(
                    "short",
                    format!("{} touches {} ({nets}) on {layer}", pair[0], pair[1]),
                )
            } else {
                Violation::error(
                    "clearance",
                    format!(
                        "{} and {} are {} mm apart on {layer} ({nets}), but the minimum clearance is {} mm",
                        pair[0],
                        pair[1],
                        r3(dist),
                        r3(need)
                    ),
                )
            };
            v.push(viol.at(mid(&a.prim, &b.prim)).on(layer).items(pair));
        }
    }

    // Shorts through connected copper (for example, a track that connects two nets).
    let mut comp_nets: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Some(n) = &it.net {
            comp_nets.entry(ctx.roots[i]).or_default().insert(n.clone());
        }
    }
    for (root, nets) in &comp_nets {
        if nets.len() > 1 {
            let at = items
                .iter()
                .enumerate()
                .find(|(i, _)| ctx.roots[*i] == *root)
                .map(|(_, it)| it.prim.anchor());
            let list: Vec<String> = nets.iter().cloned().collect();
            let mut viol = Violation::error(
                "short_nets",
                format!("The nets {} are connected", list.join(", ")),
            )
            .items(list.clone());
            if let Some(p) = at {
                viol = viol.at(p);
            }
            v.push(viol);
        }
    }
}

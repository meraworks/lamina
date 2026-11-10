//! Pours: pads with too few thermal spokes.

use std::collections::BTreeMap;

use lamina_geom::prim::ring_contains;
use lamina_model::*;

use crate::ctx::Ctx;

pub(crate) fn run(ctx: &Ctx, v: &mut Vec<Violation>) {
    let rules = &ctx.d.board.rules;
    // A pad can have more than one pour: a through-hole pad has one on each
    // layer, and a large pad can be in two zones. Count all its spokes.
    // Two pads of a part can have the same number, so the position is a
    // part of the key.
    let key = |pad: &str, at: Point| {
        (
            pad.to_string(),
            (at.x * 1000.0).round() as i64,
            (at.y * 1000.0).round() as i64,
        )
    };
    let mut total: BTreeMap<(String, i64, i64), u32> = BTreeMap::new();
    for fill in &ctx.fills {
        for thermal in &fill.pad_thermals {
            *total.entry(key(&thermal.pad, thermal.at)).or_default() += thermal.spokes as u32;
        }
    }
    // A pour of the same net with a solid connection that has the point.
    let solid_at = |net: &Option<String>, at: Point| {
        ctx.fills.iter().any(|f| {
            f.net.is_some()
                && &f.net == net
                && ctx
                    .d
                    .routing
                    .zones
                    .iter()
                    .any(|z| z.id == f.zone && z.pad_connection == PadConnection::Solid)
                && f.polygons.iter().any(|p| {
                    ring_contains(&p.outer, at) && !p.holes.iter().any(|h| ring_contains(h, at))
                })
        })
    };
    for fill in &ctx.fills {
        for thermal in &fill.pad_thermals {
            let (pad, count) = (&thermal.pad, thermal.spokes);
            if count >= rules.min_thermal_spokes {
                continue;
            }
            let all = total.get(&key(pad, thermal.at)).copied().unwrap_or(0);
            if all >= rules.min_thermal_spokes as u32 || solid_at(&fill.net, thermal.at) {
                continue;
            }
            let violation = Violation::warning(
                "starved_thermal",
                format!(
                    "Pad {pad} has {count} spoke(s) to zone {}, but the minimum is {}",
                    fill.zone, rules.min_thermal_spokes
                ),
            )
            .on(fill.layer)
            .items([pad.clone(), fill.zone.clone()])
            .at(thermal.at);
            v.push(violation);
        }
    }
}

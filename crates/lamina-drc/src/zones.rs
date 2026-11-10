//! Pours: pads with too few thermal spokes.

use lamina_model::*;

use crate::ctx::Ctx;

pub(crate) fn run(ctx: &Ctx, v: &mut Vec<Violation>) {
    let rules = &ctx.d.board.rules;
    for fill in &ctx.fills {
        for thermal in &fill.pad_thermals {
            let (pad, count) = (&thermal.pad, thermal.spokes);
            if count < rules.min_thermal_spokes {
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
}

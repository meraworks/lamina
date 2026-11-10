//! Sizes of copper and holes: track widths, via drills and rings, pad drills and rings.

use lamina_model::*;

use crate::ctx::Ctx;
use crate::r3;

pub(crate) fn run(ctx: &Ctx, v: &mut Vec<Violation>) {
    let (d, g, rules, layers) = (ctx.d, &ctx.g, &ctx.d.board.rules, ctx.layers);
    // A copper text or drawing with a stroke that the fab cannot make.
    let mut thin: std::collections::BTreeMap<String, (f64, u8)> = Default::default();
    for c in &g.copper {
        if !c.owner.is_art() {
            continue;
        }
        if let lamina_geom::Prim::Capsule { r, .. } = &c.prim {
            if 2.0 * r + 1e-9 < rules.min_track_width {
                let e = thin.entry(c.owner.label()).or_insert((2.0 * r, c.layer));
                e.0 = e.0.min(2.0 * r);
            }
        }
    }
    for (id, (width, layer)) in thin {
        v.push(
            Violation::error(
                "track_width",
                format!(
                    "The copper drawing {id} has a line {} mm wide, but the minimum width is {} mm",
                    r3(width),
                    r3(rules.min_track_width)
                ),
            )
            .items([id])
            .on(Layer::Copper(Copper::from_index(layer, layers)))
            .measured(width, rules.min_track_width),
        );
    }

    // Track widths.
    for t in &d.routing.tracks {
        let areas = match t.points.first() {
            Some(p) if !d.routing.areas.is_empty() => d.routing.areas_at(*p, Some(t.layer)),
            _ => Vec::new(),
        };
        let limits = rules.resolve_for(&Subject {
            net: Some(&t.net),
            class: d.nets.nets.get(&t.net).and_then(|n| n.class.as_deref()),
            layer: Some(t.layer),
            kind: Some(RuleKind::Track),
            part: None,
            areas: &areas,
        });
        if let Some(max) = limits.max_track_width.filter(|m| t.width > m + 1e-9) {
            v.push(
                Violation::error(
                    "track_width",
                    format!(
                        "{} is {} mm wide, but the maximum width is {} mm",
                        t.id,
                        r3(t.width),
                        r3(max)
                    ),
                )
                .items([t.id.clone()])
                .on(t.layer)
                .measured(t.width, max)
                .fix(Fix::SetTrackWidth {
                    id: t.id.clone(),
                    width: max,
                }),
            );
        }
        if t.width + 1e-9 < limits.min_track_width {
            v.push(
                Violation::error(
                    "track_width",
                    format!(
                        "{} is {} mm wide, but the minimum width is {} mm",
                        t.id,
                        r3(t.width),
                        r3(limits.min_track_width)
                    ),
                )
                .items([t.id.clone()])
                .on(t.layer)
                .measured(t.width, limits.min_track_width)
                .fix(Fix::SetTrackWidth {
                    id: t.id.clone(),
                    width: limits.min_track_width,
                }),
            );
        }
        if t.points.len() < 2 {
            v.push(
                Violation::error("track_points", format!("{} has less than 2 points", t.id))
                    .items([t.id.clone()]),
            );
        }
    }

    // Vias.
    for via in &d.routing.vias {
        let class = d.nets.nets.get(&via.net).and_then(|n| n.class.as_deref());
        let from = via
            .from
            .and_then(|l| l.copper())
            .map(|c| c.index(layers))
            .unwrap_or(0);
        let to = via
            .to
            .and_then(|l| l.copper())
            .map(|c| c.index(layers))
            .unwrap_or(layers.saturating_sub(1));
        let mut min_drill: f64 = 0.0;
        let mut min_ring: f64 = 0.0;
        for index in from.min(to)..=from.max(to) {
            let l = Layer::Copper(Copper::from_index(index, layers));
            let areas = d.routing.areas_at(via.at, Some(l));
            let limits = rules.resolve_for(&Subject {
                net: Some(&via.net),
                class,
                layer: Some(l),
                kind: Some(RuleKind::Via),
                part: None,
                areas: &areas,
            });
            min_drill = min_drill.max(limits.min_via_drill);
            min_ring = min_ring.max(limits.min_annular_ring);
        }
        if via.drill + 1e-9 < min_drill {
            v.push(
                Violation::error(
                    "via_drill",
                    format!(
                        "The drill of {} is {} mm, but the minimum is {} mm",
                        via.id,
                        r3(via.drill),
                        r3(min_drill)
                    ),
                )
                .at(via.at)
                .items([via.id.clone()]),
            );
        }
        let ring = (via.diameter - via.drill) / 2.0;
        if ring + 1e-9 < min_ring {
            v.push(
                Violation::error(
                    "annular_ring",
                    format!(
                        "The annular ring of {} is {} mm, but the minimum is {} mm",
                        via.id,
                        r3(ring),
                        r3(min_ring)
                    ),
                )
                .at(via.at)
                .items([via.id.clone()])
                .measured(ring, min_ring)
                .fix(Fix::SetViaSize {
                    id: via.id.clone(),
                    diameter: via.drill + 2.0 * min_ring,
                    drill: via.drill,
                }),
            );
        }
    }

    // Pads: drills and rings.
    for (refdes, pl) in &d.placement.placements {
        let Some(fp) = d
            .parts
            .parts
            .get(refdes)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            continue;
        };
        for pad in &fp.pads {
            if let PadKind::Through { drill, plated } = &pad.kind {
                let dmin = drill.min_size();
                let label = format!("{refdes}.{}", pad.number);
                if dmin + 1e-9 < rules.min_drill {
                    v.push(
                        Violation::error(
                            "drill_size",
                            format!(
                                "The drill of {label} is {} mm, but the minimum is {} mm",
                                r3(dmin),
                                r3(rules.min_drill)
                            ),
                        )
                        .at(pl.transform(pad.at))
                        .items([label.clone()]),
                    );
                }
                if *plated {
                    let net = d.nets.net_of(&PadRef {
                        part: refdes.clone(),
                        pad: pad.number.clone(),
                    });
                    let class = net
                        .and_then(|n| d.nets.nets.get(n))
                        .and_then(|n| n.class.as_deref());
                    let min_ring = (0..layers)
                        .map(|index| {
                            let l = Layer::Copper(Copper::from_index(index, layers));
                            let areas = d.routing.areas_at(pl.transform(pad.at), Some(l));
                            rules
                                .resolve_for(&Subject {
                                    net,
                                    class,
                                    layer: Some(l),
                                    kind: Some(RuleKind::Pad),
                                    part: Some(refdes.as_str()),
                                    areas: &areas,
                                })
                                .min_annular_ring
                        })
                        .fold(0.0, f64::max);
                    let (w, h) = pad.shape.size();
                    let dmax = match drill {
                        Drill::Round { diameter } => (*diameter, *diameter),
                        Drill::Slot { width, height } => (*width, *height),
                    };
                    let ring = ((w - dmax.0) / 2.0).min((h - dmax.1) / 2.0);
                    if ring + 1e-9 < min_ring {
                        v.push(
                            Violation::error(
                                "annular_ring",
                                format!(
                                    "The annular ring of {label} is {} mm, but the minimum is {} mm",
                                    r3(ring),
                                    r3(min_ring)
                                ),
                            )
                            .at(pl.transform(pad.at))
                            .items([label]),
                        );
                    }
                }
            }
        }
    }
}

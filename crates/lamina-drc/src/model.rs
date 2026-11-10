//! Model checks that run after each change. DRC adds geometric checks to
//! these checks.

use std::collections::BTreeMap;

use lamina_model::*;

pub fn validate(d: &ProjectData) -> Vec<Violation> {
    let mut v = Vec::new();
    let layers = d.board.stackup.copper_layers;

    match &d.board.outline {
        None => v.push(Violation::warning(
            "no_outline",
            "The board outline is not set.",
        )),
        Some(c) => {
            if let Err(e) = c.check() {
                v.push(Violation::error("bad_outline", e));
            }
        }
    }

    for (r, p) in &d.parts.parts {
        if !d.footprints.contains_key(&p.footprint) {
            v.push(
                Violation::error(
                    "missing_footprint",
                    format!(
                        "{r} uses the footprint `{}`, but this footprint is not in cache/footprints.",
                        p.footprint
                    ),
                )
                .items([r.clone()]),
            );
        }
    }

    for r in d.placement.placements.keys() {
        if !d.parts.parts.contains_key(r) {
            v.push(
                Violation::error(
                    "placement_unknown_part",
                    format!("The placement {r} is for a part that does not exist."),
                )
                .items([r.clone()]),
            );
        }
    }

    let mut seen: BTreeMap<&PadRef, &str> = BTreeMap::new();
    for (name, net) in &d.nets.nets {
        if let Some(c) = &net.class {
            if !d.board.rules.net_classes.contains_key(c) {
                v.push(
                    Violation::warning(
                        "unknown_net_class",
                        format!("The net {name} uses the net class `{c}`, which is not defined."),
                    )
                    .items([name.clone()]),
                );
            }
        }
        for p in &net.pads {
            if let Some(other) = seen.insert(p, name) {
                if other != name {
                    v.push(
                        Violation::error(
                            "pad_in_two_nets",
                            format!("{p} is in two nets: {other} and {name}."),
                        )
                        .items([
                            p.to_string(),
                            other.to_string(),
                            name.clone(),
                        ]),
                    );
                }
            }
            match d.parts.parts.get(&p.part) {
                None => v.push(
                    Violation::warning(
                        "unresolved_pad",
                        format!("{p}: the part {} does not exist yet.", p.part),
                    )
                    .items([p.to_string()]),
                ),
                Some(part) => {
                    if let Some(fp) = d.footprints.get(&part.footprint) {
                        if fp.pad(&p.pad).is_none() {
                            v.push(
                                Violation::error(
                                    "unknown_pad",
                                    format!(
                                        "{p}: the footprint `{}` has no pad `{}`.",
                                        fp.id, p.pad
                                    ),
                                )
                                .items([p.to_string()]),
                            );
                        }
                    }
                }
            }
        }
    }

    pin_roles(d, &mut v);

    let net_exists = |n: &str| d.nets.nets.contains_key(n);
    for t in &d.routing.tracks {
        if !t.layer.is_copper() || !t.layer.exists_in(layers) {
            v.push(
                Violation::error(
                    "layer_not_in_stackup",
                    format!(
                        "The track {} is on {}, which is not a copper layer in the stackup.",
                        t.id, t.layer
                    ),
                )
                .items([t.id.clone()]),
            );
        }
        if !net_exists(&t.net) {
            v.push(
                Violation::error(
                    "unknown_net",
                    format!("The track {} uses the unknown net {}.", t.id, t.net),
                )
                .items([t.id.clone()]),
            );
        }
    }
    for via in &d.routing.vias {
        for l in [via.from, via.to].into_iter().flatten() {
            if !l.is_copper() || !l.exists_in(layers) {
                v.push(
                    Violation::error(
                        "layer_not_in_stackup",
                        format!(
                            "The via {} goes to {l}, which is not a copper layer in the stackup.",
                            via.id
                        ),
                    )
                    .items([via.id.clone()]),
                );
            }
        }
        if !net_exists(&via.net) {
            v.push(
                Violation::error(
                    "unknown_net",
                    format!("The via {} uses the unknown net {}.", via.id, via.net),
                )
                .items([via.id.clone()]),
            );
        }
    }
    for z in &d.routing.zones {
        for l in &z.layers {
            if !l.is_copper() || !l.exists_in(layers) {
                v.push(
                    Violation::error(
                        "layer_not_in_stackup",
                        format!(
                            "The zone {} is on {l}, which is not a copper layer in the stackup.",
                            z.id
                        ),
                    )
                    .items([z.id.clone()]),
                );
            }
        }
        if let Some(n) = &z.net {
            if !net_exists(n) {
                v.push(
                    Violation::error(
                        "unknown_net",
                        format!("The zone {} uses the unknown net {n}.", z.id),
                    )
                    .items([z.id.clone()]),
                );
            }
        }
    }
    v
}

/// Pins with a name that does not agree with the net: a ground pin on a
/// power net, a power pin on a ground net, or a no-connect pin on a net
/// with other pads. The names come from the datasheet through the
/// footprint. The check finds the mistakes that look fine on the copper.
fn pin_roles(d: &ProjectData, v: &mut Vec<Violation>) {
    for (net, n) in &d.nets.nets {
        let net_role = pin_role(net);
        let others = n.pads.len();
        for p in &n.pads {
            let Some(pad) = d
                .parts
                .parts
                .get(&p.part)
                .and_then(|part| d.footprints.get(&part.footprint))
                .and_then(|fp| fp.pad(&p.pad))
            else {
                continue;
            };
            let Some(name) = pad.name.as_deref() else {
                continue;
            };
            let msg = match (pin_role(name), net_role) {
                (PinRole::Ground, PinRole::Power) => {
                    format!("{p} is the {name} pin, but it is on the net {net}.")
                }
                (PinRole::Power, PinRole::Ground) => {
                    format!("{p} is the {name} pin, but it is on the net {net}.")
                }
                (PinRole::NoConnect, _) if others > 1 => {
                    format!(
                        "{p} is a no-connect pin ({name}), but it is on the net {net} with other pads."
                    )
                }
                _ => continue,
            };
            v.push(Violation::warning("pin_role", msg).items([p.to_string(), net.clone()]));
        }
    }
}

//! Model checks: data that is not valid or does not agree. They run before
//! the geometric checks.

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

    for n in d
        .notes
        .notes
        .iter()
        .filter(|n| n.author == "import" && !n.resolved)
    {
        v.push(
            Violation::warning(
                "import_loss",
                format!(
                    "The import dropped data: {}. Examine it, then resolve the note {}",
                    n.text, n.id
                ),
            )
            .items([n.id.clone()]),
        );
    }

    for (id, outline) in d
        .routing
        .zones
        .iter()
        .map(|z| (&z.id, &z.outline))
        .chain(d.routing.keepouts.iter().map(|k| (&k.id, &k.outline)))
    {
        if let Err(e) = outline_ok(outline) {
            v.push(
                Violation::error(
                    "zone_outline",
                    format!("The outline of {id} is not valid: {e}"),
                )
                .items([id.clone()]),
            );
        }
    }

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
    for x in &mut v {
        if x.at.is_none() {
            x.at = x.items.iter().find_map(|i| locate(d, i));
        }
    }
    v
}

/// The board position of an item of a violation: a track, a via, a zone, a
/// keep-out, a note, a pad, a part or a net. A marker in the viewer uses it.
pub fn locate(d: &ProjectData, item: &str) -> Option<Point> {
    let ok = |p: Point| (p.x.is_finite() && p.y.is_finite()).then_some(p);
    if let Some(t) = d.routing.tracks.iter().find(|t| t.id == item) {
        return t.points.first().copied().and_then(ok);
    }
    if let Some(via) = d.routing.vias.iter().find(|x| x.id == item) {
        return ok(via.at);
    }
    let outline = d
        .routing
        .zones
        .iter()
        .find(|z| z.id == item)
        .map(|z| &z.outline)
        .or_else(|| {
            d.routing
                .keepouts
                .iter()
                .find(|k| k.id == item)
                .map(|k| &k.outline)
        });
    if let Some(o) = outline {
        return o
            .vertices
            .first()
            .map(|v| Point::new(v.x, v.y))
            .and_then(ok);
    }
    if let Some(n) = d.notes.notes.iter().find(|n| n.id == item) {
        return n.at.and_then(ok).or_else(|| {
            let pl = d.placement.placements.get(n.refdes.as_deref()?)?;
            ok(Point::new(pl.x, pl.y))
        });
    }
    if let Some(pl) = d.placement.placements.get(item) {
        return ok(Point::new(pl.x, pl.y));
    }
    if let Ok(pad) = item.parse::<PadRef>() {
        let pl = d.placement.placements.get(&pad.part)?;
        let at = d
            .parts
            .parts
            .get(&pad.part)
            .and_then(|p| d.footprints.get(&p.footprint))
            .and_then(|fp| fp.pad(&pad.pad))
            .map(|p| pl.transform(p.at))
            .unwrap_or(Point::new(pl.x, pl.y));
        return ok(at);
    }
    if let Some(net) = d.nets.nets.get(item) {
        return net.pads.iter().find_map(|p| locate(d, &p.to_string()));
    }
    None
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

/// Check that an outline is a shape that the polygon code can use.
pub fn outline_ok(c: &Contour) -> Result<(), String> {
    c.check()?;
    let pts = c.flatten(0.02);
    if pts.len() < 3 {
        return Err("The outline has fewer than 3 points.".into());
    }
    let n = pts.len();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (p, q) = (pts[j], pts[(j + 1) % n]);
            if lamina_geom::prim::segments_intersect(a, b, p, q) {
                return Err("The outline crosses itself.".into());
            }
        }
    }
    Ok(())
}

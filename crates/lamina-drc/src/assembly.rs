//! Assembly and BOM checks: fiducials, paste, parts near the edge, part
//! height, tombstone risk, vias in pads, test points and part numbers.

use std::collections::{BTreeMap, BTreeSet};

use lamina_geom::fill::ZoneFill;
use lamina_geom::{BoardGeometry, Prim, pad_prim};
use lamina_model::*;

use crate::{polys_overlap, r3};

/// What a part is for the assembly checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Fiducial,
    TestPoint,
    Other,
}

/// A part with the field `kind`, or a one-pad part with a `FID` or `TP` reference.
pub fn part_kind(refdes: &str, part: &Part, fp: &Footprint) -> PartKind {
    match part.fields.get("kind").map(String::as_str) {
        Some("fiducial") => return PartKind::Fiducial,
        Some("testpoint") => return PartKind::TestPoint,
        _ => {}
    }
    if fp.pads.len() == 1 {
        if refdes.starts_with("FID") {
            return PartKind::Fiducial;
        }
        if refdes.starts_with("TP") {
            return PartKind::TestPoint;
        }
    }
    PartKind::Other
}

struct Placed<'a> {
    refdes: &'a str,
    part: &'a Part,
    fp: &'a Footprint,
    pl: &'a Placement,
    kind: PartKind,
    courtyard: Option<Vec<Point>>,
}

fn poly(p: &[Point]) -> Prim {
    Prim::Poly {
        outer: p.to_vec(),
        holes: vec![],
    }
}

pub(crate) fn check(d: &ProjectData, g: &BoardGeometry, fills: &[ZoneFill]) -> Vec<Violation> {
    let rules = &d.board.rules;
    let nets = d.nets.pad_map();
    let mut v = Vec::new();
    let mut parts: Vec<Placed> = Vec::new();
    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        parts.push(Placed {
            refdes,
            part,
            fp,
            pl,
            kind: part_kind(refdes, part, fp),
            courtyard: lamina_geom::courtyard(fp, pl),
        });
    }
    let net_of = |refdes: &str, pad: &Pad| {
        (!pad.number.is_empty())
            .then(|| nets.get(&PadRef::new(refdes.to_string(), pad.number.clone())))
            .flatten()
    };

    // Fiducials for each side that has SMD parts.
    if rules.min_fiducials > 0 {
        for side in [Side::Top, Side::Bottom] {
            let smd = parts.iter().any(|p| {
                p.pl.side == side
                    && !p.part.dnp
                    && p.kind == PartKind::Other
                    && p.fp.pads.iter().any(Pad::is_smd)
            });
            let n = parts
                .iter()
                .filter(|p| p.pl.side == side && p.kind == PartKind::Fiducial)
                .count() as u32;
            if smd && n < rules.min_fiducials {
                let name = if side == Side::Top { "top" } else { "bottom" };
                v.push(
                    Violation::warning(
                        "fiducial_count",
                        format!(
                            "The {name} side has SMD parts and {n} fiducial(s), but the minimum is {}",
                            rules.min_fiducials
                        ),
                    )
                    .items([name.to_string()])
                    .measured(n as f64, rules.min_fiducials as f64),
                );
            }
        }
    }

    // The board edge, for the distance checks.
    let mut edges: Vec<Prim> = Vec::new();
    for ring in g.outline.iter().chain(g.cutouts.iter()) {
        for k in 0..ring.len() {
            edges.push(Prim::Capsule {
                a: ring[k],
                b: ring[(k + 1) % ring.len()],
                r: 0.0,
            });
        }
    }
    let edge_dist = |p: &Prim| {
        edges
            .iter()
            .map(|e| p.distance(e))
            .fold(f64::INFINITY, f64::min)
    };

    for p in &parts {
        let refdes = p.refdes.to_string();
        let at = Point::new(p.pl.x, p.pl.y);
        if rules.part_to_edge > 0.0 && p.kind == PartKind::Other {
            if let Some(c) = &p.courtyard {
                let dist = edge_dist(&poly(c));
                if dist + 1e-6 < rules.part_to_edge {
                    v.push(
                        Violation::warning(
                            "part_to_edge",
                            format!(
                                "{refdes} is {} mm from the board edge, but the minimum is {} mm",
                                r3(dist),
                                r3(rules.part_to_edge)
                            ),
                        )
                        .at(at)
                        .items([refdes.clone()])
                        .measured(dist, rules.part_to_edge),
                    );
                }
            }
        }
        // The height limit of a keep-out.
        if let (Some(height), Some(c)) = (p.fp.height, &p.courtyard) {
            for k in &d.routing.keepouts {
                let Some(max) = rules.height_limits.get(&k.id) else {
                    continue;
                };
                if height > *max + 1e-9 && polys_overlap(c, &k.outline.flatten(0.02)) {
                    v.push(
                        Violation::error(
                            "part_height",
                            format!(
                                "{refdes} is {} mm high in the area {}, but the maximum is {} mm",
                                r3(height),
                                k.id,
                                r3(*max)
                            ),
                        )
                        .at(at)
                        .items([refdes.clone(), k.id.clone()])
                        .measured(height, *max),
                    );
                }
            }
        }
        if p.part.dnp {
            continue;
        }
        // Paste.
        let smd: Vec<&Pad> = p.fp.pads.iter().filter(|x| x.is_smd()).collect();
        let with = smd.iter().filter(|x| x.paste).count();
        if with > 0 && p.kind == PartKind::Other {
            let paste_art: Vec<Prim> =
                p.fp.graphics
                    .iter()
                    .filter(|gr| gr.layer == FpLayer::Paste)
                    .flat_map(|gr| {
                        lamina_geom::art::shape_prims(&gr.shape, gr.width, gr.filled, &|q| q)
                    })
                    .collect();
            let bare: Vec<&str> = smd
                .iter()
                .filter(|x| !x.paste && net_of(p.refdes, x).is_some())
                .filter(|x| {
                    let pad = pad_prim(x, &Placement::at(0.0, 0.0));
                    !paste_art.iter().any(|a| a.distance(&pad) <= 1e-6)
                })
                .map(|x| x.number.as_str())
                .collect();
            if !bare.is_empty() {
                let list: BTreeSet<&str> = bare.into_iter().collect();
                v.push(
                    Violation::warning(
                        "paste_missing",
                        format!(
                            "{refdes} has pads with paste, but these pads with a net have none: {}",
                            list.into_iter().collect::<Vec<_>>().join(", ")
                        ),
                    )
                    .at(at)
                    .on(Layer::Paste(p.pl.side))
                    .items([refdes.clone()]),
                );
            }
        }
        if smd
            .iter()
            .any(|x| x.paste && x.paste_margin.unwrap_or(rules.paste_margin) > 1e-9)
        {
            v.push(
                Violation::warning(
                    "paste_oversize",
                    format!("{refdes} has a paste opening that is larger than its pad"),
                )
                .at(at)
                .on(Layer::Paste(p.pl.side))
                .items([refdes.clone()]),
            );
        }
        // Test points.
        if p.kind == PartKind::TestPoint {
            let mut why: Vec<String> = Vec::new();
            for pad in &p.fp.pads {
                let (w, h) = pad.shape.size();
                if w.min(h) + 1e-9 < rules.min_testpoint {
                    why.push(format!(
                        "its pad is {} mm, below the minimum {} mm",
                        r3(w.min(h)),
                        r3(rules.min_testpoint)
                    ));
                }
                if !pad.mask {
                    why.push("it has no mask opening".into());
                }
                let prim = pad_prim(pad, p.pl);
                if rules.testpoint_to_edge > 0.0
                    && edge_dist(&prim) + 1e-6 < rules.testpoint_to_edge
                {
                    why.push(format!(
                        "it is {} mm from the board edge, below the minimum {} mm",
                        r3(edge_dist(&prim)),
                        r3(rules.testpoint_to_edge)
                    ));
                }
                let centre = p.pl.transform(pad.at);
                for o in &parts {
                    if o.refdes != p.refdes
                        && o.pl.side == p.pl.side
                        && o.kind == PartKind::Other
                        && o.courtyard
                            .as_ref()
                            .is_some_and(|c| lamina_geom::prim::ring_contains(c, centre))
                    {
                        why.push(format!("it is under {}", o.refdes));
                    }
                }
            }
            if !why.is_empty() {
                v.push(
                    Violation::warning(
                        "testpoint_access",
                        format!(
                            "A probe cannot use the test point {refdes}: {}",
                            why.join("; ")
                        ),
                    )
                    .at(at)
                    .items([refdes.clone()]),
                );
            }
        }
        // Tombstone risk of small two-pad parts.
        if smd.len() == 2
            && p.fp.pads.len() == 2
            && smd.iter().all(|x| x.paste)
            && smd[0].at.dist(smd[1].at) <= 1.7
            && smd[0].shape.size() == smd[1].shape.size()
        {
            let escape: Vec<Option<(f64, bool)>> = smd
                .iter()
                .map(|pad| escape_of(d, fills, p, pad, net_of(p.refdes, pad)))
                .collect();
            if let (Some((a, a_solid)), Some((b, b_solid))) = (escape[0], escape[1]) {
                let (w, h) = smd[0].shape.size();
                let short = w.min(h);
                let wide = |e: f64, solid: bool| solid || e + 1e-9 >= 0.8 * short;
                let (lo, hi) = (a.min(b), a.max(b));
                let one_wide = wide(a, a_solid) != wide(b, b_solid);
                if lo > 1e-9 && one_wide && hi / lo >= 3.0 - 1e-9 {
                    v.push(
                        Violation::warning(
                            "tombstone",
                            format!(
                                "{refdes} has {} mm of copper on one pad and {} mm on the other. The pad with more copper heats more slowly, and the part can stand up",
                                r3(hi),
                                r3(lo)
                            ),
                        )
                        .at(at)
                        .items([refdes.clone()]),
                    );
                }
            }
        }
    }

    // A via in an SMD pad.
    for via in &d.routing.vias {
        for p in &parts {
            for pad in p.fp.pads.iter().filter(|x| x.is_smd()) {
                let prim = pad_prim(pad, p.pl);
                if !prim.contains(via.at) {
                    continue;
                }
                let (w, h) = pad.shape.size();
                let same = net_of(p.refdes, pad).is_some_and(|n| *n == via.net);
                // A large pad of the same net with vias is a thermal pad.
                if same && w.min(h) > 3.0 * via.diameter {
                    continue;
                }
                v.push(
                    Violation::warning(
                        "via_in_pad",
                        format!(
                            "The via {} is in the pad {}.{}. Solder can flow into the hole",
                            via.id, p.refdes, pad.number
                        ),
                    )
                    .at(via.at)
                    .items([via.id.clone(), format!("{}.{}", p.refdes, pad.number)]),
                );
            }
        }
    }

    bom(d, &parts, &mut v);
    v
}

/// The width of the copper that leaves a pad, and true if a solid pour covers it.
fn escape_of(
    d: &ProjectData,
    fills: &[ZoneFill],
    p: &Placed,
    pad: &Pad,
    net: Option<&String>,
) -> Option<(f64, bool)> {
    let net = net?;
    let prim = pad_prim(pad, p.pl);
    let layer = if p.pl.side == Side::Top {
        Layer::TOP
    } else {
        Layer::BOTTOM
    };
    let mut width = 0.0;
    for t in &d.routing.tracks {
        if &t.net != net || t.layer != layer || t.id.starts_with("td") {
            continue;
        }
        for end in [t.points.first(), t.points.last()].into_iter().flatten() {
            if prim.contains(*end) {
                width += t.width;
            }
        }
    }
    let centre = p.pl.transform(pad.at);
    let label = format!("{}.{}", p.refdes, pad.number);
    let mut solid = false;
    let mut thermal = false;
    for z in &d.routing.zones {
        if z.net.as_ref() != Some(net) || !z.layers.contains(&layer) {
            continue;
        }
        let inside = lamina_geom::prim::ring_contains(&z.outline.flatten(0.05), centre);
        if !inside {
            continue;
        }
        match z.pad_connection {
            PadConnection::Solid => {
                let (w, h) = pad.shape.size();
                solid = true;
                width += 2.0 * (w + h);
            }
            PadConnection::Thermal => {
                let spokes = fills
                    .iter()
                    .filter(|f| f.zone == z.id && f.layer == layer)
                    .flat_map(|f| f.pad_thermals.iter())
                    .filter(|t| t.pad == label)
                    .map(|t| t.spokes as f64)
                    .fold(0.0, f64::max);
                // Thermal spokes are the correct way to connect a small pad
                // to a pour. They do not count as wide copper.
                thermal = spokes > 0.0;
            }
            PadConnection::None => {}
        }
    }
    // A pad on spokes only counts as a pad on one track of the same width.
    if width <= 1e-9 && thermal {
        width = 0.25;
    }
    Some((width, solid))
}

fn bom(d: &ProjectData, parts: &[Placed], v: &mut Vec<Violation>) {
    let _ = d;
    // The BOM lines that have no part number.
    let mut lines: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut by_mpn: BTreeMap<&str, Vec<&Placed>> = BTreeMap::new();
    let mut by_line: BTreeMap<(String, String), BTreeSet<&str>> = BTreeMap::new();
    for p in parts {
        let excluded = p.part.dnp
            || p.kind != PartKind::Other
            || p.part
                .fields
                .get("exclude_from_bom")
                .is_some_and(|x| x == "yes")
            || p.fp.pads.iter().all(|x| x.number.is_empty());
        if excluded {
            continue;
        }
        let key = (
            p.part.value.clone().unwrap_or_default(),
            p.part.footprint.clone(),
        );
        if p.part.mpn.is_none() && p.part.lcsc.is_none() {
            lines.entry(key.clone()).or_default().push(p.refdes.into());
        }
        if let Some(mpn) = p.part.mpn.as_deref() {
            by_mpn.entry(mpn).or_default().push(p);
            by_line.entry(key).or_default().insert(mpn);
        }
    }
    for ((value, footprint), refs) in lines {
        v.push(
            Violation::warning(
                "bom_part_number",
                format!(
                    "{} ({value}, {footprint}) has no MPN and no LCSC number",
                    refs.join(", ")
                ),
            )
            .items(refs),
        );
    }
    let passive = |r: &str| {
        let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        matches!(letters.as_str(), "R" | "C" | "L" | "D" | "Q" | "U")
    };
    for (mpn, group) in &by_mpn {
        let hashes: BTreeSet<String> = group.iter().map(|p| p.fp.pads_hash()).collect();
        let values: BTreeSet<&str> = group
            .iter()
            .filter(|p| passive(p.refdes))
            .filter_map(|p| p.part.value.as_deref())
            .collect();
        let refs: Vec<String> = group.iter().map(|p| p.refdes.to_string()).collect();
        if hashes.len() > 1 {
            v.push(
                Violation::warning(
                    "bom_conflict",
                    format!(
                        "The part number {mpn} has footprints with different pads: {}",
                        refs.join(", ")
                    ),
                )
                .items(refs.clone()),
            );
        } else if values.len() > 1 {
            v.push(
                Violation::warning(
                    "bom_conflict",
                    format!(
                        "The part number {mpn} has different values ({}): {}",
                        values.into_iter().collect::<Vec<_>>().join(", "),
                        refs.join(", ")
                    ),
                )
                .items(refs),
            );
        }
    }
    for ((value, footprint), mpns) in by_line {
        if mpns.len() > 1 && !value.is_empty() {
            v.push(
                Violation::warning(
                    "bom_conflict",
                    format!(
                        "The value {value} with the footprint {footprint} has more than one part number: {}",
                        mpns.iter().copied().collect::<Vec<_>>().join(", ")
                    ),
                )
                .items([value.clone(), footprint.clone()]),
            );
        }
    }
}

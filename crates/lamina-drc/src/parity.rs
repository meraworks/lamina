//! Checks that KiCad has and that are not in the other modules: hole
//! sizes, via counts, courtyards, short segments, track angles, zones,
//! mask openings and text variables.

use std::collections::BTreeMap;

use lamina_geom::{BoardGeometry, Owner, Prim};
use lamina_model::*;

use crate::{polys_overlap, r3};

pub(crate) fn check(d: &ProjectData, g: &BoardGeometry) -> Vec<Violation> {
    let mut v = Vec::new();
    holes(d, &mut v);
    via_count(d, &mut v);
    courtyards(d, g.layers, &mut v);
    tracks(d, &mut v);
    zones(d, &mut v);
    mask_and_text(d, &mut v);
    let _ = g;
    v
}

/// Board holes, the largest drill and slot widths. The pad loop of the
/// analysis has the minimum drill and the ring of footprint pads.
fn holes(d: &ProjectData, v: &mut Vec<Violation>) {
    let rules = &d.board.rules;
    let one = |label: String, at: Point, drill: &Drill, v: &mut Vec<Violation>| {
        let (min, max) = match drill {
            Drill::Round { diameter } => (*diameter, *diameter),
            Drill::Slot { width, height } => (width.min(*height), width.max(*height)),
        };
        if rules.max_drill > 0.0 && min > rules.max_drill + 1e-9 {
            v.push(
                Violation::error(
                    "drill_size",
                    format!(
                        "The drill of {label} is {} mm, but the maximum is {} mm",
                        r3(min),
                        r3(rules.max_drill)
                    ),
                )
                .at(at)
                .items([label.clone()])
                .measured(min, rules.max_drill),
            );
        }
        if matches!(drill, Drill::Slot { .. }) && min + 1e-9 < rules.min_slot_width {
            v.push(
                Violation::error(
                    "drill_size",
                    format!(
                        "The slot of {label} is {} mm wide, but the minimum is {} mm",
                        r3(min),
                        r3(rules.min_slot_width)
                    ),
                )
                .at(at)
                .items([label.clone()])
                .measured(min, rules.min_slot_width),
            );
        }
        let _ = max;
    };
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
            if let PadKind::Through { drill, .. } = &pad.kind {
                one(
                    format!("{refdes}.{}", pad.number),
                    pl.transform(pad.at),
                    drill,
                    v,
                );
            }
        }
    }
    for h in &d.board.holes {
        one(h.id.clone(), h.at, &h.drill, v);
        let min = h.drill.min_size();
        if min + 1e-9 < rules.min_drill {
            v.push(
                Violation::error(
                    "drill_size",
                    format!(
                        "The drill of {} is {} mm, but the minimum is {} mm",
                        h.id,
                        r3(min),
                        r3(rules.min_drill)
                    ),
                )
                .at(h.at)
                .items([h.id.clone()])
                .measured(min, rules.min_drill),
            );
        }
        if let (true, Some(pad)) = (h.plated, h.pad_diameter) {
            let max = match &h.drill {
                Drill::Round { diameter } => *diameter,
                Drill::Slot { width, height } => width.max(*height),
            };
            let ring = (pad - max) / 2.0;
            if ring + 1e-9 < rules.min_annular_ring {
                v.push(
                    Violation::error(
                        "annular_ring",
                        format!(
                            "The annular ring of {} is {} mm, but the minimum is {} mm",
                            h.id,
                            r3(ring),
                            r3(rules.min_annular_ring)
                        ),
                    )
                    .at(h.at)
                    .items([h.id.clone()])
                    .measured(ring, rules.min_annular_ring),
                );
            }
        }
    }
}

fn via_count(d: &ProjectData, v: &mut Vec<Violation>) {
    let mut count: BTreeMap<&str, u32> = BTreeMap::new();
    for via in &d.routing.vias {
        *count.entry(via.net.as_str()).or_default() += 1;
    }
    for (name, net) in &d.nets.nets {
        let Some(max) = net.max_vias else { continue };
        let n = count.get(name.as_str()).copied().unwrap_or(0);
        if n > max {
            v.push(
                Violation::warning(
                    "via_count",
                    format!("The net {name} has {n} vias, but the maximum is {max}"),
                )
                .items([name.clone()])
                .measured(n as f64, max as f64),
            );
        }
    }
}

fn courtyards(d: &ProjectData, _layers: u8, v: &mut Vec<Violation>) {
    let need = d.board.rules.courtyard_clearance;
    let mut cys: Vec<(&String, Side, Vec<Point>)> = Vec::new();
    for (refdes, pl) in &d.placement.placements {
        let Some(fp) = d
            .parts
            .parts
            .get(refdes)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            continue;
        };
        let edges: Vec<Edge> = fp
            .graphics
            .iter()
            .filter(|g| g.layer == FpLayer::Courtyard)
            .flat_map(|g| edges_of(&g.shape))
            .collect();
        // A hole or a fiducial with one pad needs no courtyard.
        if edges.is_empty() && fp.pads.len() > 1 {
            v.push(
                Violation::warning(
                    "courtyard_missing",
                    format!(
                        "{refdes} has no courtyard drawing. The overlap check uses a box around its pads"
                    ),
                )
                .at(Point::new(pl.x, pl.y))
                .on(Layer::Courtyard(pl.side))
                .items([refdes.clone()]),
            );
        } else if !edges.is_empty() {
            let (loops, open) = chain_edges(edges, 0.05);
            if open > 0 || loops.is_empty() {
                v.push(
                    Violation::warning(
                        "courtyard_open",
                        format!("The courtyard of {refdes} is not a closed loop"),
                    )
                    .at(Point::new(pl.x, pl.y))
                    .on(Layer::Courtyard(pl.side))
                    .items([refdes.clone()]),
                );
            }
        }
        if need > 0.0 {
            if let Some(c) = lamina_geom::courtyard(fp, pl) {
                cys.push((refdes, pl.side, c));
            }
        }
    }
    for i in 0..cys.len() {
        for j in (i + 1)..cys.len() {
            if cys[i].1 != cys[j].1 || polys_overlap(&cys[i].2, &cys[j].2) {
                continue;
            }
            let poly = |p: &Vec<Point>| Prim::Poly {
                outer: p.clone(),
                holes: vec![],
            };
            let dist = poly(&cys[i].2).distance(&poly(&cys[j].2));
            if dist + 1e-6 < need {
                v.push(
                    Violation::error(
                        "courtyard_overlap",
                        format!(
                            "The courtyards of {} and {} are {} mm apart, but the minimum is {} mm",
                            cys[i].0,
                            cys[j].0,
                            r3(dist),
                            r3(need)
                        ),
                    )
                    .at(cys[i].2[0])
                    .items([cys[i].0.clone(), cys[j].0.clone()])
                    .measured(dist, need),
                );
            }
        }
    }
}

fn tracks(d: &ProjectData, v: &mut Vec<Violation>) {
    let min_angle = d.board.rules.min_track_angle;
    for t in &d.routing.tracks {
        if t.points.windows(2).any(|w| w[0].dist(w[1]) < 0.01) {
            v.push(
                Violation::warning(
                    "track_zero_length",
                    format!("{} has a segment that is shorter than 0.01 mm", t.id),
                )
                .at(t.points[0])
                .on(t.layer)
                .items([t.id.clone()]),
            );
        }
        // The angle between two segments of one track.
        for w in t.points.windows(3) {
            let (a, b) = (w[0].sub(w[1]), w[2].sub(w[1]));
            let (la, lb) = (a.x.hypot(a.y), b.x.hypot(b.y));
            if la < 0.01 || lb < 0.01 {
                continue;
            }
            let cos = ((a.x * b.x + a.y * b.y) / (la * lb)).clamp(-1.0, 1.0);
            let angle = cos.acos().to_degrees();
            if angle + 1e-6 < min_angle {
                v.push(
                    Violation::warning(
                        "track_angle",
                        format!(
                            "{} has a corner of {} degrees, but the minimum is {} degrees",
                            t.id,
                            r3(angle),
                            r3(min_angle)
                        ),
                    )
                    .at(w[1])
                    .on(t.layer)
                    .items([t.id.clone()])
                    .measured(angle, min_angle),
                );
                break;
            }
        }
    }
}

fn zones(d: &ProjectData, v: &mut Vec<Violation>) {
    let zs = &d.routing.zones;
    for i in 0..zs.len() {
        for j in (i + 1)..zs.len() {
            let (a, b) = (&zs[i], &zs[j]);
            if a.net == b.net
                || a.priority != b.priority
                || !a.layers.iter().any(|l| b.layers.contains(l))
                || a.outline.check().is_err()
                || b.outline.check().is_err()
            {
                continue;
            }
            let (pa, pb) = (a.outline.flatten(0.02), b.outline.flatten(0.02));
            if polys_overlap(&pa, &pb) {
                v.push(
                    Violation::warning(
                        "zone_overlap",
                        format!(
                            "The zones {} and {} overlap and have the same priority. Set a priority, or the pour of the first id wins",
                            a.id, b.id
                        ),
                    )
                    .at(pb[0])
                    .items([a.id.clone(), b.id.clone()]),
                );
            }
        }
    }
}

fn mask_and_text(d: &ProjectData, v: &mut Vec<Violation>) {
    let nets = d.nets.pad_map();
    let min_w = d.board.rules.min_silk_width;
    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        for pad in &fp.pads {
            let has_net = !pad.number.is_empty()
                && nets.contains_key(&PadRef::new(refdes.clone(), pad.number.clone()));
            if pad.is_smd() && pad.paste && !pad.mask && has_net {
                v.push(
                    Violation::warning(
                        "mask_missing",
                        format!(
                            "The pad {refdes}.{} gets solder paste but has no mask opening",
                            pad.number
                        ),
                    )
                    .at(pl.transform(pad.at))
                    .on(Layer::Mask(pl.side))
                    .items([format!("{refdes}.{}", pad.number)]),
                );
            }
        }
        let thin = fp
            .graphics
            .iter()
            .filter(|g| g.layer == FpLayer::Silk && !g.filled && g.width > 0.0)
            .map(|g| g.width)
            .fold(f64::INFINITY, f64::min);
        if thin + 1e-9 < min_w {
            v.push(
                Violation::warning(
                    "silk_line_width",
                    format!(
                        "The silkscreen of {refdes} has a line of {} mm, but the minimum is {} mm",
                        r3(thin),
                        r3(min_w)
                    ),
                )
                .at(Point::new(pl.x, pl.y))
                .on(Layer::Silk(pl.side))
                .items([refdes.clone()])
                .measured(thin, min_w),
            );
        }
        for t in &fp.texts {
            let text = t.text.replace("${ref}", "").replace("${value}", "");
            if text.contains("${") {
                v.push(
                    Violation::warning(
                        "text_variable",
                        format!(
                            "A text of {refdes} has a variable that Lamina does not know: {}",
                            t.text
                        ),
                    )
                    .at(pl.transform(t.at))
                    .items([refdes.clone()]),
                );
            }
        }
    }
    for gr in &d.routing.graphics {
        if matches!(gr.layer, Layer::Silk(_))
            && !gr.filled
            && gr.width > 0.0
            && gr.width + 1e-9 < min_w
        {
            v.push(
                Violation::warning(
                    "silk_line_width",
                    format!(
                        "The silkscreen drawing {} has a line of {} mm, but the minimum is {} mm",
                        gr.id,
                        r3(gr.width),
                        r3(min_w)
                    ),
                )
                .on(gr.layer)
                .items([gr.id.clone()])
                .measured(gr.width, min_w),
            );
        }
    }
    for t in &d.routing.texts {
        if t.text.contains("${") {
            v.push(
                Violation::warning(
                    "text_variable",
                    format!(
                        "The text {} has a variable that Lamina does not know: {}",
                        t.id, t.text
                    ),
                )
                .at(t.at)
                .on(t.layer)
                .items([t.id.clone()]),
            );
        }
    }
}

/// A round primitive made smaller by `d`. A polygon stays as it is.
pub(crate) fn shrunk(p: &Prim, d: f64) -> Prim {
    match p {
        Prim::Circle { c, r } => Prim::Circle {
            c: *c,
            r: (r - d).max(1e-4),
        },
        Prim::Capsule { a, b, r } => Prim::Capsule {
            a: *a,
            b: *b,
            r: (r - d).max(1e-4),
        },
        other => other.clone(),
    }
}

/// True if two items that touch connect with less than `min` of overlap.
pub(crate) fn is_weak(a: &Prim, b: &Prim, min: f64) -> bool {
    if min <= 0.0 {
        return false;
    }
    let round = |p: &Prim| !matches!(p, Prim::Poly { .. });
    match (round(a), round(b)) {
        (true, true) => shrunk(a, min / 2.0).distance(&shrunk(b, min / 2.0)) > 1e-6,
        (true, false) => shrunk(a, min).distance(b) > 1e-6,
        (false, true) => a.distance(&shrunk(b, min)) > 1e-6,
        (false, false) => false,
    }
}

pub(crate) fn weak_violation(a: &Owner, b: &Owner, at: Point, layer: Layer, min: f64) -> Violation {
    let mut pair = [a.label(), b.label()];
    pair.sort();
    Violation::warning(
        "weak_connection",
        format!(
            "{} and {} connect only where they touch. The overlap is less than {} mm",
            pair[0],
            pair[1],
            r3(min)
        ),
    )
    .at(at)
    .on(layer)
    .items(pair)
}

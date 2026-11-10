//! Electrical checks that need no schematic: nets with one pad, pads with
//! no net, track current, voltage distance, decoupling capacitors and the
//! return path of fast signals.

use std::collections::{BTreeMap, BTreeSet};

use lamina_geom::fill::ZoneFill;
use lamina_geom::prim::ring_contains;
use lamina_geom::{BoardGeometry, Index, Owner};
use lamina_model::*;
use serde::Deserialize;

use crate::r3;

#[derive(Debug, Clone, Copy, Deserialize)]
struct Spacing {
    internal: f64,
    external_bare: f64,
    external_coated: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct SpacingRow {
    up_to: f64,
    #[serde(flatten)]
    spacing: Spacing,
}

#[derive(Debug, Clone, Deserialize)]
struct SpacingTable {
    rows: Vec<SpacingRow>,
    above_500_per_volt: Spacing,
}

/// Where two conductors are, for the spacing table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// An inner layer.
    Internal,
    /// An outer layer, and one of the two items has no solder mask (a pad).
    ExternalBare,
    /// An outer layer below solder mask.
    ExternalCoated,
}

fn table() -> &'static SpacingTable {
    static TABLE: std::sync::OnceLock<SpacingTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("profiles/ipc2221_spacing.json"))
            .expect("the spacing table is valid JSON")
    })
}

/// The minimum distance in mm between two conductors with `volts` between them.
pub fn voltage_spacing(volts: f64, surface: Surface) -> f64 {
    let t = table();
    let pick = |s: &Spacing| match surface {
        Surface::Internal => s.internal,
        Surface::ExternalBare => s.external_bare,
        Surface::ExternalCoated => s.external_coated,
    };
    let v = volts.abs();
    for row in &t.rows {
        if v <= row.up_to {
            return pick(&row.spacing);
        }
    }
    let last = t.rows.last().expect("the table has rows");
    pick(&last.spacing) + (v - last.up_to) * pick(&t.above_500_per_volt)
}

/// The track width in mm that carries `amps` with a temperature rise of
/// `rise` degrees C (IPC-2221: I = k * dT^0.44 * A^0.725, A in square mil).
pub fn width_for_current(amps: f64, rise: f64, copper_oz: f64, outer: bool) -> f64 {
    let k = if outer { 0.048 } else { 0.024 };
    let area = (amps / (k * rise.max(0.1).powf(0.44))).powf(1.0 / 0.725);
    let thickness_mil = 1.378 * copper_oz.max(0.01);
    area / thickness_mil * 0.0254
}

/// The voltage of a net: its `voltage`, or 0 for a ground net.
fn volts(d: &ProjectData, net: &str) -> Option<f64> {
    let n = d.nets.nets.get(net)?;
    n.voltage
        .or_else(|| (pin_role(net) == PinRole::Ground).then_some(0.0))
}

pub(crate) fn check(d: &ProjectData, g: &BoardGeometry, fills: &[ZoneFill]) -> Vec<Violation> {
    let mut v = Vec::new();
    single_pads(d, &mut v);
    current(d, &mut v);
    voltage(d, g, &mut v);
    decoupling(d, &mut v);
    return_path(d, fills, &mut v);
    v
}

fn single_pads(d: &ProjectData, v: &mut Vec<Violation>) {
    for (name, net) in &d.nets.nets {
        if net.pads.len() == 1 {
            v.push(
                Violation::warning(
                    "net_single_pad",
                    format!(
                        "The net {name} has only the pad {}. It connects nothing",
                        net.pads[0]
                    ),
                )
                .items([name.clone()]),
            );
        }
    }
    let nets = d.nets.pad_map();
    for (refdes, part) in &d.parts.parts {
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        let free: BTreeSet<&str> = fp
            .pads
            .iter()
            .filter(|p| !p.number.is_empty())
            .filter(|p| !nets.contains_key(&PadRef::new(refdes.clone(), p.number.clone())))
            .filter(|p| {
                p.name
                    .as_deref()
                    .is_none_or(|n| pin_role(n) != PinRole::NoConnect)
            })
            .map(|p| p.number.as_str())
            .collect();
        if !free.is_empty() {
            let mut list: Vec<&str> = free.into_iter().collect();
            list.sort_by(|a, b| natural(a, b));
            v.push(
                Violation::warning(
                    "pad_no_net",
                    format!("{refdes} has pads in no net: {}", list.join(", ")),
                )
                .items([refdes.clone()]),
            );
        }
    }
}

fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let num = |s: &str| {
        s.trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse::<u64>()
            .ok()
    };
    num(a).cmp(&num(b)).then(a.cmp(b))
}

fn current(d: &ProjectData, v: &mut Vec<Violation>) {
    let stack = &d.board.stackup;
    for t in &d.routing.tracks {
        let Some(amps) = d.nets.nets.get(&t.net).and_then(|n| n.current) else {
            continue;
        };
        if amps <= 0.0 || t.id.starts_with("td") {
            continue;
        }
        let outer = matches!(t.layer, Layer::Copper(Copper::Top | Copper::Bottom));
        let oz = if outer {
            stack.outer_copper_oz
        } else {
            stack.inner_copper_oz
        };
        let need = width_for_current(amps, d.board.rules.max_temp_rise, oz, outer);
        if t.width + 1e-6 < need {
            // Round up, so that the fix is sufficient.
            let width = (need * 100.0).ceil() / 100.0;
            v.push(
                Violation::warning(
                    "track_current",
                    format!(
                        "{} is {} mm wide, but {} A in the net {} needs {} mm for a temperature rise of {} °C",
                        t.id,
                        r3(t.width),
                        r3(amps),
                        t.net,
                        r3(need),
                        r3(d.board.rules.max_temp_rise)
                    ),
                )
                .on(t.layer)
                .items([t.id.clone()])
                .measured(t.width, need)
                .fix(Fix::SetTrackWidth {
                    id: t.id.clone(),
                    width,
                }),
            );
        }
    }
}

fn voltage(d: &ProjectData, g: &BoardGeometry, v: &mut Vec<Violation>) {
    if !d.nets.nets.values().any(|n| n.voltage.is_some()) {
        return;
    }
    let last = g.layers.saturating_sub(1);
    let surface = |layer: u8, pad: bool| {
        if layer != 0 && layer != last {
            Surface::Internal
        } else if pad {
            Surface::ExternalBare
        } else {
            Surface::ExternalCoated
        }
    };
    let boxes: Vec<Rect> = g.copper.iter().map(|c| c.prim.bbox()).collect();
    let index = Index::new(boxes.iter().copied());
    let reach = d
        .nets
        .nets
        .values()
        .filter_map(|n| n.voltage)
        .fold(0.0_f64, |m, x| m.max(x.abs()));
    let reach = voltage_spacing(2.0 * reach, Surface::ExternalBare);
    // One violation for each pair of items.
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for (i, a) in g.copper.iter().enumerate() {
        let Some(va) = a.net.as_deref().and_then(|n| volts(d, n)) else {
            continue;
        };
        if matches!(a.owner, Owner::Zone { .. }) {
            continue;
        }
        for j in index.near(&boxes[i], reach) {
            if j <= i {
                continue;
            }
            let b = &g.copper[j];
            if b.layer != a.layer || b.net == a.net || matches!(b.owner, Owner::Zone { .. }) {
                continue;
            }
            let Some(vb) = b.net.as_deref().and_then(|n| volts(d, n)) else {
                continue;
            };
            let dv = (va - vb).abs();
            let pad = matches!(a.owner, Owner::Pad { .. }) || matches!(b.owner, Owner::Pad { .. });
            let need = voltage_spacing(dv, surface(a.layer, pad));
            let dist = a.prim.distance(&b.prim);
            if dist + 1e-6 >= need {
                continue;
            }
            let mut pair = [a.owner.label(), b.owner.label()];
            pair.sort();
            if !seen.insert((pair[0].clone(), pair[1].clone())) {
                continue;
            }
            v.push(
                Violation::error(
                    "voltage_clearance",
                    format!(
                        "{} ({}) and {} ({}) are {} mm apart, but {} V between them needs {} mm",
                        pair[0],
                        a.net.as_deref().unwrap_or(""),
                        pair[1],
                        b.net.as_deref().unwrap_or(""),
                        r3(dist),
                        r3(dv),
                        r3(need)
                    ),
                )
                .on(Layer::Copper(Copper::from_index(a.layer, g.layers)))
                .items(pair)
                .measured(dist, need),
            );
        }
    }
    // A pour keeps only its own clearance. Report a pour that keeps less
    // than the voltage needs from a net that has copper on its layers.
    let mut on_layer: BTreeMap<u8, BTreeSet<&str>> = BTreeMap::new();
    for c in &g.copper {
        if let Some(n) = c.net.as_deref() {
            on_layer.entry(c.layer).or_default().insert(n);
        }
    }
    for z in &d.routing.zones {
        let Some(zn) = z.net.as_deref() else { continue };
        let Some(vz) = volts(d, zn) else { continue };
        let class = d.nets.nets.get(zn).and_then(|n| n.class.as_deref());
        let own = z.clearance.unwrap_or(d.board.rules.clearance_for(class));
        for layer in &z.layers {
            let Some(copper) = layer.copper() else {
                continue;
            };
            let li = copper.index(g.layers);
            for other in on_layer.get(&li).into_iter().flatten() {
                if *other == zn {
                    continue;
                }
                let Some(vo) = volts(d, other) else { continue };
                let oclass = d.nets.nets.get(*other).and_then(|n| n.class.as_deref());
                let keeps = own.max(d.board.rules.clearance_for(oclass));
                let need = voltage_spacing((vz - vo).abs(), surface(li, true));
                if keeps + 1e-6 < need {
                    v.push(
                        Violation::error(
                            "voltage_clearance",
                            format!(
                                "The pour {} ({zn}) keeps {} mm from the net {other}, but {} V between them needs {} mm. Set the clearance of the zone",
                                z.id,
                                r3(keeps),
                                r3((vz - vo).abs()),
                                r3(need)
                            ),
                        )
                        .on(*layer)
                        .items([z.id.clone(), other.to_string()])
                        .measured(keeps, need),
                    );
                }
            }
        }
    }
}

fn decoupling(d: &ProjectData, v: &mut Vec<Violation>) {
    let nets = d.nets.pad_map();
    // The capacitor pads of each net.
    let mut caps: BTreeMap<&str, Vec<(Point, Side)>> = BTreeMap::new();
    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        let letters: String = refdes
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if letters != "C" || fp.pads.len() != 2 || part.dnp {
            continue;
        }
        for pad in &fp.pads {
            if let Some(n) = nets.get(&PadRef::new(refdes.clone(), pad.number.clone())) {
                caps.entry(n.as_str())
                    .or_default()
                    .push((pl.transform(pad.at), pl.side));
            }
        }
    }
    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        if fp.pads.len() < 4 {
            continue;
        }
        // The smallest distance between two pads.
        let mut pitch = f64::INFINITY;
        for (i, a) in fp.pads.iter().enumerate() {
            for b in &fp.pads[i + 1..] {
                let dist = a.at.dist(b.at);
                if dist > 1e-6 {
                    pitch = pitch.min(dist);
                }
            }
        }
        let reach = d.board.rules.decoupling_max_distance.max(2.0 * pitch);
        let mut far: BTreeMap<&str, (f64, &str)> = BTreeMap::new();
        for pad in &fp.pads {
            let Some(name) = pad.name.as_deref() else {
                continue;
            };
            if pin_role(name) != PinRole::Power {
                continue;
            }
            let Some(net) = nets.get(&PadRef::new(refdes.clone(), pad.number.clone())) else {
                continue;
            };
            let at = pl.transform(pad.at);
            let nearest = caps
                .get(net.as_str())
                .into_iter()
                .flatten()
                .map(|(c, _)| c.dist(at))
                .fold(f64::INFINITY, f64::min);
            if nearest > reach + 1e-6 {
                let e = far.entry(net.as_str()).or_insert((nearest, &pad.number));
                if nearest > e.0 {
                    *e = (nearest, &pad.number);
                }
            }
        }
        for (net, (dist, pad)) in far {
            let text = if dist.is_finite() {
                format!(
                    "The power pin {refdes}.{pad} ({net}) has its nearest capacitor {} mm away, but the maximum is {} mm",
                    r3(dist),
                    r3(reach)
                )
            } else {
                format!("The power pin {refdes}.{pad} ({net}) has no capacitor on its net")
            };
            let mut hit = Violation::warning("decoupling_far", text)
                .at(Point::new(pl.x, pl.y))
                .items([refdes.clone(), net.to_string()]);
            if dist.is_finite() {
                hit = hit.measured(dist, reach);
            }
            v.push(hit);
        }
    }
}

fn return_path(d: &ProjectData, fills: &[ZoneFill], v: &mut Vec<Violation>) {
    if fills.is_empty() {
        return;
    }
    let layers = d.board.stackup.copper_layers;
    let rules = &d.board.rules;
    let mut fast: BTreeSet<&str> = BTreeSet::new();
    for p in d.nets.diff_pairs.values() {
        fast.insert(&p.positive);
        fast.insert(&p.negative);
    }
    for (name, net) in &d.nets.nets {
        let class = net.class.as_deref().and_then(|c| rules.net_classes.get(c));
        if net.length_min.is_some()
            || net.length_max.is_some()
            || net.max_skew.is_some()
            || class.is_some_and(|c| c.reference_plane)
        {
            fast.insert(name);
        }
    }
    if fast.is_empty() {
        return;
    }
    let covered = |layer: u8, p: Point| {
        fills.iter().any(|f| {
            f.layer.copper().map(|c| c.index(layers)) == Some(layer)
                && f.polygons.iter().any(|poly| {
                    ring_contains(&poly.outer, p) && !poly.holes.iter().any(|h| ring_contains(h, p))
                })
        })
    };
    let has_fill = |layer: u8| {
        fills.iter().any(|f| {
            !f.polygons.is_empty() && f.layer.copper().map(|c| c.index(layers)) == Some(layer)
        })
    };
    const STEP: f64 = 0.25;
    // The plane has an opening at a pad or a via, so the ends do not count.
    const END: f64 = 1.0;
    const GAP: f64 = 0.5;
    for t in &d.routing.tracks {
        if !fast.contains(t.net.as_str()) || t.id.starts_with("td") {
            continue;
        }
        let Some(own) = t.layer.copper().map(|c| c.index(layers)) else {
            continue;
        };
        let near: Vec<u8> = [own.checked_sub(1), (own + 1 < layers).then_some(own + 1)]
            .into_iter()
            .flatten()
            .filter(|l| has_fill(*l))
            .collect();
        if near.is_empty() {
            continue;
        }
        let total: f64 = t.points.windows(2).map(|w| w[0].dist(w[1])).sum();
        if total <= 2.0 * END + GAP {
            continue;
        }
        // The sample points along the track, with the distance from the start.
        let mut samples: Vec<Point> = Vec::new();
        let mut done = 0.0;
        for w in t.points.windows(2) {
            let len = w[0].dist(w[1]);
            if len < 1e-9 {
                continue;
            }
            let mut s = (done / STEP).ceil() * STEP;
            while s <= done + len {
                if s >= END && s <= total - END {
                    let k = (s - done) / len;
                    samples.push(Point::new(
                        w[0].x + (w[1].x - w[0].x) * k,
                        w[0].y + (w[1].y - w[0].y) * k,
                    ));
                }
                s += STEP;
            }
            done += len;
        }
        // The track is good if one near layer has copper below all of it.
        let mut worst: Option<(Point, f64)> = None;
        let mut good = false;
        for layer in &near {
            let mut run = 0usize;
            let mut longest: Option<(Point, usize)> = None;
            for (i, p) in samples.iter().enumerate() {
                if covered(*layer, *p) {
                    run = 0;
                } else {
                    run += 1;
                    if longest.is_none_or(|l| run > l.1) {
                        // The middle of the run.
                        longest = Some((samples[i - run / 2], run));
                    }
                }
            }
            match longest {
                Some((p, n)) if n as f64 * STEP >= GAP => {
                    let len = n as f64 * STEP;
                    if worst.is_none_or(|w| len < w.1) {
                        worst = Some((p, len));
                    }
                }
                _ => {
                    good = true;
                    break;
                }
            }
        }
        if good {
            continue;
        }
        if let Some((at, len)) = worst {
            v.push(
                Violation::warning(
                    "return_path",
                    format!(
                        "{} ({}) goes for {} mm over a gap in the pour of the next layer. The return current has no path below the track",
                        t.id,
                        t.net,
                        r3(len)
                    ),
                )
                .at(at)
                .on(t.layer)
                .items([t.id.clone()]),
            );
        }
    }
}

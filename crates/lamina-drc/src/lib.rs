//! Design rule checks, connectivity and fab presets.

pub mod fab;
pub mod footprints;
mod holes;
mod lengths;
pub mod model;
mod pairs;
mod parity;
pub mod presets;
pub mod registry;
mod silk;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use lamina_geom::fill::ZoneFill;
use lamina_geom::prim::{Prim, ring_contains};
use lamina_geom::{BoardGeometry, CopperItem, Index, Owner};
use lamina_model::*;
use serde::Serialize;

pub use registry::{CHECKS, CheckInfo, Family, RULE_NAMES, release_view};

/// A missing connection, drawn as a ratsnest line.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Ratsnest {
    pub net: String,
    pub a: Point,
    pub b: Point,
    pub from: String,
    pub to: String,
}

pub struct Analysis {
    pub violations: Vec<Violation>,
    pub excluded: Vec<Violation>,
    /// Violations of rules that are off in design and on for release.
    pub release_only: Vec<Violation>,
    pub ratsnest: Vec<Ratsnest>,
    pub fills: Vec<ZoneFill>,
    pub geometry: BoardGeometry,
    /// The island of connected copper for each `geometry.copper` item.
    pub island: Vec<usize>,
    /// The island of each fill polygon, for each fill. It is empty for layers that are not copper.
    pub fill_island: Vec<Vec<usize>>,
}

struct Dsu(Vec<usize>);

impl Dsu {
    fn new(n: usize) -> Dsu {
        Dsu((0..n).collect())
    }
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut c = x;
        while self.0[c] != r {
            let n = self.0[c];
            self.0[c] = r;
            c = n;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn mid(a: &Prim, b: &Prim) -> Point {
    let p = a.anchor();
    let q = b.anchor();
    Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0)
}

/// Calculate the fills for the board. The result is empty if the board has no zones.
pub fn fills(d: &ProjectData, g: &BoardGeometry) -> Vec<ZoneFill> {
    if d.routing.zones.is_empty() {
        return Vec::new();
    }
    lamina_geom::fill::fill_zones(d, g)
}

/// Run every check.
pub fn analyze(d: &ProjectData) -> Analysis {
    let mut g = lamina_geom::build(d);
    let fills = fills(d, &g);
    let layers = g.layers;
    let rules = &d.board.rules;
    let class_of = |net: &Option<String>| -> Option<String> {
        net.as_ref()
            .and_then(|n| d.nets.nets.get(n))
            .and_then(|n| n.class.clone())
    };
    let mut v: Vec<Violation> = model::validate(d);
    v.extend(lengths::check(d));
    v.extend(pairs::check(d));
    for fill in &fills {
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

    // Pour fills are part of the connectivity check. They are not part of the
    // clearance check, because each fill already keeps the clearance.
    let n_items = g.copper.len();
    for f in &fills {
        let Some(c) = f.layer.copper() else { continue };
        for (k, p) in f.polygons.iter().enumerate() {
            g.copper.push(CopperItem {
                layer: c.index(layers),
                net: f.net.clone(),
                prim: p.prim(),
                owner: Owner::Zone {
                    id: f.zone.clone(),
                    piece: k,
                },
            });
        }
    }
    let items = &g.copper;
    let fill_index: Vec<lamina_geom::prim::IndexedPoly> = items[n_items..]
        .iter()
        .map(|it| match &it.prim {
            Prim::Poly { outer, holes } => lamina_geom::prim::IndexedPoly::new(outer, holes),
            other => {
                let b = other.bbox();
                let r = [
                    b.min,
                    Point::new(b.max.x, b.min.y),
                    b.max,
                    Point::new(b.min.x, b.max.y),
                ];
                lamina_geom::prim::IndexedPoly::new(&r, &[])
            }
        })
        .collect();
    let boxes: Vec<Rect> = items.iter().map(|i| i.prim.bbox()).collect();
    let index = Index::new(boxes.iter().copied());
    let max_cl = rules
        .net_classes
        .values()
        .filter_map(|c| c.clearance)
        .chain(rules.custom.iter().filter_map(|r| r.clearance))
        .fold(rules.clearance, f64::max);

    // Clearance and shorts between copper of different nets.
    let mut seen_pairs: BTreeSet<(String, String)> = BTreeSet::new();
    let mut dsu = Dsu::new(items.len());
    // The same connections without the joins that only touch.
    let mut strong = Dsu::new(items.len());
    let mut weak: Vec<(usize, usize)> = Vec::new();
    let mut by_body: HashMap<String, usize> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Some(&j) = by_body.get(&it.owner.body()) {
            dsu.union(i, j);
            strong.union(i, j);
        } else {
            by_body.insert(it.owner.body(), i);
        }
    }
    // Pads of one part with the same number are one terminal. The part connects
    // them internally (for example, the paired legs of a switch or the shield
    // tabs of a connector).
    let mut by_pad: HashMap<(&str, &str), usize> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Owner::Pad { part, pad, .. } = &it.owner {
            if pad.is_empty() {
                continue;
            }
            match by_pad.get(&(part.as_str(), pad.as_str())) {
                Some(&j) => {
                    dsu.union(i, j);
                    strong.union(i, j);
                }
                None => {
                    by_pad.insert((part.as_str(), pad.as_str()), i);
                }
            }
        }
    }
    for (i, a) in items.iter().enumerate() {
        for j in index.near(&boxes[i], max_cl) {
            if j <= i {
                continue;
            }
            let b = &items[j];
            if a.layer != b.layer || a.owner.body() == b.owner.body() {
                continue;
            }
            // Two drawings with no net can touch: a logo, or the copper of a
            // board that came from Gerber files.
            if a.owner.is_art() && b.owner.is_art() {
                continue;
            }
            let both_fill = i >= n_items && j >= n_items;
            let same_net = a.net.is_some() && a.net == b.net;
            // A pour fill keeps clear of other nets, so a pour piece is important
            // only for the connectivity of its own net. Skip the other pairs. This
            // prevents an exact distance to a polygon of thousands of edges for each pair.
            if (i >= n_items || j >= n_items) && !same_net {
                continue;
            }
            // For a pour piece and a pad or track of its net, use an indexed touch
            // test. Do not calculate the exact distance to the full polygon.
            let dist = match (i >= n_items, j >= n_items) {
                (true, false) | (false, true) if same_net => {
                    let (f, other) = if i >= n_items { (i, b) } else { (j, a) };
                    if fill_index[f - n_items].touches(&other.prim, 1e-6) {
                        0.0
                    } else {
                        1.0
                    }
                }
                (true, true) => {
                    // Pieces of one pour are always separate. Pieces of two pours
                    // of the same net can overlap.
                    let same_zone = matches!(
                        (&a.owner, &b.owner),
                        (Owner::Zone { id: x, .. }, Owner::Zone { id: y, .. }) if x == y
                    );
                    if !same_zone
                        && fill_index[i - n_items].touches_poly(&fill_index[j - n_items], 1e-6)
                    {
                        0.0
                    } else {
                        1.0
                    }
                }
                _ => a.prim.distance(&b.prim),
            };
            if dist <= 1e-6 {
                dsu.union(i, j);
                let plain = i < n_items && j < n_items;
                if plain && same_net && parity::is_weak(&a.prim, &b.prim, rules.min_connection) {
                    weak.push((i, j));
                } else {
                    strong.union(i, j);
                }
            }
            if same_net || both_fill {
                continue;
            }
            // If two pieces of one part have no net (for example, two pads that
            // touch in the footprint drawing), it is a footprint problem, not a short.
            let same_part = matches!((&a.owner, &b.owner), (Owner::Pad { part: p, .. }, Owner::Pad { part: q, .. }) if p == q);
            let a_fill = i >= n_items;
            let b_fill = j >= n_items;
            if a_fill || b_fill {
                continue;
            }
            let layer = Layer::Copper(Copper::from_index(a.layer, layers));
            let need = rules
                .resolve(a.net.as_deref(), class_of(&a.net).as_deref(), Some(layer))
                .clearance
                .max(
                    rules
                        .resolve(b.net.as_deref(), class_of(&b.net).as_deref(), Some(layer))
                        .clearance,
                );
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
    }

    // Shorts through connected copper (for example, a track that connects two nets).
    let mut comp_nets: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Some(n) = &it.net {
            comp_nets.entry(dsu.find(i)).or_default().insert(n.clone());
        }
    }
    for (root, nets) in &comp_nets {
        if nets.len() > 1 {
            let at = items
                .iter()
                .enumerate()
                .find(|(i, _)| dsu.find(*i) == *root)
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

    // Unrouted connections and ratsnest.
    let mut ratsnest = Vec::new();
    for (name, net) in &d.nets.nets {
        // Islands of this net that contain at least one pad.
        let mut islands: BTreeMap<usize, Vec<(String, Point)>> = BTreeMap::new();
        for pr in &net.pads {
            let found = items.iter().enumerate().find(|(_, it)| {
                matches!(&it.owner, Owner::Pad { part, pad, .. } if *part == pr.part && *pad == pr.pad)
            });
            if let Some((i, it)) = found {
                islands
                    .entry(dsu.find(i))
                    .or_default()
                    .push((pr.to_string(), it.prim.anchor()));
            }
        }
        if islands.len() < 2 {
            continue;
        }
        // Prim's MST of the islands. Each edge is the nearest pad pair between two islands.
        let groups: Vec<Vec<(String, Point)>> = islands.into_values().collect();
        let k = groups.len();
        let mut in_tree = vec![false; k];
        in_tree[0] = true;
        for _ in 1..k {
            let mut best: Option<(f64, usize, usize, usize, usize)> = None;
            for a in (0..k).filter(|&a| in_tree[a]) {
                for b in (0..k).filter(|&b| !in_tree[b]) {
                    for (ia, pa) in groups[a].iter().enumerate() {
                        for (ib, pb) in groups[b].iter().enumerate() {
                            let dd = pa.1.dist(pb.1);
                            if best.is_none_or(|x| dd < x.0) {
                                best = Some((dd, a, ia, b, ib));
                            }
                        }
                    }
                }
            }
            let (_, a, ia, b, ib) = best.unwrap();
            in_tree[b] = true;
            ratsnest.push(Ratsnest {
                net: name.clone(),
                a: groups[a][ia].1,
                b: groups[b][ib].1,
                from: groups[a][ia].0.clone(),
                to: groups[b][ib].0.clone(),
            });
        }
        v.push(
            Violation::warning(
                "unrouted",
                format!("The net {name} has {} unrouted connection(s)", k - 1),
            )
            .items([name.clone()]),
        );
    }

    // Track widths.
    for t in &d.routing.tracks {
        let limits = rules.resolve(
            Some(&t.net),
            d.nets.nets.get(&t.net).and_then(|n| n.class.as_deref()),
            Some(t.layer),
        );
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
                .on(t.layer),
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
            let limits = rules.resolve(
                Some(&via.net),
                class,
                Some(Layer::Copper(Copper::from_index(index, layers))),
            );
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
                .items([via.id.clone()]),
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
                            rules
                                .resolve(
                                    net,
                                    class,
                                    Some(Layer::Copper(Copper::from_index(index, layers))),
                                )
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

    // Board edge.
    if let Some(outline) = &g.outline {
        let mut edges: Vec<Prim> = Vec::new();
        let mut push_ring = |r: &Vec<Point>| {
            for k in 0..r.len() {
                edges.push(Prim::Capsule {
                    a: r[k],
                    b: r[(k + 1) % r.len()],
                    r: 0.0,
                });
            }
        };
        push_ring(outline);
        for c in &g.cutouts {
            push_ring(c);
        }
        let eboxes: Vec<Rect> = edges.iter().map(|e| e.bbox()).collect();
        let eidx = Index::new(eboxes.iter().copied());
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for it in items.iter().take(n_items) {
            let label = it.owner.label();
            if reported.contains(&label) {
                continue;
            }
            let p = it.prim.anchor();
            let inside =
                ring_contains(outline, p) && !g.cutouts.iter().any(|c| ring_contains(c, p));
            if !inside {
                reported.insert(label.clone());
                v.push(
                    Violation::error(
                        "copper_outside_board",
                        format!("{label} is outside the board"),
                    )
                    .at(p)
                    .items([label]),
                );
                continue;
            }
            let mut near = f64::INFINITY;
            for k in eidx.near(&it.prim.bbox(), rules.copper_to_edge) {
                near = near.min(it.prim.distance(&edges[k]));
            }
            if near + 1e-6 < rules.copper_to_edge {
                reported.insert(label.clone());
                v.push(
                    Violation::error(
                        "copper_to_edge",
                        format!(
                            "{label} is {} mm from the board edge, but the minimum is {} mm",
                            r3(near),
                            r3(rules.copper_to_edge)
                        ),
                    )
                    .at(p)
                    .items([label]),
                );
            }
        }
    }

    // Courtyards and keep-outs.
    let mut cys: Vec<(String, Side, Vec<Point>)> = Vec::new();
    let mut kos: Vec<(String, Vec<u8>, Vec<Point>, Forbid)> = Vec::new();
    for (refdes, pl) in &d.placement.placements {
        let Some(fp) = d
            .parts
            .parts
            .get(refdes)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            continue;
        };
        if let Some(c) = lamina_geom::courtyard(fp, pl) {
            cys.push((refdes.clone(), pl.side, c));
        }
        for (ls, pts, f) in lamina_geom::part_keepouts(fp, pl, layers) {
            kos.push((refdes.clone(), ls, pts, f));
        }
    }
    for k in &d.routing.keepouts {
        let ls: Vec<u8> = k
            .layers
            .iter()
            .filter_map(|l| l.copper().map(|c| c.index(layers)))
            .collect();
        kos.push((k.id.clone(), ls, k.outline.flatten(0.02), k.forbid));
    }
    for hole in g.drills.iter().filter(|hole| !hole.plated) {
        for (part, side, courtyard) in &cys {
            if matches!(&hole.owner, Owner::Pad { part: own, .. } if own == part) {
                continue;
            }
            let polygon = Prim::Poly {
                outer: courtyard.clone(),
                holes: vec![],
            };
            if polygon.distance(&hole.prim) <= 1e-6 {
                v.push(
                    Violation::warning(
                        "npth_inside_courtyard",
                        format!(
                            "The non-plated hole {} is inside the courtyard of {part}",
                            hole.owner.label()
                        ),
                    )
                    .at(hole.prim.anchor())
                    .on(Layer::Courtyard(*side))
                    .items([hole.owner.label(), part.clone()]),
                );
            }
        }
    }
    for i in 0..cys.len() {
        for j in (i + 1)..cys.len() {
            if cys[i].1 != cys[j].1 {
                continue;
            }
            if polys_overlap(&cys[i].2, &cys[j].2) {
                let pair = [cys[i].0.clone(), cys[j].0.clone()];
                v.push(
                    Violation::error(
                        "courtyard_overlap",
                        format!("The courtyards of {} and {} overlap", pair[0], pair[1]),
                    )
                    .at(centroid(&cys[i].2))
                    .items(pair),
                );
            }
        }
    }
    if let Some(outline) = &g.outline {
        for (r, _, c) in &cys {
            if c.iter().any(|p| !ring_contains(outline, *p)) {
                v.push(
                    Violation::warning(
                        "courtyard_outside_board",
                        format!("The courtyard of {r} is partly outside the board"),
                    )
                    .at(centroid(c))
                    .items([r.clone()]),
                );
            }
        }
    }
    for (kid, ls, pts, forbid) in &kos {
        let kp = Prim::Poly {
            outer: pts.clone(),
            holes: vec![],
        };
        let kb = kp.bbox();
        let mut hits: BTreeSet<String> = BTreeSet::new();
        for j in index.near(&kb, 0.0) {
            let it = &items[j];
            if !ls.contains(&it.layer) {
                continue;
            }
            let forbidden = match &it.owner {
                Owner::Track { .. } | Owner::Text { .. } | Owner::Graphic { .. } => forbid.tracks,
                Owner::Via { .. } => forbid.vias,
                Owner::Zone { .. } => false,
                Owner::Pad { part, .. } => forbid.pads && part != kid,
                Owner::Hole { .. } => forbid.pads,
            };
            if forbidden && it.prim.distance(&kp) <= 1e-6 {
                // An item can touch the edge, but it must not overlap it.
                let shrunk = shrink(&kp, 1e-3);
                if it.prim.distance(&shrunk) <= 1e-6 {
                    hits.insert(it.owner.label());
                }
            }
        }
        if forbid.parts {
            for (r, _, c) in &cys {
                if r != kid && polys_overlap(c, pts) {
                    hits.insert(r.clone());
                }
            }
        }
        for h in hits {
            v.push(
                Violation::error("keepout", format!("{h} is inside the keep-out {kid}"))
                    .at(centroid(pts))
                    .items([h, kid.clone()]),
            );
        }
    }

    v.extend(silk::check(d, &g, &fills));
    v.extend(holes::hole_to_copper(d, &g));
    v.extend(parity::check(d, &g));
    v.extend(footprints::check(d));
    // A join that only touches is a violation if nothing else connects the two sides.
    for (i, j) in weak {
        if strong.find(i) != strong.find(j) {
            let (a, b) = (&items[i], &items[j]);
            v.push(parity::weak_violation(
                &a.owner,
                &b.owner,
                mid(&a.prim, &b.prim),
                Layer::Copper(Copper::from_index(a.layer, layers)),
                rules.min_connection,
            ));
            strong.union(i, j);
        }
    }
    let roots: Vec<usize> = (0..items.len()).map(|i| dsu.find(i)).collect();
    v.extend(dangling(d, items, n_items, &index, &fill_index, &roots));

    for r in d.parts.parts.keys() {
        if !d.placement.placements.contains_key(r) {
            v.push(Violation::warning("unplaced", format!("{r} is not placed")).items([r.clone()]));
        }
    }

    let island: Vec<usize> = (0..n_items).map(|i| dsu.find(i)).collect();
    let mut k = n_items;
    let fill_island = fills
        .iter()
        .map(|f| {
            if f.layer.copper().is_none() {
                return Vec::new();
            }
            let v: Vec<usize> = (k..k + f.polygons.len()).map(|i| dsu.find(i)).collect();
            k += f.polygons.len();
            v
        })
        .collect();
    g.copper.truncate(n_items);
    let mut excluded = Vec::new();
    let mut matched = vec![false; rules.exclusions.len()];
    v.retain(|violation| {
        let mut items = violation.items.clone();
        items.sort();
        let mut found = false;
        for (i, entry) in rules.exclusions.iter().enumerate() {
            let mut expected = entry.items.clone();
            expected.sort();
            if entry.rule == violation.rule && expected == items {
                matched[i] = true;
                found = true;
            }
        }
        if found {
            excluded.push(violation.clone());
        }
        !found
    });
    for (entry, matched) in rules.exclusions.iter().zip(matched) {
        if !matched {
            v.push(
                Violation::warning(
                    "stale_exclusion",
                    format!(
                        "The exclusion for {} matches no violation: {}",
                        entry.rule, entry.reason
                    ),
                )
                .items(entry.items.clone()),
            );
        }
    }
    let mut release_only = Vec::new();
    v.retain_mut(|violation| {
        match rules.severities.get(&violation.rule) {
            Some(RuleSeverity::Ignore) => return false,
            Some(RuleSeverity::Error) => violation.severity = Severity::Error,
            Some(RuleSeverity::Warning) => violation.severity = Severity::Warning,
            None => {
                // A rule that is off by default. It can be on for release.
                if let Some(c) = registry::get(&violation.rule) {
                    if c.default == RuleSeverity::Ignore {
                        match c.release {
                            Some(RuleSeverity::Error) => {
                                violation.severity = Severity::Error;
                                release_only.push(violation.clone());
                            }
                            Some(RuleSeverity::Warning) => {
                                violation.severity = Severity::Warning;
                                release_only.push(violation.clone());
                            }
                            _ => {}
                        }
                        return false;
                    }
                }
            }
        }
        true
    });
    Analysis {
        violations: v,
        excluded,
        release_only,
        ratsnest,
        fills,
        geometry: g,
        island,
        fill_island,
    }
}

/// Warn about copper that connects to nothing useful.
fn dangling(
    d: &ProjectData,
    items: &[CopperItem],
    n_items: usize,
    index: &Index,
    fill_index: &[lamina_geom::prim::IndexedPoly],
    roots: &[usize],
) -> Vec<Violation> {
    let layers = d.board.stackup.copper_layers;
    let with_pad: BTreeSet<usize> = items
        .iter()
        .zip(roots)
        .filter(|(it, _)| matches!(it.owner, Owner::Pad { .. }))
        .map(|(_, r)| *r)
        .collect();
    let mut out = Vec::new();
    let mut vias: BTreeSet<&str> = BTreeSet::new();
    for (i, it) in items.iter().enumerate() {
        let free = !with_pad.contains(&roots[i]);
        match &it.owner {
            Owner::Via { id } if free && vias.insert(id) => out.push(
                Violation::warning(
                    "via_dangling",
                    format!("The via {id} does not connect to a pad"),
                )
                .at(it.prim.anchor())
                .items([id.clone()]),
            ),
            Owner::Zone { id, piece } if free && it.net.is_some() => out.push(
                Violation::warning(
                    "isolated_copper",
                    format!("Piece {piece} of the zone {id} does not connect to a pad"),
                )
                .at(it.prim.anchor())
                .items([id.clone()]),
            ),
            _ => {}
        }
    }
    for t in &d.routing.tracks {
        let Some(layer) = t.layer.copper().map(|c| c.index(layers)) else {
            continue;
        };
        let (Some(first), Some(last)) = (t.points.first(), t.points.last()) else {
            continue;
        };
        if t.points.len() < 2 {
            continue;
        }
        for end in [*first, *last] {
            let tip = Prim::Circle {
                c: end,
                r: t.width / 2.0,
            };
            let joined = index.near(&tip.bbox(), 0.0).into_iter().any(|j| {
                let o = &items[j];
                let own = matches!(&o.owner, Owner::Track { id, .. } if *id == t.id);
                if own || o.layer != layer || o.net.as_deref() != Some(t.net.as_str()) {
                    return false;
                }
                if j >= n_items {
                    fill_index[j - n_items].touches(&tip, 1e-6)
                } else {
                    tip.distance(&o.prim) <= 1e-6
                }
            });
            if !joined {
                out.push(
                    Violation::warning(
                        "track_dangling",
                        format!("An end of {} ({}) connects to nothing", t.id, t.net),
                    )
                    .at(end)
                    .on(t.layer)
                    .items([t.id.clone()]),
                );
            }
        }
    }
    out
}

/// The checker for `Project::add_checker`.
pub fn check(d: &ProjectData) -> Vec<Violation> {
    analyze(d).violations
}

fn centroid(p: &[Point]) -> Point {
    let n = p.len().max(1) as f64;
    Point::new(
        p.iter().map(|q| q.x).sum::<f64>() / n,
        p.iter().map(|q| q.y).sum::<f64>() / n,
    )
}

fn shrink(p: &Prim, d: f64) -> Prim {
    match p {
        Prim::Poly { outer, holes } => {
            let c = centroid(outer);
            let scale = |q: &Point| {
                let v = q.sub(c);
                let len = (v.x * v.x + v.y * v.y).sqrt().max(1e-9);
                let k = ((len - d) / len).max(0.0);
                Point::new(c.x + v.x * k, c.y + v.y * k)
            };
            Prim::Poly {
                outer: outer.iter().map(scale).collect(),
                holes: holes.clone(),
            }
        }
        other => other.clone(),
    }
}

/// Return true if the polygons overlap. Edges that only touch do not count.
pub fn polys_overlap(a: &[Point], b: &[Point]) -> bool {
    let pa = shrink(
        &Prim::Poly {
            outer: a.to_vec(),
            holes: vec![],
        },
        1e-3,
    );
    let pb = Prim::Poly {
        outer: b.to_vec(),
        holes: vec![],
    };
    pa.distance(&pb) <= 1e-9
}

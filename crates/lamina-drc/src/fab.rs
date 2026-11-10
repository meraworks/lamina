//! Fab profiles as data, and the smallest features of a board to compare
//! with them.

use std::collections::BTreeMap;

use lamina_geom::{BoardGeometry, Index, Owner, Prim};
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The limits of a fab for a range of layer counts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FabTier {
    pub id: String,
    /// The smallest and the largest number of copper layers.
    pub layers: (u8, u8),
    /// The limit for each measurement of `board_minimums`. A name that ends
    /// in `_max` is a maximum. All other names are minimums.
    pub limits: BTreeMap<String, f64>,
}

/// What a fab can make, from its capability page.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FabProfile {
    pub id: String,
    pub name: String,
    /// The page that the numbers come from.
    pub source: String,
    /// The date when someone compared the numbers with the page.
    pub checked: String,
    #[serde(default)]
    pub note: String,
    pub tiers: Vec<FabTier>,
}

/// The profiles that are in the program.
pub fn profiles() -> Vec<FabProfile> {
    [
        include_str!("profiles/jlcpcb.json"),
        include_str!("profiles/pcbway.json"),
    ]
    .iter()
    .map(|s| serde_json::from_str(s).expect("a profile in the program is valid"))
    .collect()
}

/// One smallest (or largest) feature of the board.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Minimum {
    /// The name of the measurement, for example `clearance`.
    pub what: String,
    pub value: f64,
    /// The items that make this value.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
}

/// A limit of a profile against the board.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LimitResult {
    pub what: String,
    pub value: f64,
    pub limit: f64,
    /// Positive: the board is inside the limit by this much.
    pub margin: f64,
    pub fits: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FabResult {
    pub profile: String,
    pub name: String,
    pub tier: String,
    pub fits: bool,
    /// The limits, the worst first.
    pub limits: Vec<LimitResult>,
    pub source: String,
    pub checked: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

fn r4(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

struct Min {
    out: BTreeMap<&'static str, Minimum>,
}

impl Min {
    fn low(&mut self, what: &'static str, value: f64, items: Vec<String>, at: Option<Point>) {
        self.put(what, value, items, at, false)
    }
    fn high(&mut self, what: &'static str, value: f64, items: Vec<String>, at: Option<Point>) {
        self.put(what, value, items, at, true)
    }
    fn put(
        &mut self,
        what: &'static str,
        value: f64,
        items: Vec<String>,
        at: Option<Point>,
        high: bool,
    ) {
        let better = match self.out.get(what) {
            None => true,
            Some(m) if high => value > m.value,
            Some(m) => value < m.value,
        };
        if better && value.is_finite() {
            self.out.insert(
                what,
                Minimum {
                    what: what.into(),
                    value: r4(value),
                    items,
                    at,
                },
            );
        }
    }
}

/// The smallest features of the board. Distances above `reach` are not
/// measured: a board with no value for a name is above `reach` there.
pub fn minimums(d: &ProjectData, g: &BoardGeometry, reach: f64) -> Vec<Minimum> {
    let mut m = Min {
        out: BTreeMap::new(),
    };
    for t in &d.routing.tracks {
        m.low(
            "track_width",
            t.width,
            vec![t.id.clone()],
            t.points.first().copied(),
        );
    }
    for v in &d.routing.vias {
        m.low("via_drill", v.drill, vec![v.id.clone()], Some(v.at));
        m.low("via_diameter", v.diameter, vec![v.id.clone()], Some(v.at));
        m.low(
            "via_ring",
            (v.diameter - v.drill) / 2.0,
            vec![v.id.clone()],
            Some(v.at),
        );
    }
    let one_drill = |label: String, at: Point, drill: &Drill, m: &mut Min| {
        let (lo, hi) = match drill {
            Drill::Round { diameter } => (*diameter, *diameter),
            Drill::Slot { width, height } => (width.min(*height), width.max(*height)),
        };
        m.low("drill", lo, vec![label.clone()], Some(at));
        m.high("drill_max", lo, vec![label.clone()], Some(at));
        if matches!(drill, Drill::Slot { .. }) {
            m.low("slot_width", lo, vec![label.clone()], Some(at));
        }
        let _ = hi;
    };
    let mut plated_min = f64::INFINITY;
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
            let PadKind::Through { drill, plated } = &pad.kind else {
                continue;
            };
            let label = format!("{refdes}.{}", pad.number);
            let at = pl.transform(pad.at);
            one_drill(label.clone(), at, drill, &mut m);
            if *plated {
                plated_min = plated_min.min(drill.min_size());
                let (w, h) = pad.shape.size();
                let (dw, dh) = match drill {
                    Drill::Round { diameter } => (*diameter, *diameter),
                    Drill::Slot { width, height } => (*width, *height),
                };
                m.low(
                    "pad_ring",
                    ((w - dw) / 2.0).min((h - dh) / 2.0),
                    vec![label],
                    Some(at),
                );
            }
        }
    }
    for h in &d.board.holes {
        one_drill(h.id.clone(), h.at, &h.drill, &mut m);
        if h.plated {
            plated_min = plated_min.min(h.drill.min_size());
        }
    }
    for v in &d.routing.vias {
        plated_min = plated_min.min(v.drill);
    }
    if plated_min.is_finite() && plated_min > 0.0 {
        m.high(
            "drill_aspect_max",
            d.board.stackup.thickness / plated_min,
            Vec::new(),
            None,
        );
    }

    // Copper of different nets, and holes against copper.
    let boxes: Vec<Rect> = g.copper.iter().map(|c| c.prim.bbox()).collect();
    let index = Index::new(boxes.iter().copied());
    for (i, a) in g.copper.iter().enumerate() {
        if matches!(a.owner, Owner::Zone { .. }) {
            continue;
        }
        for j in index.near(&boxes[i], reach) {
            let b = &g.copper[j];
            if j <= i
                || a.layer != b.layer
                || matches!(b.owner, Owner::Zone { .. })
                || a.owner.body() == b.owner.body()
                || (a.net.is_some() && a.net == b.net)
                || (a.owner.is_art() && b.owner.is_art())
            {
                continue;
            }
            // Two pads of one part with no net are the drawing of the footprint.
            let same_part = matches!((&a.owner, &b.owner), (Owner::Pad { part: p, .. }, Owner::Pad { part: q, .. }) if p == q);
            if same_part && a.net.is_none() && b.net.is_none() {
                continue;
            }
            let dist = a.prim.distance(&b.prim);
            if dist <= reach {
                m.low(
                    "clearance",
                    dist,
                    vec![a.owner.label(), b.owner.label()],
                    Some(a.prim.anchor()),
                );
            }
        }
    }
    let part_of = |o: &Owner| match o {
        Owner::Pad { part, .. } => Some(part.clone()),
        _ => None,
    };
    for hole in &g.drills {
        for j in index.near(&hole.prim.bbox(), reach) {
            let c = &g.copper[j];
            if c.layer < hole.layers.0
                || c.layer > hole.layers.1
                || matches!(c.owner, Owner::Zone { .. })
                || c.owner.body() == hole.owner.body()
                || (part_of(&c.owner).is_some() && part_of(&c.owner) == part_of(&hole.owner))
                || (hole.plated && hole.net.is_some() && c.net == hole.net)
            {
                continue;
            }
            let dist = hole.prim.distance(&c.prim);
            if dist <= reach {
                m.low(
                    "hole_to_copper",
                    dist,
                    vec![hole.owner.label(), c.owner.label()],
                    Some(hole.prim.anchor()),
                );
            }
        }
    }
    let dboxes: Vec<Rect> = g.drills.iter().map(|x| x.prim.bbox()).collect();
    let didx = Index::new(dboxes.iter().copied());
    for (i, a) in g.drills.iter().enumerate() {
        for j in didx.near(&dboxes[i], reach.max(1.0)) {
            let b = &g.drills[j];
            if j <= i || a.owner.body() == b.owner.body() {
                continue;
            }
            m.low(
                "hole_to_hole",
                a.prim.distance(&b.prim),
                vec![a.owner.label(), b.owner.label()],
                Some(a.prim.anchor()),
            );
        }
    }
    // The board edge.
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
    let eidx = Index::new(edges.iter().map(Prim::bbox));
    let far = reach.max(1.0);
    for c in &g.copper {
        if matches!(c.owner, Owner::Zone { .. }) {
            continue;
        }
        for k in eidx.near(&c.prim.bbox(), far) {
            m.low(
                "copper_to_edge",
                c.prim.distance(&edges[k]),
                vec![c.owner.label()],
                Some(c.prim.anchor()),
            );
        }
    }
    if !d.routing.zones.is_empty() {
        // A pour keeps this distance from the edge when it fills.
        m.low(
            "copper_to_edge",
            d.board.rules.copper_to_edge,
            Vec::new(),
            None,
        );
    }
    for h in &g.drills {
        for k in eidx.near(&h.prim.bbox(), far) {
            m.low(
                "hole_to_edge",
                h.prim.distance(&edges[k]),
                vec![h.owner.label()],
                Some(h.prim.anchor()),
            );
        }
    }
    // Silkscreen.
    for t in &d.routing.texts {
        if matches!(t.layer, Layer::Silk(_)) {
            m.low("silk_text_height", t.size, vec![t.id.clone()], Some(t.at));
        }
    }
    for gr in &d.routing.graphics {
        if matches!(gr.layer, Layer::Silk(_)) && !gr.filled && gr.width > 0.0 {
            m.low("silk_line", gr.width, vec![gr.id.clone()], None);
        }
    }
    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        for gr in &fp.graphics {
            if gr.layer == FpLayer::Silk && !gr.filled && gr.width > 0.0 {
                m.low("silk_line", gr.width, vec![refdes.clone()], None);
            }
        }
        for t in &fp.texts {
            let shown = t
                .text
                .replace("${ref}", refdes)
                .replace("${value}", part.value.as_deref().unwrap_or(""));
            if t.layer == FpLayer::Silk && !shown.trim().is_empty() {
                m.low(
                    "silk_text_height",
                    t.size,
                    vec![refdes.clone()],
                    Some(pl.transform(t.at)),
                );
            }
        }
    }
    m.out.into_values().collect()
}

/// Compare the minimums of a board with a profile.
pub fn compat(profile: &FabProfile, layers: u8, mins: &[Minimum]) -> Option<FabResult> {
    let tier = profile
        .tiers
        .iter()
        .find(|t| layers >= t.layers.0 && layers <= t.layers.1)?;
    let mut limits = Vec::new();
    for (what, limit) in &tier.limits {
        let Some(min) = mins.iter().find(|m| &m.what == what) else {
            continue;
        };
        let margin = if what.ends_with("_max") {
            limit - min.value
        } else {
            min.value - limit
        };
        limits.push(LimitResult {
            what: what.clone(),
            value: min.value,
            limit: *limit,
            margin: r4(margin),
            fits: margin >= -1e-6,
            items: min.items.clone(),
        });
    }
    limits.sort_by(|a, b| a.margin.total_cmp(&b.margin));
    Some(FabResult {
        profile: profile.id.clone(),
        name: profile.name.clone(),
        tier: tier.id.clone(),
        fits: limits.iter().all(|l| l.fits),
        limits,
        source: profile.source.clone(),
        checked: profile.checked.clone(),
        note: profile.note.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profiles_in_the_program_load_and_have_a_source() {
        let all = profiles();
        assert!(all.len() >= 2);
        for p in &all {
            assert!(p.source.starts_with("https://"), "{}", p.id);
            assert!(!p.checked.is_empty() && !p.tiers.is_empty());
        }
    }
}

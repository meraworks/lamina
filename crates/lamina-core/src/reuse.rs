//! Layout reuse. A block is a set of parts, usually one group (one instance
//! of a Jsonnet block). Its layout is the placement of its parts, and the
//! tracks and vias that connect to its pads in its area. You can put that
//! layout on a different block that has the same footprints with the same
//! connections. That block can be on the same board, or on a different board
//! (from a saved file).
//!
//! Lamina pairs the parts by footprint and by the connections of their pads,
//! not by name. Thus `J2`/`R8` can get the layout of `J3`/`R9`.

use std::collections::{BTreeMap, BTreeSet};

use lamina_geom::prim::{Prim, ring_contains, seg_point};
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The maximum distance outside the courtyards of the parts at which copper
/// is part of the block.
const MARGIN: f64 = 0.5;
/// The matcher stops after this number of tries.
const MAX_STEPS: usize = 200_000;
/// The pads of two footprints align if no pad is farther away than this distance.
const PAD_TOL: f64 = 0.1;

fn schema_v() -> u32 {
    SCHEMA
}

/// The parts of a block: a group name or a list of refdes.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Block {
    Group(String),
    Parts(Vec<String>),
}

impl Block {
    pub fn parts(&self, d: &ProjectData) -> Result<Vec<String>, String> {
        match self {
            Block::Group(g) => {
                let v: Vec<String> = d
                    .parts
                    .parts
                    .iter()
                    .filter(|(_, p)| p.group.as_deref() == Some(g.as_str()))
                    .map(|(r, _)| r.clone())
                    .collect();
                if !v.is_empty() {
                    return Ok(v);
                }
                let known: Vec<String> = d.parts.groups().into_keys().collect();
                Err(if known.is_empty() {
                    format!(
                        "There is no group `{g}`. The board has no groups. \
                         Set `group` on the parts, or use lamina.group in the source."
                    )
                } else {
                    format!(
                        "There is no group `{g}`. The groups are {}.",
                        known.join(", ")
                    )
                })
            }
            Block::Parts(v) => {
                if v.is_empty() {
                    return Err("The part list is empty.".into());
                }
                let mut seen = BTreeSet::new();
                for r in v {
                    if !d.parts.parts.contains_key(r) {
                        return Err(format!("The part {r} does not exist."));
                    }
                    if !seen.insert(r) {
                        return Err(format!("{r} is in the list two times."));
                    }
                }
                Ok(v.clone())
            }
        }
    }

    pub fn label(&self) -> String {
        match self {
            Block::Group(g) => g.clone(),
            Block::Parts(v) => v.join("+"),
        }
    }
}

/// A block layout in its own frame, with the anchor part at the origin and
/// rotation 0. This is also the file format of saved layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutBlock {
    #[serde(default = "schema_v")]
    pub schema: u32,
    pub name: String,
    /// The part (by its name in this block) that is at the origin.
    pub anchor: String,
    /// The key is the refdes that the part had in the source of the layout.
    pub parts: BTreeMap<String, BlockPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<Track>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vias: Vec<Via>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockPart {
    pub footprint: String,
    pub placement: Placement,
    /// The key is the pad number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pads: BTreeMap<String, BlockPad>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockPad {
    /// The pad centre in footprint coordinates. A footprint with a different
    /// name fits if its pads with the same numbers are at the same positions.
    /// For this, Lamina can turn it by quarter turns and move it.
    pub at: Point,
    /// The net name. Only the pattern is important: two pads are on the same
    /// net here only if they must be on the same net in the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

/// The position of a target footprint relative to the footprint of a block
/// part. Its pad n is at R(delta) a_n + c, where a_n is pad n of the block footprint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub delta: f64,
    pub c: Point,
}

impl Fit {
    pub const SAME: Fit = Fit {
        delta: 0.0,
        c: Point { x: 0.0, y: 0.0 },
    };

    /// The origin offset of the footprint, for a part with rotation `rot` on `side`.
    fn offset(self, rot: f64, side: Side) -> Point {
        Placement {
            rotation: rot,
            side,
            ..Placement::at(0.0, 0.0)
        }
        .transform(self.c)
    }

    /// The placement of the target part that puts its pads at the positions
    /// of the pads of the block part, when the block part is at `q`.
    fn onto_target(self, q: &Placement) -> Placement {
        let rot = match q.side {
            Side::Top => q.rotation - self.delta,
            Side::Bottom => q.rotation + self.delta,
        };
        let off = self.offset(rot, q.side);
        Placement {
            x: r6(q.x - off.x),
            y: r6(q.y - off.y),
            rotation: norm_deg(rot),
            ..*q
        }
    }

    /// The inverse of `onto_target`.
    fn onto_block(self, p: &Placement) -> Placement {
        let off = self.offset(p.rotation, p.side);
        let rot = match p.side {
            Side::Top => p.rotation + self.delta,
            Side::Bottom => p.rotation - self.delta,
        };
        Placement {
            x: p.x + off.x,
            y: p.y + off.y,
            rotation: rot,
            ..*p
        }
    }
}

/// The pad centres by pad number. If more than one pad has the same number,
/// this is their mean position.
fn pad_centres(fp: &Footprint) -> BTreeMap<&str, Point> {
    let mut sum: BTreeMap<&str, (Point, f64)> = BTreeMap::new();
    for p in fp.pads.iter().filter(|p| !p.number.is_empty()) {
        let e = sum
            .entry(p.number.as_str())
            .or_insert((Point::new(0.0, 0.0), 0.0));
        e.0 = e.0.add(p.at);
        e.1 += 1.0;
    }
    sum.into_iter()
        .map(|(k, (p, n))| (k, Point::new(p.x / n, p.y / n)))
        .collect()
}

fn mean(pts: impl Iterator<Item = Point>) -> Point {
    let (s, n) = pts.fold((Point::new(0.0, 0.0), 0.0), |(s, n), p| (s.add(p), n + 1.0));
    Point::new(s.x / n, s.y / n)
}

/// Find if `fp` can replace the block part, and find its position.
pub fn fit(bp: &BlockPart, fp: &Footprint) -> Option<Fit> {
    let b = pad_centres(fp);
    if !b.keys().copied().eq(bp.pads.keys().map(String::as_str)) {
        return None;
    }
    if bp.footprint == fp.id {
        return Some(Fit::SAME);
    }
    if b.is_empty() {
        return None;
    }
    let ca = mean(bp.pads.values().map(|p| p.at));
    let cb = mean(b.values().copied());
    let mut best: Option<(f64, Fit)> = None;
    for delta in [0.0, 90.0, 180.0, 270.0] {
        // Round to the micrometre, because the pad centres in libraries are
        // different by some nanometres.
        let c = cb.sub(ca.rotated(delta));
        let c = Point::new((c.x * 1e3).round() / 1e3, (c.y * 1e3).round() / 1e3);
        let err = bp
            .pads
            .iter()
            .map(|(k, a)| b[k.as_str()].dist(a.at.rotated(delta).add(c)))
            .fold(0.0, f64::max);
        if err <= PAD_TOL && best.is_none_or(|(e, _)| err < e - 1e-9) {
            best = Some((err, Fit { delta, c }));
        }
    }
    best.map(|(_, f)| f)
}

/// Rotation then translation: p -> R(rot) p + t.
#[derive(Debug, Clone, Copy)]
struct Rigid {
    rot: f64,
    t: Point,
}

fn r6(v: f64) -> f64 {
    json::round6(v)
}

fn norm_deg(a: f64) -> f64 {
    let r = r6(a.rem_euclid(360.0));
    if r >= 360.0 { 0.0 } else { r }
}

impl Rigid {
    /// The frame of a placed part: its local origin and axes on the board.
    fn of(pl: &Placement) -> Rigid {
        Rigid {
            rot: pl.rotation,
            t: pl.origin(),
        }
    }

    fn inverse(self) -> Rigid {
        Rigid {
            rot: -self.rot,
            t: Point::new(-self.t.x, -self.t.y).rotated(-self.rot),
        }
    }

    /// `self` first, then `g`.
    fn then(self, g: Rigid) -> Rigid {
        Rigid {
            rot: self.rot + g.rot,
            t: self.t.rotated(g.rot).add(g.t),
        }
    }

    fn point(self, p: Point) -> Point {
        let q = p.rotated(self.rot).add(self.t);
        Point::new(r6(q.x), r6(q.y))
    }

    fn placement(self, pl: &Placement) -> Placement {
        let o = self.point(pl.origin());
        Placement {
            x: o.x,
            y: o.y,
            rotation: norm_deg(pl.rotation + self.rot),
            ..*pl
        }
    }

    fn track(self, t: &Track) -> Track {
        Track {
            points: t.points.iter().map(|p| self.point(*p)).collect(),
            ..t.clone()
        }
    }

    fn via(self, v: &Via) -> Via {
        Via {
            at: self.point(v.at),
            ..v.clone()
        }
    }
}

impl LayoutBlock {
    /// The same layout, with a different part of it at the origin.
    pub fn rebase(&self, anchor: &str) -> Result<LayoutBlock, String> {
        let a = self
            .parts
            .get(anchor)
            .ok_or_else(|| format!("{anchor} is not in the block {}.", self.name))?;
        let f = Rigid::of(&a.placement).inverse();
        Ok(LayoutBlock {
            schema: self.schema,
            name: self.name.clone(),
            anchor: anchor.to_string(),
            parts: self
                .parts
                .iter()
                .map(|(k, p)| {
                    (
                        k.clone(),
                        BlockPart {
                            placement: f.placement(&p.placement),
                            ..p.clone()
                        },
                    )
                })
                .collect(),
            tracks: self.tracks.iter().map(|t| f.track(t)).collect(),
            vias: self.vias.iter().map(|v| f.via(v)).collect(),
        })
    }
}

/// The copper of a block: track segments as (track id, segment index), and
/// via ids.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Attached {
    pub segments: BTreeSet<(String, usize)>,
    pub vias: BTreeSet<String>,
}

/// Convex hull, counter-clockwise (Andrew's monotone chain).
fn hull(mut pts: Vec<Point>) -> Vec<Point> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
    if pts.len() < 3 {
        return pts;
    }
    let cross =
        |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut lower: Vec<Point> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Point> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// True if the point is in the hull, or not more than `margin` from it.
fn near_hull(h: &[Point], p: Point, margin: f64) -> bool {
    match h.len() {
        0 => false,
        1 => h[0].dist(p) <= margin,
        _ => {
            if h.len() >= 3 && ring_contains(h, p) {
                return true;
            }
            (0..h.len()).any(|i| seg_point(h[i], h[(i + 1) % h.len()], p) <= margin)
        }
    }
}

struct Dsu(Vec<usize>);

impl Dsu {
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.0[r] != r {
            r = self.0[r];
        }
        self.0[x] = r;
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

enum Kind {
    Pad,
    Seg(String, usize),
    Via(String),
}

struct Item {
    net: String,
    prim: Prim,
    layers: Vec<u8>,
    kind: Kind,
}

fn via_layers(v: &Via, layers: u8) -> Vec<u8> {
    let idx = |l: Option<Layer>, dflt: u8| {
        l.and_then(|l| l.copper())
            .map(|c| c.index(layers))
            .unwrap_or(dflt)
    };
    let (a, b) = (idx(v.from, 0), idx(v.to, layers.saturating_sub(1)));
    (a.min(b)..=a.max(b)).collect()
}

/// The copper of the block that connects to its pads:
/// - All tracks and vias of an internal net (a net that has all of its pads
///   on the parts of the block), at any distance.
/// - On nets that go out of the block, only a fan-out to a via (a pad, a
///   short track and a via) that is next to one of the parts. Routes that go
///   to the rest of the board are different for each copy, so they do not change.
pub fn attached(d: &ProjectData, parts: &[String]) -> Attached {
    let layers = d.board.stackup.copper_layers;
    let pad_nets = d.nets.pad_map();
    let members: BTreeSet<&str> = parts.iter().map(String::as_str).collect();
    let internal: BTreeSet<&str> = d
        .nets
        .nets
        .iter()
        .filter(|(_, n)| {
            !n.pads.is_empty() && n.pads.iter().all(|p| members.contains(p.part.as_str()))
        })
        .map(|(k, _)| k.as_str())
        .collect();
    let mut items: Vec<Item> = Vec::new();
    // One area for each part: its courtyard and its pads.
    let mut areas: Vec<Vec<Point>> = Vec::new();
    let mut nets = BTreeSet::new();
    for r in parts {
        let (Some(part), Some(pl)) = (d.parts.parts.get(r), d.placement.placements.get(r)) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        let mut area = lamina_geom::courtyard(fp, pl).unwrap_or_default();
        for (pad, _, prim, ls) in lamina_geom::placed_pads(fp, pl, layers) {
            area.extend(prim.outline(0.05));
            if let Some(n) = pad_nets.get(&PadRef::new(r.clone(), pad.number.clone())) {
                nets.insert(n.clone());
                items.push(Item {
                    net: n.clone(),
                    prim,
                    layers: ls,
                    kind: Kind::Pad,
                });
            }
        }
        areas.push(hull(area));
    }
    let near = |p: Point| areas.iter().any(|h| near_hull(h, p, MARGIN));
    let wanted = |net: &str, pts: &[Point]| {
        internal.contains(net) || (nets.contains(net) && pts.iter().all(|p| near(*p)))
    };
    for t in &d.routing.tracks {
        if !nets.contains(&t.net) {
            continue;
        }
        let Some(c) = t.layer.copper() else { continue };
        let l = c.index(layers);
        for (k, w) in t.points.windows(2).enumerate() {
            if wanted(&t.net, w) {
                items.push(Item {
                    net: t.net.clone(),
                    prim: Prim::Capsule {
                        a: w[0],
                        b: w[1],
                        r: t.width / 2.0,
                    },
                    layers: vec![l],
                    kind: Kind::Seg(t.id.clone(), k),
                });
            }
        }
    }
    for v in &d.routing.vias {
        if nets.contains(&v.net) && wanted(&v.net, &[v.at]) {
            items.push(Item {
                net: v.net.clone(),
                prim: Prim::Circle {
                    c: v.at,
                    r: v.diameter / 2.0,
                },
                layers: via_layers(v, layers),
                kind: Kind::Via(v.id.clone()),
            });
        }
    }
    let n = items.len();
    let mut dsu = Dsu((0..n).collect());
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&items[i], &items[j]);
            if a.net == b.net
                && a.layers.iter().any(|l| b.layers.contains(l))
                && a.prim.distance(&b.prim) <= 1e-4
            {
                dsu.union(i, j);
            }
        }
    }
    let pads: BTreeSet<usize> = (0..n)
        .filter(|&i| matches!(items[i].kind, Kind::Pad))
        .map(|i| dsu.find(i))
        .collect();
    let vias: BTreeSet<usize> = (0..n)
        .filter(|&i| matches!(items[i].kind, Kind::Via(_)))
        .map(|i| dsu.find(i))
        .collect();
    let roots: BTreeSet<usize> = pads
        .into_iter()
        .filter(|r| internal.contains(items[*r].net.as_str()) || vias.contains(r))
        .collect();
    let mut out = Attached::default();
    for (i, it) in items.iter().enumerate() {
        if !roots.contains(&dsu.find(i)) {
            continue;
        }
        match &it.kind {
            Kind::Pad => {}
            Kind::Seg(id, k) => {
                out.segments.insert((id.clone(), *k));
            }
            Kind::Via(id) => {
                out.vias.insert(id.clone());
            }
        }
    }
    out
}

/// The runs of selected segments of a track that follow each other, as point lists.
fn runs(t: &Track, keep: impl Fn(usize) -> bool) -> Vec<Vec<Point>> {
    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    for k in 0..t.points.len().saturating_sub(1) {
        if keep(k) {
            if cur.is_empty() {
                cur.push(t.points[k]);
            }
            cur.push(t.points[k + 1]);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn area_of(fp: &Footprint) -> f64 {
    fp.courtyard_bbox()
        .or_else(|| fp.pads_bbox())
        .map(|r| r.width() * r.height())
        .unwrap_or(0.0)
}

/// Get the layout of a placed block. The default anchor is its largest part.
pub fn extract(
    d: &ProjectData,
    parts: &[String],
    anchor: Option<&str>,
    name: &str,
) -> Result<LayoutBlock, String> {
    let mut placed = Vec::new();
    for r in parts {
        let part = d
            .parts
            .parts
            .get(r)
            .ok_or_else(|| format!("The part {r} does not exist."))?;
        let pl = d
            .placement
            .placements
            .get(r)
            .ok_or_else(|| format!("{r} is not placed. Lay out the source block first."))?;
        let fp = d
            .footprints
            .get(&part.footprint)
            .ok_or_else(|| format!("The footprint {} does not exist.", part.footprint))?;
        placed.push((r, part, *pl, fp));
    }
    let anchor = match anchor {
        Some(a) => {
            if !parts.iter().any(|r| r == a) {
                return Err(format!("The anchor {a} is not in the block."));
            }
            a.to_string()
        }
        None => placed
            .iter()
            .max_by(|a, b| {
                area_of(a.3)
                    .total_cmp(&area_of(b.3))
                    .then_with(|| b.0.cmp(a.0))
            })
            .map(|p| p.0.clone())
            .ok_or("The block is empty.")?,
    };
    let f = Rigid::of(&d.placement.placements[&anchor]).inverse();
    let pad_nets = d.nets.pad_map();
    let mut out = LayoutBlock {
        schema: SCHEMA,
        name: name.to_string(),
        anchor,
        parts: BTreeMap::new(),
        tracks: Vec::new(),
        vias: Vec::new(),
    };
    for (r, part, pl, fp) in &placed {
        let pads = pad_centres(fp)
            .into_iter()
            .map(|(k, at)| {
                let net = pad_nets.get(&PadRef::new((*r).clone(), k)).cloned();
                (
                    k.to_string(),
                    BlockPad {
                        at: Point::new(r6(at.x), r6(at.y)),
                        net,
                    },
                )
            })
            .collect();
        out.parts.insert(
            (*r).clone(),
            BlockPart {
                footprint: part.footprint.clone(),
                placement: Placement {
                    locked: false,
                    ..f.placement(pl)
                },
                pads,
            },
        );
    }
    let a = attached(d, parts);
    for t in &d.routing.tracks {
        for pts in runs(t, |k| a.segments.contains(&(t.id.clone(), k))) {
            let id = format!("t{}", out.tracks.len() + 1);
            out.tracks.push(f.track(&Track {
                id,
                points: pts,
                ..t.clone()
            }));
        }
    }
    for v in &d.routing.vias {
        if a.vias.contains(&v.id) {
            let id = format!("v{}", out.vias.len() + 1);
            out.vias.push(f.via(&Via { id, ..v.clone() }));
        }
    }
    Ok(out)
}

/// The pairs between a block and the target parts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Match {
    /// A map from the block part name to the target refdes.
    pub parts: BTreeMap<String, String>,
    /// A map from the block net name to the target net name.
    pub nets: BTreeMap<String, String>,
    /// The position of each target footprint relative to the footprint of
    /// its block part.
    pub fits: BTreeMap<String, Fit>,
}

/// (block, target) name pairs.
type Pairs<'a> = Vec<(&'a str, &'a str)>;

struct Matcher<'a> {
    order: Vec<&'a str>,
    cands: BTreeMap<&'a str, Vec<&'a str>>,
    /// For each (block part, target part): the (block net, target net) pairs
    /// that their pads need, or None if a pad has a net on only one side.
    demands: BTreeMap<(&'a str, &'a str), Option<Pairs<'a>>>,
    used: BTreeSet<&'a str>,
    fwd: BTreeMap<&'a str, &'a str>,
    bwd: BTreeMap<&'a str, &'a str>,
    pick: BTreeMap<&'a str, &'a str>,
    steps: usize,
    deepest: usize,
}

impl<'a> Matcher<'a> {
    fn search(&mut self, i: usize) -> Result<bool, String> {
        self.deepest = self.deepest.max(i);
        if i == self.order.len() {
            return Ok(true);
        }
        let b = self.order[i];
        for t in self.cands[b].clone() {
            if self.used.contains(t) {
                continue;
            }
            self.steps += 1;
            if self.steps > MAX_STEPS {
                return Err(
                    "There are too many ways to pair the parts. Give smaller blocks.".into(),
                );
            }
            let Some(Some(dem)) = self.demands.get(&(b, t)) else {
                continue;
            };
            let mut added: Vec<&str> = Vec::new();
            let mut ok = true;
            for &(bn, tn) in dem {
                match (self.fwd.get(bn), self.bwd.get(tn)) {
                    (None, None) => {
                        self.fwd.insert(bn, tn);
                        self.bwd.insert(tn, bn);
                        added.push(bn);
                    }
                    (Some(&x), Some(&y)) if x == tn && y == bn => {}
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                self.used.insert(t);
                self.pick.insert(b, t);
                if self.search(i + 1)? {
                    return Ok(true);
                }
                self.used.remove(t);
                self.pick.remove(b);
            }
            for bn in added {
                if let Some(tn) = self.fwd.remove(bn) {
                    self.bwd.remove(tn);
                }
            }
        }
        Ok(false)
    }
}

/// Pair each block part with a target part that has the same footprint.
/// Pads that are on the same net in the block must be on the same net in
/// the target, and the opposite.
pub fn match_block(d: &ProjectData, b: &LayoutBlock, target: &[String]) -> Result<Match, String> {
    if target.len() != b.parts.len() {
        return Err(format!(
            "The block {} has {} parts, but the target has {}.",
            b.name,
            b.parts.len(),
            target.len()
        ));
    }
    for r in target {
        if !d.parts.parts.contains_key(r) {
            return Err(format!("The part {r} does not exist."));
        }
    }
    let pad_nets = d.nets.pad_map();
    let mut fits: BTreeMap<(&str, &str), Fit> = BTreeMap::new();
    let mut m = Matcher {
        order: b.parts.keys().map(String::as_str).collect(),
        cands: BTreeMap::new(),
        demands: BTreeMap::new(),
        used: BTreeSet::new(),
        fwd: BTreeMap::new(),
        bwd: BTreeMap::new(),
        pick: BTreeMap::new(),
        steps: 0,
        deepest: 0,
    };
    for (bk, bp) in &b.parts {
        let mut c = Vec::new();
        for t in target {
            let Some(fp) = d.footprints.get(&d.parts.parts[t].footprint) else {
                continue;
            };
            let Some(f) = fit(bp, fp) else { continue };
            fits.insert((bk.as_str(), t.as_str()), f);
            c.push(t.as_str());
            let mut dem = Some(Vec::new());
            for (num, pad) in &bp.pads {
                let tn = pad_nets
                    .get(&PadRef::new(t.clone(), num.clone()))
                    .map(String::as_str);
                match (pad.net.as_deref(), tn) {
                    (None, None) => {}
                    (Some(x), Some(y)) => dem.as_mut().unwrap().push((x, y)),
                    _ => {
                        dem = None;
                        break;
                    }
                }
            }
            m.demands.insert((bk.as_str(), t.as_str()), dem);
        }
        if c.is_empty() {
            return Err(format!(
                "No part in the target can replace {bk} ({}). No footprint has its pads \
                 at the same positions.",
                bp.footprint
            ));
        }
        m.cands.insert(bk.as_str(), c);
    }
    // Do the most constrained parts first: the fewest candidates, then the most pads.
    m.order
        .sort_by_key(|k| (m.cands[k].len(), std::cmp::Reverse(b.parts[*k].pads.len())));
    if m.search(0)? {
        return Ok(Match {
            parts: m
                .pick
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            nets: m
                .fwd
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            fits: m
                .pick
                .iter()
                .map(|(a, b)| (a.to_string(), fits[&(*a, *b)]))
                .collect(),
        });
    }
    let stuck = m
        .order
        .get(m.deepest.min(m.order.len() - 1))
        .copied()
        .unwrap_or("?");
    Err(format!(
        "The target is not wired like the block {}. No part fits {stuck} ({}) \
         with the same connections.",
        b.name, b.parts[stuck].footprint
    ))
}

/// Settings for [`apply`].
#[derive(Debug, Clone, Default)]
pub struct ApplyOpts {
    /// The position of the anchor part of the target. The default is its
    /// position now.
    pub at: Option<Placement>,
    /// Copy the tracks and vias, and remove the old tracks and vias of the target.
    pub routing: bool,
    /// Also move locked parts.
    pub force: bool,
    /// Copper that must not change, also if it touches the target (the
    /// copper of the source).
    pub keep: Attached,
}

/// What [`apply`] did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct Applied {
    /// The target refdes that did not move (or that moved to `at`).
    pub anchor: String,
    pub parts: usize,
    pub tracks: usize,
    pub vias: usize,
    /// The number of old track segments and vias of the target that were removed.
    pub removed: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

fn all_ids(d: &ProjectData) -> Vec<String> {
    let r = &d.routing;
    r.tracks
        .iter()
        .map(|t| t.id.clone())
        .chain(r.vias.iter().map(|v| v.id.clone()))
        .chain(r.zones.iter().map(|z| z.id.clone()))
        .chain(r.keepouts.iter().map(|k| k.id.clone()))
        .chain(r.texts.iter().map(|k| k.id.clone()))
        .chain(r.graphics.iter().map(|k| k.id.clone()))
        .chain(d.board.holes.iter().map(|h| h.id.clone()))
        .collect()
}

fn fresh(ids: &mut Vec<String>, prefix: &str) -> String {
    let id = crate::project::next_id(prefix, ids.iter().map(String::as_str));
    ids.push(id.clone());
    id
}

/// Put the layout of a block on the target parts. The target part that is
/// paired with the anchor of the block does not move (or it moves to `at`).
/// The other parts are placed around it as in the block.
pub fn apply(
    d: &mut ProjectData,
    b: &LayoutBlock,
    target: &[String],
    o: &ApplyOpts,
) -> Result<Applied, String> {
    let m = match_block(d, b, target)?;
    let anchor = m.parts[&b.anchor].clone();
    let ba = &b.parts[&b.anchor].placement;
    let at = match o.at {
        Some(p) => p,
        None => *d.placement.placements.get(&anchor).ok_or_else(|| {
            format!(
                "{anchor} is not placed. It is the anchor of the block. \
                     Place it first, or give `at`."
            )
        })?,
    };
    if at.side != ba.side {
        return Err(format!(
            "{anchor} is on the {} side, but the block has it on the {} side. Flip it first.",
            at.side.as_str(),
            ba.side.as_str()
        ));
    }
    // Find the position of the anchor footprint of the block, then the frame map.
    let at_block = m.fits[&b.anchor].onto_block(&at);
    let f = Rigid::of(ba).inverse().then(Rigid::of(&at_block));
    let mut out = Applied {
        anchor: anchor.clone(),
        ..Applied::default()
    };

    let mut moves = Vec::new();
    for (bk, bp) in &b.parts {
        let t = &m.parts[bk];
        let old = d.placement.placements.get(t).copied();
        let mut pl = m.fits[bk].onto_target(&f.placement(&bp.placement));
        pl.locked = old.is_some_and(|p| p.locked);
        if let Some(old) = old {
            let moved =
                (old.x, old.y, old.rotation, old.side) != (pl.x, pl.y, pl.rotation, pl.side);
            if old.locked && moved && !o.force {
                return Err(format!("{t} is locked. Set force: true to move it."));
            }
        }
        moves.push((t.clone(), pl));
    }

    if o.routing {
        // Find the old copper of the target before its parts move.
        let old = attached(d, target);
        let mut ids = all_ids(d);
        let mut tracks = Vec::new();
        for t in std::mem::take(&mut d.routing.tracks) {
            let gone = |k: usize| {
                old.segments.contains(&(t.id.clone(), k))
                    && !o.keep.segments.contains(&(t.id.clone(), k))
            };
            let n = (0..t.points.len().saturating_sub(1))
                .filter(|&k| gone(k))
                .count();
            if n == 0 {
                tracks.push(t);
                continue;
            }
            out.removed += n;
            for (i, pts) in runs(&t, |k| !gone(k)).into_iter().enumerate() {
                let id = if i == 0 {
                    t.id.clone()
                } else {
                    fresh(&mut ids, "t")
                };
                tracks.push(Track {
                    id,
                    points: pts,
                    ..t.clone()
                });
            }
        }
        d.routing.tracks = tracks;
        let before = d.routing.vias.len();
        d.routing
            .vias
            .retain(|v| !old.vias.contains(&v.id) || o.keep.vias.contains(&v.id));
        out.removed += before - d.routing.vias.len();

        let layers = d.board.stackup.copper_layers;
        let usable = |l: Layer| match l.copper() {
            Some(Copper::Inner(n)) => n >= 1 && n + 1 < layers,
            Some(_) => true,
            None => false,
        };
        let mut skipped = BTreeSet::new();
        for t in &b.tracks {
            let Some(net) = m.nets.get(&t.net) else {
                skipped.insert(t.net.clone());
                continue;
            };
            if !usable(t.layer) {
                out.notes.push(format!(
                    "A track on {} was not copied, because this board does not have that layer.",
                    t.layer
                ));
                continue;
            }
            let mut nt = f.track(t);
            nt.id = fresh(&mut ids, "t");
            nt.net = net.clone();
            d.routing.tracks.push(nt);
            out.tracks += 1;
        }
        for v in &b.vias {
            let Some(net) = m.nets.get(&v.net) else {
                skipped.insert(v.net.clone());
                continue;
            };
            let mut nv = f.via(v);
            nv.id = fresh(&mut ids, "v");
            nv.net = net.clone();
            d.routing.vias.push(nv);
            out.vias += 1;
        }
        if !skipped.is_empty() {
            out.notes.push(format!(
                "The copper on {} was not copied, because no pad of the block is on that net.",
                skipped.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    for (t, pl) in moves {
        d.placement.placements.insert(t, pl);
        out.parts += 1;
    }
    Ok(out)
}

/// The groups, other than `group`, that can get its layout.
pub fn matching_groups(d: &ProjectData, group: &str) -> Vec<String> {
    let groups = d.parts.groups();
    let Some(src) = groups.get(group) else {
        return Vec::new();
    };
    let Ok(b) = extract(d, src, None, group) else {
        return Vec::new();
    };
    groups
        .iter()
        .filter(|(g, parts)| *g != group && match_block(d, &b, parts).is_ok())
        .map(|(g, _)| g.clone())
        .collect()
}

/// Copy the layout of the block `from` to each target. `anchor` is a part
/// that does not move. It can be in `from` (then the part that matches it in
/// each target does not move) or in one of the targets.
pub fn repeat(
    d: &mut ProjectData,
    from: &[String],
    to: &[Vec<String>],
    anchor: Option<&str>,
    o: &ApplyOpts,
) -> Result<Vec<Applied>, String> {
    if to.is_empty() {
        return Err("No target block is given. Give at least one block in `to`.".into());
    }
    if o.at.is_some() && to.len() > 1 {
        return Err("If you give `at`, you must give exactly one target.".into());
    }
    let mut seen: BTreeSet<&str> = from.iter().map(String::as_str).collect();
    for t in to {
        for r in t {
            if !seen.insert(r.as_str()) {
                return Err(format!("{r} is in more than one block."));
            }
        }
    }
    let mut b = extract(d, from, None, "layout")?;
    if let Some(a) = anchor {
        if from.iter().any(|r| r == a) {
            b = b.rebase(a)?;
        } else {
            let t = to
                .iter()
                .find(|t| t.iter().any(|r| r == a))
                .ok_or_else(|| format!("The anchor {a} is not in any of the blocks."))?;
            let m = match_block(d, &b, t)?;
            let local = m
                .parts
                .iter()
                .find(|(_, r)| r.as_str() == a)
                .map(|(k, _)| k.clone())
                .ok_or("No part in the layout matches the anchor.")?;
            b = b.rebase(&local)?;
        }
    }
    let o = ApplyOpts {
        keep: attached(d, from),
        ..o.clone()
    };
    let mut out = Vec::new();
    for t in to {
        out.push(apply(d, &b, t, &o)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(id: &str, pads: &[(&str, f64)]) -> Footprint {
        let mut f = Footprint::new(
            id,
            Provenance::Datasheet {
                document: "test".into(),
                page: None,
                url: None,
            },
        );
        for (n, x) in pads {
            f.pads.push(Pad::smd(
                *n,
                Point::new(*x, 0.0),
                PadShape::Rect {
                    width: 0.6,
                    height: 0.6,
                },
            ));
        }
        f
    }

    fn net(d: &mut ProjectData, name: &str, pads: &[&str]) {
        d.nets.nets.insert(
            name.into(),
            Net {
                pads: pads.iter().map(|p| p.parse().unwrap()).collect(),
                class: None,
                ..Default::default()
            },
        );
    }

    fn track(d: &mut ProjectData, id: &str, net: &str, pts: &[(f64, f64)]) {
        d.routing.tracks.push(Track {
            id: id.into(),
            net: net.into(),
            layer: Layer::TOP,
            width: 0.25,
            points: pts.iter().map(|(x, y)| Point::new(*x, *y)).collect(),
        });
    }

    /// Two LED blocks: Dn (2 pads) + Rn (2 pads), wired IN -> R -> A -> D -> GND.
    /// Block 1 is laid out and routed. Block 2 is at a different position and
    /// is not routed. Its parts are in the opposite order.
    fn board() -> ProjectData {
        let mut d = ProjectData::new("t");
        d.footprints
            .insert("LED".into(), fp("LED", &[("1", -0.8), ("2", 0.8)]));
        d.footprints
            .insert("R".into(), fp("R", &[("1", -0.5), ("2", 0.5)]));
        for (r, f, g) in [
            ("D1", "LED", "a"),
            ("R1", "R", "a"),
            ("D2", "LED", "b"),
            ("R2", "R", "b"),
        ] {
            let mut p = Part::new(f);
            p.group = Some(g.into());
            d.parts.parts.insert(r.into(), p);
        }
        // IN and GND go out of the blocks (U9 is a controller at a different position).
        net(&mut d, "IN1", &["R1.1", "U9.1"]);
        net(&mut d, "A1", &["R1.2", "D1.1"]);
        net(&mut d, "IN2", &["R2.1", "U9.2"]);
        net(&mut d, "A2", &["R2.2", "D2.1"]);
        net(&mut d, "GND", &["D1.2", "D2.2"]);
        let pl = |x, y, rot| Placement {
            rotation: rot,
            ..Placement::at(x, y)
        };
        d.placement
            .placements
            .insert("D1".into(), pl(10.0, 10.0, 0.0));
        d.placement
            .placements
            .insert("R1".into(), pl(10.0, 7.0, 90.0));
        d.placement
            .placements
            .insert("D2".into(), pl(30.0, 20.0, 90.0));
        d.placement
            .placements
            .insert("R2".into(), pl(40.0, 40.0, 0.0));
        // R1.2 is at (10, 7.5). D1.1 is at (9.2, 10).
        track(&mut d, "t1", "A1", &[(10.0, 7.5), (9.2, 8.3), (9.2, 10.0)]);
        // This track goes out of the block to the controller, so it is not
        // part of the block.
        track(
            &mut d,
            "t2",
            "IN1",
            &[(10.0, 6.5), (10.0, 6.0), (0.0, -20.0)],
        );
        // The old routing of block 2. The test replaces it.
        track(&mut d, "t3", "A2", &[(40.5, 40.0), (30.0, 19.2)]);
        // A GND track that goes near D1, but does not touch its pads.
        track(&mut d, "t4", "GND", &[(8.5, 10.6), (11.5, 10.6)]);
        // The GND fan-out of D1 to a via. It is part of the block.
        track(&mut d, "t6", "GND", &[(10.8, 10.0), (11.4, 9.4)]);
        d.routing.vias.push(Via {
            id: "v1".into(),
            net: "GND".into(),
            at: Point::new(11.4, 9.4),
            diameter: 0.6,
            drill: 0.3,
            from: None,
            to: None,
        });
        d
    }

    #[test]
    fn hull_and_margin() {
        let h = hull(vec![
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
            Point::new(0.0, 2.0),
        ]);
        assert_eq!(h.len(), 4);
        assert!(ring_area_ccw(&h));
        assert!(near_hull(&h, Point::new(1.0, 1.0), 0.0));
        assert!(near_hull(&h, Point::new(2.4, 1.0), 0.5));
        assert!(!near_hull(&h, Point::new(2.6, 1.0), 0.5));
    }

    fn ring_area_ccw(r: &[Point]) -> bool {
        lamina_geom::prim::ring_area(r) > 0.0
    }

    #[test]
    fn attached_skips_passing_copper() {
        let d = board();
        let a = attached(&d, &["D1".into(), "R1".into()]);
        let segs: Vec<(String, usize)> = a.segments.into_iter().collect();
        assert_eq!(
            segs,
            vec![("t1".into(), 0), ("t1".into(), 1), ("t6".into(), 0)]
        );
        assert_eq!(
            a.vias.into_iter().collect::<Vec<_>>(),
            vec!["v1".to_string()]
        );
    }

    #[test]
    fn matches_by_wiring_not_names() {
        let d = board();
        let b = extract(&d, &["D1".into(), "R1".into()], None, "a").unwrap();
        let m = match_block(&d, &b, &["R2".into(), "D2".into()]).unwrap();
        assert_eq!(m.parts["D1"], "D2");
        assert_eq!(m.parts["R1"], "R2");
        assert_eq!(m.nets["A1"], "A2");
        assert_eq!(m.nets["GND"], "GND");
        assert_eq!(m.nets["IN1"], "IN2");
    }

    #[test]
    fn refuses_other_wiring() {
        let mut d = board();
        // D2 is reversed: its pad 1 now connects to GND.
        net(&mut d, "A2", &["R2.2", "D2.2"]);
        net(&mut d, "GND", &["D1.2", "D2.1"]);
        let b = extract(&d, &["D1".into(), "R1".into()], None, "a").unwrap();
        let e = match_block(&d, &b, &["D2".into(), "R2".into()]).unwrap_err();
        assert!(e.contains("not wired like"), "{e}");
        let e = match_block(&d, &b, &["D2".into()]).unwrap_err();
        assert!(e.contains("2 parts"), "{e}");
    }

    #[test]
    fn repeat_places_and_routes_around_anchor() {
        let mut d = board();
        let o = ApplyOpts {
            routing: true,
            ..ApplyOpts::default()
        };
        let r = repeat(
            &mut d,
            &["D1".into(), "R1".into()],
            &[vec!["D2".into(), "R2".into()]],
            Some("D1"),
            &o,
        )
        .unwrap();
        assert_eq!(r[0].anchor, "D2");
        assert_eq!(r[0].removed, 1);
        assert_eq!((r[0].tracks, r[0].vias), (2, 1));
        // D2 stays at (30, 20) with rotation 90. R2 is relative to D2 as R1 is
        // to D1, but turned a quarter. R1 is 3 below D1, so R2 is 3 to the right of D2.
        let pl = d.placement.placements["D2"];
        assert_eq!((pl.x, pl.y, pl.rotation), (30.0, 20.0, 90.0));
        let pl = d.placement.placements["R2"];
        assert!(
            (pl.x - 33.0).abs() < 1e-9 && (pl.y - 20.0).abs() < 1e-9,
            "{pl:?}"
        );
        assert_eq!(pl.rotation, 180.0);
        // The old A2 track is removed, and the copy connects the new pads.
        assert!(!d.routing.tracks.iter().any(|t| t.id == "t3"));
        let a2: Vec<&Track> = d.routing.tracks.iter().filter(|t| t.net == "A2").collect();
        assert_eq!(a2.len(), 1);
        assert_eq!(a2[0].points[0], Point::new(32.5, 20.0));
        assert_eq!(*a2[0].points.last().unwrap(), Point::new(30.0, 19.2));
        // The GND fan-out was copied with its via. The IN route and the GND
        // track that goes near D1 were not copied.
        assert!(!d.routing.tracks.iter().any(|t| t.net == "IN2"));
        assert_eq!(
            d.routing.tracks.iter().filter(|t| t.net == "GND").count(),
            3
        );
        assert_eq!(d.routing.vias.len(), 2);
        // The source did not change.
        assert_eq!(d.placement.placements["R1"].x, 10.0);
        assert!(d.routing.tracks.iter().any(|t| t.id == "t1"));
    }

    #[test]
    fn anchor_in_target_and_locks() {
        let mut d = board();
        d.placement.placements.get_mut("D2").unwrap().locked = true;
        let o = ApplyOpts::default();
        // R2 is the anchor. D2 must move, but it is locked.
        let e = repeat(
            &mut d,
            &["D1".into(), "R1".into()],
            &[vec!["D2".into(), "R2".into()]],
            Some("R2"),
            &o,
        )
        .unwrap_err();
        assert!(e.contains("locked"), "{e}");
        let o = ApplyOpts { force: true, ..o };
        repeat(
            &mut d,
            &["D1".into(), "R1".into()],
            &[vec!["D2".into(), "R2".into()]],
            Some("R2"),
            &o,
        )
        .unwrap();
        // In the frame of R1, D1 is 3 along the x axis, turned 270. R2 has no
        // rotation, so D2 goes 3 to the right of R2, at 270.
        let r2 = d.placement.placements["R2"];
        assert_eq!((r2.x, r2.y, r2.rotation), (40.0, 40.0, 0.0));
        let d2 = d.placement.placements["D2"];
        assert!(
            (d2.x - 43.0).abs() < 1e-9 && (d2.y - 40.0).abs() < 1e-9,
            "{d2:?}"
        );
        assert_eq!(d2.rotation, 270.0);
        assert!(d2.locked);
    }

    #[test]
    fn saved_block_round_trips_and_rebases() {
        let d = board();
        let b = extract(&d, &["D1".into(), "R1".into()], Some("R1"), "led").unwrap();
        assert_eq!(b.parts["R1"].placement.origin(), Point::new(0.0, 0.0));
        assert_eq!(b.parts["R1"].placement.rotation, 0.0);
        let s = serde_json::to_string(&b).unwrap();
        let back: LayoutBlock = serde_json::from_str(&s).unwrap();
        assert_eq!(back, b);
        let r = b.rebase("D1").unwrap();
        assert_eq!(r.parts["D1"].placement.origin(), Point::new(0.0, 0.0));
        let again = r.rebase("R1").unwrap();
        for (k, p) in &b.parts {
            let q = &again.parts[k].placement;
            assert!(p.placement.origin().dist(q.origin()) < 1e-6);
            assert_eq!(p.placement.rotation, q.rotation);
        }
    }

    #[test]
    fn footprints_fit_by_pad_places() {
        let d = board();
        let b = extract(&d, &["D1".into(), "R1".into()], None, "a").unwrap();
        let bp = &b.parts["D1"];
        // The origin is on pad 1, not at the centre.
        let f = fit(bp, &fp("X", &[("1", 0.0), ("2", 1.6)])).unwrap();
        assert_eq!(f.delta, 0.0);
        assert!((f.c.x - 0.8).abs() < 1e-9 && f.c.y.abs() < 1e-9);
        // The pads have the opposite numbers: a half turn.
        assert_eq!(
            fit(bp, &fp("X", &[("2", -0.85), ("1", 0.85)]))
                .unwrap()
                .delta,
            180.0
        );
        // A wider pad spacing, or different pad numbers: no fit.
        assert!(fit(bp, &fp("X", &[("1", -1.0), ("2", 1.0)])).is_none());
        assert!(fit(bp, &fp("X", &[("A", -0.8), ("K", 0.8)])).is_none());
    }

    #[test]
    fn other_footprint_is_turned_so_its_pads_land_right() {
        let mut d = board();
        // D2 is the same LED from a different library, with the opposite pad numbers.
        d.footprints
            .insert("LEDB".into(), fp("LEDB", &[("2", -0.85), ("1", 0.85)]));
        d.parts.parts.get_mut("D2").unwrap().footprint = "LEDB".into();
        let o = ApplyOpts {
            routing: true,
            ..ApplyOpts::default()
        };
        repeat(
            &mut d,
            &["D1".into(), "R1".into()],
            &[vec!["D2".into(), "R2".into()]],
            Some("R2"),
            &o,
        )
        .unwrap();
        // With LED, D2 is at (43, 40) at 270. LEDB needs one more half turn.
        let d2 = d.placement.placements["D2"];
        assert!(
            (d2.x - 43.0).abs() < 1e-9 && (d2.y - 40.0).abs() < 1e-9,
            "{d2:?}"
        );
        assert_eq!(d2.rotation, 90.0);
        // The copied anode track ends on D2.1, now at (43, 40.85).
        let a2 = d.routing.tracks.iter().find(|t| t.net == "A2").unwrap();
        let end = *a2.points.last().unwrap();
        assert!(end.dist(Point::new(43.0, 40.85)) < 0.1, "{end:?}");
    }

    #[test]
    fn group_block_resolves() {
        let d = board();
        let mut v = Block::Group("b".into()).parts(&d).unwrap();
        v.sort();
        assert_eq!(v, vec!["D2", "R2"]);
        let e = Block::Group("zz".into()).parts(&d).unwrap_err();
        assert!(e.contains("groups are a, b"), "{e}");
        assert_eq!(matching_groups(&d, "a"), vec!["b".to_string()]);
    }
}

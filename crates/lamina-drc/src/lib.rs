//! Design rule checks, connectivity and fab presets.

pub mod assembly;
mod connect;
mod copper;
mod ctx;
mod edge;
pub mod electrical;
pub mod fab;
mod finish;
pub mod footprints;
mod holes;
mod labels;
mod lengths;
pub mod model;
mod pairs;
mod parity;
mod place;
pub mod presets;
pub mod registry;
mod silk;
mod sizes;
mod zones;

use std::collections::BTreeSet;

use lamina_geom::fill::ZoneFill;
use lamina_geom::prim::Prim;
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
pub(crate) use lamina_geom::board::item_limits;

pub fn analyze(d: &ProjectData) -> Analysis {
    let ctx = ctx::Ctx::new(d);
    let mut v: Vec<Violation> = model::validate(d);
    v.extend(lengths::check(d));
    v.extend(pairs::check(d));
    zones::run(&ctx, &mut v);
    copper::run(&ctx, &mut v);
    let ratsnest = connect::ratsnest(&ctx, &mut v);
    sizes::run(&ctx, &mut v);
    holes::hole_to_hole(&ctx, &mut v);
    edge::run(&ctx, &mut v);
    place::run(&ctx, &mut v);
    v.extend(silk::check(d, &ctx.g, &ctx.fills));
    v.extend(holes::hole_to_copper(d, &ctx.g));
    v.extend(parity::check(d, &ctx.g));
    v.extend(footprints::check(d));
    v.extend(assembly::check(d, &ctx.g, &ctx.fills));
    v.extend(labels::check(d));
    v.extend(electrical::check(d, &ctx.g, &ctx.fills));
    connect::weak(&ctx, &mut v);
    v.extend(dangling(
        d,
        &ctx.g.copper,
        ctx.n_items,
        &ctx.index,
        &ctx.fill_index,
        &ctx.roots,
    ));
    connect::unplaced(d, &mut v);
    let (violations, excluded, release_only) = finish::run(v, &d.board.rules);
    let c = into_connectivity(ctx, ratsnest);
    Analysis {
        violations,
        excluded,
        release_only,
        ratsnest: c.ratsnest,
        fills: c.fills,
        geometry: c.geometry,
        island: c.island,
        fill_island: c.fill_island,
    }
}

/// The connections of the board, with no checks: the ratsnest, the pours,
/// the geometry and the island of each copper item. The router and the
/// push preview use it, because it is faster than [`analyze`].
pub struct Connectivity {
    pub ratsnest: Vec<Ratsnest>,
    pub fills: Vec<ZoneFill>,
    /// `geometry.copper` has no pour pieces.
    pub geometry: BoardGeometry,
    /// The island of connected copper for each `geometry.copper` item.
    pub island: Vec<usize>,
    /// The island of each fill polygon, for each fill.
    pub fill_island: Vec<Vec<usize>>,
}

pub fn connectivity(d: &ProjectData) -> Connectivity {
    let ctx = ctx::Ctx::new(d);
    let ratsnest = connect::ratsnest(&ctx, &mut Vec::new());
    into_connectivity(ctx, ratsnest)
}

fn into_connectivity(ctx: ctx::Ctx, ratsnest: Vec<Ratsnest>) -> Connectivity {
    let island: Vec<usize> = ctx.roots[..ctx.n_items].to_vec();
    let fill_island = ctx.fill_islands();
    let mut g = ctx.g;
    g.copper.truncate(ctx.n_items);
    Connectivity {
        ratsnest,
        fills: ctx.fills,
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

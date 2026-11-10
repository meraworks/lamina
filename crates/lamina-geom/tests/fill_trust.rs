//! The checks skip pour pieces against other nets, because the fill keeps
//! the limits. These tests measure the fill of the real boards, so that a
//! defect in the fill does not pass with no error.

use std::path::{Path, PathBuf};

use geo::{Area, Buffer, MultiPolygon};
use lamina_geom::fill::{fill_zones, inflate};
use lamina_geom::{Index, Owner, Prim, build};
use lamina_model::*;

fn boards() -> Vec<(String, ProjectData)> {
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    ["tit-flight-computer", "yoink-main-board"]
        .iter()
        .map(|b| (b.to_string(), ProjectData::load(&root.join(b)).unwrap()))
        .collect()
}

/// The smallest clearance that a rule of the board can give.
fn floor(d: &ProjectData, zone: &Zone) -> f64 {
    let r = &d.board.rules;
    let mut least = zone.clearance.unwrap_or(r.clearance).min(r.clearance);
    for c in r.net_classes.values().filter_map(|c| c.clearance) {
        least = least.min(c);
    }
    for c in r.custom.iter().filter_map(|c| c.clearance) {
        least = least.min(c);
    }
    least
}

#[test]
fn fill_keeps_its_limits() {
    const TOL: f64 = 0.001;
    for (name, d) in boards() {
        let g = build(&d);
        let fills = fill_zones(&d, &g);
        assert!(!fills.is_empty(), "{name} has pours");
        let boxes: Vec<Rect> = g.copper.iter().map(|c| c.prim.bbox()).collect();
        let index = Index::new(boxes.iter().copied());
        let dboxes: Vec<Rect> = g.drills.iter().map(|x| x.prim.bbox()).collect();
        let dindex = Index::new(dboxes.iter().copied());
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
        let mut pieces = 0;
        for f in &fills {
            let zone = d.routing.zones.iter().find(|z| z.id == f.zone).unwrap();
            let need = floor(&d, zone);
            let layer = f.layer.copper().unwrap().index(g.layers);
            for poly in &f.polygons {
                pieces += 1;
                let prim = Prim::Poly {
                    outer: poly.outer.clone(),
                    holes: poly.holes.clone(),
                };
                let bbox = prim.bbox();
                for j in index.near(&bbox, need) {
                    let c = &g.copper[j];
                    if c.layer != layer || c.net == f.net || matches!(c.owner, Owner::Zone { .. }) {
                        continue;
                    }
                    let dist = prim.distance(&c.prim);
                    assert!(
                        dist + TOL >= need,
                        "{name}: the pour {} is {dist:.4} mm from {} (limit {need})",
                        f.zone,
                        c.owner.label()
                    );
                }
                for j in dindex.near(&bbox, d.board.rules.hole_to_copper) {
                    let h = &g.drills[j];
                    if h.plated && h.net == f.net {
                        continue;
                    }
                    if layer < h.layers.0.min(h.layers.1) || layer > h.layers.0.max(h.layers.1) {
                        continue;
                    }
                    let dist = prim.distance(&h.prim);
                    assert!(
                        dist + TOL >= d.board.rules.hole_to_copper,
                        "{name}: the pour {} is {dist:.4} mm from the hole of {} (limit {})",
                        f.zone,
                        h.owner.label(),
                        d.board.rules.hole_to_copper
                    );
                }
                let edge = edges
                    .iter()
                    .map(|e| prim.distance(e))
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    edge + TOL >= d.board.rules.copper_to_edge,
                    "{name}: the pour {} is {edge:.4} mm from the board edge (limit {})",
                    f.zone,
                    d.board.rules.copper_to_edge
                );
            }
        }
        assert!(pieces > 0, "{name}");
    }
}

#[test]
fn fill_has_no_thin_necks() {
    // Make each piece smaller by almost half of the minimum width, then
    // larger again. A neck thinner than the minimum width goes away, and
    // the area changes. Only the sharp corners can change the area.
    let mut worst: f64 = 0.0;
    for (name, d) in boards() {
        let g = build(&d);
        for f in fill_zones(&d, &g) {
            let zone = d.routing.zones.iter().find(|z| z.id == f.zone).unwrap();
            let width = zone
                .min_width
                .unwrap_or(d.board.rules.min_track_width)
                .max(0.05);
            let r = 0.4 * width;
            for poly in &f.polygons {
                let prim = Prim::Poly {
                    outer: poly.outer.clone(),
                    holes: poly.holes.clone(),
                };
                let whole: MultiPolygon<f64> = inflate(&prim, 0.0);
                let area = whole.unsigned_area();
                let opened = inflate(&prim, -r).buffer(r).unsigned_area();
                let lost = (area - opened) / area;
                worst = worst.max(lost);
                assert!(
                    lost < 0.005,
                    "{name}: a piece of the pour {} of {area:.2} mm2 loses {:.2} % when thin parts go away",
                    f.zone,
                    100.0 * lost
                );
            }
        }
    }
    eprintln!("worst loss: {:.3} %", 100.0 * worst);
}

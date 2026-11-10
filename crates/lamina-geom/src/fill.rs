//! The copper pour fill: the zone outline minus all items that it must keep clear
//! of. It has thermal spokes to its own pads. Islands with no connection are removed.

use geo::{Area, BooleanOps, Buffer, Coord, LineString, MultiPolygon, Polygon};
use lamina_model::*;
use serde::{Deserialize, Serialize};

use crate::board::{BoardGeometry, Owner};
use crate::prim::{Prim, capsule_points, circle_points};

const TOL: f64 = 0.005;

fn ring(pts: &[Point]) -> LineString<f64> {
    let mut v: Vec<Coord<f64>> = pts.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
    if let Some(f) = v.first().copied() {
        v.push(f);
    }
    LineString::new(v)
}

fn gpoly(pts: &[Point]) -> Polygon<f64> {
    Polygon::new(ring(pts), vec![])
}

fn mp(p: Polygon<f64>) -> MultiPolygon<f64> {
    MultiPolygon::new(vec![p])
}

/// The prim made larger by `d` (exact for circles and capsules).
pub fn inflate(p: &Prim, d: f64) -> MultiPolygon<f64> {
    // Polygon chords must stay outside the requested clearance boundary.
    let d = if d > 0.0 { d + TOL } else { d };
    match p {
        Prim::Circle { c, r } => mp(gpoly(&circle_points(*c, r + d, TOL))),
        Prim::Capsule { a, b, r } => mp(gpoly(&capsule_points(*a, *b, r + d, TOL))),
        Prim::Poly { outer, holes } => {
            let poly = Polygon::new(ring(outer), holes.iter().map(|h| ring(h)).collect());
            if d.abs() < 1e-9 {
                mp(poly)
            } else {
                poly.buffer(d)
            }
        }
    }
}

/// The parts of a board that are narrower than `width`: the middle of each
/// part and its area in mm2. The board is the outline without the cut-outs.
///
/// The board is made smaller by half of the width and then larger again. A
/// narrow part does not come back. A sharp corner loses a small area also,
/// so a part counts only if its area is above the square of half the width.
pub fn narrow_parts(outline: &[Point], cutouts: &[Vec<Point>], width: f64) -> Vec<(Point, f64)> {
    if outline.len() < 3 || width <= 0.0 {
        return Vec::new();
    }
    let r = width / 2.0;
    let mut board = mp(gpoly(outline));
    for c in cutouts.iter().filter(|c| c.len() >= 3) {
        board = board.difference(&mp(gpoly(c)));
    }
    // A little less than half the width, so that a part of exactly the width stays.
    let opened = board.buffer(-0.98 * r).buffer(0.98 * r);
    let lost = board.difference(&opened);
    let mut out = Vec::new();
    for piece in &lost.0 {
        let area = piece.unsigned_area();
        if area <= r * r {
            continue;
        }
        let pts: Vec<_> = piece.exterior().points().collect();
        let n = pts.len().max(1) as f64;
        let at = Point::new(
            pts.iter().map(|p| p.x()).sum::<f64>() / n,
            pts.iter().map(|p| p.y()).sum::<f64>() / n,
        );
        out.push((round_pt(at), area));
    }
    out.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)));
    out
}

fn union_all(items: Vec<MultiPolygon<f64>>) -> MultiPolygon<f64> {
    // A pairwise tree union keeps each step small.
    let mut layer = items;
    while layer.len() > 1 {
        let mut next = Vec::with_capacity(layer.len().div_ceil(2));
        let mut it = layer.into_iter();
        while let Some(a) = it.next() {
            match it.next() {
                Some(b) => next.push(a.union(&b)),
                None => next.push(a),
            }
        }
        layer = next;
    }
    layer.pop().unwrap_or_else(|| MultiPolygon::new(vec![]))
}

fn to_prims(m: &MultiPolygon<f64>) -> Vec<Prim> {
    m.0.iter()
        .filter(|p| p.unsigned_area() > 1e-6)
        .map(|p| {
            let conv = |ls: &LineString<f64>| -> Vec<Point> {
                let mut v: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                if v.len() > 1 && v.first() == v.last() {
                    v.pop();
                }
                v
            };
            let mut outer = conv(p.exterior());
            if crate::prim::ring_area(&outer) < 0.0 {
                outer.reverse();
            }
            let holes = p
                .interiors()
                .iter()
                .map(|h| {
                    let mut v = conv(h);
                    if crate::prim::ring_area(&v) > 0.0 {
                        v.reverse();
                    }
                    v
                })
                .collect();
            Prim::Poly { outer, holes }
        })
        .collect()
}

/// The calculated fill of one zone on one layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneFill {
    #[serde(default)]
    pub pad_thermals: Vec<PadThermal>,
    pub zone: String,
    pub net: Option<String>,
    pub layer: Layer,
    pub polygons: Vec<FillPolygon>,
    #[serde(default)]
    pub thermal_spokes: std::collections::BTreeMap<String, u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadThermal {
    pub pad: String,
    pub at: Point,
    pub spokes: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillPolygon {
    pub outer: Vec<Point>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<Point>>,
}

impl FillPolygon {
    pub fn keyhole(&self) -> Vec<Point> {
        keyhole(&Polygon::new(
            ring(&self.outer),
            self.holes.iter().map(|h| ring(h)).collect(),
        ))
    }
    pub fn prim(&self) -> Prim {
        Prim::Poly {
            outer: self.outer.clone(),
            holes: self.holes.clone(),
        }
    }
}

/// Fills all zones. Zones with a higher priority fill first. They are obstacles
/// for zones of other nets that have a lower priority.
pub fn fill_zones(d: &ProjectData, g: &BoardGeometry) -> Vec<ZoneFill> {
    let rules = &d.board.rules;
    let layers = g.layers;
    let class_of = |net: &Option<String>| {
        net.as_ref()
            .and_then(|n| d.nets.nets.get(n))
            .and_then(|n| n.class.clone())
    };
    let Some(outline) = &g.outline else {
        return Vec::new();
    };
    let board = {
        let mut b = mp(gpoly(outline)).buffer(-rules.copper_to_edge);
        for c in &g.cutouts {
            b = b.difference(&mp(gpoly(c)).buffer(rules.copper_to_edge));
        }
        b
    };

    let mut zones: Vec<&Zone> = d.routing.zones.iter().collect();
    zones.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
    // A pour is an obstacle only for the later pours of its layer. So each
    // copper layer is one job, and the jobs run at the same time.
    let used: std::collections::BTreeSet<u8> = zones
        .iter()
        .flat_map(|z| z.layers.iter())
        .filter_map(|l| l.copper())
        .map(|c| c.index(layers))
        .collect();
    let one_layer = |this: u8| -> Vec<ZoneFill> {
        let mut done: Vec<(u8, Option<String>, MultiPolygon<f64>)> = Vec::new();
        let mut out = Vec::new();
        for z in &zones {
            // The model checks report an outline that is not valid.
            if z.outline.check().is_err() {
                continue;
            }
            let zone_class = class_of(&z.net);
            for layer in &z.layers {
                let Some(c) = layer.copper() else { continue };
                let l = c.index(layers);
                if l != this {
                    continue;
                }
                let limits = rules.resolve(z.net.as_deref(), zone_class.as_deref(), Some(*layer));
                let zc = z
                    .clearance
                    .unwrap_or(limits.clearance)
                    .max(limits.clearance);
                let min_w = z.min_width.unwrap_or(limits.min_track_width).max(0.05);
                let clearance = |net: &Option<String>| {
                    rules
                        .resolve(net.as_deref(), class_of(net).as_deref(), Some(*layer))
                        .clearance
                };
                let region = mp(gpoly(&z.outline.flatten(TOL))).intersection(&board);
                if region.unsigned_area() < 1e-6 {
                    continue;
                }
                let rb = bbox_of(&region);
                let reach = rules
                    .custom
                    .iter()
                    .filter_map(|r| r.clearance)
                    .chain(rules.net_classes.values().filter_map(|c| c.clearance))
                    .fold(zc.max(z.thermal_gap).max(2.0), f64::max);
                let mut obstacles = Vec::new();
                // The gaps around the pads of the net of the zone. Thermal spokes
                // go across them. Spokes do not go across other obstacles.
                let mut gaps = Vec::new();
                let mut thermal = Vec::new();
                let mut anchors = Vec::new();
                for it in g.copper.iter().filter(|i| i.layer == l) {
                    let b = it.prim.bbox();
                    if b.max.x < rb.min.x - reach
                        || b.min.x > rb.max.x + reach
                        || b.max.y < rb.min.y - reach
                        || b.min.y > rb.max.y + reach
                    {
                        continue;
                    }
                    let same = z.net.is_some() && it.net == z.net;
                    if same {
                        anchors.push(it.prim.clone());
                        if matches!(it.owner, Owner::Pad { .. }) {
                            match z.pad_connection {
                                PadConnection::Solid => {}
                                PadConnection::Thermal => {
                                    gaps.push(inflate(&it.prim, z.thermal_gap));
                                    if region.intersection(&inflate(&it.prim, 0.0)).unsigned_area()
                                        > 1e-6
                                    {
                                        thermal.push((it.owner.label(), it.prim.clone()));
                                    }
                                }
                                PadConnection::None => obstacles.push(inflate(&it.prim, zc)),
                            }
                        }
                    } else {
                        let cl = zc.max(clearance(&it.net));
                        obstacles.push(inflate(&it.prim, cl));
                    }
                }
                for dr in &g.drills {
                    if l < dr.layers.0 || l > dr.layers.1 {
                        continue;
                    }
                    if !dr.plated {
                        let clearance = if matches!(dr.prim, Prim::Capsule { .. }) {
                            zc.max(rules.copper_to_edge)
                        } else {
                            zc
                        };
                        obstacles.push(inflate(&dr.prim, clearance.max(rules.hole_to_copper)));
                    } else if !(z.net.is_some() && dr.net == z.net) {
                        obstacles.push(inflate(&dr.prim, rules.hole_to_copper));
                    }
                }
                for k in &d.routing.keepouts {
                    if k.forbid.pours && k.layers.contains(layer) {
                        obstacles.push(mp(gpoly(&k.outline.flatten(TOL))));
                    }
                }
                for (refdes, pl) in &d.placement.placements {
                    let Some(fp) = d
                        .parts
                        .parts
                        .get(refdes)
                        .and_then(|p| d.footprints.get(&p.footprint))
                    else {
                        continue;
                    };
                    for (ls, pts, forbid) in crate::board::part_keepouts(fp, pl, layers) {
                        if forbid.pours && ls.contains(&l) {
                            obstacles.push(mp(gpoly(&pts)));
                        }
                    }
                }
                for (dl, dnet, fill) in &done {
                    if *dl == l && (dnet != &z.net || z.net.is_none()) {
                        obstacles.push(fill.buffer(zc.max(clearance(dnet))));
                    }
                }

                let hard = union_all(obstacles);
                let blocked = if gaps.is_empty() {
                    hard.clone()
                } else {
                    hard.union(&union_all(gaps))
                };
                let mut fill = region.difference(&blocked);
                // Remove necks that are thinner than the minimum width.
                fill = fill.buffer(-min_w / 2.0).buffer(min_w / 2.0);

                let mut counts = vec![0u8; thermal.len()];
                if !thermal.is_empty() {
                    // A spoke keeps clear of all that the pour keeps clear of:
                    // copper of other nets, holes, keep-outs and other pours.
                    let allowed = region.difference(&hard);
                    let spoke = z.thermal_spoke.max(min_w);
                    let mut spokes = Vec::new();
                    // The area around each pad that its spokes can use.
                    let window = |p: &Prim| {
                        let b = p.bbox();
                        let c = Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
                        let hx = b.width() / 2.0 + z.thermal_gap + min_w;
                        let hy = b.height() / 2.0 + z.thermal_gap + min_w;
                        (c, hx, hy)
                    };
                    let window_poly = |(c, hx, hy): (Point, f64, f64)| {
                        mp(gpoly(&[
                            Point::new(c.x - hx, c.y - hy),
                            Point::new(c.x + hx, c.y - hy),
                            Point::new(c.x + hx, c.y + hy),
                            Point::new(c.x - hx, c.y + hy),
                        ]))
                    };
                    // Cut the pour and the allowed area one time, to the windows of
                    // all the pads. Then each pad uses only the pieces near it. The
                    // full pour has thousands of points, and a cut for each pad is slow.
                    let windows = union_all(
                        thermal
                            .iter()
                            .map(|(_, p)| window_poly(window(p)))
                            .collect(),
                    );
                    let allowed_near = windows.intersection(&allowed);
                    let fill_near = windows.intersection(&fill);
                    let near = |m: &MultiPolygon<f64>, bounds: &MultiPolygon<f64>| {
                        let want = bbox_of(bounds);
                        MultiPolygon::new(
                            m.0.iter()
                                .filter(|piece| {
                                    let b = bbox_of(&mp((*piece).clone()));
                                    b.min.x <= want.max.x
                                        && b.max.x >= want.min.x
                                        && b.min.y <= want.max.y
                                        && b.max.y >= want.min.y
                                })
                                .cloned()
                                .collect(),
                        )
                        .intersection(bounds)
                    };
                    for (index, (_, p)) in thermal.iter().enumerate() {
                        let (c, hx, hy) = window(p);
                        let s = spoke / 2.0;
                        let pad = inflate(p, 0.0);
                        let bounds = window_poly((c, hx, hy));
                        let local_allowed = near(&allowed_near, &bounds);
                        let local_fill = near(&fill_near, &bounds);
                        let mut count: u8 = 0;
                        for (x0, y0, x1, y1) in [
                            (c.x, c.y - s, c.x + hx, c.y + s),
                            (c.x - hx, c.y - s, c.x, c.y + s),
                            (c.x - s, c.y, c.x + s, c.y + hy),
                            (c.x - s, c.y - hy, c.x + s, c.y),
                        ] {
                            let arm = mp(gpoly(&[
                                Point::new(x0, y0),
                                Point::new(x1, y0),
                                Point::new(x1, y1),
                                Point::new(x0, y1),
                            ]))
                            .intersection(&local_allowed)
                            // Remove the parts of the arm that are thinner than the
                            // minimum width, as for the pour.
                            .buffer(-0.45 * min_w)
                            .buffer(0.45 * min_w);
                            if arm.0.iter().any(|piece| {
                                let piece = mp(piece.clone());
                                piece.intersection(&pad).unsigned_area() > 1e-6
                                    && piece.intersection(&local_fill).unsigned_area() > 1e-6
                            }) {
                                count += 1;
                            }
                            spokes.push(arm);
                        }
                        counts[index] = count;
                    }
                    let spokes = union_all(spokes);
                    fill = fill.union(&spokes);
                }

                // Keep only the pieces that touch copper of the net of the zone.
                let prims = to_prims(&fill);
                let kept: Vec<Prim> = if z.net.is_some() {
                    prims
                        .into_iter()
                        .filter(|p| anchors.iter().any(|a| p.distance(a) <= 1e-6))
                        .collect()
                } else {
                    prims
                };
                for (index, (_, pad)) in thermal.iter().enumerate() {
                    for track in d
                        .routing
                        .tracks
                        .iter()
                        .filter(|t| t.layer == *layer && Some(&t.net) == z.net.as_ref())
                    {
                        for ends in track.points.windows(2) {
                            let joins = (pad.contains(ends[0])
                                && !pad.contains(ends[1])
                                && kept.iter().any(|p| p.contains(ends[1])))
                                || (pad.contains(ends[1])
                                    && !pad.contains(ends[0])
                                    && kept.iter().any(|p| p.contains(ends[0])));
                            if joins {
                                let count = &mut counts[index];
                                *count = count.saturating_add(1);
                            }
                        }
                    }
                }
                let kept_mp = MultiPolygon::new(
                    kept.iter()
                        .filter_map(|p| match p {
                            Prim::Poly { outer, holes } => Some(Polygon::new(
                                ring(outer),
                                holes.iter().map(|h| ring(h)).collect(),
                            )),
                            _ => None,
                        })
                        .collect(),
                );
                done.push((l, z.net.clone(), kept_mp));
                let pad_thermals: Vec<_> = thermal
                    .iter()
                    .zip(counts)
                    .map(|((pad, prim), spokes)| PadThermal {
                        pad: pad.clone(),
                        at: prim.anchor(),
                        spokes,
                    })
                    .collect();
                let mut thermal_spokes = std::collections::BTreeMap::<String, u8>::new();
                for pad in &pad_thermals {
                    thermal_spokes
                        .entry(pad.pad.clone())
                        .and_modify(|n| *n = (*n).min(pad.spokes))
                        .or_insert(pad.spokes);
                }
                out.push(ZoneFill {
                    pad_thermals,
                    thermal_spokes,
                    zone: z.id.clone(),
                    net: z.net.clone(),
                    layer: *layer,
                    polygons: kept
                        .into_iter()
                        .filter_map(|p| match p {
                            Prim::Poly { outer, holes } => Some(FillPolygon {
                                outer: outer.into_iter().map(round_pt).collect(),
                                holes: holes
                                    .into_iter()
                                    .map(|h| h.into_iter().map(round_pt).collect())
                                    .collect(),
                            }),
                            _ => None,
                        })
                        .collect(),
                });
            }
        }
        out
    };
    let mut out: Vec<ZoneFill> = if used.len() <= 1 {
        used.iter().flat_map(|l| one_layer(*l)).collect()
    } else {
        std::thread::scope(|scope| {
            let handles: Vec<_> = used
                .iter()
                .map(|l| {
                    let job = &one_layer;
                    let l = *l;
                    scope.spawn(move || job(l))
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("the pour of a layer"))
                .collect()
        })
    };
    // The order of the result does not change with the number of threads:
    // the zones in their order, and for each zone its layers in their order.
    let place = |f: &ZoneFill| {
        let zi = zones.iter().position(|z| z.id == f.zone).unwrap_or(0);
        let li = zones
            .get(zi)
            .and_then(|z| z.layers.iter().position(|l| *l == f.layer))
            .unwrap_or(0);
        (zi, li)
    };
    out.sort_by_key(place);
    out
}

fn round_pt(p: Point) -> Point {
    Point::new(json::round6(p.x), json::round6(p.y))
}

fn bbox_of(m: &MultiPolygon<f64>) -> Rect {
    let pts =
        m.0.iter()
            .flat_map(|p| p.exterior().0.iter().map(|c| Point::new(c.x, c.y)));
    Rect::from_points(pts).unwrap_or(Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(0.0, 0.0),
    })
}

/// Convert holes to zero-width cuts in one ring.
pub fn keyhole(p: &Polygon<f64>) -> Vec<Point> {
    let pts = |ls: &geo::LineString<f64>| -> Vec<Point> {
        let mut v: Vec<Point> = ls
            .coords()
            .map(|c| Point::new((c.x * 1e6).round() / 1e6, (c.y * 1e6).round() / 1e6))
            .collect();
        if v.len() > 1 && v[0] == *v.last().unwrap() {
            v.pop();
        }
        v
    };
    // The outer ring is counter-clockwise and the holes are clockwise, so the cuts subtract.
    let mut outer = pts(p.exterior());
    if crate::prim::ring_area(&outer) < 0.0 {
        outer.reverse();
    }
    let mut holes: Vec<Vec<Point>> = p
        .interiors()
        .iter()
        .map(pts)
        .filter(|h| h.len() >= 3)
        .map(|mut h| {
            if crate::prim::ring_area(&h) > 0.0 {
                h.reverse();
            }
            h
        })
        .collect();
    // Do the rightmost holes first, so that their cuts do not cross the cuts of other holes.
    holes.sort_by(|a, b| {
        let mx = |h: &Vec<Point>| h.iter().map(|q| q.x).fold(f64::MIN, f64::max);
        mx(b).total_cmp(&mx(a))
    });
    for h in holes {
        let (hi, hp) = h
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.x.total_cmp(&b.1.x))
            .map(|(i, q)| (i, *q))
            .unwrap();
        // Join the rightmost hole point to the first boundary on its right.
        let hit = (0..outer.len())
            .filter_map(|i| {
                let a = outer[i];
                let b = outer[(i + 1) % outer.len()];
                if (a.y - b.y).abs() < 1e-12 {
                    return None;
                }
                let t = (hp.y - a.y) / (b.y - a.y);
                let x = a.x + t * (b.x - a.x);
                if (0.0..=1.0).contains(&t) && x >= hp.x {
                    Some((i, Point::new(x, hp.y)))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.1.x.total_cmp(&b.1.x));
        let (edge, op) = hit.expect("a polygon hole has an outer edge to its right");
        let oi = edge + 1;
        outer.insert(oi, op);
        let mut ring: Vec<Point> = Vec::with_capacity(outer.len() + h.len() + 2);
        ring.extend_from_slice(&outer[..=oi]);
        for k in 0..=h.len() {
            ring.push(h[(hi + k) % h.len()]);
        }
        ring.push(op);
        ring.extend_from_slice(&outer[oi + 1..]);
        outer = ring;
    }
    outer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_clearance_applies_to_pours_on_the_selected_layer() {
        let mut d = ProjectData::new("custom");
        d.board.outline = Some(Contour::rect(0.0, 0.0, 20.0, 20.0));
        d.routing.tracks.push(Track {
            id: "t".into(),
            net: "BUS1".into(),
            layer: Layer::TOP,
            width: 0.3,
            points: vec![Point::new(5.0, 10.0), Point::new(15.0, 10.0)],
        });
        d.routing.zones.push(Zone {
            id: "z".into(),
            net: None,
            layers: vec![Layer::TOP],
            outline: Contour::rect(1.0, 1.0, 18.0, 18.0),
            priority: 0,
            clearance: None,
            min_width: None,
            pad_connection: PadConnection::Solid,
            thermal_gap: 0.3,
            thermal_spoke: 0.3,
        });
        d.board.rules.custom.push(CustomRule {
            name: "bus".into(),
            when: RuleMatch {
                net: Some("BUS*".into()),
                layer: Some(Layer::TOP),
                ..Default::default()
            },
            clearance: Some(3.0),
            ..Default::default()
        });
        let g = crate::build(&d);
        let fills = fill_zones(&d, &g);
        assert!(!fills[0].polygons.is_empty());
        for polygon in &fills[0].polygons {
            assert!(polygon.prim().distance(&g.copper[0].prim) >= 3.0 - 1e-6);
        }
    }

    #[test]
    fn keyhole_cut_does_not_cross_the_hole() {
        let hole = vec![
            Point::new(2.0, 3.0),
            Point::new(3.0, 2.0),
            Point::new(4.0, 3.0),
            Point::new(3.0, 4.0),
        ];
        let polygon = FillPolygon {
            outer: vec![
                Point::new(0.0, 0.0),
                Point::new(100.0, 0.0),
                Point::new(100.0, 10.0),
                Point::new(0.0, 10.0),
            ],
            holes: vec![hole.clone()],
        };
        let ring = polygon.keyhole();
        for edge in ring.windows(2) {
            if ring
                .windows(2)
                .any(|other| other[0] == edge[1] && other[1] == edge[0])
            {
                for fraction in [0.1, 0.5, 0.9] {
                    let mid = Point::new(
                        edge[0].x + fraction * (edge[1].x - edge[0].x),
                        edge[0].y + fraction * (edge[1].y - edge[0].y),
                    );
                    assert!(!crate::prim::ring_contains(&hole, mid));
                }
            }
        }
    }

    #[test]
    fn routed_holes_keep_the_board_edge_clearance() {
        let mut d = ProjectData::new("slot");
        d.board.outline = Some(Contour::rect(0.0, 0.0, 20.0, 20.0));
        d.board.rules.clearance = 0.1;
        d.board.rules.copper_to_edge = 0.3;
        d.board.holes.push(Hole {
            id: "slot".into(),
            at: Point::new(10.0, 10.0),
            drill: Drill::Slot {
                width: 1.0,
                height: 3.0,
            },
            plated: false,
            pad_diameter: None,
            net: None,
            rotation: 0.0,
        });
        d.routing.zones.push(Zone {
            id: "z".into(),
            net: None,
            layers: vec![Layer::TOP],
            outline: Contour::rect(1.0, 1.0, 18.0, 18.0),
            priority: 0,
            clearance: None,
            min_width: None,
            pad_connection: PadConnection::Solid,
            thermal_gap: 0.3,
            thermal_spoke: 0.3,
        });
        let g = crate::build(&d);
        let fills = fill_zones(&d, &g);
        assert!(!fills[0].polygons.is_empty());
        for polygon in &fills[0].polygons {
            assert!(polygon.prim().distance(&g.drills[0].prim) >= 0.3 - 1e-6);
        }
    }

    #[test]
    fn inflated_round_obstacles_keep_the_requested_clearance() {
        let center = Point::new(0.0, 0.0);
        let obstacle = Prim::Circle { c: center, r: 0.5 };
        let poly = inflate(&obstacle, 0.2);
        for edge in poly.0[0].exterior().0.windows(2) {
            let a = Point::new(edge[0].x, edge[0].y);
            let b = Point::new(edge[1].x, edge[1].y);
            assert!(crate::prim::seg_point(a, b, center) >= 0.7 - 1e-9);
        }
    }
}

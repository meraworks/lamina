//! Board items as copper primitives in board coordinates.

use std::collections::BTreeMap;

use lamina_model::*;

use crate::prim::Prim;

/// The item that a copper primitive belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Owner {
    Pad {
        part: String,
        pad: String,
        index: usize,
    },
    Track {
        id: String,
        seg: usize,
    },
    Via {
        id: String,
    },
    Hole {
        id: String,
    },
    Zone {
        id: String,
        piece: usize,
    },
}

impl Owner {
    /// The id for the item lists of violations.
    pub fn label(&self) -> String {
        match self {
            Owner::Pad { part, pad, .. } => {
                if pad.is_empty() {
                    format!("{part}.(hole)")
                } else {
                    format!("{part}.{pad}")
                }
            }
            Owner::Track { id, .. } => id.clone(),
            Owner::Via { id } => id.clone(),
            Owner::Hole { id } => id.clone(),
            Owner::Zone { id, .. } => id.clone(),
        }
    }

    /// Items with the same body have the same key (a through pad on all layers).
    pub fn body(&self) -> String {
        match self {
            Owner::Pad { part, index, .. } => format!("pad:{part}:{index}"),
            Owner::Track { id, seg } => format!("track:{id}:{seg}"),
            Owner::Via { id } => format!("via:{id}"),
            Owner::Hole { id } => format!("hole:{id}"),
            Owner::Zone { id, piece } => format!("zone:{id}:{piece}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CopperItem {
    /// The copper layer index. 0 is the top.
    pub layer: u8,
    pub net: Option<String>,
    pub prim: Prim,
    pub owner: Owner,
}

/// A drilled hole, for hole-to-hole and hole-to-edge checks.
#[derive(Debug, Clone)]
pub struct DrillItem {
    pub prim: Prim,
    pub plated: bool,
    pub owner: Owner,
    /// The net of the copper around a plated hole.
    pub net: Option<String>,
    /// The first and the last copper layer that the hole goes through.
    pub layers: (u8, u8),
}

/// The pad shape in board coordinates.
pub fn pad_prim(pad: &Pad, pl: &Placement) -> Prim {
    let rot = pad.rotation;
    let local = |p: Point| pl.transform(p.rotated(rot).add(pad.at));
    match &pad.shape {
        PadShape::Circle { diameter } => Prim::Circle {
            c: local(Point::new(0.0, 0.0)),
            r: diameter / 2.0,
        },
        PadShape::Oval { width, height } => {
            let (w, h) = (*width, *height);
            if (w - h).abs() < 1e-9 {
                return Prim::Circle {
                    c: local(Point::new(0.0, 0.0)),
                    r: w / 2.0,
                };
            }
            let (a, b, r) = if w > h {
                let d = (w - h) / 2.0;
                (Point::new(-d, 0.0), Point::new(d, 0.0), h / 2.0)
            } else {
                let d = (h - w) / 2.0;
                (Point::new(0.0, -d), Point::new(0.0, d), w / 2.0)
            };
            Prim::Capsule {
                a: local(a),
                b: local(b),
                r,
            }
        }
        PadShape::Rect { width, height } => {
            let (x, y) = (width / 2.0, height / 2.0);
            poly(
                [(-x, -y), (x, -y), (x, y), (-x, y)]
                    .map(|(a, b)| local(Point::new(a, b)))
                    .to_vec(),
            )
        }
        PadShape::RoundRect {
            width,
            height,
            radius,
        } => {
            let pts = rounded_rect(*width, *height, *radius);
            poly(pts.into_iter().map(local).collect())
        }
        PadShape::Polygon { points } => poly(points.iter().map(|p| local(*p)).collect()),
    }
}

fn rounded_rect(w: f64, h: f64, r: f64) -> Vec<Point> {
    let (x, y) = (w / 2.0, h / 2.0);
    let r = r.min(x).min(y).max(0.0);
    if r < 1e-6 {
        return vec![
            Point::new(-x, -y),
            Point::new(x, -y),
            Point::new(x, y),
            Point::new(-x, y),
        ];
    }
    let mut out = Vec::new();
    let corners = [
        (x - r, -y + r, -90.0),
        (x - r, y - r, 0.0),
        (-x + r, y - r, 90.0),
        (-x + r, -y + r, 180.0),
    ];
    for (cx, cy, start) in corners {
        for k in 0..=4 {
            let a = (start + 22.5 * k as f64).to_radians();
            out.push(Point::new(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    out
}

/// A polygon prim, always counter-clockwise.
pub fn poly(mut pts: Vec<Point>) -> Prim {
    if crate::prim::ring_area(&pts) < 0.0 {
        pts.reverse();
    }
    Prim::Poly {
        outer: pts,
        holes: Vec::new(),
    }
}

/// The copper layer index for an SMD pad on a part on `side`.
pub fn smd_layer(side: Side, layers: u8) -> u8 {
    match side {
        Side::Top => 0,
        Side::Bottom => layers.saturating_sub(1),
    }
}

/// All copper on the board, plus drilled holes.
pub struct BoardGeometry {
    pub copper: Vec<CopperItem>,
    pub drills: Vec<DrillItem>,
    /// The flattened board outline, and the cut-outs.
    pub outline: Option<Vec<Point>>,
    pub cutouts: Vec<Vec<Point>>,
    pub layers: u8,
}

pub fn drill_prim(drill: &Drill, at: Point, rotation: f64) -> Prim {
    match drill {
        Drill::Round { diameter } => Prim::Circle {
            c: at,
            r: diameter / 2.0,
        },
        Drill::Slot { width, height } => {
            let (w, h) = (*width, *height);
            let (a, b, r) = if w >= h {
                let d = (w - h) / 2.0;
                (Point::new(-d, 0.0), Point::new(d, 0.0), h / 2.0)
            } else {
                let d = (h - w) / 2.0;
                (Point::new(0.0, -d), Point::new(0.0, d), w / 2.0)
            };
            Prim::Capsule {
                a: a.rotated(rotation).add(at),
                b: b.rotated(rotation).add(at),
                r,
            }
        }
    }
}

/// The footprint pads of a placed part in board coordinates: (pad, index, prim, layers).
pub fn placed_pads<'a>(
    fp: &'a Footprint,
    pl: &Placement,
    layers: u8,
) -> Vec<(&'a Pad, usize, Prim, Vec<u8>)> {
    fp.pads
        .iter()
        .enumerate()
        .filter_map(|(i, pad)| {
            let prim = pad_prim(pad, pl);
            let ls = match &pad.kind {
                PadKind::Smd => vec![smd_layer(pl.side, layers)],
                PadKind::Through { drill, plated } => {
                    let (w, h) = pad.shape.size();
                    if !*plated && w.min(h) <= drill.min_size() + 1e-6 {
                        Vec::new()
                    } else {
                        (0..layers).collect()
                    }
                }
            };
            (!ls.is_empty()).then_some((pad, i, prim, ls))
        })
        .collect()
}

pub fn build(d: &ProjectData) -> BoardGeometry {
    let layers = d.board.stackup.copper_layers;
    let pad_nets: BTreeMap<PadRef, String> = d.nets.pad_map();
    let mut copper = Vec::new();
    let mut drills = Vec::new();

    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        for (i, pad) in fp.pads.iter().enumerate() {
            if let PadKind::Through { drill, plated } = &pad.kind {
                let at = pl.transform(pad.at);
                let rot = pl.transform_angle(pad.rotation);
                drills.push(DrillItem {
                    prim: drill_prim(drill, at, rot),
                    plated: *plated,
                    owner: Owner::Pad {
                        part: refdes.clone(),
                        pad: pad.number.clone(),
                        index: i,
                    },
                    net: (*plated && !pad.number.is_empty())
                        .then(|| {
                            pad_nets
                                .get(&PadRef::new(refdes.clone(), pad.number.clone()))
                                .cloned()
                        })
                        .flatten(),
                    layers: (0, layers.saturating_sub(1)),
                });
            }
        }
        for (pad, i, prim, ls) in placed_pads(fp, pl, layers) {
            let net = (!pad.number.is_empty())
                .then(|| {
                    pad_nets
                        .get(&PadRef::new(refdes.clone(), pad.number.clone()))
                        .cloned()
                })
                .flatten();
            for l in ls {
                copper.push(CopperItem {
                    layer: l,
                    net: net.clone(),
                    prim: prim.clone(),
                    owner: Owner::Pad {
                        part: refdes.clone(),
                        pad: pad.number.clone(),
                        index: i,
                    },
                });
            }
        }
    }

    for t in &d.routing.tracks {
        let Some(c) = t.layer.copper() else { continue };
        let l = c.index(layers);
        for (k, seg) in t.points.windows(2).enumerate() {
            copper.push(CopperItem {
                layer: l,
                net: Some(t.net.clone()),
                prim: Prim::Capsule {
                    a: seg[0],
                    b: seg[1],
                    r: t.width / 2.0,
                },
                owner: Owner::Track {
                    id: t.id.clone(),
                    seg: k,
                },
            });
        }
        if t.points.len() == 1 {
            copper.push(CopperItem {
                layer: l,
                net: Some(t.net.clone()),
                prim: Prim::Circle {
                    c: t.points[0],
                    r: t.width / 2.0,
                },
                owner: Owner::Track {
                    id: t.id.clone(),
                    seg: 0,
                },
            });
        }
    }

    for v in &d.routing.vias {
        let from = v
            .from
            .and_then(|l| l.copper())
            .map(|c| c.index(layers))
            .unwrap_or(0);
        let to =
            v.to.and_then(|l| l.copper())
                .map(|c| c.index(layers))
                .unwrap_or(layers.saturating_sub(1));
        let (lo, hi) = (from.min(to), from.max(to));
        for l in lo..=hi {
            copper.push(CopperItem {
                layer: l,
                net: Some(v.net.clone()),
                prim: Prim::Circle {
                    c: v.at,
                    r: v.diameter / 2.0,
                },
                owner: Owner::Via { id: v.id.clone() },
            });
        }
        drills.push(DrillItem {
            prim: Prim::Circle {
                c: v.at,
                r: v.drill / 2.0,
            },
            plated: true,
            owner: Owner::Via { id: v.id.clone() },
            net: Some(v.net.clone()),
            layers: (lo, hi),
        });
    }

    for h in &d.board.holes {
        drills.push(DrillItem {
            prim: drill_prim(&h.drill, h.at, h.rotation),
            plated: h.plated,
            owner: Owner::Hole { id: h.id.clone() },
            net: h.net.clone().filter(|_| h.plated),
            layers: (0, layers.saturating_sub(1)),
        });
        if h.plated {
            if let Some(pd) = h.pad_diameter {
                for l in 0..layers {
                    copper.push(CopperItem {
                        layer: l,
                        net: h.net.clone(),
                        prim: Prim::Circle {
                            c: h.at,
                            r: pd / 2.0,
                        },
                        owner: Owner::Hole { id: h.id.clone() },
                    });
                }
            }
        }
    }

    BoardGeometry {
        copper,
        drills,
        // An outline that is not valid counts as absent. The model checks report it.
        outline: d
            .board
            .outline
            .as_ref()
            .filter(|c| c.check().is_ok())
            .map(|c| c.flatten(0.01)),
        cutouts: d
            .board
            .cutouts
            .iter()
            .filter(|c| c.check().is_ok())
            .map(|c| c.flatten(0.01))
            .collect(),
        layers,
    }
}

/// The courtyard polygon of a placed part (the courtyard rect or polygon of the footprint).
pub fn courtyard(fp: &Footprint, pl: &Placement) -> Option<Vec<Point>> {
    let corners = |r: Rect| {
        vec![
            r.min,
            Point::new(r.max.x, r.min.y),
            r.max,
            Point::new(r.min.x, r.max.y),
        ]
    };
    // Courtyards are frequently loose lines and arcs. Join them into a chain.
    let edges: Vec<Edge> = fp
        .graphics
        .iter()
        .filter(|g| g.layer == FpLayer::Courtyard)
        .flat_map(|g| edges_of(&g.shape))
        .collect();
    let (loops, _) = chain_edges(edges, 0.05);
    let cy = match loops.len() {
        0 => corners(fp.courtyard_bbox()?),
        1 => loops[0].flatten(0.02),
        // If there are more pieces, use their convex hull.
        _ => convex_hull(loops.iter().flat_map(|c| c.flatten(0.02)).collect()),
    };
    if cy.len() < 3 {
        return fp
            .courtyard_bbox()
            .map(|r| corners(r).into_iter().map(|p| pl.transform(p)).collect());
    }
    let mut pts: Vec<Point> = cy.into_iter().map(|p| pl.transform(p)).collect();
    if crate::prim::ring_area(&pts) < 0.0 {
        pts.reverse();
    }
    Some(pts)
}

/// The convex hull, counter-clockwise (monotone chain).
pub fn convex_hull(mut pts: Vec<Point>) -> Vec<Point> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
    if pts.len() < 3 {
        return pts;
    }
    let cross =
        |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut h: Vec<Point> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = h.len();
        let it: Box<dyn Iterator<Item = &Point>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in it {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
                h.pop();
            }
            h.push(p);
        }
        h.pop();
    }
    h
}

/// The footprint keep-outs of a placed part: (copper layer indices, outline, forbid).
pub fn part_keepouts(
    fp: &Footprint,
    pl: &Placement,
    layers: u8,
) -> Vec<(Vec<u8>, Vec<Point>, Forbid)> {
    fp.keepouts
        .iter()
        .map(|k| {
            let front = smd_layer(pl.side, layers);
            let back = smd_layer(pl.side.flip(), layers);
            let ls = match k.scope {
                KeepoutScope::AllCopper => (0..layers).collect(),
                KeepoutScope::Front => vec![front],
                KeepoutScope::Back => vec![back],
            };
            let mut pts: Vec<Point> = k
                .outline
                .flatten(0.02)
                .into_iter()
                .map(|p| pl.transform(p))
                .collect();
            if crate::prim::ring_area(&pts) < 0.0 {
                pts.reverse();
            }
            (ls, pts, k.forbid)
        })
        .collect()
}

#[cfg(test)]
mod courtyard_tests {
    use super::*;

    fn fp(shapes: Vec<Shape>) -> Footprint {
        let mut f = Footprint::new(
            "x",
            Provenance::Datasheet {
                document: "t".into(),
                page: None,
                url: None,
            },
        );
        f.graphics = shapes
            .into_iter()
            .map(|shape| FpGraphic {
                layer: FpLayer::Courtyard,
                shape,
                width: 0.05,
                filled: false,
            })
            .collect();
        f
    }

    fn area(p: &[Point]) -> f64 {
        crate::prim::ring_area(p)
    }

    #[test]
    fn loose_lines_make_one_l_shaped_courtyard() {
        let l = |a: (f64, f64), b: (f64, f64)| Shape::Line {
            a: Point::new(a.0, a.1),
            b: Point::new(b.0, b.1),
        };
        // An L shape: 4 x 2 with a 2 x 2 block on top of its left half.
        let f = fp(vec![
            l((0.0, 0.0), (4.0, 0.0)),
            l((4.0, 0.0), (4.0, 2.0)),
            l((2.0, 4.0), (0.0, 4.0)),
            l((4.0, 2.0), (2.0, 2.0)),
            l((2.0, 2.0), (2.0, 4.0)),
            l((0.0, 4.0), (0.0, 0.0)),
        ]);
        let c = courtyard(&f, &Placement::at(0.0, 0.0)).unwrap();
        assert!((area(&c) - 12.0).abs() < 1e-6, "{c:?}");
    }

    #[test]
    fn round_courtyard_is_round() {
        let f = fp(vec![Shape::Circle {
            center: Point::new(0.0, 0.0),
            radius: 3.45,
        }]);
        let c = courtyard(&f, &Placement::at(0.0, 0.0)).unwrap();
        let want = std::f64::consts::PI * 3.45 * 3.45;
        assert!((area(&c) - want).abs() < 0.4, "{}", area(&c));
        // A corner of the bounding square is outside.
        assert!(!c.iter().any(|p| p.x > 3.0 && p.y > 3.0));
    }

    #[test]
    fn hull_of_scattered_points() {
        let h = convex_hull(vec![
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
            Point::new(0.0, 2.0),
        ]);
        assert_eq!(h.len(), 4);
        assert!((area(&h) - 4.0).abs() < 1e-9);
    }
}

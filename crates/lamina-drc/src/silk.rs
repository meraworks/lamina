//! Silkscreen and solder mask checks.

use std::collections::BTreeMap;

use lamina_geom::prim::circle_points;
use lamina_geom::{BoardGeometry, Index, Prim, font};
use lamina_model::*;

use crate::r3;

/// One drawing on the silkscreen. It is one text or one graphic.
struct SilkItem {
    side: Side,
    id: String,
    name: String,
    prims: Vec<Prim>,
    /// The part, if this is a drawing of its footprint. A footprint outline
    /// needs to clear only the mask openings of its own pads.
    outline_of: Option<String>,
    is_text: bool,
}

struct MaskPad {
    side: Side,
    label: String,
    prim: Prim,
    part: String,
    number: String,
    net: Option<String>,
    margin: f64,
    need: f64,
}

fn stroke(pts: &[Point], width: f64, closed: bool) -> Vec<Prim> {
    let r = width / 2.0;
    let capsule = |a: Point, b: Point| Prim::Capsule { a, b, r };
    let mut out: Vec<Prim> = pts.windows(2).map(|w| capsule(w[0], w[1])).collect();
    if let (true, Some(first), Some(last)) = (closed && pts.len() > 2, pts.first(), pts.last()) {
        out.push(capsule(*last, *first));
    }
    out
}

/// Warn when a silkscreen text overlaps other silkscreen. Graphics can touch
/// each other, because a logo is often made from many graphics.
fn overlaps(silk: &[SilkItem]) -> Vec<Violation> {
    let owners: Vec<(usize, &Prim)> = silk
        .iter()
        .enumerate()
        .flat_map(|(i, s)| s.prims.iter().map(move |p| (i, p)))
        .collect();
    let index = Index::new(owners.iter().map(|(_, p)| p.bbox()));
    let mut pairs: BTreeMap<(usize, usize), Point> = BTreeMap::new();
    for (k, (i, prim)) in owners.iter().enumerate() {
        for j in index.near(&prim.bbox(), 0.0) {
            let (other, q) = owners[j];
            let (a, b) = (&silk[*i], &silk[other]);
            if j <= k || other == *i || a.side != b.side || a.id == b.id {
                continue;
            }
            if (a.is_text || b.is_text) && prim.distance(q) <= 1e-6 {
                pairs
                    .entry(((*i).min(other), (*i).max(other)))
                    .or_insert(prim.anchor());
            }
        }
    }
    pairs
        .into_iter()
        .map(|((i, j), at)| {
            let layer = Layer::Silk(silk[i].side);
            Violation::warning(
                "silk_overlap",
                format!("{} overlaps {} on {layer}", silk[i].name, silk[j].name),
            )
            .at(at)
            .on(layer)
            .items([silk[i].id.clone(), silk[j].id.clone()])
        })
        .collect()
}

/// Warn when silkscreen is nearer to the board edge than `need`.
fn edge_clearance(silk: &[SilkItem], g: &BoardGeometry, need: f64) -> Vec<Violation> {
    let mut edges = Vec::new();
    for ring in g.outline.iter().chain(g.cutouts.iter()) {
        edges.extend(stroke(ring, 0.0, true));
    }
    let index = Index::new(edges.iter().map(Prim::bbox));
    let mut out = Vec::new();
    for item in silk {
        let mut nearest: Option<(f64, Point)> = None;
        for prim in &item.prims {
            for k in index.near(&prim.bbox(), need) {
                let dist = prim.distance(&edges[k]);
                if nearest.is_none_or(|n| dist < n.0) {
                    nearest = Some((dist, prim.anchor()));
                }
            }
        }
        if let Some((dist, at)) = nearest.filter(|n| n.0 + 1e-6 < need) {
            let layer = Layer::Silk(item.side);
            out.push(
                Violation::warning(
                    "silk_to_edge",
                    format!(
                        "{} is {} mm from the board edge, but the minimum is {} mm",
                        item.name,
                        r3(dist),
                        r3(need)
                    ),
                )
                .at(at)
                .on(layer)
                .items([item.id.clone()]),
            );
        }
    }
    out
}

/// Warn when board silkscreen text is too small for the fab to print.
fn text_size(d: &ProjectData) -> Vec<Violation> {
    let min = d.board.rules.min_silk_text_height;
    d.routing
        .texts
        .iter()
        .filter(|t| matches!(t.layer, Layer::Silk(_)) && t.size + 1e-6 < min)
        .map(|t| {
            Violation::warning(
                "silk_text_size",
                format!(
                    "The silkscreen text \"{}\" is {} mm high, but the minimum is {} mm",
                    t.text,
                    r3(t.size),
                    r3(min)
                ),
            )
            .at(t.at)
            .on(t.layer)
            .items([t.id.clone()])
        })
        .collect()
}

/// Warn when too little solder mask stays between pads of two nets.
fn mask_bridges(pads: &[MaskPad], min_web: f64) -> Vec<Violation> {
    let index = Index::new(pads.iter().map(|p| p.prim.bbox()));
    let reach = 2.0 * pads.iter().map(|p| p.margin).fold(0.0, f64::max) + min_web;
    let mut out = Vec::new();
    for (i, a) in pads.iter().enumerate() {
        for j in index.near(&a.prim.bbox(), reach) {
            let b = &pads[j];
            let same_pad = a.part == b.part && a.number == b.number;
            if j <= i || a.side != b.side || same_pad || (a.net.is_some() && a.net == b.net) {
                continue;
            }
            let web = a.prim.distance(&b.prim) - a.margin - b.margin;
            if web + 1e-6 >= min_web {
                continue;
            }
            let layer = Layer::Mask(a.side);
            out.push(
                Violation::warning(
                    "mask_bridge",
                    format!(
                        "The mask between pads {} and {} is {} mm wide, but the minimum is {} mm",
                        a.label,
                        b.label,
                        r3(web),
                        r3(min_web)
                    ),
                )
                .at(a.prim.anchor())
                .on(layer)
                .items([a.label.clone(), b.label.clone()]),
            );
        }
    }
    out
}

fn poly(pts: Vec<Point>) -> Vec<Prim> {
    vec![Prim::Poly {
        outer: pts,
        holes: Vec::new(),
    }]
}

fn shape_prims(shape: &Shape, width: f64, filled: bool, tf: &dyn Fn(Point) -> Point) -> Vec<Prim> {
    let w = width.max(0.02);
    let mapped = |pts: Vec<Point>| -> Vec<Point> { pts.into_iter().map(tf).collect() };
    match shape {
        Shape::Line { .. } | Shape::Arc { .. } => stroke(&mapped(shape.points()), w, false),
        Shape::Circle { center, radius } if filled => vec![Prim::Circle {
            c: tf(*center),
            r: *radius,
        }],
        Shape::Circle { center, radius } => {
            stroke(&mapped(circle_points(*center, *radius, 0.01)), w, true)
        }
        Shape::Rect { min, max } => {
            let pts = mapped(vec![
                *min,
                Point::new(max.x, min.y),
                *max,
                Point::new(min.x, max.y),
            ]);
            if filled {
                poly(pts)
            } else {
                stroke(&pts, w, true)
            }
        }
        Shape::Polygon { points } => {
            let pts = mapped(points.clone());
            if filled {
                poly(pts)
            } else {
                stroke(&pts, w, true)
            }
        }
    }
}

fn text_prims(
    text: &str,
    at: Point,
    size: f64,
    rotation: f64,
    width: f64,
    side: Side,
) -> Vec<Prim> {
    font::side_text_strokes(text, at, size, rotation, side)
        .iter()
        .flat_map(|s| stroke(s, width, false))
        .collect()
}

fn collect(d: &ProjectData) -> (Vec<SilkItem>, Vec<MaskPad>) {
    let rules = &d.board.rules;
    let layers = d.board.stackup.copper_layers;
    let mut silk = Vec::new();
    let mut pads = Vec::new();
    let nets: BTreeMap<String, &String> = d
        .nets
        .nets
        .iter()
        .flat_map(|(name, net)| net.pads.iter().map(move |p| (p.to_string(), name)))
        .collect();
    for (refdes, pl) in &d.placement.placements {
        let Some(fp) = d
            .parts
            .parts
            .get(refdes)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            continue;
        };
        for (pad, _, prim, _) in lamina_geom::placed_pads(fp, pl, layers) {
            if !pad.mask {
                continue;
            }
            let margin = pad.mask_margin.unwrap_or(rules.mask_margin);
            let need = rules.silk_to_pad.max(margin);
            let label = if pad.number.is_empty() {
                refdes.clone()
            } else {
                format!("{refdes}.{}", pad.number)
            };
            let sides = if pad.is_smd() {
                vec![pl.side]
            } else {
                vec![Side::Top, Side::Bottom]
            };
            for side in sides {
                pads.push(MaskPad {
                    side,
                    label: label.clone(),
                    prim: prim.clone(),
                    part: refdes.clone(),
                    number: pad.number.clone(),
                    net: nets.get(&label).map(|n| (*n).clone()),
                    margin,
                    need,
                });
            }
        }
        let tf = |p: Point| pl.transform(p);
        for gr in fp.graphics.iter().filter(|g| g.layer == FpLayer::Silk) {
            silk.push(SilkItem {
                side: pl.side,
                id: refdes.clone(),
                name: format!("The silkscreen drawing of {refdes}"),
                prims: shape_prims(&gr.shape, gr.width, gr.filled, &tf),
                outline_of: Some(refdes.clone()),
                is_text: false,
            });
        }
        let value = d.parts.parts[refdes].value.as_deref().unwrap_or("");
        for t in fp.texts.iter().filter(|t| t.layer == FpLayer::Silk) {
            let text = t.text.replace("${ref}", refdes).replace("${value}", value);
            if text.trim().is_empty() {
                continue;
            }
            silk.push(SilkItem {
                side: pl.side,
                id: refdes.clone(),
                name: format!("The silkscreen text \"{text}\""),
                prims: text_prims(
                    &text,
                    pl.transform(t.at),
                    t.size,
                    font::upright(t.rotation + pl.rotation),
                    font::silk_stroke_width(t.size, rules.silk_width),
                    pl.side,
                ),
                outline_of: None,
                is_text: true,
            });
        }
    }
    for t in &d.routing.texts {
        let Layer::Silk(side) = t.layer else { continue };
        silk.push(SilkItem {
            side,
            id: t.id.clone(),
            name: format!("The silkscreen text \"{}\"", t.text),
            prims: text_prims(
                &t.text,
                t.at,
                t.size,
                t.rotation,
                font::silk_stroke_width(t.size, rules.silk_width),
                side,
            ),
            outline_of: None,
            is_text: true,
        });
    }
    for gr in &d.routing.graphics {
        let Layer::Silk(side) = gr.layer else {
            continue;
        };
        silk.push(SilkItem {
            side,
            id: gr.id.clone(),
            name: format!("The silkscreen drawing {}", gr.id),
            prims: shape_prims(&gr.shape, gr.width, gr.filled, &|p| p),
            outline_of: None,
            is_text: false,
        });
    }
    (silk, pads)
}

/// Warn when silkscreen is nearer to a pad than `silk_to_pad`. The fab removes
/// or clips silkscreen that reaches a mask opening.
pub(crate) fn check(d: &ProjectData, g: &BoardGeometry) -> Vec<Violation> {
    let (silk, pads) = collect(d);
    let mut more = overlaps(&silk);
    more.extend(edge_clearance(&silk, g, d.board.rules.silk_to_edge));
    more.extend(text_size(d));
    more.extend(mask_bridges(&pads, d.board.rules.min_mask_web));
    let index = Index::new(pads.iter().map(|p| p.prim.bbox()));
    let reach = pads.iter().map(|p| p.need).fold(0.0, f64::max);
    let mut out = Vec::new();
    for item in &silk {
        // The nearest distance from this item to each pad.
        let mut nearest: BTreeMap<usize, f64> = BTreeMap::new();
        for prim in &item.prims {
            for j in index.near(&prim.bbox(), reach) {
                if pads[j].side != item.side {
                    continue;
                }
                let dist = prim.distance(&pads[j].prim);
                nearest
                    .entry(j)
                    .and_modify(|d| *d = d.min(dist))
                    .or_insert(dist);
            }
        }
        for (j, dist) in nearest {
            let pad = &pads[j];
            let need = if item.outline_of.as_deref() == Some(pad.part.as_str()) {
                pad.margin
            } else {
                pad.need
            };
            if dist + 1e-6 >= need {
                continue;
            }
            let layer = Layer::Silk(item.side);
            out.push(
                Violation::warning(
                    "silk_to_pad",
                    format!(
                        "{} is {} mm from pad {} on {layer}, but the minimum is {} mm",
                        item.name,
                        r3(dist),
                        pad.label,
                        r3(need)
                    ),
                )
                .at(pad.prim.anchor())
                .on(layer)
                .items([pad.label.clone(), item.id.clone()]),
            );
        }
    }
    out.extend(more);
    out
}

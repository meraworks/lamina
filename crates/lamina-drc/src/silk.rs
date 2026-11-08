//! Silkscreen clearance to pads with a mask opening.

use std::collections::BTreeMap;

use lamina_geom::prim::circle_points;
use lamina_geom::{Index, Prim, font};
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
}

struct MaskPad {
    side: Side,
    label: String,
    prim: Prim,
    part: String,
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
        });
    }
    (silk, pads)
}

/// Warn when silkscreen is nearer to a pad than `silk_to_pad`. The fab removes
/// or clips silkscreen that reaches a mask opening.
pub(crate) fn check(d: &ProjectData) -> Vec<Violation> {
    let (silk, pads) = collect(d);
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
    out
}

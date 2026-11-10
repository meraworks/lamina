//! Imports EasyEDA (standard edition) PCB documents into project data. To get
//! such a document, use "File → Export → EasyEDA" in EasyEDA.
//!
//! Units are 10 mil and Y points down. Footprint children have absolute
//! coordinates. The placement changes them back to footprint-local
//! coordinates.

use std::collections::BTreeMap;

use lamina_model::*;
use serde_json::Value;

use crate::easyeda::{
    Ctx, ModelInfo, RawPad, U, f, footprint_id, graphic_layer, model_info, pad_any, path_points,
    points, s, svg_arc,
};

/// A board read from EasyEDA.
#[derive(Debug)]
pub struct ImportedBoard {
    pub data: ProjectData,
    /// Maps a footprint id to its EasyEDA 3D model. The caller downloads the models.
    pub models: BTreeMap<String, ModelInfo>,
    pub notes: Vec<String>,
}

fn r6(v: f64) -> f64 {
    json::round6(v)
}

/// The board layer for an EasyEDA layer id, if Lamina keeps that layer.
fn board_layer(id: &str, copper: u8) -> Option<Layer> {
    Some(match id {
        "1" => Layer::TOP,
        "2" => Layer::BOTTOM,
        "3" => Layer::Silk(Side::Top),
        "4" => Layer::Silk(Side::Bottom),
        "5" => Layer::Paste(Side::Top),
        "6" => Layer::Paste(Side::Bottom),
        "7" => Layer::Mask(Side::Top),
        "8" => Layer::Mask(Side::Bottom),
        "10" => Layer::Outline,
        "13" => Layer::Fab(Side::Top),
        "14" => Layer::Fab(Side::Bottom),
        _ => {
            let n: u8 = id.parse().ok()?;
            if (21..=52).contains(&n) {
                let l = Layer::Copper(Copper::Inner(n - 20));
                if l.exists_in(copper) { l } else { return None }
            } else {
                return None;
            }
        }
    })
}

fn is_bottom_layer(id: &str) -> bool {
    matches!(id, "2" | "4" | "6" | "8" | "14")
}

/// Parse the `key`value`key`value` attribute string of a LIB record.
fn attrs(text: &str) -> BTreeMap<String, String> {
    let parts: Vec<&str> = text.split('`').collect();
    parts
        .chunks(2)
        .filter(|c| c.len() == 2 && !c[0].is_empty())
        .map(|c| (c[0].to_string(), c[1].to_string()))
        .collect()
}

pub fn import_board(doc: &Value, name: &str) -> Result<ImportedBoard, String> {
    let head = doc.get("head").ok_or("the document has no head")?;
    if head.get("docType").and_then(Value::as_str) != Some("3") {
        return Err("this is not an EasyEDA PCB document (docType is not 3)".into());
    }
    let num = |v: Option<&Value>| -> f64 {
        match v {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(Value::String(s)) => s.parse().unwrap_or(0.0),
            _ => 0.0,
        }
    };
    let ctx = Ctx {
        ox: num(head.get("x")),
        oy: num(head.get("y")),
    };
    let shapes: Vec<&str> = doc
        .get("shape")
        .and_then(Value::as_array)
        .ok_or("the document has no shape list")?
        .iter()
        .filter_map(Value::as_str)
        .collect();

    let mut d = ProjectData::new(name);
    let mut notes = Vec::new();
    let mut models = BTreeMap::new();

    // Get the number of copper layers from the inner layers that are in use.
    let mut max_inner = 0u8;
    for sh in &shapes {
        let fields: Vec<&str> = sh.split('~').collect();
        let layer_field = match fields[0] {
            "TRACK" | "ARC" | "COPPERAREA" => 2,
            "SOLIDREGION" => 1,
            "PAD" => 6,
            _ => continue,
        };
        if let Ok(n) = s(&fields, layer_field).parse::<u8>() {
            if (21..=52).contains(&n) {
                max_inner = max_inner.max(n - 20);
            }
        }
    }
    let copper = 2 + max_inner + (max_inner % 2);
    d.board.stackup.copper_layers = copper;
    if let Some(rule) = doc.get("DRCRULE").and_then(|r| r.get("Default")) {
        let g = |k: &str| rule.get(k).and_then(Value::as_f64).map(|v| r6(v * U));
        if let Some(v) = g("trackWidth") {
            d.board.rules.track_width = v;
        }
        if let Some(v) = g("clearance") {
            d.board.rules.clearance = v;
        }
        if let (Some(dia), Some(drill)) = (g("viaHoleDiameter"), g("viaHoleD")) {
            if drill > 0.0 && dia > drill {
                d.board.rules.via_diameter = dia;
                d.board.rules.via_drill = drill;
                d.board.rules.min_via_drill = drill.min(d.board.rules.min_via_drill);
                d.board.rules.min_drill = drill.min(d.board.rules.min_drill);
                d.board.rules.min_annular_ring =
                    r6(((dia - drill) / 2.0).min(d.board.rules.min_annular_ring));
            }
        }
        if let Some(v) = g("trackWidth") {
            d.board.rules.min_track_width = v.min(d.board.rules.min_track_width);
        }
    }
    // Use the EasyEDA defaults for the values that its rule set does not give.
    d.board.rules.hole_to_hole = d.board.rules.hole_to_hole.min(0.254);
    d.board.rules.preset = None;

    let mut nets: BTreeMap<String, Net> = BTreeMap::new();
    let ensure = |nets: &mut BTreeMap<String, Net>, n: &str| -> bool {
        let n = n.trim();
        if n.is_empty() || !valid_net_name(n) {
            return false;
        }
        nets.entry(n.to_string()).or_default();
        true
    };
    let mut edges: Vec<Edge> = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut ids = Ids::default();
    let mut used_refs: BTreeMap<String, usize> = BTreeMap::new();

    for sh in &shapes {
        let fields: Vec<&str> = sh.split('~').collect();
        match fields[0] {
            "LIB" => {
                let (children_src, head_fields) = match sh.split_once("#@$") {
                    Some((h, rest)) => (rest, h.split('~').collect::<Vec<&str>>()),
                    None => ("", fields.clone()),
                };
                match lib_instance(&ctx, &head_fields, children_src, copper) {
                    Ok(inst) => {
                        let mut refdes = inst.refdes.clone();
                        if !valid_refdes(&refdes) {
                            refdes = format!("X{}", ids.next("x"));
                            notes.push(format!("renamed `{}` to {refdes}", inst.refdes));
                        }
                        let count = used_refs.entry(refdes.clone()).or_default();
                        *count += 1;
                        if *count > 1 {
                            let r = format!("{refdes}_{count}");
                            notes.push(format!("renamed the duplicate reference {refdes} to {r}"));
                            refdes = r;
                        }
                        for n in inst.notes {
                            *skipped.entry(format!("{}: {n}", inst.package)).or_default() += 1;
                        }
                        let fp_id = register_footprint(&mut d, inst.footprint);
                        if let Some(m) = inst.model {
                            models.entry(fp_id.clone()).or_insert(m);
                        }
                        let mut part = Part::new(fp_id);
                        part.value = inst.value;
                        part.lcsc = inst
                            .attrs
                            .get("BOM_Supplier Part")
                            .cloned()
                            .map(|v| v.trim().to_ascii_uppercase())
                            .filter(|v| !v.is_empty());
                        part.mpn = inst
                            .attrs
                            .get("BOM_Manufacturer Part")
                            .cloned()
                            .filter(|v| !v.is_empty());
                        part.manufacturer = inst
                            .attrs
                            .get("BOM_Manufacturer")
                            .cloned()
                            .filter(|v| !v.is_empty());
                        d.parts.parts.insert(refdes.clone(), part);
                        d.placement
                            .placements
                            .insert(refdes.clone(), inst.placement);
                        for (pad, net) in inst.pad_nets {
                            if ensure(&mut nets, &net) {
                                nets.get_mut(net.trim())
                                    .unwrap()
                                    .pads
                                    .push(PadRef::new(refdes.clone(), pad));
                            }
                        }
                    }
                    Err(e) => notes.push(format!("skipped a footprint: {e}")),
                }
            }
            "TRACK" => {
                let layer_id = s(&fields, 2);
                let width = r6(f(&fields, 1) * U);
                let pts = points(&ctx, s(&fields, 4));
                if pts.len() < 2 {
                    continue;
                }
                match board_layer(layer_id, copper) {
                    Some(Layer::Outline) => {
                        for w in pts.windows(2) {
                            edges.push(Edge::Line(w[0], w[1]));
                        }
                    }
                    Some(l) if l.is_copper() => {
                        let net = s(&fields, 3).trim().to_string();
                        if !ensure(&mut nets, &net) {
                            *skipped
                                .entry("copper tracks without a net".into())
                                .or_default() += 1;
                            continue;
                        }
                        d.routing.tracks.push(Track {
                            id: format!("t{}", ids.next("t")),
                            net,
                            layer: l,
                            width: width.max(0.01),
                            points: pts,
                        });
                    }
                    Some(l) if matches!(l, Layer::Silk(_) | Layer::Fab(_)) => {
                        for w in pts.windows(2) {
                            d.routing.graphics.push(BoardGraphic {
                                id: format!("g{}", ids.next("g")),
                                layer: l,
                                shape: Shape::Line { a: w[0], b: w[1] },
                                width: width.max(0.05),
                                filled: false,
                            });
                        }
                    }
                    _ => {}
                }
            }
            "ARC" => {
                let layer_id = s(&fields, 2);
                let Some(shape) = svg_arc(&ctx, s(&fields, 4)) else {
                    continue;
                };
                match board_layer(layer_id, copper) {
                    Some(Layer::Outline) => edges.extend(edges_of(&shape)),
                    Some(l) if l.is_copper() => {
                        let net = s(&fields, 3).trim().to_string();
                        if !ensure(&mut nets, &net) {
                            continue;
                        }
                        if let Shape::Arc {
                            center,
                            start,
                            angle,
                        } = shape
                        {
                            let k = ((angle.abs() / 10.0).ceil() as usize).clamp(2, 36);
                            let pts: Vec<Point> = (0..=k)
                                .map(|i| {
                                    start
                                        .sub(center)
                                        .rotated(angle * i as f64 / k as f64)
                                        .add(center)
                                })
                                .map(|p| Point::new(r6(p.x), r6(p.y)))
                                .collect();
                            d.routing.tracks.push(Track {
                                id: format!("t{}", ids.next("t")),
                                net,
                                layer: l,
                                width: r6(f(&fields, 1) * U).max(0.01),
                                points: pts,
                            });
                        }
                    }
                    Some(l) if matches!(l, Layer::Silk(_) | Layer::Fab(_)) => {
                        d.routing.graphics.push(BoardGraphic {
                            id: format!("g{}", ids.next("g")),
                            layer: l,
                            shape,
                            width: r6(f(&fields, 1) * U).max(0.05),
                            filled: false,
                        });
                    }
                    _ => {}
                }
            }
            "CIRCLE" => {
                // CIRCLE~cx~cy~r~width~layer~gId~locked
                let layer_id = s(&fields, 5);
                let radius = r6(f(&fields, 3) * U);
                if radius <= 0.0 {
                    continue;
                }
                let center = ctx.pt(f(&fields, 1), f(&fields, 2));
                match board_layer(layer_id, copper) {
                    Some(Layer::Outline) => edges.push(Edge::Loop(Contour::circle(center, radius))),
                    Some(l) if matches!(l, Layer::Silk(_) | Layer::Fab(_)) => {
                        d.routing.graphics.push(BoardGraphic {
                            id: format!("g{}", ids.next("g")),
                            layer: l,
                            shape: Shape::Circle { center, radius },
                            width: r6(f(&fields, 4) * U).max(0.05),
                            filled: false,
                        });
                    }
                    _ => {}
                }
            }
            "RECT" => {
                // RECT~x~y~w~h~layer~gId~...
                let layer_id = s(&fields, 5);
                let a = ctx.pt(f(&fields, 1), f(&fields, 2));
                let b = ctx.pt(f(&fields, 1) + f(&fields, 3), f(&fields, 2) + f(&fields, 4));
                let shape = Shape::Rect {
                    min: Point::new(a.x.min(b.x), a.y.min(b.y)),
                    max: Point::new(a.x.max(b.x), a.y.max(b.y)),
                };
                match board_layer(layer_id, copper) {
                    Some(Layer::Outline) => edges.extend(edges_of(&shape)),
                    Some(l) if matches!(l, Layer::Silk(_) | Layer::Fab(_)) => {
                        d.routing.graphics.push(BoardGraphic {
                            id: format!("g{}", ids.next("g")),
                            layer: l,
                            shape,
                            width: 0.15,
                            filled: false,
                        });
                    }
                    _ => {}
                }
            }
            "VIA" => {
                // VIA~x~y~diameter~net~holeR~gId~locked
                let net = s(&fields, 4).trim().to_string();
                if !ensure(&mut nets, &net) {
                    *skipped.entry("vias without a net".into()).or_default() += 1;
                    continue;
                }
                let diameter = r6(f(&fields, 3) * U);
                let drill = r6(f(&fields, 5) * 2.0 * U);
                if !(drill > 0.0 && diameter > drill) {
                    *skipped.entry("vias with a bad size".into()).or_default() += 1;
                    continue;
                }
                d.routing.vias.push(Via {
                    id: format!("v{}", ids.next("v")),
                    net,
                    at: ctx.pt(f(&fields, 1), f(&fields, 2)),
                    diameter,
                    drill,
                    from: None,
                    to: None,
                });
            }
            "HOLE" => {
                // HOLE~x~y~r~gId~locked
                let dia = r6(f(&fields, 3) * 2.0 * U);
                if dia <= 0.0 {
                    continue;
                }
                d.board.holes.push(Hole {
                    id: format!("h{}", ids.next("h")),
                    at: ctx.pt(f(&fields, 1), f(&fields, 2)),
                    drill: Drill::Round { diameter: dia },
                    plated: false,
                    pad_diameter: None,
                    net: None,
                    rotation: 0.0,
                });
            }
            "PAD" => {
                // This pad is not in a footprint. Put it in a footprint with one pad.
                match pad_any(&ctx, &fields) {
                    Ok(raw) => {
                        let n = ids.next("p");
                        let at = raw.pad.at;
                        let side = raw.side.unwrap_or(Side::Top);
                        let mut pad = raw.pad;
                        pad.at = Point::new(0.0, 0.0);
                        if let PadShape::Polygon { points } = &mut pad.shape {
                            if side == Side::Bottom {
                                for p in points.iter_mut() {
                                    p.x = -p.x;
                                }
                            }
                        }
                        if pad.number.is_empty() {
                            pad.number = "1".into();
                        }
                        let number = pad.number.clone();
                        let mut fp = Footprint::new(
                            format!("FreePad_{n}"),
                            Provenance::Manual {
                                note: "free pad from an EasyEDA board".into(),
                            },
                        );
                        fp.pads.push(pad);
                        d.footprints.insert(fp.id.clone(), fp);
                        let refdes = format!("PAD{n}");
                        d.parts
                            .parts
                            .insert(refdes.clone(), Part::new(format!("FreePad_{n}")));
                        d.placement.placements.insert(
                            refdes.clone(),
                            Placement {
                                x: at.x,
                                y: at.y,
                                rotation: 0.0,
                                side,
                                locked: false,
                            },
                        );
                        if ensure(&mut nets, &raw.net) {
                            nets.get_mut(raw.net.trim())
                                .unwrap()
                                .pads
                                .push(PadRef::new(refdes, number));
                        }
                    }
                    Err(e) => notes.push(format!("skipped a free pad: {e}")),
                }
            }
            "COPPERAREA" => {
                // COPPERAREA~strokeWidth~layer~net~path~clearance~fillStyle~gId~thermal~keepIsland~fill~locked~...
                let Some(layer) = board_layer(s(&fields, 2), copper) else {
                    continue;
                };
                if !layer.is_copper() {
                    continue;
                }
                let pts = path_points(&ctx, s(&fields, 4));
                let Some(outline) = contour_from_points(pts) else {
                    *skipped
                        .entry("copper areas with an outline that is not valid".into())
                        .or_default() += 1;
                    continue;
                };
                let net = s(&fields, 3).trim().to_string();
                let net = if ensure(&mut nets, &net) {
                    Some(net)
                } else {
                    None
                };
                d.routing.zones.push(Zone {
                    id: format!("z{}", ids.next("z")),
                    net,
                    layers: vec![layer],
                    outline,
                    priority: 0,
                    clearance: Some(r6(f(&fields, 5) * U)).filter(|c| *c > 0.0),
                    min_width: Some(r6(f(&fields, 1) * U)).filter(|w| *w > 0.0),
                    pad_connection: if s(&fields, 8) == "direct" {
                        PadConnection::Solid
                    } else {
                        PadConnection::Thermal
                    },
                    thermal_gap: 0.3,
                    thermal_spoke: 0.3,
                });
            }
            "SOLIDREGION" => {
                // SOLIDREGION~layer~net~path~type~gId~locked
                let layer_id = s(&fields, 1);
                let kind = s(&fields, 4);
                let pts = path_points(&ctx, s(&fields, 3));
                if pts.len() < 3 {
                    continue;
                }
                match (kind, board_layer(layer_id, copper)) {
                    ("npth", _) => {
                        if let Some(c) = contour_from_points(pts) {
                            d.board.cutouts.push(c);
                        }
                    }
                    ("cutout", Some(l)) if l.is_copper() => {
                        if let Some(outline) = contour_from_points(pts) {
                            d.routing.keepouts.push(Keepout {
                                id: format!("k{}", ids.next("k")),
                                layers: vec![l],
                                outline,
                                forbid: Forbid {
                                    tracks: false,
                                    vias: false,
                                    pours: true,
                                    pads: false,
                                    parts: false,
                                },
                                max_height: None,
                            });
                        }
                    }
                    ("solid", Some(l))
                        if l.is_copper() || matches!(l, Layer::Silk(_) | Layer::Fab(_)) =>
                    {
                        if l.is_copper() {
                            *skipped
                                .entry("solid copper regions (kept only as drawings)".into())
                                .or_default() += 1;
                        }
                        d.routing.graphics.push(BoardGraphic {
                            id: format!("g{}", ids.next("g")),
                            layer: l,
                            shape: Shape::Polygon { points: pts },
                            width: 0.0,
                            filled: true,
                        });
                    }
                    _ => {}
                }
            }
            "TEXT" => {
                // TEXT~type~x~y~strokeWidth~rotation~mirror~layer~net~fontSize~text~path~display~gId~font~locked
                if s(&fields, 1) != "L" {
                    continue;
                }
                let Some(layer) = board_layer(s(&fields, 7), copper) else {
                    continue;
                };
                if !matches!(layer, Layer::Silk(_) | Layer::Fab(_) | Layer::Copper(_)) {
                    continue;
                }
                let text = s(&fields, 10).to_string();
                if text.trim().is_empty() {
                    continue;
                }
                let size = r6((f(&fields, 9) * U).max(0.3));
                // EasyEDA anchors text at its left baseline. Lamina centres text.
                let w = 0.6 * size * text.chars().count() as f64;
                let base = ctx.pt(f(&fields, 2), f(&fields, 3));
                let rot = r6((-f(&fields, 5)).rem_euclid(360.0));
                let off = Point::new(w / 2.0, size * 0.4).rotated(rot);
                d.routing.texts.push(BoardText {
                    id: format!("x{}", ids.next("x")),
                    layer,
                    text,
                    at: Point::new(r6(base.x + off.x), r6(base.y + off.y)),
                    size,
                    rotation: rot,
                });
            }
            "SVGNODE" | "DIMENSION" | "IMAGE" => {}
            other => *skipped.entry(format!("{other} records")).or_default() += 1,
        }
    }

    // In EasyEDA, outline strokes are 0.254 mm wide, so ends nearer than
    // this look joined. If a board has real gaps, close them with straight
    // lines. Then the board still gets an outline.
    let (mut loops, mut open) = chain_edges(edges.clone(), 0.15);
    if loops.is_empty() && !edges.is_empty() {
        let (l2, o2) = chain_edges(edges, 3.0);
        if !l2.is_empty() {
            notes.push(
                "the board outline had gaps. Lamina closed them with straight lines (up to 3 mm)"
                    .into(),
            );
            loops = l2;
            open = o2;
        }
    }
    if open > 0 {
        notes.push(format!(
            "{open} board outline segment(s) do not close a loop and are not used"
        ));
    }
    let mut loops: Vec<Contour> = loops.into_iter().filter(|c| c.check().is_ok()).collect();
    loops.sort_by(|a, b| b.signed_area().abs().total_cmp(&a.signed_area().abs()));
    if let Some(outline) = loops.first().cloned() {
        d.board.outline = Some(outline);
        d.board.cutouts.extend(loops[1..].iter().cloned());
    } else {
        notes.push("the board has no closed outline".into());
    }
    for (what, n) in skipped {
        notes.push(format!("skipped {n} {what}"));
    }
    for net in nets.values_mut() {
        net.pads.sort();
        net.pads.dedup();
    }
    d.nets.nets = nets;
    // If the board does not give a rule, use the value that the board contains.
    // Then an import starts with no violations. Later, `use_fab_preset`
    // applies the limits of a fab.
    let r = &mut d.board.rules;
    let floor6 = |v: f64| (v * 1e6).floor() / 1e6;
    for v in &d.routing.vias {
        r.min_annular_ring = r.min_annular_ring.min(floor6((v.diameter - v.drill) / 2.0));
        r.min_via_drill = r.min_via_drill.min(v.drill);
        r.min_drill = r.min_drill.min(v.drill);
    }
    for t in &d.routing.tracks {
        r.min_track_width = r.min_track_width.min(t.width);
    }
    for fp in d.footprints.values() {
        for pad in &fp.pads {
            if let PadKind::Through {
                drill,
                plated: true,
            } = &pad.kind
            {
                let (w, h) = pad.shape.size();
                let ring = floor6((w.min(h) - drill.min_size()) / 2.0);
                if ring > 0.0 {
                    r.min_annular_ring = r.min_annular_ring.min(ring);
                }
            }
        }
    }
    r.hole_to_hole = r.hole_to_hole.min(0.1);
    r.copper_to_edge = r.copper_to_edge.min(0.1);
    notes.push(
        "the rules were made less strict to agree with the board. To use the limits of a fab, \
         run use_fab_preset"
            .into(),
    );
    if let Some(b) = d.board.outline.as_ref().and_then(|o| o.bbox()) {
        if b.min.x != 0.0 || b.min.y != 0.0 {
            d.translate(-b.min.x, -b.min.y);
        }
    }
    notes.push(format!(
        "{} footprints, {} nets, {} tracks, {} vias, {} zones",
        d.parts.parts.len(),
        d.nets.nets.len(),
        d.routing.tracks.len(),
        d.routing.vias.len(),
        d.routing.zones.len()
    ));
    Ok(ImportedBoard {
        data: d,
        models,
        notes,
    })
}

#[derive(Default)]
struct Ids(BTreeMap<&'static str, usize>);

impl Ids {
    fn next(&mut self, prefix: &'static str) -> usize {
        let n = self.0.entry(prefix).or_default();
        *n += 1;
        *n
    }
}

struct LibInstance {
    footprint: Footprint,
    package: String,
    placement: Placement,
    refdes: String,
    value: Option<String>,
    attrs: BTreeMap<String, String>,
    pad_nets: Vec<(String, String)>,
    model: Option<ModelInfo>,
    notes: Vec<String>,
}

/// A placed footprint. The children have absolute coordinates, so find the
/// side and the placement first. Then map each child back to footprint-local
/// coordinates.
fn lib_instance(
    ctx: &Ctx,
    head: &[&str],
    children_src: &str,
    copper: u8,
) -> Result<LibInstance, String> {
    let _ = copper;
    let at = ctx.pt(f(head, 1), f(head, 2));
    let a = attrs(s(head, 3));
    let package = a
        .get("package")
        .cloned()
        .unwrap_or_else(|| "footprint".into());
    let rotation = r6(f(head, 4).rem_euclid(360.0));
    let children: Vec<Vec<&str>> = children_src
        .split("#@$")
        .filter(|c| !c.is_empty())
        .map(|c| c.split('~').collect())
        .collect();
    let bottom = children.iter().any(|c| {
        let layer = match c[0] {
            "PAD" => s(c, 6),
            "TRACK" | "ARC" | "COPPERAREA" => s(c, 2),
            "CIRCLE" | "RECT" => s(c, 5),
            "SOLIDREGION" => s(c, 1),
            "TEXT" => s(c, 7),
            _ => "",
        };
        is_bottom_layer(layer)
    });
    let side = if bottom { Side::Bottom } else { Side::Top };
    let pl = Placement {
        x: at.x,
        y: at.y,
        rotation,
        side,
        locked: false,
    };
    let local = |p: Point| {
        let q = pl.inverse(p);
        Point::new(r6(q.x), r6(q.y))
    };
    let local_angle = |abs: f64| {
        let rel = if side == Side::Bottom {
            rotation - abs
        } else {
            abs - rotation
        };
        r6(rel.rem_euclid(360.0))
    };
    let local_shape = |sh: &Shape| -> Shape {
        match sh {
            Shape::Line { a, b } => Shape::Line {
                a: local(*a),
                b: local(*b),
            },
            Shape::Circle { center, radius } => Shape::Circle {
                center: local(*center),
                radius: *radius,
            },
            Shape::Rect { min, max } => {
                let c: Vec<Point> = [
                    *min,
                    Point::new(max.x, min.y),
                    *max,
                    Point::new(min.x, max.y),
                ]
                .iter()
                .map(|p| local(*p))
                .collect();
                if rotation.rem_euclid(90.0) < 1e-6 {
                    Shape::Rect {
                        min: Point::new(
                            c.iter().map(|p| p.x).fold(f64::MAX, f64::min),
                            c.iter().map(|p| p.y).fold(f64::MAX, f64::min),
                        ),
                        max: Point::new(
                            c.iter().map(|p| p.x).fold(f64::MIN, f64::max),
                            c.iter().map(|p| p.y).fold(f64::MIN, f64::max),
                        ),
                    }
                } else {
                    Shape::Polygon { points: c }
                }
            }
            Shape::Arc {
                center,
                start,
                angle,
            } => Shape::Arc {
                center: local(*center),
                start: local(*start),
                angle: if side == Side::Bottom {
                    -*angle
                } else {
                    *angle
                },
            },
            Shape::Polygon { points } => Shape::Polygon {
                points: points.iter().map(|p| local(*p)).collect(),
            },
        }
    };

    let mut fp = Footprint::new(
        footprint_id(&package),
        Provenance::Manual {
            note: "EasyEDA board".into(),
        },
    );
    if let Some(lcsc) = a.get("BOM_Supplier Part").filter(|v| !v.trim().is_empty()) {
        fp.provenance = Provenance::Lcsc {
            part: lcsc.trim().to_ascii_uppercase(),
            fetched: String::new(),
        };
    }
    fp.description = Some(package.clone());
    let mut refdes = String::new();
    let mut value = None;
    let mut pad_nets = Vec::new();
    let mut model = None;
    let mut notes = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    for c in &children {
        match c[0] {
            "PAD" => match pad_any(ctx, c) {
                Ok(RawPad {
                    mut pad,
                    side: pside,
                    net,
                }) => {
                    if let Some(ps) = pside {
                        if ps != side {
                            *skipped.entry("pads on the other side".into()).or_default() += 1;
                            continue;
                        }
                    }
                    pad.at = local(pad.at);
                    pad.rotation = local_angle(pad.rotation);
                    if let PadShape::Polygon { points } = &mut pad.shape {
                        // The points are relative to the pad centre. Only mirror and rotate them.
                        let rel = Placement {
                            x: 0.0,
                            y: 0.0,
                            rotation,
                            side,
                            locked: false,
                        };
                        for p in points.iter_mut() {
                            let q = rel.inverse(*p);
                            *p = Point::new(r6(q.x), r6(q.y));
                        }
                    }
                    if !net.trim().is_empty() && !pad.number.is_empty() {
                        pad_nets.push((pad.number.clone(), net.trim().to_string()));
                    }
                    fp.pads.push(pad);
                }
                Err(e) => notes.push(format!("skipped a pad: {e}")),
            },
            "HOLE" => {
                let dia = r6(f(c, 3) * 2.0 * U);
                if dia > 0.0 {
                    let mut p = Pad::through(
                        "",
                        local(ctx.pt(f(c, 1), f(c, 2))),
                        PadShape::Circle { diameter: dia },
                        Drill::Round { diameter: dia },
                    );
                    p.kind = PadKind::Through {
                        drill: Drill::Round { diameter: dia },
                        plated: false,
                    };
                    fp.pads.push(p);
                }
            }
            "TRACK" => {
                let Some(layer) = fp_layer(s(c, 2), side) else {
                    continue;
                };
                let w = r6(f(c, 1) * U);
                let pts = points(ctx, s(c, 4));
                for seg in pts.windows(2) {
                    if seg[0].dist(seg[1]) > 1e-6 {
                        fp.graphics.push(FpGraphic {
                            layer,
                            shape: Shape::Line {
                                a: local(seg[0]),
                                b: local(seg[1]),
                            },
                            width: w,
                            filled: false,
                        });
                    }
                }
            }
            "CIRCLE" => {
                let Some(layer) = fp_layer(s(c, 5), side) else {
                    continue;
                };
                let radius = r6(f(c, 3) * U);
                if radius > 0.0 {
                    let w = r6(f(c, 4) * U);
                    let filled = radius < 0.2 && w >= radius;
                    fp.graphics.push(FpGraphic {
                        layer,
                        shape: Shape::Circle {
                            center: local(ctx.pt(f(c, 1), f(c, 2))),
                            radius,
                        },
                        width: if filled { 0.0 } else { w },
                        filled,
                    });
                }
            }
            "RECT" => {
                let Some(layer) = fp_layer(s(c, 5), side) else {
                    continue;
                };
                let p0 = ctx.pt(f(c, 1), f(c, 2));
                let p1 = ctx.pt(f(c, 1) + f(c, 3), f(c, 2) + f(c, 4));
                let shape = Shape::Rect {
                    min: Point::new(p0.x.min(p1.x), p0.y.min(p1.y)),
                    max: Point::new(p0.x.max(p1.x), p0.y.max(p1.y)),
                };
                fp.graphics.push(FpGraphic {
                    layer,
                    shape: local_shape(&shape),
                    width: 0.15,
                    filled: false,
                });
            }
            "ARC" => {
                let Some(layer) = fp_layer(s(c, 2), side) else {
                    continue;
                };
                if let Some(shape) = svg_arc(ctx, s(c, 4)) {
                    fp.graphics.push(FpGraphic {
                        layer,
                        shape: local_shape(&shape),
                        width: r6(f(c, 1) * U),
                        filled: false,
                    });
                }
            }
            "SOLIDREGION" => {
                let layer_id = s(c, 1);
                if s(c, 4) != "solid" {
                    continue;
                }
                let pts: Vec<Point> = path_points(ctx, s(c, 3)).into_iter().map(local).collect();
                if pts.len() < 3 {
                    continue;
                }
                let layer = match layer_id {
                    "1" | "2" if is_bottom_layer(layer_id) == (side == Side::Bottom) => {
                        FpLayer::Copper
                    }
                    "1" | "2" => continue,
                    _ => match fp_layer(layer_id, side) {
                        Some(l) => l,
                        None => continue,
                    },
                };
                if layer_id == "99" || layer_id == "100" {
                    continue;
                }
                fp.graphics.push(FpGraphic {
                    layer,
                    shape: Shape::Polygon { points: pts },
                    width: 0.0,
                    filled: true,
                });
            }
            "TEXT" => match s(c, 1) {
                "P" => refdes = s(c, 10).trim().to_string(),
                "N" => value = Some(s(c, 10).trim().to_string()).filter(|v| !v.is_empty()),
                _ => {}
            },
            "SVGNODE" => {
                if let Some(m) = model_info(ctx, c.get(1).copied().unwrap_or("")) {
                    model = Some(m);
                }
            }
            other => *skipped.entry(format!("{other} records")).or_default() += 1,
        }
    }
    for (what, n) in skipped {
        notes.push(format!("skipped {n} {what}"));
    }
    // Add the reference and value placeholders where EasyEDA shows them.
    if let Some(cy) = fp.pads_bbox() {
        fp.texts.push(FpText {
            layer: FpLayer::Silk,
            text: "${ref}".into(),
            at: Point::new(0.0, r6(cy.max.y + 0.8)),
            size: 0.8,
            rotation: 0.0,
        });
        fp.texts.push(FpText {
            layer: FpLayer::Fab,
            text: "${value}".into(),
            at: Point::new(0.0, 0.0),
            size: 0.6,
            rotation: 0.0,
        });
        // EasyEDA has no courtyards. The pad extent lets you select parts, and
        // it does not add false overlaps on dense boards.
        let m = 0.0;
        fp.graphics.push(FpGraphic {
            layer: FpLayer::Courtyard,
            shape: Shape::Rect {
                min: Point::new(r6(cy.min.x - m), r6(cy.min.y - m)),
                max: Point::new(r6(cy.max.x + m), r6(cy.max.y + m)),
            },
            width: 0.05,
            filled: false,
        });
    }
    if let Some(m) = &model {
        // The outline centre of the model moves with the footprint. Keep it local.
        let _ = m;
    }
    Ok(LibInstance {
        footprint: fp,
        package,
        placement: pl,
        refdes,
        value,
        attrs: a,
        pad_nets,
        model,
        notes,
    })
}

/// The footprint layer for an EasyEDA layer id on a footprint of `side`.
fn fp_layer(id: &str, side: Side) -> Option<FpLayer> {
    match id {
        "3" if side == Side::Top => Some(FpLayer::Silk),
        "4" if side == Side::Bottom => Some(FpLayer::Silk),
        "5" if side == Side::Top => Some(FpLayer::Paste),
        "6" if side == Side::Bottom => Some(FpLayer::Paste),
        "7" if side == Side::Top => Some(FpLayer::Mask),
        "8" if side == Side::Bottom => Some(FpLayer::Mask),
        "13" if side == Side::Top => Some(FpLayer::Fab),
        "14" if side == Side::Bottom => Some(FpLayer::Fab),
        "12" | "15" | "99" | "100" | "101" => graphic_layer(id),
        _ => None,
    }
}

fn register_footprint(d: &mut ProjectData, mut fp: Footprint) -> String {
    let base = fp.id.clone();
    let same = |a: &Footprint, b: &Footprint| a.pads == b.pads && a.graphics == b.graphics;
    let mut id = base.clone();
    let mut k = 1;
    loop {
        match d.footprints.get(&id) {
            None => {
                fp.id = id.clone();
                d.footprints.insert(id.clone(), fp);
                return id;
            }
            Some(existing) if same(existing, &fp) => return id,
            Some(_) => {
                k += 1;
                id = format!("{base}_{k}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Value {
        // The origin is (4000, 3000). Units are 10 mil, Y points down. A 20 x 10 mm board.
        let shapes = vec![
            "TRACK~1~10~~4000 3000 4078.74 3000 4078.74 3039.37 4000 3039.37 4000 3000~gge1~0".to_string(),
            // R1 is at (5, 5) mm, rotated 90 counter-clockwise. Pad 1 (left in the
            // library) moves below the centre. This is +y in the Y-down frame of EasyEDA.
            "LIB~4019.685~3019.685~package`R0603`BOM_Supplier Part`C21190`BOM_Manufacturer`UNI-ROYAL`BOM_Manufacturer Part`0603WAF1001T5E~90~~gge10~1#@$\
PAD~RECT~4019.685~3022.933~3.5433~3.1496~1~GND~1~0~~90~gge11~0~~Y~0~0#@$\
PAD~RECT~4019.685~3016.437~3.5433~3.1496~1~SIG~2~0~~90~gge12~0~~Y~0~0#@$\
TRACK~0.5~3~~4017 3013 4017 3026~gge13~0#@$\
TEXT~P~4015~3010~0.6~0~0~3~~4.5~R1~~~gge14~~0#@$\
TEXT~N~4015~3030~0.6~0~0~3~~4.5~1k~~~gge15~~0".to_string(),
            // Q1 is on the bottom at (15, 5) mm.
            "LIB~4059.055~3019.685~package`SOT-23~0~~gge20~1#@$\
PAD~RECT~4055.3~3016~2.4~3.6~2~SIG~1~0~~0~gge21~0~~Y~0~0#@$\
PAD~RECT~4062.8~3016~2.4~3.6~2~GND~2~0~~0~gge22~0~~Y~0~0#@$\
PAD~RECT~4059.055~3023.4~2.4~3.6~2~OUT~3~0~~0~gge23~0~~Y~0~0#@$\
TEXT~P~4055~3010~0.6~0~1~4~~4.5~Q1~~~gge24~~0".to_string(),
            "TRACK~1~1~SIG~4019.685 3016.437 4040 3016.437~gge30~0".to_string(),
            "VIA~4040~3016.437~2.4~SIG~0.6~gge31~0".to_string(),
            "COPPERAREA~1~2~GND~M 4000 3000 L 4078.74 3000 L 4078.74 3039.37 L 4000 3039.37 Z~1.5~solid~gge40~spoke~none~[[]]~0".to_string(),
            "HOLE~4070~3030~6~gge50~0".to_string(),
            "TEXT~L~4005~3036~0.8~0~0~3~~7.874~DEMO~~~gge60~~0".to_string(),
        ];
        serde_json::json!({
            "head": {"docType": "3", "x": "4000", "y": "3000"},
            "shape": shapes,
            "DRCRULE": {"Default": {"trackWidth": 0.5984, "clearance": 0.5984, "viaHoleDiameter": 1.7717, "viaHoleD": 1.1811}}
        })
    }

    #[test]
    fn imports_a_small_board() {
        let b = import_board(&doc(), "demo").unwrap();
        let d = &b.data;
        assert_eq!(d.board.stackup.copper_layers, 2);
        assert!((d.board.rules.clearance - 0.152).abs() < 1e-3);
        let o = d.board.outline.as_ref().unwrap().bbox().unwrap();
        assert!(
            (o.width() - 20.0).abs() < 1e-3 && (o.height() - 10.0).abs() < 1e-3,
            "{o:?}"
        );
        assert_eq!(o.min, Point::new(0.0, 0.0));
        assert_eq!(d.parts.parts.len(), 2, "{:?}", b.notes);
        assert_eq!(d.parts.parts["R1"].value.as_deref(), Some("1k"));
        assert_eq!(d.parts.parts["R1"].lcsc.as_deref(), Some("C21190"));
        let r1 = d.placement.placements["R1"];
        assert!(
            (r1.x - 5.0).abs() < 1e-3 && (r1.y - 5.0).abs() < 1e-3,
            "{r1:?}"
        );
        assert_eq!((r1.rotation, r1.side), (90.0, Side::Top));
        let fp = &d.footprints[&d.parts.parts["R1"].footprint];
        // Without the 90° rotation, the local pads are on the X axis.
        // Pad 1 is at (-0.825, 0).
        let p1 = fp.pad("1").unwrap();
        assert!(
            (p1.at.x + 0.825).abs() < 1e-3 && p1.at.y.abs() < 1e-3,
            "{:?}",
            p1.at
        );
        assert_eq!(p1.rotation, 0.0);
        let q1 = d.placement.placements["Q1"];
        assert_eq!(q1.side, Side::Bottom);
        let qfp = &d.footprints[&d.parts.parts["Q1"].footprint];
        // Pad 3 (abs (15, 4.06)) is below the centre. Pad 1 is on the left on the board.
        let p3 = qfp.pad("3").unwrap();
        assert!(p3.at.x.abs() < 1e-3 && p3.at.y < 0.0, "{:?}", p3.at);
        let p1 = qfp.pad("1").unwrap();
        assert!(
            q1.transform(p1.at).x < 15.0,
            "pad 1 must stay on the left after the mirror back"
        );
        assert_eq!(d.nets.nets["SIG"].pads.len(), 2);
        assert_eq!(d.nets.nets["GND"].pads.len(), 2);
        assert_eq!(d.routing.tracks.len(), 1);
        assert!((d.routing.tracks[0].width - 0.254).abs() < 1e-6);
        assert_eq!(d.routing.vias.len(), 1);
        assert!((d.routing.vias[0].drill - 0.3048).abs() < 1e-6);
        assert_eq!(d.routing.zones.len(), 1);
        assert_eq!(d.routing.zones[0].net.as_deref(), Some("GND"));
        assert_eq!(d.routing.zones[0].layers, vec![Layer::BOTTOM]);
        assert_eq!(d.board.holes.len(), 1);
        assert!((d.board.holes[0].drill.min_size() - 3.048).abs() < 1e-6);
        assert_eq!(d.routing.texts[0].text, "DEMO");
    }
}

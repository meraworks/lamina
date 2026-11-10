//! `.kicad_pcb` boards: import them into project data, and export them from it.

use std::collections::BTreeMap;

use lamina_model::*;

use crate::footprint::{self, Instance, KicadModel, arc_from_three, points_from, shape_from};
use crate::layers::{fp_layer_to_kicad, layer_from_kicad, layer_table, layer_to_kicad};
use lamina_model::sexpr::{self, Node, Sx, n};

/// A board read from KiCad.
#[derive(Debug)]
pub struct ImportedBoard {
    pub data: ProjectData,
    /// Footprint id to its visible KiCad 3D models. The caller resolves them
    /// and uses the first model that resolves.
    pub models: BTreeMap<String, Vec<KicadModel>>,
    pub notes: Vec<String>,
}

fn r6(v: f64) -> f64 {
    json::round6(v)
}

/// Board coordinates: KiCad Y down to Lamina Y up.
fn bp(x: f64, y: f64) -> Point {
    Point::new(r6(x), r6(-y))
}

// ---------------------------------------------------------------- import

pub fn import_board(src: &str, name: &str) -> Result<ImportedBoard, String> {
    let root = sexpr::parse(src)?;
    if !root.is("kicad_pcb") {
        return Err(format!(
            "the file is not a KiCad board (it starts with `{}`)",
            root.name().unwrap_or("?")
        ));
    }
    let mut d = ProjectData::new(name);
    let mut notes = Vec::new();
    let mut models = BTreeMap::new();

    // Layers and stackup.
    let mut copper = 0u8;
    if let Some(layers) = root.child("layers") {
        for l in layers.args() {
            let lname = l.arg(1).unwrap_or("");
            if lname.ends_with(".Cu")
                && matches!(l.arg(2), Some("signal" | "power" | "mixed" | "jumper"))
            {
                copper += 1;
            }
        }
    }
    if copper >= 1 {
        d.board.stackup.copper_layers = copper.max(2);
    }
    if let Some(t) = root.child("general").and_then(|g| g.child_num("thickness")) {
        d.board.stackup.thickness = r6(t);
    }
    if let Some(m) = root
        .child("setup")
        .and_then(|s| s.child_num("pad_to_mask_clearance"))
    {
        // Zero is a value: the mask opening is the pad.
        if m >= 0.0 {
            d.board.rules.mask_margin = r6(m);
        }
    }

    // Nets by number.
    let mut net_names: BTreeMap<i64, String> = BTreeMap::new();
    for net in root.children("net") {
        if let (Some(num), Some(nm)) = (net.arg_num(0), net.arg(1)) {
            if !nm.is_empty() {
                net_names.insert(num as i64, nm.to_string());
            }
        }
    }
    // `(net 3 "GND")` in most versions, `(net "GND")` in KiCad 10, `(net 3)` in tracks.
    let net_of = |node: &Sx| -> Option<String> {
        let net = node.child("net")?;
        if let Some(nm) = net.arg(1) {
            return Some(nm.to_string());
        }
        let first = net.arg(0)?;
        match first.parse::<i64>() {
            Ok(num) => net_names.get(&num).cloned(),
            Err(_) => Some(first.to_string()),
        }
    };
    let mut nets: BTreeMap<String, Net> = BTreeMap::new();
    let ensure_net = |nets: &mut BTreeMap<String, Net>, name: &str| {
        if !valid_net_name(name) {
            return false;
        }
        nets.entry(name.to_string()).or_default();
        true
    };

    // Footprints.
    let mut used_refs: BTreeMap<String, usize> = BTreeMap::new();
    let mut next_x = 1;
    let mut skipped_notes: BTreeMap<String, usize> = BTreeMap::new();
    // The Edge.Cuts drawings of the footprints, in board coordinates.
    let mut part_edges: Vec<Edge> = Vec::new();
    for node in root
        .args()
        .iter()
        .filter(|c| c.is("footprint") || c.is("module"))
    {
        let inst: Instance = match footprint::convert(node, "", "kicad_pcb") {
            Ok(i) => i,
            Err(e) => {
                notes.push(format!("skipped a footprint: {e}"));
                continue;
            }
        };
        let mut refdes = inst.reference.trim().to_string();
        if !valid_refdes(&refdes) || refdes.contains("**") {
            let r = format!("X{next_x}");
            next_x += 1;
            notes.push(format!("renamed `{}` to {r}", inst.reference));
            refdes = r;
        }
        let count = used_refs.entry(refdes.clone()).or_default();
        *count += 1;
        if *count > 1 {
            let r = format!("{refdes}_{count}");
            notes.push(format!("renamed the duplicate reference {refdes} to {r}"));
            refdes = r;
        }
        for note in &inst.notes {
            *skipped_notes
                .entry(format!("{}: {note}", inst.name))
                .or_default() += 1;
        }
        if !inst.edge.is_empty() {
            let pl = Placement {
                x: inst.at.x,
                y: inst.at.y,
                rotation: inst.rotation,
                side: inst.side,
                locked: false,
            };
            for shape in &inst.edge {
                part_edges.extend(edges_of(shape).into_iter().map(|e| place_edge(e, &pl)));
            }
        }
        // The footprint definition. Identical definitions are shared.
        let fp_id = register_footprint(&mut d, inst.footprint.clone());
        if !inst.models.is_empty() {
            models
                .entry(fp_id.clone())
                .or_insert_with(|| inst.models.clone());
        }
        let mut part = Part::new(fp_id);
        part.value = inst.value.clone().filter(|v| !v.is_empty());
        part.dnp = inst.dnp;
        for (k, v) in &inst.properties {
            match k.as_str() {
                "Description" => part.description = Some(v.clone()),
                "Datasheet" => {
                    part.fields.insert("datasheet".into(), v.clone());
                }
                "LCSC" | "LCSC Part" | "LCSC Part #" | "JLCPCB Part #" | "JLCPCB" => {
                    part.lcsc = Some(v.trim().to_ascii_uppercase());
                }
                "MPN" | "Manufacturer Part Number" | "Mfr. Part #" => part.mpn = Some(v.clone()),
                "Manufacturer" | "Mfr." => part.manufacturer = Some(v.clone()),
                other => {
                    part.fields.insert(other.to_lowercase(), v.clone());
                }
            }
        }
        if inst.exclude_from_bom {
            part.fields.insert("exclude_from_bom".into(), "yes".into());
        }
        d.parts.parts.insert(refdes.clone(), part);
        d.placement.placements.insert(
            refdes.clone(),
            Placement {
                x: inst.at.x,
                y: inst.at.y,
                rotation: inst.rotation,
                side: inst.side,
                locked: false,
            },
        );
        for (pad, net) in &inst.pad_nets {
            if ensure_net(&mut nets, net) {
                nets.get_mut(net)
                    .unwrap()
                    .pads
                    .push(PadRef::new(refdes.clone(), pad.clone()));
            }
        }
    }
    for (what, k) in skipped_notes {
        notes.push(if k > 1 {
            format!("{what} (x{k})")
        } else {
            what
        });
    }

    // Tracks, vias, zones.
    let layers = d.board.stackup.copper_layers;
    let mut tid = 0;
    for seg in root
        .args()
        .iter()
        .filter(|c| c.is("segment") || c.is("arc"))
    {
        let Some(layer) = seg.child_text("layer").and_then(layer_from_kicad) else {
            continue;
        };
        if !layer.is_copper() || !layer.exists_in(layers) {
            continue;
        }
        let Some(net) = net_of(seg) else { continue };
        if !ensure_net(&mut nets, &net) {
            continue;
        }
        let width = seg.child_num("width").unwrap_or(d.board.rules.track_width);
        let s = seg.xy("start").map(|(x, y)| bp(x, y));
        let e = seg.xy("end").map(|(x, y)| bp(x, y));
        let (Some(s), Some(e)) = (s, e) else { continue };
        let points = match seg.xy("mid").map(|(x, y)| bp(x, y)) {
            Some(m) => match arc_from_three(s, m, e) {
                Some(Shape::Arc {
                    center,
                    start,
                    angle,
                }) => {
                    let k = ((angle.abs() / 10.0).ceil() as usize).clamp(2, 36);
                    (0..=k)
                        .map(|i| {
                            start
                                .sub(center)
                                .rotated(angle * i as f64 / k as f64)
                                .add(center)
                        })
                        .map(|p| Point::new(r6(p.x), r6(p.y)))
                        .collect()
                }
                _ => vec![s, e],
            },
            None => vec![s, e],
        };
        tid += 1;
        d.routing.tracks.push(Track {
            id: format!("t{tid}"),
            net,
            layer,
            width: r6(width),
            points,
        });
    }
    let mut vid = 0;
    for via in root.children("via") {
        let Some(net) = net_of(via) else { continue };
        if !ensure_net(&mut nets, &net) {
            continue;
        }
        let Some((x, y)) = via.xy("at") else { continue };
        let size = via.child_num("size").unwrap_or(d.board.rules.via_diameter);
        let drill = via.child_num("drill").unwrap_or(d.board.rules.via_drill);
        let ls: Vec<Layer> = via
            .child("layers")
            .map(|l| l.atoms().into_iter().filter_map(layer_from_kicad).collect())
            .unwrap_or_default();
        let full = ls.len() == 2 && ls.contains(&Layer::TOP) && ls.contains(&Layer::BOTTOM);
        vid += 1;
        d.routing.vias.push(Via {
            id: format!("v{vid}"),
            net,
            at: bp(x, y),
            diameter: r6(size),
            drill: r6(drill),
            from: if full { None } else { ls.first().copied() },
            to: if full { None } else { ls.get(1).copied() },
        });
    }
    let mut zid = 0;
    let mut kid = 0;
    for zone in root.children("zone") {
        let mut ls: Vec<Layer> = Vec::new();
        for l in zone.children("layer").chain(zone.children("layers")) {
            for a in l.atoms() {
                match a {
                    "F&B.Cu" => ls.extend([Layer::TOP, Layer::BOTTOM]),
                    "*.Cu" => ls.extend(Layer::all_copper(layers)),
                    other => ls.extend(layer_from_kicad(other)),
                }
            }
        }
        ls.retain(|l| l.is_copper() && l.exists_in(layers));
        ls.dedup();
        if ls.is_empty() {
            notes.push("skipped a zone that is on no copper layer".into());
            continue;
        }
        let Some(pts) = zone.child("polygon").and_then(|p| p.child("pts")) else {
            continue;
        };
        let points = points_from(pts, &bp);
        let Some(outline) = contour_from_points(points) else {
            notes.push("skipped a zone with a degenerate outline".into());
            continue;
        };
        if let Some(k) = zone.child("keepout") {
            let allowed = |what: &str| k.child_text(what) != Some("not_allowed");
            kid += 1;
            d.routing.keepouts.push(Keepout {
                id: format!("k{kid}"),
                layers: ls,
                outline,
                forbid: Forbid {
                    tracks: !allowed("tracks"),
                    vias: !allowed("vias"),
                    pours: !allowed("copperpour"),
                    pads: !allowed("pads"),
                    parts: !allowed("footprints"),
                },
            });
            continue;
        }
        let net = zone
            .child("net_name")
            .and_then(|x| x.arg(0))
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .or_else(|| net_of(zone));
        let net = net.filter(|nm| ensure_net(&mut nets, nm));
        let cp = zone.child("connect_pads");
        let pad_connection = match cp.and_then(|c| c.arg(0)) {
            Some("yes") => PadConnection::Solid,
            Some("no") => PadConnection::None,
            _ => PadConnection::Thermal,
        };
        let fill = zone.child("fill");
        zid += 1;
        d.routing.zones.push(Zone {
            id: format!("z{zid}"),
            net,
            layers: ls,
            outline,
            priority: zone.child_num("priority").unwrap_or(0.0) as i32,
            clearance: cp.and_then(|c| c.child_num("clearance")).map(r6),
            min_width: zone.child_num("min_thickness").map(r6),
            pad_connection,
            thermal_gap: fill
                .and_then(|f| f.child_num("thermal_gap"))
                .map(r6)
                .unwrap_or(0.5),
            thermal_spoke: fill
                .and_then(|f| f.child_num("thermal_bridge_width"))
                .map(r6)
                .unwrap_or(0.5),
        });
    }

    // Outline, texts and drawings.
    let mut edges: Vec<Edge> = Vec::new();
    let mut gid = 0;
    let mut xid = 0;
    for g in root.args() {
        let Some(kind) = g.name() else { continue };
        if kind == "gr_text" {
            let Some(layer) = g.child_text("layer").and_then(layer_from_kicad) else {
                continue;
            };
            if !matches!(layer, Layer::Silk(_) | Layer::Fab(_) | Layer::Copper(_)) {
                continue;
            }
            let at = g.child("at");
            let (x, y, a) = at
                .map(|a| {
                    (
                        a.arg_num(0).unwrap_or(0.0),
                        a.arg_num(1).unwrap_or(0.0),
                        a.arg_num(2).unwrap_or(0.0),
                    )
                })
                .unwrap_or((0.0, 0.0, 0.0));
            let size = g
                .child("effects")
                .and_then(|e| e.child("font"))
                .and_then(|f| f.child("size"))
                .and_then(|s| s.arg_num(1).or(s.arg_num(0)))
                .unwrap_or(1.0);
            xid += 1;
            d.routing.texts.push(BoardText {
                id: format!("x{xid}"),
                layer,
                text: g.arg(0).unwrap_or("").to_string(),
                at: bp(x, y),
                size: r6(size),
                rotation: r6(a.rem_euclid(360.0)),
            });
            continue;
        }
        if !kind.starts_with("gr_") {
            continue;
        }
        let Some(layer) = g.child_text("layer").and_then(layer_from_kicad) else {
            continue;
        };
        let Some(shape) = shape_from(g, &bp) else {
            continue;
        };
        if layer == Layer::Outline {
            edges.extend(edges_of(&shape));
            continue;
        }
        if !matches!(layer, Layer::Silk(_) | Layer::Fab(_) | Layer::Copper(_)) {
            continue;
        }
        gid += 1;
        let filled = matches!(g.child_text("fill"), Some("yes") | Some("solid"));
        d.routing.graphics.push(BoardGraphic {
            id: format!("g{gid}"),
            layer,
            shape,
            width: g
                .child("stroke")
                .and_then(|s| s.child_num("width"))
                .or_else(|| g.child_num("width"))
                .map(r6)
                .unwrap_or(0.15),
            filled,
        });
    }
    edges.extend(part_edges);
    let (loops, open) = chain_edges(edges, 0.01);
    if open > 0 {
        notes.push(format!(
            "{open} board edge segment(s) do not close a loop and are not used"
        ));
    }
    let mut loops: Vec<Contour> = loops
        .into_iter()
        .filter_map(|c| match c.check() {
            Ok(()) => Some(c),
            Err(e) => {
                notes.push(format!("skipped a board edge loop: {e}"));
                None
            }
        })
        .collect();
    loops.sort_by(|a, b| b.signed_area().abs().total_cmp(&a.signed_area().abs()));
    if let Some(outline) = loops.first().cloned() {
        d.board.outline = Some(outline);
        d.board.cutouts = loops[1..].to_vec();
    } else {
        notes.push("the board has no closed outline on Edge.Cuts".into());
    }

    for net in nets.values_mut() {
        net.pads.sort();
        net.pads.dedup();
    }
    d.nets.nets = nets;
    // A KiCad board can be anywhere on the sheet. Put the corner of the outline
    // at the origin.
    if let Some(b) = d.board.outline.as_ref().and_then(|o| o.bbox()) {
        if b.min.x != 0.0 || b.min.y != 0.0 {
            d.translate(-b.min.x, -b.min.y);
            notes.push(format!(
                "moved the board by ({}, {}) to put its corner at the origin",
                r6(-b.min.x),
                r6(-b.min.y)
            ));
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

/// Add a footprint definition. If an identical definition exists, use it.
/// If a different definition has the same id, give the new one a new id.
fn register_footprint(d: &mut ProjectData, mut fp: Footprint) -> String {
    let base = fp.id.clone();
    let same = |a: &Footprint, b: &Footprint| {
        a.pads == b.pads && a.graphics == b.graphics && a.texts == b.texts
    };
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

// ---------------------------------------------------------------- export

/// A stable UUID-shaped id from a seed.
fn uuid(seed: &str) -> String {
    let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for b in seed.bytes() {
        h1 ^= b as u64;
        h1 = h1.wrapping_mul(0x100000001b3);
        h2 = h2.wrapping_mul(0x100000001b3) ^ (b as u64).rotate_left(7);
    }
    let hex = format!("{h1:016x}{h2:016x}");
    format!(
        "{}-{}-4{}-8{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

fn font(size: f64) -> Node {
    n("effects").child(
        n("font")
            .child(n("size").num(size).num(size))
            .child(n("thickness").num(r6(size * 0.15))),
    )
}

fn stroke(width: f64) -> Node {
    n("stroke")
        .child(n("width").num(width))
        .child(n("type").sym("solid"))
}

/// Export a project as a KiCad 8 board.
pub fn export_board(d: &ProjectData) -> String {
    export_board_with(d, None)
}

/// Export with absolute 3D model paths under `model_dir`, so that KiCad
/// finds the files in the project cache.
pub fn export_board_with(d: &ProjectData, model_dir: Option<&std::path::Path>) -> String {
    let copper = d.board.stackup.copper_layers;
    let mut net_ids: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, name) in d.nets.nets.keys().enumerate() {
        net_ids.insert(name.as_str(), i + 1);
    }
    let net_node = |name: &str| -> Option<Node> {
        let id = *net_ids.get(name)?;
        Some(n("net").num(id as f64).str(name))
    };
    // Tracks, vias and zones name a net by number only.
    let copper_net =
        |name: &str| -> Option<Node> { Some(n("net").num(*net_ids.get(name)? as f64)) };

    let mut root =
        n("kicad_pcb")
            .child(n("version").sym("20240108"))
            .child(n("generator").str("lamina"))
            .child(n("generator_version").str("8.0"))
            .child(
                n("general")
                    .child(n("thickness").num(d.board.stackup.thickness))
                    .child(n("legacy_teardrops").sym("no")),
            )
            .child(n("paper").str("A4"))
            .child(
                n("layers").children(layer_table(copper).into_iter().map(
                    |(id, name, kind, user)| {
                        let mut l = Node(Vec::new()).num(id as f64).str(name).sym(kind);
                        if let Some(u) = user {
                            l = l.str(u);
                        }
                        l
                    },
                )),
            )
            .child(
                n("setup")
                    .child(n("pad_to_mask_clearance").num(d.board.rules.mask_margin))
                    .child(
                        n("pcbplotparams")
                            .child(n("layerselection").sym("0x00010fc_ffffffff"))
                            .child(n("plot_on_all_layers_selection").sym("0x0000000_00000000"))
                            .child(n("disableapertmacros").sym("no"))
                            .child(n("usegerberextensions").sym("no"))
                            .child(n("usegerberattributes").sym("yes"))
                            .child(n("usegerberadvancedattributes").sym("yes"))
                            .child(n("creategerberjobfile").sym("yes"))
                            .child(n("dashed_line_dash_ratio").num(12.0))
                            .child(n("dashed_line_gap_ratio").num(3.0))
                            .child(n("svgprecision").num(4.0))
                            .child(n("plotframeref").sym("no"))
                            .child(n("viasonmask").sym("no"))
                            .child(n("mode").num(1.0))
                            .child(n("useauxorigin").sym("no"))
                            .child(n("hpglpennumber").num(1.0))
                            .child(n("hpglpenspeed").num(20.0))
                            .child(n("hpglpendiameter").num(15.0))
                            .child(n("pdf_front_fp_property_popups").sym("yes"))
                            .child(n("pdf_back_fp_property_popups").sym("yes"))
                            .child(n("dxfpolygonmode").sym("yes"))
                            .child(n("dxfimperialunits").sym("yes"))
                            .child(n("dxfusepcbnewfont").sym("yes"))
                            .child(n("psnegative").sym("no"))
                            .child(n("psa4output").sym("no"))
                            .child(n("plotreference").sym("yes"))
                            .child(n("plotvalue").sym("yes"))
                            .child(n("plotfptext").sym("yes"))
                            .child(n("plotinvisibletext").sym("no"))
                            .child(n("sketchpadsonfab").sym("no"))
                            .child(n("subtractmaskfromsilk").sym("no"))
                            .child(n("outputformat").num(1.0))
                            .child(n("mirror").sym("no"))
                            .child(n("drillshape").num(1.0))
                            .child(n("scaleselection").num(1.0))
                            .child(n("outputdirectory").str("")),
                    ),
            )
            .child(n("net").num(0.0).str(""))
            .children(
                d.nets
                    .nets
                    .keys()
                    .map(|name| n("net").num(net_ids[name.as_str()] as f64).str(name)),
            );

    // Footprints.
    let pad_nets = d.nets.pad_map();
    for (refdes, part) in &d.parts.parts {
        let Some(pl) = d.placement.placements.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        root = root.child(footprint_node(
            refdes, part, pl, fp, &pad_nets, &net_node, model_dir,
        ));
    }
    // Free holes become one-pad footprints.
    for (i, h) in d.board.holes.iter().enumerate() {
        root = root.child(hole_node(i, h, &net_node));
    }

    // Outline.
    let mut gi = 0;
    for c in d.board.outline.iter().chain(d.board.cutouts.iter()) {
        for node in contour_nodes(c, "Edge.Cuts", 0.05, &mut gi) {
            root = root.child(node);
        }
    }
    for g in &d.routing.graphics {
        gi += 1;
        if let Some(node) = shape_node(
            &g.shape,
            &layer_to_kicad(g.layer),
            g.width,
            g.filled,
            &format!("g{gi}"),
        ) {
            root = root.child(node);
        }
    }
    for t in &d.routing.texts {
        let mirror = t.layer.side() == Some(Side::Bottom);
        let mut eff = font(t.size);
        if mirror {
            eff = eff.child(n("justify").sym("mirror"));
        }
        root = root.child(
            n("gr_text")
                .str(&t.text)
                .child(n("at").num(t.at.x).num(-t.at.y).num(t.rotation))
                .child(n("layer").str(layer_to_kicad(t.layer)))
                .child(n("uuid").str(uuid(&format!("text:{}", t.id))))
                .child(eff),
        );
    }

    // Tracks, vias, zones.
    for t in &d.routing.tracks {
        if !net_ids.contains_key(t.net.as_str()) {
            continue;
        }
        for (i, w) in t.points.windows(2).enumerate() {
            let net = copper_net(&t.net).unwrap_or_else(|| n("net").num(0.0));
            root = root.child(
                n("segment")
                    .child(n("start").num(w[0].x).num(-w[0].y))
                    .child(n("end").num(w[1].x).num(-w[1].y))
                    .child(n("width").num(t.width))
                    .child(n("layer").str(layer_to_kicad(t.layer)))
                    .child(net)
                    .child(n("uuid").str(uuid(&format!("seg:{}:{i}", t.id)))),
            );
        }
    }
    for v in &d.routing.vias {
        let Some(net) = copper_net(&v.net) else {
            continue;
        };
        let (from, to) = (v.from.unwrap_or(Layer::TOP), v.to.unwrap_or(Layer::BOTTOM));
        let mut node = n("via");
        if v.from.is_some() || v.to.is_some() {
            node = node.sym("blind");
        }
        root = root.child(
            node.child(n("at").num(v.at.x).num(-v.at.y))
                .child(n("size").num(v.diameter))
                .child(n("drill").num(v.drill))
                .child(
                    n("layers")
                        .str(layer_to_kicad(from))
                        .str(layer_to_kicad(to)),
                )
                .child(net)
                .child(n("uuid").str(uuid(&format!("via:{}", v.id)))),
        );
    }
    let geometry = lamina_geom::build(d);
    let fills = lamina_geom::fill::fill_zones(d, &geometry);
    for z in &d.routing.zones {
        let net = z
            .net
            .as_deref()
            .and_then(copper_net)
            .unwrap_or_else(|| n("net").num(0.0));
        let connect = match z.pad_connection {
            PadConnection::Thermal => n("connect_pads"),
            PadConnection::Solid => n("connect_pads").sym("yes"),
            PadConnection::None => n("connect_pads").sym("no"),
        }
        .child(n("clearance").num(z.clearance.unwrap_or(d.board.rules.clearance)));
        let mut zone = n("zone")
            .child(net)
            .child(n("net_name").str(z.net.clone().unwrap_or_default()))
            .child(layers_node(&z.layers))
            .child(n("uuid").str(uuid(&format!("zone:{}", z.id))))
            .child(n("name").str(&z.id))
            .child(n("hatch").sym("edge").num(0.5))
            .child(n("priority").num(z.priority as f64))
            .child(connect)
            .child(n("min_thickness").num(z.min_width.unwrap_or(d.board.rules.min_track_width)))
            .child(n("filled_areas_thickness").sym("no"))
            .child(
                n("fill")
                    .sym("yes")
                    .child(n("thermal_gap").num(z.thermal_gap))
                    .child(n("thermal_bridge_width").num(z.thermal_spoke)),
            )
            .child(polygon_node(&z.outline));
        for fill in fills.iter().filter(|f| f.zone == z.id) {
            for polygon in &fill.polygons {
                let mut pts = n("pts");
                for p in polygon.keyhole() {
                    pts = pts.child(n("xy").num(p.x).num(-p.y));
                }
                zone = zone.child(
                    n("filled_polygon")
                        .child(n("layer").str(layer_to_kicad(fill.layer)))
                        .child(pts),
                );
            }
        }
        root = root.child(zone);
    }
    for k in &d.routing.keepouts {
        let allow = |b: bool| if b { "not_allowed" } else { "allowed" };
        root = root.child(
            n("zone")
                .child(n("net").num(0.0))
                .child(n("net_name").str(""))
                .child(layers_node(&k.layers))
                .child(n("uuid").str(uuid(&format!("keepout:{}", k.id))))
                .child(n("name").str(&k.id))
                .child(n("hatch").sym("edge").num(0.5))
                .child(n("connect_pads").child(n("clearance").num(0.0)))
                .child(n("min_thickness").num(0.25))
                .child(n("filled_areas_thickness").sym("no"))
                .child(
                    n("keepout")
                        .child(n("tracks").sym(allow(k.forbid.tracks)))
                        .child(n("vias").sym(allow(k.forbid.vias)))
                        .child(n("pads").sym(allow(k.forbid.pads)))
                        .child(n("copperpour").sym(allow(k.forbid.pours)))
                        .child(n("footprints").sym(allow(k.forbid.parts))),
                )
                .child(
                    n("fill")
                        .child(n("thermal_gap").num(0.5))
                        .child(n("thermal_bridge_width").num(0.5)),
                )
                .child(polygon_node(&k.outline)),
        );
    }
    // The keep-outs of the footprints, at their board positions.
    let layer_count = d.board.stackup.copper_layers;
    for (refdes, pl) in &d.placement.placements {
        let Some(fp) = d
            .parts
            .parts
            .get(refdes)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            continue;
        };
        for (i, (ls, pts, forbid)) in lamina_geom::part_keepouts(fp, pl, layer_count)
            .into_iter()
            .enumerate()
        {
            let Some(outline) = contour_from_points(pts) else {
                continue;
            };
            let layers: Vec<Layer> = ls
                .iter()
                .map(|l| Layer::Copper(Copper::from_index(*l, layer_count)))
                .collect();
            let allow = |b: bool| if b { "not_allowed" } else { "allowed" };
            let id = format!("{refdes}-k{}", i + 1);
            root = root.child(
                n("zone")
                    .child(n("net").num(0.0))
                    .child(n("net_name").str(""))
                    .child(layers_node(&layers))
                    .child(n("uuid").str(uuid(&format!("keepout:{id}"))))
                    .child(n("name").str(&id))
                    .child(n("hatch").sym("edge").num(0.5))
                    .child(n("connect_pads").child(n("clearance").num(0.0)))
                    .child(n("min_thickness").num(0.25))
                    .child(n("filled_areas_thickness").sym("no"))
                    .child(
                        n("keepout")
                            .child(n("tracks").sym(allow(forbid.tracks)))
                            .child(n("vias").sym(allow(forbid.vias)))
                            .child(n("pads").sym(allow(forbid.pads)))
                            .child(n("copperpour").sym(allow(forbid.pours)))
                            .child(n("footprints").sym(allow(forbid.parts))),
                    )
                    .child(
                        n("fill")
                            .child(n("thermal_gap").num(0.5))
                            .child(n("thermal_bridge_width").num(0.5)),
                    )
                    .child(polygon_node(&outline)),
            );
        }
    }
    sexpr::write(&root.done())
}

fn layers_node(ls: &[Layer]) -> Node {
    if ls.len() == 1 {
        n("layer").str(layer_to_kicad(ls[0]))
    } else {
        let mut node = n("layers");
        for l in ls {
            node = node.str(layer_to_kicad(*l));
        }
        node
    }
}

fn polygon_node(c: &Contour) -> Node {
    let mut pts = n("pts");
    for p in c.flatten(0.01) {
        pts = pts.child(n("xy").num(p.x).num(-p.y));
    }
    n("polygon").child(pts)
}

/// Board drawing nodes (`gr_*`) for a contour. Arcs stay arcs.
fn contour_nodes(c: &Contour, layer: &str, width: f64, gi: &mut usize) -> Vec<Node> {
    let n_v = c.vertices.len();
    let mut out = Vec::new();
    if n_v == 1 {
        if let Some(a) = c.vertices[0].arc {
            let p = c.vertices[0].point();
            *gi += 1;
            out.push(
                n("gr_circle")
                    .child(n("center").num(a.center.x).num(-a.center.y))
                    .child(n("end").num(p.x).num(-p.y))
                    .child(stroke(width))
                    .child(n("fill").sym("no"))
                    .child(n("layer").str(layer))
                    .child(n("uuid").str(uuid(&format!("edge:{gi}")))),
            );
        }
        return out;
    }
    for i in 0..n_v {
        let v = c.vertices[i];
        let p0 = v.point();
        let p1 = c.vertices[(i + 1) % n_v].point();
        *gi += 1;
        let node = match v.arc {
            Some(a) => {
                let sweep = arc_sweep(p0, p1, a.center, a.ccw, false);
                let mid = p0
                    .sub(a.center)
                    .rotated(sweep.to_degrees() / 2.0)
                    .add(a.center);
                n("gr_arc")
                    .child(n("start").num(p0.x).num(-p0.y))
                    .child(n("mid").num(mid.x).num(-mid.y))
                    .child(n("end").num(p1.x).num(-p1.y))
            }
            None => n("gr_line")
                .child(n("start").num(p0.x).num(-p0.y))
                .child(n("end").num(p1.x).num(-p1.y)),
        };
        out.push(
            node.child(stroke(width))
                .child(n("layer").str(layer))
                .child(n("uuid").str(uuid(&format!("edge:{gi}")))),
        );
    }
    out
}

/// A `gr_*` or `fp_*` node for a shape. `xf` changes points to KiCad coordinates.
fn shape_node_with(
    shape: &Shape,
    prefix: &str,
    layer: &str,
    width: f64,
    filled: bool,
    id: &str,
    xf: &dyn Fn(Point) -> (f64, f64),
) -> Option<Node> {
    let pt = |name: &str, p: Point| {
        let (x, y) = xf(p);
        n(name).num(x).num(y)
    };
    let node = match shape {
        Shape::Line { a, b } => n(&format!("{prefix}_line"))
            .child(pt("start", *a))
            .child(pt("end", *b)),
        Shape::Rect { min, max } => n(&format!("{prefix}_rect"))
            .child(pt("start", *min))
            .child(pt("end", *max)),
        Shape::Circle { center, radius } => n(&format!("{prefix}_circle"))
            .child(pt("center", *center))
            .child(pt("end", Point::new(center.x + radius, center.y))),
        Shape::Arc {
            center,
            start,
            angle,
        } => {
            let mid = start.sub(*center).rotated(angle / 2.0).add(*center);
            let end = start.sub(*center).rotated(*angle).add(*center);
            n(&format!("{prefix}_arc"))
                .child(pt("start", *start))
                .child(pt("mid", mid))
                .child(pt("end", end))
        }
        Shape::Polygon { points } => {
            if points.len() < 3 {
                return None;
            }
            let mut pts = n("pts");
            for p in points {
                pts = pts.child(pt("xy", *p));
            }
            n(&format!("{prefix}_poly")).child(pts)
        }
    };
    let mut node = node.child(stroke(width));
    if matches!(
        shape,
        Shape::Rect { .. } | Shape::Circle { .. } | Shape::Polygon { .. }
    ) {
        node = node.child(n("fill").sym(if filled { "yes" } else { "no" }));
    }
    Some(
        node.child(n("layer").str(layer))
            .child(n("uuid").str(uuid(id))),
    )
}

fn shape_node(shape: &Shape, layer: &str, width: f64, filled: bool, id: &str) -> Option<Node> {
    shape_node_with(shape, "gr", layer, width, filled, id, &|p| (p.x, -p.y))
}

fn library_id(id: &str) -> String {
    let mut name = String::from("fp_");
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' {
            name.push(byte as char);
        } else {
            name.push_str(&format!("_{byte:02x}"));
        }
    }
    if name.len() > 120 {
        name.truncate(120);
        name.push_str(&uuid(id));
    }
    name
}

pub(crate) fn library_footprints(d: &ProjectData) -> BTreeMap<String, String> {
    let serialize = |node: Node, name: &str| {
        let Sx::List(mut items) = node.done() else {
            unreachable!()
        };
        items[1] = Sx::str(name);
        items.retain(|item| !item.is("at") && !item.is("uuid"));
        items.insert(2, n("version").num(20240108.0).done());
        items.insert(3, n("generator").str("lamina").done());
        sexpr::write(&Sx::List(items))
    };
    let mut files = BTreeMap::new();
    for fp in d.footprints.values() {
        let default_part = Part::new(&fp.id);
        let part = d
            .parts
            .parts
            .values()
            .find(|p| p.footprint == fp.id)
            .unwrap_or(&default_part);
        let name = library_id(&fp.id);
        let node = footprint_node(
            "REF**",
            part,
            &Placement::at(0.0, 0.0),
            fp,
            &BTreeMap::new(),
            &|_| None,
            None,
        );
        files.insert(name.clone(), serialize(node, &name));
    }
    for (i, hole) in d.board.holes.iter().enumerate() {
        let mut hole = hole.clone();
        hole.at = Point::new(0.0, 0.0);
        let name = format!("hole_{}", library_id(&hole.id));
        files.insert(
            name.clone(),
            serialize(hole_node(i, &hole, &|_| None), &name),
        );
    }
    files
}

fn footprint_node(
    refdes: &str,
    part: &Part,
    pl: &Placement,
    fp: &Footprint,
    pad_nets: &BTreeMap<PadRef, String>,
    net_node: &dyn Fn(&str) -> Option<Node>,
    model_dir: Option<&std::path::Path>,
) -> Node {
    let back = pl.side == Side::Bottom;
    // Local coordinates as KiCad keeps them: Y is flipped on the front and not
    // changed on the back.
    let lk = move |p: Point| if back { (p.x, p.y) } else { (p.x, -p.y) };
    let orient = if back {
        pl.rotation + 180.0
    } else {
        pl.rotation
    };
    let orient = orient.rem_euclid(360.0);
    let angle = move |rel: f64| (if back { orient - rel } else { orient + rel }).rem_euclid(360.0);
    let side_cu = if back { "B.Cu" } else { "F.Cu" };
    let lname = |l: FpLayer| fp_layer_to_kicad(l, pl.side);

    let text_at = |layer: FpLayer, key: &str, default: Point| -> (Point, f64, f64) {
        fp.texts
            .iter()
            .find(|t| t.text == key && t.layer == layer)
            .or_else(|| fp.texts.iter().find(|t| t.text == key))
            .map(|t| (t.at, t.size, t.rotation))
            .unwrap_or((default, 1.0, 0.0))
    };
    let prop = |name: &str, value: &str, layer: &str, at: (Point, f64, f64), hide: bool| {
        let (x, y) = lk(at.0);
        let mut eff = font(at.1);
        if back {
            eff = eff.child(n("justify").sym("mirror"));
        }
        let mut node = n("property")
            .str(name)
            .str(value)
            .child(n("at").num(x).num(y).num(angle(at.2)))
            .child(n("layer").str(layer));
        if hide {
            node = node.child(n("hide").sym("yes"));
        }
        node.child(n("uuid").str(uuid(&format!("prop:{refdes}:{name}"))))
            .child(eff)
    };
    let cy = fp.courtyard_bbox();
    let ref_default = Point::new(0.0, cy.map(|r| r.max.y + 0.8).unwrap_or(1.5));
    let ref_at = text_at(FpLayer::Silk, "${ref}", ref_default);
    let val_at = text_at(FpLayer::Fab, "${value}", Point::new(0.0, 0.0));
    // A footprint with no such text shows no such field in Lamina.
    // The Reference is on the silkscreen in KiCad. A footprint that has
    // its reference only on the Fab layer shows none on the silkscreen.
    let absent = |key: &str| {
        !fp.texts
            .iter()
            .any(|t| t.text == key && (key != "${ref}" || t.layer == FpLayer::Silk))
    };
    let smd = fp.pads.iter().any(Pad::is_smd);
    let mut attr = n("attr").sym(if smd { "smd" } else { "through_hole" });
    if part.dnp {
        attr = attr.sym("dnp");
    }
    if part
        .fields
        .get("exclude_from_bom")
        .is_some_and(|v| v == "yes")
    {
        attr = attr.sym("exclude_from_bom");
    }
    let mut node = n("footprint")
        .str(format!("lamina:{}", library_id(&fp.id)))
        .child(n("layer").str(side_cu))
        .child(n("uuid").str(uuid(&format!("fp:{refdes}"))))
        .child(n("at").num(pl.x).num(-pl.y).num(orient))
        .child_if(fp.description.is_some(), || {
            n("descr").str(fp.description.clone().unwrap_or_default())
        })
        .child(prop(
            "Reference",
            refdes,
            &lname(FpLayer::Silk),
            ref_at,
            absent("${ref}"),
        ))
        .child(prop(
            "Value",
            part.value.as_deref().unwrap_or(&fp.id),
            &lname(FpLayer::Fab),
            val_at,
            absent("${value}"),
        ))
        .child(prop(
            "Footprint",
            &format!("lamina:{}", library_id(&fp.id)),
            &lname(FpLayer::Fab),
            (Point::new(0.0, 0.0), 1.27, 0.0),
            true,
        ))
        .child(prop(
            "Datasheet",
            part.fields
                .get("datasheet")
                .map(String::as_str)
                .unwrap_or(""),
            &lname(FpLayer::Fab),
            (Point::new(0.0, 0.0), 1.27, 0.0),
            true,
        ))
        .child(prop(
            "Description",
            part.description.as_deref().unwrap_or(""),
            &lname(FpLayer::Fab),
            (Point::new(0.0, 0.0), 1.27, 0.0),
            true,
        ));
    for (k, v) in [
        ("MPN", &part.mpn),
        ("Manufacturer", &part.manufacturer),
        ("LCSC", &part.lcsc),
    ] {
        if let Some(v) = v {
            node = node.child(prop(
                k,
                v,
                &lname(FpLayer::Fab),
                (Point::new(0.0, 0.0), 1.27, 0.0),
                true,
            ));
        }
    }
    node = node.child(attr);

    for (i, g) in fp.graphics.iter().enumerate() {
        if let Some(gn) = shape_node_with(
            &g.shape,
            "fp",
            &lname(g.layer),
            g.width,
            g.filled,
            &format!("fpg:{refdes}:{i}"),
            &lk,
        ) {
            node = node.child(gn);
        }
    }
    for (i, t) in fp.texts.iter().enumerate() {
        if t.text == "${ref}" || t.text == "${value}" {
            continue;
        }
        let (x, y) = lk(t.at);
        let text = t
            .text
            .replace("${ref}", "${REFERENCE}")
            .replace("${value}", "${VALUE}");
        let mut eff = font(t.size);
        if back {
            eff = eff.child(n("justify").sym("mirror"));
        }
        node = node.child(
            n("fp_text")
                .sym("user")
                .str(text)
                .child(n("at").num(x).num(y).num(angle(t.rotation)))
                .child(n("layer").str(lname(t.layer)))
                .child(n("uuid").str(uuid(&format!("fpt:{refdes}:{i}"))))
                .child(eff),
        );
    }
    for (i, pad) in fp.pads.iter().enumerate() {
        node = node.child(pad_node(
            refdes, i, pad, pl.side, &lk, &angle, pad_nets, net_node,
        ));
    }
    if let Some(m) = &fp.model {
        node = node.child(
            n("model")
                .str(
                    model_dir
                        .map(|d| d.join(&m.file).to_string_lossy().into_owned())
                        .unwrap_or_else(|| m.file.clone())
                        .replace('\\', "/"),
                )
                .child(n("offset").child(n("xyz").nums(&m.offset)))
                .child(n("scale").child(n("xyz").nums(&m.scale)))
                .child(n("rotate").child(n("xyz").nums(&[
                    -m.rotation[0],
                    -m.rotation[1],
                    -m.rotation[2],
                ]))),
        );
    }
    node
}

fn pad_node(
    refdes: &str,
    i: usize,
    pad: &Pad,
    side: Side,
    lk: &dyn Fn(Point) -> (f64, f64),
    angle: &dyn Fn(f64) -> f64,
    pad_nets: &BTreeMap<PadRef, String>,
    net_node: &dyn Fn(&str) -> Option<Node>,
) -> Node {
    let (x, y) = lk(pad.at);
    let s = if side == Side::Top { "F" } else { "B" };
    let (ptype, drill) = match &pad.kind {
        PadKind::Smd => ("smd", None),
        PadKind::Through {
            drill,
            plated: true,
        } => ("thru_hole", Some(*drill)),
        PadKind::Through {
            drill,
            plated: false,
        } => ("np_thru_hole", Some(*drill)),
    };
    let (shape, size, extra): (&str, (f64, f64), Option<Node>) = match &pad.shape {
        PadShape::Circle { diameter } => ("circle", (*diameter, *diameter), None),
        PadShape::Rect { width, height } => ("rect", (*width, *height), None),
        PadShape::Oval { width, height } => ("oval", (*width, *height), None),
        PadShape::RoundRect {
            width,
            height,
            radius,
        } => (
            "roundrect",
            (*width, *height),
            Some(n("roundrect_rratio").num(r6(radius / width.min(*height)))),
        ),
        PadShape::Polygon { points } => {
            let mut pts = n("pts");
            for p in points {
                let (px, py) = lk(*p);
                pts = pts.child(n("xy").num(px).num(py));
            }
            let (w, h) = pad.shape.size();
            (
                "custom",
                (r6((w * 0.25).max(0.05)), r6((h * 0.25).max(0.05))),
                Some(
                    n("primitives").child(
                        n("gr_poly")
                            .child(pts)
                            .child(n("width").num(0.0))
                            .child(n("fill").sym("yes")),
                    ),
                ),
            )
        }
    };
    let mut node = n("pad")
        .str(&pad.number)
        .sym(ptype)
        .sym(shape)
        .child(n("at").num(x).num(y).num(angle(pad.rotation)))
        .child(n("size").num(size.0).num(size.1));
    if let Some(dr) = drill {
        node = node.child(match dr {
            Drill::Round { diameter } => n("drill").num(diameter),
            Drill::Slot { width, height } => n("drill").sym("oval").num(width).num(height),
        });
    }
    let mut layers = n("layers");
    match pad.kind {
        PadKind::Smd => {
            layers = layers.str(format!("{s}.Cu"));
            if pad.paste {
                layers = layers.str(format!("{s}.Paste"));
            }
            if pad.mask {
                layers = layers.str(format!("{s}.Mask"));
            }
        }
        PadKind::Through { .. } => {
            layers = layers.str("*.Cu");
            if pad.mask {
                layers = layers.str("*.Mask");
            }
        }
    }
    node = node.child(layers);
    if shape == "custom" {
        node = node.child(
            n("options")
                .child(n("clearance").sym("outline"))
                .child(n("anchor").sym("rect")),
        );
    }
    if let Some(e) = extra {
        node = node.child(e);
    }
    if let Some(m) = pad.mask_margin {
        node = node.child(n("solder_mask_margin").num(m));
    }
    if let Some(m) = pad.paste_margin {
        node = node.child(n("solder_paste_margin").num(m));
    }
    if !pad.number.is_empty() {
        if let Some(net) = pad_nets.get(&PadRef::new(refdes, pad.number.clone())) {
            if let Some(nn) = net_node(net) {
                node = node.child(nn);
            }
        }
    }
    node.child(n("uuid").str(uuid(&format!("pad:{refdes}:{i}"))))
}

fn hole_node(i: usize, h: &Hole, net_node: &dyn Fn(&str) -> Option<Node>) -> Node {
    let refdes = format!("H{}", i + 1);
    let d = h.drill.min_size();
    let pad_d = h.pad_diameter.unwrap_or(d);
    let (w, hgt) = match h.drill {
        Drill::Round { diameter } => (diameter, diameter),
        Drill::Slot { width, height } => (width, height),
    };
    let mut pad = n("pad")
        .str(if h.plated { "1" } else { "" })
        .sym(if h.plated {
            "thru_hole"
        } else {
            "np_thru_hole"
        })
        .sym(if w == hgt { "circle" } else { "oval" })
        .child(n("at").num(0.0).num(0.0).num(h.rotation))
        .child(n("size").num(r6(w.max(pad_d))).num(r6(hgt.max(pad_d))))
        .child(match h.drill {
            Drill::Round { diameter } => n("drill").num(diameter),
            Drill::Slot { width, height } => n("drill").sym("oval").num(width).num(height),
        })
        .child(n("layers").str("*.Cu").str("*.Mask"));
    if let Some(net) = h.net.as_deref().and_then(net_node) {
        pad = pad.child(net);
    }
    n("footprint")
        .str(format!("lamina:hole_{}", library_id(&h.id)))
        .child(n("layer").str("F.Cu"))
        .child(n("uuid").str(uuid(&format!("hole:{}", h.id))))
        .child(n("at").num(h.at.x).num(-h.at.y))
        .child(
            n("property")
                .str("Reference")
                .str(&refdes)
                .child(n("at").num(0.0).num(r6(-(pad_d / 2.0 + 1.0))).num(0.0))
                .child(n("layer").str("F.SilkS"))
                .child(n("hide").sym("yes"))
                .child(n("uuid").str(uuid(&format!("holeref:{}", h.id))))
                .child(font(1.0)),
        )
        .child(
            n("property")
                .str("Value")
                .str("Hole")
                .child(n("at").num(0.0).num(0.0).num(0.0))
                .child(n("layer").str("F.Fab"))
                .child(n("hide").sym("yes"))
                .child(n("uuid").str(uuid(&format!("holeval:{}", h.id))))
                .child(font(1.0)),
        )
        .child(
            n("attr")
                .sym("through_hole")
                .sym("exclude_from_bom")
                .sym("exclude_from_pos_files"),
        )
        .child(pad.child(n("uuid").str(uuid(&format!("holepad:{}", h.id)))))
}

/// An edge of a footprint in board coordinates. A part on the bottom side is
/// mirrored, so the direction of an arc changes.
fn place_edge(e: Edge, pl: &Placement) -> Edge {
    let flip = pl.side == Side::Bottom;
    let tf = |p: Point| {
        let q = pl.transform(p);
        Point::new(r6(q.x), r6(q.y))
    };
    match e {
        Edge::Line(a, b) => Edge::Line(tf(a), tf(b)),
        Edge::Arc { a, b, center, ccw } => Edge::Arc {
            a: tf(a),
            b: tf(b),
            center: tf(center),
            ccw: ccw != flip,
        },
        Edge::Loop(c) => Edge::Loop(Contour {
            vertices: c
                .vertices
                .iter()
                .map(|v| {
                    let p = tf(Point::new(v.x, v.y));
                    Vertex {
                        x: p.x,
                        y: p.y,
                        arc: v.arc.as_ref().map(|a| ArcTo {
                            center: tf(a.center),
                            ccw: a.ccw != flip,
                        }),
                    }
                })
                .collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_fp(lib: &str, name: &str) -> Footprint {
        let p = format!(
            "{}/tests/fixtures/footprints/{lib}.pretty/{name}.kicad_mod",
            env!("CARGO_MANIFEST_DIR")
        );
        footprint::parse_footprint(&std::fs::read_to_string(&p).unwrap(), lib, &p)
            .unwrap()
            .footprint
    }

    fn sample_board() -> ProjectData {
        let mut d = ProjectData::new("rt");
        d.board.outline = Some(Contour::rounded_rect(0.0, 0.0, 30.0, 20.0, 2.0));
        d.board
            .cutouts
            .push(Contour::circle(Point::new(25.0, 10.0), 1.5));
        d.board.holes.push(Hole {
            id: "h1".into(),
            at: Point::new(3.0, 3.0),
            drill: Drill::Round { diameter: 2.2 },
            plated: false,
            pad_diameter: None,
            net: None,
            rotation: 0.0,
        });
        for fp in [
            fixture_fp("Resistor_SMD", "R_0603_1608Metric"),
            fixture_fp("Package_TO_SOT_SMD", "SOT-23"),
            fixture_fp(
                "Connector_PinHeader_2.54mm",
                "PinHeader_1x03_P2.54mm_Vertical",
            ),
        ] {
            d.footprints.insert(fp.id.clone(), fp);
        }
        let mut r1 = Part::new("R_0603_1608Metric");
        r1.value = Some("10k".into());
        d.parts.parts.insert("R1".into(), r1);
        d.parts
            .parts
            .insert("R2".into(), Part::new("R_0603_1608Metric"));
        d.parts.parts.insert("Q1".into(), Part::new("SOT-23"));
        d.parts
            .parts
            .insert("J1".into(), Part::new("PinHeader_1x03_P2.54mm_Vertical"));
        d.placement.placements.insert(
            "R1".into(),
            Placement {
                x: 5.0,
                y: 5.0,
                rotation: 90.0,
                side: Side::Top,
                locked: false,
            },
        );
        d.placement.placements.insert(
            "R2".into(),
            Placement {
                x: 8.0,
                y: 12.5,
                rotation: 30.0,
                side: Side::Bottom,
                locked: false,
            },
        );
        d.placement.placements.insert(
            "Q1".into(),
            Placement {
                x: 15.0,
                y: 10.0,
                rotation: 180.0,
                side: Side::Bottom,
                locked: false,
            },
        );
        d.placement.placements.insert(
            "J1".into(),
            Placement {
                x: 22.0,
                y: 4.0,
                rotation: 0.0,
                side: Side::Top,
                locked: false,
            },
        );
        d.nets.nets.insert(
            "GND".into(),
            Net {
                pads: vec![
                    PadRef::new("R1", "2"),
                    PadRef::new("Q1", "2"),
                    PadRef::new("J1", "1"),
                ],
                class: None,
                ..Default::default()
            },
        );
        d.nets.nets.insert(
            "SIG".into(),
            Net {
                pads: vec![
                    PadRef::new("R1", "1"),
                    PadRef::new("R2", "1"),
                    PadRef::new("Q1", "1"),
                ],
                class: None,
                ..Default::default()
            },
        );
        d.routing.tracks.push(Track {
            id: "t1".into(),
            net: "SIG".into(),
            layer: Layer::TOP,
            width: 0.25,
            points: vec![
                Point::new(5.0, 5.825),
                Point::new(8.0, 5.825),
                Point::new(10.0, 7.0),
            ],
        });
        d.routing.vias.push(Via {
            id: "v1".into(),
            net: "SIG".into(),
            at: Point::new(10.0, 7.0),
            diameter: 0.6,
            drill: 0.3,
            from: None,
            to: None,
        });
        d.routing.zones.push(Zone {
            id: "z1".into(),
            net: Some("GND".into()),
            layers: vec![Layer::TOP, Layer::BOTTOM],
            outline: Contour::rect(0.0, 0.0, 30.0, 20.0),
            priority: 1,
            clearance: Some(0.3),
            min_width: Some(0.2),
            pad_connection: PadConnection::Thermal,
            thermal_gap: 0.3,
            thermal_spoke: 0.4,
        });
        d.routing.keepouts.push(Keepout {
            id: "k1".into(),
            layers: vec![Layer::TOP],
            outline: Contour::rect(26.0, 14.0, 3.0, 5.0),
            forbid: Forbid {
                tracks: true,
                vias: true,
                pours: true,
                pads: false,
                parts: false,
            },
        });
        d.routing.texts.push(BoardText {
            id: "x1".into(),
            layer: Layer::Silk(Side::Top),
            text: "LAMINA".into(),
            at: Point::new(15.0, 2.0),
            size: 1.2,
            rotation: 0.0,
        });
        d
    }

    /// The absolute board positions of all pads, to compare two boards.
    fn pad_positions(d: &ProjectData) -> BTreeMap<String, (Point, f64)> {
        let mut out = BTreeMap::new();
        for (r, pl) in &d.placement.placements {
            let fp = &d.footprints[&d.parts.parts[r].footprint];
            for p in &fp.pads {
                if !p.number.is_empty() {
                    out.insert(
                        format!("{r}.{}", p.number),
                        (
                            pl.transform(p.at),
                            pl.transform_angle(p.rotation).rem_euclid(180.0),
                        ),
                    );
                }
            }
        }
        out
    }

    #[test]
    fn round_trip_through_kicad_format() {
        let d = sample_board();
        let text = export_board(&d);
        assert!(text.starts_with("(kicad_pcb\n\t(version 20240108)"));
        let back = import_board(&text, "rt").unwrap();
        let e = &back.data;
        assert_eq!(
            e.parts.parts.len(),
            5,
            "4 parts and the hole: {:?}",
            back.notes
        );
        assert_eq!(e.board.stackup.copper_layers, 2);
        let a = pad_positions(&d);
        let b = pad_positions(e);
        assert_eq!(a.len(), b.len());
        for (k, (pa, aa)) in &a {
            let (pb, ab) = &b[k];
            assert!(pa.dist(*pb) < 1e-5, "{k}: {pa:?} vs {pb:?}");
            assert!(
                (aa - ab).abs() < 1e-6 || (aa - ab).abs() > 179.999,
                "{k}: angle {aa} vs {ab}"
            );
        }
        for r in ["R1", "R2", "Q1", "J1"] {
            assert_eq!(
                e.placement.placements[r].side, d.placement.placements[r].side,
                "{r}"
            );
        }
        assert_eq!(e.parts.parts["R1"].value.as_deref(), Some("10k"));
        assert_eq!(e.nets.nets["GND"].pads.len(), 3);
        assert_eq!(e.nets.nets["SIG"].pads.len(), 3);
        assert_eq!(e.routing.tracks.len(), 2, "one segment per pair of points");
        assert_eq!(e.routing.tracks[0].points[0], Point::new(5.0, 5.825));
        assert_eq!(e.routing.vias[0].at, Point::new(10.0, 7.0));
        assert_eq!(e.routing.zones.len(), 1);
        assert_eq!(e.routing.zones[0].layers.len(), 2);
        assert_eq!(e.routing.zones[0].clearance, Some(0.3));
        assert_eq!(e.routing.keepouts.len(), 1);
        assert!(e.routing.keepouts[0].forbid.tracks && !e.routing.keepouts[0].forbid.pads);
        assert_eq!(e.routing.texts[0].text, "LAMINA");
        let o = e.board.outline.as_ref().unwrap();
        assert_eq!(o.vertices.len(), 8, "rounded rectangle keeps its four arcs");
        // The import calculates the arc centres from three points with 6 decimals.
        // Thus, allow a small tolerance.
        assert!((o.signed_area() - d.board.outline.as_ref().unwrap().signed_area()).abs() < 1e-4);
        assert_eq!(e.board.cutouts.len(), 1);
        // The free hole came back as a one-pad footprint.
        assert!(e.parts.parts.contains_key("H1"));
    }

    #[test]
    fn copper_items_name_the_net_by_number_only() {
        let text = export_board(&sample_board());
        let body = text.split("(segment").nth(1).unwrap();
        assert!(!regex_free_net_name(body), "{body}");
    }

    /// True if a `(net N "name")` node is in the text.
    fn regex_free_net_name(text: &str) -> bool {
        text.split("(net ").skip(1).any(|r| {
            let r = r.trim_start_matches(|c: char| c.is_ascii_digit());
            r.trim_start().starts_with('"')
        })
    }

    fn property<'a>(text: &'a str, name: &str, refdes: &str) -> &'a str {
        let from = text
            .find(&format!("(property \"{name}\" \"{refdes}\""))
            .or_else(|| {
                let fp = text.find(&format!("(property \"Reference\" \"{refdes}\""))?;
                Some(fp + text[fp..].find(&format!("(property \"{name}\""))?)
            })
            .unwrap();
        let end = text[from + 9..].find("(property").unwrap();
        &text[from..from + 9 + end]
    }

    #[test]
    fn reference_is_hidden_if_the_footprint_has_no_reference_text() {
        let mut d = sample_board();
        for fp in d.footprints.values_mut() {
            fp.texts
                .retain(|t| t.text != "${ref}" && t.text != "${value}");
        }
        let text = export_board(&d);
        assert!(property(&text, "Reference", "R1").contains("(hide yes)"));
        assert!(property(&text, "Value", "R1").contains("(hide yes)"));
    }

    #[test]
    fn reference_is_visible_if_the_footprint_has_reference_text() {
        let mut d = sample_board();
        let id = d.parts.parts["R1"].footprint.clone();
        let fp = d.footprints.get_mut(&id).unwrap();
        if !fp.texts.iter().any(|t| t.text == "${ref}") {
            fp.texts.push(FpText {
                text: "${ref}".into(),
                layer: FpLayer::Silk,
                at: Point::new(0.0, 2.0),
                size: 1.0,
                rotation: 0.0,
            });
        }
        let text = export_board(&d);
        assert!(!property(&text, "Reference", "R1").contains("(hide yes)"));
    }

    #[test]
    fn project_file_keeps_the_rules_and_classes() {
        let mut d = sample_board();
        d.board.rules.clearance = 0.127;
        d.board.rules.min_track_width = 0.09;
        d.board.rules.net_classes.insert(
            "Power".into(),
            NetClass {
                clearance: Some(0.3),
                track_width: Some(0.5),
                via_diameter: None,
                via_drill: None,
                reference_plane: false,
            },
        );
        d.nets.nets.get_mut("GND").unwrap().class = Some("Power".into());
        let pro = crate::export_project(&d, "board");
        let mut back = ProjectData::new("x");
        back.nets.nets = d.nets.nets.clone();
        back.nets.nets.get_mut("GND").unwrap().class = None;
        crate::apply_project(&mut back, &pro).unwrap();
        assert_eq!(back.board.rules.clearance, 0.127);
        assert_eq!(back.board.rules.min_track_width, 0.09);
        assert_eq!(back.board.rules.net_classes["Power"].clearance, Some(0.3));
        assert_eq!(back.nets.nets["GND"].class.as_deref(), Some("Power"));
    }

    #[test]
    fn chains_edges_in_any_order() {
        let edges = vec![
            Edge::Line(Point::new(10.0, 0.0), Point::new(10.0, 10.0)),
            Edge::Line(Point::new(0.0, 0.0), Point::new(10.0, 0.0)),
            Edge::Line(Point::new(0.0, 10.0), Point::new(0.0, 0.0)),
            Edge::Line(Point::new(0.0, 10.0), Point::new(10.0, 10.0)),
            Edge::Line(Point::new(50.0, 50.0), Point::new(60.0, 50.0)),
        ];
        let (loops, open) = chain_edges(edges, 0.01);
        assert_eq!(loops.len(), 1);
        assert_eq!(open, 1);
        assert!(loops[0].signed_area() > 0.0);
        assert_eq!(loops[0].vertices.len(), 4);
    }

    #[test]
    fn imports_kicad_9_style_board() {
        let src = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
          (general (thickness 1.6) (legacy_teardrops no))
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (5 "F.SilkS" user "F.Silkscreen"))
          (net 0 "") (net 1 "GND") (net 2 "VCC")
          (footprint "Resistor_SMD:R_0603" (layer "F.Cu") (uuid "a") (at 10 10 0)
            (property "Reference" "R1" (at 0 -1.4 0) (layer "F.SilkS") (uuid "b") (effects (font (size 1 1) (thickness 0.15))))
            (property "Value" "4k7" (at 0 1.4 0) (layer "F.Fab") (uuid "c") (effects (font (size 1 1) (thickness 0.15))))
            (property "LCSC" "C23162" (at 0 0 0) (layer "F.Fab") (hide yes) (uuid "d") (effects (font (size 1 1))))
            (attr smd)
            (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25) (net 1 "GND") (uuid "e"))
            (pad "2" smd roundrect (at 0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25) (net 2 "VCC") (uuid "f")))
          (gr_rect (start 0 0) (end 40 30) (stroke (width 0.05) (type default)) (fill no) (layer "Edge.Cuts") (uuid "g"))
          (segment (start 10.825 10) (end 20 10) (width 0.3) (layer "F.Cu") (net 2) (uuid "h"))
          (via (at 20 10) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 2) (uuid "i"))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (uuid "j") (hatch edge 0.5) (connect_pads (clearance 0.5)) (min_thickness 0.25)
            (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy 0 0) (xy 40 0) (xy 40 30) (xy 0 30)))))"#;
        let b = import_board(src, "demo").unwrap();
        let d = &b.data;
        assert_eq!(d.board.name, "demo");
        assert_eq!(d.parts.parts["R1"].lcsc.as_deref(), Some("C23162"));

        assert_eq!(d.nets.nets["GND"].pads, vec![PadRef::new("R1", "1")]);

        assert_eq!(d.routing.vias[0].diameter, 0.8);
        assert_eq!(d.routing.zones[0].layers, vec![Layer::BOTTOM]);
        let o = d.board.outline.as_ref().unwrap();
        assert!((o.signed_area() - 1200.0).abs() < 1e-6);
        // The board was at y = 0..30 in KiCad (Y down). After the move, its corner
        // is at the origin.
        assert_eq!(o.bbox().unwrap().min, Point::new(0.0, 0.0));
        assert_eq!(d.placement.placements["R1"].y, 20.0);
        assert_eq!(d.routing.tracks[0].points[1], Point::new(20.0, 20.0));
    }
}

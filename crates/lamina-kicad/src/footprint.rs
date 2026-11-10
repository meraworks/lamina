//! KiCad footprints: `.kicad_mod` files and the footprints in a board.
//! In KiCad, the Y axis points down and the pad angles in a board are
//! absolute. This module converts both.

use lamina_ports::format::{Loss, LossKind, lost};
use std::collections::BTreeMap;

use lamina_model::*;

use crate::layers::{FpLayerMap, fp_layer_from_kicad};
use lamina_model::sexpr::{self, Sx};

/// A footprint from KiCad, with the data that a board instance adds.
#[derive(Debug, Clone)]
pub struct Instance {
    pub footprint: Footprint,
    pub library: String,
    pub name: String,
    pub side: Side,
    /// Board position (Y up) and rotation of a placed footprint.
    pub at: Point,
    pub rotation: f64,
    pub reference: String,
    pub value: Option<String>,
    pub properties: BTreeMap<String, String>,
    pub dnp: bool,
    pub exclude_from_bom: bool,
    /// Pad number to net name, from a board file.
    pub pad_nets: Vec<(String, String)>,
    /// The visible 3D models, in file order. The first model that resolves is used.
    pub models: Vec<KicadModel>,
    /// The drawings on Edge.Cuts, in footprint coordinates: slots and
    /// cut-outs that the part needs in the board.
    pub edge: Vec<Shape>,
    pub notes: Vec<String>,
    /// The data of the footprint that is not in the result, with its kind.
    pub losses: Vec<Loss>,
}

/// The `(model ...)` entry. The KiCad path variables are not expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct KicadModel {
    pub path: String,
    pub offset: [f64; 3],
    /// Already in the Lamina direction. KiCad applies these values with the opposite sign.
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
}

/// Parse a `.kicad_mod` file. The provenance records `origin`.
pub fn parse_footprint(src: &str, library: &str, origin: &str) -> Result<Instance, String> {
    let node = sexpr::parse(src)?;
    if !(node.is("footprint") || node.is("module")) {
        return Err(format!(
            "the file is not a KiCad footprint (it starts with `{}`)",
            node.name().unwrap_or("?")
        ));
    }
    convert(&node, library, origin)
}

/// A file-safe footprint id from a KiCad name.
pub fn footprint_id(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    while s.starts_with('.') {
        s.remove(0);
    }
    if s.is_empty() {
        s = "footprint".into();
    }
    s.truncate(120);
    s
}

struct Ctx {
    side: Side,
    /// Footprint rotation on the board (0 in a library).
    rot: f64,
}

impl Ctx {
    /// Convert local KiCad coordinates to Lamina footprint-local coordinates
    /// (Y up). KiCad keeps back-side footprints flipped, and this cancels the
    /// Y flip.
    fn lp(&self, x: f64, y: f64) -> Point {
        if self.side == Side::Bottom {
            Point::new(r6(x), r6(y))
        } else {
            Point::new(r6(x), r6(-y))
        }
    }

    /// Footprint-relative angle from an absolute KiCad angle.
    fn angle(&self, a: f64) -> f64 {
        let rel = if self.side == Side::Bottom {
            self.rot - a
        } else {
            a - self.rot
        };
        norm_deg(rel)
    }
}

fn r6(v: f64) -> f64 {
    json::round6(v)
}

fn norm_deg(a: f64) -> f64 {
    let r = r6(a.rem_euclid(360.0));
    if r >= 360.0 { 0.0 } else { r }
}

/// Convert a `(footprint ...)` node from a library or a board.
pub fn convert(node: &Sx, library_hint: &str, origin: &str) -> Result<Instance, String> {
    let full = node.arg(0).ok_or("the footprint has no name")?.to_string();
    let (library, name) = match full.split_once(':') {
        Some((l, n)) => (l.to_string(), n.to_string()),
        None => (library_hint.to_string(), full.clone()),
    };
    let side = match node.child_text("layer") {
        Some("B.Cu") => Side::Bottom,
        _ => Side::Top,
    };
    let (ax, ay, arot) = node
        .child("at")
        .map(|a| {
            (
                a.arg_num(0).unwrap_or(0.0),
                a.arg_num(1).unwrap_or(0.0),
                a.arg_num(2).unwrap_or(0.0),
            )
        })
        .unwrap_or((0.0, 0.0, 0.0));
    let ctx = Ctx { side, rot: arot };
    let mut edge: Vec<Shape> = Vec::new();
    let mut fp = Footprint::new(
        footprint_id(&name),
        Provenance::Kicad {
            library: library.clone(),
            footprint: name.clone(),
            path: origin.to_string(),
        },
    );
    fp.description = node.child_text("descr").map(str::to_string);
    let mut notes = Vec::new();
    let mut reference = String::new();
    let mut value = None;
    let mut properties = BTreeMap::new();
    let mut pad_nets = Vec::new();
    let mut models = Vec::new();
    let mut dnp = false;
    let mut exclude_from_bom = false;
    let mut skipped: BTreeMap<(LossKind, String), usize> = BTreeMap::new();
    let mut losses: Vec<Loss> = Vec::new();

    for item in node.args() {
        let Some(kind) = item.name() else { continue };
        match kind {
            "attr" => {
                for a in item.atoms() {
                    match a {
                        "dnp" => dnp = true,
                        "exclude_from_bom" => exclude_from_bom = true,
                        _ => {}
                    }
                }
            }
            "property" => {
                let key = item.arg(0).unwrap_or("").to_string();
                let val = item.arg(1).unwrap_or("").to_string();
                match key.as_str() {
                    "Reference" => {
                        reference = val.clone();
                        push_text(&mut fp, item, "${ref}", &ctx, &mut skipped);
                    }
                    "Value" => {
                        value = Some(val.clone());
                        push_text(&mut fp, item, "${value}", &ctx, &mut skipped);
                    }
                    "Footprint" | "ki_fp_filters" => {}
                    _ => {
                        if !val.is_empty() {
                            properties.insert(key, val);
                        }
                    }
                }
            }
            "fp_text" => {
                // KiCad 5-7: (fp_text reference R1 ...), (fp_text user "..." ...)
                let which = item.arg(0).unwrap_or("");
                let text = item.arg(1).unwrap_or("").to_string();
                match which {
                    "reference" => {
                        reference = text;
                        push_text(&mut fp, item, "${ref}", &ctx, &mut skipped);
                    }
                    "value" => {
                        value = Some(text);
                        push_text(&mut fp, item, "${value}", &ctx, &mut skipped);
                    }
                    _ => {
                        let t = text
                            .replace("${REFERENCE}", "${ref}")
                            .replace("${VALUE}", "${value}");
                        push_text(&mut fp, item, &t, &ctx, &mut skipped);
                    }
                }
            }
            "fp_line" | "fp_rect" | "fp_circle" | "fp_arc" | "fp_poly" => {
                let layer_name = item.child_text("layer").unwrap_or("");
                match fp_layer_from_kicad(layer_name, side) {
                    FpLayerMap::Keep(layer) => {
                        if let Some(shape) = shape_from(item, &|x, y| ctx.lp(x, y)) {
                            fp.graphics.push(FpGraphic {
                                layer,
                                shape,
                                width: stroke_width(item),
                                filled: is_filled(item),
                            });
                        }
                    }
                    FpLayerMap::Drop => {
                        if layer_name == "Edge.Cuts" {
                            if let Some(shape) = shape_from(item, &|x, y| ctx.lp(x, y)) {
                                edge.push(shape);
                            }
                        }
                    }
                    FpLayerMap::OtherSide => {
                        *skipped
                            .entry((LossKind::Other, format!("graphics on {layer_name}")))
                            .or_default() += 1
                    }
                }
            }
            "pad" => match pad_from(item, &ctx) {
                Ok(Some((pad, net))) => {
                    if let Some(n) = net {
                        if !pad.number.is_empty() {
                            pad_nets.push((pad.number.clone(), n));
                        }
                    }
                    fp.pads.push(pad);
                }
                Ok(None) => {
                    // A pad with no copper is a paste or mask opening.
                    let openings = openings_from(item, &ctx);
                    if openings.is_empty() {
                        *skipped
                            .entry((LossKind::Pad, "pads without copper on this side".into()))
                            .or_default() += 1
                    } else {
                        fp.graphics.extend(openings);
                    }
                }
                Err(e) => lost(
                    &mut notes,
                    &mut losses,
                    LossKind::Pad,
                    format!("skipped pad {}: {e}", item.arg(0).unwrap_or("?")),
                ),
            },
            "model" => {
                let hidden = item.child("hide").is_some_and(|h| h.arg(0) != Some("no"))
                    || item.args().iter().any(|a| a.text() == Some("hide"));
                if !hidden {
                    models.extend(model_from(item));
                }
            }
            "zone" => match keepout_from(item, &ctx, (ax, ay)) {
                Some(k) => fp.keepouts.push(k),
                None => {
                    *skipped
                        .entry((LossKind::Keepout, "footprint zones".into()))
                        .or_default() += 1
                }
            },
            _ => {}
        }
    }
    for ((kind, what), n) in skipped {
        lost(&mut notes, &mut losses, kind, format!("skipped {n} {what}"));
    }
    if fp.pads.is_empty() {
        notes.push("the footprint has no pads".into());
    }
    Ok(Instance {
        footprint: fp,
        library,
        name,
        side,
        at: Point::new(r6(ax), r6(-ay)),
        rotation: norm_deg(if side == Side::Bottom {
            arot - 180.0
        } else {
            arot
        }),
        reference,
        value,
        properties,
        dnp,
        exclude_from_bom,
        edge,
        losses,
        pad_nets,
        models,
        notes,
    })
}

fn push_text(
    fp: &mut Footprint,
    item: &Sx,
    text: &str,
    ctx: &Ctx,
    skipped: &mut BTreeMap<(LossKind, String), usize>,
) {
    let hidden = item.child("hide").is_some_and(|h| h.arg(0) != Some("no"))
        || item.args().iter().any(|a| a.text() == Some("hide"));
    if hidden {
        return;
    }
    let layer_name = item.child_text("layer").unwrap_or("F.SilkS");
    let layer = match fp_layer_from_kicad(layer_name, ctx.side) {
        FpLayerMap::Keep(l) => l,
        FpLayerMap::Drop => return,
        FpLayerMap::OtherSide => {
            *skipped
                .entry((LossKind::Other, format!("text on {layer_name}")))
                .or_default() += 1;
            return;
        }
    };
    let at = item.child("at");
    let (x, y, a) = at
        .map(|a| {
            (
                a.arg_num(0).unwrap_or(0.0),
                a.arg_num(1).unwrap_or(0.0),
                a.arg_num(2).unwrap_or(0.0),
            )
        })
        .unwrap_or((0.0, 0.0, 0.0));
    let size = item
        .child("effects")
        .and_then(|e| e.child("font"))
        .and_then(|f| f.child("size"))
        .and_then(|s| s.arg_num(1).or(s.arg_num(0)))
        .unwrap_or(1.0);
    fp.texts.push(FpText {
        layer,
        text: text.to_string(),
        at: ctx.lp(x, y),
        size: r6(size),
        rotation: ctx.angle(a),
    });
}

fn stroke_width(item: &Sx) -> f64 {
    item.child("stroke")
        .and_then(|s| s.child_num("width"))
        .or_else(|| item.child_num("width"))
        .map(r6)
        .unwrap_or(0.1)
}

fn is_filled(item: &Sx) -> bool {
    matches!(item.child_text("fill"), Some("yes") | Some("solid"))
}

/// The geometry of `fp_*` / `gr_*` drawing nodes, transformed with `xf`.
pub(crate) fn shape_from(item: &Sx, xf: &dyn Fn(f64, f64) -> Point) -> Option<Shape> {
    let kind = item.name()?;
    let kind = kind
        .strip_prefix("fp_")
        .or_else(|| kind.strip_prefix("gr_"))?;
    let pt = |name: &str| item.xy(name).map(|(x, y)| xf(x, y));
    Some(match kind {
        "line" => Shape::Line {
            a: pt("start")?,
            b: pt("end")?,
        },
        "rect" => {
            let a = pt("start")?;
            let b = pt("end")?;
            Shape::Rect {
                min: Point::new(a.x.min(b.x), a.y.min(b.y)),
                max: Point::new(a.x.max(b.x), a.y.max(b.y)),
            }
        }
        "circle" => {
            let c = pt("center")?;
            let e = pt("end")?;
            Shape::Circle {
                center: c,
                radius: r6(c.dist(e)),
            }
        }
        "arc" => {
            let s = pt("start")?;
            let e = pt("end")?;
            match pt("mid") {
                Some(m) => arc_from_three(s, m, e)?,
                None => {
                    // KiCad 5: (fp_arc (start cx cy) (end x y) (angle a)). The start is the centre.
                    let a = item.child_num("angle")?;
                    Shape::Arc {
                        center: s,
                        start: e,
                        angle: r6(-a),
                    }
                }
            }
        }
        "poly" => Shape::Polygon {
            points: points_from(item.child("pts")?, xf),
        },
        _ => return None,
    })
}

/// `(pts (xy x y) ... (arc (start) (mid) (end)))` as points.
pub(crate) fn points_from(pts: &Sx, xf: &dyn Fn(f64, f64) -> Point) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for p in pts.args() {
        if p.is("xy") {
            if let (Some(x), Some(y)) = (p.arg_num(0), p.arg_num(1)) {
                out.push(xf(x, y));
            }
        } else if p.is("arc") {
            let pt = |name: &str| p.xy(name).map(|(x, y)| xf(x, y));
            if let (Some(s), Some(m), Some(e)) = (pt("start"), pt("mid"), pt("end")) {
                if let Some(Shape::Arc {
                    center,
                    start,
                    angle,
                }) = arc_from_three(s, m, e)
                {
                    let n = 12;
                    for k in 0..=n {
                        let a = angle * k as f64 / n as f64;
                        out.push(start.sub(center).rotated(a).add(center));
                    }
                    continue;
                }
                out.push(s);
                out.push(e);
            }
        }
    }
    out.dedup_by(|a, b| a.dist(*b) < 1e-9);
    if out.len() > 1 && out[0].dist(*out.last().unwrap()) < 1e-9 {
        out.pop();
    }
    out
}

/// The arc through three points, as the centre, the start and the signed sweep.
pub(crate) fn arc_from_three(s: Point, m: Point, e: Point) -> Option<Shape> {
    let d = 2.0 * (s.x * (m.y - e.y) + m.x * (e.y - s.y) + e.x * (s.y - m.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let s2 = s.x * s.x + s.y * s.y;
    let m2 = m.x * m.x + m.y * m.y;
    let e2 = e.x * e.x + e.y * e.y;
    let cx = (s2 * (m.y - e.y) + m2 * (e.y - s.y) + e2 * (s.y - m.y)) / d;
    let cy = (s2 * (e.x - m.x) + m2 * (s.x - e.x) + e2 * (m.x - s.x)) / d;
    let a0 = (s.y - cy).atan2(s.x - cx);
    let a1 = (e.y - cy).atan2(e.x - cx);
    let am = (m.y - cy).atan2(m.x - cx);
    let tau = std::f64::consts::TAU;
    let ccw = (a1 - a0).rem_euclid(tau);
    let mid = (am - a0).rem_euclid(tau);
    let sweep = if mid <= ccw { ccw } else { ccw - tau };
    Some(Shape::Arc {
        center: Point::new(r6(cx), r6(cy)),
        start: s,
        angle: r6(sweep.to_degrees()),
    })
}

/// A keep-out zone of a footprint. In a board file, the points of the
/// zone are board positions. In a library file the footprint is at the
/// origin, so the two are the same.
fn keepout_from(zone: &Sx, ctx: &Ctx, origin: (f64, f64)) -> Option<FpKeepout> {
    let k = zone.child("keepout")?;
    let mut front = false;
    let mut back = false;
    let mut other = false;
    for l in zone.children("layer").chain(zone.children("layers")) {
        for a in l.atoms() {
            match a {
                "F.Cu" => front = true,
                "B.Cu" => back = true,
                "F&B.Cu" => {
                    front = true;
                    back = true;
                }
                x if x.ends_with(".Cu") => other = true,
                _ => {}
            }
        }
    }
    if ctx.side == Side::Bottom {
        std::mem::swap(&mut front, &mut back);
    }
    let scope = match (front, back, other) {
        (true, false, false) => KeepoutScope::Front,
        (false, true, false) => KeepoutScope::Back,
        (false, false, false) => return None,
        _ => KeepoutScope::AllCopper,
    };
    let pts = zone.child("polygon").and_then(|p| p.child("pts"))?;
    let (sin, cos) = ctx.rot.to_radians().sin_cos();
    let points: Vec<Point> = pts
        .children("xy")
        .filter_map(|xy| Some((xy.arg_num(0)?, xy.arg_num(1)?)))
        .map(|(x, y)| {
            // Board position to footprint position: undo the move and the turn.
            let (dx, dy) = (x - origin.0, y - origin.1);
            ctx.lp(dx * cos - dy * sin, dx * sin + dy * cos)
        })
        .collect();
    let outline = contour_from_points(points)?;
    let allowed = |what: &str| k.child_text(what) != Some("not_allowed");
    Some(FpKeepout {
        scope,
        outline,
        forbid: Forbid {
            tracks: !allowed("tracks"),
            vias: !allowed("vias"),
            pours: !allowed("copperpour"),
            pads: !allowed("pads"),
            parts: !allowed("footprints"),
        },
    })
}

/// The paste and mask openings of a pad that has no copper on the side of
/// the part: one filled drawing for each layer.
fn openings_from(item: &Sx, ctx: &Ctx) -> Vec<FpGraphic> {
    let atoms = item.atoms();
    if !matches!(atoms.get(1).copied(), Some("smd" | "connect")) {
        return Vec::new();
    }
    let (Some(at), Some((w, h))) = (item.child("at"), item.xy("size")) else {
        return Vec::new();
    };
    let (Some(x), Some(y)) = (at.arg_num(0), at.arg_num(1)) else {
        return Vec::new();
    };
    if !(w > 0.0 && h > 0.0) {
        return Vec::new();
    }
    let centre = ctx.lp(x, y);
    let angle = ctx.angle(at.arg_num(2).unwrap_or(0.0));
    let shape = match atoms.get(2).copied() {
        Some("circle") => Shape::Circle {
            center: centre,
            radius: r6(w / 2.0),
        },
        Some("rect" | "roundrect" | "oval") => Shape::Polygon {
            points: [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|(sx, sy)| {
                    let p = Point::new(sx * w / 2.0, sy * h / 2.0).rotated(angle);
                    Point::new(r6(centre.x + p.x), r6(centre.y + p.y))
                })
                .collect(),
        },
        _ => return Vec::new(),
    };
    let layers: Vec<String> = item
        .child("layers")
        .map(|l| l.atoms().into_iter().map(str::to_string).collect())
        .unwrap_or_default();
    let (paste, mask) = if ctx.side == Side::Top {
        ("F.Paste", "F.Mask")
    } else {
        ("B.Paste", "B.Mask")
    };
    let mut out = Vec::new();
    for (name, layer) in [(paste, FpLayer::Paste), (mask, FpLayer::Mask)] {
        if layers.iter().any(|l| l == name) {
            out.push(FpGraphic {
                layer,
                shape: shape.clone(),
                width: 0.0,
                filled: true,
            });
        }
    }
    out
}

fn pad_from(item: &Sx, ctx: &Ctx) -> Result<Option<(Pad, Option<String>)>, String> {
    let atoms = item.atoms();
    let number = atoms.first().copied().unwrap_or("").to_string();
    let ptype = atoms.get(1).copied().unwrap_or("smd");
    let pshape = atoms.get(2).copied().unwrap_or("rect");
    let at = item.child("at").ok_or("the pad has no position")?;
    let (x, y) = (
        at.arg_num(0).ok_or("the pad position is not valid")?,
        at.arg_num(1).ok_or("the pad position is not valid")?,
    );
    let abs_angle = at.arg_num(2).unwrap_or(0.0);
    let (w, h) = item.xy("size").ok_or("the pad has no size")?;
    if !(w > 0.0 && h > 0.0) {
        return Err("the pad size must be more than zero".into());
    }
    let layers: Vec<String> = item
        .child("layers")
        .map(|l| l.atoms().into_iter().map(str::to_string).collect())
        .unwrap_or_default();
    // `*.Cu` is a layer on the two sides. `*.Mask` is not a copper or paste layer.
    let on = |name: &str| {
        let kind = name.split('.').nth(1).unwrap_or("");
        layers
            .iter()
            .any(|l| l == name || (l.starts_with("*.") && l.ends_with(kind)))
    };
    let side_cu = if ctx.side == Side::Top {
        "F.Cu"
    } else {
        "B.Cu"
    };
    let side_paste = if ctx.side == Side::Top {
        "F.Paste"
    } else {
        "B.Paste"
    };
    let has_mask = layers.iter().any(|l| l.ends_with(".Mask"));
    let has_paste = on(side_paste) || layers.iter().any(|l| l == "*.Paste");

    let shape = match pshape {
        "circle" => PadShape::Circle { diameter: r6(w) },
        "rect" => PadShape::Rect {
            width: r6(w),
            height: r6(h),
        },
        "oval" => {
            if (w - h).abs() < 1e-9 {
                PadShape::Circle { diameter: r6(w) }
            } else {
                PadShape::Oval {
                    width: r6(w),
                    height: r6(h),
                }
            }
        }
        "roundrect" => {
            let ratio = item.child_num("roundrect_rratio").unwrap_or(0.25);
            let chamfer = item.child_num("chamfer_ratio").unwrap_or(0.0);
            let corners: Vec<&str> = item.child("chamfer").map(|c| c.atoms()).unwrap_or_default();
            if chamfer > 0.0 && !corners.is_empty() {
                PadShape::Polygon {
                    points: chamfered(w, h, ratio, chamfer, &corners, ctx),
                }
            } else {
                PadShape::RoundRect {
                    width: r6(w),
                    height: r6(h),
                    radius: r6((ratio * w.min(h)).clamp(0.0, w.min(h) / 2.0)),
                }
            }
        }
        "trapezoid" => {
            let (dx, dy) = item.xy("rect_delta").unwrap_or((0.0, 0.0));
            let (hx, hy, dx, dy) = (w / 2.0, h / 2.0, dx / 2.0, dy / 2.0);
            let k = [
                (-hx - dy, hy + dx),
                (-hx + dy, -hy - dx),
                (hx - dy, -hy + dx),
                (hx + dy, hy - dx),
            ];
            PadShape::Polygon {
                points: k.iter().map(|(x, y)| ctx.lp(*x, *y)).collect(),
            }
        }
        "custom" => {
            let polys: Vec<Vec<Point>> = item
                .child("primitives")
                .map(|p| {
                    p.children("gr_poly")
                        .filter_map(|g| g.child("pts"))
                        .map(|pts| points_from(pts, &|x, y| ctx.lp(x, y)))
                        .filter(|v| v.len() >= 3)
                        .collect()
                })
                .unwrap_or_default();
            match polys
                .into_iter()
                .max_by(|a, b| ring_area(a).abs().total_cmp(&ring_area(b).abs()))
            {
                Some(points) => PadShape::Polygon { points },
                None => PadShape::Rect {
                    width: r6(w),
                    height: r6(h),
                },
            }
        }
        other => return Err(format!("the pad shape `{other}` is not known")),
    };

    let kind = match ptype {
        "smd" | "connect" => {
            if !on(side_cu) {
                return Ok(None);
            }
            PadKind::Smd
        }
        "thru_hole" | "np_thru_hole" => {
            let d = item.child("drill").ok_or("the through pad has no drill")?;
            let drill = if d.arg(0) == Some("oval") {
                let dw = d.arg_num(1).ok_or("the oval drill is not valid")?;
                let dh = d.arg_num(2).unwrap_or(dw);
                Drill::Slot {
                    width: r6(dw),
                    height: r6(dh),
                }
            } else {
                Drill::Round {
                    diameter: r6(d.arg_num(0).ok_or("the drill is not valid")?),
                }
            };
            PadKind::Through {
                drill,
                plated: ptype == "thru_hole",
            }
        }
        other => return Err(format!("the pad type `{other}` is not known")),
    };
    let mut pad = Pad::smd(number, ctx.lp(x, y), shape);
    pad.name = item
        .child("pinfunction")
        .and_then(|p| p.arg(0))
        .map(str::to_string)
        .filter(|n| !n.is_empty() && n != &pad.number);
    pad.kind = kind;
    pad.rotation = ctx.angle(abs_angle);
    pad.paste = matches!(pad.kind, PadKind::Smd) && has_paste && ptype != "connect";
    pad.mask = has_mask;
    pad.mask_margin = item.child_num("solder_mask_margin").map(r6);
    pad.paste_margin = item.child_num("solder_paste_margin").map(r6);
    if item
        .child("drill")
        .and_then(|d| d.child("offset"))
        .is_some()
    {
        return Err("a drill offset is not supported".into());
    }
    // `(net 3 "GND")` up to KiCad 9, `(net "GND")` from KiCad 10.
    let net = item.child("net").and_then(|n| {
        n.arg(1)
            .or_else(|| n.arg(0).filter(|a| a.parse::<f64>().is_err()))
            .map(str::to_string)
    });
    Ok(Some((pad, net)))
}

/// Rounded rectangle with some corners chamfered, in pad-local coordinates.
fn chamfered(w: f64, h: f64, rratio: f64, cratio: f64, corners: &[&str], ctx: &Ctx) -> Vec<Point> {
    let c = cratio * w.min(h);
    let r = rratio * w.min(h);
    let (hx, hy) = (w / 2.0, h / 2.0);
    // KiCad frame: Y down, so "top" is -y.
    let specs = [
        ("top_left", -1.0, -1.0),
        ("top_right", 1.0, -1.0),
        ("bottom_right", 1.0, 1.0),
        ("bottom_left", -1.0, 1.0),
    ];
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for (name, sx, sy) in specs {
        let corner = (sx * hx, sy * hy);
        if corners.contains(&name) {
            // Two points, cut parallel to the diagonal.
            let a = (corner.0 - sx * c, corner.1);
            let b = (corner.0, corner.1 - sy * c);
            // Put the points in order so that the outline goes around. The
            // order changes with the quadrant.
            if (sx < 0.0) == (sy < 0.0) {
                pts.push(a);
                pts.push(b);
            } else {
                pts.push(b);
                pts.push(a);
            }
        } else if r > 0.0 {
            let cc = (corner.0 - sx * r, corner.1 - sy * r);
            let base = match name {
                "top_left" => 180.0,
                "top_right" => 270.0,
                "bottom_right" => 0.0,
                _ => 90.0,
            };
            for k in 0..=6 {
                let a = (base + 90.0 * k as f64 / 6.0_f64).to_radians();
                pts.push((cc.0 + r * a.cos(), cc.1 + r * a.sin()));
            }
        } else {
            pts.push(corner);
        }
    }
    let mut out: Vec<Point> = pts.into_iter().map(|(x, y)| ctx.lp(x, y)).collect();
    if ring_area(&out) < 0.0 {
        out.reverse();
    }
    out
}

/// Signed area of a ring (positive when counter-clockwise).
pub(crate) fn ring_area(r: &[Point]) -> f64 {
    let n = r.len();
    (0..n)
        .map(|i| {
            let (p, q) = (r[i], r[(i + 1) % n]);
            p.x * q.y - q.x * p.y
        })
        .sum::<f64>()
        / 2.0
}

fn model_from(item: &Sx) -> Option<KicadModel> {
    let path = item.arg(0)?.to_string();
    let xyz = |name: &str| -> Option<[f64; 3]> {
        let v = item.child(name)?.child("xyz")?;
        Some([v.arg_num(0)?, v.arg_num(1)?, v.arg_num(2)?])
    };
    let offset = xyz("offset")
        .or_else(|| xyz("at").map(|a| [a[0] * 25.4, a[1] * 25.4, a[2] * 25.4]))
        .unwrap_or([0.0; 3]);
    let rot = xyz("rotate").unwrap_or([0.0; 3]);
    let scale = xyz("scale").unwrap_or([1.0; 3]);
    Some(KicadModel {
        path,
        offset: offset.map(r6),
        rotation: [r6(-rot[0]), r6(-rot[1]), r6(-rot[2])],
        scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(lib: &str, name: &str) -> Instance {
        let p = format!(
            "{}/tests/fixtures/footprints/{lib}.pretty/{name}.kicad_mod",
            env!("CARGO_MANIFEST_DIR")
        );
        let src = std::fs::read_to_string(&p).unwrap();
        parse_footprint(&src, lib, &p).unwrap()
    }

    #[test]
    fn resistor_0603() {
        let i = fixture("Resistor_SMD", "R_0603_1608Metric");
        let fp = &i.footprint;
        assert_eq!(fp.id, "R_0603_1608Metric");
        assert_eq!(fp.pads.len(), 2);
        let p1 = fp.pad("1").unwrap();
        assert_eq!(p1.at, Point::new(-0.825, 0.0));
        assert_eq!(
            p1.shape,
            PadShape::RoundRect {
                width: 0.8,
                height: 0.95,
                radius: 0.2
            }
        );
        assert!(p1.paste && p1.mask);
        assert!(fp.graphics.iter().any(|g| g.layer == FpLayer::Courtyard));
        // The silk lines are above and below the body. KiCad -0.5225 becomes +0.5225.
        assert!(
            fp.graphics
                .iter()
                .any(|g| matches!(g.shape, Shape::Line { a, .. } if (a.y - 0.5225).abs() < 1e-9))
        );
        assert!(
            fp.texts
                .iter()
                .any(|t| t.text == "${ref}" && t.layer == FpLayer::Silk && t.at.y > 0.0)
        );
        let m = &i.models[0];
        assert!(m.path.ends_with("R_0603_1608Metric.step"));
        assert!(i.notes.is_empty(), "{:?}", i.notes);
    }

    #[test]
    fn through_hole_header_and_slots() {
        let i = fixture(
            "Connector_PinHeader_2.54mm",
            "PinHeader_1x03_P2.54mm_Vertical",
        );
        let fp = &i.footprint;
        assert_eq!(fp.pads.len(), 3);
        let p1 = fp.pad("1").unwrap();
        assert!(matches!(p1.kind, PadKind::Through { plated: true, .. }));
        assert!(matches!(p1.shape, PadShape::Rect { .. }));
        let p3 = fp.pad("3").unwrap();
        assert_eq!(p3.at, Point::new(0.0, -5.08));
        assert!(!p3.paste && p3.mask);

        let usb = fixture("Connector_USB", "USB_C_Receptacle_HRO_TYPE-C-31-M-12");
        let slots: Vec<&Pad> = usb
            .footprint
            .pads
            .iter()
            .filter(|p| {
                matches!(
                    p.kind,
                    PadKind::Through {
                        drill: Drill::Slot { .. },
                        ..
                    }
                )
            })
            .collect();
        assert!(!slots.is_empty(), "USB-C has oval mounting slots");
        let np: Vec<&Pad> = usb
            .footprint
            .pads
            .iter()
            .filter(|p| matches!(p.kind, PadKind::Through { plated: false, .. }))
            .collect();
        assert_eq!(np.len(), 2, "two unplated alignment holes");
        assert!(
            usb.footprint
                .pads
                .iter()
                .any(|p| p.number == "A1B12" || p.number == "A1")
        );
    }

    #[test]
    fn qfn_exposed_pad_and_module() {
        let q = fixture("Package_DFN_QFN", "QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm");
        assert!(q.footprint.pads.len() >= 33, "{}", q.footprint.pads.len());
        let ep = q.footprint.pad("33").unwrap();
        assert!(
            matches!(ep.shape, PadShape::Rect { width, .. } | PadShape::RoundRect { width, .. } if (width - 3.45).abs() < 1e-6)
        );
        let e = fixture("RF_Module", "ESP32-S3-WROOM-1");
        assert!(e.footprint.pads.len() >= 40);
        assert!(e.footprint.graphics.iter().any(|g| g.layer == FpLayer::Fab));
    }

    #[test]
    fn board_instance_front_and_back() {
        // A rotated footprint on the front. In a board, the pad angles are absolute.
        let front = r#"(footprint "Lib:R" (layer "F.Cu") (at 101 114.5 90)
            (property "Reference" "R6" (at 0 0 90) (layer "F.SilkS") (effects (font (size 1 1))))
            (property "Value" "100k" (at 0 1.43 90) (layer "F.Fab") (hide yes) (effects (font (size 1 1))))
            (pad "1" smd rect (at -0.825 0 90) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste") (net 3 "GND"))
            (pad "2" smd rect (at 0.825 0 90) (size 0.8 0.95) (layers "F.Cu" "F.Mask" "F.Paste") (net "FIRE1")))"#;
        let i = convert(&sexpr::parse(front).unwrap(), "", "test").unwrap();
        assert_eq!(i.reference, "R6");
        assert_eq!(i.value.as_deref(), Some("100k"));
        assert_eq!(i.at, Point::new(101.0, -114.5));
        assert_eq!(i.rotation, 90.0);
        assert_eq!(i.side, Side::Top);
        let p1 = i.footprint.pad("1").unwrap();
        assert_eq!(p1.rotation, 0.0);
        assert_eq!(
            i.pad_nets,
            vec![
                ("1".to_string(), "GND".to_string()),
                ("2".to_string(), "FIRE1".to_string())
            ]
        );
        // The value is hidden. Only the reference text stays.
        assert_eq!(i.footprint.texts.len(), 1);
        let pl = Placement {
            x: i.at.x,
            y: i.at.y,
            rotation: i.rotation,
            side: i.side,
            locked: false,
        };
        // Pad 1 is at local (-0.825, 0). After a 90° CCW rotation, it is below
        // the centre (Y up).
        let b = pl.transform(p1.at);
        assert!(
            (b.x - 101.0).abs() < 1e-9 && (b.y - (-114.5 - 0.825)).abs() < 1e-9,
            "{b:?}"
        );

        // The same part, flipped to the back. Compared with Lamina, KiCad keeps
        // the local Y negated and the orientation + 180.
        let back = r#"(footprint "Lib:R" (layer "B.Cu") (at 101 114.5 270)
            (property "Reference" "R6" (at 0 0 270) (layer "B.SilkS") (effects (font (size 1 1))))
            (pad "1" smd rect (at -0.825 0.3 270) (size 0.8 0.95) (layers "B.Cu" "B.Mask" "B.Paste"))
            (pad "2" smd rect (at 0.825 0.3 270) (size 0.8 0.95) (layers "B.Cu" "B.Mask" "B.Paste")))"#;
        let i = convert(&sexpr::parse(back).unwrap(), "", "test").unwrap();
        assert_eq!(i.side, Side::Bottom);
        assert_eq!(i.rotation, 90.0);
        let p1 = i.footprint.pad("1").unwrap();
        // In library-local coordinates (Y up), pad 1 is at (-0.825, -0.3).
        // KiCad flipped the stored y.
        assert_eq!(p1.at, Point::new(-0.825, 0.3));
        let pl = Placement {
            x: i.at.x,
            y: i.at.y,
            rotation: i.rotation,
            side: i.side,
            locked: false,
        };
        // KiCad: abs = pos + Rk(270)·(-0.825, 0.3) in Y-down coordinates.
        // Calculate it directly:
        // Rk(o) = [[cos, sin], [-sin, cos]] on Y-down coordinates.
        let (c, s) = (270.0_f64.to_radians().cos(), 270.0_f64.to_radians().sin());
        let kx = 101.0 + c * -0.825 + s * 0.3;
        let ky = 114.5 + -s * -0.825 + c * 0.3;
        let ours = pl.transform(p1.at);
        assert!(
            (ours.x - kx).abs() < 1e-9 && (ours.y + ky).abs() < 1e-9,
            "ours {ours:?} kicad ({kx}, {ky})"
        );
    }

    #[test]
    fn arcs_and_polygons() {
        let src = r#"(footprint "A" (layer "F.Cu")
            (fp_arc (start 1 0) (mid 0 -1) (end -1 0) (stroke (width 0.1) (type solid)) (layer "F.SilkS"))
            (fp_poly (pts (xy 0 0) (xy 1 0) (xy 1 1)) (stroke (width 0)) (fill yes) (layer "F.Fab"))
            (pad "1" smd trapezoid (at 0 0) (size 2 1) (rect_delta 0 0.5) (layers "F.Cu"))
            (pad "2" smd roundrect (at 3 0) (size 1 1) (layers "F.Cu") (roundrect_rratio 0.2) (chamfer_ratio 0.3) (chamfer top_left))
            (pad "3" smd custom (at 6 0) (size 1 1) (layers "F.Cu") (options (clearance outline) (anchor rect))
               (primitives (gr_poly (pts (xy -1 -1) (xy 1 -1) (xy 1 1) (xy -1 1)) (width 0) (fill yes)))))"#;
        let i = convert(&sexpr::parse(src).unwrap(), "", "t").unwrap();
        let fp = &i.footprint;
        // The KiCad arc from (1,0) through (0,-1) to (-1,0) is the upper half
        // after the Y flip: CCW 180°.
        match fp.graphics[0].shape {
            Shape::Arc {
                center,
                start,
                angle,
            } => {
                assert!(center.dist(Point::new(0.0, 0.0)) < 1e-6);
                assert_eq!(start, Point::new(1.0, 0.0));
                assert!((angle - 180.0).abs() < 1e-6, "{angle}");
            }
            ref other => panic!("{other:?}"),
        }
        assert!(fp.graphics[1].filled);
        assert!(
            matches!(fp.pad("1").unwrap().shape, PadShape::Polygon { ref points } if points.len() == 4)
        );
        assert!(
            matches!(fp.pad("2").unwrap().shape, PadShape::Polygon { ref points } if points.len() > 8)
        );
        assert!(
            matches!(fp.pad("3").unwrap().shape, PadShape::Polygon { ref points } if points.len() == 4)
        );
    }
}

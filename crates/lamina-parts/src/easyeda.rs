//! Converts EasyEDA "standard" footprints to Lamina footprints.
//!
//! Units are 10 mil (0.254 mm), Y points down, and coordinates are relative
//! to `dataStr.head.x/y`.

use lamina_model::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const U: f64 = 0.254;

fn r6(v: f64) -> f64 {
    lamina_model::json::round6(v)
}

/// Part data read from an EasyEDA component record.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PartMeta {
    pub lcsc: String,
    pub mpn: Option<String>,
    pub manufacturer: Option<String>,
    pub package: Option<String>,
    pub description: Option<String>,
    /// True for a JLCPCB basic part (no extended-part fee).
    pub basic: Option<bool>,
    pub datasheet: Option<String>,
    pub stock: Option<u64>,
    pub price: Option<f64>,
}

/// The position of the EasyEDA 3D model, before Lamina reads the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub uuid: String,
    pub title: String,
    /// The EasyEDA rotation in degrees (x, y, z), as stored.
    pub rotation: [f64; 3],
    /// The EasyEDA z offset in mm.
    pub z: f64,
    /// The centre of the top-view outline of the model, in footprint coordinates.
    pub outline_center: Option<Point>,
}

#[derive(Debug, Clone)]
pub struct Imported {
    pub footprint: Footprint,
    pub model: Option<ModelInfo>,
    pub meta: PartMeta,
    /// The items that the conversion skipped or made approximate.
    pub notes: Vec<String>,
}

pub(crate) struct Ctx {
    pub(crate) ox: f64,
    pub(crate) oy: f64,
}

impl Ctx {
    pub(crate) fn pt(&self, x: f64, y: f64) -> Point {
        Point::new(r6((x - self.ox) * U), r6(-(y - self.oy) * U))
    }
}

pub(crate) fn f(fields: &[&str], i: usize) -> f64 {
    fields
        .get(i)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0.0)
}

pub(crate) fn s<'a>(fields: &[&'a str], i: usize) -> &'a str {
    fields.get(i).copied().unwrap_or("")
}

pub(crate) fn nums(text: &str) -> Vec<f64> {
    text.split([' ', ','])
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect()
}

pub(crate) fn points(ctx: &Ctx, text: &str) -> Vec<Point> {
    nums(text)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| ctx.pt(c[0], c[1]))
        .collect()
}

/// Parse an SVG path of M, L, Z and A commands into points. An arc becomes
/// only its end point. This is enough for outlines.
pub(crate) fn path_points(ctx: &Ctx, path: &str) -> Vec<Point> {
    let mut out = Vec::new();
    let mut tok = String::new();
    let mut tokens: Vec<String> = Vec::new();
    for c in path.chars() {
        if c.is_ascii_alphabetic() {
            if !tok.trim().is_empty() {
                tokens.push(tok.clone());
            }
            tok.clear();
            tokens.push(c.to_string());
        } else if c == ',' || c == ' ' {
            if !tok.trim().is_empty() {
                tokens.push(tok.clone());
            }
            tok.clear();
        } else {
            tok.push(c);
        }
    }
    if !tok.trim().is_empty() {
        tokens.push(tok);
    }
    let mut i = 0;
    let mut cmd = 'M';
    let num = |t: &String| t.parse::<f64>().ok();
    while i < tokens.len() {
        let t = &tokens[i];
        if t.len() == 1 && t.chars().next().unwrap().is_ascii_alphabetic() {
            cmd = t.chars().next().unwrap().to_ascii_uppercase();
            i += 1;
            continue;
        }
        match cmd {
            'M' | 'L' => {
                if let (Some(x), Some(y)) = (num(t), tokens.get(i + 1).and_then(num)) {
                    out.push(ctx.pt(x, y));
                }
                i += 2;
            }
            'A' => {
                // rx ry rot large sweep x y
                if let (Some(x), Some(y)) = (
                    tokens.get(i + 5).and_then(num),
                    tokens.get(i + 6).and_then(num),
                ) {
                    out.push(ctx.pt(x, y));
                }
                i += 7;
            }
            _ => i += 1,
        }
    }
    out
}

/// Convert an SVG arc `M x0 y0 A r r 0 large sweep x1 y1` to a Lamina arc.
pub(crate) fn svg_arc(ctx: &Ctx, path: &str) -> Option<Shape> {
    let cleaned = path.replace(['M', 'A', 'm', 'a'], " ");
    let v = nums(&cleaned);
    if v.len() < 9 {
        return None;
    }
    let p0 = ctx.pt(v[0], v[1]);
    let r = v[2] * U;
    let large = v[5] != 0.0;
    let sweep = v[6] != 0.0;
    let p1 = ctx.pt(v[7], v[8]);
    // Y is flipped, so a positive SVG sweep becomes clockwise.
    let ccw = !sweep;
    let m = Point::new((p0.x + p1.x) / 2.0, (p0.y + p1.y) / 2.0);
    let dx = p1.x - p0.x;
    let dy = p1.y - p0.y;
    let half = (dx * dx + dy * dy).sqrt() / 2.0;
    if half < 1e-9 {
        return None;
    }
    let r = r.max(half);
    let h = (r * r - half * half).max(0.0).sqrt();
    let n = Point::new(-dy / (2.0 * half), dx / (2.0 * half));
    let sign = if ccw != large { 1.0 } else { -1.0 };
    let c = Point::new(m.x + n.x * h * sign, m.y + n.y * h * sign);
    let sweep_rad = lamina_model::arc_sweep(p0, p1, c, ccw, false);
    Some(Shape::Arc {
        center: Point::new(r6(c.x), r6(c.y)),
        start: p0,
        angle: r6(sweep_rad.to_degrees()),
    })
}

pub(crate) fn graphic_layer(layer: &str) -> Option<FpLayer> {
    match layer {
        "3" => Some(FpLayer::Silk),
        "12" | "13" | "15" | "99" | "100" | "101" => Some(FpLayer::Fab),
        _ => None,
    }
}

fn str_field(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    match cur {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Make a footprint id from the EasyEDA package title.
pub fn footprint_id(title: &str) -> String {
    let mut id: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    while id.starts_with('.') || id.starts_with('_') {
        id.remove(0);
    }
    id.truncate(100);
    if id.is_empty() {
        "lcsc_part".into()
    } else {
        id
    }
}

/// Convert an EasyEDA component API response (`{"result": {...}}` or the
/// result itself).
pub fn convert(resp: &Value, lcsc: &str, fetched: &str) -> Result<Imported, String> {
    let r = resp.get("result").unwrap_or(resp);
    let pkg = r
        .get("packageDetail")
        .ok_or("the component has no packageDetail (no footprint)")?;
    let ds = pkg.get("dataStr").ok_or("packageDetail has no dataStr")?;
    let head = ds.get("head").ok_or("dataStr has no head")?;
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
    let title = pkg
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or(lcsc)
        .to_string();
    let mut fp = Footprint::new(
        footprint_id(&title),
        Provenance::Lcsc {
            part: lcsc.to_string(),
            fetched: fetched.to_string(),
        },
    );
    fp.description = Some(title.clone());
    let mut notes = Vec::new();
    let mut model = None;
    let pin_names = symbol_pin_names(r);
    let mut skipped: std::collections::BTreeMap<String, usize> = Default::default();
    let mut body_pts: Vec<Point> = Vec::new();

    let shapes = ds
        .get("shape")
        .and_then(|s| s.as_array())
        .ok_or("dataStr has no shape list")?;
    for shape in shapes {
        let Some(text) = shape.as_str() else { continue };
        let fields: Vec<&str> = text.split('~').collect();
        match fields[0] {
            "PAD" => match pad(&ctx, &fields) {
                Ok(Some(p)) => fp.pads.push(p),
                Ok(None) => *skipped.entry("bottom-side PAD".into()).or_default() += 1,
                Err(e) => notes.push(format!("skipped a pad: {e}")),
            },
            "HOLE" => {
                let d = r6(f(&fields, 3) * 2.0 * U);
                if d > 0.0 {
                    let mut p = Pad::through(
                        "",
                        ctx.pt(f(&fields, 1), f(&fields, 2)),
                        PadShape::Circle { diameter: d },
                        Drill::Round { diameter: d },
                    );
                    p.kind = PadKind::Through {
                        drill: Drill::Round { diameter: d },
                        plated: false,
                    };
                    fp.pads.push(p);
                }
            }
            "TRACK" => {
                let Some(layer) = graphic_layer(s(&fields, 2)) else {
                    continue;
                };
                let w = r6(f(&fields, 1) * U);
                let pts = points(&ctx, s(&fields, 4));
                if s(&fields, 2) == "99" {
                    body_pts.extend(pts.iter().copied());
                }
                for seg in pts.windows(2) {
                    if seg[0].dist(seg[1]) > 1e-6 {
                        fp.graphics.push(FpGraphic {
                            layer,
                            shape: Shape::Line {
                                a: seg[0],
                                b: seg[1],
                            },
                            width: w,
                            filled: false,
                        });
                    }
                }
            }
            "CIRCLE" => {
                let Some(layer) = graphic_layer(s(&fields, 5)) else {
                    continue;
                };
                let w = r6(f(&fields, 4) * U);
                let radius = r6(f(&fields, 3) * U);
                if radius <= 0.0 {
                    continue;
                }
                // Small circles on the marking layer are polarity dots.
                let filled = radius < 0.2 && w >= radius;
                fp.graphics.push(FpGraphic {
                    layer,
                    shape: Shape::Circle {
                        center: ctx.pt(f(&fields, 1), f(&fields, 2)),
                        radius,
                    },
                    width: if filled { 0.0 } else { w },
                    filled,
                });
            }
            "ARC" => {
                let Some(layer) = graphic_layer(s(&fields, 2)) else {
                    continue;
                };
                if let Some(shape) = svg_arc(&ctx, s(&fields, 4)) {
                    fp.graphics.push(FpGraphic {
                        layer,
                        shape,
                        width: r6(f(&fields, 1) * U),
                        filled: false,
                    });
                }
            }
            "SOLIDREGION" => {
                let layer_id = s(&fields, 1);
                let kind = s(&fields, 4);
                match (graphic_layer(layer_id), kind) {
                    (Some(layer), "solid") => {
                        let pts = path_points(&ctx, s(&fields, 3));
                        if layer_id == "99" {
                            body_pts.extend(pts.iter().copied());
                        }
                        if pts.len() >= 3 && layer_id != "99" && layer_id != "100" {
                            fp.graphics.push(FpGraphic {
                                layer,
                                shape: Shape::Polygon { points: pts },
                                width: 0.0,
                                filled: true,
                            });
                        }
                    }
                    (_, "npth") => *skipped.entry("NPTH region".into()).or_default() += 1,
                    _ => {}
                }
            }
            "VIA" => *skipped.entry("footprint VIA".into()).or_default() += 1,
            "SVGNODE" => {
                if let Some(m) = model_info(&ctx, fields.get(1).copied().unwrap_or("")) {
                    model = Some(m);
                }
            }
            "TEXT" | "SOLIDREGION_CUTOUT" => {}
            other => *skipped.entry(other.to_string()).or_default() += 1,
        }
    }
    for (k, n) in skipped {
        notes.push(format!("skipped {n} {k} record(s)"));
    }
    if fp.pads.is_empty() {
        return Err(format!("{lcsc}: the footprint has no pads"));
    }

    // Make the courtyard from the pads and the body, 0.25 mm larger, on a 0.05 mm grid.
    let mut bb = fp.pads_bbox().unwrap();
    if let Some(b) = Rect::from_points(body_pts.iter().copied()) {
        bb = bb.union(&b);
    }
    let snap = |v: f64, up: bool| {
        let s = v / 0.05;
        (if up {
            (s - 1e-9).ceil()
        } else {
            (s + 1e-9).floor()
        }) * 0.05
    };
    let cy = Rect {
        min: Point::new(
            r6(snap(bb.min.x - 0.25, false)),
            r6(snap(bb.min.y - 0.25, false)),
        ),
        max: Point::new(
            r6(snap(bb.max.x + 0.25, true)),
            r6(snap(bb.max.y + 0.25, true)),
        ),
    };
    fp.graphics.push(FpGraphic {
        layer: FpLayer::Courtyard,
        shape: Shape::Rect {
            min: cy.min,
            max: cy.max,
        },
        width: 0.05,
        filled: false,
    });
    fp.texts.push(FpText {
        layer: FpLayer::Silk,
        text: "${ref}".into(),
        at: Point::new(0.0, r6(cy.max.y + 0.6)),
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

    let c_para = head.get("c_para").cloned().unwrap_or(Value::Null);
    let rpara = r
        .get("dataStr")
        .and_then(|d| d.get("head"))
        .and_then(|h| h.get("c_para"))
        .cloned()
        .unwrap_or(Value::Null);
    let class = str_field(&rpara, &["JLCPCB Part Class"]);
    let meta = PartMeta {
        lcsc: lcsc.to_string(),
        mpn: str_field(&rpara, &["Manufacturer Part"])
            .or_else(|| str_field(&rpara, &["name"]))
            .or_else(|| str_field(r, &["title"])),
        manufacturer: str_field(&rpara, &["Manufacturer"]),
        package: str_field(&c_para, &["package"]).or(Some(title)),
        description: str_field(r, &["description"]),
        basic: class.map(|c| c.to_ascii_lowercase().contains("basic")),
        datasheet: str_field(r, &["lcsc", "url"]),
        stock: r
            .get("lcsc")
            .and_then(|l| l.get("stock"))
            .and_then(|v| v.as_u64()),
        price: r
            .get("lcsc")
            .and_then(|l| l.get("price"))
            .and_then(|v| v.as_f64()),
    };
    if !pin_names.is_empty() {
        for p in &mut fp.pads {
            if let Some(n) = pin_names.get(&p.number) {
                if n != &p.number {
                    p.name = Some(n.clone());
                }
            }
        }
    }
    Ok(Imported {
        footprint: fp,
        model,
        meta,
        notes,
    })
}

/// A pad, its side (`None` for through pads) and its net.
pub(crate) struct RawPad {
    pub pad: Pad,
    pub side: Option<Side>,
    pub net: String,
}

fn pad(ctx: &Ctx, fl: &[&str]) -> Result<Option<Pad>, String> {
    let raw = pad_any(ctx, fl)?;
    Ok(match raw.side {
        Some(Side::Bottom) => None,
        _ => Some(raw.pad),
    })
}

pub(crate) fn pad_any(ctx: &Ctx, fl: &[&str]) -> Result<RawPad, String> {
    let shape = s(fl, 1);
    let at = ctx.pt(f(fl, 2), f(fl, 3));
    let w = r6(f(fl, 4) * U);
    let h = r6(f(fl, 5) * U);
    let layer = s(fl, 6);
    let number = s(fl, 8).trim().to_string();
    let hole_r = f(fl, 9);
    let rot = f(fl, 11);
    let hole_len = f(fl, 13);
    let plated = s(fl, 15) != "N";
    // EasyEDA rotates counter-clockwise on screen, like Lamina does.
    let mut rotation = r6(rot.rem_euclid(360.0));
    if rotation >= 359.999999 {
        rotation = 0.0;
    }
    let shape = match shape {
        "RECT" => PadShape::Rect {
            width: w,
            height: h,
        },
        "ELLIPSE" => {
            if (w - h).abs() < 1e-6 {
                PadShape::Circle { diameter: w }
            } else {
                PadShape::Oval {
                    width: w,
                    height: h,
                }
            }
        }
        "OVAL" => PadShape::Oval {
            width: w,
            height: h,
        },
        "POLYGON" => {
            let pts: Vec<Point> = points(ctx, s(fl, 10))
                .into_iter()
                .map(|p| Point::new(r6(p.x - at.x), r6(p.y - at.y)))
                .collect();
            if pts.len() < 3 {
                return Err(format!("the polygon pad {number} has too few points"));
            }
            rotation = 0.0;
            PadShape::Polygon { points: pts }
        }
        other => return Err(format!("unknown pad shape {other}")),
    };
    shape.check()?;
    let side = match layer {
        "1" => Some(Side::Top),
        "2" => Some(Side::Bottom),
        _ => None,
    };
    let kind = match layer {
        "1" | "2" => PadKind::Smd,
        "11" => {
            let d = r6(hole_r * 2.0 * U);
            if d <= 0.0 {
                return Err(format!("the through pad {number} has no hole"));
            }
            let drill = if hole_len > 0.0 {
                let l = r6(hole_len * U);
                let (pw, ph) = shape.size();
                if pw >= ph {
                    Drill::Slot {
                        width: l.max(d),
                        height: d,
                    }
                } else {
                    Drill::Slot {
                        width: d,
                        height: l.max(d),
                    }
                }
            } else {
                Drill::Round { diameter: d }
            };
            PadKind::Through { drill, plated }
        }
        other => {
            return Err(format!(
                "pad {number} is on layer {other}, which is not supported"
            ));
        }
    };
    let smd = matches!(kind, PadKind::Smd);
    Ok(RawPad {
        pad: Pad {
            number,
            name: None,
            at,
            rotation,
            shape,
            kind,
            mask_margin: None,
            paste_margin: None,
            paste: smd,
            mask: true,
        },
        side,
        net: s(fl, 7).trim().to_string(),
    })
}

pub(crate) fn model_info(ctx: &Ctx, json_text: &str) -> Option<ModelInfo> {
    let v: Value = serde_json::from_str(json_text).ok()?;
    let a = v.get("attrs")?;
    if a.get("c_etype").and_then(|x| x.as_str()) != Some("outline3D") {
        return None;
    }
    let uuid = a.get("uuid")?.as_str()?.to_string();
    let title = a
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let rot: Vec<f64> = a
        .get("c_rotation")
        .and_then(|t| t.as_str())
        .map(nums)
        .unwrap_or_default();
    let z = a
        .get("z")
        .and_then(|t| {
            t.as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or(t.as_f64())
        })
        .unwrap_or(0.0);
    // The outline polylines give the true top-view position of the model.
    let mut pts = Vec::new();
    if let Some(children) = v.get("childNodes").and_then(|c| c.as_array()) {
        for c in children {
            if let Some(p) = c
                .get("attrs")
                .and_then(|a| a.get("points"))
                .and_then(|p| p.as_str())
            {
                pts.extend(points(ctx, p));
            }
        }
    }
    let outline_center = Rect::from_points(pts)
        .map(|r| Point::new(r6((r.min.x + r.max.x) / 2.0), r6((r.min.y + r.max.y) / 2.0)));
    Some(ModelInfo {
        uuid,
        title,
        rotation: [
            rot.first().copied().unwrap_or(0.0),
            rot.get(1).copied().unwrap_or(0.0),
            rot.get(2).copied().unwrap_or(0.0),
        ],
        z: z * U,
        outline_center,
    })
}

/// Rotation matrix for degrees about X, then Y, then Z.
pub fn rotation_matrix(r: [f64; 3]) -> [[f64; 3]; 3] {
    let (sx, cx) = r[0].to_radians().sin_cos();
    let (sy, cy) = r[1].to_radians().sin_cos();
    let (sz, cz) = r[2].to_radians().sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    mul(rz, mul(ry, rx))
}

fn mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

pub fn apply(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// EasyEDA rotations are clockwise as seen on its Y-down canvas.
pub fn lamina_rotation(easyeda: [f64; 3]) -> [f64; 3] {
    let n = |v: f64| r6((-v).rem_euclid(360.0) % 360.0);
    [n(easyeda[0]), n(easyeda[1]), n(easyeda[2])]
}

/// Bounding box of OBJ vertices: (min, max).
pub fn obj_bounds(obj: &str) -> Option<([f64; 3], [f64; 3])> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for line in obj.lines() {
        if let Some(rest) = line.strip_prefix("v ") {
            let v: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            if v.len() >= 3 {
                any = true;
                for i in 0..3 {
                    lo[i] = lo[i].min(v[i]);
                    hi[i] = hi[i].max(v[i]);
                }
            }
        }
    }
    any.then_some((lo, hi))
}

/// Place an EasyEDA OBJ mesh. Rotate it, then move its top-view centre to
/// the outline centre. Move its base to the board surface plus the z offset.
pub fn model_ref(info: &ModelInfo, obj: &str, file: String) -> ModelRef {
    let rotation = lamina_rotation(info.rotation);
    let m = rotation_matrix(rotation);
    let (lo, hi) = obj_bounds(obj).unwrap_or(([0.0; 3], [0.0; 3]));
    // Get the rotated bounding box from the 8 corners.
    let mut rlo = [f64::INFINITY; 3];
    let mut rhi = [f64::NEG_INFINITY; 3];
    for i in 0..8 {
        let c = [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ];
        let p = apply(&m, c);
        for k in 0..3 {
            rlo[k] = rlo[k].min(p[k]);
            rhi[k] = rhi[k].max(p[k]);
        }
    }
    let center = info.outline_center.unwrap_or(Point::new(0.0, 0.0));
    let cx = (rlo[0] + rhi[0]) / 2.0;
    let cy = (rlo[1] + rhi[1]) / 2.0;
    // If a model is far below the board after rotation, lift it onto the board.
    let lift = if rlo[2] < -0.3 { -rlo[2] } else { 0.0 };
    ModelRef {
        file,
        offset: [r6(center.x - cx), r6(center.y - cy), r6(info.z + lift)],
        rotation,
        scale: [1.0; 3],
    }
}

/// The height of the placed model above the board.
pub fn model_height(info: &ModelInfo, obj: &str) -> Option<f64> {
    let m = rotation_matrix(lamina_rotation(info.rotation));
    let (lo, hi) = obj_bounds(obj)?;
    let mut top = f64::NEG_INFINITY;
    let mut bottom = f64::INFINITY;
    for i in 0..8 {
        let c = [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ];
        let z = apply(&m, c)[2];
        top = top.max(z);
        bottom = bottom.min(z);
    }
    let lift = if bottom < -0.3 { -bottom } else { 0.0 };
    Some(r6(top + lift + info.z))
}

/// The pin names of the symbol in a component record, by pin number. The
/// symbol is the `dataStr` at the top of the record; its `P~` shapes hold
/// the name in the fourth group and the number in the fifth.
fn symbol_pin_names(r: &Value) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let Some(shapes) = r
        .get("dataStr")
        .and_then(|d| d.get("shape"))
        .and_then(|s| s.as_array())
    else {
        return out;
    };
    for sh in shapes.iter().filter_map(|v| v.as_str()) {
        if !sh.starts_with("P~") {
            continue;
        }
        let groups: Vec<&str> = sh.split("^^").collect();
        let field = |g: usize| {
            groups
                .get(g)
                .and_then(|t| t.split('~').nth(4))
                .unwrap_or("")
        };
        let (name, number) = (field(3).trim(), field(4).trim());
        if !name.is_empty() && !number.is_empty() {
            out.insert(number.to_string(), name.to_string());
        }
    }
    out
}

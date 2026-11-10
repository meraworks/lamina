//! Draw lists shared by the PNG renderer and the live viewer.

pub mod theme;

pub use lamina_geom::font;

use std::path::{Path, PathBuf};

use lamina_drc::{Analysis, Ratsnest};
use lamina_geom::Prim;
use lamina_geom::fill::inflate;
use lamina_model::*;

/// One shape to draw, in board millimetres.
#[derive(Debug, Clone, PartialEq)]
pub enum Geom {
    /// Filled polygon with holes.
    Poly {
        outer: Vec<Point>,
        holes: Vec<Vec<Point>>,
    },
    /// Stroked polyline with round caps and joins.
    Stroke {
        pts: Vec<Point>,
        width: f64,
        closed: bool,
    },
    /// Filled disc.
    Disc { c: Point, r: f64 },
}

impl Geom {
    pub fn from_prim(p: &Prim) -> Geom {
        match p {
            Prim::Circle { c, r } => Geom::Disc { c: *c, r: *r },
            Prim::Capsule { a, b, r } => Geom::Stroke {
                pts: vec![*a, *b],
                width: 2.0 * r,
                closed: false,
            },
            Prim::Poly { outer, holes } => Geom::Poly {
                outer: outer.clone(),
                holes: holes.clone(),
            },
        }
    }

    pub fn bbox(&self) -> Option<Rect> {
        match self {
            Geom::Poly { outer, .. } => Rect::from_points(outer.iter().copied()),
            Geom::Stroke { pts, width, .. } => {
                Rect::from_points(pts.iter().copied()).map(|r| Rect {
                    min: Point::new(r.min.x - width / 2.0, r.min.y - width / 2.0),
                    max: Point::new(r.max.x + width / 2.0, r.max.y + width / 2.0),
                })
            }
            Geom::Disc { c, r } => Some(Rect {
                min: Point::new(c.x - r, c.y - r),
                max: Point::new(c.x + r, c.y + r),
            }),
        }
    }
}

/// The layers to draw, from back to front in the order of `Scene2D::layers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SceneLayer {
    Copper(u8),
    /// Mask openings.
    Mask(Side),
    Paste(Side),
    Silk(Side),
    Courtyard(Side),
    Fab(Side),
    Keepout,
    Outline,
    Drills,
    Ratsnest,
    Drc,
    DrcWarning,
}

impl SceneLayer {
    pub fn name(&self, copper_layers: u8) -> String {
        match self {
            SceneLayer::Copper(i) => {
                Layer::Copper(Copper::from_index(*i, copper_layers)).to_string()
            }
            SceneLayer::Mask(s) => format!("{}.mask", s.as_str()),
            SceneLayer::Paste(s) => format!("{}.paste", s.as_str()),
            SceneLayer::Silk(s) => format!("{}.silk", s.as_str()),
            SceneLayer::Courtyard(s) => format!("{}.courtyard", s.as_str()),
            SceneLayer::Fab(s) => format!("{}.fab", s.as_str()),
            SceneLayer::Keepout => "keepout".into(),
            SceneLayer::Outline => "outline".into(),
            SceneLayer::Drills => "drills".into(),
            SceneLayer::Ratsnest => "ratsnest".into(),
            SceneLayer::Drc => "drc".into(),
            SceneLayer::DrcWarning => "drc.warning".into(),
        }
    }

    pub fn side(&self, copper_layers: u8) -> Option<Side> {
        match self {
            SceneLayer::Copper(i) => Copper::from_index(*i, copper_layers).side(),
            SceneLayer::Mask(s)
            | SceneLayer::Paste(s)
            | SceneLayer::Silk(s)
            | SceneLayer::Courtyard(s)
            | SceneLayer::Fab(s) => Some(*s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayerDraw {
    pub layer: SceneLayer,
    pub items: Vec<Geom>,
    /// The first `pour` items are copper pour fills.
    pub pour: usize,
}

#[derive(Debug, Clone)]
pub struct Scene2D {
    pub layers: Vec<LayerDraw>,
    /// The extent of the board. If there is no outline, the extent of all items.
    pub bbox: Rect,
    pub copper_layers: u8,
    pub mask_color: String,
    pub silk_color: String,
    pub finish: String,
}

impl Scene2D {
    pub fn layer(&self, l: SceneLayer) -> Option<&LayerDraw> {
        self.layers.iter().find(|d| d.layer == l)
    }
}

fn push(layers: &mut Vec<LayerDraw>, l: SceneLayer, g: Geom) {
    match layers.iter_mut().find(|d| d.layer == l) {
        Some(d) => d.items.push(g),
        None => layers.push(LayerDraw {
            layer: l,
            items: vec![g],
            pour: 0,
        }),
    }
}

fn shape_geom(shape: &Shape, width: f64, filled: bool, tf: &dyn Fn(Point) -> Point) -> Geom {
    let w = width.max(0.02);
    match shape {
        Shape::Line { a, b } => Geom::Stroke {
            pts: vec![tf(*a), tf(*b)],
            width: w,
            closed: false,
        },
        Shape::Arc { .. } => Geom::Stroke {
            pts: shape.points().into_iter().map(tf).collect(),
            width: w,
            closed: false,
        },
        Shape::Circle { center, radius } => {
            if filled {
                Geom::Disc {
                    c: tf(*center),
                    r: *radius,
                }
            } else {
                Geom::Stroke {
                    pts: lamina_geom::prim::circle_points(*center, *radius, 0.01)
                        .into_iter()
                        .map(tf)
                        .collect(),
                    width: w,
                    closed: true,
                }
            }
        }
        Shape::Rect { min, max } => {
            let pts: Vec<Point> = vec![
                *min,
                Point::new(max.x, min.y),
                *max,
                Point::new(min.x, max.y),
            ]
            .into_iter()
            .map(tf)
            .collect();
            if filled {
                Geom::Poly {
                    outer: pts,
                    holes: vec![],
                }
            } else {
                Geom::Stroke {
                    pts,
                    width: w,
                    closed: true,
                }
            }
        }
        Shape::Polygon { points } => {
            let pts: Vec<Point> = points.iter().map(|p| tf(*p)).collect();
            if filled {
                Geom::Poly {
                    outer: pts,
                    holes: vec![],
                }
            } else {
                Geom::Stroke {
                    pts,
                    width: w,
                    closed: true,
                }
            }
        }
    }
}

fn text_geoms(
    text: &str,
    at: Point,
    size: f64,
    rotation: f64,
    width: f64,
    side: Side,
) -> Vec<Geom> {
    font::side_text_strokes(text, at, size, rotation, side)
        .into_iter()
        .map(|pts| Geom::Stroke {
            pts,
            width,
            closed: false,
        })
        .collect()
}

fn fp_side_layer(l: FpLayer, side: Side) -> Option<SceneLayer> {
    Some(match l {
        FpLayer::Silk => SceneLayer::Silk(side),
        FpLayer::Courtyard => SceneLayer::Courtyard(side),
        FpLayer::Fab => SceneLayer::Fab(side),
        // Paste and mask drawings are openings, as the pad openings are.
        FpLayer::Paste => SceneLayer::Paste(side),
        FpLayer::Mask => SceneLayer::Mask(side),
        FpLayer::Copper => return None,
    })
}

fn mp_geoms(m: &geo::MultiPolygon<f64>) -> Vec<Geom> {
    m.0.iter()
        .map(|p| {
            let conv = |ls: &geo::LineString<f64>| -> Vec<Point> {
                let mut v: Vec<Point> = ls.0.iter().map(|c| Point::new(c.x, c.y)).collect();
                if v.len() > 1 && v.first() == v.last() {
                    v.pop();
                }
                v
            };
            Geom::Poly {
                outer: conv(p.exterior()),
                holes: p.interiors().iter().map(conv).collect(),
            }
        })
        .collect()
}

/// Build the 2D scene. The analysis gives the fills, the ratsnest and the DRC markers.
pub fn build_2d(d: &ProjectData, a: &Analysis) -> Scene2D {
    let layers_n = d.board.stackup.copper_layers;
    let rules = &d.board.rules;
    let mut layers: Vec<LayerDraw> = Vec::new();
    let g = &a.geometry;

    // Copper: add the fills first, so that pads and tracks are drawn over them.
    for f in &a.fills {
        if let Some(c) = f.layer.copper() {
            for p in &f.polygons {
                push(
                    &mut layers,
                    SceneLayer::Copper(c.index(layers_n)),
                    Geom::Poly {
                        outer: p.outer.clone(),
                        holes: p.holes.clone(),
                    },
                );
            }
        }
    }
    // Everything on a copper layer so far is pour. The renderers draw the
    // first `pour` items lighter, so that pads and tracks stand out.
    for l in &mut layers {
        l.pour = l.items.len();
    }
    for it in &g.copper {
        push(
            &mut layers,
            SceneLayer::Copper(it.layer),
            Geom::from_prim(&it.prim),
        );
    }
    for dr in &g.drills {
        push(&mut layers, SceneLayer::Drills, Geom::from_prim(&dr.prim));
    }

    // Pads: mask and paste openings, footprint graphics and texts.
    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        let side = pl.side;
        for (pad, _, prim, _) in lamina_geom::placed_pads(fp, pl, layers_n) {
            let sides: Vec<Side> = if pad.is_smd() {
                vec![side]
            } else {
                vec![Side::Top, Side::Bottom]
            };
            if pad.mask {
                let m = pad.mask_margin.unwrap_or(rules.mask_margin);
                for s in &sides {
                    for gm in mp_geoms(&inflate(&prim, m)) {
                        push(&mut layers, SceneLayer::Mask(*s), gm);
                    }
                }
            }
            if pad.is_smd() && pad.paste {
                let m = pad.paste_margin.unwrap_or(rules.paste_margin);
                for gm in mp_geoms(&inflate(&prim, m)) {
                    push(&mut layers, SceneLayer::Paste(side), gm);
                }
            }
        }
        let tf = |p: Point| pl.transform(p);
        for gr in &fp.graphics {
            if let Some(l) = fp_side_layer(gr.layer, side) {
                push(
                    &mut layers,
                    l,
                    shape_geom(&gr.shape, gr.width, gr.filled, &tf),
                );
            }
        }
        for t in &fp.texts {
            let Some(l) = fp_side_layer(t.layer, side) else {
                continue;
            };
            let text = t
                .text
                .replace("${ref}", refdes)
                .replace("${value}", part.value.as_deref().unwrap_or(""));
            if text.trim().is_empty() {
                continue;
            }
            let rot = font::upright(t.rotation + pl.rotation);
            let at = pl.transform(t.at);
            let width = if l == SceneLayer::Silk(side) {
                font::silk_stroke_width(t.size, rules.silk_width)
            } else {
                (t.size * 0.12).max(0.05)
            };
            for gm in text_geoms(&text, at, t.size, rot, width, side) {
                push(&mut layers, l, gm);
            }
        }
        for (_, pts, _) in lamina_geom::part_keepouts(fp, pl, layers_n) {
            push(
                &mut layers,
                SceneLayer::Keepout,
                Geom::Poly {
                    outer: pts,
                    holes: vec![],
                },
            );
        }
    }
    for t in &d.routing.texts {
        let l = match t.layer {
            Layer::Silk(s) => SceneLayer::Silk(s),
            Layer::Fab(s) => SceneLayer::Fab(s),
            Layer::Copper(c) => SceneLayer::Copper(c.index(layers_n)),
            _ => continue,
        };
        for gm in text_geoms(
            &t.text,
            t.at,
            t.size,
            t.rotation,
            font::silk_stroke_width(t.size, rules.silk_width),
            t.layer.side().unwrap_or(Side::Top),
        ) {
            push(&mut layers, l, gm);
        }
    }
    for gr in &d.routing.graphics {
        let l = match gr.layer {
            Layer::Silk(s) => SceneLayer::Silk(s),
            Layer::Fab(s) => SceneLayer::Fab(s),
            Layer::Copper(c) => SceneLayer::Copper(c.index(layers_n)),
            _ => continue,
        };
        push(
            &mut layers,
            l,
            shape_geom(&gr.shape, gr.width, gr.filled, &|p| p),
        );
    }
    for k in &d.routing.keepouts {
        push(
            &mut layers,
            SceneLayer::Keepout,
            Geom::Poly {
                outer: k.outline.flatten(0.02),
                holes: vec![],
            },
        );
    }
    if let Some(o) = &g.outline {
        push(
            &mut layers,
            SceneLayer::Outline,
            Geom::Stroke {
                pts: o.clone(),
                width: 0.1,
                closed: true,
            },
        );
    }
    for c in &g.cutouts {
        push(
            &mut layers,
            SceneLayer::Outline,
            Geom::Stroke {
                pts: c.clone(),
                width: 0.1,
                closed: true,
            },
        );
    }
    for r in &a.ratsnest {
        push(&mut layers, SceneLayer::Ratsnest, ratsnest_geom(r));
    }
    for v in &a.violations {
        if let Some(at) = v.at {
            let layer = if v.severity == Severity::Error {
                SceneLayer::Drc
            } else {
                SceneLayer::DrcWarning
            };
            push(
                &mut layers,
                layer,
                Geom::Stroke {
                    pts: lamina_geom::prim::circle_points(at, 0.6, 0.02),
                    width: 0.12,
                    closed: true,
                },
            );
        }
    }
    layers.sort_by_key(|l| order_key(l.layer, layers_n));

    let bbox = g
        .outline
        .as_ref()
        .and_then(|o| Rect::from_points(o.iter().copied()))
        .or_else(|| {
            layers
                .iter()
                .flat_map(|l| l.items.iter())
                .filter_map(|g| g.bbox())
                .reduce(|a, b| a.union(&b))
        })
        .unwrap_or(Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(10.0, 10.0),
        });
    Scene2D {
        layers,
        bbox,
        copper_layers: layers_n,
        mask_color: d.board.stackup.mask_color.clone(),
        silk_color: d.board.stackup.silk_color.clone(),
        finish: d.board.stackup.finish.clone(),
    }
}

fn ratsnest_geom(r: &Ratsnest) -> Geom {
    Geom::Stroke {
        pts: vec![r.a, r.b],
        width: 0.08,
        closed: false,
    }
}

/// Back-to-front order: bottom side first, top side last, overlays on top.
pub fn order_key(l: SceneLayer, n: u8) -> (u8, u8) {
    match l {
        SceneLayer::Courtyard(Side::Bottom) => (0, 0),
        SceneLayer::Fab(Side::Bottom) => (0, 1),
        SceneLayer::Silk(Side::Bottom) => (0, 2),
        SceneLayer::Paste(Side::Bottom) => (0, 3),
        SceneLayer::Mask(Side::Bottom) => (0, 4),
        SceneLayer::Copper(i) => (1, n.saturating_sub(1).saturating_sub(i)),
        SceneLayer::Drills => (2, 0),
        SceneLayer::Mask(Side::Top) => (3, 0),
        SceneLayer::Paste(Side::Top) => (3, 1),
        SceneLayer::Silk(Side::Top) => (3, 2),
        SceneLayer::Fab(Side::Top) => (3, 3),
        SceneLayer::Courtyard(Side::Top) => (3, 4),
        SceneLayer::Keepout => (4, 0),
        SceneLayer::Outline => (4, 1),
        SceneLayer::Ratsnest => (4, 2),
        SceneLayer::DrcWarning => (4, 3),
        SceneLayer::Drc => (4, 4),
    }
}

/// A part model placed on the board.
#[derive(Debug, Clone)]
pub struct Instance {
    pub refdes: String,
    /// Model file, or `None` for a placeholder box.
    pub model: Option<PathBuf>,
    /// The 4x4 transform from model to board, column-major (mm).
    pub transform: [f32; 16],
    /// Placeholder box (courtyard extents and height) in footprint coordinates.
    pub placeholder: Option<([f32; 3], [f32; 3])>,
}

#[derive(Debug, Clone)]
pub struct Scene3D {
    /// Board outline and holes, extruded by `thickness`.
    pub outline: Vec<Point>,
    pub holes: Vec<Vec<Point>>,
    pub thickness: f64,
    pub instances: Vec<Instance>,
    pub bbox: Rect,
}

type M4 = [[f32; 4]; 4];

fn mat_mul(a: &M4, b: &M4) -> M4 {
    let mut m = [[0.0; 4]; 4];
    for (c, col) in m.iter_mut().enumerate() {
        for (r, v) in col.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    m
}

fn translate(x: f32, y: f32, z: f32) -> M4 {
    [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [x, y, z, 1.],
    ]
}

fn rot_x(deg: f32) -> M4 {
    let (s, c) = deg.to_radians().sin_cos();
    [
        [1., 0., 0., 0.],
        [0., c, s, 0.],
        [0., -s, c, 0.],
        [0., 0., 0., 1.],
    ]
}

fn rot_y(deg: f32) -> M4 {
    let (s, c) = deg.to_radians().sin_cos();
    [
        [c, 0., -s, 0.],
        [0., 1., 0., 0.],
        [s, 0., c, 0.],
        [0., 0., 0., 1.],
    ]
}

fn rot_z(deg: f32) -> M4 {
    let (s, c) = deg.to_radians().sin_cos();
    [
        [c, s, 0., 0.],
        [-s, c, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]
}

fn scale(x: f32, y: f32, z: f32) -> M4 {
    [
        [x, 0., 0., 0.],
        [0., y, 0., 0.],
        [0., 0., z, 0.],
        [0., 0., 0., 1.],
    ]
}

fn flat(m: &M4) -> [f32; 16] {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = m[c][r];
        }
    }
    o
}

/// The transform from footprint coordinates (Z up from the board surface) to
/// board coordinates for a placement.
pub fn placement_matrix(pl: &Placement, thickness: f64) -> M4 {
    let base = mat_mul(
        &translate(pl.x as f32, pl.y as f32, 0.0),
        &rot_z(pl.rotation as f32),
    );
    match pl.side {
        Side::Top => mat_mul(&base, &translate(0.0, 0.0, thickness as f32)),
        // Turn the part over (this mirrors X, as footprints do) and put it below the board.
        Side::Bottom => mat_mul(&base, &rot_y(180.0)),
    }
}

pub fn build_3d(d: &ProjectData, root: &Path) -> Scene3D {
    let t = d.board.stackup.thickness;
    let outline = d
        .board
        .outline
        .as_ref()
        .map(|c| c.flatten(0.02))
        .unwrap_or_default();
    let mut holes: Vec<Vec<Point>> = d.board.cutouts.iter().map(|c| c.flatten(0.02)).collect();
    let g = lamina_geom::build(d);
    for dr in &g.drills {
        holes.push(dr.prim.outline(0.01));
    }
    let mut instances = Vec::new();
    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        if part.dnp {
            continue;
        }
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        let place = placement_matrix(pl, t);
        let (model, m) = match &fp.model {
            Some(mr) => {
                let path = lamina_model::find_model(root, &mr.file)
                    .unwrap_or_else(|| root.join(MODEL_DIR).join(&mr.file));
                let local = mat_mul(
                    &translate(
                        mr.offset[0] as f32,
                        mr.offset[1] as f32,
                        mr.offset[2] as f32,
                    ),
                    &mat_mul(
                        &rot_z(mr.rotation[2] as f32),
                        &mat_mul(
                            &rot_y(mr.rotation[1] as f32),
                            &mat_mul(
                                &rot_x(mr.rotation[0] as f32),
                                &scale(mr.scale[0] as f32, mr.scale[1] as f32, mr.scale[2] as f32),
                            ),
                        ),
                    ),
                );
                (path.is_file().then_some(path), mat_mul(&place, &local))
            }
            None => (None, place),
        };
        let placeholder = fp.body_bbox().filter(|_| fp.has_body()).map(|r| {
            let h = fp.height.unwrap_or(1.0) as f32;
            let shrink = 0.25_f32;
            (
                [r.min.x as f32 + shrink, r.min.y as f32 + shrink, 0.0],
                [r.max.x as f32 - shrink, r.max.y as f32 - shrink, h],
            )
        });
        let transform = if model.is_some() {
            flat(&m)
        } else {
            flat(&place)
        };
        instances.push(Instance {
            refdes: refdes.clone(),
            model,
            transform,
            placeholder,
        });
    }
    let bbox = Rect::from_points(outline.iter().copied()).unwrap_or(Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(10.0, 10.0),
    });
    Scene3D {
        outline,
        holes,
        thickness: t,
        instances,
        bbox,
    }
}

/// Build the analysis and the two scenes. This is the usual entry point.
pub fn build(d: &ProjectData, root: &Path) -> (Analysis, Scene2D, Scene3D) {
    let a = lamina_drc::analyze(d);
    let s2 = build_2d(d, &a);
    let s3 = build_3d(d, root);
    (a, s2, s3)
}

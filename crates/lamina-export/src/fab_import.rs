//! Make a board from the fab outputs (Gerbers and drill files) of any CAD program.
//! The result is artwork: the outline, holes, copper and silkscreen as drawings,
//! with no parts or nets. Use it to look at a board and to check it.

use lamina_ports::format::{Loss, LossKind, lost};
use std::collections::BTreeMap;

use geo::{Area, MultiPolygon, Polygon};
use lamina_model::*;

use crate::gerber_in::{self, DrillHit};

/// What a fab file contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FabLayer {
    /// Copper, by position from the top (1 = top). `bottom` is set for the last layer.
    Copper {
        pos: u8,
        bottom: bool,
    },
    Silk(Side),
    Mask(Side),
    Paste(Side),
    Outline,
    Drill,
    Other,
}

fn r6(v: f64) -> f64 {
    json::round6(v)
}

/// Find the layer of a file from its X2 file function. If it has none, use its name.
pub fn classify(name: &str, text: &str) -> FabLayer {
    let head: String = text.chars().take(4000).collect();
    if head.contains("M48")
        || head
            .lines()
            .take(40)
            .any(|l| l.starts_with("T1C") || l.starts_with("T01C"))
    {
        return FabLayer::Drill;
    }
    if let Some(i) = head.find("%TF.FileFunction,") {
        let f: &str = head[i + 17..].split('*').next().unwrap_or("");
        let parts: Vec<&str> = f.split(',').collect();
        let side = |s: Option<&&str>| {
            if s == Some(&"Bot") {
                Side::Bottom
            } else {
                Side::Top
            }
        };
        match parts.first().copied() {
            Some("Copper") => {
                let pos = parts
                    .get(1)
                    .and_then(|l| l.strip_prefix('L'))
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1);
                return FabLayer::Copper {
                    pos,
                    bottom: parts.get(2) == Some(&"Bot"),
                };
            }
            Some("Legend") => return FabLayer::Silk(side(parts.get(1))),
            Some("Soldermask") => return FabLayer::Mask(side(parts.get(1))),
            Some("Paste") => return FabLayer::Paste(side(parts.get(1))),
            Some("Profile") => return FabLayer::Outline,
            Some(_) => return FabLayer::Other,
            None => {}
        }
    }
    let lower = name.to_ascii_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    let ext = base.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let has = |s: &str| base.contains(s);
    // Try KiCad names first, then Protel/Altium/EasyEDA extensions, then Eagle.
    if has("f_cu") || has("f.cu") {
        return FabLayer::Copper {
            pos: 1,
            bottom: false,
        };
    }
    if has("b_cu") || has("b.cu") {
        return FabLayer::Copper {
            pos: 99,
            bottom: true,
        };
    }
    if has("_cu") || has(".cu") {
        // `In2_Cu`: the inner layer number is between "in" and "_cu".
        for (i, _) in base.match_indices("in") {
            let n: String = base[i + 2..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            let rest = &base[i + 2 + n.len()..];
            if let (Ok(n), true) = (
                n.parse::<u8>(),
                rest.starts_with("_cu") || rest.starts_with(".cu"),
            ) {
                return FabLayer::Copper {
                    pos: n + 1,
                    bottom: false,
                };
            }
        }
    }
    if has("edge_cuts") || has("edge.cuts") || has("outline") || has("profile") {
        return FabLayer::Outline;
    }
    if has("f_silk") || has("f.silk") || has("topsilk") {
        return FabLayer::Silk(Side::Top);
    }
    if has("b_silk") || has("b.silk") || has("bottomsilk") {
        return FabLayer::Silk(Side::Bottom);
    }
    if has("f_mask") || has("f.mask") || has("topsolder") {
        return FabLayer::Mask(Side::Top);
    }
    if has("b_mask") || has("b.mask") || has("bottomsolder") {
        return FabLayer::Mask(Side::Bottom);
    }
    if has("f_paste") || has("f.paste") || has("toppaste") {
        return FabLayer::Paste(Side::Top);
    }
    if has("b_paste") || has("b.paste") || has("bottompaste") {
        return FabLayer::Paste(Side::Bottom);
    }
    match ext {
        "gtl" | "cmp" | "top" => FabLayer::Copper {
            pos: 1,
            bottom: false,
        },
        "gbl" | "sol" | "bot" => FabLayer::Copper {
            pos: 99,
            bottom: true,
        },
        "gto" | "plc" => FabLayer::Silk(Side::Top),
        "gbo" | "pls" => FabLayer::Silk(Side::Bottom),
        "gts" | "stc" => FabLayer::Mask(Side::Top),
        "gbs" | "sts" => FabLayer::Mask(Side::Bottom),
        "gtp" | "crc" => FabLayer::Paste(Side::Top),
        "gbp" | "crs" => FabLayer::Paste(Side::Bottom),
        "gko" | "gm1" | "gml" | "gm" | "dim" | "mil" | "gbr_outline" => FabLayer::Outline,
        "drl" | "xln" | "exc" | "drd" => FabLayer::Drill,
        // A file with a generic extension is a drill file only if its contents look like one.
        "txt" | "nc" | "tap"
            if head.contains("METRIC") || head.contains("INCH") || head.contains("T1") =>
        {
            FabLayer::Drill
        }
        e if e.starts_with('g') && e[1..].parse::<u8>().is_ok() => {
            // .G1, .G2 and more: inner layers (Protel).
            FabLayer::Copper {
                pos: e[1..].parse::<u8>().unwrap() + 1,
                bottom: false,
            }
        }
        e if e.starts_with("gp") && e[2..].parse::<u8>().is_ok() => FabLayer::Copper {
            pos: e[2..].parse::<u8>().unwrap() + 1,
            bottom: false,
        },
        _ => FabLayer::Other,
    }
}

/// Make one ring from a polygon with holes. A zero-width cut connects each
/// hole to the outside, as in Gerber regions.
pub fn keyhole(p: &Polygon<f64>) -> Vec<Point> {
    lamina_geom::fill::keyhole(p)
}

/// The board that `import` makes from a set of fab files.
#[derive(Debug)]
pub struct FabBoard {
    pub data: ProjectData,
    pub notes: Vec<String>,
    /// The data of the files that is not in the board, with its kind.
    pub losses: Vec<Loss>,
    /// The layer of each file, as `import` read it.
    pub layers: BTreeMap<String, String>,
}

fn layer_name(l: FabLayer) -> String {
    match l {
        FabLayer::Copper {
            pos: 99,
            bottom: true,
        } => "bottom copper".into(),
        FabLayer::Copper { pos, bottom } => {
            if bottom {
                format!("copper L{pos} (bottom)")
            } else if pos == 1 {
                "top copper".into()
            } else {
                format!("copper L{pos}")
            }
        }
        FabLayer::Silk(s) => format!("{} silkscreen", s.as_str()),
        FabLayer::Mask(s) => format!("{} solder mask", s.as_str()),
        FabLayer::Paste(s) => format!("{} paste", s.as_str()),
        FabLayer::Outline => "board outline".into(),
        FabLayer::Drill => "drill".into(),
        FabLayer::Other => "not used".into(),
    }
}

/// Build a board from fab files given as (name, contents).
pub fn import(files: &[(String, Vec<u8>)], name: &str) -> Result<FabBoard, String> {
    let mut notes = Vec::new();
    let mut losses: Vec<Loss> = Vec::new();
    let mut layers = BTreeMap::new();
    let mut copper: Vec<(u8, bool, String, MultiPolygon<f64>)> = Vec::new();
    let mut silk: Vec<(Side, MultiPolygon<f64>)> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut outline_image: Option<MultiPolygon<f64>> = None;
    let mut drills: Vec<DrillHit> = Vec::new();
    for (fname, bytes) in files {
        let text = String::from_utf8_lossy(bytes);
        let kind = classify(fname, &text);
        layers.insert(fname.clone(), layer_name(kind));
        match kind {
            FabLayer::Drill => {
                let d = gerber_in::read_drill(&text).map_err(|e| format!("{fname}: {e}"))?;
                let lower = fname.to_ascii_lowercase();
                let file_plated = d.plated.or(
                    if lower.contains("npth")
                        || lower.contains("non_plated")
                        || lower.contains("non-plated")
                    {
                        Some(false)
                    } else {
                        None
                    },
                );
                for mut h in d.hits {
                    h.plated = h.plated.or(file_plated);
                    drills.push(h);
                }
                notes.extend(d.notes.into_iter().map(|n| format!("{fname}: {n}")));
            }
            FabLayer::Other | FabLayer::Mask(_) | FabLayer::Paste(_) => {}
            _ => {
                let g = gerber_in::read(&text).map_err(|e| format!("{fname}: {e}"))?;
                notes.extend(g.notes.iter().map(|n| format!("{fname}: {n}")));
                match kind {
                    FabLayer::Copper { pos, bottom } => {
                        copper.push((pos, bottom, fname.clone(), g.image))
                    }
                    FabLayer::Silk(s) => silk.push((s, g.image)),
                    FabLayer::Outline => {
                        edges.extend(g.edges);
                        outline_image = Some(g.image);
                    }
                    _ => {}
                }
            }
        }
    }
    if copper.is_empty() && edges.is_empty() {
        return Err("There is no copper or outline Gerber in the files".into());
    }
    let skipped: Vec<&String> = layers
        .iter()
        .filter(|(_, l)| l.contains("mask") || l.contains("paste") || *l == "not used")
        .map(|(f, _)| f)
        .collect();
    if !skipped.is_empty() {
        lost(
            &mut notes,
            &mut losses,
            LossKind::Other,
            format!(
                "Not imported, because mask and paste come from pads and artwork has no pads: {}",
                skipped
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    let mut d = ProjectData::new(name);
    d.board.rules.preset = None;
    // Copper order: top first, bottom last.
    copper.sort_by_key(|(pos, bottom, ..)| (*bottom, *pos));
    let n = copper.len().max(2);
    let n = if n % 2 == 1 { n + 1 } else { n } as u8;
    d.board.stackup.copper_layers = n;
    let mut gid = 0;
    for (k, (_, bottom, _, image)) in copper.iter().enumerate() {
        let layer = if *bottom || (k + 1 == copper.len() && copper.len() > 1) {
            Layer::BOTTOM
        } else if k == 0 {
            Layer::TOP
        } else {
            Layer::Copper(Copper::Inner(k as u8))
        };
        for p in &image.0 {
            if p.unsigned_area() < 1e-6 {
                continue;
            }
            gid += 1;
            d.routing.graphics.push(BoardGraphic {
                id: format!("g{gid}"),
                layer,
                shape: Shape::Polygon { points: keyhole(p) },
                width: 0.0,
                filled: true,
            });
        }
    }
    for (side, image) in &silk {
        for p in &image.0 {
            if p.unsigned_area() < 1e-6 {
                continue;
            }
            gid += 1;
            d.routing.graphics.push(BoardGraphic {
                id: format!("g{gid}"),
                layer: Layer::Silk(*side),
                shape: Shape::Polygon { points: keyhole(p) },
                width: 0.0,
                filled: true,
            });
        }
    }

    // Make the outline from the drawn centre lines. If that fails, use the image.
    // Hand-drawn outlines often have gaps of less than 1 mm, so also try a larger
    // tolerance. Keep that result if it finds a larger board.
    let largest = |loops: &[Contour]| {
        loops
            .iter()
            .filter(|c| c.check().is_ok())
            .map(|c| c.signed_area().abs())
            .fold(0.0, f64::max)
    };
    let (mut loops, open) = chain_edges(edges.clone(), 0.02);
    if open > 0 || loops.is_empty() {
        let (wide, wide_open) = chain_edges(edges, 0.5);
        if largest(&wide) > largest(&loops) * 1.01 {
            notes.push(
                "The outline had small gaps (less than 0.5 mm), and Lamina closed them".into(),
            );
            loops = wide;
            if wide_open > 0 {
                lost(
                    &mut notes,
                    &mut losses,
                    LossKind::Edge,
                    format!(
                        "Lamina ignored {wide_open} outline segment(s) that are not part of a closed loop"
                    ),
                );
            }
        } else if open > 0 {
            lost(
                &mut notes,
                &mut losses,
                LossKind::Edge,
                format!(
                    "Lamina ignored {open} outline segment(s) that are not part of a closed loop"
                ),
            );
        }
    }
    loops.retain(|c| c.check().is_ok());
    loops.sort_by(|a, b| b.signed_area().abs().total_cmp(&a.signed_area().abs()));
    if let Some(o) = loops.first() {
        d.board.outline = Some(o.clone());
        d.board.cutouts = loops[1..].to_vec();
    } else if let Some(img) = outline_image.as_ref().and_then(|m| {
        m.0.iter()
            .max_by(|a, b| a.unsigned_area().total_cmp(&b.unsigned_area()))
            .cloned()
    }) {
        // The outline layer is filled. Its outside boundary is the board.
        let pts: Vec<Point> = img
            .exterior()
            .coords()
            .map(|c| Point::new(r6(c.x), r6(c.y)))
            .collect();
        d.board.outline = contour_from_points(pts);
        notes.push("The board outline comes from the filled shape on the outline layer".into());
    } else {
        if let Some(r) = Rect::from_points(d.routing.graphics.iter().flat_map(|g| g.shape.points()))
        {
            d.board.outline = Some(Contour::rect(r.min.x, r.min.y, r.width(), r.height()));
            notes.push(
                "There is no board outline, so Lamina uses the bounding box of the copper".into(),
            );
        }
    }

    // Holes. Eagle and other programs make slots as rows of drill hits that overlap.
    let drills = merge_slot_hits(drills);
    let mut hid = 0;
    let mut unknown_plating = 0;
    for h in &drills {
        hid += 1;
        let plated = h.plated.unwrap_or_else(|| {
            unknown_plating += 1;
            true
        });
        let (drill, at, rotation) = if h.a.dist(h.b) < 1e-6 {
            (
                Drill::Round {
                    diameter: r6(h.diameter),
                },
                h.a,
                0.0,
            )
        } else {
            let len = h.a.dist(h.b);
            let ang = (h.b.y - h.a.y).atan2(h.b.x - h.a.x).to_degrees();
            (
                Drill::Slot {
                    width: r6(len + h.diameter),
                    height: r6(h.diameter),
                },
                Point::new((h.a.x + h.b.x) / 2.0, (h.a.y + h.b.y) / 2.0),
                r6(ang.rem_euclid(360.0)),
            )
        };
        d.board.holes.push(Hole {
            id: format!("h{hid}"),
            at: Point::new(r6(at.x), r6(at.y)),
            drill,
            plated,
            pad_diameter: None,
            net: None,
            rotation,
        });
    }
    if unknown_plating > 0 {
        notes.push(format!(
            "{unknown_plating} hole(s) have no plating data, so Lamina made them plated"
        ));
    }
    if let Some(b) = d.board.outline.as_ref().and_then(|o| o.bbox()) {
        if b.min.x != 0.0 || b.min.y != 0.0 {
            d.translate(-b.min.x, -b.min.y);
        }
    }
    // Artwork has no nets. Change the rules so that they do not flag what the fab files contain.
    let r = &mut d.board.rules;
    for h in &d.board.holes {
        r.min_drill = r.min_drill.min(h.drill.min_size());
    }
    r.hole_to_hole = r.hole_to_hole.min(0.1);
    notes.push(format!(
        "{} copper layer(s), {} drawings and {} holes, as artwork only (no parts or nets)",
        copper.len(),
        d.routing.graphics.len(),
        d.board.holes.len()
    ));
    Ok(FabBoard {
        data: d,
        notes,
        losses,
        layers,
    })
}

/// Change each row of round hits that overlap, have the same size and are on a line into a slot.
fn merge_slot_hits(hits: Vec<DrillHit>) -> Vec<DrillHit> {
    let mut out: Vec<DrillHit> = Vec::new();
    for h in hits {
        let round = h.a.dist(h.b) < 1e-6;
        if let Some(last) = out.last_mut() {
            let gap = last.b.dist(h.a);
            let same = (last.diameter - h.diameter).abs() < 1e-6 && last.plated == h.plated;
            if round && same && gap > 1e-6 && gap < h.diameter * 0.95 {
                let straight = if last.a.dist(last.b) < 1e-6 {
                    true
                } else {
                    let u = last.b.sub(last.a);
                    let v = h.a.sub(last.b);
                    let cross = (u.x * v.y - u.y * v.x).abs() / (u.x.hypot(u.y) * v.x.hypot(v.y));
                    cross < 0.02 && u.x * v.x + u.y * v.y > 0.0
                };
                if straight {
                    last.b = h.a;
                    continue;
                }
            }
        }
        out.push(h);
    }
    out
}

/// Read the files of a folder or a zip archive.
pub fn read_files(path: &std::path::Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    if path.is_dir() {
        let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for p in entries {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if name.to_ascii_lowercase().ends_with(".zip") {
                continue;
            }
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            out.push((name, bytes));
        }
        return Ok(out);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.starts_with(b"PK") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
            if f.is_dir() {
                continue;
            }
            let full = f.name().map_err(|e| e.to_string())?.to_string();
            let name = full.rsplit('/').next().unwrap_or("").to_string();
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut v).map_err(|e| e.to_string())?;
            out.push((name, v));
        }
        return Ok(out);
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    out.push((name, bytes));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_names() {
        let c = |n: &str| classify(n, "");
        assert_eq!(
            c("board-F_Cu.gbr"),
            FabLayer::Copper {
                pos: 1,
                bottom: false
            }
        );
        assert_eq!(
            c("board-In2_Cu.gbr"),
            FabLayer::Copper {
                pos: 3,
                bottom: false
            }
        );
        assert_eq!(
            c("board-B_Cu.gbr"),
            FabLayer::Copper {
                pos: 99,
                bottom: true
            }
        );
        assert_eq!(
            c("Gerber_TopLayer.GTL"),
            FabLayer::Copper {
                pos: 1,
                bottom: false
            }
        );
        assert_eq!(
            c("Gerber_InnerLayer1.G1"),
            FabLayer::Copper {
                pos: 2,
                bottom: false
            }
        );
        assert_eq!(c("board.GKO"), FabLayer::Outline);
        assert_eq!(c("board-Edge_Cuts.gm1"), FabLayer::Outline);
        assert_eq!(c("x.GTO"), FabLayer::Silk(Side::Top));
        assert_eq!(c("x.GBS"), FabLayer::Mask(Side::Bottom));
        assert_eq!(c("Drill_PTH_Through.DRL"), FabLayer::Drill);
        assert_eq!(
            classify(
                "drill.txt",
                "M48
METRIC
T1C0.3
"
            ),
            FabLayer::Drill
        );
        assert_eq!(classify("readme.txt", "hello"), FabLayer::Other);
        assert_eq!(
            c("main_board-In1_Cu.gbr"),
            FabLayer::Copper {
                pos: 2,
                bottom: false
            }
        );
        assert_eq!(
            classify("anything.gbr", "%TF.FileFunction,Copper,L4,Bot*%"),
            FabLayer::Copper {
                pos: 4,
                bottom: true
            }
        );
        assert_eq!(
            classify("x.gbr", "%TF.FileFunction,Legend,Bot*%"),
            FabLayer::Silk(Side::Bottom)
        );
    }

    #[test]
    fn overlapping_hits_become_slots() {
        let hit = |x: f64| DrillHit {
            diameter: 1.0,
            a: Point::new(x, 0.0),
            b: Point::new(x, 0.0),
            plated: Some(true),
        };
        let out = merge_slot_hits(vec![hit(0.0), hit(0.5), hit(1.0), hit(1.5), hit(5.0)]);
        assert_eq!(out.len(), 2);
        assert_eq!(
            (out[0].a, out[0].b),
            (Point::new(0.0, 0.0), Point::new(1.5, 0.0))
        );
        assert_eq!(out[1].a, out[1].b);
    }

    #[test]
    fn keyhole_keeps_area() {
        let outer = geo::LineString::from(vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);
        let hole = |x: f64, y: f64| {
            geo::LineString::from(vec![
                (x, y),
                (x + 2.0, y),
                (x + 2.0, y + 2.0),
                (x, y + 2.0),
                (x, y),
            ])
        };
        let p = Polygon::new(outer, vec![hole(2.0, 2.0), hole(6.0, 6.0)]);
        let ring = keyhole(&p);
        let area = lamina_geom::prim::ring_area(&ring).abs();
        assert!((area - (100.0 - 8.0)).abs() < 1e-9, "{area}");
    }
}

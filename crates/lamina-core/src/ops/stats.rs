use crate::Project;
use lamina_model::*;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DrillStats {
    pub width: f64,
    pub height: f64,
    pub plated: bool,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BoardStats {
    pub units: &'static str,
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// Outline area minus cutouts in square mm; drill bores are not subtracted.
    pub area: Option<f64>,
    pub parts_top: usize,
    pub parts_bottom: usize,
    pub parts_unplaced: usize,
    /// Pad counts include placed parts only.
    pub smd_pads: usize,
    pub through_hole_pads: usize,
    pub vias: usize,
    pub drills: Vec<DrillStats>,
    pub smallest_track: Option<f64>,
    /// Sum of every track segment in mm, including branches.
    pub track_length: f64,
}

impl Project {
    pub fn board_stats(&self) -> BoardStats {
        let d = &self.data;
        let bounds = d.board.outline.as_ref().and_then(Contour::bbox);
        let mut s = BoardStats {
            units: "mm",
            width: bounds.map(|b| b.width()),
            height: bounds.map(|b| b.height()),
            area: d.board.outline.as_ref().map(|o| {
                (o.signed_area().abs()
                    - d.board
                        .cutouts
                        .iter()
                        .map(|c| c.signed_area().abs())
                        .sum::<f64>())
                .max(0.0)
            }),
            parts_top: 0,
            parts_bottom: 0,
            parts_unplaced: 0,
            smd_pads: 0,
            through_hole_pads: 0,
            vias: d.routing.vias.len(),
            drills: Vec::new(),
            smallest_track: d.routing.tracks.iter().map(|t| t.width).reduce(f64::min),
            track_length: d.net_lengths().values().map(|n| n.length).sum(),
        };
        for (name, part) in &d.parts.parts {
            let Some(pl) = d.placement.placements.get(name) else {
                s.parts_unplaced += 1;
                continue;
            };
            match pl.side {
                Side::Top => s.parts_top += 1,
                Side::Bottom => s.parts_bottom += 1,
            }
            if let Some(fp) = d.footprints.get(&part.footprint) {
                for pad in &fp.pads {
                    match pad.kind {
                        PadKind::Smd => s.smd_pads += 1,
                        PadKind::Through { .. } => s.through_hole_pads += 1,
                    }
                }
            }
        }
        let mut drills = BTreeMap::new();
        for drill in lamina_geom::build(d).drills {
            let (w, h) = match drill.prim {
                lamina_geom::Prim::Circle { r, .. } => (2.0 * r, 2.0 * r),
                lamina_geom::Prim::Capsule { a, b, r } => (2.0 * r, a.dist(b) + 2.0 * r),
                _ => continue,
            };
            *drills
                .entry((
                    (w * 1e6).round() as i64,
                    (h * 1e6).round() as i64,
                    drill.plated,
                ))
                .or_insert(0) += 1;
        }
        s.drills = drills
            .into_iter()
            .map(|((w, h, plated), count)| DrillStats {
                width: w as f64 / 1e6,
                height: h as f64 / 1e6,
                plated,
                count,
            })
            .collect();
        s
    }
}

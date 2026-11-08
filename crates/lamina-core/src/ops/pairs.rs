use crate::project::{OpResult, Project, Touched};
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SetDiffPairs {
    /// Replace all named pairs. Each has positive/negative net names, edge gap
    /// and track width in mm. An empty map removes all pair definitions.
    pub pairs: BTreeMap<String, DiffPair>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RouteDiffPair {
    /// Name from nets.json diff_pairs.
    pub pair: String,
    pub layer: Layer,
    /// Centre path in mm. Positive net goes left of the path; negative goes right.
    /// Both tracks are offset by (gap + width) / 2. New DRC errors refuse both.
    pub points: Vec<Point>,
}

fn offset_path(points: &[Point], offset: f64) -> Result<Vec<Point>, Error> {
    let mut normals = Vec::new();
    for edge in points.windows(2) {
        let length = edge[0].dist(edge[1]);
        if length < 1e-6 {
            return Err(Error::invalid("The centre path has a zero-length segment."));
        }
        normals.push(Point::new(
            -(edge[1].y - edge[0].y) / length,
            (edge[1].x - edge[0].x) / length,
        ));
    }
    let mut out = vec![points[0] + normals[0] * offset];
    for i in 1..points.len() - 1 {
        let (a, b) = (normals[i - 1], normals[i]);
        let divisor = 1.0 + a.x * b.x + a.y * b.y;
        if divisor < 0.125 {
            return Err(Error::invalid(
                "The centre path reverses too sharply. Use gentler bends.",
            ));
        }
        out.push(points[i] + (a + b) * (offset / divisor));
    }
    out.push(points[points.len() - 1] + normals[normals.len() - 1] * offset);
    for (edge, normal) in out.windows(2).zip(&normals) {
        let delta = edge[1] - edge[0];
        if delta.x * normal.y - delta.y * normal.x <= 1e-6 {
            return Err(Error::invalid(
                "A bend leaves no room for the inner track. Lengthen its segments.",
            ));
        }
    }
    Ok(out)
}

pub(crate) fn check_pairs(nets: &NetsFile) -> Result<(), Error> {
    let mut used = BTreeSet::new();
    for (name, pair) in &nets.diff_pairs {
        if name.trim().is_empty() || pair.positive == pair.negative {
            return Err(Error::invalid(
                "A pair needs a non-empty name and two distinct nets.",
            ));
        }
        for net in [&pair.positive, &pair.negative] {
            if !nets.nets.contains_key(net) {
                return Err(Error::invalid(format!(
                    "Pair {name}: net `{net}` does not exist. Remove the pair or add its net."
                )));
            }
            if !used.insert(net) {
                return Err(Error::invalid(format!(
                    "Net `{net}` belongs to more than one pair."
                )));
            }
        }
        if !pair.gap.is_finite() || pair.gap <= 0.0 || !pair.width.is_finite() || pair.width <= 0.0
        {
            return Err(Error::invalid(format!(
                "Pair {name}: gap and width must be finite and more than 0 mm."
            )));
        }
    }
    Ok(())
}

impl Project {
    pub fn route_diff_pair(&mut self, p: RouteDiffPair) -> Result<OpResult, Error> {
        check_pairs(&self.data.nets)?;
        let pair = self.data.nets.diff_pairs.get(&p.pair).ok_or_else(|| {
            Error::invalid(format!(
                "Pair `{}` does not exist. Define it with set_diff_pairs.",
                p.pair
            ))
        })?;
        if p.layer.copper().is_none() || !p.layer.exists_in(self.data.board.stackup.copper_layers) {
            return Err(Error::invalid("Choose an existing copper layer."));
        }
        if p.points.len() < 2 || p.points.iter().any(|p| !p.is_finite()) {
            return Err(Error::invalid(
                "Give at least two finite points for the centre path.",
            ));
        }
        let offset = (pair.gap + pair.width) / 2.0;
        let positive = offset_path(&p.points, offset)?;
        let negative = offset_path(&p.points, -offset)?;
        let mut d = self.data.clone();
        let mut ids = self.all_ids();
        for (net, points) in [(&pair.positive, positive), (&pair.negative, negative)] {
            let id = crate::next_id("t", ids.iter().map(String::as_str));
            ids.push(id.clone());
            d.routing.tracks.push(Track {
                id,
                net: net.clone(),
                layer: p.layer,
                width: pair.width,
                points,
            });
        }
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            true,
            format!("Routed pair {}", p.pair),
        )
    }

    pub fn set_diff_pairs(&mut self, p: SetDiffPairs) -> Result<OpResult, Error> {
        let mut d = self.data.clone();
        d.nets.diff_pairs = p.pairs;
        check_pairs(&d.nets)?;
        self.commit(
            d,
            Touched::file(FileKind::Nets),
            false,
            "Differential pairs set",
        )
    }
}

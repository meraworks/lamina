use lamina_model::*;
use std::collections::BTreeSet;

pub fn check(d: &ProjectData) -> Vec<Violation> {
    let mut out = Vec::new();
    for (name, pair) in &d.nets.diff_pairs {
        let mut gap_items = BTreeSet::new();
        let mut uncoupled = BTreeSet::new();
        for (net, partner) in [
            (&pair.positive, &pair.negative),
            (&pair.negative, &pair.positive),
        ] {
            for track in d.routing.tracks.iter().filter(|t| &t.net == net) {
                for edge in track.points.windows(2) {
                    let length = edge[0].dist(edge[1]);
                    if length < 1e-9 {
                        continue;
                    }
                    let u = (edge[1] - edge[0]) * (1.0 / length);
                    let mut covered = Vec::new();
                    let mut wrong_gap = BTreeSet::new();
                    for other in d
                        .routing
                        .tracks
                        .iter()
                        .filter(|t| &t.net == partner && t.layer == track.layer)
                    {
                        for q in other.points.windows(2) {
                            let delta = q[1] - q[0];
                            let qlen = q[0].dist(q[1]);
                            if qlen < 1e-9 || (u.x * delta.y - u.y * delta.x).abs() > qlen * 1e-6 {
                                continue;
                            }
                            let a = q[0] - edge[0];
                            let b = q[1] - edge[0];
                            let x = a.x * u.x + a.y * u.y;
                            let y = b.x * u.x + b.y * u.y;
                            let lo = x.min(y).max(0.0);
                            let hi = x.max(y).min(length);
                            if hi <= lo + 1e-6 {
                                continue;
                            }
                            let gap =
                                (a.x * u.y - a.y * u.x).abs() - (track.width + other.width) / 2.0;
                            if (gap - pair.gap).abs() > 0.01 {
                                wrong_gap.insert(other.id.clone());
                            } else {
                                // Offset paths have unequal segment ends at corners.
                                let corner = pair.gap + pair.width;
                                covered.push(((lo - corner).max(0.0), (hi + corner).min(length)));
                            }
                        }
                    }
                    covered.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let mut end: f64 = 0.0;
                    let mut missing = false;
                    for (lo, hi) in covered {
                        if lo > end + 1e-6 {
                            missing = true;
                        }
                        end = end.max(hi);
                    }
                    if missing || end + 1e-6 < length {
                        uncoupled.insert(track.id.clone());
                        if !wrong_gap.is_empty() {
                            gap_items.insert(track.id.clone());
                            gap_items.extend(wrong_gap);
                        }
                    }
                }
            }
        }
        if !gap_items.is_empty() {
            out.push(Violation::warning("diff_pair_gap", format!("Pair {name} has parallel tracks outside its {:.3} mm edge gap (tolerance 0.01 mm).", pair.gap)).items(gap_items));
        }
        if !uncoupled.is_empty() {
            out.push(Violation::warning("diff_pair_uncoupled", format!("Pair {name} has tracks without a parallel partner at its gap on the same layer.")).items(uncoupled));
        }
    }
    out
}

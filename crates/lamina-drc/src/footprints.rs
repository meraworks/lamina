//! Checks of the footprints in use: their evidence and their symmetry.

use std::collections::BTreeMap;

use lamina_model::*;

/// The state of the evidence of a footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceState {
    /// The current pads have a record that passed, or the IPC generator made them.
    Verified,
    /// The last record of the current pads is a fail.
    Mismatch,
    /// The records are for other pads: the footprint changed after the check.
    Stale,
    /// No record.
    Unverified,
}

pub fn evidence_state(fp: &Footprint) -> EvidenceState {
    match fp.current_evidence() {
        Some(e) if e.pass => EvidenceState::Verified,
        Some(_) => EvidenceState::Mismatch,
        None if !fp.evidence.is_empty() => EvidenceState::Stale,
        None if matches!(fp.provenance, Provenance::Ipc7351 { .. }) => EvidenceState::Verified,
        None => EvidenceState::Unverified,
    }
}

pub(crate) fn check(d: &ProjectData) -> Vec<Violation> {
    let mut used: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (r, part) in &d.parts.parts {
        used.entry(part.footprint.as_str())
            .or_default()
            .push(r.as_str());
    }
    let mut v = Vec::new();
    for (id, parts) in used {
        let Some(fp) = d.footprints.get(id) else {
            continue;
        };
        let who = parts.join(", ");
        match evidence_state(fp) {
            EvidenceState::Verified => {}
            EvidenceState::Mismatch => {
                let e = fp.current_evidence().expect("a record");
                v.push(
                    Violation::error(
                        "footprint_mismatch",
                        format!(
                            "The footprint {id} ({who}) does not agree with {}: {}",
                            e.document,
                            e.findings.join(" ")
                        ),
                    )
                    .items([id.to_string()]),
                );
            }
            EvidenceState::Stale => v.push(
                Violation::warning(
                    "footprint_stale",
                    format!(
                        "The pads of the footprint {id} ({who}) changed after its check. Verify it again"
                    ),
                )
                .items([id.to_string()]),
            ),
            EvidenceState::Unverified => {
                if fp
                    .source_hash
                    .as_ref()
                    .is_some_and(|h| *h != fp.pads_hash())
                {
                    v.push(
                        Violation::warning(
                            "footprint_changed",
                            format!(
                                "The pads of the footprint {id} ({who}) differ from its source, and no check covers them"
                            ),
                        )
                        .items([id.to_string()]),
                    );
                }
                v.push(
                    Violation::warning(
                        "footprint_unverified",
                        format!(
                            "The footprint {id} ({who}) has no evidence. Use `verify_footprint` or `audit_footprints`"
                        ),
                    )
                    .items([id.to_string()]),
                );
            }
        }
        let near = lamina_footprint::compare::near_symmetry(fp);
        if !near.is_empty() {
            let list: Vec<String> = near
                .iter()
                .take(6)
                .map(|(n, d)| format!("{n} ({d} mm)"))
                .collect();
            v.push(
                Violation::warning(
                    "footprint_symmetry",
                    format!(
                        "The footprint {id} ({who}) is almost symmetric: these pads are off their mirror position: {}",
                        list.join(", ")
                    ),
                )
                .items([id.to_string()]),
            );
        }
        if let Some(text) = pin1_mark(fp) {
            v.push(
                Violation::warning("pin1_mark", format!("The footprint {id} ({who}) {text}"))
                    .items([id.to_string()]),
            );
        }
    }
    v
}

/// The centre and the size of each silkscreen drawing of a footprint.
fn silk_boxes(fp: &Footprint) -> Vec<(Point, f64, f64)> {
    fp.graphics
        .iter()
        .filter(|g| g.layer == FpLayer::Silk)
        .flat_map(|g| lamina_geom::art::shape_prims(&g.shape, g.width, g.filled, &|p| p))
        .map(|p| {
            let b = p.bbox();
            (
                Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0),
                b.max.x - b.min.x,
                b.max.y - b.min.y,
            )
        })
        .collect()
}

/// The problem with the pin 1 mark of a footprint, if it has one.
///
/// The check is for a footprint with more than two pads whose pad pattern is
/// the same after a turn, while the pad numbers are not. Then only the
/// silkscreen tells a person how to put the part on the board.
pub fn pin1_mark(fp: &Footprint) -> Option<String> {
    const TOL: f64 = 0.02;
    const SILK_TOL: f64 = 0.06;
    let pads: Vec<&Pad> = fp.pads.iter().filter(|p| !p.number.is_empty()).collect();
    let numbers: std::collections::BTreeSet<&str> =
        pads.iter().map(|p| p.number.as_str()).collect();
    if numbers.len() <= 2 {
        return None;
    }
    let first = pads
        .iter()
        .find(|p| p.number == "1")
        .or_else(|| pads.iter().find(|p| p.number == "A1"))?;
    // The centre of the pad pattern.
    let b = fp.pads_bbox()?;
    let c = Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
    let silk = silk_boxes(fp);
    for turn in [180.0_f64, 90.0, 270.0] {
        let tf = |p: Point| p.sub(c).rotated(turn).add(c);
        let quarter = (turn / 90.0).round() as i64 % 2 == 1;
        let size = |p: &Pad| {
            let (w, h) = p.shape.size();
            let odd = ((p.rotation / 90.0).round() as i64).rem_euclid(2) == 1;
            if odd { (h, w) } else { (w, h) }
        };
        // The pad that is at the turned position of each pad.
        let mut same_numbers = true;
        let mut symmetric = true;
        for p in &pads {
            let at = tf(p.at);
            let (w, h) = size(p);
            let (w, h) = if quarter { (h, w) } else { (w, h) };
            let image = pads.iter().find(|q| {
                let (qw, qh) = size(q);
                q.at.dist(at) <= TOL
                    && (qw - w).abs() <= TOL
                    && (qh - h).abs() <= TOL
                    && q.is_smd() == p.is_smd()
            });
            match image {
                None => {
                    symmetric = false;
                    break;
                }
                Some(q) if q.number != p.number => same_numbers = false,
                _ => {}
            }
        }
        if !symmetric || same_numbers {
            continue;
        }
        // The silkscreen drawings that have no partner after the turn.
        let marks: Vec<Point> = silk
            .iter()
            .filter(|(at, w, h)| {
                let t = tf(*at);
                let (w, h) = if quarter { (*h, *w) } else { (*w, *h) };
                !silk.iter().any(|(o, ow, oh)| {
                    o.dist(t) <= SILK_TOL
                        && (ow - w).abs() <= SILK_TOL
                        && (oh - h).abs() <= SILK_TOL
                })
            })
            .map(|(at, _, _)| *at)
            .collect();
        if marks.is_empty() {
            return Some(format!(
                "has the same pads after a turn of {turn} degrees, and its silkscreen is also the same. No mark shows where pad {} is",
                first.number
            ));
        }
        // A footprint can have more than one mark: a dot at pad 1 and, for
        // example, a ground mark at a different pad. One mark at pad 1 is
        // sufficient. The mark is wrong only if none is at pad 1 and one is
        // at the pad that takes its position after the turn.
        let turned = tf(first.at);
        let at_first = marks
            .iter()
            .any(|m| m.dist(first.at) + 0.2 < m.dist(turned));
        let at_other = marks
            .iter()
            .any(|m| m.dist(turned) + 0.2 < m.dist(first.at));
        if !at_first && at_other {
            let other = pads
                .iter()
                .find(|q| q.at.dist(turned) <= TOL)
                .map(|q| q.number.as_str())
                .unwrap_or("?");
            return Some(format!(
                "has a silkscreen mark that is nearer to pad {other} than to pad {}",
                first.number
            ));
        }
    }
    None
}

//! Compare the pads of two footprints: the same part from two sources, or a
//! footprint against the numbers of a datasheet.

use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How much two footprints can differ.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tolerance {
    /// Pad centre distance in mm.
    pub centre: f64,
    /// Pad width and height difference in mm.
    pub size: f64,
    /// Drill difference in mm.
    pub drill: f64,
}

impl Tolerance {
    /// For a land pattern that the datasheet gives: the pads must agree.
    pub const EXACT: Tolerance = Tolerance {
        centre: 0.05,
        size: 0.1,
        drill: 0.05,
    };
    /// For two sources of one package: land patterns differ by design.
    pub const SOURCE: Tolerance = Tolerance {
        centre: 0.15,
        size: 0.4,
        drill: 0.15,
    };
}

/// The difference of one pad pair.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PadDiff {
    pub pad: String,
    /// The number of the pad on the other side, if it is different.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    pub centre: f64,
    pub width: f64,
    pub height: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drill: Option<f64>,
}

/// The result of a comparison of footprint `a` with footprint `b`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Comparison {
    /// True if no difference is above the tolerance.
    pub same: bool,
    /// The turn of `b` in degrees that makes it fit `a`.
    pub turn: f64,
    /// True if `b` fits only as a mirror image (a bottom view).
    pub mirrored: bool,
    /// The move of `b` after the turn, in mm (different origins).
    pub shift: Point,
    pub worst_centre: f64,
    pub worst_size: f64,
    pub worst_drill: f64,
    /// The pads of `a` that have no partner in `b`.
    pub only_a: Vec<String>,
    /// The pads of `b` that have no partner in `a`.
    pub only_b: Vec<String>,
    /// The pad pairs that differ, the worst first.
    pub pads: Vec<PadDiff>,
    /// The differences in words.
    pub findings: Vec<String>,
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn turned(p: Point, turn: f64, mirror: bool) -> Point {
    let p = if mirror { Point::new(-p.x, p.y) } else { p };
    p.rotated(turn)
}

fn drill_of(p: &Pad) -> Option<f64> {
    match &p.kind {
        PadKind::Through { drill, .. } => Some(drill.min_size()),
        PadKind::Smd => None,
    }
}

/// The pad size as it lies after its own rotation and the turn.
fn size_of(p: &Pad, turn: f64) -> (f64, f64) {
    let (w, h) = p.shape.size();
    let quarter = ((p.rotation + turn) / 90.0).round() as i64;
    if quarter.rem_euclid(2) == 1 {
        (h, w)
    } else {
        (w, h)
    }
}

/// The turn, mirror and shift of `b` that puts the most pads on pads of `a`.
fn align(a: &Footprint, b: &Footprint, reach: f64) -> (f64, bool, Point, usize) {
    let pa: Vec<Point> = a.pads.iter().map(|p| p.at).collect();
    let mut best = (0.0, false, Point::new(0.0, 0.0), 0usize);
    // Vote with a limited number of pads, so that a large array stays fast.
    let step_a = (pa.len() / 40).max(1);
    for mirror in [false, true] {
        for turn in [0.0, 90.0, 180.0, 270.0] {
            let pb: Vec<Point> = b.pads.iter().map(|p| turned(p.at, turn, mirror)).collect();
            let step_b = (pb.len() / 40).max(1);
            for qa in pa.iter().step_by(step_a) {
                for qb in pb.iter().step_by(step_b) {
                    let shift = qa.sub(*qb);
                    let hits = pb
                        .iter()
                        .filter(|q| {
                            let m = q.add(shift);
                            pa.iter().any(|p| p.dist(m) <= reach)
                        })
                        .count();
                    if hits > best.3 {
                        best = (turn, mirror, shift, hits);
                    }
                }
            }
            // The same footprint fits with no turn. Prefer that.
            if best.3 == pb.len() && best.3 == pa.len() {
                return best;
            }
        }
    }
    best
}

/// Compare the pads of two footprints. The comparison finds the turn, the
/// mirror and the move of `b` itself, so the two sources can have a
/// different origin, a different pad 1 corner, or a view from below.
pub fn compare(a: &Footprint, b: &Footprint, tol: &Tolerance) -> Comparison {
    let reach = tol.centre.max(0.3);
    let (turn, mirrored, rough, _) = align(a, b, reach);
    // The vote can use a pad that is out of place. Use the median of the
    // pad offsets, so that one bad pad does not move all of the others.
    let (mut dx, mut dy) = (Vec::new(), Vec::new());
    for q in &b.pads {
        let m = turned(q.at, turn, mirrored).add(rough);
        let near = a
            .pads
            .iter()
            .map(|p| (p.at.dist(m), p.at.sub(m)))
            .filter(|(d, _)| *d <= reach)
            .min_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((_, off)) = near {
            dx.push(off.x);
            dy.push(off.y);
        }
    }
    let median = |v: &mut Vec<f64>| {
        if v.is_empty() {
            return 0.0;
        }
        v.sort_by(|x, y| x.total_cmp(y));
        v[v.len() / 2]
    };
    let shift = rough.add(Point::new(median(&mut dx), median(&mut dy)));
    let place = |p: &Pad| turned(p.at, turn, mirrored).add(shift);
    let mut used = vec![false; b.pads.len()];
    let mut pads = Vec::new();
    let mut only_a = Vec::new();
    let mut renumbered = 0;
    let (mut worst_centre, mut worst_size, mut worst_drill) = (0.0f64, 0.0f64, 0.0f64);
    for p in &a.pads {
        // The nearest free pad of `b`. A pad with the same number wins a tie.
        let mut hit: Option<(usize, f64)> = None;
        for (j, q) in b.pads.iter().enumerate() {
            if used[j] {
                continue;
            }
            let dist = p.at.dist(place(q));
            let better = match hit {
                None => true,
                Some((k, best)) => {
                    dist + 1e-6 < best
                        || ((dist - best).abs() <= 1e-6
                            && q.number == p.number
                            && b.pads[k].number != p.number)
                }
            };
            if dist <= reach && better {
                hit = Some((j, dist));
            }
        }
        let Some((j, centre)) = hit else {
            only_a.push(p.number.clone());
            continue;
        };
        used[j] = true;
        let q = &b.pads[j];
        let (wa, ha) = size_of(p, 0.0);
        let (wb, hb) = size_of(q, turn);
        let (dw, dh) = (wa - wb, ha - hb);
        let drill = match (drill_of(p), drill_of(q)) {
            (Some(x), Some(y)) => Some(x - y),
            (None, None) => None,
            // One side has a hole and the other has not.
            (Some(x), None) | (None, Some(x)) => Some(x),
        };
        worst_centre = worst_centre.max(centre);
        worst_size = worst_size.max(dw.abs()).max(dh.abs());
        worst_drill = worst_drill.max(drill.unwrap_or(0.0).abs());
        let other = (q.number != p.number).then(|| q.number.clone());
        if other.is_some() {
            renumbered += 1;
        }
        let bad = centre > tol.centre
            || dw.abs() > tol.size
            || dh.abs() > tol.size
            || drill.is_some_and(|d| d.abs() > tol.drill)
            || other.is_some();
        if bad {
            pads.push(PadDiff {
                pad: p.number.clone(),
                other,
                centre: r3(centre),
                width: r3(dw),
                height: r3(dh),
                drill: drill.map(r3),
            });
        }
    }
    let only_b: Vec<String> = b
        .pads
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(p, _)| p.number.clone())
        .collect();
    pads.sort_by(|x, y| {
        let k = |d: &PadDiff| d.centre.max(d.width.abs()).max(d.height.abs());
        k(y).total_cmp(&k(x))
    });

    let mut findings = Vec::new();
    if a.pads.len() != b.pads.len() {
        findings.push(format!(
            "The pad count is different: {} and {}.",
            a.pads.len(),
            b.pads.len()
        ));
    }
    if !only_a.is_empty() {
        findings.push(format!(
            "Pads with no partner in the second footprint: {}.",
            only_a.join(", ")
        ));
    }
    if !only_b.is_empty() {
        findings.push(format!(
            "Pads of the second footprint with no partner: {}.",
            only_b.join(", ")
        ));
    }
    if mirrored {
        findings
            .push("The second footprint fits only as a mirror image (a view from below).".into());
    }
    if renumbered > 0 {
        findings.push(format!(
            "{renumbered} pad(s) are in the same place but have different numbers."
        ));
    }
    if worst_centre > tol.centre {
        findings.push(format!(
            "A pad centre differs by {} mm (tolerance {} mm).",
            r3(worst_centre),
            tol.centre
        ));
    }
    if worst_size > tol.size {
        findings.push(format!(
            "A pad size differs by {} mm (tolerance {} mm).",
            r3(worst_size),
            tol.size
        ));
    }
    if worst_drill > tol.drill {
        findings.push(format!(
            "A drill differs by {} mm (tolerance {} mm).",
            r3(worst_drill),
            tol.drill
        ));
    }
    let same = findings.is_empty();
    if turn != 0.0 && !mirrored {
        findings.push(format!(
            "The second footprint is turned by {turn} degrees. Examine the pick-and-place rotation."
        ));
    }
    Comparison {
        same,
        turn,
        mirrored,
        shift: Point::new(r3(shift.x), r3(shift.y)),
        worst_centre: r3(worst_centre),
        worst_size: r3(worst_size),
        worst_drill: r3(worst_drill),
        only_a,
        only_b,
        pads,
        findings,
    }
}

/// The pads that have a mirror partner that is almost, but not exactly, in
/// the mirror position. A land pattern is symmetric in most packages, so a
/// small difference is frequently an error.
pub fn near_symmetry(fp: &Footprint) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    if fp.pads.len() < 4 {
        return out;
    }
    let Some(b) = fp.pads_bbox() else { return out };
    let cx = (b.min.x + b.max.x) / 2.0;
    let mut exact = 0;
    let mut near = Vec::new();
    for p in &fp.pads {
        // Some pads share a number: an array of vias in one large pad. Its
        // layout follows the heat, not the symmetry of the package.
        if fp.pads.iter().filter(|q| q.number == p.number).count() > 1 {
            continue;
        }
        let m = Point::new(2.0 * cx - p.at.x, p.at.y);
        let d = fp
            .pads
            .iter()
            .filter(|q| q.shape.size() == p.shape.size())
            .map(|q| q.at.dist(m))
            .fold(f64::INFINITY, f64::min);
        if d <= 0.02 {
            exact += 1;
        } else if d <= 0.2 {
            near.push((p.number.clone(), r3(d)));
        }
    }
    // Most pads mirror exactly, and some do not.
    if !near.is_empty() && exact * 2 >= fp.pads.len() {
        out = near;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, from_package};

    fn fp(name: &str) -> Footprint {
        from_package(name, &Options::default(), None).unwrap()
    }

    #[test]
    fn the_same_package_is_the_same() {
        let c = compare(&fp("SOIC-8"), &fp("SOIC-8"), &Tolerance::EXACT);
        assert!(c.same, "{:?}", c.findings);
        assert_eq!(c.turn, 0.0);
    }

    #[test]
    fn different_packages_differ() {
        for (a, b) in [("R_0603", "R_0805"), ("SOIC-8", "TSSOP-8")] {
            let c = compare(&fp(a), &fp(b), &Tolerance::SOURCE);
            assert!(!c.same, "{a} against {b}");
        }
        let c = compare(&fp("SOIC-8"), &fp("SOIC-14"), &Tolerance::SOURCE);
        assert!(c.findings[0].contains("pad count"), "{:?}", c.findings);
    }

    #[test]
    fn a_shift_is_found_and_a_moved_pad_is_reported() {
        let a = fp("SOIC-8");
        let mut b = a.clone();
        for p in &mut b.pads {
            p.at = p.at.add(Point::new(1.0, -2.0));
        }
        let c = compare(&a, &b, &Tolerance::EXACT);
        assert!(
            c.same,
            "a different origin is not a difference: {:?}",
            c.findings
        );
        assert_eq!(c.shift, Point::new(-1.0, 2.0));
        b.pads[0].at.x += 0.1;
        let c = compare(&a, &b, &Tolerance::EXACT);
        assert!(!c.same);
        assert_eq!(c.pads[0].pad, a.pads[0].number);
        assert!((c.worst_centre - 0.1).abs() < 1e-6);
    }

    #[test]
    fn new_numbers_a_mirror_and_a_drill_are_reported() {
        let a = fp("SOIC-8");
        let mut b = a.clone();
        // The numbers in the reverse order: the geometry is the same.
        let n = b.pads.len();
        for (i, p) in b.pads.iter_mut().enumerate() {
            p.number = (n - i).to_string();
        }
        let c = compare(&a, &b, &Tolerance::EXACT);
        assert!(!c.same);
        assert!(c.findings.iter().any(|f| f.contains("different numbers")));

        let a = fp("PinHeader_1x03_P2.54");
        let mut b = a.clone();
        if let PadKind::Through { drill, .. } = &mut b.pads[0].kind {
            *drill = Drill::Round { diameter: 0.7 };
        }
        let c = compare(&a, &b, &Tolerance::EXACT);
        assert!(
            c.findings.iter().any(|f| f.contains("drill")),
            "{:?}",
            c.findings
        );
    }

    #[test]
    fn near_symmetry_finds_a_pad_that_is_off() {
        let mut f = fp("SOIC-8");
        assert!(near_symmetry(&f).is_empty());
        f.pads[0].at.x += 0.05;
        let near = near_symmetry(&f);
        assert!(!near.is_empty(), "the moved pad and its partner");
    }
}

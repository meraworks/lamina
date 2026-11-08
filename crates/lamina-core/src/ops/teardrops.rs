use crate::project::{OpResult, Project, Touched};
use lamina_model::*;
type EditResult = Result<String, String>;
fn p6(p: Point) -> Point {
    Point::new(json::round6(p.x), json::round6(p.y))
}

fn new_id(d: &ProjectData, prefix: &str) -> String {
    let r = &d.routing;
    let ids: Vec<&str> = r
        .tracks
        .iter()
        .map(|t| t.id.as_str())
        .chain(r.vias.iter().map(|v| v.id.as_str()))
        .chain(r.zones.iter().map(|z| z.id.as_str()))
        .chain(r.keepouts.iter().map(|k| k.id.as_str()))
        .chain(r.texts.iter().map(|k| k.id.as_str()))
        .chain(r.graphics.iter().map(|k| k.id.as_str()))
        .chain(d.board.holes.iter().map(|h| h.id.as_str()))
        .collect();
    crate::next_id(prefix, ids)
}

/// The id prefix of teardrop tracks, so that they can be found and removed.
pub const TEARDROP: &str = "tdrop";

/// Teardrops in the line style of other editors: where a track meets a pad
/// or a via that is wider than the track, two short tracks go from the edge
/// of the pad to a point on the track. They make the joint stronger. A
/// teardrop that comes too close to other copper is not added.
pub fn add_teardrops(d: &mut ProjectData) -> EditResult {
    use lamina_geom::{Owner, Prim};
    remove_teardrops(d)?;
    let geo = lamina_geom::build(d);
    let layers = d.board.stackup.copper_layers;
    let rules = d.board.rules.clone();
    let clearance = |n: &str, layer: Layer| {
        let class = d.nets.nets.get(n).and_then(|x| x.class.clone());
        rules
            .resolve(Some(n), class.as_deref(), Some(layer))
            .clearance
    };
    let mut added: Vec<Track> = Vec::new();
    let mut joints = 0;
    for t in &d.routing.tracks {
        let Some(li) = t.layer.copper().map(|c| c.index(layers)) else {
            continue;
        };
        if t.points.len() < 2 {
            continue;
        }
        let fwd: Vec<Point> = t.points.clone();
        let back: Vec<Point> = t.points.iter().rev().copied().collect();
        for path in [fwd, back] {
            let p = path[0];
            // The pad or via of the net under the end of the track.
            let Some((pad, half)) = geo
                .copper
                .iter()
                .filter(|it| {
                    it.layer == li
                        && it.net.as_deref() == Some(t.net.as_str())
                        && matches!(it.owner, Owner::Pad { .. } | Owner::Via { .. })
                        && it.prim.contains(p)
                })
                .map(|it| {
                    let b = it.prim.bbox();
                    (&it.prim, b.width().min(b.height()) / 2.0)
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
            else {
                continue;
            };
            // Only where the pad is clearly wider than the track.
            if half * 2.0 < t.width * 1.6 {
                continue;
            }
            let total: f64 = path.windows(2).map(|w| w[0].dist(w[1])).sum();
            let reach = (half * 2.5).min(total * 0.6);
            if reach <= half + t.width {
                continue;
            }
            // The tip is on the track, `reach` from the end, measured along it.
            let Some(tip) = along(&path, reach) else {
                continue;
            };
            let dir = tip.sub(p);
            let dl = p.dist(tip);
            if dl < 1e-6 {
                continue;
            }
            let u = Point::new(dir.x / dl, dir.y / dl);
            let n = Point::new(-u.y, u.x);
            let side = (half - t.width / 2.0).max(0.0);
            let pair: Vec<Track> = [1.0, -1.0]
                .into_iter()
                .map(|s| {
                    let base = p.add(Point::new(n.x * side * s, n.y * side * s));
                    Track {
                        id: String::new(),
                        net: t.net.clone(),
                        layer: t.layer,
                        width: t.width,
                        points: vec![p6(base), p6(tip)],
                    }
                })
                .collect();
            // Both legs must start on the pad copper (a track can end off the
            // centre of a pad, or on a turned pad).
            if !pair.iter().all(|td| pad.contains(td.points[0])) {
                continue;
            }
            // The teardrop must keep the clearance to the copper of other nets,
            // also to the teardrops of other nets that this pass adds.
            let near_new = pair.iter().any(|td| {
                let cap = Prim::Capsule {
                    a: td.points[0],
                    b: td.points[1],
                    r: td.width / 2.0,
                };
                added.iter().any(|o| {
                    o.net != td.net && o.layer == td.layer && {
                        let oc = Prim::Capsule {
                            a: o.points[0],
                            b: o.points[1],
                            r: o.width / 2.0,
                        };
                        let clr = clearance(&td.net, td.layer).max(clearance(&o.net, o.layer));
                        cap.distance(&oc) < clr - 1e-6
                    }
                })
            });
            let clear = !near_new
                && pair.iter().all(|td| {
                    let cap = Prim::Capsule {
                        a: td.points[0],
                        b: td.points[1],
                        r: td.width / 2.0,
                    };
                    geo.copper.iter().all(|it| {
                        it.layer != li || it.net.as_deref() == Some(t.net.as_str()) || {
                            let clr = clearance(&t.net, t.layer)
                                .max(it.net.as_deref().map_or(0.0, |n| clearance(n, t.layer)));
                            cap.distance(&it.prim) >= clr - 1e-6
                        }
                    })
                });
            if clear {
                joints += 1;
                added.extend(pair);
            }
        }
    }
    for mut td in added {
        td.id = new_id(d, TEARDROP);
        d.routing.tracks.push(td);
    }
    if joints == 0 {
        return Err(
            "There is no joint of a track and a wider pad or via with room for a teardrop.".into(),
        );
    }
    Ok(format!(
        "Added teardrops at {joints} joint{}",
        if joints == 1 { "" } else { "s" }
    ))
}

fn along(path: &[Point], dist: f64) -> Option<Point> {
    let mut left = dist;
    for w in path.windows(2) {
        let l = w[0].dist(w[1]);
        if l >= left {
            let k = if l > 0.0 { left / l } else { 0.0 };
            return Some(Point::new(
                w[0].x + (w[1].x - w[0].x) * k,
                w[0].y + (w[1].y - w[0].y) * k,
            ));
        }
        left -= l;
    }
    None
}

/// Remove the teardrops that `add_teardrops` made.
pub fn remove_teardrops(d: &mut ProjectData) -> EditResult {
    let before = d.routing.tracks.len();
    d.routing.tracks.retain(|t| !is_teardrop(&t.id));
    Ok(format!(
        "Removed {} teardrop tracks",
        before - d.routing.tracks.len()
    ))
}

/// True for the id of a teardrop track (`td` and a number).
pub fn is_teardrop(id: &str) -> bool {
    id.strip_prefix(TEARDROP)
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

impl Project {
    pub fn add_teardrops(&mut self) -> Result<OpResult, Error> {
        let mut d = self.data.clone();
        let summary = add_teardrops(&mut d).map_err(Error::invalid)?;
        self.commit(d, Touched::file(FileKind::Routing), true, summary)
    }
    pub fn remove_teardrops(&mut self) -> Result<OpResult, Error> {
        let mut d = self.data.clone();
        let summary = remove_teardrops(&mut d).map_err(Error::invalid)?;
        self.commit(d, Touched::file(FileKind::Routing), true, summary)
    }
}

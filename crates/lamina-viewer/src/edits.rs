//! Board edits from the viewer. Each edit changes a copy of the project
//! data. The editor thread commits the copy through the engine.

use lamina_model::*;

/// Something the user can select on the board.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sel {
    Part(String),
    /// A track. `seg` is the segment that the user clicked.
    Track {
        id: String,
        seg: usize,
    },
    Via(String),
    Zone(String),
    Keepout(String),
    Hole(String),
    Outline,
}

impl Sel {
    pub fn part(&self) -> Option<&str> {
        match self {
            Sel::Part(r) => Some(r),
            _ => None,
        }
    }

    /// True if it is the same item. The segment of a track is not compared.
    pub fn same_item(&self, o: &Sel) -> bool {
        match (self, o) {
            (Sel::Track { id: a, .. }, Sel::Track { id: b, .. }) => a == b,
            _ => self == o,
        }
    }
}

/// An outline whose vertices the user can drag.
#[derive(Clone, Debug, PartialEq)]
pub enum VertexOf {
    Track(String),
    Zone(String),
    Keepout(String),
    Outline,
}

pub type EditResult = Result<String, String>;

fn r6(v: f64) -> f64 {
    json::round6(v)
}

fn p6(p: Point) -> Point {
    Point::new(r6(p.x), r6(p.y))
}

fn norm_deg(a: f64) -> f64 {
    let r = r6(a.rem_euclid(360.0));
    if r >= 360.0 { 0.0 } else { r }
}

/// The next free id with this prefix in all routing items and holes.
pub fn new_id(d: &ProjectData, prefix: &str) -> String {
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
    let max = ids
        .iter()
        .filter_map(|s| s.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", max + 1)
}

fn placed_mut<'a>(d: &'a mut ProjectData, r: &str) -> Result<&'a mut Placement, String> {
    let pl = d
        .placement
        .placements
        .get_mut(r)
        .ok_or_else(|| format!("{r} is not on the board"))?;
    if pl.locked {
        return Err(format!("{r} is locked. Unlock it in the inspector first."));
    }
    Ok(pl)
}

pub fn move_parts(d: &mut ProjectData, refs: &[String], dx: f64, dy: f64) -> EditResult {
    for r in refs {
        let pl = placed_mut(d, r)?;
        pl.x = r6(pl.x + dx);
        pl.y = r6(pl.y + dy);
    }
    Ok(format!("Moved {} by {:.3}, {:.3} mm", list(refs), dx, dy))
}

/// Rotate parts by `deg` around the centre of their origins.
pub fn rotate_parts(d: &mut ProjectData, refs: &[String], deg: f64) -> EditResult {
    let mut c = Point::new(0.0, 0.0);
    for r in refs {
        let pl = placed_mut(d, r)?;
        c = c.add(pl.origin());
    }
    let n = refs.len().max(1) as f64;
    let c = Point::new(c.x / n, c.y / n);
    rotate_parts_about(d, refs, deg, c)
}

/// Turn parts by `deg` about the point `c`. A drag turns the parts about
/// the part that the pointer holds, as in other editors.
pub fn rotate_parts_about(d: &mut ProjectData, refs: &[String], deg: f64, c: Point) -> EditResult {
    for r in refs {
        let pl = placed_mut(d, r)?;
        let p = pl.origin().sub(c).rotated(deg).add(c);
        pl.x = r6(p.x);
        pl.y = r6(p.y);
        pl.rotation = norm_deg(pl.rotation + deg);
    }
    Ok(format!("Rotated {} by {deg}°", list(refs)))
}

pub fn flip_parts(d: &mut ProjectData, refs: &[String]) -> EditResult {
    for r in refs {
        let pl = placed_mut(d, r)?;
        pl.side = pl.side.flip();
    }
    Ok(format!("Flipped {}", list(refs)))
}

pub fn set_locked(d: &mut ProjectData, refs: &[String], locked: bool) -> EditResult {
    for r in refs {
        d.placement
            .placements
            .get_mut(r)
            .ok_or_else(|| format!("{r} is not on the board"))?
            .locked = locked;
    }
    Ok(format!(
        "{} {}",
        if locked { "Locked" } else { "Unlocked" },
        list(refs)
    ))
}

pub fn set_placement(d: &mut ProjectData, r: &str, mut pl: Placement) -> EditResult {
    if !d.parts.parts.contains_key(r) {
        return Err(format!("There is no part {r}"));
    }
    if !(pl.x.is_finite() && pl.y.is_finite()) {
        return Err("The position is not a number".into());
    }
    pl.x = r6(pl.x);
    pl.y = r6(pl.y);
    pl.rotation = norm_deg(pl.rotation);
    let was = d.placement.placements.insert(r.to_string(), pl);
    Ok(if was.is_some() {
        format!("{r} at {:.3}, {:.3} mm, {}°", pl.x, pl.y, pl.rotation)
    } else {
        format!("Placed {r} at {:.3}, {:.3} mm", pl.x, pl.y)
    })
}

/// Delete the selected items. A part goes off the board but stays in the
/// parts list, so you can place it again. A track loses only the clicked
/// segment.
pub fn delete(d: &mut ProjectData, sels: &[Sel]) -> EditResult {
    let mut names = Vec::new();
    // Do the highest segment first, so that the lower indices stay valid.
    let mut segs: Vec<(&str, usize)> = sels
        .iter()
        .filter_map(|s| match s {
            Sel::Track { id, seg } => Some((id.as_str(), *seg)),
            _ => None,
        })
        .collect();
    segs.sort_by(|a, b| b.cmp(a));
    for (id, seg) in segs {
        delete_segment(d, id, seg)?;
        names.push(format!("{id} segment {}", seg + 1));
    }
    for s in sels {
        match s {
            Sel::Part(r) => {
                placed_mut(d, r)?;
                d.placement.placements.remove(r);
                names.push(r.clone());
            }
            Sel::Via(id) => {
                d.routing.vias.retain(|v| &v.id != id);
                names.push(id.clone());
            }
            Sel::Zone(id) => {
                d.routing.zones.retain(|v| &v.id != id);
                names.push(id.clone());
            }
            Sel::Keepout(id) => {
                d.routing.keepouts.retain(|v| &v.id != id);
                names.push(id.clone());
            }
            Sel::Hole(id) => {
                d.board.holes.retain(|v| &v.id != id);
                names.push(id.clone());
            }
            Sel::Outline => {
                return Err("You can edit the board outline, but you cannot delete it".into());
            }
            Sel::Track { .. } => {}
        }
    }
    if names.is_empty() {
        return Err("There is nothing to delete".into());
    }
    Ok(format!("Deleted {}", names.join(", ")))
}

fn delete_segment(d: &mut ProjectData, id: &str, seg: usize) -> Result<(), String> {
    let next = new_id(d, "t");
    let i = d
        .routing
        .tracks
        .iter()
        .position(|t| t.id == id)
        .ok_or_else(|| format!("There is no track {id}"))?;
    let t = d.routing.tracks[i].clone();
    if seg + 1 >= t.points.len() {
        return Err(format!("{id} has no segment {}", seg + 1));
    }
    let head: Vec<Point> = t.points[..=seg].to_vec();
    let tail: Vec<Point> = t.points[seg + 1..].to_vec();
    d.routing.tracks.remove(i);
    let mut at = i;
    if head.len() >= 2 {
        d.routing.tracks.insert(
            at,
            Track {
                points: head,
                ..t.clone()
            },
        );
        at += 1;
    }
    if tail.len() >= 2 {
        let id = if at == i { t.id.clone() } else { next };
        d.routing.tracks.insert(
            at,
            Track {
                id,
                points: tail,
                ..t
            },
        );
    }
    Ok(())
}

/// Move a via and the track ends of its net that are on it.
pub fn move_via(d: &mut ProjectData, id: &str, delta: Point) -> EditResult {
    let v = d
        .routing
        .vias
        .iter_mut()
        .find(|v| v.id == id)
        .ok_or_else(|| format!("There is no via {id}"))?;
    let old = v.at;
    v.at = p6(old.add(delta));
    let net = v.net.clone();
    let new = v.at;
    for t in d.routing.tracks.iter_mut().filter(|t| t.net == net) {
        for p in t.points.iter_mut() {
            if p.dist(old) < 1e-4 {
                *p = new;
            }
        }
    }
    Ok(format!("Moved {id} to {:.3}, {:.3} mm", new.x, new.y))
}

/// Drag one track segment. The segments next to it stretch to follow it.
pub fn move_segment(d: &mut ProjectData, id: &str, seg: usize, delta: Point) -> EditResult {
    let t = d
        .routing
        .tracks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| format!("There is no track {id}"))?;
    if seg + 1 >= t.points.len() {
        return Err(format!("{id} has no segment {}", seg + 1));
    }
    for k in [seg, seg + 1] {
        t.points[k] = p6(t.points[k].add(delta));
    }
    Ok(format!("Moved segment {} of {id}", seg + 1))
}

/// Move (and turn) parts, as in the push mode of other editors: the tracks on
/// their pads follow, and the tracks of other nets that are now too close
/// move aside. A track with both ends on moved parts moves with them. A track
/// with one end on a moved pad is routed again to the new pad, with its own
/// width and layer. If the tracks cannot move, the edit fails.
pub fn drag_parts_and_push(
    d: &mut ProjectData,
    refs: &[String],
    delta: Point,
    turn: f64,
    about: Point,
) -> EditResult {
    use lamina_geom::Prim;
    let layers = d.board.stackup.copper_layers;
    let centres = |d: &ProjectData| -> Vec<((String, String), Point)> {
        let mut out = Vec::new();
        for r in refs {
            let (Some(pl), Some(fp)) = (
                d.placement.placements.get(r),
                d.parts
                    .parts
                    .get(r)
                    .and_then(|p| d.footprints.get(&p.footprint)),
            ) else {
                continue;
            };
            for pad in fp.pads.iter().filter(|p| !p.number.is_empty()) {
                out.push(((r.clone(), pad.number.clone()), pl.transform(pad.at)));
            }
        }
        out
    };
    let before = centres(d);
    let mut msg = move_parts(d, refs, delta.x, delta.y)?;
    if turn != 0.0 {
        msg = format!("{msg}. {}", rotate_parts_about(d, refs, turn, about)?);
    }
    let after = centres(d);
    let moved_to = |p: Point| -> Option<Point> {
        let i = before.iter().position(|(_, c)| c.dist(p) < 1e-3)?;
        after
            .iter()
            .find(|(k, _)| *k == before[i].0)
            .map(|(_, c)| *c)
    };
    // The tracks on the pads of the moved parts follow them.
    let mut followed: Vec<String> = Vec::new();
    let tracks = d.routing.tracks.clone();
    for t in &tracks {
        let (Some(&a), Some(&b)) = (t.points.first(), t.points.last()) else {
            continue;
        };
        let (na, nb) = (moved_to(a), moved_to(b));
        if na.is_none() && nb.is_none() {
            continue;
        }
        let idx = d.routing.tracks.iter().position(|x| x.id == t.id).unwrap();
        if na.is_some() && nb.is_some() {
            // Both ends move: the track moves as the parts move.
            let pts: Vec<Point> = t
                .points
                .iter()
                .map(|p| {
                    let q = p.add(delta);
                    p6(if turn != 0.0 {
                        q.sub(about).rotated(turn).add(about)
                    } else {
                        q
                    })
                })
                .collect();
            d.routing.tracks[idx].points = pts;
        } else {
            // One end moves: route the track again from its other end.
            let (fixed, to) = match (na, nb) {
                (Some(n), None) => (b, n),
                (None, Some(n)) => (a, n),
                _ => unreachable!(),
            };
            let old = d.routing.tracks.remove(idx);
            let walk = lamina_route::Walk {
                net: old.net.clone(),
                layer: old.layer,
                width: old.width,
                from: fixed,
                to,
                grid: (old.width / 2.0).clamp(0.05, 0.1),
            };
            let mut pts = lamina_route::walkaround(d, &walk).unwrap_or_else(|| vec![fixed, to]);
            if na.is_some() {
                pts.reverse();
            }
            d.routing.tracks.insert(
                idx,
                Track {
                    points: pts.into_iter().map(p6).collect(),
                    ..old
                },
            );
        }
        followed.push(t.id.clone());
    }
    // The pads of the moved parts and the tracks that followed push.
    let net_of = |part: &str, pad: &str| {
        d.nets
            .nets
            .iter()
            .find(|(_, n)| n.pads.iter().any(|p| p.part == part && p.pad == pad))
            .map(|(name, _)| name.clone())
    };
    let mut ours: Vec<Pusher> = Vec::new();
    for r in refs {
        let (Some(pl), Some(fp)) = (
            d.placement.placements.get(r),
            d.parts
                .parts
                .get(r)
                .and_then(|p| d.footprints.get(&p.footprint)),
        ) else {
            continue;
        };
        for (pad, _, prim, ls) in lamina_geom::placed_pads(fp, pl, layers) {
            let net = net_of(r, &pad.number).unwrap_or_default();
            for l in ls {
                let layer = Layer::Copper(Copper::from_index(l, layers));
                ours.push((Some(layer), vec![prim.clone()], net.clone()));
            }
        }
    }
    for t in d.routing.tracks.iter().filter(|t| followed.contains(&t.id)) {
        let caps: Vec<Prim> = t
            .points
            .windows(2)
            .map(|w| Prim::Capsule {
                a: w[0],
                b: w[1],
                r: t.width / 2.0,
            })
            .collect();
        ours.push((Some(t.layer), caps, t.net.clone()));
    }
    let pushed = push_from(d, ours, true)?;
    Ok(if pushed.is_empty() {
        msg
    } else {
        format!("{msg}. {pushed}")
    })
}

/// The id prefix of teardrop tracks, so that they can be found and removed.
pub const TEARDROP: &str = "td";

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
    let clearance = |n: &str| {
        let class = d.nets.nets.get(n).and_then(|x| x.class.clone());
        rules.clearance_for(class.as_deref())
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
            let Some(half) = geo
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
                    b.width().min(b.height()) / 2.0
                })
                .reduce(f64::max)
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
            let side = half - t.width / 2.0;
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
            // The teardrop must keep the clearance to the copper of other nets.
            let clear = pair.iter().all(|td| {
                let cap = Prim::Capsule {
                    a: td.points[0],
                    b: td.points[1],
                    r: td.width / 2.0,
                };
                geo.copper.iter().all(|it| {
                    it.layer != li || it.net.as_deref() == Some(t.net.as_str()) || {
                        let clr = clearance(&t.net).max(it.net.as_deref().map_or(0.0, clearance));
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

/// The point at the distance `dist` along a polyline from its first point.
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

/// Move a segment, then push the tracks of other nets that are now too close
/// aside. If they cannot move, the edit fails and nothing changes.
pub fn move_segment_and_push(
    d: &mut ProjectData,
    id: &str,
    seg: usize,
    delta: Point,
) -> EditResult {
    let moved = move_segment(d, id, seg, delta)?;
    let net = d
        .routing
        .tracks
        .iter()
        .find(|t| t.id == id)
        .map(|t| t.net.clone())
        .unwrap_or_default();
    let pushed = push_aside(d, &net, &[id.to_string()], &[], true)?;
    Ok(if pushed.is_empty() {
        moved
    } else {
        format!("{moved}. {pushed}")
    })
}

fn contour_mut<'a>(d: &'a mut ProjectData, of: &VertexOf) -> Result<&'a mut Contour, String> {
    match of {
        VertexOf::Zone(id) => d
            .routing
            .zones
            .iter_mut()
            .find(|z| &z.id == id)
            .map(|z| &mut z.outline)
            .ok_or_else(|| format!("There is no zone {id}")),
        VertexOf::Keepout(id) => d
            .routing
            .keepouts
            .iter_mut()
            .find(|z| &z.id == id)
            .map(|z| &mut z.outline)
            .ok_or_else(|| format!("There is no keep-out {id}")),
        VertexOf::Outline => d
            .board
            .outline
            .as_mut()
            .ok_or_else(|| "The board has no outline".to_string()),
        VertexOf::Track(_) => Err("A track is not an outline".into()),
    }
}

/// The points that the user can drag, in order.
pub fn vertices(d: &ProjectData, of: &VertexOf) -> Vec<Point> {
    match of {
        VertexOf::Track(id) => d
            .routing
            .tracks
            .iter()
            .find(|t| &t.id == id)
            .map(|t| t.points.clone())
            .unwrap_or_default(),
        VertexOf::Zone(id) => d
            .routing
            .zones
            .iter()
            .find(|z| &z.id == id)
            .map(|z| z.outline.vertices.iter().map(|v| v.point()).collect())
            .unwrap_or_default(),
        VertexOf::Keepout(id) => d
            .routing
            .keepouts
            .iter()
            .find(|z| &z.id == id)
            .map(|z| z.outline.vertices.iter().map(|v| v.point()).collect())
            .unwrap_or_default(),
        VertexOf::Outline => d
            .board
            .outline
            .as_ref()
            .map(|o| o.vertices.iter().map(|v| v.point()).collect())
            .unwrap_or_default(),
    }
}

pub fn move_vertex(d: &mut ProjectData, of: &VertexOf, idx: usize, to: Point) -> EditResult {
    let to = p6(to);
    if let VertexOf::Track(id) = of {
        let t = d
            .routing
            .tracks
            .iter_mut()
            .find(|t| &t.id == id)
            .ok_or_else(|| format!("There is no track {id}"))?;
        let p = t.points.get_mut(idx).ok_or("This point does not exist")?;
        *p = to;
        t.points.dedup_by(|a, b| a.dist(*b) < 1e-9);
        if t.points.len() < 2 {
            return Err("The track cannot become a single point".into());
        }
        return Ok(format!("Moved a corner of {id}"));
    }
    let c = contour_mut(d, of)?;
    let v = c
        .vertices
        .get_mut(idx)
        .ok_or("This vertex does not exist")?;
    let delta = to.sub(v.point());
    v.x = to.x;
    v.y = to.y;
    // An arc centre moves with its start vertex, so the shape stays valid.
    if let Some(a) = v.arc.as_mut() {
        a.center = p6(a.center.add(Point::new(delta.x / 2.0, delta.y / 2.0)));
    }
    c.check()?;
    Ok("Moved a vertex".into())
}

/// Move a full zone or keep-out outline.
pub fn move_area(d: &mut ProjectData, of: &VertexOf, delta: Point) -> EditResult {
    let c = contour_mut(d, of)?;
    for v in c.vertices.iter_mut() {
        v.x = r6(v.x + delta.x);
        v.y = r6(v.y + delta.y);
        if let Some(a) = v.arc.as_mut() {
            a.center = p6(a.center.add(delta));
        }
    }
    Ok(format!(
        "Moved {}",
        match of {
            VertexOf::Zone(id) => format!("pour {id}"),
            VertexOf::Keepout(id) => format!("keep-out {id}"),
            VertexOf::Outline => "the board outline".to_string(),
            VertexOf::Track(id) => format!("track {id}"),
        }
    ))
}

/// A route that the user draws: runs of points on each layer, with vias between them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Route {
    pub net: String,
    pub width: f64,
    pub runs: Vec<(Layer, Vec<Point>)>,
    pub vias: Vec<Point>,
}

/// Route again the tracks of other nets that are too close to the new tracks
/// `new` of `net`, so that they go around them. If a net cannot be routed
/// again, the edit fails and nothing changes.
/// With `router` false, a track with no path between its ends makes the push
/// fail, and the autorouter does not run (for a fast preview).
pub fn push_aside(
    d: &mut ProjectData,
    net: &str,
    new: &[String],
    new_vias: &[String],
    router: bool,
) -> EditResult {
    use lamina_geom::Prim;
    let caps = |t: &Track| -> Vec<Prim> {
        t.points
            .windows(2)
            .map(|w| Prim::Capsule {
                a: w[0],
                b: w[1],
                r: t.width / 2.0,
            })
            .collect()
    };
    let mut ours: Vec<Pusher> = d
        .routing
        .tracks
        .iter()
        .filter(|t| new.contains(&t.id))
        .map(|t| (Some(t.layer), caps(t), net.to_string()))
        .collect();
    // A via is on all copper layers.
    for v in d.routing.vias.iter().filter(|v| new_vias.contains(&v.id)) {
        ours.push((
            None,
            vec![Prim::Circle {
                c: v.at,
                r: v.diameter / 2.0,
            }],
            net.to_string(),
        ));
    }
    push_from(d, ours, router)
}

/// Copper that pushes: its layer (`None` for all copper layers), its shapes
/// and its net.
type Pusher = (Option<Layer>, Vec<lamina_geom::Prim>, String);

/// Route again the tracks of other nets that are too close to `ours`, so that
/// they go around it. See `push_aside`.
fn push_from(d: &mut ProjectData, ours: Vec<Pusher>, router: bool) -> EditResult {
    use lamina_geom::Prim;
    let caps = |t: &Track| -> Vec<Prim> {
        t.points
            .windows(2)
            .map(|w| Prim::Capsule {
                a: w[0],
                b: w[1],
                r: t.width / 2.0,
            })
            .collect()
    };
    let rules = d.board.rules.clone();
    let clearance = |n: &str| {
        let class = d.nets.nets.get(n).and_then(|x| x.class.clone());
        rules.clearance_for(class.as_deref())
    };
    let own: Vec<&String> = ours.iter().map(|(_, _, n)| n).collect();
    let mut hit: Vec<String> = Vec::new();
    let mut nets: Vec<String> = Vec::new();
    for t in d.routing.tracks.iter().filter(|t| !own.contains(&&t.net)) {
        let theirs = caps(t);
        let close = ours
            .iter()
            .filter(|(l, _, _)| l.is_none_or(|l| l == t.layer))
            .any(|(_, ps, n)| {
                let clr = clearance(n).max(clearance(&t.net));
                ps.iter()
                    .any(|a| theirs.iter().any(|b| a.distance(b) < clr - 1e-6))
            });
        if close {
            hit.push(t.id.clone());
            if !nets.contains(&t.net) {
                nets.push(t.net.clone());
            }
        }
    }
    if hit.is_empty() {
        return Ok(String::new());
    }
    // The open connections of the pushed nets before the push. After it, there
    // must not be more.
    let open_of = |d: &ProjectData| {
        lamina_drc::analyze(d)
            .ratsnest
            .iter()
            .filter(|r| nets.contains(&r.net))
            .count()
    };
    let open_before = open_of(d);
    // Take each track in the way out and route it again between its own two
    // ends, with its own width and layer, around the new copper. So the push
    // changes only these tracks, keeps their widths and leaves no stubs.
    let removed: Vec<Track> = d
        .routing
        .tracks
        .iter()
        .filter(|t| hit.contains(&t.id))
        .cloned()
        .collect();
    d.routing.tracks.retain(|t| !hit.contains(&t.id));
    let mut stuck: Vec<String> = Vec::new();
    let mut moved: Vec<String> = Vec::new();
    for t in &removed {
        let (Some(&from), Some(&to)) = (t.points.first(), t.points.last()) else {
            continue;
        };
        // Another track or a via of the net joins this track at a middle
        // corner. A new path between the two ends only would cut it off.
        let inner = &t.points[1..t.points.len().saturating_sub(1)];
        let joined = d.routing.tracks.iter().any(|o| {
            o.net == t.net
                && [o.points.first(), o.points.last()]
                    .into_iter()
                    .flatten()
                    .any(|e| inner.iter().any(|p| p.dist(*e) < 1e-6))
        }) || d
            .routing
            .vias
            .iter()
            .any(|v| v.net == t.net && inner.iter().any(|p| p.dist(v.at) < 1e-6));
        if joined {
            if !stuck.contains(&t.net) {
                stuck.push(t.net.clone());
            }
            continue;
        }
        let walk = lamina_route::Walk {
            net: t.net.clone(),
            layer: t.layer,
            width: t.width,
            from,
            to,
            grid: (t.width / 2.0).clamp(0.05, 0.1),
        };
        match lamina_route::walkaround(d, &walk) {
            Some(points) => {
                let id = new_id(d, "t");
                moved.push(id.clone());
                d.routing.tracks.push(Track {
                    id,
                    net: t.net.clone(),
                    layer: t.layer,
                    width: t.width,
                    points: points.into_iter().map(p6).collect(),
                });
            }
            None => {
                if !stuck.contains(&t.net) {
                    stuck.push(t.net.clone());
                }
            }
        }
    }
    // A last check with exact geometry: the walk is on a grid, and it can
    // start in a band. A moved track that is too close to other copper goes
    // out again, and its net goes to the router.
    let geo = lamina_geom::build(d);
    let layers = d.board.stackup.copper_layers;
    let mut bad: Vec<String> = Vec::new();
    for t in d.routing.tracks.iter().filter(|t| moved.contains(&t.id)) {
        let Some(c) = t.layer.copper() else { continue };
        let li = c.index(layers);
        let ours = caps(t);
        let too_close = geo.copper.iter().any(|it| {
            it.layer == li
                && it.net.as_deref() != Some(t.net.as_str())
                && ours.iter().any(|cap| {
                    let clr = clearance(&t.net).max(it.net.as_deref().map_or(0.0, clearance));
                    cap.distance(&it.prim) < clr - 1e-6
                })
        });
        if too_close {
            bad.push(t.id.clone());
            if !stuck.contains(&t.net) {
                stuck.push(t.net.clone());
            }
        }
    }
    d.routing.tracks.retain(|t| !bad.contains(&t.id));
    if !stuck.is_empty() && !router {
        return Err(format!(
            "There is no room to move {} aside on its layer.",
            stuck.join(", ")
        ));
    }
    // A track with no clear path between its ends on its layer: let the router
    // connect the net again, on any layer.
    let mut why = String::new();
    for n in &stuck {
        // The router routes the net with the width of its moved track, so
        // that a width set by hand stays.
        let width = removed
            .iter()
            .filter(|t| &t.net == n)
            .map(|t| t.width)
            .fold(0.0, f64::max);
        let out = lamina_route::autoroute(
            d,
            &lamina_route::RouteOptions {
                nets: Some(vec![n.clone()]),
                time_limit: Some(10.0),
                track_width: (width > 0.0).then_some(width),
                ..Default::default()
            },
        )?;
        if let Some(f) = out.failed.first() {
            why = format!(" ({})", f.reason);
        }
        d.routing.tracks.extend(out.tracks);
        d.routing.vias.extend(out.vias);
    }
    // A failure of a connection that was open before the push does not count.
    if open_of(d) > open_before {
        return Err(format!(
            "There is no room to move {} aside{why}. Use walkaround (Shift+R), or route on another layer.",
            nets.join(", ")
        ));
    }
    Ok(format!(
        "Moved {} track{} of {} aside",
        hit.len(),
        if hit.len() == 1 { "" } else { "s" },
        nets.join(", ")
    ))
}

pub fn add_route(d: &mut ProjectData, r: &Route) -> EditResult {
    let net = d
        .nets
        .nets
        .get(&r.net)
        .ok_or_else(|| format!("There is no net {}", r.net))?;
    let nc = d.board.rules.class(net.class.as_deref());
    let via_d = nc.via_diameter.unwrap_or(d.board.rules.via_diameter);
    let via_drill = nc.via_drill.unwrap_or(d.board.rules.via_drill);
    let mut made = 0;
    let mut len = 0.0;
    for (layer, pts) in &r.runs {
        let mut pts: Vec<Point> = pts.iter().map(|p| p6(*p)).collect();
        pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
        if pts.len() < 2 {
            continue;
        }
        len += pts.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>();
        let id = new_id(d, "t");
        d.routing.tracks.push(Track {
            id,
            net: r.net.clone(),
            layer: *layer,
            width: r.width,
            points: pts,
        });
        made += 1;
    }
    for at in &r.vias {
        let id = new_id(d, "v");
        d.routing.vias.push(Via {
            id,
            net: r.net.clone(),
            at: p6(*at),
            diameter: via_d,
            drill: via_drill,
            from: None,
            to: None,
        });
    }
    if made == 0 && r.vias.is_empty() {
        return Err("The route has no length".into());
    }
    Ok(format!(
        "Routed {} · {:.2} mm{}",
        r.net,
        len,
        match r.vias.len() {
            0 => String::new(),
            1 => " · 1 via".into(),
            n => format!(" · {n} vias"),
        }
    ))
}

pub fn add_via(d: &mut ProjectData, net: &str, at: Point) -> EditResult {
    add_route(
        d,
        &Route {
            net: net.into(),
            width: 0.0,
            runs: Vec::new(),
            vias: vec![at],
        },
    )
    .map(|_| format!("Via on {net} at {:.3}, {:.3} mm", at.x, at.y))
}

fn polygon(pts: &[Point]) -> Result<Contour, String> {
    let mut pts: Vec<Point> = pts.iter().map(|p| p6(*p)).collect();
    pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
    if pts.len() > 2 && pts[0].dist(*pts.last().unwrap()) < 1e-9 {
        pts.pop();
    }
    if pts.len() < 3 {
        return Err("An outline must have at least three corners".into());
    }
    let mut c = Contour {
        vertices: pts.iter().map(|p| Vertex::line(p.x, p.y)).collect(),
    };
    if c.signed_area() < 0.0 {
        c.vertices.reverse();
    }
    c.check()?;
    Ok(c)
}

pub fn add_zone(
    d: &mut ProjectData,
    net: Option<String>,
    layer: Layer,
    pts: &[Point],
) -> EditResult {
    let outline = polygon(pts)?;
    if let Some(n) = &net {
        if !d.nets.nets.contains_key(n) {
            return Err(format!("There is no net {n}"));
        }
    }
    let id = new_id(d, "z");
    d.routing.zones.push(Zone {
        id: id.clone(),
        net: net.clone(),
        layers: vec![layer],
        outline,
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::default(),
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    Ok(format!(
        "Added pour {id} on {layer} ({})",
        net.as_deref().unwrap_or("no net")
    ))
}

pub fn add_keepout(d: &mut ProjectData, layers: Vec<Layer>, pts: &[Point]) -> EditResult {
    let outline = polygon(pts)?;
    let id = new_id(d, "k");
    d.routing.keepouts.push(Keepout {
        id: id.clone(),
        layers,
        outline,
        forbid: Forbid::default(),
    });
    Ok(format!("Added keep-out {id}"))
}

pub fn set_outline(d: &mut ProjectData, pts: &[Point]) -> EditResult {
    let c = polygon(pts)?;
    let b = c.bbox().unwrap();
    d.board.outline = Some(c);
    Ok(format!(
        "Board outline {:.2} × {:.2} mm",
        b.width(),
        b.height()
    ))
}

fn list(refs: &[String]) -> String {
    if refs.len() <= 4 {
        refs.join(", ")
    } else {
        format!("{} parts", refs.len())
    }
}

/// The grid steps that the user can select in turn, in mm.
pub const GRIDS: [f64; 8] = [0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 1.27, 2.54];

pub fn snap(v: f64, g: f64) -> f64 {
    r6((v / g).round() * g)
}

pub fn snap_pt(p: Point, g: f64) -> Point {
    Point::new(snap(p.x, g), snap(p.y, g))
}

/// The corner of a 45° route from `a` to `b`: first straight, then diagonal.
/// With `diagonal_first`, the order is opposite.
pub fn posture(a: Point, b: Point, diagonal_first: bool) -> Vec<Point> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let (ax, ay) = (dx.abs(), dy.abs());
    if ax < 1e-9 || ay < 1e-9 || (ax - ay).abs() < 1e-9 {
        return vec![b];
    }
    let d = ax.min(ay);
    let diag = Point::new(d * dx.signum(), d * dy.signum());
    let corner = if diagonal_first {
        a.add(diag)
    } else {
        b.sub(diag)
    };
    vec![p6(corner), b]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> ProjectData {
        let mut d = ProjectData::new("t");
        d.parts.parts.insert("R1".into(), Part::new("fp"));
        d.parts.parts.insert("R2".into(), Part::new("fp"));
        d.placement
            .placements
            .insert("R1".into(), Placement::at(0.0, 0.0));
        d.placement
            .placements
            .insert("R2".into(), Placement::at(4.0, 0.0));
        d.nets.nets.insert(
            "N".into(),
            Net {
                pads: Vec::new(),
                class: None,
            },
        );
        d.routing.tracks.push(Track {
            id: "t1".into(),
            net: "N".into(),
            layer: Layer::Copper(Copper::Top),
            width: 0.25,
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 0.0),
                Point::new(2.0, 1.0),
                Point::new(3.0, 1.0),
            ],
        });
        d
    }

    #[test]
    fn rotate_group_about_centre() {
        let mut d = board();
        rotate_parts(&mut d, &["R1".into(), "R2".into()], 90.0).unwrap();
        let a = d.placement.placements["R1"];
        let b = d.placement.placements["R2"];
        assert_eq!((a.x, a.y, a.rotation), (2.0, -2.0, 90.0));
        assert_eq!((b.x, b.y), (2.0, 2.0));
    }

    #[test]
    fn locked_part_refuses_move() {
        let mut d = board();
        set_locked(&mut d, &["R1".into()], true).unwrap();
        assert!(move_parts(&mut d, &["R1".into()], 1.0, 0.0).is_err());
    }

    #[test]
    fn delete_middle_segment_splits_track() {
        let mut d = board();
        delete(
            &mut d,
            &[Sel::Track {
                id: "t1".into(),
                seg: 1,
            }],
        )
        .unwrap();
        let t = &d.routing.tracks;
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].id, "t1");
        assert_eq!(t[0].points.len(), 2);
        assert_eq!(t[1].id, "t2");
        assert_eq!(t[1].points[0], Point::new(2.0, 1.0));
    }

    #[test]
    fn delete_end_segment_keeps_id() {
        let mut d = board();
        delete(
            &mut d,
            &[Sel::Track {
                id: "t1".into(),
                seg: 0,
            }],
        )
        .unwrap();
        assert_eq!(d.routing.tracks.len(), 1);
        assert_eq!(d.routing.tracks[0].id, "t1");
        assert_eq!(d.routing.tracks[0].points.len(), 3);
    }

    #[test]
    fn deleting_part_unplaces_it() {
        let mut d = board();
        delete(&mut d, &[Sel::Part("R1".into())]).unwrap();
        assert!(!d.placement.placements.contains_key("R1"));
        assert!(d.parts.parts.contains_key("R1"));
    }

    #[test]
    fn via_drags_track_ends() {
        let mut d = board();
        add_via(&mut d, "N", Point::new(3.0, 1.0)).unwrap();
        move_via(&mut d, "v1", Point::new(0.0, 1.0)).unwrap();
        assert_eq!(d.routing.vias[0].at, Point::new(3.0, 2.0));
        assert_eq!(d.routing.tracks[0].points[3], Point::new(3.0, 2.0));
    }

    #[test]
    fn route_with_layer_change() {
        let mut d = board();
        let r = Route {
            net: "N".into(),
            width: 0.3,
            runs: vec![
                (
                    Layer::Copper(Copper::Top),
                    vec![Point::new(0.0, 5.0), Point::new(2.0, 5.0)],
                ),
                (
                    Layer::Copper(Copper::Bottom),
                    vec![Point::new(2.0, 5.0), Point::new(2.0, 8.0)],
                ),
            ],
            vias: vec![Point::new(2.0, 5.0)],
        };
        let msg = add_route(&mut d, &r).unwrap();
        assert!(msg.contains("5.00 mm"), "{msg}");
        assert_eq!(d.routing.tracks.len(), 3);
        assert_eq!(d.routing.tracks[2].id, "t3");
        assert_eq!(d.routing.vias[0].id, "v1");
    }

    #[test]
    fn zone_from_clockwise_clicks() {
        let mut d = board();
        let pts = [
            Point::new(0.0, 0.0),
            Point::new(0.0, 5.0),
            Point::new(5.0, 5.0),
            Point::new(5.0, 0.0),
        ];
        add_zone(
            &mut d,
            Some("N".into()),
            Layer::Copper(Copper::Bottom),
            &pts,
        )
        .unwrap();
        assert!(d.routing.zones[0].outline.signed_area() > 0.0);
        assert!(add_zone(&mut d, None, Layer::Copper(Copper::Top), &pts[..2]).is_err());
    }

    #[test]
    fn segment_drag_and_vertex_drag() {
        let mut d = board();
        move_segment(&mut d, "t1", 1, Point::new(0.0, 1.0)).unwrap();
        assert_eq!(d.routing.tracks[0].points[1], Point::new(1.0, 1.0));
        assert_eq!(d.routing.tracks[0].points[2], Point::new(2.0, 2.0));
        set_outline(
            &mut d,
            &[
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 10.0),
            ],
        )
        .unwrap();
        move_vertex(&mut d, &VertexOf::Outline, 2, Point::new(12.0, 12.0)).unwrap();
        assert_eq!(vertices(&d, &VertexOf::Outline)[2], Point::new(12.0, 12.0));
    }

    #[test]
    fn posture_45() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(5.0, 2.0);
        assert_eq!(posture(a, b, false), vec![Point::new(3.0, 0.0), b]);
        assert_eq!(posture(a, b, true), vec![Point::new(2.0, 2.0), b]);
        assert_eq!(posture(a, Point::new(3.0, 3.0), false).len(), 1);
    }

    #[test]
    fn push_aside_routes_the_other_net_around_the_new_track() {
        use lamina_footprint::{Options, from_package};
        let mut d = ProjectData::new("t");
        d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
        let fp = from_package("R_0603", &Options::default(), None).unwrap();
        d.footprints.insert(fp.id.clone(), fp.clone());
        for (r, x) in [("R1", 5.0), ("R2", 25.0)] {
            d.parts.parts.insert(r.into(), Part::new(&fp.id));
            d.placement
                .placements
                .insert(r.into(), Placement::at(x, 10.0));
        }
        d.nets.nets.insert(
            "A".into(),
            Net {
                pads: vec!["R1.2".parse().unwrap(), "R2.1".parse().unwrap()],
                class: None,
            },
        );
        d.nets.nets.insert(
            "X".into(),
            Net {
                pads: Vec::new(),
                class: None,
            },
        );
        let out = lamina_route::autoroute(&d, &Default::default()).unwrap();
        d.routing.tracks.extend(out.tracks);
        // A new track of X across the middle of the board, over the track of A.
        d.routing.tracks.push(Track {
            id: "tx".into(),
            net: "X".into(),
            layer: Layer::Copper(Copper::Top),
            width: 0.25,
            points: vec![Point::new(15.0, 4.0), Point::new(15.0, 16.0)],
        });
        // A hand-set width on the track of A.
        for t in d.routing.tracks.iter_mut().filter(|t| t.net == "A") {
            t.width = 0.4;
        }
        let tracks_of_a = d.routing.tracks.iter().filter(|t| t.net == "A").count();
        let msg = push_aside(&mut d, "X", &["tx".to_string()], &[], true).unwrap();
        assert!(msg.contains('A'), "{msg}");
        // The moved track keeps its width, and no piece of it stays behind.
        let a: Vec<&Track> = d.routing.tracks.iter().filter(|t| t.net == "A").collect();
        assert_eq!(a.len(), tracks_of_a, "{a:?}");
        assert!(a.iter().all(|t| (t.width - 0.4).abs() < 1e-9), "{a:?}");
        let a = lamina_drc::analyze(&d);
        assert!(
            a.ratsnest.is_empty(),
            "A is connected again: {:?}",
            a.ratsnest
        );
        let errors: Vec<_> = a
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
    }

    #[test]
    fn a_dragged_track_pushes_the_other_net_aside() {
        use lamina_footprint::{Options, from_package};
        let mut d = ProjectData::new("t");
        d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
        let fp = from_package("R_0603", &Options::default(), None).unwrap();
        d.footprints.insert(fp.id.clone(), fp.clone());
        for (r, x) in [("R1", 5.0), ("R2", 25.0)] {
            d.parts.parts.insert(r.into(), Part::new(&fp.id));
            d.placement
                .placements
                .insert(r.into(), Placement::at(x, 10.0));
        }
        d.nets.nets.insert(
            "A".into(),
            Net {
                pads: vec!["R1.2".parse().unwrap(), "R2.1".parse().unwrap()],
                class: None,
            },
        );
        d.nets.nets.insert(
            "X".into(),
            Net {
                pads: Vec::new(),
                class: None,
            },
        );
        let out = lamina_route::autoroute(&d, &Default::default()).unwrap();
        d.routing.tracks.extend(out.tracks);
        // A track of X beside the board middle, then dragged over the track of A.
        d.routing.tracks.push(Track {
            id: "tx".into(),
            net: "X".into(),
            layer: Layer::Copper(Copper::Top),
            width: 0.25,
            points: vec![Point::new(12.0, 14.0), Point::new(18.0, 14.0)],
        });
        let msg = move_segment_and_push(&mut d, "tx", 0, Point::new(0.0, -4.0)).unwrap();
        assert!(msg.contains("aside"), "{msg}");
        let a = lamina_drc::analyze(&d);
        assert!(a.ratsnest.is_empty(), "{:?}", a.ratsnest);
        let errors: Vec<_> = a
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
    }

    #[test]
    fn a_dragged_part_takes_its_tracks_and_pushes_others() {
        use lamina_footprint::{Options, from_package};
        let mut d = ProjectData::new("t");
        d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
        let fp = from_package("R_0603", &Options::default(), None).unwrap();
        d.footprints.insert(fp.id.clone(), fp.clone());
        for (r, x) in [("R1", 5.0), ("R2", 25.0)] {
            d.parts.parts.insert(r.into(), Part::new(&fp.id));
            d.placement
                .placements
                .insert(r.into(), Placement::at(x, 10.0));
        }
        d.nets.nets.insert(
            "A".into(),
            Net {
                pads: vec!["R1.2".parse().unwrap(), "R2.1".parse().unwrap()],
                class: None,
            },
        );
        d.nets.nets.insert(
            "X".into(),
            Net {
                pads: Vec::new(),
                class: None,
            },
        );
        let out = lamina_route::autoroute(&d, &Default::default()).unwrap();
        d.routing.tracks.extend(out.tracks);
        // A track of X where R2 goes.
        d.routing.tracks.push(Track {
            id: "tx".into(),
            net: "X".into(),
            layer: Layer::Copper(Copper::Top),
            width: 0.25,
            points: vec![Point::new(22.0, 14.0), Point::new(28.0, 14.0)],
        });
        let at = d.placement.placements["R2"].origin();
        drag_parts_and_push(&mut d, &["R2".to_string()], Point::new(0.0, 4.0), 0.0, at).unwrap();
        let a = lamina_drc::analyze(&d);
        assert!(a.ratsnest.is_empty(), "A follows R2: {:?}", a.ratsnest);
        let errors: Vec<_> = a
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
        // The track of X moved.
        let x = d.routing.tracks.iter().find(|t| t.net == "X").unwrap();
        assert_ne!(
            x.points,
            vec![Point::new(22.0, 14.0), Point::new(28.0, 14.0)]
        );
    }

    #[test]
    fn teardrops_keep_the_rules_and_come_off_again() {
        use lamina_footprint::{Options, from_package};
        let mut d = ProjectData::new("t");
        d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
        let fp = from_package("R_0805", &Options::default(), None).unwrap();
        d.footprints.insert(fp.id.clone(), fp.clone());
        for (r, x) in [("R1", 5.0), ("R2", 25.0)] {
            d.parts.parts.insert(r.into(), Part::new(&fp.id));
            d.placement
                .placements
                .insert(r.into(), Placement::at(x, 10.0));
        }
        d.nets.nets.insert(
            "A".into(),
            Net {
                pads: vec!["R1.2".parse().unwrap(), "R2.1".parse().unwrap()],
                class: None,
            },
        );
        let out = lamina_route::autoroute(&d, &Default::default()).unwrap();
        d.routing.tracks.extend(out.tracks);
        let tracks = d.routing.tracks.len();
        let msg = add_teardrops(&mut d).unwrap();
        assert!(msg.contains("joint"), "{msg}");
        let drops = d
            .routing
            .tracks
            .iter()
            .filter(|t| is_teardrop(&t.id))
            .count();
        assert!(drops >= 2 && drops % 2 == 0, "{drops}");
        let a = lamina_drc::analyze(&d);
        let errors: Vec<_> = a
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
        remove_teardrops(&mut d).unwrap();
        assert_eq!(d.routing.tracks.len(), tracks);
        assert!(!is_teardrop("t1") && is_teardrop("td3") && !is_teardrop("tdx"));
    }
}

//! The editor of the 2D canvas: tools, gestures, snap, overlays, the toolbar
//! and the properties panel.

use std::time::Instant;

use eframe::egui::{
    self, Color32, CornerRadius, Key, Pos2, Rect as ERect, Stroke, StrokeKind, Vec2,
};
use lamina_geom::{Owner, Prim};
use lamina_model::{Copper, Layer, Placement, Point, ProjectData, Rect, Side};
use lamina_scene::SceneLayer;

use crate::edits::{self, Route, Sel, VertexOf, posture, snap_pt};
use crate::style::*;
use crate::{App, Pick, Xf, draw_prim, widgets};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Route,
    Via,
    Zone,
    Keepout,
    Outline,
    Measure,
}

impl Tool {
    pub const ALL: [(Tool, &'static str, &'static str); 7] = [
        (Tool::Select, "Select", "S"),
        (Tool::Route, "Route", "X"),
        (Tool::Via, "Via", "V"),
        (Tool::Zone, "Pour", "Z"),
        (Tool::Keepout, "Keep-out", "K"),
        (Tool::Outline, "Outline", "O"),
        (Tool::Measure, "Measure", "D"),
    ];

    fn hint(self) -> &'static str {
        match self {
            Tool::Select => {
                "Click to select · Shift+click adds · drag to move · drag empty space to box-select · right-click for the menu · right-drag to pan"
            }
            Tool::Route => {
                "Click a pad to start · click to add corners · V via and switch layer · / bend · W / Shift+W width · Shift+R direct / walkaround / push · Backspace undo corner · Enter finish · Esc cancel"
            }
            Tool::Via => "Click the copper of a net to put a via there",
            Tool::Zone => {
                "Click corners, double-click or Enter to close · or drag a rectangle · the copper under the first corner sets the net"
            }
            Tool::Keepout => "Click corners, double-click or Enter to close · or drag a rectangle",
            Tool::Outline => {
                "Click corners, double-click or Enter to close · or drag a rectangle · replaces the board outline"
            }
            Tool::Measure => {
                "Click two points to measure · snaps to pad and via centres, or to the grid · click again to restart"
            }
        }
    }
}

/// How the route tool treats the copper of other nets, as in other editors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteMode {
    /// The segment goes straight. The tag shows the clearance errors.
    Direct,
    /// A segment that breaks the clearance goes around the copper.
    Walkaround,
    /// The segment goes straight. When the route is done, the tracks of other
    /// nets in its way are routed again around it. A pad or a via in the way
    /// makes the segment go around, as in walkaround.
    Push,
}

impl RouteMode {
    fn next(self) -> RouteMode {
        match self {
            RouteMode::Direct => RouteMode::Walkaround,
            RouteMode::Walkaround => RouteMode::Push,
            RouteMode::Push => RouteMode::Direct,
        }
    }

    fn name(self) -> &'static str {
        match self {
            RouteMode::Direct => "direct",
            RouteMode::Walkaround => "walkaround",
            RouteMode::Push => "push",
        }
    }
}

/// An action of the right-click menu.
#[derive(Clone)]
enum MenuAct {
    Rotate(f64),
    Flip,
    Lock,
    Move,
    Delete,
    SelectNet(String),
    HighlightNet(String),
    Via(String),
    Route,
    Fit,
    SelectAll,
}

pub enum Drag {
    /// Parts and the origin of the anchor part. The origin snaps to the grid.
    Parts(Vec<String>, Point),
    Via(String, Point),
    Segment(String, usize),
    Area(VertexOf),
    Vertex(VertexOf, usize),
}

pub enum Gesture {
    None,
    Box {
        from: Point,
    },
    Drag {
        from: Point,
        what: Drag,
    },
    /// `M`: the selection follows the pointer until a click.
    Grab {
        from: Point,
        refs: Vec<String>,
        anchor: Point,
    },
    /// Rectangle for the outline tools.
    Rect {
        from: Point,
    },
}

pub struct Placing {
    pub refdes: String,
    pub rotation: f64,
    pub side: Side,
}

/// A committed move. Its ghost stays until the reload shows the move.
pub struct Settling {
    pub what: Drag,
    pub delta: Point,
    /// The rotation of the parts, in degrees.
    pub turn: f64,
    pub to: Point,
    pub replied: bool,
}

pub struct Magnet {
    pub at: Point,
    pub net: String,
    pub layer: u8,
}

/// The usual track widths in mm.
const WIDTHS: [f64; 13] = [
    0.1, 0.127, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.5, 2.0,
];

/// The next track width up (or down) from `w` in the list of usual widths.
fn next_width(w: f64, up: bool) -> f64 {
    if up {
        WIDTHS.iter().copied().find(|x| *x > w + 1e-6).unwrap_or(w)
    } else {
        WIDTHS
            .iter()
            .rev()
            .copied()
            .find(|x| *x < w - 1e-6)
            .unwrap_or(w)
    }
}

/// True if the point is inside the closed ring (even-odd rule).
fn point_in_ring(p: Point, ring: &[Point]) -> bool {
    let mut inside = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// True if the segment from `a` to `b` touches the rectangle (Liang-Barsky clip).
fn segment_meets_rect(a: Point, b: Point, r: &Rect) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a.x - r.min.x),
        (dx, r.max.x - a.x),
        (-dy, a.y - r.min.y),
        (dy, r.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn rect_pts(a: Point, b: Point) -> Vec<Point> {
    vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)]
}

fn closest_on_seg(a: Point, b: Point, p: Point) -> Point {
    let d = b.sub(a);
    let l2 = d.x * d.x + d.y * d.y;
    if l2 < 1e-18 {
        return a;
    }
    let t = (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0);
    Point::new(a.x + d.x * t, a.y + d.y * t)
}

fn fmt_grid(g: f64) -> String {
    let s = format!("{g:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} mm")
}

impl App {
    pub(crate) fn grid(&self) -> f64 {
        edits::GRIDS[self.grid_i.min(edits::GRIDS.len() - 1)]
    }

    fn copper_count(&self) -> u8 {
        self.loaded
            .as_ref()
            .map(|l| l.data.board.stackup.copper_layers)
            .unwrap_or(2)
    }

    fn layer_of(&self, i: u8) -> Layer {
        Layer::Copper(Copper::from_index(i, self.copper_count()))
    }

    fn index_of(&self, l: Layer) -> u8 {
        l.copper()
            .map(|c| c.index(self.copper_count()))
            .unwrap_or(0)
    }

    fn layer_color(&self, i: u8) -> Color32 {
        rgba(lamina_scene::theme::layer_color(
            SceneLayer::Copper(i),
            self.copper_count(),
        ))
    }

    pub(crate) fn commit(
        &mut self,
        f: impl FnOnce(&mut ProjectData) -> edits::EditResult + Send + 'static,
    ) {
        if let Some(e) = self.editor.as_mut() {
            e.edit(f);
            self.own_edit = Some(Instant::now());
        }
    }

    pub(crate) fn say(&mut self, msg: impl Into<String>, ok: bool) {
        self.toast = Some((msg.into(), ok, Instant::now()));
    }

    /// Copper with a net under `p`. Pads and vias come before tracks. Snaps to
    /// pad and via centres and to track ends.
    pub(crate) fn magnet(&self, p: Point, layer: Option<u8>, net: Option<&str>) -> Option<Magnet> {
        let l = self.loaded.as_ref()?;
        let tol = 4.0 * self.mm_per_px;
        let probe = Prim::Circle { c: p, r: tol };
        let mut best: Option<(u8, Magnet)> = None;
        for it in &l.analysis.geometry.copper {
            let Some(n) = it.net.as_deref() else { continue };
            if net.is_some_and(|x| x != n)
                || layer.is_some_and(|x| x != it.layer)
                || !self.is_visible(SceneLayer::Copper(it.layer))
            {
                continue;
            }
            let rank = match it.owner {
                Owner::Pad { .. } | Owner::Hole { .. } => 0,
                Owner::Via { .. } => 1,
                Owner::Track { .. } => 2,
                Owner::Zone { .. } => continue,
            };
            if best.as_ref().is_some_and(|b| b.0 <= rank) {
                continue;
            }
            if it.prim.distance(&probe) > 0.0 {
                continue;
            }
            let at = match &it.prim {
                Prim::Capsule { a, b, r } if rank == 2 => {
                    let reach = (r * 1.6).max(tol * 2.0);
                    if a.dist(p) <= reach {
                        *a
                    } else if b.dist(p) <= reach {
                        *b
                    } else {
                        closest_on_seg(*a, *b, p)
                    }
                }
                prim => prim.anchor(),
            };
            best = Some((
                rank,
                Magnet {
                    at,
                    net: n.to_string(),
                    layer: it.layer,
                },
            ));
        }
        best.map(|b| b.1)
    }

    /// True if a track piece is nearer than the clearance to the copper of
    /// another net.
    fn conflicts(&self, a: Point, b: Point, layer: u8, net: &str, width: f64) -> bool {
        let (tracks, other) = self.blockers(a, b, layer, net, width);
        tracks || other
    }

    /// The copper of other nets that a segment is too close to: (tracks, the
    /// rest, such as pads and vias).
    fn blockers(&self, a: Point, b: Point, layer: u8, net: &str, width: f64) -> (bool, bool) {
        let mut found = (false, false);
        let Some(l) = &self.loaded else { return found };
        let d = &l.data;
        let class = d.nets.nets.get(net).and_then(|n| n.class.clone());
        let clr = d.board.rules.clearance_for(class.as_deref());
        let cap = Prim::Capsule {
            a,
            b,
            r: width / 2.0,
        };
        let bb = cap.bbox();
        for it in &l.analysis.geometry.copper {
            if it.layer != layer
                || it.net.as_deref() == Some(net)
                || matches!(it.owner, Owner::Zone { .. })
            {
                continue;
            }
            let ib = it.prim.bbox();
            if ib.min.x > bb.max.x + clr
                || ib.max.x < bb.min.x - clr
                || ib.min.y > bb.max.y + clr
                || ib.max.y < bb.min.y - clr
            {
                continue;
            }
            let theirs = it
                .net
                .as_deref()
                .and_then(|n| d.nets.nets.get(n))
                .and_then(|n| n.class.clone());
            let clr = clr.max(d.board.rules.clearance_for(theirs.as_deref()));
            if it.prim.distance(&cap) < clr - 1e-6 {
                if matches!(it.owner, Owner::Track { .. }) {
                    found.0 = true;
                } else {
                    found.1 = true;
                }
            }
        }
        found
    }

    pub(crate) fn sel_of(p: &Pick) -> Option<Sel> {
        Some(match p {
            Pick::Pad { part, .. } | Pick::Part { refdes: part } => Sel::Part(part.clone()),
            Pick::Track { id, seg, .. } => Sel::Track {
                id: id.clone(),
                seg: *seg,
            },
            Pick::Via { id, .. } => Sel::Via(id.clone()),
            Pick::Fill { zone, .. } => Sel::Zone(zone.clone()),
            Pick::Keepout { id } => Sel::Keepout(id.clone()),
            Pick::Hole { id } => Sel::Hole(id.clone()),
            Pick::Outline => Sel::Outline,
        })
    }

    /// The single selected item whose corners the user can drag.
    fn editable_outline(&self) -> Option<VertexOf> {
        if self.sel.len() != 1 {
            return None;
        }
        Some(match &self.sel[0] {
            Sel::Track { id, .. } => VertexOf::Track(id.clone()),
            Sel::Zone(id) => VertexOf::Zone(id.clone()),
            Sel::Keepout(id) => VertexOf::Keepout(id.clone()),
            Sel::Outline => VertexOf::Outline,
            _ => return None,
        })
    }

    fn handle_at(&self, p: Point) -> Option<(VertexOf, usize)> {
        let of = self.editable_outline()?;
        let d = &self.loaded.as_ref()?.data;
        let tol = 6.0 * self.mm_per_px;
        let vs = edits::vertices(d, &of);
        let (i, dist) = vs
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.dist(p)))
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        (dist <= tol).then_some((of, i))
    }

    fn selected_parts(&self) -> Vec<String> {
        self.sel
            .iter()
            .filter_map(|s| s.part().map(str::to_string))
            .collect()
    }

    /// The menu on a right click: the actions for the item under the pointer,
    /// with their shortcuts. A right drag still pans.
    fn context_menu(&mut self, resp: &egui::Response, p: Point) {
        if resp.secondary_clicked() {
            let pick = self.pick(p);
            let hit = pick.as_ref().and_then(App::sel_of);
            if let Some(h) = &hit {
                if !self.sel.iter().any(|s| s.same_item(h)) {
                    self.sel = vec![h.clone()];
                    self.dirty = true;
                }
            }
            self.menu = Some((p, pick));
        }
        let Some((at, pick)) = self.menu.clone() else {
            return;
        };
        let net = pick.as_ref().and_then(|k| k.net().map(str::to_string));
        let part = pick.as_ref().and_then(|k| k.part().map(str::to_string));
        let locked = part.as_ref().and_then(|r| {
            self.loaded
                .as_ref()
                .and_then(|l| l.data.placement.placements.get(r))
                .map(|pl| pl.locked)
        });
        let on_pad = matches!(pick, Some(Pick::Pad { .. }));
        let mut act: Option<MenuAct> = None;
        let shown = resp.context_menu(|ui| {
            // A compact menu, as in other editors: it must not cover the board.
            ui.set_min_width(210.0);
            ui.set_max_width(270.0);
            let mut item = |ui: &mut egui::Ui, label: &str, key: &str, a: MenuAct| {
                let r = ui.add(widgets::menu_row(label, key));
                if r.clicked() {
                    act = Some(a);
                    ui.close();
                }
            };
            if let Some(r) = &part {
                ui.label(egui::RichText::new(r).font(semibold(12.5)).color(TEXT));
                item(ui, "Rotate 90°", "R", MenuAct::Rotate(90.0));
                item(ui, "Rotate -90°", "Shift+R", MenuAct::Rotate(-90.0));
                item(ui, "Flip to the other side", "F", MenuAct::Flip);
                item(ui, "Move with the pointer", "M", MenuAct::Move);
                let lock = if locked == Some(true) {
                    "Unlock"
                } else {
                    "Lock"
                };
                item(ui, lock, "L", MenuAct::Lock);
                item(ui, "Remove from the board", "Del", MenuAct::Delete);
                ui.separator();
            }
            if on_pad {
                item(ui, "Route from this pad", "X", MenuAct::Route);
            }
            if let Some(n) = &net {
                item(
                    ui,
                    &format!("Highlight the net {n}"),
                    "`",
                    MenuAct::HighlightNet(n.clone()),
                );
                item(
                    ui,
                    &format!("Select the net {n}"),
                    "",
                    MenuAct::SelectNet(n.clone()),
                );
                if matches!(pick, Some(Pick::Track { .. })) {
                    item(ui, "Add a via here", "V", MenuAct::Via(n.clone()));
                }
            }
            if part.is_none()
                && matches!(
                    pick,
                    Some(Pick::Track { .. } | Pick::Via { .. } | Pick::Fill { .. })
                )
            {
                item(ui, "Delete", "Del", MenuAct::Delete);
            }
            if pick.is_none() {
                item(ui, "Fit the board", "Home", MenuAct::Fit);
                item(ui, "Select all parts", "Ctrl+A", MenuAct::SelectAll);
            }
        });
        if shown.is_none() {
            self.menu = None;
        }
        let Some(a) = act else { return };
        self.menu = None;
        match a {
            MenuAct::Rotate(by) => {
                let refs = self.selected_parts();
                if !refs.is_empty() {
                    self.commit(move |d| edits::rotate_parts(d, &refs, by));
                }
            }
            MenuAct::Flip => {
                let refs = self.selected_parts();
                if !refs.is_empty() {
                    self.commit(move |d| edits::flip_parts(d, &refs));
                }
            }
            MenuAct::Lock => {
                let refs = self.selected_parts();
                let lock = locked != Some(true);
                if !refs.is_empty() {
                    self.commit(move |d| edits::set_locked(d, &refs, lock));
                }
            }
            MenuAct::Move => {
                let refs = self.selected_parts();
                if let (Some(first), Some(l)) = (refs.first(), &self.loaded) {
                    if let Some(pl) = l.data.placement.placements.get(first) {
                        self.turn = 0.0;
                        self.gesture = Gesture::Grab {
                            from: at,
                            anchor: pl.origin(),
                            refs,
                        };
                    }
                }
            }
            MenuAct::Delete => {
                if !self.sel.is_empty() {
                    let sels = std::mem::take(&mut self.sel);
                    self.commit(move |d| edits::delete(d, &sels));
                }
            }
            MenuAct::SelectNet(n) => {
                if let Some(l) = &self.loaded {
                    let d = &l.data;
                    let mut sel = Vec::new();
                    for t in d.routing.tracks.iter().filter(|t| t.net == n) {
                        for seg in 0..t.points.len().saturating_sub(1) {
                            sel.push(Sel::Track {
                                id: t.id.clone(),
                                seg,
                            });
                        }
                    }
                    for v in d.routing.vias.iter().filter(|v| v.net == n) {
                        sel.push(Sel::Via(v.id.clone()));
                    }
                    self.sel = sel;
                    self.dirty = true;
                }
            }
            MenuAct::HighlightNet(n) => self.highlight_net(Some(n)),
            MenuAct::Via(n) => {
                let ps = snap_pt(at, self.grid());
                self.commit(move |d| edits::add_via(d, &n, ps));
            }
            MenuAct::Route => {
                self.tool = Tool::Route;
                self.sel.clear();
                self.start_route(at);
            }
            MenuAct::Fit => self.fit_pending = true,
            MenuAct::SelectAll => {
                if let Some(l) = &self.loaded {
                    self.sel = l
                        .data
                        .placement
                        .placements
                        .keys()
                        .map(|r| Sel::Part(r.clone()))
                        .collect();
                    self.dirty = true;
                }
            }
        }
    }

    fn select(&mut self, hit: Option<Sel>, shift: bool) {
        match hit {
            Some(h) if shift => {
                if let Some(i) = self.sel.iter().position(|s| s.same_item(&h)) {
                    self.sel.remove(i);
                } else {
                    self.sel.push(h);
                }
            }
            Some(h) => self.sel = vec![h],
            None if !shift => self.sel.clear(),
            None => {}
        }
        self.dirty = true;
    }

    fn box_select(&mut self, a: Point, b: Point, shift: bool) {
        let Some(l) = &self.loaded else { return };
        let d = &l.data;
        let r = Rect {
            min: Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: Point::new(a.x.max(b.x), a.y.max(b.y)),
        };
        // Left to right: the item must be fully inside. Right to left: it must touch the box.
        let enclose = b.x >= a.x;
        let inside =
            |p: Point| p.x >= r.min.x && p.x <= r.max.x && p.y >= r.min.y && p.y <= r.max.y;
        let take = |pts: &[Point]| {
            if enclose {
                !pts.is_empty() && pts.iter().all(|p| inside(*p))
            } else {
                pts.iter().any(|p| inside(*p))
            }
        };
        let mut out: Vec<Sel> = if shift { self.sel.clone() } else { Vec::new() };
        // Each segment of a track is its own item here, so compare the segment too.
        let mut add = |s: Sel| {
            if !out.contains(&s) {
                out.push(s);
            }
        };
        for (refdes, pl) in &d.placement.placements {
            let Some(fp) = d
                .parts
                .parts
                .get(refdes)
                .and_then(|p| d.footprints.get(&p.footprint))
            else {
                continue;
            };
            if let Some(c) = lamina_geom::courtyard(fp, pl) {
                // A crossing box takes a part whose courtyard it touches: a
                // corner in the box, an edge across the box, or the box in the part.
                let hit = if enclose {
                    take(&c)
                } else {
                    take(&c)
                        || (0..c.len()).any(|i| segment_meets_rect(c[i], c[(i + 1) % c.len()], &r))
                        || point_in_ring(
                            Point::new((r.min.x + r.max.x) / 2.0, (r.min.y + r.max.y) / 2.0),
                            &c,
                        )
                };
                if hit {
                    add(Sel::Part(refdes.clone()));
                }
            }
        }
        for v in &d.routing.vias {
            if inside(v.at) {
                add(Sel::Via(v.id.clone()));
            }
        }
        // Tracks go by segment, as in other editors: a window takes the
        // segments inside it, and a crossing box takes each segment that it
        // touches, also if both ends are outside the box.
        for t in &d.routing.tracks {
            for (seg, w) in t.points.windows(2).enumerate() {
                let hit = if enclose {
                    inside(w[0]) && inside(w[1])
                } else {
                    segment_meets_rect(w[0], w[1], &r)
                };
                if hit {
                    add(Sel::Track {
                        id: t.id.clone(),
                        seg,
                    });
                }
            }
        }
        if enclose {
            for z in &d.routing.zones {
                if take(&z.outline.flatten(0.05)) {
                    add(Sel::Zone(z.id.clone()));
                }
            }
            for k in &d.routing.keepouts {
                if take(&k.outline.flatten(0.05)) {
                    add(Sel::Keepout(k.id.clone()));
                }
            }
        }
        self.sel = out;
        self.dirty = true;
    }

    /// The delta for a drag from `from` to `p`, snapped to the grid.
    fn drag_delta(&self, what: &Drag, from: Point, p: Point) -> (Point, Point) {
        let g = self.grid();
        let raw = p.sub(from);
        match what {
            Drag::Parts(_, anchor) | Drag::Via(_, anchor) => {
                let to = snap_pt(anchor.add(raw), g);
                (to.sub(*anchor), to)
            }
            Drag::Vertex(..) => {
                let to = snap_pt(p, g);
                (raw, to)
            }
            Drag::Segment(..) | Drag::Area(..) => {
                let d = snap_pt(raw, g);
                (d, p)
            }
        }
    }

    fn commit_drag(&mut self, what: Drag, delta: Point, to: Point) {
        let turn = std::mem::take(&mut self.turn);
        let turned = matches!(what, Drag::Parts(..)) && turn != 0.0;
        if delta.x.abs() < 1e-9
            && delta.y.abs() < 1e-9
            && !turned
            && !matches!(what, Drag::Vertex(..))
        {
            return;
        }
        match &what {
            Drag::Parts(refs, anchor) => {
                let refs = refs.clone();
                // The held part is the centre of the turn.
                let held = anchor.add(delta);
                self.commit(move |d| {
                    let moved = edits::move_parts(d, &refs, delta.x, delta.y)?;
                    if turn == 0.0 {
                        return Ok(moved);
                    }
                    let turned = edits::rotate_parts_about(d, &refs, turn, held)?;
                    Ok(format!("{moved}. {turned}"))
                });
            }
            Drag::Via(id, _) => {
                let id = id.clone();
                self.commit(move |d| edits::move_via(d, &id, delta));
            }
            Drag::Segment(id, seg) => {
                let (id, seg) = (id.clone(), *seg);
                self.commit(move |d| edits::move_segment(d, &id, seg, delta));
            }
            Drag::Area(of) => {
                let of = of.clone();
                self.commit(move |d| edits::move_area(d, &of, delta));
            }
            Drag::Vertex(of, i) => {
                let (of, i) = (of.clone(), *i);
                self.commit(move |d| edits::move_vertex(d, &of, i, to));
            }
        }
        self.settling = Some(Settling {
            what,
            delta,
            turn: if turned { turn } else { 0.0 },
            to,
            replied: false,
        });
    }

    // ------------------------------------------------------------ input

    pub(crate) fn edit_input(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        xf: &Xf,
        size_px: Vec2,
    ) {
        let (shift, origin) = ui.input(|i| {
            (
                i.modifiers.shift || i.modifiers.command,
                i.pointer.press_origin(),
            )
        });
        let origin = origin.map(|s| xf.to_board(s));
        self.keys(ui, size_px);
        let Some(p) = self.cursor_mm.or(self.last_cursor) else {
            return;
        };
        self.last_cursor = Some(p);
        let ps = snap_pt(p, self.grid());

        if self.placing.is_some() {
            if resp.clicked() {
                let pl = self.placing.take().unwrap();
                let placement = Placement {
                    x: ps.x,
                    y: ps.y,
                    rotation: pl.rotation,
                    side: pl.side,
                    locked: false,
                };
                let r = pl.refdes.clone();
                self.commit(move |d| edits::set_placement(d, &r, placement));
                self.sel = vec![Sel::Part(pl.refdes)];
            }
            return;
        }

        match self.tool {
            Tool::Select => {
                self.select_input(resp, p, origin, shift);
                self.context_menu(resp, p);
            }
            Tool::Route => self.route_input(resp, p, ps),
            Tool::Via => {
                if resp.clicked() {
                    match self.magnet(p, None, None) {
                        Some(m) => {
                            let net = m.net.clone();
                            self.commit(move |d| edits::add_via(d, &net, ps));
                        }
                        None => self.say("Click on copper that belongs to a net", false),
                    }
                }
            }
            Tool::Zone | Tool::Keepout | Tool::Outline => self.poly_input(resp, p, ps, origin),
            Tool::Measure => self.measure_input(resp, p),
        }
    }

    fn select_input(
        &mut self,
        resp: &egui::Response,
        p: Point,
        origin: Option<Point>,
        shift: bool,
    ) {
        if let Gesture::Grab { .. } = self.gesture {
            if resp.clicked() {
                if let Gesture::Grab { from, refs, anchor } =
                    std::mem::replace(&mut self.gesture, Gesture::None)
                {
                    let what = Drag::Parts(refs, anchor);
                    let (delta, to) = self.drag_delta(&what, from, p);
                    self.commit_drag(what, delta, to);
                }
            }
            return;
        }
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let from = origin.unwrap_or(p);
            if let Some((of, i)) = self.handle_at(from) {
                self.gesture = Gesture::Drag {
                    from,
                    what: Drag::Vertex(of, i),
                };
            } else if let Some(hit) = self
                .pick(from)
                .as_ref()
                .and_then(App::sel_of)
                // A pour or a keep-out often covers the board. A drag on it
                // selects with a box, as on an empty board, unless the area
                // is already selected: then the drag moves it.
                .filter(|hit| {
                    !matches!(hit, Sel::Zone(_) | Sel::Keepout(_))
                        || self.sel.iter().any(|s| s.same_item(hit))
                })
            {
                if !self.sel.iter().any(|s| s.same_item(&hit)) {
                    self.select(Some(hit.clone()), shift);
                }
                let d = &self.loaded.as_ref().unwrap().data;
                let what = match &hit {
                    Sel::Part(r) => d.placement.placements.get(r).map(|pl| {
                        let mut refs = self.selected_parts();
                        if !refs.contains(r) {
                            refs.push(r.clone());
                        }
                        Drag::Parts(refs, pl.origin())
                    }),
                    Sel::Via(id) => d
                        .routing
                        .vias
                        .iter()
                        .find(|v| &v.id == id)
                        .map(|v| Drag::Via(id.clone(), v.at)),
                    Sel::Track { id, seg } => Some(Drag::Segment(id.clone(), *seg)),
                    Sel::Zone(id) => Some(Drag::Area(VertexOf::Zone(id.clone()))),
                    Sel::Keepout(id) => Some(Drag::Area(VertexOf::Keepout(id.clone()))),
                    Sel::Hole(_) | Sel::Outline => None,
                };
                self.gesture = match what {
                    Some(what) => {
                        self.turn = 0.0;
                        Gesture::Drag { from, what }
                    }
                    None => Gesture::Box { from },
                };
            } else {
                self.gesture = Gesture::Box { from };
            }
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            match std::mem::replace(&mut self.gesture, Gesture::None) {
                Gesture::Drag { from, what } => {
                    let (delta, to) = self.drag_delta(&what, from, p);
                    self.commit_drag(what, delta, to);
                }
                Gesture::Box { from } => self.box_select(from, p, shift),
                _ => {}
            }
        }
        if resp.clicked() {
            let hit = self.pick(p).as_ref().and_then(App::sel_of);
            self.select(hit, shift);
            self.note_at = Some(p);
        }
        if resp.double_clicked() && self.pick(p).is_none() {
            self.fit_pending = true;
        }
    }

    /// Start a route on the copper under `p`. False if there is none.
    fn start_route(&mut self, p: Point) -> bool {
        let m = self
            .magnet(p, Some(self.active), None)
            .or_else(|| self.magnet(p, None, None));
        let Some(m) = m else {
            self.say("Start a route on a pad, track or via", false);
            return false;
        };
        let d = &self.loaded.as_ref().unwrap().data;
        let class = d.nets.nets.get(&m.net).and_then(|n| n.class.clone());
        let width = d.board.rules.track_width_for(class.as_deref());
        self.active = m.layer;
        self.route = Some(Route {
            net: m.net,
            width,
            runs: vec![(self.layer_of(m.layer), vec![m.at])],
            vias: Vec::new(),
        });
        self.dirty = true;
        true
    }

    fn route_input(&mut self, resp: &egui::Response, p: Point, ps: Point) {
        let Some(r) = &self.route else {
            if resp.clicked() {
                self.start_route(p);
            }
            return;
        };
        let li = self.index_of(r.runs.last().unwrap().0);
        let hit = self.magnet(p, Some(li), Some(&r.net));
        let target = hit.as_ref().map(|m| m.at).unwrap_or(ps);
        if resp.clicked() || resp.double_clicked() {
            let diag = self.diagonal_first;
            let start = r.runs[0].1[0];
            let last = *r.runs.last().unwrap().1.last().unwrap();
            let mut pts = posture(last, target, diag);
            // Walkaround, as in other editors: if the straight segments break
            // the clearance, find a path around the copper of other nets.
            let mut prev = last;
            let (mut on_tracks, mut on_other) = (false, false);
            for &p in &pts {
                let (t, o) = self.blockers(prev, p, li, &r.net, r.width);
                on_tracks |= t;
                on_other |= o;
                prev = p;
            }
            let walk = target.dist(last) > 1e-6
                && match self.route_mode {
                    RouteMode::Direct => false,
                    RouteMode::Walkaround => on_tracks || on_other,
                    // Tracks move aside when the route is done; pads and vias do not.
                    RouteMode::Push => on_other,
                };
            if walk {
                let walk = lamina_route::Walk {
                    net: r.net.clone(),
                    layer: r.runs.last().unwrap().0,
                    width: r.width,
                    from: last,
                    to: target,
                    grid: (self.grid() / 2.0).clamp(0.05, 0.25),
                };
                match self
                    .loaded
                    .as_ref()
                    .and_then(|l| lamina_route::walkaround(&l.data, &walk))
                {
                    // A last check with exact geometry: the search is on a grid.
                    // The first segment can start in a band where the user put
                    // the last corner, so it is not checked.
                    Some(path)
                        if path
                            .windows(2)
                            .skip(1)
                            .all(|s| !self.conflicts(s[0], s[1], li, &r.net, r.width)) =>
                    {
                        pts = path.into_iter().skip(1).collect()
                    }
                    Some(_) => {
                        self.say(
                            "The path around the copper is too close at one place. Add a corner nearer to the gap.",
                            false,
                        );
                        self.dirty = true;
                        return;
                    }
                    None => {
                        self.say(
                            "There is no clear path to here on this layer. Add a corner, or V for a via.",
                            false,
                        );
                        self.dirty = true;
                        return;
                    }
                }
            }
            let r = self.route.as_mut().unwrap();
            if target.dist(last) > 1e-9 {
                r.runs.last_mut().unwrap().1.extend(pts);
            }
            let moved = r.runs.iter().map(|x| x.1.len()).sum::<usize>() > 1;
            if resp.double_clicked() || (hit.is_some() && moved && target.dist(start) > 1e-6) {
                self.finish_route();
            }
            self.dirty = true;
        }
    }

    fn route_via(&mut self, to_layer: Option<u8>) {
        let n = self.copper_count();
        let Some(p) = self.cursor_mm else { return };
        let ps = snap_pt(p, self.grid());
        let diag = self.diagonal_first;
        let Some(r) = &self.route else { return };
        let li = self.index_of(r.runs.last().unwrap().0);
        let target = self
            .magnet(p, Some(li), Some(&r.net))
            .map(|m| m.at)
            .unwrap_or(ps);
        let next = to_layer.unwrap_or(if li == 0 { n - 1 } else { 0 });
        if next == li {
            return;
        }
        let new_layer = self.layer_of(next);
        let r = self.route.as_mut().unwrap();
        let last = *r.runs.last().unwrap().1.last().unwrap();
        if target.dist(last) > 1e-9 {
            r.runs
                .last_mut()
                .unwrap()
                .1
                .extend(posture(last, target, diag));
        }
        r.vias.push(target);
        r.runs.push((new_layer, vec![target]));
        self.active = next;
        self.dirty = true;
    }

    fn finish_route(&mut self) {
        if let Some(r) = self.route.take() {
            if r.runs.iter().any(|x| x.1.len() > 1) || !r.vias.is_empty() {
                if self.route_mode == RouteMode::Push {
                    self.commit(move |d| {
                        let before: Vec<String> =
                            d.routing.tracks.iter().map(|t| t.id.clone()).collect();
                        let before_vias: Vec<String> =
                            d.routing.vias.iter().map(|v| v.id.clone()).collect();
                        let added = edits::add_route(d, &r)?;
                        let new: Vec<String> = d
                            .routing
                            .tracks
                            .iter()
                            .map(|t| t.id.clone())
                            .filter(|id| !before.contains(id))
                            .collect();
                        let new_vias: Vec<String> = d
                            .routing
                            .vias
                            .iter()
                            .map(|v| v.id.clone())
                            .filter(|id| !before_vias.contains(id))
                            .collect();
                        let pushed = edits::push_aside(d, &r.net, &new, &new_vias)?;
                        Ok(if pushed.is_empty() {
                            added
                        } else {
                            format!("{added}. {pushed}")
                        })
                    });
                } else {
                    self.commit(move |d| edits::add_route(d, &r));
                }
            }
        }
    }

    fn poly_input(&mut self, resp: &egui::Response, p: Point, ps: Point, origin: Option<Point>) {
        let g = self.grid();
        if resp.drag_started_by(egui::PointerButton::Primary) && self.poly.is_empty() {
            self.gesture = Gesture::Rect {
                from: snap_pt(origin.unwrap_or(p), g),
            };
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            if let Gesture::Rect { from } = std::mem::replace(&mut self.gesture, Gesture::None) {
                self.finish_poly(rect_pts(from, ps));
            }
        }
        if resp.clicked() {
            let close = self.poly.len() >= 3 && self.poly[0].dist(ps) <= 6.0 * self.mm_per_px;
            if close || (resp.double_clicked() && self.poly.len() >= 3) {
                let pts = std::mem::take(&mut self.poly);
                self.finish_poly(pts);
            } else if self.poly.last().is_none_or(|l| l.dist(ps) > 1e-9) {
                self.poly.push(ps);
            }
            self.dirty = true;
        }
    }

    fn finish_poly(&mut self, pts: Vec<Point>) {
        self.poly.clear();
        let n = self.copper_count();
        match self.tool {
            Tool::Zone => {
                let layer = self.layer_of(self.active);
                let net = self
                    .magnet(pts[0], Some(self.active), None)
                    .map(|m| m.net)
                    .or_else(|| {
                        let d = &self.loaded.as_ref()?.data;
                        d.nets.nets.contains_key("GND").then(|| "GND".to_string())
                    });
                self.commit(move |d| edits::add_zone(d, net, layer, &pts));
            }
            Tool::Keepout => {
                let layers = Layer::all_copper(n);
                self.commit(move |d| edits::add_keepout(d, layers, &pts));
            }
            Tool::Outline => self.commit(move |d| edits::set_outline(d, &pts)),
            _ => {}
        }
        self.tool = Tool::Select;
    }

    fn keys(&mut self, ui: &egui::Ui, size_px: Vec2) {
        // If several boards are open, only the focused board gets shortcuts.
        if !self.focused || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let k = |key: Key| ui.input(|i| i.key_pressed(key));
        let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));

        if cmd {
            // Undo, redo and find work in both views: see `global_keys`.
            if k(Key::A) {
                if let Some(l) = &self.loaded {
                    self.sel = l
                        .data
                        .placement
                        .placements
                        .keys()
                        .map(|r| Sel::Part(r.clone()))
                        .collect();
                    self.dirty = true;
                }
            }
            return;
        }
        if self.help || self.find.is_some() {
            return;
        }
        let backtick = ui.input(|i| {
            i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key, physical_key, pressed: true, .. }
                    if *key == Key::Backtick || *physical_key == Some(Key::Backtick))
            })
        });
        if backtick {
            let net = self
                .hover
                .as_ref()
                .and_then(|p| p.net())
                .map(str::to_string);
            let same = self.hl.as_ref().map(|h| &h.net) == net.as_ref();
            self.highlight_net(if same { None } else { net });
        }
        if k(Key::Escape) {
            self.turn = 0.0;
            // Esc cancels the work in progress first, then the highlight, then
            // the tool, then the selection.
            if self.measure.a.is_some() {
                self.measure = Default::default();
            } else if self.placing.take().is_some()
                || self.route.take().is_some()
                || !std::mem::take(&mut self.poly).is_empty()
                || !matches!(
                    std::mem::replace(&mut self.gesture, Gesture::None),
                    Gesture::None
                )
            {
            } else if self.hl.is_some() {
                self.highlight_net(None);
            } else if self.tool != Tool::Select {
                self.tool = Tool::Select;
            } else {
                self.sel.clear();
            }
            self.dirty = true;
        }
        if shift && k(Key::S) {
            self.single = !self.single;
            self.dirty = true;
        }
        for (tool, key) in [
            (Tool::Select, Key::S),
            (Tool::Route, Key::X),
            (Tool::Zone, Key::Z),
            (Tool::Keepout, Key::K),
            (Tool::Outline, Key::O),
            (Tool::Measure, Key::D),
        ] {
            if k(key) && !(shift && key == Key::S) {
                self.set_tool(tool);
            }
        }
        if k(Key::V) {
            if self.route.is_some() {
                self.route_via(None);
            } else {
                self.set_tool(Tool::Via);
            }
        }
        if k(Key::Enter) {
            if self.route.is_some() {
                self.finish_route();
            } else if self.poly.len() >= 3 {
                let pts = std::mem::take(&mut self.poly);
                self.finish_poly(pts);
            }
        }
        if k(Key::Slash) {
            self.diagonal_first = !self.diagonal_first;
        }
        // W and Shift+W: the next or the last track width in the list, as in
        // other editors. The width is for all of the route in progress.
        if k(Key::W) {
            if let Some(r) = self.route.as_mut() {
                r.width = next_width(r.width, !shift);
                self.dirty = true;
            }
        }
        let n = self.copper_count();
        for (key, to) in [(Key::PageUp, 0u8), (Key::PageDown, n - 1)] {
            if k(key) {
                if self.route.is_some() {
                    self.route_via(Some(to));
                } else {
                    self.active = to;
                    self.dirty = true;
                }
            }
        }
        if k(Key::N) {
            let len = edits::GRIDS.len();
            self.grid_i = if shift {
                (self.grid_i + len - 1) % len
            } else {
                (self.grid_i + 1) % len
            };
        }
        if k(Key::Backspace) || k(Key::Delete) {
            if let Some(r) = self.route.as_mut() {
                let run = &mut r.runs.last_mut().unwrap().1;
                if run.len() > 1 {
                    run.pop();
                } else if r.runs.len() > 1 {
                    r.runs.pop();
                    r.vias.pop();
                    let l = r.runs.last().unwrap().0;
                    self.active = self.index_of(l);
                } else {
                    self.route = None;
                }
            } else if !self.poly.is_empty() {
                self.poly.pop();
            } else if !self.sel.is_empty() {
                let sels = std::mem::take(&mut self.sel);
                self.commit(move |d| edits::delete(d, &sels));
            }
            self.dirty = true;
        }
        if k(Key::Home) {
            self.fit_2d(size_px);
        }
        if shift && k(Key::R) && self.tool == Tool::Route {
            self.route_mode = self.route_mode.next();
            self.say(
                match self.route_mode {
                    RouteMode::Direct => {
                        "Direct routing: a segment goes straight, and shows the clearance errors"
                    }
                    RouteMode::Walkaround => {
                        "Walkaround routing: a segment goes around the copper of other nets"
                    }
                    RouteMode::Push => {
                        "Push routing: the tracks of other nets in the way move aside when the route is done"
                    }
                },
                true,
            );
            self.dirty = true;
        } else if k(Key::R) && self.route.is_none() {
            let by = if shift { -90.0 } else { 90.0 };
            let dragging_parts = matches!(
                self.gesture,
                Gesture::Drag {
                    what: Drag::Parts(..),
                    ..
                } | Gesture::Grab { .. }
            );
            if dragging_parts {
                // As in other editors: R turns the parts that move with the pointer.
                self.turn = (self.turn + by).rem_euclid(360.0);
                self.dirty = true;
            } else if let Some(pl) = self.placing.as_mut() {
                pl.rotation = (pl.rotation + by).rem_euclid(360.0);
            } else {
                let refs = self.selected_parts();
                if !refs.is_empty() {
                    self.commit(move |d| edits::rotate_parts(d, &refs, by));
                }
            }
        }
        if k(Key::F) {
            if let Some(pl) = self.placing.as_mut() {
                pl.side = pl.side.flip();
            } else {
                let refs = self.selected_parts();
                if refs.is_empty() {
                    self.fit_2d(size_px);
                } else {
                    self.commit(move |d| edits::flip_parts(d, &refs));
                }
            }
        }
        if k(Key::L) {
            let refs = self.selected_parts();
            if let (false, Some(l)) = (refs.is_empty(), &self.loaded) {
                let lock = !refs
                    .iter()
                    .all(|r| l.data.placement.placements.get(r).is_some_and(|p| p.locked));
                self.commit(move |d| edits::set_locked(d, &refs, lock));
            }
        }
        if k(Key::M) {
            let refs = self.selected_parts();
            if let (Some(first), Some(p), Some(l)) = (refs.first(), self.cursor_mm, &self.loaded) {
                if let Some(pl) = l.data.placement.placements.get(first) {
                    self.turn = 0.0;
                    self.gesture = Gesture::Grab {
                        from: p,
                        anchor: pl.origin(),
                        refs,
                    };
                }
            }
        }
        let g = self.grid();
        for (key, dx, dy) in [
            (Key::ArrowLeft, -g, 0.0),
            (Key::ArrowRight, g, 0.0),
            (Key::ArrowUp, 0.0, g),
            (Key::ArrowDown, 0.0, -g),
        ] {
            if k(key) {
                let refs = self.selected_parts();
                if !refs.is_empty() {
                    self.commit(move |d| edits::move_parts(d, &refs, dx, dy));
                }
            }
        }
    }

    /// Shortcuts that work in 2D and 3D alike.
    pub(crate) fn global_keys(&mut self, ui: &egui::Ui) {
        if !self.focused || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let k = |key: Key| ui.input(|i| i.key_pressed(key));
        let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        if k(Key::F1) {
            self.help = !self.help;
        }
        if !cmd {
            return;
        }
        if k(Key::Z) && !shift {
            self.undo();
        } else if k(Key::Y) || (k(Key::Z) && shift) {
            self.redo();
        } else if k(Key::F) {
            self.find = Some(crate::actions::Find {
                just_opened: true,
                ..Default::default()
            });
        }
    }

    pub(crate) fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.turn = 0.0;
        self.route = None;
        self.poly.clear();
        self.placing = None;
        self.gesture = Gesture::None;
        self.dirty = true;
    }

    pub(crate) fn undo(&mut self) {
        if let Some(e) = self.editor.as_mut() {
            e.undo();
            self.own_edit = Some(Instant::now());
        }
    }

    pub(crate) fn redo(&mut self) {
        if let Some(e) = self.editor.as_mut() {
            e.redo();
            self.own_edit = Some(Instant::now());
        }
    }

    // ------------------------------------------------------------ drawing

    fn ghost_part(
        &self,
        painter: &egui::Painter,
        xf: &Xf,
        refdes: &str,
        pl: &Placement,
        color: Color32,
    ) {
        let Some(l) = &self.loaded else { return };
        let d = &l.data;
        let Some(fp) = d
            .parts
            .parts
            .get(refdes)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            return;
        };
        if let Some(c) = lamina_geom::courtyard(fp, pl) {
            let pts: Vec<Pos2> = c.into_iter().map(|p| xf.to_screen(p)).collect();
            painter.add(egui::Shape::convex_polygon(
                pts.clone(),
                color.gamma_multiply(0.10),
                Stroke::NONE,
            ));
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, color)));
        }
        for (_, _, prim, _) in lamina_geom::placed_pads(fp, pl, d.board.stackup.copper_layers) {
            let pts: Vec<Pos2> = prim
                .outline(0.02)
                .into_iter()
                .map(|p| xf.to_screen(p))
                .collect();
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.2, color)));
        }
        painter.text(
            xf.to_screen(pl.origin()),
            egui::Align2::CENTER_CENTER,
            refdes,
            semibold(12.0),
            color,
        );
    }

    fn ghost(
        &self,
        painter: &egui::Painter,
        xf: &Xf,
        what: &Drag,
        delta: Point,
        to: Point,
        turn: f64,
    ) {
        let Some(l) = &self.loaded else { return };
        let d = &l.data;
        let c = ACCENT;
        match what {
            Drag::Parts(refs, anchor) => {
                // As the commit: turn about the held part.
                let moved: Vec<(&String, Placement)> = refs
                    .iter()
                    .filter_map(|r| d.placement.placements.get(r).map(|pl| (r, *pl)))
                    .map(|(r, mut pl)| {
                        pl.x += delta.x;
                        pl.y += delta.y;
                        (r, pl)
                    })
                    .collect();
                let centre = anchor.add(delta);
                for (r, mut pl) in moved {
                    if turn != 0.0 {
                        let p = pl.origin().sub(centre).rotated(turn).add(centre);
                        pl.x = p.x;
                        pl.y = p.y;
                        pl.rotation = (pl.rotation + turn).rem_euclid(360.0);
                    }
                    self.ghost_part(painter, xf, r, &pl, c);
                }
            }
            Drag::Via(id, _) => {
                if let Some(v) = d.routing.vias.iter().find(|v| &v.id == id) {
                    let s = xf.to_screen(v.at.add(delta));
                    let r = (v.diameter / 2.0 * xf.ppm_pt) as f32;
                    painter.circle_stroke(s, r.max(3.0), Stroke::new(1.5, c));
                }
            }
            Drag::Segment(id, seg) => {
                if let Some(t) = d.routing.tracks.iter().find(|t| &t.id == id) {
                    let mut pts = t.points.clone();
                    for k in [*seg, seg + 1] {
                        if let Some(p) = pts.get_mut(k) {
                            *p = p.add(delta);
                        }
                    }
                    self.track_line(painter, xf, &pts, t.width, c.gamma_multiply(0.7));
                }
            }
            Drag::Area(of) => {
                let pts: Vec<Pos2> = edits::vertices(d, of)
                    .into_iter()
                    .map(|p| xf.to_screen(p.add(delta)))
                    .collect();
                painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, c)));
            }
            Drag::Vertex(of, i) => {
                let mut vs = edits::vertices(d, of);
                if let Some(v) = vs.get_mut(*i) {
                    *v = to;
                }
                let pts: Vec<Pos2> = vs.into_iter().map(|p| xf.to_screen(p)).collect();
                if matches!(of, VertexOf::Track(_)) {
                    painter.add(egui::Shape::line(pts, Stroke::new(1.5, c)));
                } else {
                    painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, c)));
                }
            }
        }
    }

    fn track_line(
        &self,
        painter: &egui::Painter,
        xf: &Xf,
        pts: &[Point],
        width: f64,
        color: Color32,
    ) {
        let w = ((width * xf.ppm_pt) as f32).max(1.5);
        for s in pts.windows(2) {
            painter.line_segment(
                [xf.to_screen(s[0]), xf.to_screen(s[1])],
                Stroke::new(w, color),
            );
        }
        for p in pts {
            painter.circle_filled(xf.to_screen(*p), w / 2.0, color);
        }
    }

    pub(crate) fn edit_overlays(&self, painter: &egui::Painter, xf: &Xf) {
        let Some(l) = &self.loaded else { return };
        let d = &l.data;
        self.grid_dots(painter, xf);

        // Selection.
        for s in &self.sel {
            match s {
                Sel::Part(r) => {
                    if let Some(pl) = d.placement.placements.get(r) {
                        self.ghost_part(painter, xf, r, pl, ACCENT);
                    }
                }
                Sel::Track { id, .. } => {
                    for it in &l.analysis.geometry.copper {
                        if matches!(&it.owner, Owner::Track { id: t, .. } if t == id) {
                            draw_prim(painter, xf, &it.prim, ACCENT, true);
                        }
                    }
                }
                Sel::Via(id) | Sel::Hole(id) => {
                    for it in &l.analysis.geometry.copper {
                        let hit = matches!(&it.owner, Owner::Via { id: v } | Owner::Hole { id: v } if v == id);
                        if hit {
                            draw_prim(painter, xf, &it.prim, ACCENT, false);
                        }
                    }
                    if let Some(h) = d.board.holes.iter().find(|h| &h.id == id) {
                        let r = (h.drill.min_size() / 2.0 * xf.ppm_pt) as f32;
                        painter.circle_stroke(
                            xf.to_screen(h.at),
                            r.max(3.0),
                            Stroke::new(1.5, ACCENT),
                        );
                    }
                }
                Sel::Zone(_) | Sel::Keepout(_) | Sel::Outline => {
                    let of = match s {
                        Sel::Zone(id) => VertexOf::Zone(id.clone()),
                        Sel::Keepout(id) => VertexOf::Keepout(id.clone()),
                        _ => VertexOf::Outline,
                    };
                    let pts: Vec<Pos2> = match &of {
                        VertexOf::Zone(id) => d
                            .routing
                            .zones
                            .iter()
                            .find(|z| &z.id == id)
                            .map(|z| z.outline.flatten(0.02)),
                        VertexOf::Keepout(id) => d
                            .routing
                            .keepouts
                            .iter()
                            .find(|z| &z.id == id)
                            .map(|z| z.outline.flatten(0.02)),
                        _ => d.board.outline.as_ref().map(|o| o.flatten(0.02)),
                    }
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| xf.to_screen(p))
                    .collect();
                    painter.add(egui::Shape::closed_line(pts, Stroke::new(2.0, ACCENT)));
                }
            }
        }
        if let Some(of) = self.editable_outline() {
            let hot = self.cursor_mm.and_then(|p| self.handle_at(p)).map(|h| h.1);
            for (i, v) in edits::vertices(d, &of).into_iter().enumerate() {
                let s = xf.to_screen(v);
                let r =
                    ERect::from_center_size(s, Vec2::splat(if hot == Some(i) { 9.0 } else { 7.0 }));
                painter.rect_filled(r, CornerRadius::same(2), CANVAS_EDGE);
                painter.rect_stroke(
                    r,
                    CornerRadius::same(2),
                    Stroke::new(1.5, ACCENT),
                    StrokeKind::Inside,
                );
            }
        }

        // Gestures in progress.
        let cur = self.cursor_mm.or(self.last_cursor);
        match (&self.gesture, cur) {
            (Gesture::Drag { from, what }, Some(p)) => {
                let (delta, to) = self.drag_delta(what, *from, p);
                self.ghost(painter, xf, what, delta, to, self.turn);
            }
            (Gesture::Grab { from, refs, anchor }, Some(p)) => {
                let what = Drag::Parts(refs.clone(), *anchor);
                let (delta, to) = self.drag_delta(&what, *from, p);
                self.ghost(painter, xf, &what, delta, to, self.turn);
            }
            (Gesture::Box { from }, Some(p)) => {
                let r = ERect::from_two_pos(xf.to_screen(*from), xf.to_screen(p));
                let enclose = p.x >= from.x;
                let c = if enclose {
                    ACCENT
                } else {
                    Color32::from_rgb(0x6C, 0xB4, 0xFF)
                };
                painter.rect_filled(r, CornerRadius::same(2), c.gamma_multiply(0.08));
                if enclose {
                    painter.rect_stroke(
                        r,
                        CornerRadius::same(2),
                        Stroke::new(1.2, c),
                        StrokeKind::Inside,
                    );
                } else {
                    let pts = [
                        r.left_top(),
                        r.right_top(),
                        r.right_bottom(),
                        r.left_bottom(),
                        r.left_top(),
                    ];
                    for w in pts.windows(2) {
                        painter.extend(egui::Shape::dashed_line(
                            &[w[0], w[1]],
                            Stroke::new(1.2, c),
                            5.0,
                            4.0,
                        ));
                    }
                }
            }
            (Gesture::Rect { from }, Some(p)) => {
                let ps = snap_pt(p, self.grid());
                let pts: Vec<Pos2> = rect_pts(*from, ps)
                    .into_iter()
                    .map(|q| xf.to_screen(q))
                    .collect();
                painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, ACCENT)));
            }
            _ => {}
        }
        if let Some(s) = &self.settling {
            self.ghost(painter, xf, &s.what, s.delta, s.to, s.turn);
        }
        if let (Some(pl), Some(p)) = (&self.placing, cur) {
            let ps = snap_pt(p, self.grid());
            let placement = Placement {
                x: ps.x,
                y: ps.y,
                rotation: pl.rotation,
                side: pl.side,
                locked: false,
            };
            self.ghost_part(painter, xf, &pl.refdes, &placement, ACCENT);
        }

        // Route draft.
        if let (Some(r), Some(p)) = (&self.route, cur) {
            for (layer, pts) in &r.runs {
                let c = self.layer_color(self.index_of(*layer));
                self.track_line(
                    painter,
                    xf,
                    pts,
                    r.width,
                    Color32::from_rgb(c.r(), c.g(), c.b()).gamma_multiply(0.85),
                );
            }
            let (layer, run) = r.runs.last().unwrap();
            let li = self.index_of(*layer);
            let last = *run.last().unwrap();
            let target = self
                .magnet(p, Some(li), Some(&r.net))
                .map(|m| m.at)
                .unwrap_or_else(|| snap_pt(p, self.grid()));
            let mut pts = vec![last];
            pts.extend(posture(last, target, self.diagonal_first));
            let bad = pts
                .windows(2)
                .any(|w| self.conflicts(w[0], w[1], li, &r.net, r.width));
            let c = self.layer_color(li);
            let col = if bad {
                ERROR
            } else {
                Color32::from_rgb(c.r(), c.g(), c.b())
            };
            self.track_line(painter, xf, &pts, r.width, col.gamma_multiply(0.55));
            for w in pts.windows(2) {
                painter.line_segment(
                    [xf.to_screen(w[0]), xf.to_screen(w[1])],
                    Stroke::new(1.0, col),
                );
            }
            for v in &r.vias {
                let s = xf.to_screen(*v);
                let d = &l.data.board.rules;
                painter.circle_filled(s, ((d.via_diameter / 2.0) * xf.ppm_pt) as f32, TEXT_2);
                painter.circle_filled(s, ((d.via_drill / 2.0) * xf.ppm_pt) as f32, CANVAS_EDGE);
            }
            // The length of the route so far, with the part to the pointer, as in
            // other editors.
            let seg_len = |w: &[Point]| w[0].dist(w[1]);
            let length: f64 = r
                .runs
                .iter()
                .flat_map(|(_, run)| run.windows(2).map(seg_len))
                .chain(pts.windows(2).map(seg_len))
                .sum();
            let label = format!(
                "{} · {} · {:.2} mm wide · {:.2} mm long{}",
                r.net,
                SceneLayer::Copper(li).name(self.copper_count()),
                r.width,
                length,
                match (bad, self.route_mode) {
                    (true, RouteMode::Walkaround) =>
                        " · too close: a click goes around".to_string(),
                    (true, RouteMode::Push) =>
                        " · too close: tracks move aside at the end".to_string(),
                    (true, RouteMode::Direct) => " · too close".to_string(),
                    (false, m) => format!(" · {}", m.name()),
                }
            );
            // A dark tag, so that the text reads over copper and pad labels.
            let color = if bad { ERROR } else { TEXT };
            let galley = painter.layout_no_wrap(label, mono(11.0), color);
            let tip = xf.to_screen(target);
            let size = galley.size();
            let mut at = tip + Vec2::new(16.0, -16.0 - size.y);
            // Keep the tag on the canvas: near an edge, put it on the other side.
            if at.x + size.x + 8.0 > xf.rect.right() {
                at.x = tip.x - 16.0 - size.x;
            }
            if at.y - 4.0 < xf.rect.top() {
                at.y = tip.y + 16.0;
            }
            let r = egui::Rect::from_min_size(at, size).expand2(Vec2::new(6.0, 3.0));
            painter.rect_filled(r, 4.0, RAISED);
            painter.galley(at, galley, color);
        }

        // Outline drafts.
        if !self.poly.is_empty() {
            let mut pts: Vec<Pos2> = self.poly.iter().map(|p| xf.to_screen(*p)).collect();
            if let Some(p) = cur {
                pts.push(xf.to_screen(snap_pt(p, self.grid())));
            }
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.5, ACCENT)));
            if pts.len() > 2 {
                painter.extend(egui::Shape::dashed_line(
                    &[*pts.last().unwrap(), pts[0]],
                    Stroke::new(1.0, ACCENT.gamma_multiply(0.6)),
                    4.0,
                    4.0,
                ));
            }
            for p in &pts[..self.poly.len()] {
                painter.circle_filled(*p, 3.0, ACCENT);
            }
        }

        // Snapped crosshair for the draw tools.
        if self.tool != Tool::Select || self.placing.is_some() {
            if let Some(p) = self.cursor_mm {
                let s = xf.to_screen(snap_pt(p, self.grid()));
                let c = TEXT_2.gamma_multiply(0.8);
                painter.line_segment(
                    [s - Vec2::new(7.0, 0.0), s + Vec2::new(7.0, 0.0)],
                    Stroke::new(1.0, c),
                );
                painter.line_segment(
                    [s - Vec2::new(0.0, 7.0), s + Vec2::new(0.0, 7.0)],
                    Stroke::new(1.0, c),
                );
            }
        }
    }

    fn grid_dots(&self, painter: &egui::Painter, xf: &Xf) {
        let g = self.grid();
        let mut step = g;
        while step * xf.ppm_pt < 14.0 {
            step *= 5.0;
            if step > 50.0 {
                return;
            }
        }
        let a = xf.to_board(xf.rect.left_bottom());
        let b = xf.to_board(xf.rect.right_top());
        let nx = ((b.x - a.x) / step) as usize;
        let ny = ((b.y - a.y) / step) as usize;
        if nx * ny > 12_000 {
            return;
        }
        let c = Color32::from_white_alpha(22);
        let x0 = (a.x / step).ceil() * step;
        let y0 = (a.y / step).ceil() * step;
        let mut y = y0;
        while y <= b.y {
            let mut x = x0;
            while x <= b.x {
                painter.circle_filled(xf.to_screen(Point::new(x, y)), 0.9, c);
                x += step;
            }
            y += step;
        }
    }

    pub(crate) fn tool_hint(&self) -> &'static str {
        if self.placing.is_some() {
            return "Click to place · R rotate · F flip side · Esc cancel";
        }
        if let Gesture::Grab { .. } = self.gesture {
            return "Click to drop · R rotate · Esc cancel";
        }
        self.tool.hint()
    }

    // ------------------------------------------------------------ toolbar

    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, canvas: ERect) {
        let n = self.copper_count();
        let bar = ERect::from_min_size(
            canvas.left_top() + Vec2::new(12.0, 12.0),
            Vec2::new(canvas.width() - 24.0, 36.0),
        );
        let mut b = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        egui::Frame::new()
            .fill(RAISED.gamma_multiply(0.97))
            .stroke(Stroke::new(1.0, HAIRLINE))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(egui::Margin::symmetric(4, 4))
            .show(&mut b, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // Narrow canvases (several boards open) get the compact bar.
                let compact = canvas.width() < 900.0;
                let tiny = canvas.width() < 560.0;
                ui.horizontal(|ui| {
                    for (t, label, key) in Tool::ALL {
                        let shown = if compact { "" } else { label };
                        if widgets::tool_button(ui, shown, key, self.tool == t)
                            .on_hover_text(format!("{label} ({key}) · {}", t.hint()))
                            .clicked()
                        {
                            self.set_tool(t);
                        }
                    }
                    widgets::vdivider(ui);
                    let c = self.layer_color(self.active);
                    let name = SceneLayer::Copper(self.active).name(n);
                    if widgets::pill(ui, Some(c), &name)
                        .on_hover_text("Active copper layer for routes and pours · PgUp / PgDn")
                        .clicked()
                    {
                        self.active = (self.active + 1) % n.max(1);
                        if n == 2 {
                            self.active = if self.active == 0 { 0 } else { n - 1 };
                        }
                        self.dirty = true;
                    }
                    // A tiny bar keeps the tools, the layer and the Board menu. N still
                    // changes the grid.
                    if !tiny
                        && widgets::pill(ui, None, &fmt_grid(self.grid()))
                            .on_hover_text("Grid · N / Shift+N")
                            .clicked()
                    {
                        self.grid_i = (self.grid_i + 1) % edits::GRIDS.len();
                    }
                    widgets::vdivider(ui);
                    self.board_menu(ui);
                    if tiny {
                        return;
                    }
                    widgets::vdivider(ui);
                    let (undo, redo, busy) = self
                        .editor
                        .as_ref()
                        .map(|e| (e.undo.clone(), e.redo.clone(), e.busy))
                        .unwrap_or((None, None, 0));
                    let u = widgets::pill(ui, None, "Undo");
                    let u = match &undo {
                        Some(l) => u.on_hover_text(format!("Undo {l} · Ctrl+Z")),
                        None => u.on_hover_text("There is nothing to undo"),
                    };
                    if u.clicked() {
                        self.undo();
                    }
                    let r = widgets::pill(ui, None, "Redo");
                    let r = match &redo {
                        Some(l) => r.on_hover_text(format!("Redo {l} · Ctrl+Y")),
                        None => r.on_hover_text("There is nothing to redo"),
                    };
                    if r.clicked() {
                        self.redo();
                    }
                    if busy > 0 || !self.bg.is_empty() {
                        ui.add_space(6.0);
                        ui.add(egui::Spinner::new().size(12.0).color(TEXT_3));
                    }
                });
            });
    }
}

// ---------------------------------------------------------------- properties

/// A number field that commits once: when a drag ends or when the text entry ends.
fn num_field(ui: &mut egui::Ui, key: &str, value: f64, speed: f64, suffix: &str) -> Option<f64> {
    let id = ui.scope_id().with(("num", key));
    let mut v = ui.data(|d| d.get_temp::<f64>(id)).unwrap_or(value);
    let resp = ui.add(
        egui::DragValue::new(&mut v)
            .speed(speed)
            .suffix(suffix)
            .max_decimals(4),
    );
    if resp.dragged() || resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, v));
        None
    } else {
        ui.data_mut(|d| d.remove::<f64>(id));
        ((resp.drag_stopped() || resp.lost_focus()) && (v - value).abs() > 1e-9).then_some(v)
    }
}

fn kv(ui: &mut egui::Ui, k: &str) {
    ui.label(egui::RichText::new(k).font(body(12.0)).color(TEXT_3));
}

fn val(ui: &mut egui::Ui, v: impl Into<String>) {
    let v = v.into();
    ui.add(
        egui::Label::new(egui::RichText::new(v.clone()).font(mono(12.0)).color(TEXT)).truncate(),
    )
    .on_hover_text(v);
}

impl App {
    /// Group actions for a selected part: select the full group, copy the
    /// layout of another group and keep this part in its position, or copy the
    /// layout of this group to all groups with the same connections.
    fn group_panel(&mut self, ui: &mut egui::Ui, d: &ProjectData, r: &str, g: &str) {
        let fresh =
            matches!(&self.group_matches, Some((k, t, _)) if k == g && *t == self.last_reload);
        if !fresh {
            let m = lamina_core::reuse::matching_groups(d, g);
            self.group_matches = Some((g.to_string(), self.last_reload, m));
        }
        let matches = self
            .group_matches
            .as_ref()
            .map(|m| m.2.clone())
            .unwrap_or_default();
        let members = d.parts.groups().remove(g).unwrap_or_default();
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(format!("Group {g} · {} parts", members.len()))
                .font(semibold(13.0))
                .color(TEXT),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if widgets::text_button(ui, "Select group") {
                self.sel = members
                    .iter()
                    .filter(|m| d.placement.placements.contains_key(*m))
                    .map(|m| Sel::Part(m.clone()))
                    .collect();
            }
            if !matches.is_empty() && widgets::text_button(ui, "Copy layout to all like it") {
                let (from, to, n) = (g.to_string(), matches.clone(), matches.len());
                self.commit(move |d| {
                    let src = lamina_core::reuse::Block::Group(from.clone()).parts(d)?;
                    let tgt = to
                        .iter()
                        .map(|t| lamina_core::reuse::Block::Group(t.clone()).parts(d))
                        .collect::<Result<Vec<_>, _>>()?;
                    let o = lamina_core::reuse::ApplyOpts {
                        routing: true,
                        ..Default::default()
                    };
                    lamina_core::reuse::repeat(d, &src, &tgt, None, &o)?;
                    Ok(format!("Copied the layout of {from} to {n} group(s)"))
                });
            }
        });
        if matches.is_empty() {
            ui.label(
                egui::RichText::new("No other group has the same connections as this group.")
                    .font(body(12.0))
                    .color(TEXT_3),
            );
            return;
        }
        ui.horizontal(|ui| {
            kv(ui, "layout like");
            egui::ComboBox::from_id_salt(("layout-from", g))
                .selected_text("select a group")
                .show_ui(ui, |ui| {
                    for m in &matches {
                        if ui.selectable_label(false, m).clicked() {
                            let (from, to, anchor) = (m.clone(), g.to_string(), r.to_string());
                            self.commit(move |d| {
                                let src =
                                    lamina_core::reuse::Block::Group(from.clone()).parts(d)?;
                                let tgt = lamina_core::reuse::Block::Group(to.clone()).parts(d)?;
                                let o = lamina_core::reuse::ApplyOpts {
                                    routing: true,
                                    ..Default::default()
                                };
                                lamina_core::reuse::repeat(d, &src, &[tgt], Some(&anchor), &o)?;
                                Ok(format!(
                                    "Copied the layout of {from} to {to}, around {anchor}"
                                ))
                            });
                        }
                    }
                });
        });
    }

    pub(crate) fn selection_panel(&mut self, ui: &mut egui::Ui) {
        let Some(l) = &self.loaded else { return };
        let d = l.data.clone();
        let n = d.board.stackup.copper_layers;
        // Remove selections whose items are gone (deleted by the viewer or the
        // agent). Do not do this while our own edit is not yet in the files.
        let settled = self.editor.as_ref().is_none_or(|e| e.busy == 0)
            && match (self.own_edit, self.last_reload) {
                (Some(e), Some(r)) => r > e,
                (Some(_), None) => false,
                _ => true,
            };
        self.sel.retain(|s| {
            !settled
                || match s {
                    Sel::Part(r) => d.placement.placements.contains_key(r),
                    Sel::Track { id, .. } => d.routing.tracks.iter().any(|t| &t.id == id),
                    Sel::Via(id) => d.routing.vias.iter().any(|t| &t.id == id),
                    Sel::Zone(id) => d.routing.zones.iter().any(|t| &t.id == id),
                    Sel::Keepout(id) => d.routing.keepouts.iter().any(|t| &t.id == id),
                    Sel::Hole(id) => d.board.holes.iter().any(|t| &t.id == id),
                    Sel::Outline => d.board.outline.is_some(),
                }
        });
        if self.sel.is_empty() {
            return;
        }
        let mut items: Vec<Sel> = Vec::new();
        for s in &self.sel {
            if !items.iter().any(|x| x.same_item(s)) {
                items.push(s.clone());
            }
        }
        if items.len() > 1 {
            ui.label(
                egui::RichText::new(format!("{} items selected", items.len()))
                    .font(semibold(16.0))
                    .color(TEXT),
            );
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let refs = self.selected_parts();
                if !refs.is_empty() {
                    if widgets::text_button(ui, "Rotate 90°") {
                        let r = refs.clone();
                        self.commit(move |d| edits::rotate_parts(d, &r, 90.0));
                    }
                    if widgets::text_button(ui, "Flip") {
                        let r = refs.clone();
                        self.commit(move |d| edits::flip_parts(d, &r));
                    }
                }
                if widgets::text_button(ui, "Delete") {
                    let s = std::mem::take(&mut self.sel);
                    self.commit(move |d| edits::delete(d, &s));
                }
            });
            return;
        }
        let s = items[0].clone();
        let grid = |ui: &mut egui::Ui, f: &mut dyn FnMut(&mut egui::Ui)| {
            egui::Grid::new("props")
                .num_columns(2)
                .spacing([14.0, 7.0])
                .show(ui, |ui| f(ui));
        };
        match s {
            Sel::Part(r) => {
                let Some(pl) = d.placement.placements.get(&r).copied() else {
                    return;
                };
                let part = d.parts.parts.get(&r);
                ui.label(egui::RichText::new(&r).font(semibold(16.0)).color(TEXT));
                if let Some(v) = part.and_then(|p| p.value.clone().or(p.mpn.clone())) {
                    ui.label(egui::RichText::new(v).font(body(12.5)).color(TEXT_2));
                }
                ui.add_space(8.0);
                let mut new = pl;
                grid(ui, &mut |ui| {
                    kv(ui, "x");
                    if let Some(v) = num_field(ui, &format!("{r}.x"), pl.x, 0.05, " mm") {
                        new.x = v;
                    }
                    ui.end_row();
                    kv(ui, "y");
                    if let Some(v) = num_field(ui, &format!("{r}.y"), pl.y, 0.05, " mm") {
                        new.y = v;
                    }
                    ui.end_row();
                    kv(ui, "rotation");
                    ui.horizontal(|ui| {
                        if let Some(v) = num_field(ui, &format!("{r}.rot"), pl.rotation, 1.0, "°")
                        {
                            new.rotation = v;
                        }
                        for a in [0.0, 90.0, 180.0, 270.0] {
                            if widgets::text_button(ui, &format!("{a}")) {
                                new.rotation = a;
                            }
                        }
                    });
                    ui.end_row();
                    kv(ui, "side");
                    ui.horizontal(|ui| {
                        for side in [Side::Top, Side::Bottom] {
                            let on = pl.side == side;
                            let t = egui::RichText::new(side.as_str())
                                .font(body(12.5))
                                .color(if on { ACCENT } else { TEXT_2 });
                            if ui.add(egui::Button::new(t).selected(on)).clicked() && !on {
                                new.side = side;
                            }
                        }
                    });
                    ui.end_row();
                    kv(ui, "locked");
                    let mut lk = pl.locked;
                    if ui.checkbox(&mut lk, "").changed() {
                        new.locked = lk;
                    }
                    ui.end_row();
                });
                if new != pl {
                    let r2 = r.clone();
                    self.commit(move |d| {
                        if let Some(old) = d.placement.placements.get(&r2) {
                            if old.locked
                                && new.locked
                                && (old.x, old.y, old.rotation, old.side)
                                    != (new.x, new.y, new.rotation, new.side)
                            {
                                return Err(format!("{r2} is locked. Unlock it first."));
                            }
                        }
                        edits::set_placement(d, &r2, new)
                    });
                }
                ui.add_space(8.0);
                let mut rows = Vec::new();
                crate::part_rows(&d, &r, &mut rows);
                egui::Grid::new("partinfo")
                    .num_columns(2)
                    .spacing([14.0, 5.0])
                    .show(ui, |ui| {
                        for (k, v) in rows
                            .into_iter()
                            .filter(|(k, _)| !matches!(*k, "at" | "rotation" | "side"))
                        {
                            kv(ui, k);
                            ui.label(egui::RichText::new(v).font(body(12.5)).color(TEXT));
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if widgets::text_button(ui, "Rotate 90°") {
                        let v = vec![r.clone()];
                        self.commit(move |d| edits::rotate_parts(d, &v, 90.0));
                    }
                    if widgets::text_button(ui, "Flip") {
                        let v = vec![r.clone()];
                        self.commit(move |d| edits::flip_parts(d, &v));
                    }
                    if widgets::text_button(ui, "Remove from board") {
                        let v = vec![Sel::Part(r.clone())];
                        self.sel.clear();
                        self.commit(move |d| edits::delete(d, &v));
                    }
                });
                if let Some(g) = part.and_then(|p| p.group.clone()) {
                    self.group_panel(ui, &d, &r, &g);
                }
            }
            Sel::Track { id, seg } => {
                let Some(t) = d.routing.tracks.iter().find(|t| t.id == id).cloned() else {
                    return;
                };
                ui.label(
                    egui::RichText::new(format!("Track {id}"))
                        .font(semibold(16.0))
                        .color(TEXT),
                );
                ui.add_space(8.0);
                let len: f64 = t.points.windows(2).map(|w| w[0].dist(w[1])).sum();
                let mut width = None;
                let mut layer = None;
                grid(ui, &mut |ui| {
                    kv(ui, "net");
                    val(ui, &t.net);
                    ui.end_row();
                    kv(ui, "width");
                    width = num_field(ui, &format!("{id}.w"), t.width, 0.01, " mm");
                    ui.end_row();
                    kv(ui, "layer");
                    let mut li = t.layer.copper().map(|c| c.index(n)).unwrap_or(0);
                    let before = li;
                    egui::ComboBox::from_id_salt(("tl", &id))
                        .selected_text(SceneLayer::Copper(li).name(n))
                        .show_ui(ui, |ui| {
                            for i in 0..n {
                                ui.selectable_value(&mut li, i, SceneLayer::Copper(i).name(n));
                            }
                        });
                    if li != before {
                        layer = Some(Layer::Copper(Copper::from_index(li, n)));
                    }
                    ui.end_row();
                    kv(ui, "length");
                    val(ui, format!("{len:.2} mm"));
                    ui.end_row();
                    kv(ui, "segment");
                    val(
                        ui,
                        format!("{} of {}", seg + 1, t.points.len().saturating_sub(1)),
                    );
                    ui.end_row();
                });
                if width.is_some() || layer.is_some() {
                    let id2 = id.clone();
                    self.commit(move |d| {
                        let t = d
                            .routing
                            .tracks
                            .iter_mut()
                            .find(|t| t.id == id2)
                            .ok_or("The track was deleted")?;
                        if let Some(w) = width {
                            if w <= 0.0 {
                                return Err("The width must be more than 0".into());
                            }
                            t.width = lamina_model::json::round6(w);
                        }
                        if let Some(l) = layer {
                            t.layer = l;
                        }
                        Ok(format!("Track {id2}: {} mm on {}", t.width, t.layer))
                    });
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if widgets::text_button(ui, "Delete segment") {
                        let v = vec![Sel::Track {
                            id: id.clone(),
                            seg,
                        }];
                        self.sel.clear();
                        self.commit(move |d| edits::delete(d, &v));
                    }
                    if widgets::text_button(ui, "Delete track") {
                        let id2 = id.clone();
                        self.sel.clear();
                        self.commit(move |d| {
                            d.routing.tracks.retain(|t| t.id != id2);
                            Ok(format!("Deleted track {id2}"))
                        });
                    }
                });
            }
            Sel::Via(id) => {
                let Some(v) = d.routing.vias.iter().find(|v| v.id == id).cloned() else {
                    return;
                };
                ui.label(
                    egui::RichText::new(format!("Via {id}"))
                        .font(semibold(16.0))
                        .color(TEXT),
                );
                ui.add_space(8.0);
                let (mut dia, mut drill, mut x, mut y) = (None, None, None, None);
                grid(ui, &mut |ui| {
                    kv(ui, "net");
                    val(ui, &v.net);
                    ui.end_row();
                    kv(ui, "x");
                    x = num_field(ui, &format!("{id}.x"), v.at.x, 0.05, " mm");
                    ui.end_row();
                    kv(ui, "y");
                    y = num_field(ui, &format!("{id}.y"), v.at.y, 0.05, " mm");
                    ui.end_row();
                    kv(ui, "diameter");
                    dia = num_field(ui, &format!("{id}.d"), v.diameter, 0.01, " mm");
                    ui.end_row();
                    kv(ui, "drill");
                    drill = num_field(ui, &format!("{id}.dr"), v.drill, 0.01, " mm");
                    ui.end_row();
                });
                if x.is_some() || y.is_some() {
                    let to = Point::new(x.unwrap_or(v.at.x), y.unwrap_or(v.at.y));
                    let delta = to.sub(v.at);
                    let id2 = id.clone();
                    self.commit(move |d| edits::move_via(d, &id2, delta));
                }
                if dia.is_some() || drill.is_some() {
                    let id2 = id.clone();
                    self.commit(move |d| {
                        let v = d
                            .routing
                            .vias
                            .iter_mut()
                            .find(|v| v.id == id2)
                            .ok_or("The via was deleted")?;
                        let nd = dia.unwrap_or(v.diameter);
                        let ndr = drill.unwrap_or(v.drill);
                        if !(ndr > 0.0 && nd > ndr) {
                            return Err("A via must have 0 < drill < diameter".into());
                        }
                        v.diameter = lamina_model::json::round6(nd);
                        v.drill = lamina_model::json::round6(ndr);
                        Ok(format!("Via {id2}: {} / {} mm", v.diameter, v.drill))
                    });
                }
                ui.add_space(8.0);
                if widgets::text_button(ui, "Delete via") {
                    let s = vec![Sel::Via(id.clone())];
                    self.sel.clear();
                    self.commit(move |d| edits::delete(d, &s));
                }
            }
            Sel::Zone(id) => {
                let Some(z) = d.routing.zones.iter().find(|z| z.id == id).cloned() else {
                    return;
                };
                ui.label(
                    egui::RichText::new(format!("Pour {id}"))
                        .font(semibold(16.0))
                        .color(TEXT),
                );
                ui.add_space(8.0);
                let mut nz = z.clone();
                grid(ui, &mut |ui| {
                    kv(ui, "net");
                    egui::ComboBox::from_id_salt(("zn", &id))
                        .selected_text(nz.net.clone().unwrap_or_else(|| "none".into()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut nz.net, None, "none");
                            for name in d.nets.nets.keys() {
                                ui.selectable_value(&mut nz.net, Some(name.clone()), name);
                            }
                        });
                    ui.end_row();
                    kv(ui, "layers");
                    ui.horizontal_wrapped(|ui| {
                        for i in 0..n {
                            let l = Layer::Copper(Copper::from_index(i, n));
                            let mut on = nz.layers.contains(&l);
                            if ui
                                .checkbox(&mut on, SceneLayer::Copper(i).name(n))
                                .changed()
                            {
                                if on {
                                    nz.layers.push(l);
                                } else {
                                    nz.layers.retain(|x| *x != l);
                                }
                            }
                        }
                    });
                    ui.end_row();
                    kv(ui, "priority");
                    if let Some(v) = num_field(ui, &format!("{id}.p"), z.priority as f64, 0.1, "") {
                        nz.priority = v.round() as i32;
                    }
                    ui.end_row();
                    kv(ui, "clearance");
                    let cur = z.clearance.unwrap_or(d.board.rules.clearance);
                    if let Some(v) = num_field(ui, &format!("{id}.c"), cur, 0.01, " mm") {
                        nz.clearance = Some(lamina_model::json::round6(v.max(0.0)));
                    }
                    ui.end_row();
                });
                if nz != z {
                    if nz.layers.is_empty() {
                        self.say("A pour must have at least one layer", false);
                    } else {
                        let id2 = id.clone();
                        self.commit(move |d| {
                            let z = d
                                .routing
                                .zones
                                .iter_mut()
                                .find(|z| z.id == id2)
                                .ok_or("The pour was deleted")?;
                            *z = nz;
                            Ok(format!("Pour {id2} updated"))
                        });
                    }
                }
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Drag the corner handles to change the shape. Drag inside to move the pour.",
                    )
                    .font(body(12.0))
                    .color(TEXT_3),
                );
                ui.add_space(4.0);
                if widgets::text_button(ui, "Delete pour") {
                    let s = vec![Sel::Zone(id.clone())];
                    self.sel.clear();
                    self.commit(move |d| edits::delete(d, &s));
                }
            }
            Sel::Keepout(id) => {
                let Some(k) = d.routing.keepouts.iter().find(|z| z.id == id).cloned() else {
                    return;
                };
                ui.label(
                    egui::RichText::new(format!("Keep-out {id}"))
                        .font(semibold(16.0))
                        .color(TEXT),
                );
                ui.add_space(8.0);
                let mut nk = k.clone();
                grid(ui, &mut |ui| {
                    kv(ui, "layers");
                    ui.horizontal_wrapped(|ui| {
                        for i in 0..n {
                            let l = Layer::Copper(Copper::from_index(i, n));
                            let mut on = nk.layers.contains(&l);
                            if ui
                                .checkbox(&mut on, SceneLayer::Copper(i).name(n))
                                .changed()
                            {
                                if on {
                                    nk.layers.push(l);
                                } else {
                                    nk.layers.retain(|x| *x != l);
                                }
                            }
                        }
                    });
                    ui.end_row();
                    kv(ui, "forbids");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut nk.forbid.tracks, "tracks");
                        ui.checkbox(&mut nk.forbid.vias, "vias");
                        ui.checkbox(&mut nk.forbid.pours, "pours");
                        ui.checkbox(&mut nk.forbid.pads, "pads");
                        ui.checkbox(&mut nk.forbid.parts, "parts");
                    });
                    ui.end_row();
                });
                if nk != k {
                    let id2 = id.clone();
                    self.commit(move |d| {
                        let k = d
                            .routing
                            .keepouts
                            .iter_mut()
                            .find(|z| z.id == id2)
                            .ok_or("The keep-out was deleted")?;
                        *k = nk;
                        Ok(format!("Keep-out {id2} updated"))
                    });
                }
                ui.add_space(8.0);
                if widgets::text_button(ui, "Delete keep-out") {
                    let s = vec![Sel::Keepout(id.clone())];
                    self.sel.clear();
                    self.commit(move |d| edits::delete(d, &s));
                }
            }
            Sel::Hole(id) => {
                let Some(h) = d.board.holes.iter().find(|h| h.id == id).cloned() else {
                    return;
                };
                ui.label(
                    egui::RichText::new(format!("Hole {id}"))
                        .font(semibold(16.0))
                        .color(TEXT),
                );
                ui.add_space(8.0);
                let (mut x, mut y) = (None, None);
                grid(ui, &mut |ui| {
                    kv(ui, "x");
                    x = num_field(ui, &format!("{id}.x"), h.at.x, 0.05, " mm");
                    ui.end_row();
                    kv(ui, "y");
                    y = num_field(ui, &format!("{id}.y"), h.at.y, 0.05, " mm");
                    ui.end_row();
                    kv(ui, "drill");
                    val(
                        ui,
                        format!(
                            "{:.2} mm {}",
                            h.drill.min_size(),
                            if h.plated { "plated" } else { "NPTH" }
                        ),
                    );
                    ui.end_row();
                });
                if x.is_some() || y.is_some() {
                    let to = Point::new(
                        lamina_model::json::round6(x.unwrap_or(h.at.x)),
                        lamina_model::json::round6(y.unwrap_or(h.at.y)),
                    );
                    let id2 = id.clone();
                    self.commit(move |d| {
                        let h = d
                            .board
                            .holes
                            .iter_mut()
                            .find(|h| h.id == id2)
                            .ok_or("The hole was deleted")?;
                        h.at = to;
                        Ok(format!("Hole {id2} at {:.3}, {:.3} mm", to.x, to.y))
                    });
                }
                ui.add_space(8.0);
                if widgets::text_button(ui, "Delete hole") {
                    let s = vec![Sel::Hole(id.clone())];
                    self.sel.clear();
                    self.commit(move |d| edits::delete(d, &s));
                }
            }
            Sel::Outline => {
                ui.label(
                    egui::RichText::new("Board outline")
                        .font(semibold(16.0))
                        .color(TEXT),
                );
                ui.add_space(8.0);
                if let Some(b) = d.board.outline.as_ref().and_then(|o| o.bbox()) {
                    grid(ui, &mut |ui| {
                        kv(ui, "size");
                        val(ui, format!("{:.2} × {:.2} mm", b.width(), b.height()));
                        ui.end_row();
                        kv(ui, "corners");
                        val(
                            ui,
                            format!(
                                "{}",
                                d.board
                                    .outline
                                    .as_ref()
                                    .map(|o| o.vertices.len())
                                    .unwrap_or(0)
                            ),
                        );
                        ui.end_row();
                    });
                }
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Drag the corner handles to change the shape. The Outline tool draws a new outline.",
                    )
                    .font(body(12.0))
                    .color(TEXT_3),
                );
            }
        }
    }

    /// Parts in the parts list that are not on the board.
    pub(crate) fn unplaced_panel(&mut self, ui: &mut egui::Ui) {
        let Some(l) = &self.loaded else { return };
        let d = &l.data;
        let list: Vec<String> = d
            .parts
            .parts
            .keys()
            .filter(|r| !d.placement.placements.contains_key(*r))
            .cloned()
            .collect();
        if list.is_empty() {
            return;
        }
        ui.add_space(18.0);
        widgets::hairline(ui);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            widgets::section_title(ui, "Not on the board");
            ui.label(
                egui::RichText::new(list.len().to_string())
                    .font(mono(11.5))
                    .color(TEXT_3),
            );
        });
        ui.label(
            egui::RichText::new("Click a part, then click the board to place it.")
                .font(body(12.0))
                .color(TEXT_3),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for r in list {
                let on = self.placing.as_ref().is_some_and(|p| p.refdes == r);
                if widgets::tool_button(ui, &r, "", on).clicked() {
                    self.set_tool(Tool::Select);
                    self.mode = crate::Mode::TwoD;
                    self.placing = Some(Placing {
                        refdes: r,
                        rotation: 0.0,
                        side: Side::Top,
                    });
                }
            }
        });
    }
}

#[cfg(test)]
mod box_tests {
    use super::*;

    #[test]
    fn a_crossing_segment_meets_the_box() {
        let r = Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(10.0, 10.0),
        };
        // Both ends outside, the middle inside.
        assert!(segment_meets_rect(
            Point::new(-5.0, 5.0),
            Point::new(15.0, 5.0),
            &r
        ));
        assert!(segment_meets_rect(
            Point::new(2.0, 2.0),
            Point::new(3.0, 3.0),
            &r
        ));
        assert!(!segment_meets_rect(
            Point::new(-5.0, 15.0),
            Point::new(15.0, 12.0),
            &r
        ));
        assert!(!segment_meets_rect(
            Point::new(11.0, -1.0),
            Point::new(11.0, 20.0),
            &r
        ));
    }

    #[test]
    fn track_widths_step_up_and_down() {
        assert_eq!(next_width(0.25, true), 0.3);
        assert_eq!(next_width(0.25, false), 0.2);
        assert_eq!(next_width(0.22, true), 0.25);
        assert_eq!(next_width(2.0, true), 2.0);
        assert_eq!(next_width(0.1, false), 0.1);
    }

    #[test]
    fn a_point_in_a_ring() {
        let sq = [
            Point::new(0.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 4.0),
            Point::new(0.0, 4.0),
        ];
        assert!(point_in_ring(Point::new(2.0, 2.0), &sq));
        assert!(!point_in_ring(Point::new(5.0, 2.0), &sq));
    }
}

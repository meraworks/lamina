// In release builds, the viewer has no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod editor;
mod edits;
mod highlight;
mod labels;
mod live;
#[cfg(feature = "headless")]
mod shot;
mod style;
mod tools;
mod widgets;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Pos2, Rect as ERect, Sense, Stroke, StrokeKind, Vec2,
};
use eframe::egui_wgpu;
use lamina_geom::{Owner, Prim};
use lamina_model::{
    Layer, NotesFile, Point, ProjectData, Rect, Severity, Side, json, write_atomic,
};
use lamina_render::{Camera, Gpu, Gpu2D, Gpu3D, MeshCache, Target};
use lamina_scene::SceneLayer;

use editor::Editor;
use edits::Sel;
use live::{Live, Loaded, Msg};
use style::*;
use tools::{Gesture, Placing, Settling, Tool};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    TwoD,
    ThreeD,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pick {
    Pad {
        part: String,
        pad: String,
        net: Option<String>,
        layer: u8,
    },
    Track {
        id: String,
        net: String,
        seg: usize,
    },
    Via {
        id: String,
        net: String,
    },
    Fill {
        zone: String,
        net: Option<String>,
    },
    Part {
        refdes: String,
    },
    Keepout {
        id: String,
    },
    Hole {
        id: String,
    },
    Outline,
}

impl Pick {
    fn net(&self) -> Option<&str> {
        match self {
            Pick::Pad { net, .. } => net.as_deref(),
            Pick::Track { net, .. } | Pick::Via { net, .. } => Some(net),
            Pick::Fill { net, .. } => net.as_deref(),
            Pick::Part { .. } | Pick::Keepout { .. } | Pick::Hole { .. } | Pick::Outline => None,
        }
    }

    fn part(&self) -> Option<&str> {
        match self {
            Pick::Pad { part, .. } | Pick::Part { refdes: part } => Some(part),
            _ => None,
        }
    }
}

pub(crate) struct App {
    root: Option<PathBuf>,
    live: Option<Live>,
    loaded: Option<Box<Loaded>>,
    /// The loaded board has text that needs the wide (CJK) font.
    needs_wide: bool,
    error: Option<String>,
    gpu: Option<Gpu>,
    target: Option<Target>,
    tex: Option<egui::TextureId>,
    g2d: Option<Gpu2D>,
    /// The copper layer that `g2d` draws at full strength.
    g2d_bright: u8,
    g3d: Option<Gpu3D>,
    cache: MeshCache,
    mode: Mode,
    center: [f64; 2],
    ppm: f64,
    fitted: bool,
    cam: Camera,
    cam_fitted: bool,
    visible: BTreeMap<SceneLayer, bool>,
    hover: Option<Pick>,
    sel: Vec<Sel>,
    editor: Option<Editor>,
    tool: Tool,
    gesture: Gesture,
    route: Option<edits::Route>,
    poly: Vec<Point>,
    placing: Option<Placing>,
    settling: Option<Settling>,
    /// The rotation (degrees) that R adds to the parts of a drag in progress.
    turn: f64,
    /// Single-layer mode: only the active copper layer shows.
    single: bool,
    /// The 3D view draws the part bodies.
    models: bool,
    /// How the route tool treats the copper of other nets.
    route_mode: tools::RouteMode,
    /// Push mode: what the push does to the other tracks for the route to the
    /// pointer, made when the pointer stops.
    push_preview: Option<tools::PushPreview>,
    /// The route to the pointer that waits for a push preview, and since when.
    push_wait: Option<(String, Instant)>,
    /// The segment that a click adds for the route to the pointer, made when
    /// the pointer stops for a moment: (key, points or why there is none).
    seg_preview: Option<(String, Result<Vec<Point>, String>)>,
    /// The worker thread that makes a push preview.
    push_job: Option<std::sync::mpsc::Receiver<tools::PushPreview>>,
    grid_i: usize,
    /// The right-click menu: where it was opened and what was under the pointer.
    menu: Option<(Point, Option<Pick>)>,
    active: u8,
    diagonal_first: bool,
    toast: Option<(String, bool, Instant)>,
    own_edit: Option<Instant>,
    /// The groups that can take the layout of a group: (group, load time, groups).
    group_matches: Option<(String, Option<Instant>, Vec<String>)>,
    mm_per_px: f64,
    last_cursor: Option<Point>,
    fit_pending: bool,
    flashes: Vec<(Rect, Instant)>,
    last_reload: Option<Instant>,
    cursor_mm: Option<Point>,
    note_text: String,
    note_at: Option<Point>,
    /// The highlighted net: the rest of the board is dim.
    hl: Option<highlight::NetHighlight>,
    note_status: Option<(String, Instant)>,
    dirty: bool,
    open_path: String,
    /// This board gets the keyboard shortcuts. If several boards are open, it is
    /// the board that the user clicked last.
    pub(crate) focused: bool,
    /// A request from the empty state. The shell does it.
    pub(crate) request: Option<OpenRequest>,
    /// The canvas size in pixels at the last frame. The panels use it to zoom.
    last_size_px: Vec2,
    /// Exports and other background jobs. Their results show as toasts.
    bg: Vec<std::sync::mpsc::Receiver<actions::BgDone>>,
    find: Option<actions::Find>,
    help: bool,
    measure: actions::Measure,
    /// The view and the canvas size of the last automatic fit. If the view did
    /// not change, a resized canvas gets a new fit.
    fit_2d_at: Option<([f64; 2], f64, Vec2)>,
    fit_3d_at: Option<(Camera, Vec2)>,
}

fn default_visible(l: SceneLayer) -> bool {
    !matches!(
        l,
        SceneLayer::Mask(_)
            | SceneLayer::Paste(_)
            | SceneLayer::Fab(_)
            | SceneLayer::Courtyard(Side::Bottom)
    )
}

fn config_file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("lamina").join("viewer-last-project.txt"))
}

impl App {
    fn new(gpu: Option<Gpu>, root: Option<PathBuf>, three_d: bool, ctx: &egui::Context) -> App {
        let mut app = App {
            root: None,
            live: None,
            loaded: None,
            needs_wide: false,
            error: None,
            gpu,
            target: None,
            tex: None,
            g2d: None,
            g2d_bright: 0,
            g3d: None,
            cache: MeshCache::default(),
            mode: if three_d { Mode::ThreeD } else { Mode::TwoD },
            center: [0.0, 0.0],
            ppm: 20.0,
            fitted: false,
            cam: Camera::frame([0.0; 3], [50.0, 50.0, 2.0]),
            cam_fitted: false,
            visible: BTreeMap::new(),
            hover: None,
            sel: Vec::new(),
            editor: None,
            tool: Tool::Select,
            gesture: Gesture::None,
            route: None,
            poly: Vec::new(),
            placing: None,
            settling: None,
            turn: 0.0,
            single: false,
            models: true,
            route_mode: tools::RouteMode::Walkaround,
            push_preview: None,
            push_wait: None,
            seg_preview: None,
            push_job: None,
            grid_i: 2,
            menu: None,
            active: 0,
            diagonal_first: false,
            toast: None,
            own_edit: None,
            group_matches: None,
            mm_per_px: 0.05,
            last_cursor: None,
            fit_pending: false,
            flashes: Vec::new(),
            last_reload: None,
            cursor_mm: None,
            note_text: String::new(),
            note_at: None,
            hl: None,
            note_status: None,
            dirty: true,
            open_path: String::new(),
            focused: true,
            request: None,
            last_size_px: Vec2::new(900.0, 700.0),
            bg: Vec::new(),
            find: None,
            help: false,
            measure: actions::Measure::default(),
            fit_2d_at: None,
            fit_3d_at: None,
        };
        if let Some(r) = root {
            app.open(r, ctx);
        }
        app
    }

    fn open(&mut self, root: PathBuf, ctx: &egui::Context) {
        if !ProjectData::is_project(&root) {
            self.error = Some(format!(
                "{} is not a Lamina project. It has no board.json.",
                root.display()
            ));
            return;
        }
        let ctx2 = ctx.clone();
        match Live::start(root.clone(), move || ctx2.request_repaint()) {
            Ok(l) => {
                self.live = Some(l);
                let ctx3 = ctx.clone();
                self.editor = Some(Editor::start(root.clone(), move || ctx3.request_repaint()));
                self.loaded = None;
                self.hl = None;
                self.fitted = false;
                self.cam_fitted = false;
                self.sel.clear();
                self.set_tool(Tool::Select);
                self.error = None;
                self.root = Some(root);
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn poll(&mut self) {
        self.poll_bg();
        while let Some(r) = self.editor.as_mut().and_then(|e| e.poll()) {
            if let Some(s) = self.settling.as_mut() {
                s.replied = true;
                if !r.ok {
                    self.settling = None;
                }
            }
            self.toast = Some((r.message, r.ok, Instant::now()));
        }
        let Some(live) = &self.live else { return };
        while let Ok(msg) = live.rx.try_recv() {
            match msg {
                Msg::Loaded(l) => {
                    let now = Instant::now();
                    if self.settling.as_ref().is_some_and(|s| s.replied) {
                        self.settling = None;
                    }
                    // Flash only the changes that someone else made.
                    let own = self
                        .own_edit
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(2500));
                    if self.loaded.is_some() && !own {
                        for r in &l.changed {
                            self.flashes.push((*r, now));
                        }
                    }
                    if l.models_changed {
                        self.cache = MeshCache::default();
                    }
                    for layer in l.s2.layers.iter().map(|d| d.layer) {
                        self.visible
                            .entry(layer)
                            .or_insert_with(|| default_visible(layer));
                    }
                    for i in 0..l.s2.copper_layers {
                        self.visible.entry(SceneLayer::Copper(i)).or_insert(true);
                    }
                    if let Some(gpu) = &self.gpu {
                        self.g2d_bright = self.bright_copper_of(&l.s2);
                        self.g2d = Some(gpu.upload_2d(&l.s2, 40.0, self.g2d_bright));
                        self.g3d = Some(gpu.upload_3d(&l.s3, &l.s2, &mut self.cache));
                    }
                    // Vendor part data can have Chinese text; then the viewer needs a wide font.
                    self.needs_wide = !style::has_wide()
                        && serde_json::to_string(&l.data.parts)
                            .is_ok_and(|t| style::needs_wide(&t));
                    if self
                        .hl
                        .as_ref()
                        .is_some_and(|h| !l.data.nets.nets.contains_key(&h.net))
                    {
                        self.hl = None;
                    }
                    if let Some(h) = &mut self.hl {
                        h.rebuild(
                            &l.analysis.geometry.copper,
                            &l.analysis.fills,
                            l.data.board.stackup.copper_layers,
                        );
                    }
                    self.loaded = Some(l);
                    self.error = None;
                    self.last_reload = Some(now);
                    // A preview of the board before the reload is stale.
                    self.push_preview = None;
                    self.dirty = true;
                }
                Msg::Error(e) => self.error = Some(e),
            }
        }
    }

    fn is_visible(&self, l: SceneLayer) -> bool {
        if let (true, SceneLayer::Copper(i)) = (self.single, l) {
            return i == self.active;
        }
        self.visible.get(&l).copied().unwrap_or(true)
    }

    fn fit_2d(&mut self, size_px: Vec2) {
        let Some(l) = &self.loaded else { return };
        let b = l.s2.bbox;
        self.center = [(b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0];
        let sx = (size_px.x as f64 * 0.86) / b.width().max(1.0);
        let sy = (size_px.y as f64 * 0.86) / b.height().max(1.0);
        self.ppm = sx.min(sy).clamp(0.5, 4000.0);
        self.fit_2d_at = Some((self.center, self.ppm, size_px));
        self.fitted = true;
        self.dirty = true;
    }

    fn fit_3d(&mut self, size_px: Vec2) {
        if let Some(g) = &self.g3d {
            let aspect = size_px.x / size_px.y.max(1.0);
            self.cam = Camera::frame_aspect(g.bounds.0, g.bounds.1, aspect);
            self.fit_3d_at = Some((self.cam, size_px));
            self.cam_fitted = true;
            self.dirty = true;
        }
    }

    /// The top or the bottom view in 3D, with the board fitted in it.
    fn look_straight(&mut self, from_top: bool, size_px: Vec2) {
        self.fit_3d(size_px);
        self.cam.look_straight(from_top);
        self.dirty = true;
    }

    fn zoom_to(&mut self, at: Point, span_mm: f64, size_px: Vec2) {
        self.mode = Mode::TwoD;
        self.center = [at.x, at.y];
        self.ppm = (size_px.x.min(size_px.y) as f64 / span_mm).clamp(0.5, 4000.0);
        self.dirty = true;
    }

    /// The item at a board point. The top layer comes first.
    fn pick(&self, p: Point) -> Option<Pick> {
        let l = self.loaded.as_ref()?;
        let n = l.s2.copper_layers;
        let mut best: Option<(u8, u8, Pick)> = None;
        for it in &l.analysis.geometry.copper {
            if !self.is_visible(SceneLayer::Copper(it.layer)) || !it.prim.contains(p) {
                continue;
            }
            let (rank, pick) = match &it.owner {
                Owner::Pad { part, pad, .. } => (
                    0,
                    Pick::Pad {
                        part: part.clone(),
                        pad: pad.clone(),
                        net: it.net.clone(),
                        layer: it.layer,
                    },
                ),
                Owner::Via { id } => (
                    1,
                    Pick::Via {
                        id: id.clone(),
                        net: it.net.clone().unwrap_or_default(),
                    },
                ),
                Owner::Hole { id } => (1, Pick::Hole { id: id.clone() }),
                Owner::Track { id, seg } => (
                    2,
                    Pick::Track {
                        id: id.clone(),
                        net: it.net.clone().unwrap_or_default(),
                        seg: *seg,
                    },
                ),
                _ => continue,
            };
            let key = (it.layer, rank);
            if best.as_ref().is_none_or(|b| (b.0, b.1) > key) {
                best = Some((key.0, key.1, pick));
            }
        }
        if let Some((_, _, p)) = best {
            return Some(p);
        }
        let d = &l.data;
        for side in [Side::Top, Side::Bottom] {
            for (r, pl) in &d.placement.placements {
                if pl.side != side {
                    continue;
                }
                let Some(fp) = d
                    .parts
                    .parts
                    .get(r)
                    .and_then(|x| d.footprints.get(&x.footprint))
                else {
                    continue;
                };
                if let Some(c) = lamina_geom::courtyard(fp, pl) {
                    if lamina_geom::prim::ring_contains(&c, p) {
                        return Some(Pick::Part { refdes: r.clone() });
                    }
                }
            }
        }
        // Pours come after parts. A click in a courtyard selects the part.
        for f in &l.analysis.fills {
            let Some(c) = f.layer.copper() else { continue };
            if !self.is_visible(SceneLayer::Copper(c.index(n))) {
                continue;
            }
            if f.polygons.iter().any(|poly| poly.prim().contains(p)) {
                return Some(Pick::Fill {
                    zone: f.zone.clone(),
                    net: f.net.clone(),
                });
            }
        }
        for h in &d.board.holes {
            if h.at.dist(p) <= h.drill.min_size() / 2.0 {
                return Some(Pick::Hole { id: h.id.clone() });
            }
        }
        let edge = 5.0 * self.mm_per_px;
        let near_edge = |c: &lamina_model::Contour| {
            let pts = c.flatten(0.02);
            (0..pts.len())
                .any(|i| lamina_geom::prim::seg_point(pts[i], pts[(i + 1) % pts.len()], p) <= edge)
        };
        if self.is_visible(SceneLayer::Keepout) {
            for k in &d.routing.keepouts {
                let pts = k.outline.flatten(0.02);
                if near_edge(&k.outline) || lamina_geom::prim::ring_contains(&pts, p) {
                    return Some(Pick::Keepout { id: k.id.clone() });
                }
            }
        }
        // Do the outline before the pours, because the edge of a pour is usually on it.
        if let Some(o) = &d.board.outline {
            if near_edge(o) {
                return Some(Pick::Outline);
            }
        }
        for z in &d.routing.zones {
            let visible = z.layers.iter().any(|l| {
                l.copper()
                    .is_some_and(|c| self.is_visible(SceneLayer::Copper(c.index(n))))
            });
            if visible && near_edge(&z.outline) {
                return Some(Pick::Fill {
                    zone: z.id.clone(),
                    net: z.net.clone(),
                });
            }
        }
        None
    }

    fn add_note(&mut self) {
        let text = self.note_text.trim().to_string();
        let Some(root) = &self.root else { return };
        if text.is_empty() {
            return;
        }
        let path = root.join("notes.json");
        let mut notes: NotesFile = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        notes.schema = lamina_model::SCHEMA;
        let next = notes
            .notes
            .iter()
            .filter_map(|n| n.id.strip_prefix('n')?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let refdes = self.sel.iter().find_map(|s| s.part()).map(str::to_string);
        notes.notes.push(lamina_model::Note {
            id: format!("n{next}"),
            text,
            at: self
                .note_at
                .map(|p| Point::new(json::round6(p.x), json::round6(p.y))),
            refdes,
            author: "user".into(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            resolved: false,
        });
        let res = json::to_string(&notes)
            .map_err(|e| e.to_string())
            .and_then(|s| write_atomic(&path, s.as_bytes()).map_err(|e| e.to_string()));
        match res {
            Ok(()) => {
                self.note_text.clear();
                self.note_status = Some((
                    format!("Sent as n{next}. The agent sees it with the notes tool."),
                    Instant::now(),
                ));
            }
            Err(e) => {
                self.note_status = Some((format!("Cannot save the note: {e}"), Instant::now()))
            }
        }
    }
}

// ---------------------------------------------------------------- drawing helpers

struct Xf {
    rect: ERect,
    center: [f64; 2],
    ppm_pt: f64,
}

impl Xf {
    fn to_screen(&self, p: Point) -> Pos2 {
        Pos2::new(
            self.rect.center().x + ((p.x - self.center[0]) * self.ppm_pt) as f32,
            self.rect.center().y - ((p.y - self.center[1]) * self.ppm_pt) as f32,
        )
    }

    fn to_board(&self, s: Pos2) -> Point {
        Point::new(
            self.center[0] + (s.x - self.rect.center().x) as f64 / self.ppm_pt,
            self.center[1] - (s.y - self.rect.center().y) as f64 / self.ppm_pt,
        )
    }

    fn rect(&self, r: &Rect) -> ERect {
        ERect::from_two_pos(self.to_screen(r.min), self.to_screen(r.max))
    }
}

fn draw_prim(painter: &egui::Painter, xf: &Xf, prim: &Prim, color: Color32, glow: bool) {
    match prim {
        Prim::Capsule { a, b, r } if glow => {
            let w = ((2.0 * r * xf.ppm_pt) as f32).max(2.0);
            painter.line_segment(
                [xf.to_screen(*a), xf.to_screen(*b)],
                Stroke::new(w + 3.0, color.gamma_multiply(0.35)),
            );
            painter.line_segment(
                [xf.to_screen(*a), xf.to_screen(*b)],
                Stroke::new(1.2, color),
            );
        }
        _ => {
            let pts: Vec<Pos2> = prim
                .outline(0.02)
                .into_iter()
                .map(|p| xf.to_screen(p))
                .collect();
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, color)));
        }
    }
}

// ---------------------------------------------------------------- app

impl App {
    /// Upkeep for each frame after the drawing. Returns true while something animates.
    fn animating(&mut self) -> bool {
        let animating = self
            .flashes
            .iter()
            .any(|(_, t)| t.elapsed() < Duration::from_millis(900))
            || self
                .note_status
                .as_ref()
                .is_some_and(|(_, t)| t.elapsed() < Duration::from_secs(4))
            || self
                .toast
                .as_ref()
                .is_some_and(|(_, _, t)| t.elapsed() < Duration::from_secs(4))
            || self.editor.as_ref().is_some_and(|e| e.busy > 0);
        self.flashes
            .retain(|(_, t)| t.elapsed() < Duration::from_millis(900));
        animating
    }

    fn recently_reloaded(&self) -> bool {
        self.last_reload
            .is_some_and(|t| t.elapsed() < Duration::from_secs(60))
    }

    /// Name shown on the board's tab.
    fn tab_name(&self) -> String {
        if let Some(l) = &self.loaded {
            return l.data.board.name.clone();
        }
        self.root
            .as_ref()
            .and_then(|r| std::path::absolute(r).ok())
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Open a board".into())
    }

    /// The health colour: error, warning, clean, or not loaded.
    fn health(&self) -> Color32 {
        let Some(l) = &self.loaded else {
            return TEXT_3;
        };
        let v = &l.analysis.violations;
        if v.iter().any(|v| v.severity == Severity::Error) {
            ERROR
        } else if !v.is_empty() {
            WARN
        } else {
            OK
        }
    }

    /// The DRC, routing and live chips, from right to left.
    fn chips(&self, ui: &mut egui::Ui) {
        let Some(l) = self.loaded.as_ref() else {
            return;
        };
        let errors = l
            .analysis
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count();
        let warnings = l.analysis.violations.len() - errors;
        let unrouted = l.analysis.ratsnest.len();
        widgets::live_dot(ui, self.last_reload);
        ui.add_space(6.0);
        if !l.generated.is_empty() {
            widgets::chip(ui, TEXT_2, "design.jsonnet");
        }
        widgets::chip(
            ui,
            if unrouted == 0 { OK } else { TEXT_2 },
            &if unrouted == 0 {
                "all routed".to_string()
            } else {
                format!("{unrouted} unrouted")
            },
        );
        let (c, text) = if errors > 0 {
            (
                ERROR,
                format!("{errors} error{}", if errors == 1 { "" } else { "s" }),
            )
        } else if warnings > 0 {
            (
                WARN,
                format!("{warnings} warning{}", if warnings == 1 { "" } else { "s" }),
            )
        } else {
            (OK, "DRC clean".to_string())
        };
        widgets::chip(ui, c, &text);
    }

    /// The strip above the canvas of a board: name and path, 2D/3D switch and
    /// close button. Returns true if the user clicked the close button.
    fn pane_header(&mut self, ui: &mut egui::Ui, closable: bool) -> bool {
        let mut close = false;
        // Add the controls on the right first. Then the name and path get the
        // remaining space. They are cut short and do not go under the controls.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            {
                ui.add_space(8.0);
                if closable
                    && ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("×").font(body(15.0)).color(TEXT_2),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Close this board")
                        .clicked()
                {
                    close = true;
                }
                if self.loaded.is_some() {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(108.0, 24.0), Sense::hover());
                    let mut sel = if self.mode == Mode::TwoD { 0 } else { 1 };
                    if widgets::segmented(ui, rect, &["2D", "3D"], &mut sel) {
                        self.mode = if sel == 0 { Mode::TwoD } else { Mode::ThreeD };
                        self.dirty = true;
                    }
                }
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, self.health());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(self.tab_name())
                            .font(semibold(13.0))
                            .color(TEXT),
                    )
                    .truncate(),
                );
                if let Some(root) = &self.root {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(root.display().to_string())
                                .font(body(11.5))
                                .color(TEXT_3),
                        )
                        .truncate(),
                    )
                    .on_hover_text(root.display().to_string());
                }
            });
        });
        close
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let mono_c = |s: String| egui::RichText::new(s).font(mono(11.5)).color(TEXT_2);
            match (self.mode, self.cursor_mm) {
                (Mode::TwoD, Some(p)) => {
                    ui.label(mono_c(format!("x {:>8.3}   y {:>8.3} mm", p.x, p.y)));
                }
                _ => {
                    ui.label(mono_c("x        –   y        – mm".into()));
                }
            }
            ui.add_space(16.0);
            if self.mode == Mode::TwoD {
                ui.label(mono_c(format!("{:.1} px/mm", self.ppm)));
            } else {
                ui.label(mono_c(format!(
                    "yaw {:.0}°  pitch {:.0}°",
                    self.cam.yaw, self.cam.pitch
                )));
            }
            if let Some(err) = &self.error {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(format!("Cannot reload: {err}"))
                        .font(body(12.0))
                        .color(ERROR),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(l) = &self.loaded {
                    let d = &l.data;
                    let mut items = vec![
                        format!("rebuilt in {} ms", l.took.as_millis()),
                        format!(
                            "{} parts · {} nets · {} tracks · {} vias",
                            d.parts.parts.len(),
                            d.nets.nets.len(),
                            d.routing.tracks.len(),
                            d.routing.vias.len()
                        ),
                    ];
                    if let Some(b) = d.board.outline.as_ref().and_then(|o| o.bbox()) {
                        items.push(format!(
                            "{:.1} × {:.1} mm · {} layers",
                            b.width(),
                            b.height(),
                            d.board.stackup.copper_layers
                        ));
                    }
                    // In a narrow window, show only the items that fit next to the
                    // coordinates; they must not write over them.
                    for (i, text) in items.into_iter().enumerate() {
                        let w = ui
                            .painter()
                            .layout_no_wrap(text.clone(), mono(11.5), TEXT_2)
                            .size()
                            .x;
                        if ui.available_width() < w + 24.0 {
                            break;
                        }
                        if i > 0 {
                            ui.add_space(14.0);
                        }
                        ui.label(mono_c(text));
                    }
                }
            });
        });
    }

    fn layer_panel(&mut self, ui: &mut egui::Ui) {
        let Some(l) = &self.loaded else { return };
        let n = l.s2.copper_layers;
        let groups: [(&str, Vec<SceneLayer>); 3] = [
            ("Copper", (0..n).map(SceneLayer::Copper).collect()),
            (
                "Technical",
                vec![
                    SceneLayer::Silk(Side::Top),
                    SceneLayer::Silk(Side::Bottom),
                    SceneLayer::Courtyard(Side::Top),
                    SceneLayer::Courtyard(Side::Bottom),
                    SceneLayer::Fab(Side::Top),
                    SceneLayer::Fab(Side::Bottom),
                    SceneLayer::Mask(Side::Top),
                    SceneLayer::Mask(Side::Bottom),
                    SceneLayer::Paste(Side::Top),
                    SceneLayer::Paste(Side::Bottom),
                ],
            ),
            (
                "Overlays",
                vec![
                    SceneLayer::Outline,
                    SceneLayer::Drills,
                    SceneLayer::Keepout,
                    SceneLayer::Ratsnest,
                    SceneLayer::Drc,
                ],
            ),
        ];
        let present: Vec<SceneLayer> = l.s2.layers.iter().map(|d| d.layer).collect();
        let count = |layer: SceneLayer| l.s2.layer(layer).map(|d| d.items.len()).unwrap_or(0);
        let mut changed = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // In a narrow panel, the title goes above the side buttons.
                let narrow = ui.available_width() < 200.0;
                if narrow {
                    widgets::section_title(ui, "Layers");
                }
                ui.horizontal(|ui| {
                    if !narrow {
                        widgets::section_title(ui, "Layers");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for (label, f) in [("Bottom", 2u8), ("Top", 1u8), ("All", 0u8)] {
                            if widgets::text_button(ui, label) {
                                for (k, v) in self.visible.iter_mut() {
                                    *v = match f {
                                        0 => default_visible(*k),
                                        1 => k.side(n) != Some(Side::Bottom) && default_visible(*k),
                                        _ => k.side(n) != Some(Side::Top) && default_visible(*k),
                                    };
                                }
                                match f {
                                    1 => self.active = 0,
                                    2 => self.active = n - 1,
                                    _ => {}
                                }
                                changed = true;
                            }
                        }
                    });
                });
                ui.add_space(6.0);
                for (gname, layers) in &groups {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(*gname).font(body(11.5)).color(TEXT_3));
                    ui.add_space(2.0);
                    for layer in layers {
                        if !matches!(layer, SceneLayer::Copper(_)) && !present.contains(layer) {
                            continue;
                        }
                        // The row shows what the canvas shows, also in single-layer mode.
                        let single_hides = self.single
                            && matches!(layer, SceneLayer::Copper(i) if *i != self.active);
                        let single_shows = self.single
                            && matches!(layer, SceneLayer::Copper(i) if *i == self.active);
                        let mut on = single_shows
                            || (!single_hides && self.visible.get(layer).copied().unwrap_or(true));
                        let color = rgba(lamina_scene::theme::layer_color(*layer, n));
                        if widgets::layer_row(ui, &layer.name(n), color, count(*layer), &mut on) {
                            match (self.single, layer) {
                                // In single-layer mode, a copper row picks the layer to show.
                                (true, SceneLayer::Copper(i)) => self.active = *i,
                                _ => {
                                    self.visible.insert(*layer, on);
                                }
                            }
                            changed = true;
                        }
                    }
                }
            });
        if changed {
            self.dirty = true;
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let size_hint = self.last_size_px;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                widgets::section_title(ui, "Inspector");
                ui.add_space(6.0);
                if !self.sel.is_empty() {
                    self.selection_panel(ui);
                }
                if self.sel.is_empty() {
                    match (&self.hover, &self.loaded) {
                        (Some(p), Some(l)) => self.pick_details(ui, p, &l.data),
                        _ => {
                            ui.label(
                                egui::RichText::new(
                                    "Hover over a pad, track or part to see its data. Click it to select and edit it.",
                                )
                                .font(body(12.5))
                                .color(TEXT_3),
                            );
                        }
                    }
                }
                self.unplaced_panel(ui);

                ui.add_space(18.0);
                widgets::hairline(ui);
                ui.add_space(12.0);
                let Some(l) = &self.loaded else { return };
                let mut violations: Vec<&lamina_model::Violation> =
                    l.analysis.violations.iter().collect();
                violations.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.rule.cmp(&b.rule)));
                ui.horizontal(|ui| {
                    widgets::section_title(ui, "Checks");
                    ui.label(
                        egui::RichText::new(format!("{}", violations.len()))
                            .font(mono(11.5))
                            .color(TEXT_3),
                    );
                });
                ui.add_space(6.0);
                if violations.is_empty() {
                    ui.label(
                        egui::RichText::new("There are no rule violations. All checks pass.")
                            .font(body(12.5))
                            .color(OK),
                    );
                }
                let mut goto = None;
                for v in violations.iter().take(200) {
                    let color = if v.severity == Severity::Error {
                        ERROR
                    } else {
                        WARN
                    };
                    if widgets::violation_row(ui, color, &v.rule, &v.message, v.at.is_some()) {
                        goto = v.at;
                    }
                }
                if let Some(at) = goto {
                    self.zoom_to(at, 8.0, size_hint);
                }

                ui.add_space(18.0);
                widgets::hairline(ui);
                ui.add_space(12.0);
                self.notes_section(ui);
            });
    }

    fn pick_details(&self, ui: &mut egui::Ui, p: &Pick, d: &ProjectData) {
        let mut rows: Vec<(&str, String)> = Vec::new();
        let title;
        match p {
            Pick::Pad {
                part,
                pad,
                net,
                layer,
            } => {
                title = format!("{part}.{pad}");
                if let Some(name) = d
                    .parts
                    .parts
                    .get(part)
                    .and_then(|x| d.footprints.get(&x.footprint))
                    .and_then(|fp| fp.pad(pad))
                    .and_then(|pd| pd.name.clone())
                {
                    rows.push(("pin", name));
                }
                rows.push(("net", net.clone().unwrap_or_else(|| "unconnected".into())));
                rows.push((
                    "layer",
                    Layer::Copper(lamina_model::Copper::from_index(
                        *layer,
                        d.board.stackup.copper_layers,
                    ))
                    .to_string(),
                ));
                if let Some(fp) = d
                    .parts
                    .parts
                    .get(part)
                    .and_then(|x| d.footprints.get(&x.footprint))
                {
                    if let Some(pd) = fp.pad(pad) {
                        let (w, h) = pd.shape.size();
                        rows.push(("size", format!("{w:.3} × {h:.3} mm")));
                        if let lamina_model::PadKind::Through { drill, plated } = &pd.kind {
                            rows.push((
                                "drill",
                                format!(
                                    "{:.3} mm {}",
                                    drill.min_size(),
                                    if *plated { "plated" } else { "NPTH" }
                                ),
                            ));
                        }
                    }
                }
                part_rows(d, part, &mut rows);
            }
            Pick::Part { refdes } => {
                title = refdes.clone();
                part_rows(d, refdes, &mut rows);
            }
            Pick::Track { id, net, seg } => {
                title = format!("Track {id}");
                rows.push(("net", net.clone()));
                if let Some(t) = d.routing.tracks.iter().find(|t| &t.id == id) {
                    let len: f64 = t.points.windows(2).map(|w| w[0].dist(w[1])).sum();
                    rows.push(("layer", t.layer.to_string()));
                    rows.push(("width", format!("{:.3} mm", t.width)));
                    rows.push(("length", format!("{len:.2} mm")));
                    rows.push((
                        "segment",
                        format!("{} of {}", seg + 1, t.points.len().saturating_sub(1)),
                    ));
                }
            }
            Pick::Via { id, net } => {
                title = format!("Via {id}");
                rows.push(("net", net.clone()));
                if let Some(v) = d.routing.vias.iter().find(|v| &v.id == id) {
                    rows.push(("at", format!("{:.3}, {:.3}", v.at.x, v.at.y)));
                    rows.push(("size", format!("{:.2} / {:.2} mm", v.diameter, v.drill)));
                }
            }
            Pick::Fill { zone, net } => {
                title = format!("Pour {zone}");
                rows.push(("net", net.clone().unwrap_or_else(|| "none".into())));
            }
            Pick::Keepout { id } => {
                title = format!("Keep-out {id}");
                if let Some(k) = d.routing.keepouts.iter().find(|k| &k.id == id) {
                    let ls: Vec<String> = k.layers.iter().map(|l| l.to_string()).collect();
                    rows.push(("layers", ls.join(", ")));
                }
            }
            Pick::Hole { id } => {
                title = format!("Hole {id}");
                if let Some(h) = d.board.holes.iter().find(|h| &h.id == id) {
                    rows.push(("at", format!("{:.3}, {:.3}", h.at.x, h.at.y)));
                    rows.push(("drill", format!("{:.2} mm", h.drill.min_size())));
                }
            }
            Pick::Outline => {
                title = "Board outline".to_string();
                if let Some(b) = d.board.outline.as_ref().and_then(|o| o.bbox()) {
                    rows.push(("size", format!("{:.2} × {:.2} mm", b.width(), b.height())));
                }
            }
        }
        ui.label(egui::RichText::new(title).font(semibold(16.0)).color(TEXT));
        ui.add_space(6.0);
        egui::Grid::new("kv")
            .num_columns(2)
            .spacing([14.0, 5.0])
            .show(ui, |ui| {
                for (k, v) in rows {
                    ui.label(egui::RichText::new(k).font(body(12.0)).color(TEXT_3));
                    let mono_like = v
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit() || c == '-');
                    let rt = egui::RichText::new(v.clone()).color(TEXT);
                    let rt = if mono_like {
                        rt.font(mono(12.0))
                    } else {
                        rt.font(body(12.5))
                    };
                    // Cut long values short. Do not make the panel wider.
                    ui.add(egui::Label::new(rt).truncate()).on_hover_text(v);
                    ui.end_row();
                }
            });
    }

    fn notes_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "Notes for the agent");
        ui.add_space(6.0);
        if let Some(l) = &self.loaded {
            let open: Vec<&lamina_model::Note> = l.data.notes.notes.iter().rev().take(12).collect();
            for n in open {
                widgets::note_row(ui, n);
            }
        }
        ui.add_space(6.0);
        let anchor = match (self.sel.iter().find_map(|s| s.part()), self.note_at) {
            (Some(r), _) => format!("Attached to {r}"),
            (None, Some(p)) => format!("Attached to ({:.2}, {:.2}) mm", p.x, p.y),
            _ => "Click the board to attach a location".to_string(),
        };
        ui.label(egui::RichText::new(anchor).font(body(11.5)).color(TEXT_3));
        ui.add(
            egui::TextEdit::multiline(&mut self.note_text)
                .hint_text("Example: move C3 nearer to U1 pin 1")
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .font(body(13.0)),
        );
        ui.add_space(4.0);
        let enabled = !self.note_text.trim().is_empty();
        if widgets::primary_button(ui, "Send to agent", enabled) {
            self.add_note();
        }
        if let Some((msg, t)) = &self.note_status {
            if t.elapsed() < Duration::from_secs(4) {
                ui.label(egui::RichText::new(msg).font(body(11.5)).color(TEXT_2));
            }
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, CANVAS_EDGE);
        let ctx = ui.ctx().clone();
        let mut open = None;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(ERect::from_center_size(rect.center(), Vec2::new(460.0, 300.0))),
            |ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Open a board").font(semibold(22.0)).color(TEXT));
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "A Lamina project folder, or a KiCad, EasyEDA or Gerber file (the viewer imports it). \
                             The viewer draws the board again when the agent changes it. Ctrl+O also opens a file. \
                             You can also drop folders or files on the window.",
                        )
                        .font(body(13.5))
                        .color(TEXT_2),
                    );
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, "Open board file…", true) {
                            self.request = Some(OpenRequest::PickFile);
                        }
                        ui.add_space(6.0);
                        if ui.button("Open project folder…").clicked() {
                            self.request = Some(OpenRequest::PickFolder);
                        }
                    });
                    ui.add_space(10.0);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.open_path)
                            .hint_text("or type a path and press Enter")
                            .desired_width(f32::INFINITY)
                            .font(mono(13.0)),
                    );
                    if edit.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !self.open_path.trim().is_empty()
                    {
                        open = Some(PathBuf::from(self.open_path.trim()));
                    }
                    if let Some(e) = &self.error {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(e).font(body(12.5)).color(ERROR));
                    }
                });
            },
        );
        let _ = ctx;
        if let Some(p) = open {
            self.request = Some(OpenRequest::Path(p));
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui, rs: &egui_wgpu::RenderState) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let w = (rect.width() * ppp).round().max(1.0) as u32;
        let h = (rect.height() * ppp).round().max(1.0) as u32;
        let size_px = Vec2::new(w as f32, h as f32);
        self.last_size_px = size_px;
        if self.gpu.is_none() {
            return;
        }

        let resized = self
            .target
            .as_ref()
            .is_none_or(|t| t.width != w || t.height != h);
        if resized {
            let t = self.gpu.as_ref().unwrap().target(w, h);
            let mut r = rs.renderer.write();
            match self.tex {
                None => {
                    self.tex =
                        Some(r.register_native_texture(&rs.device, &t.resolve_view, wgpu_filter()))
                }
                Some(id) => r.update_egui_texture_from_wgpu_texture(
                    &rs.device,
                    &t.resolve_view,
                    wgpu_filter(),
                    id,
                ),
            }
            self.target = Some(t);
            self.dirty = true;
        }
        // If the window or pane changes size and the view is as fitted, fit again.
        if let Some((c, p, s)) = self.fit_2d_at {
            if s != size_px && c == self.center && p == self.ppm {
                self.fitted = false;
            }
        }
        if let Some((cam, s)) = self.fit_3d_at {
            let same = cam.target == self.cam.target
                && cam.yaw == self.cam.yaw
                && cam.pitch == self.cam.pitch
                && cam.distance == self.cam.distance;
            if s != size_px && same {
                self.cam_fitted = false;
            }
        }
        if !self.fitted {
            self.fit_2d(size_px);
        }
        if !self.cam_fitted {
            self.fit_3d(size_px);
        }

        // Input.
        let (scroll, zoom_gesture, hover_pos, keys) = ui.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.pointer.hover_pos(),
                (
                    i.key_pressed(egui::Key::F),
                    i.key_pressed(egui::Key::Num1),
                    i.key_pressed(egui::Key::Num2),
                ),
            )
        });
        let focus_free = self.focused && !ui.ctx().egui_wants_keyboard_input();
        self.global_keys(ui);
        if focus_free {
            if keys.1 {
                self.mode = Mode::TwoD;
                self.dirty = true;
            }
            if keys.2 {
                self.mode = Mode::ThreeD;
                self.dirty = true;
            }
            if keys.0 && self.mode == Mode::ThreeD {
                self.fit_3d(size_px);
            }
        }
        let inside = hover_pos.is_some_and(|p| rect.contains(p));
        let xf = Xf {
            rect,
            center: self.center,
            ppm_pt: self.ppm / ppp as f64,
        };
        match self.mode {
            Mode::TwoD => {
                // + and - zoom at the pointer (or at the centre), as the wheel does.
                let key_zoom = if focus_free {
                    ui.input(|i| {
                        let plus =
                            i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals);
                        let minus = i.key_pressed(egui::Key::Minus);
                        if i.modifiers.command {
                            1.0
                        } else if plus {
                            1.3
                        } else if minus {
                            1.0 / 1.3
                        } else {
                            1.0
                        }
                    })
                } else {
                    1.0
                };
                if key_zoom != 1.0 || (inside && (scroll != 0.0 || zoom_gesture != 1.0)) {
                    let pos = hover_pos
                        .filter(|p| rect.contains(*p))
                        .unwrap_or(rect.center());
                    let before = xf.to_board(pos);
                    let factor = (scroll as f64 * 0.0018).exp() * zoom_gesture as f64 * key_zoom;
                    self.ppm = (self.ppm * factor).clamp(0.5, 4000.0);
                    let xf2 = Xf {
                        rect,
                        center: self.center,
                        ppm_pt: self.ppm / ppp as f64,
                    };
                    let after = xf2.to_board(pos);
                    self.center[0] += before.x - after.x;
                    self.center[1] += before.y - after.y;
                    self.dirty = true;
                }
                // Pan with the right or middle button, or Space + left drag.
                let space = ui.input(|i| i.key_down(egui::Key::Space))
                    && !ui.ctx().egui_wants_keyboard_input();
                let panning = resp.dragged_by(egui::PointerButton::Secondary)
                    || resp.dragged_by(egui::PointerButton::Middle)
                    || (space && resp.dragged_by(egui::PointerButton::Primary));
                if panning {
                    let d = resp.drag_delta();
                    self.center[0] -= d.x as f64 / xf.ppm_pt;
                    self.center[1] += d.y as f64 / xf.ppm_pt;
                    self.dirty = true;
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
                let xf = Xf {
                    rect,
                    center: self.center,
                    ppm_pt: self.ppm / ppp as f64,
                };
                self.mm_per_px = 1.0 / xf.ppm_pt;
                self.cursor_mm = hover_pos
                    .filter(|p| rect.contains(*p))
                    .map(|p| xf.to_board(p));
                let busy = !matches!(self.gesture, Gesture::None) || panning;
                let hover = if busy {
                    None
                } else {
                    self.cursor_mm.and_then(|p| self.pick(p))
                };
                if hover != self.hover {
                    self.hover = hover;
                }
                if !space {
                    self.edit_input(ui, &resp, &xf, size_px);
                }
                if std::mem::take(&mut self.fit_pending) {
                    self.fit_2d(size_px);
                }
                if self.tool != Tool::Select || self.placing.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                } else if matches!(self.gesture, Gesture::Drag { .. } | Gesture::Grab { .. }) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                }
            }
            Mode::ThreeD => {
                self.cursor_mm = None;
                self.hover = None;
                if inside && (scroll != 0.0 || zoom_gesture != 1.0) {
                    let factor = (-(scroll * 0.0018)).exp() / zoom_gesture;
                    // Zoom toward the point of the board plane under the pointer, as in
                    // other viewers. If that point is far away (a low camera), zoom
                    // toward the centre of the view.
                    let pos = hover_pos.unwrap();
                    let ndc = [
                        (pos.x - rect.left()) / rect.width() * 2.0 - 1.0,
                        1.0 - (pos.y - rect.top()) / rect.height() * 2.0,
                    ];
                    let aspect = rect.width() / rect.height().max(1.0);
                    let cam = self.cam;
                    let near = |at: &[f32; 3]| {
                        let d: f32 = (0..3).map(|i| (at[i] - cam.target[i]).powi(2)).sum();
                        d.sqrt() < 3.0 * cam.distance
                    };
                    match self
                        .cam
                        .hit_plane(aspect, ndc, self.cam.target[2])
                        .filter(near)
                    {
                        Some(at) => self.cam.zoom_about(at, factor),
                        None => self.cam.distance = (self.cam.distance * factor).clamp(2.0, 5000.0),
                    }
                    self.dirty = true;
                }
                if focus_free {
                    let k = |key| ui.input(|i| i.key_pressed(key) && !i.modifiers.command);
                    if k(egui::Key::T) {
                        self.look_straight(true, size_px);
                        self.dirty = true;
                    }
                    if k(egui::Key::B) {
                        self.look_straight(false, size_px);
                        self.dirty = true;
                    }
                    if k(egui::Key::I) {
                        self.fit_3d(size_px);
                    }
                    if k(egui::Key::P) {
                        self.models = !self.models;
                        self.dirty = true;
                    }
                }
                if resp.dragged_by(egui::PointerButton::Primary) {
                    let d = resp.drag_delta();
                    self.cam.yaw -= d.x * 0.35;
                    self.cam.pitch = (self.cam.pitch + d.y * 0.35).clamp(-89.0, 89.9);
                    self.dirty = true;
                }
                if resp.dragged_by(egui::PointerButton::Secondary)
                    || resp.dragged_by(egui::PointerButton::Middle)
                {
                    let d = resp.drag_delta();
                    let k = self.cam.distance * 0.0016;
                    let (sy, cy) = self.cam.yaw.to_radians().sin_cos();
                    let (sp, cp) = self.cam.pitch.to_radians().sin_cos();
                    // The right and up vectors of the camera. A drag moves the target in
                    // the view plane.
                    let fwd = [-cp * cy, -cp * sy, -sp];
                    let right = {
                        let r = [fwd[1], -fwd[0], 0.0];
                        let l = (r[0] * r[0] + r[1] * r[1]).sqrt().max(1e-6);
                        [r[0] / l, r[1] / l, 0.0]
                    };
                    let up = [
                        right[1] * fwd[2] - right[2] * fwd[1],
                        right[2] * fwd[0] - right[0] * fwd[2],
                        right[0] * fwd[1] - right[1] * fwd[0],
                    ];
                    for i in 0..3 {
                        self.cam.target[i] += (-right[i] * d.x + up[i] * d.y) * k;
                    }
                    self.dirty = true;
                }
                if resp.double_clicked() {
                    self.fit_3d(size_px);
                }
            }
        }

        if self.dirty && self.mode == Mode::TwoD {
            // Copper on other layers than the active one is dimmer. Upload again
            // when the active layer, or the layers that show, change.
            if let Some(l) = &self.loaded {
                let bright = self.bright_copper_of(&l.s2);
                if bright != self.g2d_bright {
                    if let Some(gpu) = &self.gpu {
                        self.g2d = Some(gpu.upload_2d(&l.s2, 40.0, bright));
                    }
                    self.g2d_bright = bright;
                }
            }
        }
        if self.dirty {
            let gpu = self.gpu.as_ref().unwrap();
            if let Some(t) = &self.target {
                match self.mode {
                    Mode::TwoD => {
                        if let Some(g) = &self.g2d {
                            let vis = |l: SceneLayer| self.is_visible(l);
                            gpu.draw_2d(t, g, self.center, self.ppm, &vis);
                        }
                    }
                    Mode::ThreeD => {
                        if let Some(g) = &self.g3d {
                            gpu.draw_3d_with(t, g, &self.cam, self.models);
                        }
                    }
                }
            }
            self.dirty = false;
        }
        let painter = ui.painter_at(rect);
        if let Some(id) = self.tex {
            painter.image(
                id,
                rect,
                ERect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        if self.mode == Mode::TwoD {
            let xf = Xf {
                rect,
                center: self.center,
                ppm_pt: self.ppm / ppp as f64,
            };
            self.overlays(&painter, &xf);
            self.edit_overlays(&painter, &xf);
            self.measure_overlay(&painter, &xf);
            widgets::hint(&painter, rect, self.tool_hint());
            self.toolbar(ui, rect);
        } else {
            widgets::hint(
                &painter,
                rect,
                "Drag to orbit · right-drag to pan · scroll to zoom at the pointer · T top · B bottom · I or F reset · P parts",
            );
            // The view buttons, and the Board menu, which works from 3D too.
            let bar = ERect::from_min_size(
                rect.left_top() + Vec2::new(12.0, 12.0),
                Vec2::new(rect.width() - 24.0, 36.0),
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
                    ui.horizontal(|ui| {
                        let top = self.cam.pitch > 89.0;
                        let bottom = self.cam.pitch < -88.9;
                        let compact = rect.width() < 900.0;
                        let name = |s: &'static str| if compact { "" } else { s };
                        if widgets::tool_button(ui, name("Top"), "T", top).clicked() {
                            self.look_straight(true, size_px);
                            self.dirty = true;
                        }
                        if widgets::tool_button(ui, name("Bottom"), "B", bottom).clicked() {
                            self.look_straight(false, size_px);
                            self.dirty = true;
                        }
                        if widgets::tool_button(ui, name("Reset"), "I", false).clicked() {
                            self.fit_3d(size_px);
                        }
                        widgets::vdivider(ui);
                        if widgets::tool_button(ui, name("Parts"), "P", self.models)
                            .on_hover_text("Show or hide the part bodies")
                            .clicked()
                        {
                            self.models = !self.models;
                            self.dirty = true;
                        }
                        widgets::vdivider(ui);
                        ui.add_space(4.0);
                        self.board_menu(ui);
                    });
                });
        }
        // Toasts show in both views. A "…" message stays while its work runs.
        let working = self.editor.as_ref().is_some_and(|e| e.busy > 0) || !self.bg.is_empty();
        if let Some((msg, ok, t)) = &self.toast {
            let held = working && msg.ends_with('…');
            let age = if held { 0.0 } else { t.elapsed().as_secs_f32() };
            if age < 4.0 {
                let a = if age > 3.4 { (4.0 - age) / 0.6 } else { 1.0 };
                widgets::toast(
                    &painter,
                    Pos2::new(rect.center().x, rect.top() + 72.0),
                    msg,
                    if *ok { OK } else { ERROR },
                    a,
                );
            }
        }
    }

    fn overlays(&self, painter: &egui::Painter, xf: &Xf) {
        let Some(l) = &self.loaded else { return };
        if let Some(h) = &self.hl {
            let n = l.data.board.stackup.copper_layers;
            h.draw(
                painter,
                xf.rect,
                |p| xf.to_screen(p),
                |i| {
                    let layer = SceneLayer::Copper(i);
                    self.is_visible(layer).then(|| {
                        let c = lamina_scene::theme::layer_color(layer, n);
                        Color32::from_rgb(c[0], c[1], c[2])
                    })
                },
                self.g2d_bright,
            );
            widgets::banner(
                painter,
                xf.rect,
                &format!("Net {} · {} items · ` or Esc to clear", h.net, h.items),
                0,
            );
        }
        if self.single {
            let n = l.data.board.stackup.copper_layers;
            widgets::banner(
                painter,
                xf.rect,
                &format!(
                    "Single layer: {} · Shift+S to show all",
                    SceneLayer::Copper(self.active).name(n)
                ),
                if self.hl.is_some() { 1 } else { 0 },
            );
        }
        self.pad_labels(painter, xf, &l.data);
        self.track_labels(painter, xf, &l.data);
        // Outline the net under the cursor in the accent colour.
        let focus = self.hover.as_ref();
        if let Some(net) = focus.and_then(|p| p.net()) {
            for it in &l.analysis.geometry.copper {
                if it.net.as_deref() == Some(net) && self.is_visible(SceneLayer::Copper(it.layer)) {
                    draw_prim(painter, xf, &it.prim, ACCENT, true);
                }
            }
        }
        if let Some(part) = focus.and_then(|p| p.part()) {
            let d = &l.data;
            if let (Some(pl), Some(fp)) = (
                d.placement.placements.get(part),
                d.parts
                    .parts
                    .get(part)
                    .and_then(|x| d.footprints.get(&x.footprint)),
            ) {
                if let Some(c) = lamina_geom::courtyard(fp, pl) {
                    let pts: Vec<Pos2> = c.into_iter().map(|p| xf.to_screen(p)).collect();
                    painter.add(egui::Shape::closed_line(
                        pts,
                        Stroke::new(1.2, ACCENT.gamma_multiply(0.55)),
                    ));
                }
            }
        }
        // Flash the items that the agent changed.
        for (r, t) in &self.flashes {
            let k = (t.elapsed().as_secs_f32() / 0.9).clamp(0.0, 1.0);
            let a = (1.0 - k).powi(2);
            let grow = 2.0 + 6.0 * k;
            let sr = xf.rect(r).expand(grow);
            painter.rect_filled(sr, CornerRadius::same(4), ACCENT.gamma_multiply(0.10 * a));
            painter.rect_stroke(
                sr,
                CornerRadius::same(4),
                Stroke::new(1.5, ACCENT.gamma_multiply(a)),
                StrokeKind::Outside,
            );
        }
        if let Some(p) = self.note_at {
            let s = xf.to_screen(p);
            painter.circle_stroke(s, 5.0, Stroke::new(1.5, ACCENT));
            painter.circle_filled(s, 1.8, ACCENT);
        }
        // Scale bar.
        let mm_per_100 = 100.0 / xf.ppm_pt;
        let nice = [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]
            .into_iter()
            .find(|v| *v >= mm_per_100 * 0.6)
            .unwrap_or(100.0);
        let len = (nice * xf.ppm_pt) as f32;
        let base = xf.rect.left_bottom() + Vec2::new(18.0, -18.0);
        painter.line_segment([base, base + Vec2::new(len, 0.0)], Stroke::new(1.5, TEXT_2));
        for x in [0.0, len] {
            painter.line_segment(
                [base + Vec2::new(x, -4.0), base + Vec2::new(x, 0.0)],
                Stroke::new(1.5, TEXT_2),
            );
        }
        painter.text(
            base + Vec2::new(len + 8.0, -1.0),
            Align2::LEFT_CENTER,
            format!("{nice} mm"),
            mono(11.0),
            TEXT_2,
        );
    }
}

impl App {
    /// Highlight a net, or clear the highlight with `None`.
    pub(crate) fn highlight_net(&mut self, net: Option<String>) {
        self.hl = match (net, &self.loaded) {
            (Some(n), Some(l)) => Some(highlight::NetHighlight::new(
                n,
                &l.analysis.geometry.copper,
                &l.analysis.fills,
                l.data.board.stackup.copper_layers,
            )),
            _ => None,
        };
        self.dirty = true;
    }

    /// The copper layer to draw at full strength: the active layer, or the
    /// first copper layer that shows if the active layer is hidden.
    fn bright_copper_of(&self, s2: &lamina_scene::Scene2D) -> u8 {
        let shows = |i: u8| self.is_visible(SceneLayer::Copper(i));
        if shows(self.active) {
            return self.active;
        }
        (0..s2.copper_layers)
            .find(|&i| shows(i))
            .unwrap_or(self.active)
    }

    /// The net name on each track, along its longest segment on the screen,
    /// when the zoom gives it room.
    fn track_labels(&self, painter: &egui::Painter, xf: &Xf, d: &ProjectData) {
        let n = d.board.stackup.copper_layers;
        let advance = painter
            .layout_no_wrap("0000000000".into(), mono(100.0), Color32::WHITE)
            .size()
            .x
            / 1000.0;
        let clip = painter.clip_rect();
        for t in &d.routing.tracks {
            let Some(c) = t.layer.copper() else { continue };
            let layer = c.index(n);
            if !self.is_visible(SceneLayer::Copper(layer)) {
                continue;
            }
            if let Some(h) = &self.hl {
                if h.net != t.net {
                    continue;
                }
            }
            let w_px = (t.width * xf.ppm_pt) as f32;
            let size = (w_px * 0.72).min(11.0);
            if size < labels::MIN_NUMBER {
                continue;
            }
            let Some((a, b)) = t
                .points
                .windows(2)
                .map(|w| (xf.to_screen(w[0]), xf.to_screen(w[1])))
                .max_by(|x, y| (x.1 - x.0).length().total_cmp(&(y.1 - y.0).length()))
            else {
                continue;
            };
            let text_w = t.net.chars().count() as f32 * advance * size;
            if (b - a).length() < text_w + 4.0 * w_px {
                continue;
            }
            let mid = a + (b - a) * 0.5;
            if !clip.contains(mid) {
                continue;
            }
            let ink = if layer == self.g2d_bright {
                NAME_INK
            } else {
                NAME_INK.gamma_multiply(0.6)
            };
            let g = painter.layout_no_wrap(t.net.clone(), mono(size), ink);
            label(painter, mid, labels::upright((b - a).angle()), g);
        }
    }

    /// Pad numbers and pin names on the pads, when the zoom gives them room.
    fn pad_labels(&self, painter: &egui::Painter, xf: &Xf, d: &ProjectData) {
        let n = d.board.stackup.copper_layers;
        // The width of one monospace character, as a factor of the font size.
        let advance = painter
            .layout_no_wrap("0000000000".into(), mono(100.0), Color32::WHITE)
            .size()
            .x
            / 1000.0;
        // A pad without a pin name shows its net, as in other editors.
        let mut net_of: std::collections::HashMap<(&str, &str), &str> = Default::default();
        for (name, net) in &d.nets.nets {
            for pr in &net.pads {
                net_of.insert((pr.part.as_str(), pr.pad.as_str()), name.as_str());
            }
        }
        for (refdes, pl) in &d.placement.placements {
            let Some(fp) = d
                .parts
                .parts
                .get(refdes)
                .and_then(|p| d.footprints.get(&p.footprint))
            else {
                continue;
            };
            let copper = match pl.side {
                Side::Top => lamina_model::Copper::Top,
                Side::Bottom => lamina_model::Copper::Bottom,
            };
            // No work for a part that is not on the screen.
            let near = painter.clip_rect().expand(100.0);
            if !fp
                .pads
                .iter()
                .any(|pad| near.contains(xf.to_screen(pl.transform(pad.at))))
            {
                continue;
            }
            let side_shows = self.is_visible(SceneLayer::Copper(copper.index(n)));
            let any_shows = (0..n).any(|i| self.is_visible(SceneLayer::Copper(i)));
            let views: Vec<(&lamina_model::Pad, labels::PadView)> = fp
                .pads
                .iter()
                .filter(|pad| !pad.number.is_empty())
                .filter(|pad| match pad.kind {
                    lamina_model::PadKind::Through { .. } => any_shows,
                    lamina_model::PadKind::Smd => side_shows,
                })
                .map(|pad| {
                    let (w, h) = pad.shape.size();
                    let view = labels::PadView {
                        along_x: (w * xf.ppm_pt) as f32,
                        along_y: (h * xf.ppm_pt) as f32,
                        // Board angles turn counter-clockwise; the screen Y axis points down.
                        x_angle: -(pl.transform_angle(pad.rotation) as f32).to_radians(),
                    };
                    (pad, view)
                })
                .collect();
            // Pads of the same size get labels of the same size: the size
            // that fits the longest label among them.
            let key = |pad: &lamina_model::Pad| {
                let (w, h) = pad.shape.size();
                let um = |v: f64| (v * 1000.0).round() as i64;
                (um(w), um(h), um(pad.rotation))
            };
            let mut cap: BTreeMap<(i64, i64, i64), f32> = BTreeMap::new();
            for (pad, view) in &views {
                if let Some(size) = labels::comfortable(
                    *view,
                    &pad.number,
                    pad.name
                        .as_deref()
                        .or(net_of.get(&(refdes.as_str(), pad.number.as_str())).copied()),
                    advance,
                ) {
                    let c = cap.entry(key(pad)).or_insert(size);
                    *c = c.min(size);
                }
            }
            for (pad, view) in views {
                let Some(&max) = cap.get(&key(pad)) else {
                    continue;
                };
                let Some(fit) = labels::fit(
                    view,
                    &pad.number,
                    pad.name
                        .as_deref()
                        .or(net_of.get(&(refdes.as_str(), pad.number.as_str())).copied()),
                    advance,
                    max,
                ) else {
                    continue;
                };
                let c = xf.to_screen(pl.transform(pad.at));
                let font = mono(fit.size);
                let gal = |text: &str, color: Color32| {
                    painter.layout_no_wrap(text.to_string(), font.clone(), color)
                };
                match (fit.arrange, fit.name.as_deref()) {
                    (labels::Arrange::Stacked, Some(name)) => {
                        // One line above the centre line of the pad, one below.
                        let step = Vec2::angled(fit.angle + std::f32::consts::FRAC_PI_2)
                            * (fit.size * labels::LINE * 0.5);
                        label(painter, c - step, fit.angle, gal(&pad.number, NUMBER_INK));
                        label(painter, c + step, fit.angle, gal(name, NAME_INK));
                    }
                    (labels::Arrange::Inline, Some(name)) => {
                        let mut job = egui::text::LayoutJob::default();
                        let fmt = |color| egui::TextFormat::simple(font.clone(), color);
                        job.append(&pad.number, 0.0, fmt(NUMBER_INK));
                        job.append(name, fit.size * 0.6, fmt(NAME_INK));
                        label(painter, c, fit.angle, painter.layout_job(job));
                    }
                    _ => label(painter, c, fit.angle, gal(&pad.number, NUMBER_INK)),
                }
            }
        }
    }
}

/// The colour of a pad number on copper.
const NUMBER_INK: Color32 = Color32::from_rgb(0xFA, 0xF7, 0xF2);
/// The colour of a pin name on copper.
const NAME_INK: Color32 = Color32::from_rgb(0xFF, 0xE2, 0xC4);

/// Text centred on `c`, turned by `angle`, with a dark edge so that it can
/// be read on any copper colour.
fn label(painter: &egui::Painter, c: Pos2, angle: f32, galley: std::sync::Arc<egui::Galley>) {
    let pos = c - galley.rect.center().to_vec2();
    let shape = |at: Pos2| {
        egui::epaint::TextShape::new(at, galley.clone(), NUMBER_INK)
            .with_angle_and_anchor(angle, Align2::CENTER_CENTER)
    };
    let edge = Color32::from_black_alpha(190);
    for d in [
        Vec2::new(1.0, 0.0),
        Vec2::new(-1.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(0.0, -1.0),
    ] {
        painter.add(shape(pos + d).with_override_text_color(edge));
    }
    painter.add(shape(pos));
}

fn wgpu_filter() -> egui_wgpu::wgpu::FilterMode {
    egui_wgpu::wgpu::FilterMode::Linear
}

fn part_rows(d: &ProjectData, refdes: &str, rows: &mut Vec<(&str, String)>) {
    if let Some(p) = d.parts.parts.get(refdes) {
        if let Some(v) = &p.value {
            rows.push(("value", v.clone()));
        }
        if let Some(m) = &p.mpn {
            rows.push(("mpn", m.clone()));
        }
        if let Some(m) = &p.manufacturer {
            rows.push(("maker", m.clone()));
        }
        if let Some(c) = &p.lcsc {
            rows.push(("lcsc", c.clone()));
        }
        rows.push(("footprint", p.footprint.clone()));
        if let Some(g) = &p.group {
            rows.push(("group", g.clone()));
        }
    }
    if let Some(pl) = d.placement.placements.get(refdes) {
        rows.push(("at", format!("{:.3}, {:.3}", pl.x, pl.y)));
        rows.push(("rotation", format!("{}°", pl.rotation)));
        rows.push(("side", pl.side.as_str().to_string()));
    }
}

/// The end of a long path. It counts characters, so it never cuts a non-ASCII
/// folder name (`Masaüstü`) in a character.
fn short_path(p: &Path) -> String {
    let s = p.display().to_string();
    let n = s.chars().count();
    if n > 48 {
        format!("…{}", s.chars().skip(n - 46).collect::<String>())
    } else {
        s
    }
}

/// The window: several boards side by side. Side panels, the status bar
/// and keyboard shortcuts belong to the focused board.
struct Shell {
    boards: Vec<App>,
    focus: usize,
    three_d: bool,
    add_path: String,
    add_error: Option<(String, Instant)>,
    /// File dialogs and imports that run in the background.
    jobs: Vec<Job>,
    /// Project folders to open on the next frame.
    pending_open: Vec<PathBuf>,
    title: String,
    remembered: Vec<PathBuf>,
    /// False when the viewer runs without a window: do not change the
    /// list of boards that the next start opens.
    remember: bool,
    panes_seen: usize,
}

fn remembered_boards() -> Vec<PathBuf> {
    config_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|s| {
            s.lines()
                .map(|l| PathBuf::from(l.trim()))
                .filter(|p| ProjectData::is_project(p))
                .collect()
        })
        .unwrap_or_default()
}

/// True if two paths name the same folder, also if they are written differently.
fn same_path(a: &Path, b: &Path) -> bool {
    let abs = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    abs(a) == abs(b)
}

/// The project root of a dropped folder, or of a file in a project folder.
fn project_root(p: &Path) -> Option<PathBuf> {
    if ProjectData::is_project(p) {
        return Some(p.to_path_buf());
    }
    p.parent()
        .filter(|d| ProjectData::is_project(d))
        .map(Path::to_path_buf)
}

fn gpu_of(rs: Option<&egui_wgpu::RenderState>) -> Option<Gpu> {
    rs.map(|rs| Gpu::new(rs.device.clone(), rs.queue.clone()))
}

impl Shell {
    fn new(cc: &eframe::CreationContext<'_>, roots: Vec<PathBuf>, three_d: bool) -> Shell {
        style::install(&cc.egui_ctx);
        let roots = if roots.is_empty() {
            remembered_boards()
        } else {
            roots
        };
        let mut s = Shell {
            boards: Vec::new(),
            focus: 0,
            three_d,
            add_path: String::new(),
            add_error: None,
            jobs: Vec::new(),
            pending_open: Vec::new(),
            title: String::new(),
            remembered: Vec::new(),
            remember: true,
            panes_seen: 0,
        };
        let rs = cc.wgpu_render_state.as_ref();
        for r in roots {
            if ProjectData::is_project(&r) {
                s.add(r, &cc.egui_ctx, rs);
            } else {
                // A board file or a file in a project. Import or resolve it in
                // the background, as with files that open later.
                s.request(OpenRequest::Path(r), &cc.egui_ctx);
            }
        }
        if s.boards.is_empty() {
            s.boards
                .push(App::new(gpu_of(rs), None, three_d, &cc.egui_ctx));
        }
        s.focus = 0;
        s
    }

    fn add(&mut self, root: PathBuf, ctx: &egui::Context, rs: Option<&egui_wgpu::RenderState>) {
        if let Some(i) = self
            .boards
            .iter()
            .position(|b| b.root.as_deref().is_some_and(|x| same_path(x, &root)))
        {
            self.focus = i;
            return;
        }
        let b = App::new(gpu_of(rs), Some(root), self.three_d, ctx);
        // The first real board replaces an empty placeholder pane.
        if self.boards.len() == 1 && self.boards[0].root.is_none() {
            self.boards.clear();
        }
        self.boards.push(b);
        self.focus = self.boards.len() - 1;
    }

    fn close(&mut self, i: usize, rs: Option<&egui_wgpu::RenderState>, ctx: &egui::Context) {
        let b = self.boards.remove(i);
        if let (Some(id), Some(rs)) = (b.tex, rs) {
            rs.renderer.write().free_texture(&id);
        }
        if self.boards.is_empty() {
            self.boards
                .push(App::new(gpu_of(rs), None, self.three_d, ctx));
        }
        self.focus = self.focus.min(self.boards.len() - 1);
    }

    /// Make the window title and the list of remembered boards match the open boards.
    fn sync_title_and_memory(&mut self, ctx: &egui::Context) {
        let names: Vec<String> = self
            .boards
            .iter()
            .filter(|b| b.root.is_some())
            .map(App::tab_name)
            .collect();
        let title = if names.is_empty() {
            "Lamina".to_string()
        } else {
            format!("{} - Lamina", names.join(" · "))
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        let roots: Vec<PathBuf> = self.boards.iter().filter_map(|b| b.root.clone()).collect();
        if self.remember && roots != self.remembered && !roots.is_empty() {
            if let Some(f) = config_file() {
                let _ = std::fs::create_dir_all(f.parent().unwrap());
                let text: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
                let _ = std::fs::write(f, text.join("\n"));
            }
            self.remembered = roots;
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, rs: Option<&egui_wgpu::RenderState>) {
        let ctx = ui.ctx().clone();
        let mut close = None;
        ui.horizontal_centered(|ui| {
            widgets::wordmark(ui);
            ui.add_space(14.0);
            let many = self.boards.len() > 1;
            for (i, b) in self.boards.iter().enumerate() {
                if b.root.is_none() {
                    continue;
                }
                let text = egui::RichText::new(b.tab_name())
                    .font(semibold(13.0))
                    .color(if i == self.focus { TEXT } else { TEXT_2 });
                let r = ui
                    .add(egui::Button::selectable(i == self.focus && many, text))
                    .on_hover_text(
                        b.root
                            .as_ref()
                            .map(|r| r.display().to_string())
                            .unwrap_or_default(),
                    );
                ui.painter().circle_filled(
                    Pos2::new(r.rect.right() - 3.0, r.rect.top() + 4.0),
                    2.5,
                    b.health(),
                );
                if r.clicked() {
                    self.focus = i;
                }
                if r.middle_clicked() {
                    close = Some(i);
                }
                ui.add_space(2.0);
            }
            let mut req = None;
            ui.menu_button(
                egui::RichText::new("+").font(semibold(15.0)).color(TEXT_2),
                |ui| {
                    if ui.button("Open board file…    Ctrl+O").clicked() {
                        req = Some(OpenRequest::PickFile);
                        ui.close();
                    }
                    if ui.button("Open project folder…").clicked() {
                        req = Some(OpenRequest::PickFolder);
                        ui.close();
                    }
                    ui.separator();
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.add_path)
                            .hint_text("path, then Enter")
                            .desired_width(300.0)
                            .font(mono(12.0)),
                    );
                    if edit.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !self.add_path.trim().is_empty()
                    {
                        req = Some(OpenRequest::Path(PathBuf::from(self.add_path.trim())));
                        self.add_path.clear();
                        ui.close();
                    }
                    let recent: Vec<PathBuf> = remembered_boards()
                        .into_iter()
                        .filter(|r| {
                            !self
                                .boards
                                .iter()
                                .any(|b| b.root.as_deref().is_some_and(|x| same_path(x, r)))
                        })
                        .collect();
                    if !recent.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("Recent").font(body(11.5)).color(TEXT_3));
                        for r in recent {
                            if ui.button(short_path(&r)).clicked() {
                                req = Some(OpenRequest::Path(r));
                                ui.close();
                            }
                        }
                    }
                },
            )
            .response
            .on_hover_text("Open another board next to this one. You can also drop files and folders on the window.");
            if let Some(r) = req {
                self.request(r, &ctx);
            }
            for j in &self.jobs {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(egui::RichText::new(&j.label).font(body(12.0)).color(TEXT_2));
            }
            if let Some((e, t)) = &self.add_error {
                if t.elapsed() < Duration::from_secs(8) {
                    // A long import error stays on one line. Hover over it to see the full text.
                    let short: String = if e.chars().count() > 90 {
                        format!("{}…", e.chars().take(89).collect::<String>())
                    } else {
                        e.clone()
                    };
                    ui.label(egui::RichText::new(short).font(body(12.0)).color(ERROR))
                        .on_hover_text(e);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.boards[self.focus].chips(ui);
            });
        });
        if let Some(i) = close {
            self.close(i, rs, &ctx);
        }
    }
}

/// Something the user asked to open.
pub(crate) enum OpenRequest {
    PickFile,
    PickFolder,
    Path(PathBuf),
}

/// A file dialog or an import that runs off the UI thread.
struct Job {
    label: String,
    /// The file to import. `None` for a dialog.
    path: Option<PathBuf>,
    rx: std::sync::mpsc::Receiver<JobDone>,
}

enum JobDone {
    /// The dialog closed. `None` if the user cancelled it.
    Picked(Option<PathBuf>),
    Opened(Result<PathBuf, String>),
}

impl Shell {
    fn spawn(
        &mut self,
        label: String,
        path: Option<PathBuf>,
        ctx: &egui::Context,
        f: impl FnOnce() -> JobDone + Send + 'static,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
        self.jobs.push(Job { label, path, rx });
    }

    fn request(&mut self, r: OpenRequest, ctx: &egui::Context) {
        // One dialog at a time, and one import per file.
        let dialog_open = self.jobs.iter().any(|j| j.path.is_none());
        if dialog_open && matches!(r, OpenRequest::PickFile | OpenRequest::PickFolder) {
            return;
        }
        match r {
            OpenRequest::PickFile => self.spawn(String::new(), None, ctx, || {
                JobDone::Picked(
                    rfd::FileDialog::new()
                        .set_title("Open a board")
                        .add_filter(
                            "Boards",
                            &["json", "kicad_pcb", "zip", "gbr", "gtl", "gbl", "drl"],
                        )
                        .add_filter("Lamina project (board.json)", &["json"])
                        .add_filter("KiCad board", &["kicad_pcb"])
                        .add_filter("EasyEDA board", &["json"])
                        .add_filter("Gerbers", &["zip", "gbr", "gtl", "gbl", "drl"])
                        .add_filter("All files", &["*"])
                        .pick_file(),
                )
            }),
            OpenRequest::PickFolder => self.spawn(String::new(), None, ctx, || {
                JobDone::Picked(
                    rfd::FileDialog::new()
                        .set_title("Open a Lamina project or Gerber folder")
                        .pick_folder(),
                )
            }),
            OpenRequest::Path(p) => {
                if let Some(r) = project_root(&p) {
                    // A project, or a file in a project. An import is not necessary.
                    if lamina_wiring::board_file_kind(&p).is_none()
                        || ProjectData::is_project(&p)
                        || p.file_name().is_some_and(|n| n == "board.json")
                    {
                        self.open_root(r, ctx);
                        return;
                    }
                }
                if lamina_wiring::board_file_kind(&p).is_none() {
                    self.add_error = Some((
                        format!("{} is not a Lamina project or a board file", p.display()),
                        Instant::now(),
                    ));
                    return;
                }
                if self
                    .jobs
                    .iter()
                    .any(|j| j.path.as_deref() == Some(p.as_path()))
                {
                    return;
                }
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let key = Some(p.clone());
                self.spawn(format!("Import {name}…"), key, ctx, move || {
                    JobDone::Opened(
                        lamina_wiring::open_board_path(&p)
                            .map(|(dir, _)| dir)
                            .map_err(|e| e.to_string()),
                    )
                });
            }
        }
    }

    fn open_root(&mut self, r: PathBuf, ctx: &egui::Context) {
        self.pending_open.push(r);
        ctx.request_repaint();
    }

    fn finish_jobs(&mut self, ctx: &egui::Context, rs: Option<&egui_wgpu::RenderState>) {
        let mut done = Vec::new();
        self.jobs.retain(|j| match j.rx.try_recv() {
            Ok(d) => {
                done.push(d);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for d in done {
            match d {
                JobDone::Picked(Some(p)) => self.request(OpenRequest::Path(p), ctx),
                JobDone::Picked(None) => {}
                JobDone::Opened(Ok(dir)) => self.add(dir, ctx, rs),
                JobDone::Opened(Err(e)) => self.add_error = Some((e, Instant::now())),
            }
        }
        for r in std::mem::take(&mut self.pending_open) {
            self.add(r, ctx, rs);
        }
    }
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let rs = frame.wgpu_render_state().cloned();
        for b in &mut self.boards {
            b.poll();
            if b.needs_wide {
                style::install_wide(&ctx);
            }
        }
        // Folders dropped on the window open next to the others.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        for p in dropped {
            self.request(OpenRequest::Path(p), &ctx);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
            self.request(OpenRequest::PickFile, &ctx);
        }
        self.finish_jobs(&ctx, rs.as_ref());
        let asked: Vec<OpenRequest> = self
            .boards
            .iter_mut()
            .filter_map(|b| b.request.take())
            .collect();
        for r in asked {
            self.request(r, &ctx);
        }
        // A new pane layout changes the size of each canvas. Fit them all again.
        if self.boards.len() != self.panes_seen {
            self.panes_seen = self.boards.len();
            for b in &mut self.boards {
                b.fitted = false;
                b.cam_fitted = false;
            }
        }
        for (i, b) in self.boards.iter_mut().enumerate() {
            b.focused = i == self.focus;
        }

        egui::Panel::top("top")
            .exact_size(44.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ui, |ui| self.top_bar(ui, rs.as_ref()));
        let f = self.focus;
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ui, |ui| self.boards[f].status_bar(ui));
        if self.boards[f].loaded.is_some() {
            // In a narrow window the side panels are narrower, so that the board
            // keeps the room in the middle.
            let narrow = ctx.content_rect().width() < 1100.0;
            let (left_w, right_w) = if narrow {
                (176.0, 228.0)
            } else {
                (236.0, 312.0)
            };
            egui::Panel::left("layers")
                .exact_size(left_w)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(PANEL)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ui, |ui| self.boards[f].layer_panel(ui));
            // Two panel ids, so that the narrow width does not replace the width
            // that the user set in a wide window.
            let inspector = if narrow {
                egui::Panel::right("inspector-narrow")
                    .exact_size(right_w)
                    .resizable(false)
            } else {
                egui::Panel::right("inspector")
                    .default_size(right_w)
                    .size_range(260.0..=520.0)
                    .resizable(true)
            };
            inspector
                .frame(
                    egui::Frame::new()
                        .fill(PANEL)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ui, |ui| self.boards[f].inspector(ui));
        }
        let mut close = None;
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let area = ui.max_rect();
            let n = self.boards.len();
            let gap = if n > 1 { 2.0 } else { 0.0 };
            let w = (area.width() - gap * (n as f32 - 1.0)) / n as f32;
            let header = 34.0;
            let (pressed, press_at) = ctx.input(|i| {
                (
                    i.pointer.any_pressed(),
                    i.pointer.interact_pos().or(i.pointer.latest_pos()),
                )
            });
            for (i, b) in self.boards.iter_mut().enumerate() {
                let x = area.left() + i as f32 * (w + gap);
                let pane =
                    ERect::from_min_size(Pos2::new(x, area.top()), Vec2::new(w, area.height()));
                if pressed && press_at.is_some_and(|p| pane.contains(p)) {
                    self.focus = i;
                }
                let head = ERect::from_min_size(pane.min, Vec2::new(w, header));
                let body_r = ERect::from_min_max(Pos2::new(pane.left(), head.bottom()), pane.max);
                ui.painter().rect_filled(head, 0.0, PANEL);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(head)
                        .id_salt(("pane-head", i)),
                    |ui| {
                        if b.pane_header(ui, n > 1) {
                            close = Some(i);
                        }
                    },
                );
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(body_r).id_salt(("pane", i)),
                    |ui| {
                        if b.loaded.is_some() {
                            if let Some(rs) = &rs {
                                b.canvas(ui, rs);
                                let ppp = ui.ctx().pixels_per_point();
                                b.canvas_panels(ui, body_r, body_r.size() * ppp);
                            }
                        } else {
                            b.empty_state(ui);
                        }
                    },
                );
                if n > 1 && i == self.focus {
                    ui.painter().rect_stroke(
                        pane.shrink(0.5),
                        0.0,
                        Stroke::new(1.0, ACCENT.gamma_multiply(0.7)),
                        StrokeKind::Inside,
                    );
                }
            }
        });
        if let Some(i) = close {
            self.close(i, rs.as_ref(), &ctx);
        }
        self.sync_title_and_memory(&ctx);

        let mut animating = false;
        let mut recent = false;
        for b in &mut self.boards {
            animating |= b.animating();
            recent |= b.recently_reloaded();
        }
        if animating || !self.jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if recent {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}

/// A GUI-subsystem exe has no console. Print to the console that started it.
fn attach_console() {
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn AttachConsole(pid: u32) -> i32;
        }
        // ATTACH_PARENT_PROCESS
        unsafe {
            AttachConsole(u32::MAX);
        }
    }
}

const USAGE: &str = "Usage: lamina-viewer [--3d] [BOARD_DIR | BOARD_FILE]...

Opens Lamina project folders, or KiCad, EasyEDA and Gerber files, side by
side in one window. The viewer imports board files when it opens them.
Without arguments, it opens the boards that were open last time. To open
more boards, click + in the top bar or drop a folder on the window.

  --3d  start in the 3D view";

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "shot") {
        attach_console();
        #[cfg(feature = "headless")]
        std::process::exit(shot::main(&args[1..]));
        #[cfg(not(feature = "headless"))]
        {
            eprintln!(
                "This build has no shot command. Build it with `cargo build -p lamina-viewer --features headless`."
            );
            std::process::exit(2);
        }
    }
    if args.iter().any(|a| a.starts_with('-')) {
        attach_console();
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("lamina-viewer {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let three_d = args.iter().any(|a| a == "--3d");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--3d").collect();
    if let Some(bad) = args.iter().find(|a| a.starts_with('-')) {
        eprintln!(
            "{bad} is not a known option.

{USAGE}"
        );
        std::process::exit(2);
    }
    let roots: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lamina")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png"))
                    .expect("icon.png is a valid PNG"),
            )
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([760.0, 480.0])
            .with_maximized(true),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Lamina",
        options,
        Box::new(move |cc| Ok(Box::new(Shell::new(cc, roots, three_d)))),
    )
}

#[cfg(test)]
mod path_tests {
    #[test]
    fn short_path_keeps_whole_characters() {
        let p = std::path::PathBuf::from(format!("C:/Users/oğuz/Masaüstü/{}", "ü".repeat(60)));
        let s = super::short_path(&p);
        assert_eq!(s.chars().count(), 47);
        assert!(s.starts_with('…'));
        assert_eq!(super::short_path(std::path::Path::new("C:/a")), "C:/a");
    }
}

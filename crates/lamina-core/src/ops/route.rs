use lamina_model::error::Result;
use lamina_model::*;
use lamina_route::RouteOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ops::place::Target;
use crate::project::{OpResult, Project, Touched, next_id};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RouteTrace {
    /// The net. If you do not give it, Lamina uses the net of the first pad.
    #[serde(default)]
    pub net: Option<String>,
    /// The copper layer, for example `top` or `bottom`.
    pub layer: Layer,
    /// The width in mm (default: the width from the net class or the board rules).
    #[serde(default)]
    pub width: Option<f64>,
    /// Points in order. Pad references (`U1.3`) snap to the pad centre.
    pub points: Vec<Target>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddVia {
    pub net: String,
    /// The position, or a pad reference to put the via on.
    pub at: Target,
    #[serde(default)]
    pub diameter: Option<f64>,
    #[serde(default)]
    pub drill: Option<f64>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddZone {
    /// The net of the pour. If you do not give a net, the pour is an island
    /// with no connection.
    #[serde(default)]
    pub net: Option<String>,
    pub layers: Vec<Layer>,
    /// The outline. The default is the board outline.
    #[serde(default)]
    pub outline: Option<Contour>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub clearance: Option<f64>,
    #[serde(default)]
    pub min_width: Option<f64>,
    /// `thermal` (default), `solid` or `none`.
    #[serde(default)]
    pub pad_connection: PadConnection,
    #[serde(default)]
    pub thermal_gap: Option<f64>,
    #[serde(default)]
    pub thermal_spoke: Option<f64>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddKeepout {
    pub layers: Vec<Layer>,
    pub outline: Contour,
    #[serde(default)]
    pub forbid: Forbid,
    /// The maximum height of a part in this area, in mm. The check
    /// `part_height` uses it.
    #[serde(default)]
    pub max_height: Option<f64>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddArea {
    /// The id of the area, for `when.area` of a custom rule. With no value,
    /// Lamina makes an id (`a1`, `a2`).
    #[serde(default)]
    pub id: Option<String>,
    /// The layers of the area. With no value, the area is on all layers.
    #[serde(default)]
    pub layers: Vec<Layer>,
    pub outline: Contour,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct Ripup {
    /// Remove the tracks and vias of this net.
    #[serde(default)]
    pub net: Option<String>,
    /// Remove the tracks or vias that have these ids.
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    /// Remove only on this layer (only tracks).
    #[serde(default)]
    pub layer: Option<Layer>,
    /// Remove only the items that have a point in this box.
    #[serde(default)]
    pub region: Option<Rect>,
    /// Remove all tracks and vias.
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DeleteItems {
    /// The ids of tracks, vias, zones, keep-outs, rule areas, texts, graphics or holes.
    pub ids: Vec<String>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Autoroute {
    #[serde(flatten)]
    pub options: RouteOptions,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddText {
    /// `top.silk`, `bottom.silk`, a copper layer or `top.fab`.
    pub layer: Layer,
    pub text: String,
    pub at: Point,
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddHole {
    pub at: Point,
    pub diameter: f64,
    /// If you give this, the hole is plated and has a copper ring of this diameter.
    #[serde(default)]
    pub pad_diameter: Option<f64>,
    #[serde(default)]
    pub net: Option<String>,
    #[serde(default)]
    pub strict: bool,
}

fn check_copper(l: Layer, layers: u8) -> Result<()> {
    if l.is_copper() && l.exists_in(layers) {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "{l} is not a copper layer of this {layers}-layer board."
        )))
    }
}

impl Project {
    pub(crate) fn all_ids(&self) -> Vec<String> {
        let r = &self.data.routing;
        r.tracks
            .iter()
            .map(|t| t.id.clone())
            .chain(r.vias.iter().map(|v| v.id.clone()))
            .chain(r.zones.iter().map(|z| z.id.clone()))
            .chain(r.keepouts.iter().map(|k| k.id.clone()))
            .chain(r.areas.iter().map(|k| k.id.clone()))
            .chain(r.texts.iter().map(|k| k.id.clone()))
            .chain(r.graphics.iter().map(|k| k.id.clone()))
            .chain(self.data.board.holes.iter().map(|h| h.id.clone()))
            .collect()
    }

    fn new_id(&self, prefix: &str) -> String {
        let ids = self.all_ids();
        next_id(prefix, ids.iter().map(|s| s.as_str()))
    }

    pub fn route_trace(&mut self, p: RouteTrace) -> Result<OpResult> {
        let layers = self.data.board.stackup.copper_layers;
        check_copper(p.layer, layers)?;
        if p.points.len() < 2 {
            return Err(Error::invalid("A trace needs at least two points."));
        }
        let li = p.layer.copper().unwrap().index(layers);
        let mut pts = Vec::new();
        let mut inferred: Option<String> = None;
        let pad_nets = self.data.nets.pad_map();
        for t in &p.points {
            match t {
                Target::Point(q) => {
                    if !q.is_finite() {
                        return Err(Error::invalid("A trace point is not a finite number."));
                    }
                    pts.push(Point::new(json::round6(q.x), json::round6(q.y)));
                }
                Target::Name(s) => {
                    let pr: PadRef = s.parse().map_err(|e| {
                        Error::invalid(format!(
                            "`{s}` is not a pad or a point. A trace point must be a pad \
                             (`U1.3`) or {{x, y}}. Details: {e}"
                        ))
                    })?;
                    let (c, _, ls) = self.pad_position(&pr)?;
                    if !ls.contains(&li) {
                        return Err(Error::invalid(format!(
                            "{pr} has no copper on {}. Route on the layer of the pad, or add a via.",
                            p.layer
                        )));
                    }
                    if inferred.is_none() {
                        inferred = pad_nets.get(&pr).cloned();
                    }
                    pts.push(Point::new(json::round6(c.x), json::round6(c.y)));
                }
            }
        }
        pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
        if pts.len() < 2 {
            return Err(Error::invalid(
                "All trace points are at the same position. Give at least two different points.",
            ));
        }
        let net = p
            .net
            .or(inferred)
            .ok_or_else(|| Error::invalid("None of the pads has a net. Give `net`."))?;
        if !self.data.nets.nets.contains_key(&net) {
            return Err(Error::invalid(format!("The net `{net}` does not exist.")));
        }
        let class = self.data.nets.nets[&net].class.clone();
        let width = p
            .width
            .unwrap_or(self.data.board.rules.track_width_for(class.as_deref()));
        if !(width.is_finite() && width > 0.0) {
            return Err(Error::invalid("The width must be more than 0."));
        }
        let id = self.new_id("t");
        let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
        let mut d = self.data.clone();
        d.routing.tracks.push(Track {
            id: id.clone(),
            net: net.clone(),
            layer: p.layer,
            width,
            points: pts,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "{id}: {net} on {}, {} mm long, {width} mm wide",
                p.layer,
                json::round6(len)
            ),
        )
    }

    pub fn add_via(&mut self, p: AddVia) -> Result<OpResult> {
        if !self.data.nets.nets.contains_key(&p.net) {
            return Err(Error::invalid(format!(
                "The net `{}` does not exist.",
                p.net
            )));
        }
        let at = match &p.at {
            Target::Point(q) => *q,
            Target::Name(s) => {
                let pr: PadRef = s.parse().map_err(Error::Invalid)?;
                self.pad_position(&pr)?.0
            }
        };
        let class = self.data.nets.nets[&p.net].class.clone();
        let nc = self.data.board.rules.class(class.as_deref());
        let diameter = p
            .diameter
            .or(nc.via_diameter)
            .unwrap_or(self.data.board.rules.via_diameter);
        let drill = p
            .drill
            .or(nc.via_drill)
            .unwrap_or(self.data.board.rules.via_drill);
        if !(drill > 0.0 && diameter > drill) {
            return Err(Error::invalid(
                "The via drill must be more than 0 and less than the via diameter.",
            ));
        }
        let id = self.new_id("v");
        let mut d = self.data.clone();
        d.routing.vias.push(Via {
            id: id.clone(),
            net: p.net.clone(),
            at: Point::new(json::round6(at.x), json::round6(at.y)),
            diameter,
            drill,
            from: None,
            to: None,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "{id}: {} via at ({}, {})",
                p.net,
                json::round6(at.x),
                json::round6(at.y)
            ),
        )
    }

    pub fn add_zone(&mut self, p: AddZone) -> Result<OpResult> {
        let layers = self.data.board.stackup.copper_layers;
        if p.layers.is_empty() {
            return Err(Error::invalid("Give at least one layer."));
        }
        for l in &p.layers {
            check_copper(*l, layers)?;
        }
        if let Some(n) = &p.net {
            if !self.data.nets.nets.contains_key(n) {
                return Err(Error::invalid(format!("The net `{n}` does not exist.")));
            }
        }
        let outline = match p.outline {
            Some(c) => c,
            None => self.data.board.outline.clone().ok_or_else(|| {
                Error::invalid(
                    "No outline is given, and the board outline is not set. \
                     Give `outline`, or set the board outline first.",
                )
            })?,
        };
        outline.check().map_err(Error::Invalid)?;
        let id = self.new_id("z");
        let mut d = self.data.clone();
        d.routing.zones.push(Zone {
            id: id.clone(),
            net: p.net.clone(),
            layers: p.layers.clone(),
            outline,
            priority: p.priority,
            clearance: p.clearance,
            min_width: p.min_width,
            pad_connection: p.pad_connection,
            thermal_gap: p.thermal_gap.unwrap_or(0.3),
            thermal_spoke: p.thermal_spoke.unwrap_or(0.3),
        });
        let names: Vec<String> = p.layers.iter().map(|l| l.to_string()).collect();
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "{id}: {} pour on {}",
                p.net.as_deref().unwrap_or("unconnected"),
                names.join(", ")
            ),
        )
    }

    pub fn add_keepout(&mut self, p: AddKeepout) -> Result<OpResult> {
        p.outline.check().map_err(Error::Invalid)?;
        if p.layers.is_empty() {
            return Err(Error::invalid("Give at least one layer."));
        }
        if p.max_height.is_some_and(|h| !h.is_finite() || h <= 0.0) {
            return Err(Error::invalid("max_height must be more than 0."));
        }
        let id = self.new_id("k");
        let mut d = self.data.clone();
        d.routing.keepouts.push(Keepout {
            id: id.clone(),
            layers: p.layers,
            outline: p.outline,
            forbid: p.forbid,
            max_height: p.max_height,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!("{id}: keep-out added"),
        )
    }

    /// Add an area that custom rules can select with `when.area`.
    pub fn add_area(&mut self, p: AddArea) -> Result<OpResult> {
        p.outline.check().map_err(Error::Invalid)?;
        let id = match p.id {
            Some(id) => {
                let ok = !id.is_empty()
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
                if !ok {
                    return Err(Error::invalid(
                        "The id of an area has letters, digits, `_` and `-` only.",
                    ));
                }
                if self.all_ids().contains(&id) {
                    return Err(Error::invalid(format!("The id {id} is in use.")));
                }
                id
            }
            None => self.new_id("a"),
        };
        let layers = self.data.board.stackup.copper_layers;
        for l in &p.layers {
            check_copper(*l, layers)?;
        }
        let mut d = self.data.clone();
        d.routing.areas.push(RuleArea {
            id: id.clone(),
            layers: p.layers,
            outline: p.outline,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            false,
            format!("{id}: rule area added"),
        )
    }

    pub fn ripup(&mut self, p: Ripup) -> Result<OpResult> {
        if !p.all && p.net.is_none() && p.ids.is_none() && p.region.is_none() {
            return Err(Error::invalid("Give net, ids or region, or set all: true."));
        }
        let inside = |q: &Point| {
            p.region.as_ref().is_none_or(|r| {
                q.x >= r.min.x && q.x <= r.max.x && q.y >= r.min.y && q.y <= r.max.y
            })
        };
        let mut d = self.data.clone();
        let before = (d.routing.tracks.len(), d.routing.vias.len());
        d.routing.tracks.retain(|t| {
            let hit = (p.all || p.net.as_ref().is_none_or(|n| *n == t.net))
                && p.ids.as_ref().is_none_or(|ids| ids.contains(&t.id))
                && p.layer.is_none_or(|l| l == t.layer)
                && t.points.iter().any(&inside);
            !hit
        });
        if p.layer.is_none() {
            d.routing.vias.retain(|v| {
                let hit = (p.all || p.net.as_ref().is_none_or(|n| *n == v.net))
                    && p.ids.as_ref().is_none_or(|ids| ids.contains(&v.id))
                    && inside(&v.at);
                !hit
            });
        }
        let removed = (
            before.0 - d.routing.tracks.len(),
            before.1 - d.routing.vias.len(),
        );
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!("removed {} track(s) and {} via(s)", removed.0, removed.1),
        )
    }

    pub fn delete_items(&mut self, p: DeleteItems) -> Result<OpResult> {
        let mut d = self.data.clone();
        let mut missing = Vec::new();
        let mut touched = Touched::file(FileKind::Routing);
        for id in &p.ids {
            let r = &mut d.routing;
            let n = r.tracks.len()
                + r.vias.len()
                + r.zones.len()
                + r.keepouts.len()
                + r.areas.len()
                + r.texts.len()
                + r.graphics.len();
            r.tracks.retain(|x| &x.id != id);
            r.vias.retain(|x| &x.id != id);
            r.zones.retain(|x| &x.id != id);
            r.keepouts.retain(|x| &x.id != id);
            r.areas.retain(|x| &x.id != id);
            r.texts.retain(|x| &x.id != id);
            r.graphics.retain(|x| &x.id != id);
            let m = r.tracks.len()
                + r.vias.len()
                + r.zones.len()
                + r.keepouts.len()
                + r.areas.len()
                + r.texts.len()
                + r.graphics.len();
            let h = d.board.holes.len();
            d.board.holes.retain(|x| &x.id != id);
            if d.board.holes.len() != h {
                touched = touched.and(FileKind::Board);
            } else if m == n {
                missing.push(id.clone());
            }
        }
        if !missing.is_empty() {
            return Err(Error::invalid(format!(
                "No items have these ids: {}.",
                missing.join(", ")
            )));
        }
        self.commit(d, touched, p.strict, format!("deleted {}", p.ids.join(" ")))
    }

    pub fn autoroute(&mut self, p: Autoroute) -> Result<OpResult> {
        let bad: Vec<String> = lamina_drc::model::validate(&self.data)
            .into_iter()
            .filter(|v| {
                v.severity == Severity::Error
                    && lamina_drc::registry::get(&v.rule).is_some_and(|c| !c.overridable)
            })
            .take(8)
            .map(|v| v.message)
            .collect();
        if !bad.is_empty() {
            return Err(Error::Refused(format!(
                "Correct these errors first: {}",
                bad.join(" ")
            )));
        }
        let out = lamina_route::autoroute(&self.data, &p.options).map_err(Error::Invalid)?;
        let mut d = self.data.clone();
        let n_t = out.tracks.len();
        let n_v = out.vias.len();
        d.routing.tracks.extend(out.tracks);
        d.routing.vias.extend(out.vias);
        let mut res = self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "routed {} connection(s) with {n_t} track(s) and {n_v} via(s), {} failed",
                out.routed.len(),
                out.failed.len()
            ),
        )?;
        res.notes = out
            .failed
            .iter()
            .map(|f| format!("failed {} {} -> {}: {}", f.net, f.from, f.to, f.reason))
            .collect();
        Ok(res)
    }

    pub fn add_text(&mut self, p: AddText) -> Result<OpResult> {
        if p.text.trim().is_empty() {
            return Err(Error::invalid("The text is empty. Give a text."));
        }
        let id = self.new_id("txt");
        let mut d = self.data.clone();
        d.routing.texts.push(BoardText {
            id: id.clone(),
            layer: p.layer,
            text: p.text.clone(),
            at: p.at,
            size: p.size.unwrap_or(1.0),
            rotation: p.rotation,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!("{id}: \"{}\" on {}", p.text, p.layer),
        )
    }

    pub fn add_hole(&mut self, p: AddHole) -> Result<OpResult> {
        if !p.diameter.is_finite() || p.diameter <= 0.0 {
            return Err(Error::invalid("The diameter must be more than 0."));
        }
        if let Some(pd) = p.pad_diameter {
            if pd <= p.diameter {
                return Err(Error::invalid(
                    "pad_diameter must be more than the hole diameter.",
                ));
            }
        }
        let id = self.new_id("h");
        let mut d = self.data.clone();
        d.board.holes.push(Hole {
            id: id.clone(),
            at: p.at,
            drill: Drill::Round {
                diameter: p.diameter,
            },
            plated: p.pad_diameter.is_some(),
            pad_diameter: p.pad_diameter,
            net: p.net,
            rotation: 0.0,
        });
        self.commit(
            d,
            Touched::file(FileKind::Board),
            p.strict,
            format!("{id}: {} mm hole", p.diameter),
        )
    }

    /// The connections that are not routed, as lines between pads.
    pub fn ratsnest(&self) -> Vec<lamina_drc::Ratsnest> {
        lamina_drc::connectivity(&self.data).ratsnest
    }
}

use std::collections::{BTreeMap, BTreeSet};

use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::{OpResult, Project, Touched};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Place {
    pub refdes: String,
    pub x: f64,
    pub y: f64,
    /// The rotation in degrees counter-clockwise. The default keeps the
    /// rotation that the part has, or 0.
    #[serde(default)]
    pub rotation: Option<f64>,
    /// `top` or `bottom`. The default keeps the side that the part has, or `top`.
    #[serde(default)]
    pub side: Option<Side>,
    #[serde(default)]
    pub locked: Option<bool>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PlaceMany {
    /// A map from the refdes to its placement.
    pub placements: BTreeMap<String, Placement>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MoveParts {
    pub refdes: Vec<String>,
    pub dx: f64,
    pub dy: f64,
    /// Also move locked parts.
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RotatePart {
    pub refdes: String,
    /// Absolute rotation in degrees.
    #[serde(default)]
    pub rotation: Option<f64>,
    /// Relative rotation in degrees.
    #[serde(default)]
    pub by: Option<f64>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FlipPart {
    pub refdes: String,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Align {
    pub refdes: Vec<String>,
    /// `x` makes the X coordinates equal (a vertical line). `y` makes the
    /// Y coordinates equal.
    pub axis: Axis,
    /// The target coordinate. The default is the coordinate of the first part.
    #[serde(default)]
    pub to: Option<f64>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Distribute {
    /// The parts, in order.
    pub refdes: Vec<String>,
    pub axis: Axis,
    /// The start coordinate (default: the coordinate of the first part).
    #[serde(default)]
    pub from: Option<f64>,
    /// The spacing from centre to centre. The default spreads the parts
    /// equally from the first part to the last part.
    #[serde(default)]
    pub spacing: Option<f64>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Unplace {
    /// Known parts to take off the board. Parts already unplaced are skipped.
    pub refdes: Vec<String>,
    /// Refuse the change if it adds a DRC error.
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Unplaced {
    #[serde(flatten)]
    pub result: OpResult,
    /// Requested parts that were already off the board, in request order.
    pub already_unplaced: Vec<String>,
}

/// A location: a pad reference (`U1.3`), a refdes (`U1`) or a point.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Target {
    Point(Point),
    Name(String),
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Measure {
    pub from: Target,
    pub to: Target,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Measurement {
    pub from: Point,
    pub to: Point,
    pub dx: f64,
    pub dy: f64,
    /// Centre to centre.
    pub distance: f64,
    /// From copper edge to copper edge, if both ends are pads.
    pub edge_distance: Option<f64>,
}

fn check_point(x: f64, y: f64) -> Result<()> {
    if x.is_finite() && y.is_finite() && x.abs() < 10_000.0 && y.abs() < 10_000.0 {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "The position ({x}, {y}) is not valid. Each coordinate must be a finite \
             number between -10000 and 10000 mm."
        )))
    }
}

fn norm_deg(a: f64) -> f64 {
    let r = json::round6(a.rem_euclid(360.0));
    if r >= 360.0 { 0.0 } else { r }
}

impl Project {
    fn need_part(&self, r: &str) -> Result<()> {
        if self.data.parts.parts.contains_key(r) {
            Ok(())
        } else {
            Err(Error::invalid(format!("The part {r} does not exist.")))
        }
    }

    fn need_placed(&self, r: &str) -> Result<Placement> {
        self.need_part(r)?;
        self.data
            .placement
            .placements
            .get(r)
            .copied()
            .ok_or_else(|| {
                Error::invalid(format!("{r} is not placed yet. Place it first with place."))
            })
    }

    pub fn place(&mut self, p: Place) -> Result<OpResult> {
        self.need_part(&p.refdes)?;
        check_point(p.x, p.y)?;
        let mut d = self.data.clone();
        let old = d.placement.placements.get(&p.refdes).copied();
        let pl = Placement {
            x: json::round6(p.x),
            y: json::round6(p.y),
            rotation: norm_deg(p.rotation.or(old.map(|o| o.rotation)).unwrap_or(0.0)),
            side: p.side.or(old.map(|o| o.side)).unwrap_or_default(),
            locked: p.locked.or(old.map(|o| o.locked)).unwrap_or(false),
        };
        d.placement.placements.insert(p.refdes.clone(), pl);
        self.commit(
            d,
            Touched::file(FileKind::Placement),
            p.strict,
            format!(
                "{} at ({}, {}) {}° {}",
                p.refdes,
                pl.x,
                pl.y,
                pl.rotation,
                pl.side.as_str()
            ),
        )
    }

    pub fn place_many(&mut self, p: PlaceMany) -> Result<OpResult> {
        let mut d = self.data.clone();
        for (r, pl) in &p.placements {
            self.need_part(r)?;
            check_point(pl.x, pl.y)?;
            let mut pl = *pl;
            pl.x = json::round6(pl.x);
            pl.y = json::round6(pl.y);
            pl.rotation = norm_deg(pl.rotation);
            d.placement.placements.insert(r.clone(), pl);
        }
        self.commit(
            d,
            Touched::file(FileKind::Placement),
            p.strict,
            format!("placed {} part(s)", p.placements.len()),
        )
    }

    pub fn move_parts(&mut self, p: MoveParts) -> Result<OpResult> {
        let mut d = self.data.clone();
        for r in &p.refdes {
            let mut pl = self.need_placed(r)?;
            if pl.locked && !p.force {
                return Err(Error::Refused(format!(
                    "{r} is locked. Set force: true to move it."
                )));
            }
            pl.x = json::round6(pl.x + p.dx);
            pl.y = json::round6(pl.y + p.dy);
            check_point(pl.x, pl.y)?;
            d.placement.placements.insert(r.clone(), pl);
        }
        self.commit(
            d,
            Touched::file(FileKind::Placement),
            p.strict,
            format!("moved {} by ({}, {})", p.refdes.join(" "), p.dx, p.dy),
        )
    }

    pub fn rotate_part(&mut self, p: RotatePart) -> Result<OpResult> {
        let mut pl = self.need_placed(&p.refdes)?;
        if pl.locked && !p.force {
            return Err(Error::Refused(format!(
                "{} is locked. Set force: true to rotate it.",
                p.refdes
            )));
        }
        pl.rotation = match (p.rotation, p.by) {
            (Some(a), None) => norm_deg(a),
            (None, Some(b)) => norm_deg(pl.rotation + b),
            _ => return Err(Error::invalid("Give exactly one of `rotation` and `by`.")),
        };
        let mut d = self.data.clone();
        d.placement.placements.insert(p.refdes.clone(), pl);
        self.commit(
            d,
            Touched::file(FileKind::Placement),
            p.strict,
            format!("{} rotated to {}°", p.refdes, pl.rotation),
        )
    }

    pub fn flip_part(&mut self, p: FlipPart) -> Result<OpResult> {
        let mut pl = self.need_placed(&p.refdes)?;
        if pl.locked && !p.force {
            return Err(Error::Refused(format!(
                "{} is locked. Set force: true to flip it.",
                p.refdes
            )));
        }
        pl.side = pl.side.flip();
        let mut d = self.data.clone();
        d.placement.placements.insert(p.refdes.clone(), pl);
        self.commit(
            d,
            Touched::file(FileKind::Placement),
            p.strict,
            format!("{} now on {}", p.refdes, pl.side.as_str()),
        )
    }

    pub fn align(&mut self, p: Align) -> Result<OpResult> {
        if p.refdes.is_empty() {
            return Err(Error::invalid(
                "No parts are given. Give at least one refdes.",
            ));
        }
        let first = self.need_placed(&p.refdes[0])?;
        let to = p.to.unwrap_or(match p.axis {
            Axis::X => first.x,
            Axis::Y => first.y,
        });
        let mut d = self.data.clone();
        for r in &p.refdes {
            let mut pl = self.need_placed(r)?;
            match p.axis {
                Axis::X => pl.x = json::round6(to),
                Axis::Y => pl.y = json::round6(to),
            }
            d.placement.placements.insert(r.clone(), pl);
        }
        self.commit(
            d,
            Touched::file(FileKind::Placement),
            p.strict,
            format!("aligned {} on {:?} = {to}", p.refdes.len(), p.axis),
        )
    }

    pub fn distribute(&mut self, p: Distribute) -> Result<OpResult> {
        if p.refdes.len() < 2 {
            return Err(Error::invalid("Give at least two parts."));
        }
        let coord = |pl: &Placement| match p.axis {
            Axis::X => pl.x,
            Axis::Y => pl.y,
        };
        let first = self.need_placed(&p.refdes[0])?;
        let last = self.need_placed(p.refdes.last().unwrap())?;
        let start = p.from.unwrap_or(coord(&first));
        let step = p
            .spacing
            .unwrap_or((coord(&last) - start) / (p.refdes.len() - 1) as f64);
        let mut d = self.data.clone();
        for (i, r) in p.refdes.iter().enumerate() {
            let mut pl = self.need_placed(r)?;
            let v = json::round6(start + step * i as f64);
            match p.axis {
                Axis::X => pl.x = v,
                Axis::Y => pl.y = v,
            }
            d.placement.placements.insert(r.clone(), pl);
        }
        self.commit(
            d,
            Touched::file(FileKind::Placement),
            p.strict,
            format!(
                "distributed {} parts, step {}",
                p.refdes.len(),
                json::round6(step)
            ),
        )
    }

    pub fn unplace(&mut self, p: Unplace) -> Result<Unplaced> {
        // Validate the whole request before changing any placement.
        for r in &p.refdes {
            self.need_part(r)?;
        }
        let mut d = self.data.clone();
        let mut seen = BTreeSet::new();
        let mut unplaced = Vec::new();
        let mut already_unplaced = Vec::new();
        for r in &p.refdes {
            if !seen.insert(r) {
                continue;
            }
            if d.placement.placements.remove(r).is_some() {
                unplaced.push(r.clone());
            } else {
                already_unplaced.push(r.clone());
            }
        }
        let touched = if unplaced.is_empty() {
            Touched::default()
        } else {
            Touched::file(FileKind::Placement)
        };
        let summary = if unplaced.is_empty() {
            "no placements changed".to_string()
        } else {
            format!("unplaced {}", unplaced.join(" "))
        };
        let result = self.commit(d, touched, p.strict, summary)?;
        Ok(Unplaced {
            result,
            already_unplaced,
        })
    }

    /// The position of a pad on the board.
    pub fn pad_position(&self, pr: &PadRef) -> Result<(Point, lamina_geom::Prim, Vec<u8>)> {
        let part = self
            .data
            .parts
            .parts
            .get(&pr.part)
            .ok_or_else(|| Error::invalid(format!("The part {} does not exist.", pr.part)))?;
        let pl = self
            .data
            .placement
            .placements
            .get(&pr.part)
            .ok_or_else(|| Error::invalid(format!("{} is not placed.", pr.part)))?;
        let fp = self.data.footprints.get(&part.footprint).ok_or_else(|| {
            Error::invalid(format!("The footprint {} does not exist.", part.footprint))
        })?;
        let layers = self.data.board.stackup.copper_layers;
        let pads = lamina_geom::placed_pads(fp, pl, layers);
        let (pad, _, prim, ls) = pads
            .into_iter()
            .find(|(p, ..)| p.number == pr.pad)
            .ok_or_else(|| Error::invalid(format!("{pr}: this pad does not exist.")))?;
        Ok((pl.transform(pad.at), prim, ls))
    }

    fn resolve(&self, t: &Target) -> Result<(Point, Option<lamina_geom::Prim>)> {
        match t {
            Target::Point(p) => Ok((*p, None)),
            Target::Name(s) => {
                if s.contains('.') {
                    let pr: PadRef = s.parse().map_err(Error::Invalid)?;
                    let (p, prim, _) = self.pad_position(&pr)?;
                    Ok((p, Some(prim)))
                } else {
                    let pl = self.need_placed(s)?;
                    Ok((pl.origin(), None))
                }
            }
        }
    }

    pub fn measure(&self, p: Measure) -> Result<Measurement> {
        let (a, pa) = self.resolve(&p.from)?;
        let (b, pb) = self.resolve(&p.to)?;
        let r = |v: f64| json::round6(v);
        Ok(Measurement {
            from: a,
            to: b,
            dx: r(b.x - a.x),
            dy: r(b.y - a.y),
            distance: r(a.dist(b)),
            edge_distance: match (pa, pb) {
                (Some(x), Some(y)) => Some(r(x.distance(&y))),
                _ => None,
            },
        })
    }
}

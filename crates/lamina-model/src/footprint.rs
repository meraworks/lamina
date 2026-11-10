//! Footprints. A footprint is defined as seen from the top, with the origin at
//! the part centre. When you put a part on the bottom side, the footprint is
//! mirrored in X.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::geom::{Contour, Point, Rect, Shape};

pub const FOOTPRINT_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Footprint {
    pub schema: u32,
    /// A file-safe id. It is also the file name in `cache/footprints/<id>.json`.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub pads: Vec<Pad>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphics: Vec<FpGraphic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<FpText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keepouts: Vec<FpKeepout>,
    /// The height of the body above the board, in mm. 3D placeholders and checks use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    pub provenance: Provenance,
    /// The hash of the pads when the footprint came into the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// The records of each check of this footprint against a datasheet or
    /// a second source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
}

/// How a footprint was verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceMethod {
    /// Against the package dimensions of the datasheet.
    Package,
    /// Against the land pattern that the datasheet recommends.
    LandPattern,
    /// Against the footprint of a second source.
    SecondSource,
    /// Against the standard package of the same name.
    Preset,
    /// Accepted with a reason, with no comparison.
    Accepted,
}

/// One record of a footprint check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    /// The hash of the pads that were checked. If the pads change, the record is stale.
    pub pads_hash: String,
    pub method: EvidenceMethod,
    /// The datasheet, the second source, or the reason for an acceptance.
    pub document: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The date of the check (YYYY-MM-DD).
    pub checked: String,
    pub pass: bool,
    /// The differences that the check found.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
}

impl Footprint {
    /// A hash of the pads and the keep-outs at 1 µm. It changes when the
    /// copper or the holes of the footprint change.
    pub fn pads_hash(&self) -> String {
        let um = |v: f64| (v * 1000.0).round() as i64;
        let mut lines: Vec<String> = self
            .pads
            .iter()
            .map(|p| {
                let (w, h) = p.shape.size();
                let drill = match &p.kind {
                    PadKind::Smd => "smd".to_string(),
                    PadKind::Through { drill, plated } => match drill {
                        Drill::Round { diameter } => format!("r{}:{plated}", um(*diameter)),
                        Drill::Slot { width, height } => {
                            format!("s{}x{}:{plated}", um(*width), um(*height))
                        }
                    },
                };
                format!(
                    "{}|{}|{}|{}|{}|{}|{}",
                    p.number,
                    um(p.at.x),
                    um(p.at.y),
                    um(w),
                    um(h),
                    um(p.rotation),
                    drill
                )
            })
            .collect();
        for k in &self.keepouts {
            let pts: Vec<String> = k
                .outline
                .flatten(0.01)
                .iter()
                .map(|p| format!("{},{}", um(p.x), um(p.y)))
                .collect();
            lines.push(format!("k|{}", pts.join(";")));
        }
        lines.sort();
        // FNV-1a, 64 bit: stable in all versions and on all computers.
        let mut h: u64 = 0xcbf29ce484222325;
        for b in lines.join("\n").bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("{h:016x}")
    }

    /// The last record for the pads that the footprint has now.
    pub fn current_evidence(&self) -> Option<&Evidence> {
        let hash = self.pads_hash();
        self.evidence.iter().rev().find(|e| e.pads_hash == hash)
    }

    pub fn new(id: impl Into<String>, provenance: Provenance) -> Self {
        Footprint {
            schema: FOOTPRINT_SCHEMA,
            id: id.into(),
            description: None,
            pads: Vec::new(),
            graphics: Vec::new(),
            texts: Vec::new(),
            keepouts: Vec::new(),
            height: None,
            model: None,
            provenance,
            source_hash: None,
            evidence: Vec::new(),
        }
    }

    pub fn pad(&self, number: &str) -> Option<&Pad> {
        self.pads.iter().find(|p| p.number == number)
    }

    /// The bounding box of the courtyard. If there is no courtyard, the pad extents.
    pub fn courtyard_bbox(&self) -> Option<Rect> {
        let pts: Vec<Point> = self
            .graphics
            .iter()
            .filter(|g| g.layer == FpLayer::Courtyard)
            .flat_map(|g| g.shape.points())
            .collect();
        if let Some(r) = Rect::from_points(pts) {
            return Some(r);
        }
        self.pads_bbox()
    }

    /// The part body for placeholders: the fab drawing, or else the pads, or else the courtyard.
    pub fn body_bbox(&self) -> Option<Rect> {
        let fab: Vec<Point> = self
            .graphics
            .iter()
            .filter(|g| g.layer == FpLayer::Fab)
            .flat_map(|g| g.shape.points())
            .collect();
        Rect::from_points(fab)
            .or_else(|| self.pads_bbox())
            .or_else(|| self.courtyard_bbox())
    }

    /// True if the part has a physical body to draw: a model, a height, a
    /// plated hole or a pad with paste. Mounting holes and fiducials have no body.
    pub fn has_body(&self) -> bool {
        self.model.is_some()
            || self.height.is_some()
            || self
                .pads
                .iter()
                .any(|p| p.paste || matches!(p.kind, PadKind::Through { plated: true, .. }))
    }

    pub fn pads_bbox(&self) -> Option<Rect> {
        let mut out: Option<Rect> = None;
        for p in &self.pads {
            let r = p.bbox();
            out = Some(match out {
                Some(o) => o.union(&r),
                None => r,
            });
        }
        out
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 120
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !id.starts_with('.')
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pad {
    /// The pad number that nets use (`U1.3`). It can be empty for mechanical pads.
    pub number: String,
    /// The pin name from the datasheet (`VDD`, `SCL`), when the source has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub at: Point,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
    pub shape: PadShape,
    pub kind: PadKind,
    /// The solder mask expansion. `None` uses the board rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask_margin: Option<f64>,
    /// The paste expansion (usually negative). `None` uses the board rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paste_margin: Option<f64>,
    /// True if the pad has a paste aperture (SMD only).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub paste: bool,
    /// True if the pad has a mask opening.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub mask: bool,
}

/// What a pin name says about the pin, from the name alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinRole {
    Ground,
    Power,
    /// Not connected inside the part.
    NoConnect,
    Other,
}

/// The role of a pin or net name. `GND`, `VSS`, `AGND` and `GNDIO` are
/// ground. `VDD`, `VCC`, `VIN`, `VBAT`, `3V3` and `5V` are power. `NC`
/// is a pin with no connection.
pub fn pin_role(name: &str) -> PinRole {
    let n = name.trim().to_ascii_uppercase();
    let n = n.trim_start_matches('+');
    if n.is_empty() {
        return PinRole::Other;
    }
    if matches!(
        n,
        "NC" | "N/C" | "N.C." | "N.C" | "DNC" | "NOCONNECT" | "NO_CONNECT"
    ) {
        return PinRole::NoConnect;
    }
    if matches!(n, "V+" | "V-") {
        return PinRole::Power;
    }
    let head = n
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or("");
    if head.starts_with("GND")
        || head.starts_with("VSS")
        || head.ends_with("GND")
        || head == "GROUND"
        || head == "0V"
    {
        return PinRole::Ground;
    }
    if head.starts_with("VDD")
        || head.starts_with("VCC")
        || head.starts_with("VBAT")
        || head.starts_with("VIN")
        || head == "VBUS"
        || head == "V+"
        || head.ends_with("VDD")
        || head.ends_with("VCC")
        || (head.len() <= 5
            && head.starts_with(|c: char| c.is_ascii_digit())
            && head.contains('V')
            && head.chars().all(|c| c.is_ascii_digit() || c == 'V'))
    {
        return PinRole::Power;
    }
    PinRole::Other
}

impl Pad {
    pub fn smd(number: impl Into<String>, at: Point, shape: PadShape) -> Pad {
        Pad {
            number: number.into(),
            name: None,
            at,
            rotation: 0.0,
            shape,
            kind: PadKind::Smd,
            mask_margin: None,
            paste_margin: None,
            paste: true,
            mask: true,
        }
    }

    pub fn through(number: impl Into<String>, at: Point, shape: PadShape, drill: Drill) -> Pad {
        Pad {
            number: number.into(),
            name: None,
            at,
            rotation: 0.0,
            shape,
            kind: PadKind::Through {
                drill,
                plated: true,
            },
            mask_margin: None,
            paste_margin: None,
            paste: false,
            mask: true,
        }
    }

    /// Bounding box in footprint coordinates.
    pub fn bbox(&self) -> Rect {
        let pts = self.shape.outline_points();
        let pts = pts
            .into_iter()
            .map(|p| p.rotated(self.rotation).add(self.at));
        Rect::from_points(pts).unwrap_or(Rect {
            min: self.at,
            max: self.at,
        })
    }

    pub fn is_smd(&self) -> bool {
        matches!(self.kind, PadKind::Smd)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PadShape {
    Circle {
        diameter: f64,
    },
    Rect {
        width: f64,
        height: f64,
    },
    RoundRect {
        width: f64,
        height: f64,
        radius: f64,
    },
    Oval {
        width: f64,
        height: f64,
    },
    /// A custom outline, relative to the pad centre.
    Polygon {
        points: Vec<Point>,
    },
}

impl PadShape {
    pub fn size(&self) -> (f64, f64) {
        match self {
            PadShape::Circle { diameter } => (*diameter, *diameter),
            PadShape::Rect { width, height }
            | PadShape::RoundRect { width, height, .. }
            | PadShape::Oval { width, height } => (*width, *height),
            PadShape::Polygon { points } => Rect::from_points(points.iter().copied())
                .map(|r| (r.width(), r.height()))
                .unwrap_or((0.0, 0.0)),
        }
    }

    /// The corner points of the box (or polygon) of the shape, not rotated, centred.
    pub fn outline_points(&self) -> Vec<Point> {
        match self {
            PadShape::Polygon { points } => points.clone(),
            _ => {
                let (w, h) = self.size();
                vec![Point::new(-w / 2.0, -h / 2.0), Point::new(w / 2.0, h / 2.0)]
            }
        }
    }

    pub fn check(&self) -> Result<(), String> {
        let pos = |v: f64, what: &str| {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(format!("The pad {what} is {v}. It must be more than zero."))
            }
        };
        match self {
            PadShape::Circle { diameter } => pos(*diameter, "diameter"),
            PadShape::Rect { width, height } | PadShape::Oval { width, height } => {
                pos(*width, "width")?;
                pos(*height, "height")
            }
            PadShape::RoundRect {
                width,
                height,
                radius,
            } => {
                pos(*width, "width")?;
                pos(*height, "height")?;
                if !(radius.is_finite() && *radius >= 0.0 && *radius <= width.min(*height) / 2.0) {
                    return Err(format!(
                        "The round-rect radius {radius} is out of range. Use 0 to half the smaller side."
                    ));
                }
                Ok(())
            }
            PadShape::Polygon { points } => {
                if points.len() < 3 {
                    return Err("A polygon pad must have a minimum of 3 points.".into());
                }
                if points.iter().any(|p| !p.is_finite()) {
                    return Err("The polygon pad has a point that is not finite.".into());
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PadKind {
    /// A surface mount pad on the side of the footprint.
    Smd,
    /// A through-hole pad on all copper layers.
    Through { drill: Drill, plated: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Drill {
    Round {
        diameter: f64,
    },
    /// An oblong slot. It turns with its pad or hole.
    Slot {
        width: f64,
        height: f64,
    },
}

impl Drill {
    pub fn min_size(&self) -> f64 {
        match self {
            Drill::Round { diameter } => *diameter,
            Drill::Slot { width, height } => width.min(*height),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        match self {
            Drill::Round { diameter } if ok(*diameter) => Ok(()),
            Drill::Slot { width, height } if ok(*width) && ok(*height) => Ok(()),
            _ => Err(format!("The drill size must be more than zero: {self:?}")),
        }
    }
}

/// Footprint-local layers, relative to the side that the part is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FpLayer {
    Silk,
    Courtyard,
    Fab,
    Copper,
    Mask,
    Paste,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FpGraphic {
    pub layer: FpLayer,
    pub shape: Shape,
    /// The stroke width. Use 0 for filled shapes.
    #[serde(default)]
    pub width: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub filled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FpText {
    pub layer: FpLayer,
    /// `${ref}` and `${value}` are replaced when the text is drawn.
    pub text: String,
    pub at: Point,
    #[serde(default = "default_text_size")]
    pub size: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeepoutScope {
    /// Every copper layer.
    AllCopper,
    /// Copper on the side that the part is on.
    Front,
    /// Copper on the opposite side.
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Forbid {
    #[serde(default = "yes")]
    pub tracks: bool,
    #[serde(default = "yes")]
    pub vias: bool,
    #[serde(default = "yes")]
    pub pours: bool,
    #[serde(default = "yes")]
    pub pads: bool,
    #[serde(default)]
    pub parts: bool,
}

impl Default for Forbid {
    fn default() -> Self {
        Forbid {
            tracks: true,
            vias: true,
            pours: true,
            pads: true,
            parts: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FpKeepout {
    pub scope: KeepoutScope,
    pub outline: Contour,
    #[serde(default)]
    pub forbid: Forbid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelRef {
    /// The path relative to `cache/models/` in the project.
    pub file: String,
    /// The offset in mm. It is applied after the rotation.
    #[serde(default, skip_serializing_if = "is_zero3")]
    pub offset: [f64; 3],
    /// Rotation in degrees about X, then Y, then Z.
    #[serde(default, skip_serializing_if = "is_zero3")]
    pub rotation: [f64; 3],
    #[serde(default = "one3", skip_serializing_if = "is_one3")]
    pub scale: [f64; 3],
}

/// The source of the geometry of a footprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Provenance {
    /// Generated from IPC-7351 formulas.
    Ipc7351 {
        package: String,
        density: String,
        inputs: serde_json::Value,
    },
    /// Typed in from the land pattern in a datasheet.
    Datasheet {
        document: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// Imported from LCSC / EasyEDA.
    Lcsc {
        part: String,
        fetched: String,
    },
    /// Imported from a KiCad footprint library or board file.
    Kicad {
        /// The library nickname, for example `Resistor_SMD`.
        library: String,
        footprint: String,
        /// The file or URL that the footprint was read from.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        path: String,
    },
    Manual {
        note: String,
    },
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
fn is_zero3(v: &[f64; 3]) -> bool {
    v.iter().all(|x| *x == 0.0)
}
fn one3() -> [f64; 3] {
    [1.0; 3]
}
fn is_one3(v: &[f64; 3]) -> bool {
    v.iter().all(|x| *x == 1.0)
}
fn default_text_size() -> f64 {
    1.0
}

#[cfg(test)]
mod pin_role_tests {
    use super::{PinRole, pin_role};

    #[test]
    fn names_have_roles() {
        for n in [
            "GND", "gnd", "AGND", "GNDIO", "VSS", "VSS_PA", "DGND", "+0V",
        ] {
            assert_eq!(pin_role(n), PinRole::Ground, "{n}");
        }
        for n in [
            "VDD", "VDDIO", "VCC", "+3V3", "3V3", "5V", "VIN", "VBAT", "VBUS", "AVDD", "V+",
        ] {
            assert_eq!(pin_role(n), PinRole::Power, "{n}");
        }
        for n in ["NC", "N/C", "n.c.", "DNC"] {
            assert_eq!(pin_role(n), PinRole::NoConnect, "{n}");
        }
        for n in ["SCL", "P0.01", "ANT", "A", "VOUT_EN", ""] {
            assert_eq!(pin_role(n), PinRole::Other, "{n}");
        }
    }
}

//! The project files: board, parts, nets, placement, routing, notes.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::footprint::{Drill, Forbid};
use crate::geom::{Contour, Point, Shape};
use crate::layer::{Layer, Side};

pub const SCHEMA: u32 = 1;

fn schema_v() -> u32 {
    SCHEMA
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

// ---------------------------------------------------------------- board.json

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoardFile {
    #[serde(default = "schema_v")]
    pub schema: u32,
    pub name: String,
    /// The edge of the board. `None` until you set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Contour>,
    /// Internal cut-outs through the board.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cutouts: Vec<Contour>,
    /// Mechanical holes that do not belong to a part.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Hole>,
    pub stackup: Stackup,
    pub rules: Rules,
}

impl BoardFile {
    pub fn new(name: impl Into<String>) -> Self {
        BoardFile {
            schema: SCHEMA,
            name: name.into(),
            outline: None,
            cutouts: Vec::new(),
            holes: Vec::new(),
            stackup: Stackup::default(),
            rules: Rules::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Hole {
    pub id: String,
    pub at: Point,
    pub drill: Drill,
    #[serde(default, skip_serializing_if = "is_false")]
    pub plated: bool,
    /// The diameter of the copper ring for plated holes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Stackup {
    /// 1, or an even number from 2 to 32.
    pub copper_layers: u8,
    /// The thickness of the finished board, in mm.
    pub thickness: f64,
    /// The weight of the outer copper, in oz.
    #[serde(default = "one")]
    pub outer_copper_oz: f64,
    /// The weight of the inner copper, in oz.
    #[serde(default = "half")]
    pub inner_copper_oz: f64,
    /// Surface finish, for example `HASL`, `LeadFree HASL`, `ENIG`.
    #[serde(default = "default_finish")]
    pub finish: String,
    #[serde(default = "default_mask")]
    pub mask_color: String,
    #[serde(default = "default_silk")]
    pub silk_color: String,
}

fn one() -> f64 {
    1.0
}
fn half() -> f64 {
    0.5
}
fn default_finish() -> String {
    "LeadFree HASL".into()
}
fn default_mask() -> String {
    "green".into()
}
fn default_silk() -> String {
    "white".into()
}

impl Default for Stackup {
    fn default() -> Self {
        Stackup {
            copper_layers: 2,
            thickness: 1.6,
            outer_copper_oz: 1.0,
            inner_copper_oz: 0.5,
            finish: default_finish(),
            mask_color: default_mask(),
            silk_color: default_silk(),
        }
    }
}

impl Stackup {
    pub fn check(&self) -> Result<(), String> {
        let n = self.copper_layers;
        if !(n == 1 || ((2..=32).contains(&n) && n.is_multiple_of(2))) {
            return Err(format!(
                "copper_layers is {n}. It must be 1 or an even number from 2 to 32."
            ));
        }
        if !(self.thickness.is_finite() && self.thickness > 0.1 && self.thickness < 10.0) {
            return Err(format!(
                "The thickness {} mm is out of range. Use more than 0.1 mm and less than 10 mm.",
                self.thickness
            ));
        }
        Ok(())
    }
}

fn default_silk_to_edge() -> f64 {
    0.15
}

fn default_silk_text_height() -> f64 {
    0.8
}

/// Design rules. All sizes are in mm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rules {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<crate::rules::CustomRule>,
    #[serde(default = "default_thermal_spokes")]
    pub min_thermal_spokes: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusions: Vec<Exclusion>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub severities: BTreeMap<String, RuleSeverity>,
    /// The fab preset that these rules came from, if there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    pub clearance: f64,
    pub track_width: f64,
    pub min_track_width: f64,
    pub via_diameter: f64,
    pub via_drill: f64,
    pub min_via_drill: f64,
    pub min_annular_ring: f64,
    pub min_drill: f64,
    pub hole_to_hole: f64,
    pub copper_to_edge: f64,
    pub mask_margin: f64,
    pub paste_margin: f64,
    pub silk_width: f64,
    pub silk_to_pad: f64,
    /// The minimum distance from silkscreen to the board edge.
    #[serde(default = "default_silk_to_edge")]
    pub silk_to_edge: f64,
    /// The minimum height of silkscreen text.
    #[serde(default = "default_silk_text_height")]
    pub min_silk_text_height: f64,
    /// The minimum solder mask between openings of two nets. Zero permits
    /// openings that touch only at their edges.
    #[serde(default)]
    pub min_mask_web: f64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub net_classes: BTreeMap<String, NetClass>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            custom: Vec::new(),
            min_thermal_spokes: default_thermal_spokes(),
            exclusions: Vec::new(),
            severities: BTreeMap::new(),
            preset: None,
            clearance: 0.15,
            track_width: 0.25,
            min_track_width: 0.15,
            via_diameter: 0.6,
            via_drill: 0.3,
            min_via_drill: 0.3,
            min_annular_ring: 0.13,
            min_drill: 0.3,
            hole_to_hole: 0.5,
            copper_to_edge: 0.3,
            mask_margin: 0.05,
            paste_margin: 0.0,
            silk_width: 0.15,
            silk_to_pad: 0.15,
            silk_to_edge: default_silk_to_edge(),
            min_silk_text_height: default_silk_text_height(),
            min_mask_web: 0.0,
            net_classes: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleSeverity {
    Error,
    Warning,
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Exclusion {
    pub rule: String,
    pub items: Vec<String>,
    pub reason: String,
}

impl Exclusion {
    pub fn new(rule: String, mut items: Vec<String>, reason: String) -> Result<Self, String> {
        if reason.trim().is_empty() {
            return Err("An exclusion needs a reason.".into());
        }
        items.sort();
        Ok(Self {
            rule,
            items,
            reason,
        })
    }
}

fn default_thermal_spokes() -> u8 {
    2
}

impl Rules {
    pub fn class(&self, name: Option<&str>) -> NetClass {
        name.and_then(|n| self.net_classes.get(n))
            .cloned()
            .unwrap_or_default()
    }

    pub fn clearance_for(&self, class: Option<&str>) -> f64 {
        self.class(class).clearance.unwrap_or(self.clearance)
    }

    pub fn track_width_for(&self, class: Option<&str>) -> f64 {
        self.class(class).track_width.unwrap_or(self.track_width)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NetClass {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_drill: Option<f64>,
}

// ---------------------------------------------------------------- parts.json

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PartsFile {
    #[serde(default = "schema_v")]
    pub schema: u32,
    /// The key is the reference designator.
    #[serde(default)]
    pub parts: BTreeMap<String, Part>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Part {
    /// Footprint id in `cache/footprints/`.
    pub footprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    /// LCSC part number, for example `C2040`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lcsc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Do not populate.
    #[serde(default, skip_serializing_if = "is_false")]
    pub dnp: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    /// The block instance that this part belongs to, for example `servo1`.
    /// Parts of one group move together. They can take the layout of another group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

impl Part {
    pub fn new(footprint: impl Into<String>) -> Part {
        Part {
            footprint: footprint.into(),
            value: None,
            mpn: None,
            manufacturer: None,
            lcsc: None,
            description: None,
            dnp: false,
            fields: BTreeMap::new(),
            group: None,
        }
    }
}

impl PartsFile {
    /// Group names with their parts, sorted.
    pub fn groups(&self) -> BTreeMap<String, Vec<String>> {
        let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (r, p) in &self.parts {
            if let Some(g) = &p.group {
                m.entry(g.clone()).or_default().push(r.clone());
            }
        }
        m
    }
}

pub fn valid_refdes(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= 32
        && r.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && r.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ---------------------------------------------------------------- nets.json

/// A pad reference written as `REFDES.PAD`, for example `U1.3`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PadRef {
    pub part: String,
    pub pad: String,
}

impl PadRef {
    pub fn new(part: impl Into<String>, pad: impl Into<String>) -> Self {
        PadRef {
            part: part.into(),
            pad: pad.into(),
        }
    }
}

impl fmt::Display for PadRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.part, self.pad)
    }
}

impl FromStr for PadRef {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (part, pad) = s
            .split_once('.')
            .ok_or_else(|| format!("The pad reference `{s}` is not valid. Use the form `U1.3`."))?;
        if !valid_refdes(part) {
            return Err(format!("The reference designator in `{s}` is not valid."));
        }
        if pad.is_empty() {
            return Err(format!("The pad number in `{s}` is empty."));
        }
        Ok(PadRef::new(part, pad))
    }
}

impl Serialize for PadRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PadRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = Cow::<str>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for PadRef {
    fn schema_name() -> Cow<'static, str> {
        "PadRef".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "Pad reference `REFDES.PAD`, for example `U1.3` or `J1.A5`.",
            "pattern": "^[A-Za-z][A-Za-z0-9_-]*\\..+$"
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct NetsFile {
    #[serde(default = "schema_v")]
    pub schema: u32,
    #[serde(default)]
    pub nets: BTreeMap<String, Net>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Net {
    pub pads: Vec<PadRef>,
    /// The name of a net class in the board rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Minimum total track length in mm, not a pad-to-pad path length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_min: Option<f64>,
    /// Maximum total track length in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skew_group: Option<String>,
    /// Maximum spread of total track lengths in the group, in mm.
    /// The smallest supplied limit among group members applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_skew: Option<f64>,
}

impl NetsFile {
    /// The net that a pad belongs to.
    pub fn net_of(&self, pad: &PadRef) -> Option<&str> {
        self.nets
            .iter()
            .find(|(_, n)| n.pads.contains(pad))
            .map(|(k, _)| k.as_str())
    }

    pub fn pad_map(&self) -> BTreeMap<PadRef, String> {
        let mut m = BTreeMap::new();
        for (name, net) in &self.nets {
            for p in &net.pads {
                m.entry(p.clone()).or_insert_with(|| name.clone());
            }
        }
        m
    }
}

pub fn valid_net_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && !n.chars().any(|c| c.is_control() || c == '"')
}

// ---------------------------------------------------------------- placement.json

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PlacementFile {
    #[serde(default = "schema_v")]
    pub schema: u32,
    #[serde(default)]
    pub placements: BTreeMap<String, Placement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
    #[serde(default, skip_serializing_if = "is_top")]
    pub side: Side,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
}

fn is_top(s: &Side) -> bool {
    *s == Side::Top
}

impl Placement {
    pub fn at(x: f64, y: f64) -> Self {
        Placement {
            x,
            y,
            rotation: 0.0,
            side: Side::Top,
            locked: false,
        }
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    /// Changes a footprint-local point to board coordinates. Parts on the bottom
    /// side are mirrored in X first. The rotation is CCW when you look from the top.
    pub fn transform(&self, p: Point) -> Point {
        let p = if self.side == Side::Bottom {
            Point::new(-p.x, p.y)
        } else {
            p
        };
        p.rotated(self.rotation).add(self.origin())
    }

    /// Changes board coordinates back to footprint-local coordinates.
    pub fn inverse(&self, p: Point) -> Point {
        let q = p.sub(self.origin()).rotated(-self.rotation);
        if self.side == Side::Bottom {
            Point::new(-q.x, q.y)
        } else {
            q
        }
    }

    /// Board rotation of a footprint-local angle.
    pub fn transform_angle(&self, deg: f64) -> f64 {
        if self.side == Side::Bottom {
            self.rotation - deg
        } else {
            self.rotation + deg
        }
    }
}

// ---------------------------------------------------------------- routing.json

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct RoutingFile {
    #[serde(default = "schema_v")]
    pub schema: u32,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub vias: Vec<Via>,
    #[serde(default)]
    pub zones: Vec<Zone>,
    #[serde(default)]
    pub keepouts: Vec<Keepout>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<BoardText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphics: Vec<BoardGraphic>,
}

/// A polyline of copper on one layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Track {
    pub id: String,
    pub net: String,
    pub layer: Layer,
    pub width: f64,
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Via {
    pub id: String,
    pub net: String,
    pub at: Point,
    pub diameter: f64,
    pub drill: f64,
    /// The start layer. `None` means top (through via).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Layer>,
    /// The end layer. `None` means bottom (through via).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Layer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PadConnection {
    #[default]
    Thermal,
    Solid,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Zone {
    pub id: String,
    /// `None` makes a copper island fill that has no net.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    pub layers: Vec<Layer>,
    pub outline: Contour,
    /// A zone with a higher priority fills first and wins where zones overlap.
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<f64>,
    #[serde(default)]
    pub pad_connection: PadConnection,
    #[serde(default = "default_gap")]
    pub thermal_gap: f64,
    #[serde(default = "default_spoke")]
    pub thermal_spoke: f64,
}

fn default_gap() -> f64 {
    0.3
}
fn default_spoke() -> f64 {
    0.3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Keepout {
    pub id: String,
    pub layers: Vec<Layer>,
    pub outline: Contour,
    #[serde(default)]
    pub forbid: Forbid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoardText {
    pub id: String,
    pub layer: Layer,
    pub text: String,
    pub at: Point,
    #[serde(default = "default_text")]
    pub size: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
}

fn default_text() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoardGraphic {
    pub id: String,
    pub layer: Layer,
    pub shape: Shape,
    #[serde(default)]
    pub width: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub filled: bool,
}

// ---------------------------------------------------------------- notes.json

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct NotesFile {
    #[serde(default = "schema_v")]
    pub schema: u32,
    #[serde(default)]
    pub notes: Vec<Note>,
}

/// A comment from the user in the viewer, or from the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refdes: Option<String>,
    /// `user` or `agent`.
    pub author: String,
    /// Seconds after the Unix epoch.
    pub created: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub resolved: bool,
}

// ---------------------------------------------------------------- violations

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// A rule failure that validation or DRC found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Violation {
    /// Stable rule id, for example `clearance` or `pad_in_two_nets`.
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<Layer>,
    /// The ids of the related items (refdes, pad refs, track ids).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
}

impl Violation {
    pub fn error(rule: &str, message: impl Into<String>) -> Self {
        Violation {
            rule: rule.into(),
            severity: Severity::Error,
            message: message.into(),
            at: None,
            layer: None,
            items: Vec::new(),
        }
    }

    pub fn warning(rule: &str, message: impl Into<String>) -> Self {
        Violation {
            severity: Severity::Warning,
            ..Self::error(rule, message)
        }
    }

    pub fn at(mut self, p: Point) -> Self {
        self.at = Some(p);
        self
    }

    pub fn on(mut self, l: Layer) -> Self {
        self.layer = Some(l);
        self
    }

    pub fn items<I: IntoIterator<Item = S>, S: Into<String>>(mut self, it: I) -> Self {
        self.items = it.into_iter().map(Into::into).collect();
        self
    }

    /// The identity that tells new violations from old violations.
    pub fn key(&self) -> String {
        let mut items = self.items.clone();
        items.sort();
        format!(
            "{}|{}|{}",
            self.rule,
            self.layer.map(|l| l.to_string()).unwrap_or_default(),
            items.join(",")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_rules_default_to_two_thermal_spokes() {
        let mut value = serde_json::to_value(Rules::default()).unwrap();
        value.as_object_mut().unwrap().remove("min_thermal_spokes");
        let rules: Rules = serde_json::from_value(value).unwrap();
        assert_eq!(rules.min_thermal_spokes, 2);
    }

    #[test]
    fn exclusions_require_a_reason_and_sort_items() {
        assert!(Exclusion::new("clearance".into(), vec!["b".into()], "  ".into()).is_err());
        let entry = Exclusion::new(
            "clearance".into(),
            vec!["b".into(), "a".into()],
            "Accepted gap".into(),
        )
        .unwrap();
        assert_eq!(entry.items, vec!["a", "b"]);
        let mut rules: Rules =
            serde_json::from_value(serde_json::to_value(Rules::default()).unwrap()).unwrap();
        assert!(rules.exclusions.is_empty());
        rules.exclusions.push(entry);
        let round: Rules = serde_json::from_value(serde_json::to_value(&rules).unwrap()).unwrap();
        assert_eq!(round, rules);
    }

    #[test]
    fn rule_severity_round_trip_and_old_rules() {
        let old = serde_json::to_value(Rules::default()).unwrap();
        assert!(old.get("severities").is_none());
        let mut rules: Rules = serde_json::from_value(old).unwrap();
        assert!(rules.severities.is_empty());
        rules
            .severities
            .insert("unrouted".into(), RuleSeverity::Ignore);
        let value = serde_json::to_value(&rules).unwrap();
        assert_eq!(value["severities"]["unrouted"], "ignore");
        assert_eq!(serde_json::from_value::<Rules>(value).unwrap(), rules);
    }

    #[test]
    fn padref_parse() {
        let p: PadRef = "U1.3".parse().unwrap();
        assert_eq!(p, PadRef::new("U1", "3"));
        let p: PadRef = "J1.A5".parse().unwrap();
        assert_eq!(p.pad, "A5");
        let p: PadRef = "U2.EP.1".parse().unwrap();
        assert_eq!(p.pad, "EP.1");
        assert!("U1".parse::<PadRef>().is_err());
        assert!("1U.3".parse::<PadRef>().is_err());
    }

    #[test]
    fn placement_transform() {
        let pl = Placement {
            x: 10.0,
            y: 5.0,
            rotation: 90.0,
            side: Side::Top,
            locked: false,
        };
        let p = pl.transform(Point::new(1.0, 0.0));
        assert!((p.x - 10.0).abs() < 1e-12 && (p.y - 6.0).abs() < 1e-12);

        let pl = Placement {
            side: Side::Bottom,
            rotation: 0.0,
            ..pl
        };
        let p = pl.transform(Point::new(1.0, 0.0));
        assert!((p.x - 9.0).abs() < 1e-12 && (p.y - 5.0).abs() < 1e-12);
    }

    #[test]
    fn stackup_check() {
        assert!(Stackup::default().check().is_ok());
        let s = Stackup {
            copper_layers: 3,
            ..Stackup::default()
        };
        assert!(s.check().is_err());
    }
}

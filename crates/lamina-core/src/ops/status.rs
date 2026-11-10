use std::collections::BTreeMap;

use lamina_model::*;
use schemars::JsonSchema;
use serde::Serialize;

use crate::ops::provider::MissingModel;
use crate::project::Project;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Status {
    pub name: String,
    pub path: String,
    /// The bounding box of the outline, if the outline is set.
    pub outline: Option<Rect>,
    pub copper_layers: u8,
    pub thickness: f64,
    pub parts: usize,
    pub placed: usize,
    /// Parts without a placement.
    pub unplaced: Vec<String>,
    pub footprints: usize,
    pub nets: usize,
    pub tracks: usize,
    pub vias: usize,
    pub zones: usize,
    pub keepouts: usize,
    pub open_notes: usize,
    pub errors: usize,
    pub warnings: usize,
    /// The errors that stop `export`. Open connections and parts with no
    /// placement are errors for release.
    pub release_errors: usize,
    /// The number of violations for each rule.
    pub by_rule: BTreeMap<String, usize>,
    /// The design source file, if the board is built from a source file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The files that the source generates. Edit the source, not these files.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<String>,
    /// The workspace root, if the board is in a workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// The part library folder that gets new downloads.
    pub library: String,
    /// The footprints whose 3D model file is not in `cache/models`.
    /// `restore_models` gets them back.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_models: Vec<MissingModel>,
    /// Part groups (block instances) and their parts.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub groups: BTreeMap<String, Vec<String>>,
}

impl Project {
    pub fn status(&self) -> Status {
        let d = &self.data;
        let a = lamina_drc::analyze(d);
        let v = a.violations.clone();
        let errors = v.iter().filter(|x| x.severity == Severity::Error).count();
        let mut by_rule = BTreeMap::new();
        for x in &v {
            *by_rule.entry(x.rule.clone()).or_insert(0) += 1;
        }
        Status {
            name: d.board.name.clone(),
            path: self.root().display().to_string(),
            outline: d.board.outline.as_ref().and_then(|c| c.bbox()),
            copper_layers: d.board.stackup.copper_layers,
            thickness: d.board.stackup.thickness,
            parts: d.parts.parts.len(),
            placed: d
                .parts
                .parts
                .keys()
                .filter(|r| d.placement.placements.contains_key(*r))
                .count(),
            unplaced: d
                .parts
                .parts
                .keys()
                .filter(|r| !d.placement.placements.contains_key(*r))
                .cloned()
                .collect(),
            footprints: d.footprints.len(),
            nets: d.nets.nets.len(),
            tracks: d.routing.tracks.len(),
            vias: d.routing.vias.len(),
            zones: d.routing.zones.len(),
            keepouts: d.routing.keepouts.len(),
            open_notes: d.notes.notes.iter().filter(|n| !n.resolved).count(),
            errors,
            warnings: v.len() - errors,
            release_errors: lamina_drc::release_view(&a, &d.board.rules)
                .iter()
                .filter(|x| x.severity == Severity::Error)
                .count(),
            by_rule,
            source: self.source_file(),
            generated: self
                .source_owned()
                .iter()
                .map(|k| k.file_name().to_string())
                .collect(),
            workspace: Workspace::find(self.root()).map(|w| w.root.display().to_string()),
            library: self.library().store.display().to_string(),
            missing_models: self.missing_models(),
            groups: d.parts.groups(),
        }
    }
}

/// A check, as the `checks` tool lists it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CheckRow {
    pub id: String,
    pub family: lamina_drc::Family,
    /// `error`, `warning` or `ignore`. The board can change it with `set_rules`.
    pub severity: RuleSeverity,
    /// The severity for `export`, if it is different.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<RuleSeverity>,
    pub can_change_severity: bool,
    pub can_exclude: bool,
    pub reports: String,
    pub why: String,
    /// The rule fields that set its limits.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

impl Project {
    /// All checks with their severity on this board.
    pub fn checks(&self) -> Vec<CheckRow> {
        lamina_drc::CHECKS
            .iter()
            .map(|c| CheckRow {
                id: c.id.into(),
                family: c.family,
                severity: self
                    .data
                    .board
                    .rules
                    .severities
                    .get(c.id)
                    .copied()
                    .unwrap_or(c.default),
                release: c.release,
                can_change_severity: c.overridable,
                can_exclude: c.excludable && c.id != "stale_exclusion",
                reports: c.summary.into(),
                why: c.why.into(),
                fields: c.fields.iter().map(|f| f.to_string()).collect(),
            })
            .collect()
    }
}

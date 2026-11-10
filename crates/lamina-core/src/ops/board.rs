use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::{OpResult, Project, Touched};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RectSpec {
    /// Lower-left corner X in mm.
    #[serde(default)]
    pub x: f64,
    /// Lower-left corner Y in mm.
    #[serde(default)]
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Corner radius in mm (0 for sharp corners).
    #[serde(default)]
    pub corner_radius: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SetOutline {
    /// A rectangular board. Give `rect` or `contour`, not both.
    #[serde(default)]
    pub rect: Option<RectSpec>,
    /// An outline of any shape, made of lines and arcs.
    #[serde(default)]
    pub contour: Option<Contour>,
    /// Internal cut-outs. If you give them, they replace the cut-outs that
    /// are on the board.
    #[serde(default)]
    pub cutouts: Option<Vec<Contour>>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct SetStackup {
    /// 1, or an even number 2..32.
    pub copper_layers: Option<u8>,
    /// Finished thickness in mm.
    pub thickness: Option<f64>,
    pub outer_copper_oz: Option<f64>,
    pub inner_copper_oz: Option<f64>,
    pub finish: Option<String>,
    pub mask_color: Option<String>,
    pub silk_color: Option<String>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct SetRules {
    /// Partial rules. Only the fields that you give are changed. A net class
    /// that you give here replaces the net class that has the same name.
    /// `severities` replaces the rule-name map. Values are `error`, `warning`,
    /// or `ignore`. An empty map restores the default severities.
    /// `custom` replaces the ordered list of named rules. Match with `when.net`
    /// (`*` and `?` wildcards), `when.class`, and `when.layer` (copper layer).
    /// Later matching rules override supplied clearance/minimum dimensions.
    /// An empty list removes custom rules. All dimensions are positive mm.
    #[serde(default)]
    pub rules: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub strict: bool,
}

impl Project {
    pub fn set_outline(&mut self, p: SetOutline) -> Result<OpResult> {
        let contour = match (p.rect, p.contour) {
            (Some(r), None) => {
                for (v, n) in [(r.width, "width"), (r.height, "height")] {
                    if !(v.is_finite() && v > 0.0) {
                        return Err(Error::invalid(format!("The rect {n} must be more than 0.")));
                    }
                }
                if r.corner_radius < 0.0 || r.corner_radius * 2.0 > r.width.min(r.height) {
                    return Err(Error::invalid(
                        "corner_radius is out of range. Use 0 to half the smaller side of the rect.",
                    ));
                }
                Contour::rounded_rect(r.x, r.y, r.width, r.height, r.corner_radius)
            }
            (None, Some(c)) => c,
            (None, None) => {
                return Err(Error::invalid(
                    "No outline is given. Give `rect` or `contour`.",
                ));
            }
            (Some(_), Some(_)) => {
                return Err(Error::invalid("Give only one of `rect` and `contour`."));
            }
        };
        contour.check().map_err(Error::Invalid)?;
        let mut d = self.data.clone();
        if let Some(cuts) = p.cutouts {
            for c in &cuts {
                c.check()
                    .map_err(|e| Error::invalid(format!("cutout: {e}")))?;
            }
            d.board.cutouts = cuts;
        }
        let bb = contour.bbox().expect("checked contour has points");
        d.board.outline = Some(contour);
        self.commit(
            d,
            Touched::file(FileKind::Board),
            p.strict,
            format!("outline set, {:.3} x {:.3} mm", bb.width(), bb.height()),
        )
    }

    pub fn set_stackup(&mut self, p: SetStackup) -> Result<OpResult> {
        let mut d = self.data.clone();
        let s = &mut d.board.stackup;
        if let Some(n) = p.copper_layers {
            s.copper_layers = n;
        }
        if let Some(t) = p.thickness {
            s.thickness = t;
        }
        if let Some(v) = p.outer_copper_oz {
            s.outer_copper_oz = v;
        }
        if let Some(v) = p.inner_copper_oz {
            s.inner_copper_oz = v;
        }
        if let Some(v) = p.finish {
            s.finish = v;
        }
        if let Some(v) = p.mask_color {
            s.mask_color = v;
        }
        if let Some(v) = p.silk_color {
            s.silk_color = v;
        }
        s.check().map_err(Error::Invalid)?;
        let n = s.copper_layers;
        // If a layer that has copper on it is removed, that copper has no layer.
        let stranded: Vec<String> = d
            .routing
            .tracks
            .iter()
            .filter(|t| !t.layer.exists_in(n))
            .map(|t| t.id.clone())
            .chain(
                d.routing
                    .zones
                    .iter()
                    .filter(|z| z.layers.iter().any(|l| !l.exists_in(n)))
                    .map(|z| z.id.clone()),
            )
            .collect();
        if !stranded.is_empty() {
            return Err(Error::Refused(format!(
                "These items are on layers that a {n}-layer board does not have. \
                 Move or delete them first: {}",
                stranded.join(", ")
            )));
        }
        let summary = format!(
            "stackup: {n} copper layers, {} mm",
            d.board.stackup.thickness
        );
        self.commit(d, Touched::file(FileKind::Board), p.strict, summary)
    }

    pub fn set_rules(&mut self, p: SetRules) -> Result<OpResult> {
        let mut d = self.data.clone();
        let mut current =
            serde_json::to_value(&d.board.rules).map_err(|e| Error::invalid(e.to_string()))?;
        let obj = current
            .as_object_mut()
            .expect("rules serialize to an object");
        for (k, v) in p.rules {
            if k == "exclusions" {
                return Err(Error::invalid(
                    "Use exclude_violation or include_violation to change exclusions.",
                ));
            }
            if k == "net_classes" {
                let classes = obj
                    .entry("net_classes")
                    .or_insert_with(|| serde_json::Value::Object(Default::default()));
                let (Some(dst), Some(src)) = (classes.as_object_mut(), v.as_object()) else {
                    return Err(Error::invalid("net_classes must be a JSON object."));
                };
                for (name, c) in src {
                    dst.insert(name.clone(), c.clone());
                }
            } else if obj.contains_key(&k)
                || matches!(k.as_str(), "preset" | "severities" | "custom")
            {
                obj.insert(k, v);
            } else {
                return Err(Error::invalid(format!("The rule `{k}` is unknown.")));
            }
        }
        let rules: Rules =
            serde_json::from_value(current).map_err(|e| Error::invalid(format!("rules: {e}")))?;
        for rule in &rules.custom {
            if rule.name.trim().is_empty() {
                return Err(Error::invalid("A custom rule needs a non-empty name."));
            }
            if rule.when.layer.is_some_and(|l| {
                l.copper().is_none() || !l.exists_in(d.board.stackup.copper_layers)
            }) {
                return Err(Error::invalid(format!(
                    "Custom rule `{}` must select an existing copper layer.",
                    rule.name
                )));
            }
            for (field, value) in [
                ("clearance", rule.clearance),
                ("min_track_width", rule.min_track_width),
                ("min_via_drill", rule.min_via_drill),
                ("min_annular_ring", rule.min_annular_ring),
                ("hole_to_copper", rule.hole_to_copper),
                ("courtyard_clearance", rule.courtyard_clearance),
                ("max_track_width", rule.max_track_width),
                ("min_track_angle", rule.min_track_angle),
            ] {
                if value.is_some_and(|v| !v.is_finite() || v <= 0.0) {
                    return Err(Error::invalid(format!(
                        "Custom rule `{}`: {field} must be more than 0.",
                        rule.name
                    )));
                }
            }
        }
        for rule in &rules.custom {
            if let Some(area) = &rule.when.area {
                if !d.routing.areas.iter().any(|a| &a.id == area) {
                    return Err(Error::invalid(format!(
                        "Custom rule `{}`: the area `{area}` does not exist. Make it with `add_area`.",
                        rule.name
                    )));
                }
            }
            if rule
                .max_track_width
                .zip(rule.min_track_width)
                .is_some_and(|(max, min)| max < min)
            {
                return Err(Error::invalid(format!(
                    "Custom rule `{}`: max_track_width must not be below min_track_width.",
                    rule.name
                )));
            }
        }
        lamina_drc::registry::check_rules(&rules).map_err(Error::invalid)?;
        for (name, v) in [
            ("clearance", rules.clearance),
            ("track_width", rules.track_width),
            ("via_diameter", rules.via_diameter),
            ("via_drill", rules.via_drill),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(Error::invalid(format!("{name} must be more than 0.")));
            }
        }
        if rules.via_drill >= rules.via_diameter {
            return Err(Error::invalid("via_drill must be less than via_diameter."));
        }
        d.board.rules = rules;
        self.commit(d, Touched::file(FileKind::Board), p.strict, "rules updated")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct UseFabPreset {
    /// The name of the preset: `jlcpcb-2layer`, `jlcpcb-4layer` or `relaxed`.
    pub name: String,
    #[serde(default)]
    pub strict: bool,
}

impl Project {
    pub fn use_fab_preset(&mut self, p: UseFabPreset) -> Result<OpResult> {
        // A name is a preset, or the id of a fab profile.
        let layers = self.data.board.stackup.copper_layers;
        let profiles = lamina_drc::fab::profiles();
        let rules = match lamina_drc::presets::apply(&self.data.board.rules, &p.name) {
            Some(r) => r,
            None => match profiles.iter().find(|f| f.id == p.name) {
                Some(profile) => {
                    lamina_drc::presets::apply_profile(&self.data.board.rules, profile, layers)
                        .map_err(Error::invalid)?
                }
                None => {
                    let names: Vec<&str> = lamina_drc::presets::PRESETS
                        .iter()
                        .map(|x| x.name)
                        .chain(profiles.iter().map(|f| f.id.as_str()))
                        .collect();
                    return Err(Error::invalid(format!(
                        "The preset `{}` is unknown. Use one of these: {}.",
                        p.name,
                        names.join(", ")
                    )));
                }
            },
        };
        let mut d = self.data.clone();
        d.board.rules = rules;
        self.commit(
            d,
            Touched::file(FileKind::Board),
            p.strict,
            format!("rules from preset {}", p.name),
        )
    }
}

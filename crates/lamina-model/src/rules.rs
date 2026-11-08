use crate::{Layer, Rules};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleMatch {
    pub net: Option<String>,
    pub class: Option<String>,
    pub layer: Option<Layer>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomRule {
    pub name: String,
    #[serde(default)]
    pub when: RuleMatch,
    pub clearance: Option<f64>,
    pub min_track_width: Option<f64>,
    pub min_via_drill: Option<f64>,
    pub min_annular_ring: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuleValues {
    pub clearance: f64,
    pub min_track_width: f64,
    pub min_via_drill: f64,
    pub min_annular_ring: f64,
}

impl Rules {
    pub fn resolve(
        &self,
        net: Option<&str>,
        class: Option<&str>,
        layer: Option<Layer>,
    ) -> RuleValues {
        let mut values = RuleValues {
            clearance: self.clearance_for(class),
            min_track_width: self.min_track_width,
            min_via_drill: self.min_via_drill,
            min_annular_ring: self.min_annular_ring,
        };
        for rule in &self.custom {
            if rule
                .when
                .net
                .as_ref()
                .is_some_and(|p| !net.is_some_and(|n| wildcard(p, n)))
                || rule.when.class.as_deref().is_some_and(|c| Some(c) != class)
                || rule.when.layer.is_some_and(|l| Some(l) != layer)
            {
                continue;
            }
            if let Some(v) = rule.clearance {
                values.clearance = v;
            }
            if let Some(v) = rule.min_track_width {
                values.min_track_width = v;
            }
            if let Some(v) = rule.min_via_drill {
                values.min_via_drill = v;
            }
            if let Some(v) = rule.min_annular_ring {
                values.min_annular_ring = v;
            }
        }
        values
    }
}

/// Match a name with `*` and `?` wildcards.
pub fn wildcard(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut i, mut j, mut star, mut retry) = (0, 0, None, 0);
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            i += 1;
            retry = j;
        } else if let Some(s) = star {
            retry += 1;
            j = retry;
            i = s + 1;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}

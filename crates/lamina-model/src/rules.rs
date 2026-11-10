use crate::{Layer, Rules};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The type of the item that a custom rule is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    Track,
    Via,
    Pad,
    Zone,
    Hole,
}

/// The items that a custom rule is for. All the conditions must be true.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleMatch {
    /// A net name. `*` and `?` are wildcards.
    pub net: Option<String>,
    pub class: Option<String>,
    pub layer: Option<Layer>,
    /// The type of the item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<RuleKind>,
    /// The reference of the part that the item belongs to. `*` and `?` are wildcards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// The id of a rule area. The item must be in that area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
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
    /// The minimum distance from a hole wall to copper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hole_to_copper: Option<f64>,
    /// The minimum distance between the courtyard of a part and other courtyards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub courtyard_clearance: Option<f64>,
    /// The maximum track width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_track_width: Option<f64>,
    /// The minimum angle between two tracks that join, in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_track_angle: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuleValues {
    pub clearance: f64,
    pub min_track_width: f64,
    pub min_via_drill: f64,
    pub min_annular_ring: f64,
    pub hole_to_copper: f64,
    pub courtyard_clearance: f64,
    /// `None` is no maximum.
    pub max_track_width: Option<f64>,
    pub min_track_angle: f64,
}

/// The item that the rules are resolved for.
#[derive(Debug, Clone, Copy, Default)]
pub struct Subject<'a> {
    pub net: Option<&'a str>,
    pub class: Option<&'a str>,
    pub layer: Option<Layer>,
    pub kind: Option<RuleKind>,
    /// The reference of the part of the item.
    pub part: Option<&'a str>,
    /// The ids of the rule areas that the item is in.
    pub areas: &'a [&'a str],
}

impl CustomRule {
    /// True if the rule is for the subject.
    pub fn matches(&self, s: &Subject) -> bool {
        let w = &self.when;
        !(w.net
            .as_ref()
            .is_some_and(|p| !s.net.is_some_and(|n| wildcard(p, n)))
            || w.class.as_deref().is_some_and(|c| Some(c) != s.class)
            || w.layer.is_some_and(|l| Some(l) != s.layer)
            || w.kind.is_some_and(|k| Some(k) != s.kind)
            || w.part
                .as_ref()
                .is_some_and(|p| !s.part.is_some_and(|n| wildcard(p, n)))
            || w.area.as_deref().is_some_and(|a| !s.areas.contains(&a)))
    }
}

impl Rules {
    /// The limits for a net on a layer. A custom rule with a `kind`, `part`
    /// or `area` condition does not match here. Use [`Rules::resolve_for`]
    /// for an item.
    pub fn resolve(
        &self,
        net: Option<&str>,
        class: Option<&str>,
        layer: Option<Layer>,
    ) -> RuleValues {
        self.resolve_for(&Subject {
            net,
            class,
            layer,
            ..Default::default()
        })
    }

    /// The limits for an item. A later custom rule wins.
    pub fn resolve_for(&self, s: &Subject) -> RuleValues {
        let mut values = RuleValues {
            clearance: self.clearance_for(s.class),
            min_track_width: self.min_track_width,
            min_via_drill: self.min_via_drill,
            min_annular_ring: self.min_annular_ring,
            hole_to_copper: self.hole_to_copper,
            courtyard_clearance: self.courtyard_clearance,
            max_track_width: None,
            min_track_angle: self.min_track_angle,
        };
        for rule in self.custom.iter().filter(|r| r.matches(s)) {
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
            if let Some(v) = rule.hole_to_copper {
                values.hole_to_copper = v;
            }
            if let Some(v) = rule.courtyard_clearance {
                values.courtyard_clearance = v;
            }
            if let Some(v) = rule.max_track_width {
                values.max_track_width = Some(v);
            }
            if let Some(v) = rule.min_track_angle {
                values.min_track_angle = v;
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

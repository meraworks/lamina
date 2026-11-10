//! Fab capability presets.
//!
//! The values come from the standard capabilities that JLCPCB publishes for
//! HASL boards. Each value has a small margin above the absolute minimum.

use lamina_model::{CustomRule, RuleKind, RuleMatch, Rules};

pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub rules: fn() -> Rules,
}

fn jlc_2() -> Rules {
    Rules {
        preset: Some("jlcpcb-2layer".into()),
        clearance: 0.15,
        track_width: 0.25,
        min_track_width: 0.127,
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
        net_classes: Default::default(),
        ..Rules::default()
    }
}

fn jlc_4() -> Rules {
    Rules {
        preset: Some("jlcpcb-4layer".into()),
        clearance: 0.127,
        min_track_width: 0.09,
        via_diameter: 0.45,
        via_drill: 0.2,
        min_via_drill: 0.2,
        min_annular_ring: 0.1,
        min_drill: 0.2,
        ..jlc_2()
    }
}

fn relaxed() -> Rules {
    Rules {
        preset: Some("relaxed".into()),
        clearance: 0.2,
        track_width: 0.3,
        min_track_width: 0.2,
        via_diameter: 0.8,
        via_drill: 0.4,
        min_via_drill: 0.4,
        min_annular_ring: 0.15,
        min_drill: 0.4,
        hole_to_hole: 0.6,
        hole_to_copper: 0.3,
        copper_to_edge: 0.5,
        ..jlc_2()
    }
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "jlcpcb-2layer",
        description: "JLCPCB 1-2 layer standard: 0.127 mm minimum track, 0.3 mm via drill",
        rules: jlc_2,
    },
    Preset {
        name: "jlcpcb-4layer",
        description: "JLCPCB 4+ layer standard: 0.09 mm minimum track, 0.2 mm via drill",
        rules: jlc_4,
    },
    Preset {
        name: "relaxed",
        description: "Large margins for all fabs and for hand soldering",
        rules: relaxed,
    },
];

pub fn get(name: &str) -> Option<Rules> {
    PRESETS.iter().find(|p| p.name == name).map(|p| (p.rules)())
}

/// The rules of the board with the limits of a preset. The net classes, the
/// custom rules, the exclusions and the severities of the board stay.
pub fn apply(current: &Rules, name: &str) -> Option<Rules> {
    let mut rules = get(name)?;
    rules.net_classes = current.net_classes.clone();
    rules.custom = current.custom.clone();
    rules.exclusions = current.exclusions.clone();
    rules.severities = current.severities.clone();
    Some(rules)
}

/// The name of the custom rule that `apply_profile` makes for the ring of a via.
pub const FAB_VIA_RING: &str = "fab via ring";

/// The rules of the board with the limits of a fab profile for `layers`
/// copper layers. The limits come from the profile. The design values
/// (clearance, track width, via size) stay, but not below the limits. The
/// net classes, the custom rules, the exclusions and the severities stay.
pub fn apply_profile(
    current: &Rules,
    profile: &crate::fab::FabProfile,
    layers: u8,
) -> Result<Rules, String> {
    let tier = profile
        .tiers
        .iter()
        .find(|t| t.layers.0 <= layers && layers <= t.layers.1)
        .ok_or_else(|| format!("{} has no limits for {layers} copper layers.", profile.name))?;
    let get = |name: &str| tier.limits.get(name).copied();
    let mut r = current.clone();
    r.preset = Some(profile.id.clone());
    if let Some(v) = get("clearance") {
        r.clearance = r.clearance.max(v);
    }
    if let Some(v) = get("track_width") {
        r.min_track_width = v;
        r.track_width = r.track_width.max(v);
    }
    if let Some(v) = get("via_drill") {
        r.min_via_drill = v;
        r.via_drill = r.via_drill.max(v);
    }
    if let Some(v) = get("drill") {
        r.min_drill = v;
    }
    if let Some(v) = get("drill_max") {
        r.max_drill = v;
    }
    if let Some(v) = get("slot_width") {
        r.min_slot_width = v;
    }
    if let Some(v) = get("hole_to_hole") {
        r.hole_to_hole = v;
    }
    if let Some(v) = get("hole_to_copper") {
        r.hole_to_copper = v;
    }
    if let Some(v) = get("copper_to_edge") {
        r.copper_to_edge = v;
    }
    if let Some(v) = get("silk_line") {
        r.min_silk_width = v;
        r.silk_width = r.silk_width.max(v);
    }
    if let Some(v) = get("silk_text_height") {
        r.min_silk_text_height = v;
    }
    // A fab has one ring limit for pads and a smaller one for vias. The
    // board limit is the pad limit, and a custom rule gives the via limit.
    let pad_ring = get("pad_ring").or(get("via_ring"));
    if let Some(v) = pad_ring {
        r.min_annular_ring = v;
    }
    r.custom.retain(|c| c.name != FAB_VIA_RING);
    if let Some(via) = get("via_ring").filter(|v| pad_ring.is_some_and(|p| *v < p)) {
        r.custom.insert(
            0,
            CustomRule {
                name: FAB_VIA_RING.into(),
                when: RuleMatch {
                    kind: Some(RuleKind::Via),
                    ..Default::default()
                },
                min_annular_ring: Some(via),
                ..Default::default()
            },
        );
    }
    let via_ring = get("via_ring").unwrap_or(r.min_annular_ring);
    r.via_diameter = r
        .via_diameter
        .max(get("via_diameter").unwrap_or(0.0))
        .max(r.via_drill + 2.0 * via_ring);
    Ok(r)
}

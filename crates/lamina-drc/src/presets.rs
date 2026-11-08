//! Fab capability presets.
//!
//! The values come from the standard capabilities that JLCPCB publishes for
//! HASL boards. Each value has a small margin above the absolute minimum.

use lamina_model::Rules;

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

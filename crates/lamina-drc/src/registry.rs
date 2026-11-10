//! The list of all checks: one row for each rule id.

use lamina_model::{RuleSeverity, Rules, Severity, Violation};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Model,
    Connectivity,
    Copper,
    Holes,
    Edge,
    Place,
    Zones,
    Mask,
    Silk,
    Import,
    Assembly,
    Footprint,
    Labels,
    Electrical,
}

/// What a check is, for the tools and for the severity rules.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CheckInfo {
    pub id: &'static str,
    pub family: Family,
    pub default: RuleSeverity,
    /// A higher severity for release (`export`, `check` with `release`).
    pub release: Option<RuleSeverity>,
    /// The user can change the severity.
    pub overridable: bool,
    /// The user can exclude a violation.
    pub excludable: bool,
    /// What the check reports.
    pub summary: &'static str,
    /// What goes wrong if you ignore it.
    pub why: &'static str,
    /// The rule fields that the check reads.
    pub fields: &'static [&'static str],
}

const E: RuleSeverity = RuleSeverity::Error;
const W: RuleSeverity = RuleSeverity::Warning;
const OFF: RuleSeverity = RuleSeverity::Ignore;

const fn row(
    id: &'static str,
    family: Family,
    default: RuleSeverity,
    summary: &'static str,
    why: &'static str,
    fields: &'static [&'static str],
) -> CheckInfo {
    CheckInfo {
        id,
        family,
        default,
        release: None,
        overridable: true,
        excludable: true,
        summary,
        why,
        fields,
    }
}

/// A rule about broken data. The user cannot ignore or exclude it.
const fn fixed(id: &'static str, summary: &'static str, why: &'static str) -> CheckInfo {
    CheckInfo {
        id,
        family: Family::Model,
        default: E,
        release: None,
        overridable: false,
        excludable: false,
        summary,
        why,
        fields: &[],
    }
}

const fn release(mut c: CheckInfo, s: RuleSeverity) -> CheckInfo {
    c.release = Some(s);
    c
}

/// All checks, sorted by id.
pub const CHECKS: &[CheckInfo] = &[
    row(
        "annular_ring",
        Family::Holes,
        E,
        "The copper ring around a plated hole is too narrow.",
        "The drill can break out of the pad.",
        &["min_annular_ring"],
    ),
    fixed(
        "bad_outline",
        "The board outline is not a valid closed shape.",
        "The fab cannot cut the board.",
    ),
    row(
        "clearance",
        Family::Copper,
        E,
        "Copper of two nets is too near.",
        "The fab can make a short circuit.",
        &["clearance"],
    ),
    row(
        "copper_outside_board",
        Family::Edge,
        E,
        "Copper is outside the board outline.",
        "The copper is cut off.",
        &[],
    ),
    row(
        "copper_to_edge",
        Family::Edge,
        E,
        "Copper is too near to the board edge.",
        "The router bit can show or tear the copper.",
        &["copper_to_edge"],
    ),
    row(
        "courtyard_outside_board",
        Family::Place,
        W,
        "The courtyard of a part is partly outside the board.",
        "The part can hang over the edge.",
        &[],
    ),
    row(
        "courtyard_overlap",
        Family::Place,
        E,
        "The courtyards of two parts overlap.",
        "The parts can collide in assembly.",
        &[],
    ),
    row(
        "diff_pair_gap",
        Family::Copper,
        W,
        "The gap of a differential pair is not the specified gap.",
        "The pair impedance changes.",
        &[],
    ),
    row(
        "diff_pair_uncoupled",
        Family::Copper,
        W,
        "A part of a differential pair has no partner track beside it.",
        "The pair loses its coupling.",
        &[],
    ),
    row(
        "drill_size",
        Family::Holes,
        E,
        "A drill is smaller than the minimum.",
        "The fab cannot drill the hole.",
        &["min_drill"],
    ),
    row(
        "hole_to_copper",
        Family::Holes,
        E,
        "A hole wall is too near to copper of another net, or a non-plated hole is too near to copper.",
        "The drill can cut or short the copper.",
        &["hole_to_copper"],
    ),
    row(
        "hole_to_hole",
        Family::Holes,
        E,
        "Two holes are too near.",
        "The drill can break the wall between the holes.",
        &["hole_to_hole"],
    ),
    release(
        row(
            "import_loss",
            Family::Import,
            W,
            "An import dropped data that changes the fabrication files.",
            "The board is not the board of the source file.",
            &[],
        ),
        E,
    ),
    row(
        "isolated_copper",
        Family::Zones,
        W,
        "A piece of a zone connects to no pad.",
        "The copper has no function and can act as an antenna.",
        &[],
    ),
    row(
        "keepout",
        Family::Place,
        E,
        "An item is in a keep-out that forbids it.",
        "The keep-out protects an antenna, a hole or a mechanical part.",
        &[],
    ),
    fixed(
        "layer_not_in_stackup",
        "An item is on a copper layer that the board does not have.",
        "The item is not in the fabrication files.",
    ),
    row(
        "length_out_of_range",
        Family::Copper,
        W,
        "The track length of a net is not in its limits.",
        "The signal timing can be wrong.",
        &[],
    ),
    row(
        "mask_bridge",
        Family::Mask,
        W,
        "Too little solder mask stays between two openings of different nets.",
        "Solder can bridge the pads.",
        &["min_mask_web", "mask_margin"],
    ),
    fixed(
        "missing_footprint",
        "A part uses a footprint that is not in the project.",
        "The part has no pads.",
    ),
    row(
        "no_outline",
        Family::Model,
        W,
        "The board has no outline.",
        "The fab cannot cut the board.",
        &[],
    ),
    row(
        "npth_inside_courtyard",
        Family::Place,
        W,
        "A non-plated hole is in the courtyard of a part.",
        "The hole can be under the part.",
        &[],
    ),
    fixed(
        "pad_in_two_nets",
        "A pad is in two nets.",
        "The netlist is not valid.",
    ),
    row(
        "pin_role",
        Family::Model,
        W,
        "The name of a pin does not agree with its net.",
        "A ground pin on a power net is a short circuit.",
        &[],
    ),
    fixed(
        "placement_unknown_part",
        "A placement is for a part that does not exist.",
        "The project files do not agree.",
    ),
    row(
        "short",
        Family::Copper,
        E,
        "Copper of two nets touches.",
        "It is a short circuit.",
        &[],
    ),
    row(
        "short_nets",
        Family::Connectivity,
        E,
        "Two nets are connected through copper.",
        "It is a short circuit.",
        &[],
    ),
    row(
        "silk_overlap",
        Family::Silk,
        W,
        "A silkscreen text overlaps other silkscreen.",
        "The text is not readable.",
        &[],
    ),
    row(
        "silk_text_size",
        Family::Silk,
        W,
        "A silkscreen text is too small.",
        "The fab cannot print it clearly.",
        &["min_silk_text_height"],
    ),
    row(
        "silk_to_edge",
        Family::Silk,
        W,
        "Silkscreen is too near to the board edge.",
        "The fab cuts it off.",
        &["silk_to_edge"],
    ),
    row(
        "silk_to_pad",
        Family::Silk,
        W,
        "Silkscreen is too near to a pad.",
        "The fab removes silkscreen on mask openings.",
        &["silk_to_pad", "mask_margin"],
    ),
    row(
        "skew_out_of_range",
        Family::Copper,
        W,
        "The nets of a skew group differ too much in length.",
        "The signals do not arrive together.",
        &[],
    ),
    row(
        "stale_exclusion",
        Family::Model,
        W,
        "An exclusion matches no violation.",
        "The accepted problem is gone, or its items changed.",
        &[],
    ),
    row(
        "starved_thermal",
        Family::Zones,
        W,
        "A pad has too few thermal spokes to its zone.",
        "The connection can be weak.",
        &["min_thermal_spokes"],
    ),
    row(
        "track_dangling",
        Family::Connectivity,
        W,
        "An end of a track connects to nothing.",
        "The stub has no function and can act as an antenna.",
        &[],
    ),
    row(
        "track_points",
        Family::Model,
        E,
        "A track has less than two points.",
        "The track is not valid.",
        &[],
    ),
    row(
        "track_width",
        Family::Copper,
        E,
        "A track is narrower than the minimum.",
        "The fab cannot make it reliably.",
        &["min_track_width"],
    ),
    fixed(
        "unknown_net",
        "An item uses a net that does not exist.",
        "The netlist is not valid.",
    ),
    row(
        "unknown_net_class",
        Family::Model,
        W,
        "A net uses a net class that is not defined.",
        "The net gets the default limits.",
        &[],
    ),
    fixed(
        "unknown_pad",
        "A net uses a pad that the footprint does not have.",
        "The netlist is not valid.",
    ),
    release(
        row(
            "unplaced",
            Family::Place,
            W,
            "A part has no placement.",
            "The part is not on the board.",
            &[],
        ),
        E,
    ),
    row(
        "unresolved_pad",
        Family::Model,
        W,
        "A net uses a pad of a part that does not exist yet.",
        "The connection is not made.",
        &[],
    ),
    release(
        row(
            "unrouted",
            Family::Connectivity,
            W,
            "A net has open connections.",
            "The circuit is not complete.",
            &[],
        ),
        E,
    ),
    row(
        "via_dangling",
        Family::Connectivity,
        W,
        "A via connects to no pad.",
        "The via has no function.",
        &[],
    ),
    row(
        "via_drill",
        Family::Holes,
        E,
        "A via drill is smaller than the minimum.",
        "The fab cannot drill the hole.",
        &["min_via_drill"],
    ),
    // KiCad parity.
    row(
        "via_count",
        Family::Copper,
        W,
        "A net has more vias than its limit.",
        "Each via changes the impedance of a fast signal.",
        &[],
    ),
    row(
        "courtyard_missing",
        Family::Place,
        W,
        "A part has no courtyard drawing.",
        "The overlap check uses a box around the pads, which can be too small.",
        &[],
    ),
    row(
        "courtyard_open",
        Family::Place,
        W,
        "The courtyard of a part is not a closed loop.",
        "The overlap check cannot use it.",
        &[],
    ),
    row(
        "weak_connection",
        Family::Connectivity,
        W,
        "Two copper items of a net connect only where they touch.",
        "The fab can etch the join open.",
        &["min_connection"],
    ),
    row(
        "track_zero_length",
        Family::Copper,
        W,
        "A track has a segment of almost no length.",
        "It is a rest of an edit and can hide a break.",
        &[],
    ),
    row(
        "track_angle",
        Family::Copper,
        OFF,
        "A track has a corner below the minimum angle.",
        "Etch fluid can stay in a sharp corner (acid trap).",
        &["min_track_angle"],
    ),
    row(
        "zone_overlap",
        Family::Zones,
        W,
        "Two zones of different nets overlap with the same priority.",
        "The id of the zone decides which pour wins, not the design.",
        &[],
    ),
    row(
        "mask_missing",
        Family::Mask,
        W,
        "A pad gets solder paste but has no mask opening.",
        "The mask covers the pad and it cannot be soldered.",
        &[],
    ),
    row(
        "silk_on_hole",
        Family::Silk,
        W,
        "Silkscreen is on a drilled hole.",
        "The ink goes into the hole or is cut off.",
        &["silk_to_pad"],
    ),
    row(
        "silk_outside_board",
        Family::Silk,
        W,
        "Silkscreen is outside the board.",
        "The fab does not print it.",
        &[],
    ),
    row(
        "silk_line_width",
        Family::Silk,
        W,
        "A silkscreen line is thinner than the minimum.",
        "The fab cannot print it.",
        &["min_silk_width"],
    ),
    row(
        "text_variable",
        Family::Silk,
        W,
        "A text has a variable with no value.",
        "The board shows the variable name.",
        &[],
    ),
    // Footprint evidence.
    row(
        "footprint_mismatch",
        Family::Footprint,
        E,
        "The last check of a footprint found a difference.",
        "The part cannot be soldered on a wrong land pattern.",
        &[],
    ),
    row(
        "footprint_stale",
        Family::Footprint,
        W,
        "The pads of a footprint changed after its check.",
        "The evidence is for other pads.",
        &[],
    ),
    row(
        "footprint_changed",
        Family::Footprint,
        W,
        "The pads of a footprint differ from its source, and no check covers them.",
        "A hand edit can be wrong.",
        &[],
    ),
    release(
        row(
            "footprint_unverified",
            Family::Footprint,
            OFF,
            "A footprint in use has no evidence.",
            "No check finds a footprint that is wrong for its part. Only a comparison with the datasheet does.",
            &[],
        ),
        E,
    ),
    row(
        "footprint_symmetry",
        Family::Footprint,
        W,
        "Some pads of a footprint are a small distance off their mirror position.",
        "A land pattern is symmetric in most packages. A small difference is frequently an error.",
        &[],
    ),
    // Fab checks.
    row(
        "hole_to_edge",
        Family::Holes,
        E,
        "A hole is too near to the board edge.",
        "The wall between the hole and the edge breaks.",
        &["hole_to_edge"],
    ),
    row(
        "drill_aspect",
        Family::Holes,
        W,
        "The smallest plated drill is too small for the board thickness.",
        "The plating does not reach the middle of the hole.",
        &["max_drill_aspect"],
    ),
    row(
        "mask_sliver",
        Family::Mask,
        W,
        "A thin strip of mask stays between two openings of one net.",
        "The strip breaks off and can be under a part.",
        &["min_mask_web"],
    ),
    row(
        "board_neck",
        Family::Edge,
        W,
        "A cut-out is narrower than the router bit.",
        "The fab cannot mill it.",
        &["min_route_width"],
    ),
    // Assembly and BOM.
    row(
        "fiducial_count",
        Family::Assembly,
        W,
        "A side with SMD parts has too few fiducials.",
        "The placement machine cannot find the board position.",
        &["min_fiducials"],
    ),
    row(
        "part_to_edge",
        Family::Assembly,
        W,
        "A part is too near to the board edge.",
        "The conveyor or the panel tabs need this space.",
        &["part_to_edge"],
    ),
    row(
        "part_height",
        Family::Assembly,
        E,
        "A part is higher than the limit of the area that it is in.",
        "The part does not fit in the housing.",
        &[],
    ),
    row(
        "paste_missing",
        Family::Assembly,
        W,
        "A pad with a net has no paste, in a part that has paste on other pads.",
        "The pad gets no solder.",
        &[],
    ),
    row(
        "paste_oversize",
        Family::Assembly,
        W,
        "A paste opening is larger than its pad.",
        "Solder balls and bridges can form.",
        &["paste_margin"],
    ),
    row(
        "tombstone",
        Family::Assembly,
        OFF,
        "A small two-pad part has much more copper on one pad than on the other.",
        "The pads do not heat at the same speed, and the part can stand up.",
        &[],
    ),
    row(
        "via_in_pad",
        Family::Assembly,
        W,
        "A via is in an SMD pad.",
        "Solder flows into the hole and the joint is weak.",
        &[],
    ),
    row(
        "testpoint_access",
        Family::Assembly,
        W,
        "A probe cannot use a test point.",
        "The board cannot be tested.",
        &["min_testpoint", "testpoint_to_edge"],
    ),
    release(
        row(
            "bom_part_number",
            Family::Assembly,
            OFF,
            "A BOM line has no manufacturer part number and no LCSC number.",
            "The assembler cannot buy the part.",
            &[],
        ),
        W,
    ),
    row(
        "bom_conflict",
        Family::Assembly,
        W,
        "One part number has different values or footprints, or one value has two part numbers.",
        "A wrong part can be on the board.",
        &[],
    ),
    // What the silkscreen says.
    row(
        "silk_under_part",
        Family::Labels,
        W,
        "A text is below the body of a part.",
        "The part hides the text after assembly.",
        &[],
    ),
    row(
        "label_far",
        Family::Labels,
        W,
        "A text names a part but is far from it, or nearer to a different part.",
        "A person reads the label for the wrong part.",
        &["label_max_distance"],
    ),
    row(
        "label_pin",
        Family::Labels,
        W,
        "Pin labels of a connector are at the wrong pins.",
        "A person connects the cable in the wrong order.",
        &[],
    ),
    row(
        "polarity_mark",
        Family::Labels,
        W,
        "A plus or minus mark is at the pad of the opposite polarity.",
        "A person connects the supply with the wrong polarity.",
        &[],
    ),
    // Electrical checks that need no schematic.
    row(
        "net_single_pad",
        Family::Electrical,
        W,
        "A net has only one pad.",
        "The net connects nothing. A connection is possibly missing.",
        &[],
    ),
    row(
        "pad_no_net",
        Family::Electrical,
        OFF,
        "A part has pads in no net.",
        "A pin is possibly not connected by mistake.",
        &[],
    ),
    row(
        "track_current",
        Family::Electrical,
        W,
        "A track is too narrow for the current of its net.",
        "The track becomes hot.",
        &["max_temp_rise"],
    ),
    row(
        "voltage_clearance",
        Family::Electrical,
        E,
        "Copper of two nets is too near for the voltage between them.",
        "An arc or a leakage current can occur.",
        &[],
    ),
    row(
        "decoupling_far",
        Family::Electrical,
        W,
        "A power pin has no capacitor near it.",
        "The supply of the part is not stable.",
        &["decoupling_max_distance"],
    ),
    row(
        "return_path",
        Family::Electrical,
        W,
        "A track of a fast signal goes over a gap in the pour of the next layer.",
        "The signal radiates and its impedance changes.",
        &[],
    ),
    row(
        "pin1_mark",
        Family::Footprint,
        W,
        "A footprint has the same pads after a turn, and no silkscreen mark shows pad 1, or the mark is at the wrong pad.",
        "A person puts the part on the board turned.",
        &[],
    ),
    fixed(
        "zone_outline",
        "The outline of a zone or keep-out is not a valid shape.",
        "The pour cannot be calculated.",
    ),
];

/// The ids of all checks.
pub const RULE_NAMES: [&str; CHECKS.len()] = {
    let mut a = [""; CHECKS.len()];
    let mut i = 0;
    while i < CHECKS.len() {
        a[i] = CHECKS[i].id;
        i += 1;
    }
    a
};

pub fn get(id: &str) -> Option<&'static CheckInfo> {
    CHECKS.iter().find(|c| c.id == id)
}

/// Check the parts of the rules that refer to checks.
pub fn check_rules(rules: &Rules) -> Result<(), String> {
    for name in rules.severities.keys() {
        match get(name) {
            None => {
                return Err(format!(
                    "The check `{name}` is unknown. Valid checks: {}.",
                    RULE_NAMES.join(", ")
                ));
            }
            Some(c) if !c.overridable => {
                return Err(format!(
                    "The severity of the check `{name}` cannot change, because it reports data that is not valid."
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn level(s: RuleSeverity) -> Option<Severity> {
    match s {
        RuleSeverity::Error => Some(Severity::Error),
        RuleSeverity::Warning => Some(Severity::Warning),
        RuleSeverity::Ignore => None,
    }
}

/// The violations as `export` sees them: some rules are errors for release,
/// and some rules are on only for release. A severity that the user set wins.
pub fn release_view(a: &crate::Analysis, rules: &Rules) -> Vec<Violation> {
    let mut out: Vec<Violation> = a
        .violations
        .iter()
        .cloned()
        .map(|mut v| {
            if !rules.severities.contains_key(&v.rule) {
                if let Some(Severity::Error) = get(&v.rule).and_then(|c| c.release).and_then(level)
                {
                    v.severity = Severity::Error;
                }
            }
            v
        })
        .collect();
    out.extend(a.release_only.iter().cloned());
    out
}

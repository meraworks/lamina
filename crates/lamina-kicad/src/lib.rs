//! KiCad compatibility: read `.kicad_mod` footprints and `.kicad_pcb`
//! boards, write boards KiCad can open, and find footprints in local or
//! online libraries. Nothing here runs KiCad.

pub mod footprint;
pub mod format;
pub mod layers;
pub mod library;
pub mod pcb;
pub mod project;
pub mod provider;

pub use lamina_model::sexpr;

pub use footprint::{Instance, KicadModel, convert, footprint_id, parse_footprint};
pub use format::KicadFormat;
pub use library::{DirSource, Hit, HttpSource, KicadSource};
pub use pcb::{ImportedBoard, export_board, export_board_with, import_board};
pub use project::{apply_project, export_project, export_rules};

/// The KiCad DRC item that reports the same defect as a Lamina rule.
pub fn kicad_rule_name(rule: &str) -> Option<&'static str> {
    Some(match rule {
        "clearance" => "clearance",
        "short" => "shorting_items",
        "track_width" => "track_width",
        "annular_ring" => "annular_width",
        "drill_size" => "drill_out_of_range",
        "hole_to_hole" => "hole_to_hole",
        "hole_to_copper" => "hole_clearance",
        "copper_to_edge" => "copper_edge_clearance",
        "unrouted" => "unconnected_items",
        "courtyard_overlap" => "courtyards_overlap",
        "courtyard_missing" => "missing_courtyard",
        "courtyard_open" => "malformed_courtyard",
        "track_dangling" => "track_dangling",
        "via_dangling" => "via_dangling",
        "weak_connection" => "connection_width",
        "track_zero_length" => "track_segment_length",
        "track_angle" => "track_angle",
        "zone_overlap" => "zones_intersect",
        "via_count" => "too_many_vias",
        "silk_to_pad" => "silk_over_copper",
        "silk_overlap" => "silk_overlap",
        "silk_to_edge" => "silk_edge_clearance",
        "silk_text_size" => "text_height",
        "mask_bridge" => "solder_mask_bridge",
        "starved_thermal" => "starved_thermal",
        "isolated_copper" => "isolated_copper",
        "keepout" => "items_not_allowed",
        "bad_outline" => "invalid_outline",
        "layer_not_in_stackup" => "item_on_disabled_layer",
        "text_variable" => "unresolved_variable",
        "npth_inside_courtyard" => "npth_inside_courtyard",
        "length" => "length_out_of_range",
        "skew" => "skew_out_of_range",
        _ => return None,
    })
}

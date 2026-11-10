//! A KiCad footprint keeps its keep-out zones and its paste openings.

use lamina_model::*;

fn esp32() -> Footprint {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/footprints/RF_Module.pretty/ESP32-S3-WROOM-1.kicad_mod");
    let src = std::fs::read_to_string(path).unwrap();
    lamina_kicad::parse_footprint(&src, "RF_Module", "test")
        .unwrap()
        .footprint
}

#[test]
fn footprint_keepout_survives_import() {
    let fp = esp32();
    assert_eq!(fp.keepouts.len(), 1, "the antenna keep-out");
    let k = &fp.keepouts[0];
    assert!(k.forbid.pours && k.forbid.tracks && k.forbid.vias);
    assert_eq!(k.scope, KeepoutScope::AllCopper);
    // The antenna is at the top end of the module (Y up).
    let b = k.outline.bbox().unwrap();
    assert!(b.min.y > 5.0, "{b:?}");
}

#[test]
fn paste_only_pads_become_paste_drawings() {
    let fp = esp32();
    let paste: Vec<&FpGraphic> = fp
        .graphics
        .iter()
        .filter(|g| g.layer == FpLayer::Paste)
        .collect();
    assert_eq!(paste.len(), 9, "the paste of the ground pad is 9 openings");
    assert!(paste.iter().all(|g| g.filled));
    // The ground pad itself has no paste: the 9 openings give it.
    let ground: Vec<&Pad> = fp.pads.iter().filter(|p| p.number == "41").collect();
    assert!(!ground.is_empty());
    assert!(ground.iter().all(|p| !p.paste));
}

#[test]
fn keepout_of_a_placed_footprint_is_in_the_export() {
    let mut d = ProjectData::new("t");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 60.0, 60.0));
    let fp = esp32();
    let id = fp.id.clone();
    d.footprints.insert(id.clone(), fp);
    d.parts.parts.insert("U1".into(), Part::new(&id));
    d.placement
        .placements
        .insert("U1".into(), Placement::at(30.0, 30.0));
    let text = lamina_kicad::export_board(&d);
    assert!(
        text.contains("U1-k1"),
        "the footprint keep-out is a board zone"
    );
    assert!(text.contains("(copperpour not_allowed)"));
}

#[test]
fn project_rules_read_the_board_minimums() {
    let mut d = ProjectData::new("t");
    let pro = r#"{"board":{"design_settings":{"rules":{
        "min_clearance":0.3,"min_hole_clearance":0.25,"min_text_height":1.0,"min_resolved_spokes":1}}},
        "net_settings":{"classes":[{"name":"Default","clearance":0.2}]}}"#;
    lamina_kicad::apply_project(&mut d, pro).unwrap();
    let r = &d.board.rules;
    assert_eq!(
        r.clearance, 0.3,
        "the board minimum is above the class value"
    );
    assert_eq!(r.hole_to_copper, 0.25);
    assert_eq!(r.min_silk_text_height, 1.0);
    assert_eq!(r.min_thermal_spokes, 1);
    let out = lamina_kicad::export_project(&d, "t");
    assert!(out.contains("\"min_hole_clearance\": 0.25"), "{out}");
}

#[test]
fn reference_is_hidden_when_only_the_fab_layer_has_it() {
    let mut d = ProjectData::new("t");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 60.0, 60.0));
    let mut fp = esp32();
    fp.texts
        .retain(|t| !(t.text == "${ref}" && t.layer == FpLayer::Silk));
    assert!(
        fp.texts.iter().any(|t| t.text == "${ref}"),
        "the Fab reference stays"
    );
    let id = fp.id.clone();
    d.footprints.insert(id.clone(), fp);
    d.parts.parts.insert("U1".into(), Part::new(&id));
    d.placement
        .placements
        .insert("U1".into(), Placement::at(30.0, 30.0));
    let text = lamina_kicad::export_board(&d);
    let at = text.find("(property \"Reference\" \"U1\"").unwrap();
    let end = text[at..].find("(property \"Value\"").unwrap();
    assert!(
        text[at..at + end].contains("(hide yes)"),
        "{}",
        &text[at..at + end]
    );
}

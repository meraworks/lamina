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

#[test]
fn edge_drawings_of_a_footprint_become_board_cutouts() {
    // A board of 20 x 20 mm with one footprint at (10, 10), turned 90 degrees.
    // The footprint has a circle and a rectangle on Edge.Cuts.
    let text = r#"(kicad_pcb (version 20240108) (generator "test")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (gr_rect (start 0 0) (end 20 20) (layer "Edge.Cuts") (stroke (width 0.1) (type default)))
  (footprint "lib:SLOT" (layer "F.Cu") (at 10 10 90)
    (property "Reference" "J1" (at 0 0 0) (layer "F.SilkS"))
    (fp_circle (center 0 0) (end 1 0) (layer "Edge.Cuts") (stroke (width 0.1) (type default)))
    (fp_rect (start 3 -1) (end 5 1) (layer "Edge.Cuts") (stroke (width 0.1) (type default)))
    (pad "1" smd rect (at 0 4) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  )
)"#;
    let b = lamina_kicad::import_board(text, "slots").unwrap();
    let d = &b.data;
    assert!(d.board.outline.is_some());
    assert_eq!(d.board.cutouts.len(), 2, "{:?}", b.notes);
    let boxes: Vec<lamina_model::Rect> = d
        .board
        .cutouts
        .iter()
        .map(|c| lamina_model::Rect::from_points(c.flatten(0.01)).unwrap())
        .collect();
    let near = |a: f64, b: f64| (a - b).abs() < 0.02;
    // The circle stays at the part position.
    assert!(boxes.iter().any(|r| near(r.min.x, 9.0)
        && near(r.max.x, 11.0)
        && near(r.min.y, 9.0)
        && near(r.max.y, 11.0)));
    // The rectangle is 4 mm from the part, and its size is 2 x 2 mm.
    assert!(boxes.iter().any(|r| {
        let (cx, cy) = ((r.min.x + r.max.x) / 2.0, (r.min.y + r.max.y) / 2.0);
        near(r.max.x - r.min.x, 2.0)
            && near(r.max.y - r.min.y, 2.0)
            && near((cx - 10.0).hypot(cy - 10.0), 4.0)
    }));
}

#[test]
fn zone_parts_that_lamina_does_not_have_give_notes() {
    let text = r#"(kicad_pcb (version 20240108) (generator "test")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "GND")
  (gr_rect (start 0 0) (end 20 20) (layer "Edge.Cuts") (stroke (width 0.1) (type default)))
  (gr_text_box "note" (start 1 1) (end 5 3) (layer "F.SilkS"))
  (zone (net 1) (net_name "GND") (layer "F.Cu") (hatch edge 0.5)
    (connect_pads thru_hole_only (clearance 0.2))
    (min_thickness 0.2)
    (fill yes (mode hatch) (thermal_gap 0.3) (thermal_bridge_width 0.3))
    (polygon (pts (xy 1 1) (xy 9 1) (xy 9 9) (xy 1 9)))
    (polygon (pts (xy 11 11) (xy 19 11) (xy 19 19) (xy 11 19)))
  )
)"#;
    let b = lamina_kicad::import_board(text, "zones").unwrap();
    assert_eq!(b.data.routing.zones.len(), 1);
    let has = |s: &str| b.notes.iter().any(|n| n.contains(s));
    assert!(has("thru_hole_only"), "{:?}", b.notes);
    assert!(has("hatch pattern"), "{:?}", b.notes);
    assert!(has("1 more outline(s) of the zone z1"), "{:?}", b.notes);
    assert!(has("1 text box(es)"), "{:?}", b.notes);
}

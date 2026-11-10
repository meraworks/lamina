use lamina_drc::*;
use lamina_footprint::{Options, from_package};
use lamina_model::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("t");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    for pkg in ["R_0603", "PinHeader_1x03_P2.54"] {
        let fp = from_package(pkg, &Options::default(), None).unwrap();
        d.footprints.insert(fp.id.clone(), fp);
    }
    d.parts.parts.insert("R1".into(), Part::new("R_0603"));
    d.parts.parts.insert("R2".into(), Part::new("R_0603"));
    d.placement
        .placements
        .insert("R1".into(), Placement::at(10.0, 10.0));
    d.placement
        .placements
        .insert("R2".into(), Placement::at(20.0, 10.0));
    d.nets.nets.insert(
        "A".into(),
        Net {
            pads: vec!["R1.2".parse().unwrap(), "R2.1".parse().unwrap()],
            class: None,
            ..Default::default()
        },
    );
    d.nets.nets.insert(
        "B".into(),
        Net {
            pads: vec!["R1.1".parse().unwrap()],
            class: None,
            ..Default::default()
        },
    );
    d
}

fn rules(a: &Analysis) -> Vec<&str> {
    a.violations.iter().map(|v| v.rule.as_str()).collect()
}

#[test]
fn review_footprint_text_size_uses_placed_side_and_position() {
    let mut d = board();
    d.board.rules.min_silk_text_height = 3.0;
    let pl = d.placement.placements.get_mut("R1").unwrap();
    pl.side = Side::Bottom;
    pl.rotation = 90.0;
    let text = d.footprints["R_0603"]
        .texts
        .iter()
        .find(|t| t.layer == FpLayer::Silk && t.text == "${ref}")
        .unwrap();
    let expected = pl.transform(text.at);
    let a = analyze(&d);
    let hit = a
        .violations
        .iter()
        .find(|v| v.rule == "silk_text_size" && v.items == vec!["R1"])
        .expect("small reference label");
    assert_eq!(hit.at, Some(expected));
    assert_eq!(hit.layer, Some(Layer::Silk(Side::Bottom)));
}

#[test]
fn severity_overrides_preserve_connectivity() {
    let mut d = board();
    let original = analyze(&d);
    assert_eq!(only(&original, "unrouted")[0].severity, Severity::Warning);
    d.board
        .rules
        .severities
        .insert("unrouted".into(), RuleSeverity::Error);
    assert_eq!(only(&analyze(&d), "unrouted")[0].severity, Severity::Error);
    d.board
        .rules
        .severities
        .insert("unrouted".into(), RuleSeverity::Ignore);
    let ignored = analyze(&d);
    assert!(only(&ignored, "unrouted").is_empty());
    assert_eq!(ignored.ratsnest, original.ratsnest);
    let (pad, _) = r1_pad_2(&d);
    track(&mut d, "A", vec![pad, Point::new(15.0, 5.0)]);
    d.board
        .rules
        .severities
        .insert("track_dangling".into(), RuleSeverity::Error);
    assert_eq!(
        only(&analyze(&d), "track_dangling")[0].severity,
        Severity::Error
    );
    d.routing.tracks[0].width = 0.01;
    assert_eq!(
        only(&analyze(&d), "track_width")[0].severity,
        Severity::Error
    );
    d.board
        .rules
        .severities
        .insert("track_width".into(), RuleSeverity::Warning);
    assert_eq!(
        only(&analyze(&d), "track_width")[0].severity,
        Severity::Warning
    );
}

#[test]
fn unrouted_then_routed() {
    let mut d = board();
    let a = analyze(&d);
    assert!(rules(&a).contains(&"unrouted"), "{:?}", a.violations);
    assert_eq!(a.ratsnest.len(), 1);
    assert_eq!(a.ratsnest[0].net, "A");
    let r1p2 = d.footprints["R_0603"]
        .pad("2")
        .unwrap()
        .at
        .add(Point::new(10.0, 10.0));
    let r2p1 = d.footprints["R_0603"]
        .pad("1")
        .unwrap()
        .at
        .add(Point::new(20.0, 10.0));
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "A".into(),
        layer: Layer::TOP,
        width: 0.25,
        points: vec![r1p2, r2p1],
    });
    let a = analyze(&d);
    assert!(a.ratsnest.is_empty(), "{:?}", a.ratsnest);
    assert!(!rules(&a).contains(&"unrouted"));
    assert!(
        a.violations.iter().all(|v| v.severity != Severity::Error),
        "{:?}",
        a.violations
    );
}

#[test]
fn clearance_and_short() {
    let mut d = board();
    // A track of net B, 0.05 mm above pad 1 of R2 (net A).
    let p = d.footprints["R_0603"].pad("1").unwrap().clone();
    let top = 10.0 + p.shape.size().1 / 2.0;
    d.routing.tracks.push(Track {
        id: "t9".into(),
        net: "B".into(),
        layer: Layer::TOP,
        width: 0.2,
        points: vec![Point::new(15.0, top + 0.15), Point::new(25.0, top + 0.15)],
    });
    let a = analyze(&d);
    assert!(rules(&a).contains(&"clearance"), "{:?}", a.violations);
    // Move it onto the pad. This makes a short.
    d.routing.tracks[0].points = vec![Point::new(15.0, 10.0), Point::new(25.0, 10.0)];
    let a = analyze(&d);
    assert!(rules(&a).contains(&"short"), "{:?}", a.violations);
    assert!(rules(&a).contains(&"short_nets"), "{:?}", a.violations);
}

#[test]
fn edge_courtyard_drill() {
    let mut d = board();
    d.placement
        .placements
        .insert("R2".into(), Placement::at(10.5, 10.0));
    d.parts
        .parts
        .insert("J1".into(), Part::new("PinHeader_1x03_P2.54"));
    d.placement
        .placements
        .insert("J1".into(), Placement::at(0.2, 5.0));
    let a = analyze(&d);
    let r = rules(&a);
    assert!(r.contains(&"courtyard_overlap"), "{r:?}");
    assert!(
        r.contains(&"copper_outside_board") || r.contains(&"copper_to_edge"),
        "{r:?}"
    );
    d.board.rules.min_drill = 1.2;
    let a = analyze(&d);
    assert!(rules(&a).contains(&"drill_size"));
}

#[test]
fn via_rules() {
    let mut d = board();
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "A".into(),
        at: Point::new(5.0, 5.0),
        diameter: 0.45,
        drill: 0.25,
        from: None,
        to: None,
    });
    let a = analyze(&d);
    let r = rules(&a);
    assert!(
        r.contains(&"via_drill") && r.contains(&"annular_ring"),
        "{r:?}"
    );
}

#[test]
fn pour_connects_pads() {
    let mut d = board();
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("A".into()),
        layers: vec![Layer::TOP],
        outline: Contour::rect(0.0, 0.0, 30.0, 20.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Thermal,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    let a = analyze(&d);
    assert_eq!(a.fills.len(), 1);
    assert!(!a.fills[0].polygons.is_empty());
    assert!(
        a.ratsnest.is_empty(),
        "the pour must connect R1.2 and R2.1: {:?}",
        a.ratsnest
    );
    assert!(
        a.violations.iter().all(|v| v.severity != Severity::Error),
        "{:?}",
        a.violations
    );
    // The fill keeps clear of the pad of net B.
    let b_pad = lamina_geom::pad_prim(
        d.footprints["R_0603"].pad("1").unwrap(),
        &d.placement.placements["R1"],
    );
    for p in &a.fills[0].polygons {
        assert!(
            p.prim().distance(&b_pad) >= 0.149,
            "{}",
            p.prim().distance(&b_pad)
        );
    }
}

#[test]
fn mask_opening_near_a_filled_zone_warns() {
    let mut d = board();
    d.nets.nets.remove("B");
    d.board.rules.mask_margin = 0.3;
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("A".into()),
        layers: vec![Layer::TOP],
        outline: Contour::rect(0.0, 0.0, 30.0, 20.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Solid,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    let a = analyze(&d);
    assert!(
        a.violations
            .iter()
            .any(|v| v.rule == "mask_bridge" && v.items == ["R1.1", "z1"])
    );
    assert!(
        !a.violations
            .iter()
            .any(|v| v.rule == "mask_bridge" && v.items == ["R1.2", "z1"])
    );
    d.board.rules.mask_margin = 0.0;
    assert!(
        !analyze(&d)
            .violations
            .iter()
            .any(|v| v.rule == "mask_bridge" && v.items.contains(&"z1".into()))
    );
}

#[test]
fn presets_exist() {
    assert!(presets::get("jlcpcb-2layer").is_some());
    assert!(presets::get("nope").is_none());
}

#[test]
fn two_pours_of_a_net_join_where_they_overlap() {
    let zone = |id: &str, x: f64, w: f64| Zone {
        id: id.into(),
        net: Some("A".into()),
        layers: vec![Layer::TOP],
        outline: Contour::rect(x, 5.0, w, 10.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Solid,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    };
    // R1.2 is near x 10.8 and R2.1 is near x 19.2. Put one pour around each pad.
    let mut d = board();
    d.routing.zones.push(zone("left", 10.5, 5.0));
    d.routing.zones.push(zone("right", 14.0, 6.0));
    let a = analyze(&d);
    assert!(
        a.ratsnest.is_empty(),
        "pours that overlap must join: {:?}",
        a.ratsnest
    );
    // If there is a gap between the pours, the pads stay separate.
    let mut d = board();
    d.routing.zones.push(zone("left", 10.5, 3.0));
    d.routing.zones.push(zone("right", 16.0, 4.0));
    let a = analyze(&d);
    assert_eq!(a.ratsnest.len(), 1, "{:?}", a.ratsnest);
}

/// The centre and the size of pad 2 of R1, on the board.
fn r1_pad_2(d: &ProjectData) -> (Point, (f64, f64)) {
    let pad = d.footprints["R_0603"].pad("2").unwrap();
    (
        Point::new(10.0 + pad.at.x, 10.0 + pad.at.y),
        pad.shape.size(),
    )
}

fn board_text(d: &mut ProjectData, layer: Layer, at: Point) {
    d.routing.texts.push(BoardText {
        id: "txt1".into(),
        layer,
        text: "AB".into(),
        at,
        size: 0.8,
        rotation: 0.0,
    });
}

fn silk_warnings(d: &ProjectData) -> Vec<Violation> {
    analyze(d)
        .violations
        .into_iter()
        .filter(|v| v.rule == "silk_to_pad")
        .collect()
}

#[test]
fn silk_text_on_a_pad_warns() {
    let mut d = board();
    let (at, _) = r1_pad_2(&d);
    board_text(&mut d, Layer::Silk(Side::Top), at);
    let w = silk_warnings(&d);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].severity, Severity::Warning);
    assert!(w[0].message.contains("R1.2"), "{}", w[0].message);
    assert!(w[0].message.contains("AB"), "{}", w[0].message);
}

#[test]
fn silk_text_clear_of_the_pads_is_quiet() {
    let mut d = board();
    board_text(&mut d, Layer::Silk(Side::Top), Point::new(15.0, 16.0));
    assert!(silk_warnings(&d).is_empty());
}

#[test]
fn silk_on_the_other_side_of_a_smd_pad_is_quiet() {
    let mut d = board();
    let (at, _) = r1_pad_2(&d);
    board_text(&mut d, Layer::Silk(Side::Bottom), at);
    assert!(silk_warnings(&d).is_empty());
}

#[test]
fn a_footprint_label_on_its_pad_warns() {
    let mut d = board();
    let at = d.footprints["R_0603"].pad("2").unwrap().at;
    d.footprints.get_mut("R_0603").unwrap().texts.push(FpText {
        layer: FpLayer::Silk,
        text: "${ref}".into(),
        at,
        size: 0.8,
        rotation: 0.0,
    });
    let w = silk_warnings(&d);
    assert!(w.iter().any(|v| v.message.contains("R1.2")), "{w:?}");
    assert!(w.iter().any(|v| v.message.contains("R2.2")), "{w:?}");
}

#[test]
fn a_footprint_outline_only_needs_to_clear_its_own_mask_opening() {
    let mut d = board();
    let (pad_x, w) = {
        let pad = d.footprints["R_0603"].pad("2").unwrap();
        (pad.at.x, pad.shape.size().0)
    };
    let margin = d.board.rules.mask_margin;
    let line_at = |x: f64| FpGraphic {
        layer: FpLayer::Silk,
        shape: Shape::Line {
            a: Point::new(x, -1.0),
            b: Point::new(x, 1.0),
        },
        width: 0.15,
        filled: false,
    };
    let edge = pad_x + w / 2.0;
    let fp = d.footprints.get_mut("R_0603").unwrap();
    fp.graphics.retain(|g| g.layer != FpLayer::Silk);
    fp.graphics.push(line_at(edge + 0.075 + margin + 0.02));
    assert!(silk_warnings(&d).is_empty());
    let fp = d.footprints.get_mut("R_0603").unwrap();
    *fp.graphics.last_mut().unwrap() = line_at(edge + 0.075 + margin - 0.02);
    assert_eq!(silk_warnings(&d).len(), 2);
}

#[test]
fn silk_line_uses_the_silk_to_pad_limit() {
    let line = |x: f64| BoardGraphic {
        id: "g1".into(),
        layer: Layer::Silk(Side::Top),
        shape: Shape::Line {
            a: Point::new(x, 8.0),
            b: Point::new(x, 12.0),
        },
        width: 0.15,
        filled: false,
    };
    let mut d = board();
    let (c, (w, _)) = r1_pad_2(&d);
    let edge = c.x + w / 2.0;
    let limit = d.board.rules.silk_to_pad;
    d.routing.graphics.push(line(edge + 0.075 + limit - 0.01));
    assert_eq!(silk_warnings(&d).len(), 1);
    d.routing.graphics[0] = line(edge + 0.075 + limit + 0.01);
    assert!(silk_warnings(&d).is_empty());
}

fn only<'a>(a: &'a Analysis, rule: &str) -> Vec<&'a Violation> {
    a.violations.iter().filter(|v| v.rule == rule).collect()
}

fn text(d: &mut ProjectData, id: &str, at: Point, size: f64) {
    d.routing.texts.push(BoardText {
        id: id.into(),
        layer: Layer::Silk(Side::Top),
        text: "ABC".into(),
        at,
        size,
        rotation: 0.0,
    });
}

fn track(d: &mut ProjectData, net: &str, points: Vec<Point>) {
    let id = format!("t{}", d.routing.tracks.len());
    d.routing.tracks.push(Track {
        id,
        net: net.into(),
        layer: Layer::TOP,
        width: 0.25,
        points,
    });
}

#[test]
fn silk_texts_on_each_other_warn() {
    let mut d = board();
    text(&mut d, "a", Point::new(15.0, 16.0), 1.0);
    text(&mut d, "b", Point::new(15.2, 16.0), 1.0);
    let a = analyze(&d);
    let hits = only(&a, "silk_overlap");
    assert_eq!(hits.len(), 1, "{:?}", a.violations);
    assert_eq!(hits[0].severity, Severity::Warning);
}

#[test]
fn silk_texts_apart_are_quiet() {
    let mut d = board();
    text(&mut d, "a", Point::new(8.0, 16.0), 1.0);
    text(&mut d, "b", Point::new(22.0, 16.0), 1.0);
    assert!(only(&analyze(&d), "silk_overlap").is_empty());
}

#[test]
fn silk_near_the_board_edge_warns() {
    let mut d = board();
    text(&mut d, "a", Point::new(15.0, 19.6), 1.0);
    assert_eq!(only(&analyze(&d), "silk_to_edge").len(), 1);
}

#[test]
fn small_silk_text_warns() {
    let mut d = board();
    text(&mut d, "a", Point::new(15.0, 16.0), 0.5);
    assert_eq!(only(&analyze(&d), "silk_text_size").len(), 1);
    d.routing.texts[0].size = 1.0;
    assert!(only(&analyze(&d), "silk_text_size").is_empty());
}

#[test]
fn near_mask_openings_of_two_nets_warn() {
    let mut d = board();
    d.board.rules.min_mask_web = 5.0;
    let a = analyze(&d);
    assert!(!only(&a, "mask_bridge").is_empty(), "{:?}", a.violations);
    d.board.rules.min_mask_web = 0.0;
    assert!(only(&analyze(&d), "mask_bridge").is_empty());
}

#[test]
fn track_with_a_free_end_warns() {
    let mut d = board();
    let (at, _) = r1_pad_2(&d);
    track(&mut d, "A", vec![at, Point::new(at.x, at.y + 4.0)]);
    assert_eq!(only(&analyze(&d), "track_dangling").len(), 1);
}

#[test]
fn track_between_pads_is_not_dangling() {
    let mut d = board();
    let (at, _) = r1_pad_2(&d);
    let other = d.footprints["R_0603"]
        .pad("1")
        .unwrap()
        .at
        .add(Point::new(20.0, 10.0));
    track(&mut d, "A", vec![at, other]);
    assert!(only(&analyze(&d), "track_dangling").is_empty());
}

#[test]
fn via_with_no_connection_warns() {
    let mut d = board();
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "A".into(),
        at: Point::new(15.0, 5.0),
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    assert_eq!(only(&analyze(&d), "via_dangling").len(), 1);
}

#[test]
fn via_connected_to_a_pad_is_not_dangling() {
    let mut d = board();
    let (pad, _) = r1_pad_2(&d);
    let at = Point::new(15.0, 5.0);
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "A".into(),
        at,
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    assert_eq!(only(&analyze(&d), "via_dangling").len(), 1);
    track(&mut d, "A", vec![pad, at]);
    assert!(only(&analyze(&d), "via_dangling").is_empty());
}

#[test]
fn zone_without_a_pad_connection_warns() {
    let mut d = board();
    track(
        &mut d,
        "A",
        vec![Point::new(3.0, 3.0), Point::new(4.0, 4.0)],
    );
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("A".into()),
        layers: vec![Layer::TOP],
        outline: Contour::rect(2.0, 2.0, 4.0, 4.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Solid,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    let a = analyze(&d);
    assert!(!a.fills[0].polygons.is_empty());
    assert_eq!(only(&a, "isolated_copper").len(), 1);
    let (pad, _) = r1_pad_2(&d);
    track(&mut d, "A", vec![pad, Point::new(4.0, 4.0)]);
    assert!(only(&analyze(&d), "isolated_copper").is_empty());
}

#[test]
fn registry_ids_are_unique_and_old_ids_stay() {
    let mut ids: Vec<&str> = lamina_drc::RULE_NAMES.to_vec();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "a rule id is in the registry two times");
    for old in [
        "annular_ring",
        "clearance",
        "copper_outside_board",
        "copper_to_edge",
        "courtyard_outside_board",
        "courtyard_overlap",
        "drill_size",
        "diff_pair_gap",
        "diff_pair_uncoupled",
        "hole_to_hole",
        "isolated_copper",
        "keepout",
        "length_out_of_range",
        "mask_bridge",
        "npth_inside_courtyard",
        "short",
        "short_nets",
        "skew_out_of_range",
        "silk_overlap",
        "silk_text_size",
        "silk_to_edge",
        "silk_to_pad",
        "stale_exclusion",
        "starved_thermal",
        "track_dangling",
        "track_points",
        "track_width",
        "unplaced",
        "unrouted",
        "via_dangling",
        "via_drill",
        "pin_role",
        "unknown_net",
    ] {
        assert!(ids.contains(&old), "{old} is not in the registry");
    }
}

#[test]
fn registry_fields_are_rules_fields() {
    let rules = serde_json::to_value(Rules::default()).unwrap();
    for c in lamina_drc::CHECKS {
        for f in c.fields {
            assert!(
                rules.get(f).is_some(),
                "{}: `{f}` is not a rule field",
                c.id
            );
        }
    }
}

#[test]
fn bad_data_gives_violations_and_does_not_panic() {
    let mut d = board();
    d.board.outline = Some(Contour::rect(0.0, 0.0, f64::NAN, 20.0));
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("A".into()),
        layers: vec![Layer::TOP],
        // A bow tie: the outline crosses itself.
        outline: Contour {
            vertices: vec![
                Vertex::line(1.0, 1.0),
                Vertex::line(9.0, 9.0),
                Vertex::line(9.0, 1.0),
                Vertex::line(1.0, 9.0),
            ],
        },
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Thermal,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    let a = analyze(&d);
    let r = rules(&a);
    assert!(r.contains(&"bad_outline"), "{r:?}");
    assert!(r.contains(&"zone_outline"), "{r:?}");
}

#[test]
fn release_view_makes_open_connections_errors() {
    let d = board();
    let a = analyze(&d);
    let open = |v: &[Violation]| {
        v.iter()
            .find(|v| v.rule == "unrouted")
            .map(|v| v.severity)
            .unwrap()
    };
    assert_eq!(open(&a.violations), Severity::Warning);
    assert_eq!(
        open(&lamina_drc::release_view(&a.violations, &d.board.rules)),
        Severity::Error
    );
    let mut d = d;
    d.board
        .rules
        .severities
        .insert("unrouted".into(), RuleSeverity::Warning);
    let a = analyze(&d);
    assert_eq!(
        open(&lamina_drc::release_view(&a.violations, &d.board.rules)),
        Severity::Warning
    );
}

fn hole(d: &mut ProjectData, at: Point, plated: bool) {
    d.board.holes.push(Hole {
        id: "h1".into(),
        at,
        drill: Drill::Round { diameter: 1.0 },
        plated,
        pad_diameter: plated.then_some(1.6),
        net: plated.then(|| "A".to_string()),
        rotation: 0.0,
    });
}

#[test]
fn track_through_a_non_plated_hole_is_an_error() {
    let mut d = board();
    hole(&mut d, Point::new(15.0, 5.0), false);
    track(
        &mut d,
        "A",
        vec![Point::new(12.0, 5.0), Point::new(18.0, 5.0)],
    );
    let a = analyze(&d);
    let v = only(&a, "hole_to_copper");
    assert_eq!(v.len(), 1, "{:?}", a.violations);
    assert_eq!(v[0].severity, Severity::Error);
    assert_eq!(v[0].required, Some(0.2));
    assert_eq!(v[0].actual, Some(0.0));
}

#[test]
fn track_at_the_hole_limit_is_quiet_and_just_inside_is_not() {
    // Hole radius 0.5, track half width 0.125, limit 0.2: centre distance 0.825.
    let mut d = board();
    hole(&mut d, Point::new(15.0, 5.0), false);
    track(
        &mut d,
        "A",
        vec![Point::new(12.0, 5.825), Point::new(18.0, 5.825)],
    );
    assert!(only(&analyze(&d), "hole_to_copper").is_empty());
    d.routing.tracks[0].points = vec![Point::new(12.0, 5.82), Point::new(18.0, 5.82)];
    assert_eq!(only(&analyze(&d), "hole_to_copper").len(), 1);
}

#[test]
fn copper_of_the_net_of_a_plated_hole_can_touch_it() {
    let mut d = board();
    hole(&mut d, Point::new(15.0, 5.0), true);
    track(
        &mut d,
        "A",
        vec![Point::new(12.0, 5.0), Point::new(15.0, 5.0)],
    );
    assert!(only(&analyze(&d), "hole_to_copper").is_empty());
    // A track of another net at the hole is a violation.
    track(
        &mut d,
        "B",
        vec![Point::new(15.0, 5.6), Point::new(15.0, 8.0)],
    );
    assert_eq!(only(&analyze(&d), "hole_to_copper").len(), 1);
}

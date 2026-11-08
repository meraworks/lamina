use lamina_drc::analyze;
use lamina_model::*;

#[test]
fn thermal_spokes_count_connections_to_the_pour() {
    let mut d = ProjectData::new("thermal");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 20.0, 20.0));
    let mut fp = Footprint::new(
        "pad",
        Provenance::Manual {
            note: "test".into(),
        },
    );
    fp.pads.push(Pad::smd(
        "1",
        Point::new(0.0, 0.0),
        PadShape::Circle { diameter: 1.0 },
    ));
    d.parts.parts.insert("U1".into(), Part::new("pad"));
    d.footprints.insert("pad".into(), fp);
    d.placement
        .placements
        .insert("U1".into(), Placement::at(10.0, 10.0));
    d.nets.nets.insert(
        "GND".into(),
        Net {
            pads: vec!["U1.1".parse().unwrap()],
            class: None,
            ..Default::default()
        },
    );
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("GND".into()),
        layers: vec![Layer::TOP],
        outline: Contour::rect(1.0, 1.0, 18.0, 18.0),
        priority: 0,
        clearance: None,
        min_width: Some(0.1),
        pad_connection: PadConnection::Thermal,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    let a = analyze(&d);
    assert_eq!(a.fills[0].thermal_spokes["U1.1"], 4);
    assert!(!a.violations.iter().any(|v| v.rule == "starved_thermal"));
    d.routing.zones[0].outline = Contour::rect(10.0, 9.8, 5.0, 0.4);
    let a = analyze(&d);
    let hits: Vec<_> = a
        .violations
        .iter()
        .filter(|v| v.rule == "starved_thermal")
        .collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].items, vec!["U1.1", "z1"]);
    assert!(hits[0].message.contains("1 spoke"));
    assert_eq!(a.fills[0].thermal_spokes["U1.1"], 1);
    d.routing.tracks.push(Track {
        id: "manual".into(),
        net: "GND".into(),
        layer: Layer::TOP,
        width: 0.3,
        points: vec![Point::new(10.0, 10.0), Point::new(13.0, 10.0)],
    });
    assert!(
        !analyze(&d)
            .violations
            .iter()
            .any(|v| v.rule == "starved_thermal")
    );
    d.routing.tracks.clear();
    d.routing.zones[0].outline = Contour::rect(9.8, 9.8, 0.4, 0.4);
    let isolated = analyze(&d);
    assert_eq!(isolated.fills[0].thermal_spokes["U1.1"], 0);
    assert!(
        isolated
            .violations
            .iter()
            .any(|v| v.rule == "starved_thermal")
    );
    d.routing.zones[0].outline = Contour::rect(10.0, 9.8, 5.0, 0.4);
    d.board.rules.min_thermal_spokes = 1;
    assert!(
        !analyze(&d)
            .violations
            .iter()
            .any(|v| v.rule == "starved_thermal")
    );
}

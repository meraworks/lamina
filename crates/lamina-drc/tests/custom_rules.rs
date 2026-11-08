use lamina_model::*;

#[test]
fn custom_clearance_width_and_via_limits_use_actual_layers() {
    let mut d = ProjectData::new("custom");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    for (id, net, y) in [("a", "BUS1", 5.0), ("b", "GND", 6.0)] {
        d.routing.tracks.push(Track {
            id: id.into(),
            net: net.into(),
            layer: Layer::TOP,
            width: 0.25,
            points: vec![Point::new(5.0, y), Point::new(15.0, y)],
        });
    }
    d.routing.vias.push(Via {
        id: "v".into(),
        net: "BUS1".into(),
        at: Point::new(20.0, 10.0),
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    let count = |d: &ProjectData, rule: &str| {
        lamina_drc::analyze(d)
            .violations
            .iter()
            .filter(|v| v.rule == rule)
            .count()
    };
    assert_eq!(count(&d, "clearance"), 0);
    d.board.rules.custom = vec![
        CustomRule {
            name: "bus top".into(),
            when: RuleMatch {
                net: Some("BUS*".into()),
                layer: Some(Layer::TOP),
                ..Default::default()
            },
            clearance: Some(1.0),
            min_track_width: Some(0.4),
            ..Default::default()
        },
        CustomRule {
            name: "bus bottom".into(),
            when: RuleMatch {
                net: Some("BUS*".into()),
                layer: Some(Layer::BOTTOM),
                ..Default::default()
            },
            min_via_drill: Some(0.4),
            min_annular_ring: Some(0.2),
            ..Default::default()
        },
    ];
    assert_eq!(count(&d, "clearance"), 1);
    assert_eq!(count(&d, "track_width"), 1);
    assert_eq!(count(&d, "via_drill"), 1);
    assert_eq!(count(&d, "annular_ring"), 1);
    for track in &mut d.routing.tracks {
        track.layer = Layer::BOTTOM;
    }
    assert_eq!(count(&d, "clearance"), 0);
    assert_eq!(count(&d, "track_width"), 0);
}

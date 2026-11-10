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

fn count(d: &ProjectData, rule: &str) -> usize {
    lamina_drc::analyze(d)
        .violations
        .iter()
        .filter(|v| v.rule == rule)
        .count()
}

#[test]
fn a_rule_for_an_item_type_does_not_change_other_types() {
    let mut d = ProjectData::new("kind");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    for (name, pads) in [("A", vec![]), ("B", vec![])] {
        let pads: Vec<PadRef> = pads;
        d.nets.nets.insert(
            name.into(),
            Net {
                pads,
                ..Default::default()
            },
        );
    }
    // A track and a via of net A, each 0.3 mm from a track of net B.
    d.routing.tracks.push(Track {
        id: "ta".into(),
        net: "A".into(),
        layer: Layer::TOP,
        width: 0.2,
        points: vec![Point::new(5.0, 5.0), Point::new(15.0, 5.0)],
    });
    d.routing.tracks.push(Track {
        id: "tb".into(),
        net: "B".into(),
        layer: Layer::TOP,
        width: 0.2,
        points: vec![Point::new(5.0, 5.5), Point::new(25.0, 5.5)],
    });
    d.routing.vias.push(Via {
        id: "va".into(),
        net: "A".into(),
        at: Point::new(22.0, 4.8),
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    assert_eq!(
        count(&d, "clearance"),
        0,
        "0.3 mm is above the limit of 0.2 mm"
    );
    d.board.rules.custom = vec![CustomRule {
        name: "via gap".into(),
        when: RuleMatch {
            kind: Some(RuleKind::Via),
            ..Default::default()
        },
        clearance: Some(0.4),
        ..Default::default()
    }];
    // Only the via is too near now: the top and the bottom layer have copper
    // of the via, and the track of net B is on the top.
    let v: Vec<_> = lamina_drc::analyze(&d)
        .violations
        .into_iter()
        .filter(|v| v.rule == "clearance")
        .collect();
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].items.contains(&"va".to_string()));
}

#[test]
fn a_rule_for_an_area_and_a_maximum_track_width() {
    let mut d = ProjectData::new("area");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    d.nets.nets.insert("A".into(), Net::default());
    for (id, y) in [("t1", 5.0), ("t2", 15.0)] {
        d.routing.tracks.push(Track {
            id: id.into(),
            net: "A".into(),
            layer: Layer::TOP,
            width: 0.5,
            points: vec![Point::new(5.0, y), Point::new(25.0, y)],
        });
    }
    d.routing.areas.push(RuleArea {
        id: "low".into(),
        layers: vec![],
        outline: Contour::rect(0.0, 0.0, 30.0, 10.0),
    });
    assert_eq!(d.routing.areas_at(Point::new(5.0, 5.0), None), vec!["low"]);
    assert!(d.routing.areas_at(Point::new(5.0, 15.0), None).is_empty());
    d.board.rules.custom = vec![CustomRule {
        name: "thin in the area".into(),
        when: RuleMatch {
            area: Some("low".into()),
            ..Default::default()
        },
        max_track_width: Some(0.3),
        ..Default::default()
    }];
    let v: Vec<_> = lamina_drc::analyze(&d)
        .violations
        .into_iter()
        .filter(|v| v.rule == "track_width")
        .collect();
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, vec!["t1"]);
    assert!(v[0].message.contains("maximum width is 0.3 mm"));
    assert_eq!(
        v[0].fix,
        vec![Fix::SetTrackWidth {
            id: "t1".into(),
            width: 0.3
        }]
    );
}

#[test]
fn a_courtyard_clearance_for_one_part() {
    use lamina_footprint::{Options, from_package};
    let mut d = ProjectData::new("cy");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    let fp = from_package("R_0603", &Options::default(), None).unwrap();
    d.footprints.insert(fp.id.clone(), fp);
    for (r, x) in [("R1", 10.0), ("R2", 13.5), ("R3", 17.0)] {
        d.parts.parts.insert(r.into(), Part::new("R_0603"));
        d.placement
            .placements
            .insert(r.into(), Placement::at(x, 10.0));
    }
    assert_eq!(count(&d, "courtyard_overlap"), 0);
    d.board.rules.custom = vec![CustomRule {
        name: "space for R1".into(),
        when: RuleMatch {
            part: Some("R1".into()),
            ..Default::default()
        },
        courtyard_clearance: Some(2.0),
        ..Default::default()
    }];
    let v: Vec<_> = lamina_drc::analyze(&d)
        .violations
        .into_iter()
        .filter(|v| v.rule == "courtyard_overlap")
        .collect();
    assert_eq!(v.len(), 1, "only the pair with R1: {v:?}");
    assert_eq!(v[0].items, vec!["R1", "R2"]);
}

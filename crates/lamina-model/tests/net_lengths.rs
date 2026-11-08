use lamina_model::*;

#[test]
fn net_limits_are_optional_and_round_trip() {
    let old: Net = serde_json::from_str(r#"{"pads":[]}"#).unwrap();
    assert_eq!(old.length_min, None);
    assert_eq!(old.length_max, None);
    assert_eq!(old.skew_group, None);
    assert_eq!(old.max_skew, None);
    let net = Net {
        length_min: Some(10.0),
        length_max: Some(20.0),
        skew_group: Some("bus".into()),
        max_skew: Some(0.5),
        ..old
    };
    assert_eq!(
        serde_json::from_str::<Net>(&serde_json::to_string(&net).unwrap()).unwrap(),
        net
    );
}

#[test]
fn lengths_sum_branches_and_count_vias_without_adding_via_depth() {
    let mut d = ProjectData::new("lengths");
    d.nets.nets.insert("empty".into(), Net::default());
    for (id, end) in [("t1", Point::new(3.0, 4.0)), ("t2", Point::new(0.0, 2.0))] {
        d.routing.tracks.push(Track {
            id: id.into(),
            net: "A".into(),
            layer: Layer::TOP,
            width: 0.2,
            points: vec![Point::new(0.0, 0.0), end],
        });
    }
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "A".into(),
        at: Point::new(0.0, 0.0),
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    let lengths = d.net_lengths();
    assert_eq!(lengths["A"].length, 7.0);
    assert_eq!(lengths["A"].via_count, 1);
    assert_eq!(lengths["empty"].length, 0.0);
}

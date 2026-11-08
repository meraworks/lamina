use lamina_model::*;

#[test]
fn length_and_skew_limits_warn_with_stable_net_items() {
    let mut d = ProjectData::new("lengths");
    for (net, length) in [("A", 10.0), ("B", 12.0)] {
        d.nets.nets.insert(
            net.into(),
            Net {
                length_min: Some(11.0),
                length_max: Some(11.0),
                skew_group: Some("bus".into()),
                max_skew: Some(1.0),
                ..Default::default()
            },
        );
        d.routing.tracks.push(Track {
            id: net.into(),
            net: net.into(),
            layer: Layer::TOP,
            width: 0.25,
            points: vec![Point::new(0.0, 0.0), Point::new(length, 0.0)],
        });
    }
    let a = lamina_drc::analyze(&d);
    let lengths: Vec<_> = a
        .violations
        .iter()
        .filter(|v| v.rule == "length_out_of_range")
        .collect();
    assert_eq!(lengths.len(), 2);
    assert!(lengths.iter().all(|v| v.severity == Severity::Warning));
    let skew: Vec<_> = a
        .violations
        .iter()
        .filter(|v| v.rule == "skew_out_of_range")
        .collect();
    assert_eq!(skew.len(), 1);
    assert_eq!(skew[0].items, vec!["A", "B"]);
    d.nets.nets.get_mut("A").unwrap().length_min = Some(10.0);
    d.nets.nets.get_mut("B").unwrap().length_max = Some(12.0);
    for net in d.nets.nets.values_mut() {
        net.max_skew = Some(2.0);
    }
    assert!(
        lamina_drc::analyze(&d)
            .violations
            .iter()
            .all(|v| !matches!(v.rule.as_str(), "length_out_of_range" | "skew_out_of_range"))
    );
}

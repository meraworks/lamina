use lamina_model::*;

#[test]
fn custom_rules_match_net_class_layer_and_order() {
    let rules = Rules { custom: serde_json::from_value(serde_json::json!([
        {"name":"bus", "when":{"net":"BUS*"}, "clearance":0.4, "min_track_width":0.3},
        {"name":"top power", "when":{"net":"BUS?", "class":"power", "layer":"top"}, "clearance":0.6}
    ]))
    .unwrap(), ..Rules::default() };
    let top = rules.resolve(Some("BUS1"), Some("power"), Some(Layer::TOP));
    assert_eq!(top.clearance, 0.6);
    assert_eq!(top.min_track_width, 0.3);
    assert_eq!(
        rules
            .resolve(Some("BUS1"), Some("power"), Some(Layer::BOTTOM))
            .clearance,
        0.4
    );
    assert_eq!(
        rules
            .resolve(Some("BUS1"), None, Some(Layer::TOP))
            .clearance,
        0.4
    );
    assert_eq!(
        rules
            .resolve(Some("OTHER"), None, Some(Layer::TOP))
            .clearance,
        rules.clearance
    );
    assert_eq!(
        rules.resolve(None, None, None).min_via_drill,
        rules.min_via_drill
    );
    let mut old = serde_json::to_value(&rules).unwrap();
    old.as_object_mut().unwrap().remove("custom");
    assert!(
        serde_json::from_value::<Rules>(old)
            .unwrap()
            .custom
            .is_empty()
    );
}

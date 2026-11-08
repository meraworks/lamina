use lamina_core::*;

#[test]
fn set_custom_rules_validates_and_replaces_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "rules", Ports::default()).unwrap();
    let set = |value| SetRules {
        rules: serde_json::from_value(serde_json::json!({"custom": value})).unwrap(),
        strict: false,
    };
    p.set_rules(set(
        serde_json::json!([{"name":"bus", "when":{"net":"BUS*","layer":"top"}, "clearance":0.4}]),
    ))
    .unwrap();
    let original = p.data.board.rules.custom.clone();
    for rule in [
        serde_json::json!({"name":"bad","clearance":-0.1}),
        serde_json::json!({"name":" ","clearance":0.1}),
        serde_json::json!({"name":"bad","when":{"layer":"top.silk"},"clearance":0.1}),
        serde_json::json!({"name":"bad","when":{"layer":"in1"},"clearance":0.1}),
        serde_json::json!({"name":"bad","min_track_wdith":0.1}),
    ] {
        assert!(p.set_rules(set(serde_json::json!([rule]))).is_err());
        assert_eq!(p.data.board.rules.custom, original);
    }
    p.set_rules(set(serde_json::json!([]))).unwrap();
    assert!(p.data.board.rules.custom.is_empty());
}

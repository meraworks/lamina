use lamina_core::*;

#[test]
fn net_length_limits_reject_malformed_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "lengths", Ports::default()).unwrap();
    for net in [
        serde_json::json!({"pads":[], "length_min":-1.0}),
        serde_json::json!({"pads":[], "length_min":2.0,"length_max":1.0}),
        serde_json::json!({"pads":[], "max_skew":1.0}),
        serde_json::json!({"pads":[], "skew_group":" ","max_skew":1.0}),
        serde_json::json!({"pads":[], "skew_group":"bus","max_skew":-1.0}),
    ] {
        let params = serde_json::from_value(serde_json::json!({"nets":{"A": net}})).unwrap();
        assert!(p.set_nets(params).is_err());
        assert!(p.data.nets.nets.is_empty());
    }
}

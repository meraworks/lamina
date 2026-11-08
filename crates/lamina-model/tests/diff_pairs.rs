use lamina_model::*;

#[test]
fn differential_pair_records_are_optional_and_round_trip() {
    let mut nets: NetsFile = serde_json::from_str(r#"{"schema":1,"nets":{}}"#).unwrap();
    assert!(nets.diff_pairs.is_empty());
    nets.diff_pairs.insert(
        "usb".into(),
        DiffPair {
            positive: "D+".into(),
            negative: "D-".into(),
            gap: 0.2,
            width: 0.25,
        },
    );
    assert_eq!(
        serde_json::from_str::<NetsFile>(&serde_json::to_string(&nets).unwrap()).unwrap(),
        nets
    );
}

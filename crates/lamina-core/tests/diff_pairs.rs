use lamina_core::model::*;
use lamina_core::*;

#[test]
fn named_pairs_validate_nets_dimensions_and_persist() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "pair", Ports::default()).unwrap();
    p.set_nets(
        serde_json::from_value(serde_json::json!({"nets":{"P":{"pads":[]},"N":{"pads":[]}}}))
            .unwrap(),
    )
    .unwrap();
    let pair = DiffPair {
        positive: "P".into(),
        negative: "N".into(),
        gap: 0.2,
        width: 0.25,
    };
    p.set_diff_pairs(SetDiffPairs {
        pairs: [("data".into(), pair.clone())].into(),
    })
    .unwrap();
    for bad in [
        DiffPair {
            gap: -0.2,
            ..pair.clone()
        },
        DiffPair {
            negative: "P".into(),
            ..pair.clone()
        },
        DiffPair {
            positive: "missing".into(),
            ..pair.clone()
        },
    ] {
        assert!(
            p.set_diff_pairs(SetDiffPairs {
                pairs: [("bad".into(), bad)].into()
            })
            .is_err()
        );
        assert_eq!(p.data.nets.diff_pairs["data"], pair);
    }
    let reopened = Project::open_shared_with(dir.path(), Ports::default()).unwrap();
    assert_eq!(reopened.data.nets.diff_pairs["data"], pair);
    assert!(
        p.disconnect(Disconnect {
            net: "P".into(),
            pads: None,
            strict: false
        })
        .is_err()
    );
}

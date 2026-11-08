use lamina_core::model::*;
use lamina_core::*;

#[test]
fn center_path_routes_both_nets_and_refuses_new_drc_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "pair", Ports::default()).unwrap();
    p.set_outline(SetOutline {
        rect: Some(RectSpec {
            x: 0.0,
            y: 0.0,
            width: 30.0,
            height: 20.0,
            corner_radius: 0.0,
        }),
        contour: None,
        cutouts: None,
        strict: false,
    })
    .unwrap();
    p.set_nets(
        serde_json::from_value(serde_json::json!({"nets":{"P":{"pads":[]},"N":{"pads":[]}}}))
            .unwrap(),
    )
    .unwrap();
    p.set_diff_pairs(SetDiffPairs {
        pairs: [(
            "data".into(),
            DiffPair {
                positive: "P".into(),
                negative: "N".into(),
                gap: 0.2,
                width: 0.25,
            },
        )]
        .into(),
    })
    .unwrap();
    p.data.routing.vias.push(Via {
        id: "t1".into(),
        net: "P".into(),
        at: Point::new(25.0, 10.0),
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    let params = RouteDiffPair {
        pair: "data".into(),
        layer: Layer::TOP,
        points: vec![
            Point::new(5.0, 5.0),
            Point::new(15.0, 5.0),
            Point::new(15.0, 15.0),
        ],
    };
    p.route_diff_pair(params).unwrap();
    assert_eq!(p.data.routing.tracks.len(), 2);
    assert!(p.data.routing.tracks.iter().all(|t| t.id != "t1"));
    assert_eq!(p.data.routing.tracks[0].net, "P");
    assert!((p.data.routing.tracks[0].points[0].y - 5.225).abs() < 1e-6);
    assert!((p.data.routing.tracks[1].points[0].y - 4.775).abs() < 1e-6);
    assert!(
        !p.check().iter().any(|v| v.severity == Severity::Error),
        "{:?}",
        p.check()
    );
    let original = p.data.routing.clone();
    assert!(
        p.route_diff_pair(RouteDiffPair {
            pair: "data".into(),
            layer: Layer::TOP,
            points: vec![Point::new(1.0, 0.0), Point::new(20.0, 0.0)]
        })
        .is_err()
    );
    assert_eq!(p.data.routing, original);
    assert!(
        p.route_diff_pair(RouteDiffPair {
            pair: "data".into(),
            layer: Layer::TOP,
            points: vec![
                Point::new(5.0, 5.0),
                Point::new(10.0, 5.0),
                Point::new(5.0, 5.0)
            ]
        })
        .is_err()
    );
    assert_eq!(p.data.routing, original);
}

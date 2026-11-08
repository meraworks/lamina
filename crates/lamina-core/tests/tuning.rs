use lamina_core::model::*;
use lamina_core::*;

fn setup(dir: &std::path::Path) -> Project {
    let mut p = Project::create_with(dir, "tune", Ports::default()).unwrap();
    p.data.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    p.data.nets.nets.insert("A".into(), Net::default());
    for (id, a, b) in [
        ("t1", Point::new(5.0, 5.0), Point::new(15.0, 5.0)),
        ("t2", Point::new(15.0, 5.0), Point::new(20.0, 5.0)),
    ] {
        p.data.routing.tracks.push(Track {
            id: id.into(),
            net: "A".into(),
            layer: Layer::TOP,
            width: 0.25,
            points: vec![a, b],
        });
    }
    p
}

fn params() -> TuneLength {
    TuneLength {
        net: "A".into(),
        track: "t1".into(),
        segment: 0,
        target: 19.0,
        amplitude: 1.0,
        spacing: 1.0,
    }
}

#[test]
fn review_tuning_preserves_interior_track_via_and_pad_contacts() {
    for contact in ["track", "via", "pad"] {
        let dir = tempfile::tempdir().unwrap();
        let mut p = setup(dir.path());
        match contact {
            "track" => p.data.routing.tracks.push(Track {
                id: "branch".into(),
                net: "A".into(),
                layer: Layer::TOP,
                width: 0.25,
                points: vec![Point::new(9.0, 2.0), Point::new(9.0, 5.0)],
            }),
            "via" => p.data.routing.vias.push(Via {
                id: "v".into(),
                net: "A".into(),
                at: Point::new(9.0, 5.0),
                diameter: 0.6,
                drill: 0.3,
                from: None,
                to: None,
            }),
            _ => {
                let mut fp = Footprint::new(
                    "contact",
                    Provenance::Manual {
                        note: "test".into(),
                    },
                );
                fp.pads.push(Pad::smd(
                    "1",
                    Point::new(0.0, 0.0),
                    PadShape::Circle { diameter: 0.4 },
                ));
                p.data.footprints.insert("contact".into(), fp);
                p.data.parts.parts.insert("J1".into(), Part::new("contact"));
                p.data
                    .placement
                    .placements
                    .insert("J1".into(), Placement::at(9.0, 5.0));
                p.data
                    .nets
                    .nets
                    .get_mut("A")
                    .unwrap()
                    .pads
                    .push("J1.1".parse().unwrap());
            }
        }
        let original = p.data.routing.clone();
        let target = p.data.net_lengths()["A"].length + 4.0;
        assert!(
            p.tune_length(TuneLength { target, ..params() }).is_err(),
            "detached {contact}"
        );
        assert_eq!(p.data.routing, original);
    }
}

#[test]
fn tuning_reaches_total_net_target_and_preserves_endpoints() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = setup(dir.path());
    p.tune_length(params()).unwrap();
    assert!((p.data.net_lengths()["A"].length - 19.0).abs() < 1e-6);
    assert_eq!(
        p.data.routing.tracks[0].points.first(),
        Some(&Point::new(5.0, 5.0))
    );
    assert_eq!(
        p.data.routing.tracks[0].points.last(),
        Some(&Point::new(15.0, 5.0))
    );
    assert_eq!(p.data.routing.tracks[1].points.len(), 2);
    assert!(!p.check().iter().any(|v| v.severity == Severity::Error));
}

#[test]
fn tuning_refuses_new_clearance_errors_and_invalid_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = setup(dir.path());
    p.data.nets.nets.insert("B".into(), Net::default());
    p.data.routing.tracks.push(Track {
        id: "block".into(),
        net: "B".into(),
        layer: Layer::TOP,
        width: 0.25,
        points: vec![Point::new(4.0, 6.0), Point::new(16.0, 6.0)],
    });
    let before = p.data.routing.clone();
    assert!(p.tune_length(params()).is_err());
    assert_eq!(p.data.routing, before);
    for bad in [
        TuneLength {
            spacing: 0.1,
            ..params()
        },
        TuneLength {
            amplitude: 0.0,
            ..params()
        },
        TuneLength {
            segment: 99,
            ..params()
        },
        TuneLength {
            net: "B".into(),
            ..params()
        },
    ] {
        assert!(p.tune_length(bad).is_err());
        assert_eq!(p.data.routing, before);
    }
}

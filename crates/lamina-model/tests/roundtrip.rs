use lamina_model::*;
use proptest::prelude::*;

fn nm() -> impl Strategy<Value = f64> {
    (-200_000_000i64..200_000_000).prop_map(|v| v as f64 / 1e6)
}

fn point() -> impl Strategy<Value = Point> {
    (nm(), nm()).prop_map(|(x, y)| Point::new(x, y))
}

fn sample() -> ProjectData {
    let mut d = ProjectData::new("demo");
    d.board.outline = Some(Contour::rounded_rect(0.0, 0.0, 50.0, 30.0, 2.0));
    d.board.holes.push(Hole {
        id: "h1".into(),
        at: Point::new(3.0, 3.0),
        drill: Drill::Round { diameter: 3.2 },
        plated: false,
        pad_diameter: None,
        net: None,
        rotation: 0.0,
    });
    let mut fp = Footprint::new(
        "R_0603",
        Provenance::Manual {
            note: "test".into(),
        },
    );
    fp.pads.push(Pad::smd(
        "1",
        Point::new(-0.8, 0.0),
        PadShape::RoundRect {
            width: 0.9,
            height: 0.95,
            radius: 0.2,
        },
    ));
    fp.pads.push(Pad::through(
        "2",
        Point::new(0.8, 0.0),
        PadShape::Circle { diameter: 1.6 },
        Drill::Slot {
            width: 0.8,
            height: 1.2,
        },
    ));
    d.footprints.insert(fp.id.clone(), fp);
    let mut p = Part::new("R_0603");
    p.value = Some("10k".into());
    p.lcsc = Some("C25804".into());
    d.parts.parts.insert("R1".into(), p);
    d.nets.nets.insert(
        "GND".into(),
        Net {
            pads: vec!["R1.1".parse().unwrap()],
            class: None,
            ..Default::default()
        },
    );
    d.placement
        .placements
        .insert("R1".into(), Placement::at(10.0, 12.5));
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "GND".into(),
        layer: Layer::TOP,
        width: 0.25,
        points: vec![Point::new(9.2, 12.5), Point::new(5.0, 12.5)],
    });
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("GND".into()),
        layers: vec![Layer::BOTTOM],
        outline: Contour::rect(0.0, 0.0, 50.0, 30.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Thermal,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    d
}

#[test]
fn save_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let d = sample();
    d.save_all(dir.path()).unwrap();
    let back = ProjectData::load(dir.path()).unwrap();
    assert_eq!(d, back);
    let nets = std::fs::read_to_string(dir.path().join("nets.json")).unwrap();
    assert!(nets.contains("\"R1.1\""), "{nets}");
}

#[test]
fn unchanged_save_does_not_touch_file() {
    let dir = tempfile::tempdir().unwrap();
    let d = sample();
    d.save_all(dir.path()).unwrap();
    let p = dir.path().join("board.json");
    let t0 = std::fs::metadata(&p).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    d.save(dir.path(), FileKind::Board).unwrap();
    assert_eq!(t0, std::fs::metadata(&p).unwrap().modified().unwrap());
}

#[test]
fn schemas_exist() {
    for k in SCHEMA_KINDS {
        let s = schema_for(k).unwrap();
        assert!(
            s.get("properties").is_some() || s.get("$defs").is_some(),
            "{k}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn tracks_round_trip(pts in prop::collection::vec(point(), 2..40), w in 1u32..2000) {
        let mut d = ProjectData::new("p");
        d.routing.tracks.push(Track {
            id: "t".into(),
            net: "N".into(),
            layer: Layer::BOTTOM,
            width: w as f64 / 1000.0,
            points: pts,
        });
        let s = json::to_string(&d.routing).unwrap();
        let back: RoutingFile = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(back, d.routing);
    }
}

use lamina_core::model::*;
use lamina_core::*;

#[test]
fn fabrication_export_includes_electrical_netlist() {
    let dir = tempfile::tempdir().unwrap();
    let ports = Ports {
        fabricators: vec![std::sync::Arc::new(lamina_export::GerberFabricator)],
        ..Default::default()
    };
    let mut p = Project::create_with(dir.path(), "fab", ports).unwrap();
    p.data.board.outline = Some(Contour::rect(0.0, 0.0, 20.0, 10.0));
    let result = p
        .export(Export {
            dir: None,
            force: false,
            fabricator: None,
        })
        .unwrap();
    let netlist = result
        .files
        .iter()
        .find(|f| f.ends_with(".d356"))
        .expect("electrical netlist in fabrication outputs");
    assert!(
        std::fs::read_to_string(netlist)
            .unwrap()
            .contains("IPC-D-356A")
    );
}

#[test]
fn board_stats_measure_outline_cutouts_tracks_and_drills() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "stats", Ports::default()).unwrap();
    p.data.board.outline = Some(Contour::rect(0.0, 0.0, 20.0, 10.0));
    p.data.board.cutouts.push(Contour::rect(2.0, 2.0, 2.0, 3.0));
    p.data.routing.tracks.push(Track {
        id: "t1".into(),
        net: "A".into(),
        layer: Layer::TOP,
        width: 0.2,
        points: vec![Point::new(0.0, 0.0), Point::new(3.0, 4.0)],
    });
    p.data.routing.vias.push(Via {
        id: "v1".into(),
        net: "A".into(),
        at: Point::new(3.0, 4.0),
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    let s = p.board_stats();
    assert_eq!(s.width, Some(20.0));
    assert_eq!(s.height, Some(10.0));
    assert_eq!(s.area, Some(194.0));
    assert_eq!(s.track_length, 5.0);
    assert_eq!(s.smallest_track, Some(0.2));
    assert_eq!(s.vias, 1);
    assert_eq!(s.drills[0].width, 0.3);
    assert_eq!(s.drills[0].count, 1);
}

#[test]
fn fabrication_export_aliases_long_pad_names() {
    let dir = tempfile::tempdir().unwrap();
    let ports = Ports {
        fabricators: vec![std::sync::Arc::new(lamina_export::GerberFabricator)],
        ..Default::default()
    };
    let mut p = Project::create_with(dir.path(), "usbc", ports).unwrap();
    p.data.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    let mut fp = Footprint::new("USB_C", Provenance::Manual { note: "t".into() });
    let names = ["A1B12", "A4B9", "A5", "B4A9", "B1A12"];
    for (i, name) in names.iter().enumerate() {
        fp.pads.push(Pad::smd(
            *name,
            Point::new(i as f64 * 1.27 - 2.54, 0.0),
            PadShape::Rect {
                width: 0.5,
                height: 1.0,
            },
        ));
    }
    p.data.parts.parts.insert("J1".into(), Part::new(&fp.id));
    p.data
        .placement
        .placements
        .insert("J1".into(), Placement::at(15.24, 10.16));
    p.data.footprints.insert(fp.id.clone(), fp);
    p.data.nets.nets.insert(
        "GND".into(),
        Net {
            pads: vec!["J1.A1B12".parse().unwrap(), "J1.B1A12".parse().unwrap()],
            ..Default::default()
        },
    );
    let result = p
        .export(Export {
            dir: None,
            force: true,
            fabricator: None,
        })
        .unwrap();
    let netlist = result.files.iter().find(|f| f.ends_with(".d356")).unwrap();
    let text = std::fs::read_to_string(netlist).unwrap();
    assert!(text.lines().all(|l| l.len() == 80));
    let records: Vec<&str> = text.lines().filter(|l| l.starts_with("327")).collect();
    assert_eq!(records.len(), 5);
    let pins: Vec<&str> = records.iter().map(|r| r[27..31].trim_end()).collect();
    for pin in ["P001", "P002", "A5", "A4B9", "B4A9"] {
        assert!(pins.contains(&pin), "{pin} in {pins:?}");
    }
    assert!(text.contains("C  PIN   J1-P001 A1B12"));
    assert!(text.contains("C  PIN   J1-P002 B1A12"));
    assert!(text.contains("P  NNAMEN0001 GND"));
    let gnd: Vec<&str> = records
        .iter()
        .filter(|r| r.starts_with("327N0001"))
        .map(|r| &r[27..31])
        .collect();
    assert_eq!(gnd, ["P001", "P002"]);
    // A1B12 is the first pad: 15.24 - 2.54 = 12.7 mm, which is 5000 decimils.
    let a1b12 = records.iter().find(|r| &r[27..31] == "P001").unwrap();
    assert_eq!(&a1b12[41..57], "X+005000Y+004000");
}

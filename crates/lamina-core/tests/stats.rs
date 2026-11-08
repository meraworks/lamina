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

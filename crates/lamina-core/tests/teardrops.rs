use lamina_core::model::*;
use lamina_core::*;

#[test]
fn agent_teardrops_are_same_net_copper_in_fabrication_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "drops", Ports::default()).unwrap();
    p.data.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    p.data.nets.nets.insert("A".into(), Net::default());
    for (id, x) in [("v1", 5.0), ("v2", 25.0)] {
        p.data.routing.vias.push(Via {
            id: id.into(),
            net: "A".into(),
            at: Point::new(x, 10.0),
            diameter: 2.0,
            drill: 1.0,
            from: None,
            to: None,
        });
    }
    p.data.routing.tracks.push(Track {
        id: "t1".into(),
        net: "A".into(),
        layer: Layer::TOP,
        width: 0.25,
        points: vec![Point::new(5.0, 10.0), Point::new(25.0, 10.0)],
    });
    let contains = |d: &ProjectData| {
        let scene = lamina_scene::build_2d(d, &lamina_drc::analyze(d));
        let files = lamina_export::gerbers(&scene, "drops");
        let text = std::str::from_utf8(
            &files
                .iter()
                .find(|f| f.name.ends_with("GTL"))
                .unwrap()
                .contents,
        )
        .unwrap();
        let copper = lamina_export::reader::read_gerber(text).unwrap();
        copper.0.iter().any(|poly| {
            let ring: Vec<Point> = poly
                .exterior()
                .0
                .iter()
                .map(|p| Point::new(p.x, p.y))
                .collect();
            lamina_geom::prim::ring_contains(&ring, Point::new(6.4, 10.385))
        })
    };
    assert!(!contains(&p.data));
    p.add_teardrops().unwrap();
    assert!(p.data.routing.tracks.len() > 1);
    assert!(p.data.routing.tracks.iter().all(|t| t.net == "A"));
    assert!(contains(&p.data));
    p.remove_teardrops().unwrap();
    assert_eq!(p.data.routing.tracks.len(), 1);
    assert!(!contains(&p.data));
}

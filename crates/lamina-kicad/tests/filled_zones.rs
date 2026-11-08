use lamina_model::*;

#[test]
fn export_contains_filled_polygons_without_changing_zone_outlines() {
    let mut d = ProjectData::new("filled");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 20.0, 20.0));
    d.board.holes.push(Hole {
        id: "h1".into(),
        at: Point::new(10.0, 10.0),
        drill: Drill::Round { diameter: 2.0 },
        plated: false,
        pad_diameter: None,
        net: None,
        rotation: 0.0,
    });
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: None,
        layers: vec![Layer::TOP, Layer::BOTTOM],
        outline: Contour::rect(1.0, 1.0, 18.0, 18.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Solid,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    let text = lamina_kicad::export_board(&d);
    assert!(text.contains("(filled_polygon"));
    let tree = sexpr::parse(&text).unwrap();
    let polygons: Vec<_> = tree
        .child("zone")
        .unwrap()
        .children("filled_polygon")
        .collect();
    assert_eq!(polygons.len(), 2);
    let geometry = lamina_geom::build(&d);
    let fills = lamina_geom::fill::fill_zones(&d, &geometry);
    for (node, fill) in polygons.iter().zip(&fills) {
        let points: Vec<_> = node
            .child("pts")
            .unwrap()
            .children("xy")
            .map(|p| Point::new(p.arg_num(0).unwrap(), -p.arg_num(1).unwrap()))
            .collect();
        assert!(!lamina_geom::prim::ring_contains(
            &points,
            Point::new(10.0, 10.0)
        ));
        assert!(lamina_geom::prim::ring_contains(
            &points,
            Point::new(3.0, 3.0)
        ));
        let expected: f64 = fill
            .polygons
            .iter()
            .map(|p| {
                lamina_geom::prim::ring_area(&p.outer).abs()
                    - p.holes
                        .iter()
                        .map(|h| lamina_geom::prim::ring_area(h).abs())
                        .sum::<f64>()
            })
            .sum();
        assert!((lamina_geom::prim::ring_area(&points).abs() - expected).abs() < 1e-5);
    }
    let round = lamina_kicad::import_board(&text, "round").unwrap();
    assert_eq!(round.data.routing.zones.len(), 1);
    assert_eq!(
        round.data.routing.zones[0].outline,
        d.routing.zones[0].outline
    );
}

use lamina_drc::analyze;
use lamina_footprint::{Options, from_package};
use lamina_model::*;

#[test]
fn a_parts_hole_ignores_its_own_courtyard() {
    let mut d = ProjectData::new("holes");
    let mut fp = from_package("R_0603", &Options::default(), None).unwrap();
    fp.pads[0].kind = PadKind::Through {
        drill: Drill::Round { diameter: 0.3 },
        plated: false,
    };
    d.parts.parts.insert("H1".into(), Part::new(&fp.id));
    d.placement
        .placements
        .insert("H1".into(), Placement::at(10.0, 10.0));
    d.footprints.insert(fp.id.clone(), fp);
    assert!(
        !analyze(&d)
            .violations
            .iter()
            .any(|v| v.rule == "npth_inside_courtyard")
    );
    let other = from_package("R_0603", &Options::default(), None).unwrap();
    let mut other = other;
    other.id = "other".into();
    d.parts.parts.insert("R1".into(), Part::new("other"));
    d.placement
        .placements
        .insert("R1".into(), Placement::at(10.0, 10.0));
    d.footprints.insert(other.id.clone(), other);
    assert!(
        analyze(&d)
            .violations
            .iter()
            .any(|v| v.rule == "npth_inside_courtyard" && v.items == ["H1.1", "R1"])
    );
}

#[test]
fn board_hole_inside_a_courtyard_warns_on_either_side() {
    let mut d = ProjectData::new("holes");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 20.0, 20.0));
    let fp = from_package("R_0603", &Options::default(), None).unwrap();
    d.parts.parts.insert("R1".into(), Part::new(&fp.id));
    d.footprints.insert(fp.id.clone(), fp);
    d.placement
        .placements
        .insert("R1".into(), Placement::at(10.0, 10.0));
    d.board.holes.push(Hole {
        id: "H1".into(),
        at: Point::new(10.0, 10.0),
        drill: Drill::Round { diameter: 0.3 },
        plated: false,
        pad_diameter: None,
        net: None,
        rotation: 0.0,
    });
    for side in [Side::Top, Side::Bottom] {
        d.placement.placements.get_mut("R1").unwrap().side = side;
        let a = analyze(&d);
        let hits: Vec<_> = a
            .violations
            .iter()
            .filter(|v| v.rule == "npth_inside_courtyard")
            .collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].items, vec!["H1", "R1"]);
    }
    d.board.holes[0].at = Point::new(3.0, 3.0);
    assert!(
        !analyze(&d)
            .violations
            .iter()
            .any(|v| v.rule == "npth_inside_courtyard")
    );
}

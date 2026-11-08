use lamina_footprint::{Options, from_package};
use lamina_model::*;

#[test]
fn blind_vias_have_layer_span_and_surface_continuation_in_archive() {
    use lamina_ports::fab::Fabricator;
    let mut d = ProjectData::new("blind");
    d.board.stackup.copper_layers = 4;
    d.routing.vias.push(Via {
        id: "v".into(),
        net: "N".into(),
        at: Point::new(5.0, 5.0),
        diameter: 0.6,
        drill: 0.3,
        from: Some(Layer::TOP),
        to: Some(Layer::Copper(Copper::Inner(1))),
    });
    let files = lamina_export::GerberFabricator
        .netlist(&d, "blind")
        .unwrap();
    let text = std::str::from_utf8(&files[0].contents).unwrap();
    let via = text.lines().find(|l| l.starts_with("307")).unwrap();
    assert_eq!(&via[74..80], "L01L02");
    assert!(text.lines().any(|l| l.starts_with("027")));
    let archive = lamina_export::archive(&files, "blind").unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive.contents)).unwrap();
    let entry = zip.by_name("blind.d356").unwrap();
    assert_eq!(entry.size(), files[0].contents.len() as u64);
}

#[test]
fn netlist_has_fixed_columns_units_pad_access_and_vias() {
    let mut d = ProjectData::new("test");
    let mut fp = from_package("R_0603", &Options::default(), None).unwrap();
    fp.pads.truncate(1);
    fp.pads[0].at = Point::new(0.0, 0.0);
    fp.pads[0].rotation = 90.0;
    fp.pads[0].shape = PadShape::Rect {
        width: 1.016,
        height: 2.032,
    };
    d.parts.parts.insert("R1".into(), Part::new(&fp.id));
    d.placement
        .placements
        .insert("R1".into(), Placement::at(2.54, -5.08));
    d.footprints.insert(fp.id.clone(), fp);
    d.nets.nets.insert(
        "Signal".into(),
        Net {
            pads: vec!["R1.1".parse().unwrap()],
            ..Default::default()
        },
    );
    d.nets.nets.insert("signal".into(), Net::default());
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "signal".into(),
        at: Point::new(10.16, 7.62),
        diameter: 1.016,
        drill: 0.508,
        from: None,
        to: None,
    });
    let output = lamina_export::ipc356::write(&d, "test").unwrap();
    assert_eq!(output.name, "test.d356");
    let text = String::from_utf8(output.contents).unwrap();
    assert!(text.lines().all(|line| line.len() == 80));
    assert!(text.contains("P  UNITS CUST 0"));
    let pad = text.lines().find(|l| l.starts_with("327")).unwrap();
    assert_eq!(&pad[20..26], "R1    ");
    assert_eq!(&pad[38..41], "A01");
    assert_eq!(&pad[41..57], "X+001000Y-002000");
    assert_eq!(&pad[57..71], "X0400Y0800R270");
    let via = text.lines().find(|l| l.starts_with("317")).unwrap();
    assert_eq!(&via[31..38], "MD0200P");
    assert_ne!(&pad[3..17], &via[3..17]);
    assert!(text.lines().last().unwrap().starts_with("999"));
    d.routing.vias[0].at.x = 3000.0;
    assert!(lamina_export::ipc356::write(&d, "test").is_err());
}

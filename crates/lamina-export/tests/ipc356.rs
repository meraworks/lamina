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

/// Pads of a USB-C receptacle like LCSC C165948: some names are longer than
/// the 4-column pin field, and the four shield pads share one long name.
const USB_C_SMD: [(&str, f64); 12] = [
    ("A1B12", -3.048),
    ("A4B9", -2.286),
    ("B8", -1.778),
    ("A5", -1.270),
    ("B7", -0.762),
    ("A6", -0.254),
    ("A7", 0.254),
    ("B6", 0.762),
    ("A8", 1.270),
    ("B5", 1.778),
    ("B4A9", 2.286),
    ("B1A12", 3.048),
];
const USB_C_SHIELD: [(f64, f64); 4] = [
    (-4.318, -1.016),
    (4.318, -1.016),
    (-4.318, 3.048),
    (4.318, 3.048),
];

fn usb_c_footprint() -> Footprint {
    let mut fp = from_package("R_0603", &Options::default(), None).unwrap();
    fp.id = "USB_C_TEST".into();
    fp.pads.clear();
    for (name, x) in USB_C_SMD {
        fp.pads.push(Pad::smd(
            name,
            Point::new(x, -1.524),
            PadShape::Rect {
                width: 0.254,
                height: 1.016,
            },
        ));
    }
    for (x, y) in USB_C_SHIELD {
        fp.pads.push(Pad::through(
            "SHIELD",
            Point::new(x, y),
            PadShape::Circle { diameter: 1.016 },
            Drill::Round { diameter: 0.508 },
        ));
    }
    fp
}

fn usb_c_project() -> ProjectData {
    let mut d = ProjectData::new("usbc");
    let fp = usb_c_footprint();
    for (reference, placement) in [
        ("J1", Placement::at(12.7, 7.62)),
        (
            "J2",
            Placement {
                side: Side::Bottom,
                rotation: 90.0,
                ..Placement::at(25.4, 12.7)
            },
        ),
    ] {
        d.parts.parts.insert(reference.into(), Part::new(&fp.id));
        d.placement.placements.insert(reference.into(), placement);
    }
    d.footprints.insert(fp.id.clone(), fp);
    for (net, pads) in [
        (
            "GND",
            &["J1.A1B12", "J1.B1A12", "J1.SHIELD", "J2.A1B12"][..],
        ),
        ("VBUS", &["J1.A4B9", "J1.B4A9", "J2.B4A9"][..]),
        ("CC1", &["J1.A5"][..]),
        ("USB_D+", &["J1.A6", "J1.B6", "J2.A6"][..]),
    ] {
        d.nets.nets.insert(
            net.into(),
            Net {
                pads: pads.iter().map(|p| p.parse().unwrap()).collect(),
                ..Default::default()
            },
        );
    }
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "GND".into(),
        at: Point::new(5.08, 5.08),
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    d
}

fn decimils(mm: f64) -> i64 {
    (mm / 0.00254).round() as i64
}

fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn signed(text: &str) -> bool {
    (text.starts_with('+') || text.starts_with('-')) && digits(&text[1..])
}

/// Checks the fixed columns of one test record. The ranges are 0-based, so
/// `3..17` is columns 4 to 17 of the spec in `ipc356.rs`.
fn check_test_record(r: &str) {
    assert_eq!(r.len(), 80, "{r:?}");
    assert!(digits(&r[0..3]), "code: {r:?}");
    assert!(
        ["317", "327", "367", "307", "027"].contains(&&r[0..3]),
        "{r:?}"
    );
    let net = r[3..17].trim_end();
    assert!(
        net == "N/C" || (net.len() == 5 && net.starts_with('N') && digits(&net[1..])),
        "net: {r:?}"
    );
    assert!(!r[3..17].trim_end().contains(' '), "net: {r:?}");
    assert_eq!(&r[17..20], "   ", "gap: {r:?}");
    assert!(!r[20..26].trim_end().is_empty(), "reference: {r:?}");
    assert!(!r[20..26].trim_end().contains(' '), "reference: {r:?}");
    match &r[26..27] {
        "-" => assert!(!r[27..31].trim_end().is_empty(), "pin: {r:?}"),
        " " => assert_eq!(&r[27..31], "    ", "pin: {r:?}"),
        _ => panic!("pin marker: {r:?}"),
    }
    assert!(!r[27..31].trim_end().contains(' '), "pin: {r:?}");
    assert!(matches!(&r[31..32], " " | "M"), "mid-point: {r:?}");
    let drill = &r[32..38];
    assert!(
        drill == "      "
            || (drill.starts_with('D') && digits(&drill[1..5]) && matches!(&drill[5..], "P" | "U")),
        "drill: {r:?}"
    );
    assert!(r[38..39] == *"A" && digits(&r[39..41]), "access: {r:?}");
    assert!(r[41..42] == *"X" && signed(&r[42..49]), "x: {r:?}");
    assert!(r[49..50] == *"Y" && signed(&r[50..57]), "y: {r:?}");
    let size = &r[57..71];
    assert!(
        size == "              "
            || (size.starts_with('X')
                && digits(&size[1..5])
                && &size[5..6] == "Y"
                && digits(&size[6..10])
                && &size[10..11] == "R"
                && digits(&size[11..14])),
        "size: {r:?}"
    );
    assert_eq!(&r[71..72], " ", "gap: {r:?}");
    assert!(r[72..73] == *"S" && digits(&r[73..74]), "mask: {r:?}");
    let span = &r[74..80];
    assert!(
        span == "      "
            || (&span[0..1] == "L"
                && digits(&span[1..3])
                && &span[3..4] == "L"
                && digits(&span[4..6])),
        "span: {r:?}"
    );
}

/// Checks that every line has 80 columns and a known record type.
fn check_layout(text: &str) {
    assert!(text.ends_with('\n'));
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() > 6);
    for line in &lines {
        assert_eq!(line.len(), 80, "{line:?}");
        assert!(line.is_ascii(), "{line:?}");
    }
    assert!(lines[0].starts_with("P  JOB   "));
    assert_eq!(lines.last().unwrap().trim_end(), "999");
    for line in &lines[..lines.len() - 1] {
        match &line[0..1] {
            "P" | "C" => assert_eq!(&line[1..3], "  ", "{line:?}"),
            _ => check_test_record(line),
        }
    }
}

/// The pin field of each `C  PIN` comment, keyed by reference and alias.
fn pin_comments(text: &str) -> std::collections::BTreeMap<(String, String), String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("C  PIN   "))
        .map(|rest| {
            let mut words = rest.split_whitespace();
            let (reference, alias) = words.next().unwrap().split_once('-').unwrap();
            let name = words.next().unwrap();
            assert!(words.next().is_none());
            ((reference.into(), alias.into()), name.into())
        })
        .collect()
}

fn net_names(text: &str) -> std::collections::BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("P  NNAME"))
        .map(|rest| {
            let (alias, name) = rest.trim_end().split_once(' ').unwrap();
            (alias.into(), name.into())
        })
        .collect()
}

#[test]
fn long_pad_names_export_with_pin_aliases() {
    let d = usb_c_project();
    let output = lamina_export::ipc356::write(&d, "usbc").unwrap();
    let text = String::from_utf8(output.contents).unwrap();
    check_layout(&text);

    // Every pad and the via has one test record.
    let records: Vec<&str> = text
        .lines()
        .filter(|l| l.as_bytes()[0].is_ascii_digit() && !l.starts_with("999"))
        .collect();
    assert_eq!(records.len(), 2 * 16 + 1);
    assert_eq!(records.iter().filter(|r| r.starts_with("327")).count(), 24);
    assert_eq!(records.iter().filter(|r| r.starts_with("317")).count(), 9);

    // Only the connected nets are named, each with its own alias.
    let nets = net_names(&text);
    let expected: Vec<&str> = vec!["CC1", "GND", "USB_D+", "VBUS"];
    assert_eq!(
        nets.values().map(String::as_str).collect::<Vec<_>>(),
        expected
    );

    // The aliases are listed per part, and the two parts reuse them.
    let comments = pin_comments(&text);
    let mut listed: Vec<_> = comments
        .iter()
        .map(|((r, a), n)| format!("{r}-{a} {n}"))
        .collect();
    listed.sort();
    assert_eq!(
        listed,
        [
            "J1-P001 A1B12",
            "J1-P002 B1A12",
            "J1-P003 SHIELD",
            "J2-P001 A1B12",
            "J2-P002 B1A12",
            "J2-P003 SHIELD",
        ]
    );
    let comment_lines: Vec<_> = text.lines().filter(|l| l.starts_with("C  ")).collect();
    assert_eq!(comment_lines.len(), 6);
    let first_record = text.lines().position(|l| l.starts_with("3")).unwrap();
    let last_comment = text
        .lines()
        .collect::<Vec<_>>()
        .iter()
        .rposition(|l| l.starts_with("C  "))
        .unwrap();
    assert!(last_comment < first_record);

    // Each record traces back to its pad, net and position.
    let pad_nets = d.nets.pad_map();
    let fp = &d.footprints["USB_C_TEST"];
    let mut seen = std::collections::BTreeMap::<(String, String), usize>::new();
    for r in records.iter().filter(|r| &r[20..26] != "VIA   ") {
        let reference = r[20..26].trim_end().to_string();
        let pin = r[27..31].trim_end().to_string();
        let name = comments
            .get(&(reference.clone(), pin.clone()))
            .cloned()
            .unwrap_or(pin.clone());
        assert!(name.len() > 4 || !comments.contains_key(&(reference.clone(), pin.clone())));
        *seen.entry((reference.clone(), name.clone())).or_default() += 1;
        let pads: Vec<&Pad> = fp.pads.iter().filter(|p| p.number == name).collect();
        assert!(!pads.is_empty(), "no pad {name} in {r:?}");
        let pl = d.placement.placements[&reference];
        let x: i64 = r[42..49].parse().unwrap();
        let y: i64 = r[50..57].parse().unwrap();
        // Pads that share a name have no order between them, so match any of them.
        let positions: Vec<(i64, i64)> = pads
            .iter()
            .map(|p| {
                let at = pl.transform(p.at);
                (decimils(at.x), decimils(at.y))
            })
            .collect();
        assert!(positions.contains(&(x, y)), "{r:?} not at {positions:?}");
        let net = r[3..17].trim_end();
        let want = pad_nets.get(&PadRef {
            part: reference.clone(),
            pad: name.clone(),
        });
        match want {
            Some(want) => assert_eq!(&nets[net], want, "{r:?}"),
            None => assert_eq!(net, "N/C", "{r:?}"),
        }
    }
    assert_eq!(seen.values().sum::<usize>(), 32);
    assert_eq!(seen[&("J1".into(), "SHIELD".into())], 4);

    // Hand-checked positions on the top side.
    let a1b12 = records
        .iter()
        .find(|r| r.starts_with("327") && &r[20..31] == "J1    -P001")
        .unwrap();
    assert_eq!(
        &a1b12[41..57],
        format!(
            "X{:+07}Y{:+07}",
            decimils(12.7 - 3.048),
            decimils(7.62 - 1.524)
        )
    );
    assert_eq!(&a1b12[41..57], "X+003800Y+002400");
    assert_eq!(&a1b12[38..41], "A01");
    let a5 = records
        .iter()
        .find(|r| &r[20..31] == "J1    -A5  ")
        .unwrap();
    assert_eq!(&a5[41..57], "X+004500Y+002400");
    let bottom = records
        .iter()
        .find(|r| &r[20..31] == "J2    -A5  ")
        .unwrap();
    assert_eq!(&bottom[38..41], "A02");
}

#[test]
fn pin_alias_export_is_deterministic() {
    let d = usb_c_project();
    let first = lamina_export::ipc356::write(&d, "usbc").unwrap().contents;
    for _ in 0..5 {
        assert_eq!(
            lamina_export::ipc356::write(&d, "usbc").unwrap().contents,
            first
        );
    }
}

#[test]
fn every_record_of_mixed_boards_has_the_fixed_columns() {
    let mut d = usb_c_project();
    d.board.stackup.copper_layers = 4;
    d.routing.vias.push(Via {
        id: "blind".into(),
        net: "VBUS".into(),
        at: Point::new(-5.08, 2.54),
        diameter: 0.6,
        drill: 0.3,
        from: Some(Layer::TOP),
        to: Some(Layer::Copper(Copper::Inner(1))),
    });
    d.routing.vias.push(Via {
        id: "buried".into(),
        net: "CC1".into(),
        at: Point::new(-7.62, 2.54),
        diameter: 0.6,
        drill: 0.3,
        from: Some(Layer::Copper(Copper::Inner(1))),
        to: Some(Layer::Copper(Copper::Inner(2))),
    });
    d.board.holes.push(Hole {
        id: "h1".into(),
        at: Point::new(1.27, 1.27),
        drill: Drill::Round { diameter: 3.2 },
        plated: false,
        pad_diameter: None,
        net: None,
        rotation: 0.0,
    });
    let text =
        String::from_utf8(lamina_export::ipc356::write(&d, "mixed").unwrap().contents).unwrap();
    check_layout(&text);
    assert!(
        text.lines()
            .any(|l| l.starts_with("307") && &l[74..80] == "L01L02")
    );
    assert!(
        text.lines()
            .any(|l| l.starts_with("307") && &l[74..80] == "L02L03")
    );
    assert!(text.lines().any(|l| l.starts_with("027")));
    assert!(
        text.lines()
            .any(|l| l.starts_with("367") && &l[20..26] == "HOLE  ")
    );
}

#[test]
fn pin_aliases_are_refused_when_a_part_has_too_many() {
    let mut d = usb_c_project();
    let fp = d.footprints.get_mut("USB_C_TEST").unwrap();
    let template = fp.pads[0].clone();
    for i in 0..1000 {
        let mut pad = template.clone();
        pad.number = format!("LONG{i:04}");
        fp.pads.push(pad);
    }
    let err = lamina_export::ipc356::write(&d, "usbc").unwrap_err();
    assert!(err.starts_with("J1: "), "{err}");
    assert!(err.contains("999"), "{err}");
}

#[test]
fn references_longer_than_their_field_are_still_refused() {
    let mut d = usb_c_project();
    let pl = d.placement.placements.remove("J1").unwrap();
    let part = d.parts.parts.remove("J1").unwrap();
    d.placement.placements.insert("JLONG01".into(), pl);
    d.parts.parts.insert("JLONG01".into(), part);
    let err = lamina_export::ipc356::write(&d, "usbc").unwrap_err();
    assert!(err.contains("exceeds 6 characters"), "{err}");
}

//! The assembly and BOM checks.

use lamina_drc::*;
use lamina_footprint::{Options, from_package};
use lamina_model::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("t");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    let fp = from_package("R_0603", &Options::default(), None).unwrap();
    d.footprints.insert(fp.id.clone(), fp);
    for (r, x) in [("R1", 10.0), ("R2", 20.0)] {
        let mut part = Part::new("R_0603");
        part.value = Some("10k".into());
        d.parts.parts.insert(r.into(), part);
        d.placement
            .placements
            .insert(r.into(), Placement::at(x, 10.0));
    }
    d.nets.nets.insert(
        "A".into(),
        Net {
            pads: vec!["R1.2".parse().unwrap(), "R2.1".parse().unwrap()],
            ..Default::default()
        },
    );
    d
}

fn count(d: &ProjectData, rule: &str) -> usize {
    analyze(d)
        .violations
        .iter()
        .filter(|v| v.rule == rule)
        .count()
}

/// A part with one round SMD pad and no paste.
fn one_pad(d: &mut ProjectData, refdes: &str, at: Point, size: f64) {
    let id = format!("PAD_{refdes}");
    let mut fp = Footprint::new(
        id.clone(),
        Provenance::Manual {
            note: "test".into(),
        },
    );
    let mut pad = Pad::smd(
        "1",
        Point::new(0.0, 0.0),
        PadShape::Circle { diameter: size },
    );
    pad.paste = false;
    fp.pads.push(pad);
    d.footprints.insert(id.clone(), fp);
    d.parts.parts.insert(refdes.into(), Part::new(&id));
    d.placement
        .placements
        .insert(refdes.into(), Placement::at(at.x, at.y));
}

#[test]
fn fiducials_are_counted_for_each_side_with_smd_parts() {
    let mut d = board();
    assert_eq!(
        count(&d, "fiducial_count"),
        0,
        "the check is off by default"
    );
    d.board.rules.min_fiducials = 2;
    assert_eq!(count(&d, "fiducial_count"), 1);
    one_pad(&mut d, "FID1", Point::new(3.0, 3.0), 1.0);
    assert_eq!(count(&d, "fiducial_count"), 1);
    one_pad(&mut d, "FID2", Point::new(27.0, 17.0), 1.0);
    assert_eq!(count(&d, "fiducial_count"), 0);
}

#[test]
fn part_near_the_edge_and_part_above_a_height_limit() {
    let mut d = board();
    d.board.rules.part_to_edge = 3.0;
    assert_eq!(count(&d, "part_to_edge"), 0);
    d.placement.placements.get_mut("R1").unwrap().x = 2.0;
    assert_eq!(count(&d, "part_to_edge"), 1);
    d.board.rules.part_to_edge = 0.0;

    d.routing.keepouts.push(Keepout {
        id: "low".into(),
        layers: vec![Layer::TOP],
        outline: Contour::rect(15.0, 5.0, 10.0, 10.0),
        forbid: Forbid {
            tracks: false,
            vias: false,
            pours: false,
            pads: false,
            parts: false,
        },
    });
    d.footprints.get_mut("R_0603").unwrap().height = Some(0.55);
    d.board.rules.height_limits.insert("low".into(), 0.4);
    assert_eq!(count(&d, "part_height"), 1, "only R2 is in the area");
    d.board.rules.height_limits.insert("low".into(), 0.6);
    assert_eq!(count(&d, "part_height"), 0);
}

#[test]
fn paste_missing_and_paste_oversize() {
    let mut d = board();
    assert_eq!(count(&d, "paste_missing"), 0);
    d.footprints.get_mut("R_0603").unwrap().pads[1].paste = false;
    // R1.2 has a net and no paste. R2.2 has no net.
    assert_eq!(count(&d, "paste_missing"), 1);
    // A paste drawing on the pad gives the paste.
    let at = d.footprints["R_0603"].pads[1].at;
    d.footprints
        .get_mut("R_0603")
        .unwrap()
        .graphics
        .push(FpGraphic {
            layer: FpLayer::Paste,
            shape: Shape::Circle {
                center: at,
                radius: 0.2,
            },
            width: 0.0,
            filled: true,
        });
    assert_eq!(count(&d, "paste_missing"), 0);
    d.footprints.get_mut("R_0603").unwrap().pads[0].paste_margin = Some(0.05);
    assert_eq!(count(&d, "paste_oversize"), 2);
}

#[test]
fn via_in_a_pad_and_a_test_point_that_a_probe_cannot_use() {
    let mut d = board();
    let pad = d.placement.placements["R1"].transform(d.footprints["R_0603"].pads[1].at);
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "A".into(),
        at: pad,
        diameter: 0.5,
        drill: 0.3,
        from: None,
        to: None,
    });
    assert_eq!(count(&d, "via_in_pad"), 1);
    d.routing.vias[0].at = Point::new(15.0, 15.0);
    assert_eq!(count(&d, "via_in_pad"), 0);

    one_pad(&mut d, "TP1", Point::new(15.0, 4.0), 1.5);
    assert_eq!(count(&d, "testpoint_access"), 0);
    one_pad(&mut d, "TP2", Point::new(18.0, 4.0), 0.6);
    assert_eq!(count(&d, "testpoint_access"), 1, "the pad is too small");
    d.placement.placements.get_mut("TP1").unwrap().x = 10.0;
    d.placement.placements.get_mut("TP1").unwrap().y = 10.0;
    assert_eq!(count(&d, "testpoint_access"), 2, "TP1 is under R1");
}

#[test]
fn bom_lines_with_no_number_show_for_release_and_conflicts_always() {
    let mut d = board();
    let a = analyze(&d);
    assert_eq!(count(&d, "bom_part_number"), 0, "off in design");
    let release = release_view(&a, &d.board.rules);
    let hits: Vec<&Violation> = release
        .iter()
        .filter(|v| v.rule == "bom_part_number")
        .collect();
    assert_eq!(hits.len(), 1, "one BOM line for R1 and R2");
    assert_eq!(hits[0].severity, Severity::Warning);
    assert_eq!(hits[0].items, vec!["R1", "R2"]);

    d.parts.parts.get_mut("R1").unwrap().mpn = Some("RC0603-10K".into());
    d.parts.parts.get_mut("R2").unwrap().mpn = Some("RC0603-10K".into());
    assert_eq!(count(&d, "bom_conflict"), 0);
    d.parts.parts.get_mut("R2").unwrap().value = Some("22k".into());
    assert_eq!(count(&d, "bom_conflict"), 1, "one number, two values");
    d.parts.parts.get_mut("R2").unwrap().value = Some("10k".into());
    d.parts.parts.get_mut("R2").unwrap().mpn = Some("OTHER-10K".into());
    assert_eq!(count(&d, "bom_conflict"), 1, "one value, two numbers");
}

#[test]
fn tombstone_is_off_by_default_and_finds_a_solid_pour_on_one_pad() {
    let mut d = board();
    d.nets.nets.insert(
        "B".into(),
        Net {
            pads: vec!["R1.1".parse().unwrap()],
            ..Default::default()
        },
    );
    let p1 = d.placement.placements["R1"].transform(d.footprints["R_0603"].pads[0].at);
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "B".into(),
        layer: Layer::TOP,
        width: 0.15,
        points: vec![p1, Point::new(p1.x - 3.0, p1.y)],
    });
    d.routing.zones.push(Zone {
        id: "z1".into(),
        net: Some("A".into()),
        layers: vec![Layer::TOP],
        outline: Contour::rect(10.2, 5.0, 12.0, 10.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Solid,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    });
    assert_eq!(count(&d, "tombstone"), 0, "off by default");
    d.board
        .rules
        .severities
        .insert("tombstone".into(), RuleSeverity::Warning);
    assert_eq!(
        count(&d, "tombstone"),
        1,
        "R1: a solid pour and a thin track"
    );
    d.routing.zones[0].pad_connection = PadConnection::Thermal;
    assert_eq!(count(&d, "tombstone"), 0, "thermal spokes are correct");
}

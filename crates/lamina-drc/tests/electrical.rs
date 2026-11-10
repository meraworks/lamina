//! The electrical checks.

use lamina_drc::electrical::{Surface, voltage_spacing, width_for_current};
use lamina_drc::*;
use lamina_footprint::{Options, from_package};
use lamina_model::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("t");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 40.0, 30.0));
    let fp = from_package("R_0603", &Options::default(), None).unwrap();
    d.footprints.insert(fp.id.clone(), fp);
    for (r, x) in [("R1", 10.0), ("R2", 20.0)] {
        d.parts.parts.insert(r.into(), Part::new("R_0603"));
        d.placement
            .placements
            .insert(r.into(), Placement::at(x, 10.0));
    }
    net(&mut d, "A", &["R1.2", "R2.1"]);
    d
}

fn net(d: &mut ProjectData, name: &str, pads: &[&str]) {
    d.nets.nets.insert(
        name.into(),
        Net {
            pads: pads.iter().map(|p| p.parse().unwrap()).collect(),
            ..Default::default()
        },
    );
}

fn pad(d: &ProjectData, r: &str, i: usize) -> Point {
    d.placement.placements[r].transform(d.footprints["R_0603"].pads[i].at)
}

fn track(d: &mut ProjectData, id: &str, net: &str, width: f64, points: Vec<Point>) {
    d.routing.tracks.push(Track {
        id: id.into(),
        net: net.into(),
        layer: Layer::TOP,
        width,
        points,
    });
}

fn hits(d: &ProjectData, rule: &str) -> Vec<Violation> {
    analyze(d)
        .violations
        .into_iter()
        .filter(|v| v.rule == rule)
        .collect()
}

#[test]
fn a_net_with_one_pad_and_a_part_with_free_pads() {
    let mut d = board();
    assert!(hits(&d, "net_single_pad").is_empty());
    net(&mut d, "B", &["R1.1"]);
    let v = hits(&d, "net_single_pad");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].items, vec!["B"]);

    assert!(hits(&d, "pad_no_net").is_empty(), "off by default");
    d.board
        .rules
        .severities
        .insert("pad_no_net".into(), RuleSeverity::Warning);
    let v = hits(&d, "pad_no_net");
    assert_eq!(v.len(), 1, "R2.2 has no net");
    assert_eq!(v[0].items, vec!["R2"]);
}

#[test]
fn the_current_formula_gives_the_known_values() {
    // 1 A, 10 °C, 1 oz, outer layer: about 0.30 mm (11.8 mil).
    let w = width_for_current(1.0, 10.0, 1.0, true);
    assert!((w - 0.30).abs() < 0.01, "{w}");
    // An inner layer needs more than two times the width.
    assert!(width_for_current(1.0, 10.0, 1.0, false) > 2.0 * w);
    // More copper needs less width.
    assert!(width_for_current(1.0, 10.0, 2.0, true) < w);
}

#[test]
fn a_track_too_narrow_for_its_current_is_found_with_a_fix() {
    let mut d = board();
    let (a, b) = (pad(&d, "R1", 1), pad(&d, "R2", 0));
    track(&mut d, "t1", "A", 0.2, vec![a, b]);
    assert!(hits(&d, "track_current").is_empty(), "no current is set");
    d.nets.nets.get_mut("A").unwrap().current = Some(2.0);
    let v = hits(&d, "track_current");
    assert_eq!(v.len(), 1);
    let need = v[0].required.unwrap();
    assert!(need > 0.7 && need < 0.9, "{need}");
    let Fix::SetTrackWidth { width, .. } = &v[0].fix[0] else {
        panic!("no fix");
    };
    d.routing.tracks[0].width = *width;
    assert!(hits(&d, "track_current").is_empty());
}

#[test]
fn the_spacing_table_grows_with_the_voltage() {
    assert_eq!(voltage_spacing(12.0, Surface::ExternalBare), 0.1);
    assert_eq!(voltage_spacing(48.0, Surface::ExternalBare), 0.6);
    assert_eq!(voltage_spacing(48.0, Surface::ExternalCoated), 0.13);
    assert_eq!(voltage_spacing(230.0, Surface::ExternalBare), 1.25);
    assert_eq!(voltage_spacing(230.0, Surface::Internal), 0.2);
    assert_eq!(voltage_spacing(500.0, Surface::ExternalBare), 2.5);
    assert!((voltage_spacing(600.0, Surface::ExternalBare) - 3.0).abs() < 1e-9);
    let mut last = 0.0;
    for volts in [
        5.0, 20.0, 40.0, 90.0, 140.0, 160.0, 240.0, 290.0, 450.0, 700.0,
    ] {
        for s in [
            Surface::Internal,
            Surface::ExternalBare,
            Surface::ExternalCoated,
        ] {
            assert!(voltage_spacing(volts, s) > 0.0);
        }
        let now = voltage_spacing(volts, Surface::ExternalBare);
        assert!(now >= last);
        last = now;
    }
}

#[test]
fn copper_too_near_for_its_voltage_is_an_error() {
    let mut d = board();
    net(&mut d, "HV", &["R1.1"]);
    net(&mut d, "GND", &["R2.2"]);
    let a = pad(&d, "R1", 1);
    // Two parallel tracks under mask, 0.3 mm apart (edge to edge).
    track(
        &mut d,
        "t1",
        "HV",
        0.2,
        vec![Point::new(5.0, 20.0), Point::new(15.0, 20.0)],
    );
    track(
        &mut d,
        "t2",
        "GND",
        0.2,
        vec![Point::new(5.0, 20.5), Point::new(15.0, 20.5)],
    );
    let _ = a;
    assert!(
        hits(&d, "voltage_clearance").is_empty(),
        "no voltage is set"
    );
    d.nets.nets.get_mut("HV").unwrap().voltage = Some(48.0);
    assert!(
        hits(&d, "voltage_clearance").is_empty(),
        "48 V below mask needs 0.13 mm"
    );
    d.nets.nets.get_mut("HV").unwrap().voltage = Some(200.0);
    let v = hits(&d, "voltage_clearance");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].severity, Severity::Error);
    assert_eq!(v[0].required, Some(0.4));
    assert_eq!(v[0].items, vec!["t1", "t2"]);
    // A net with no voltage and no ground name is not checked.
    d.nets.nets.remove("GND");
    net(&mut d, "SIG", &["R2.2"]);
    d.routing.tracks[1].net = "SIG".into();
    assert!(hits(&d, "voltage_clearance").is_empty());
}

#[test]
fn a_power_pin_with_no_capacitor_near_it_is_found() {
    let mut d = board();
    // A part with four pads and pin names.
    let mut fp = Footprint::new(
        "IC4".to_string(),
        Provenance::Manual {
            note: "test".into(),
        },
    );
    for (i, (name, x, y)) in [
        ("VDD", -1.0, 1.0),
        ("GND", -1.0, -1.0),
        ("OUT", 1.0, -1.0),
        ("IN", 1.0, 1.0),
    ]
    .iter()
    .enumerate()
    {
        let mut p = Pad::smd(
            (i + 1).to_string(),
            Point::new(*x, *y),
            PadShape::Rect {
                width: 0.6,
                height: 0.6,
            },
        );
        p.name = Some(name.to_string());
        fp.pads.push(p);
    }
    d.footprints.insert("IC4".into(), fp);
    d.parts.parts.insert("U1".into(), Part::new("IC4"));
    d.placement
        .placements
        .insert("U1".into(), Placement::at(30.0, 20.0));
    d.parts.parts.insert("C1".into(), Part::new("R_0603"));
    d.placement
        .placements
        .insert("C1".into(), Placement::at(10.0, 25.0));
    net(&mut d, "3V3", &["U1.1", "C1.1"]);
    net(&mut d, "GND", &["U1.2", "C1.2"]);
    let v = hits(&d, "decoupling_far");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, vec!["U1", "3V3"]);
    // The capacitor next to the pin.
    *d.placement.placements.get_mut("C1").unwrap() = Placement::at(27.5, 21.0);
    assert!(hits(&d, "decoupling_far").is_empty());
    // A part with no pin names gets no check.
    *d.placement.placements.get_mut("C1").unwrap() = Placement::at(10.0, 25.0);
    for p in &mut d.footprints.get_mut("IC4").unwrap().pads {
        p.name = None;
    }
    assert!(hits(&d, "decoupling_far").is_empty());
}

#[test]
fn a_fast_track_over_a_gap_in_the_plane_is_found() {
    let mut d = board();
    net(&mut d, "GND", &["R1.1", "R2.2"]);
    let (a, b) = (pad(&d, "R1", 1), pad(&d, "R2", 0));
    track(&mut d, "t1", "A", 0.2, vec![a, b]);
    let zone = |id: &str, x: f64, w: f64| Zone {
        id: id.into(),
        net: Some("GND".into()),
        layers: vec![Layer::BOTTOM],
        outline: Contour::rect(x, 2.0, w, 26.0),
        priority: 0,
        clearance: None,
        min_width: None,
        pad_connection: PadConnection::Solid,
        thermal_gap: 0.3,
        thermal_spoke: 0.3,
    };
    // Two ground pours on the bottom with a gap of 2 mm below the track.
    // A via in each pour connects it, so that the fill keeps it.
    d.routing.zones.push(zone("z1", 2.0, 12.0));
    d.routing.zones.push(zone("z2", 16.0, 22.0));
    for (id, x) in [("v1", 5.0), ("v2", 30.0)] {
        d.routing.vias.push(Via {
            id: id.into(),
            net: "GND".into(),
            at: Point::new(x, 20.0),
            diameter: 0.6,
            drill: 0.3,
            from: None,
            to: None,
        });
    }
    assert!(
        hits(&d, "return_path").is_empty(),
        "the net is not a fast net"
    );
    d.board.rules.net_classes.insert(
        "fast".into(),
        NetClass {
            reference_plane: true,
            ..Default::default()
        },
    );
    d.nets.nets.get_mut("A").unwrap().class = Some("fast".into());
    let v = hits(&d, "return_path");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, vec!["t1"]);
    let at = v[0].at.unwrap();
    assert!(at.x > 13.5 && at.x < 16.5, "{at:?}");
    // One pour below all of the track.
    d.routing.zones.clear();
    d.routing.zones.push(zone("z1", 2.0, 36.0));
    assert!(hits(&d, "return_path").is_empty());
}

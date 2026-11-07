use lamina_footprint::{Options, from_package};
use lamina_model::*;
use lamina_route::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("t");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    for pkg in ["R_0603", "SOIC-8"] {
        let fp = from_package(pkg, &Options::default(), None).unwrap();
        d.footprints.insert(fp.id.clone(), fp);
    }
    for (r, f, x, y) in [
        ("R1", "R_0603", 5.0, 10.0),
        ("R2", "R_0603", 25.0, 10.0),
        ("U1", "SOIC-8", 15.0, 10.0),
    ] {
        d.parts.parts.insert(r.into(), Part::new(f));
        d.placement.placements.insert(r.into(), Placement::at(x, y));
    }
    // A goes under or around U1. B and C are short.
    let net = |pads: &[&str]| Net {
        pads: pads.iter().map(|p| p.parse().unwrap()).collect(),
        class: None,
    };
    d.nets.nets.insert("A".into(), net(&["R1.2", "R2.1"]));
    d.nets.nets.insert("B".into(), net(&["U1.1", "R1.1"]));
    d.nets.nets.insert("C".into(), net(&["U1.5", "R2.2"]));
    d
}

#[test]
fn routes_without_errors() {
    let mut d = board();
    let out = autoroute(&d, &RouteOptions::default()).unwrap();
    assert!(out.failed.is_empty(), "{:?}", out.failed);
    assert_eq!(out.routed.len(), 3);
    d.routing.tracks.extend(out.tracks);
    d.routing.vias.extend(out.vias);
    let a = lamina_drc::analyze(&d);
    assert!(a.ratsnest.is_empty(), "{:?}", a.ratsnest);
    let errors: Vec<_> = a
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    // Tracks are octilinear.
    for t in &d.routing.tracks {
        for w in t
            .points
            .windows(2)
            .skip(1)
            .take(t.points.len().saturating_sub(3))
        {
            let dx = (w[1].x - w[0].x).abs();
            let dy = (w[1].y - w[0].y).abs();
            assert!(
                dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6,
                "{} is not octilinear: {:?}",
                t.id,
                w
            );
        }
    }
}

#[test]
fn single_layer_forces_detour_or_failure_report() {
    let d = board();
    let opt = RouteOptions {
        layers: Some(vec![Layer::TOP]),
        ..Default::default()
    };
    let out = autoroute(&d, &opt).unwrap();
    assert!(out.vias.is_empty());
    assert_eq!(out.routed.len() + out.failed.len(), 3);
}

#[test]
fn skip_nets() {
    let d = board();
    let opt = RouteOptions {
        skip: vec!["A".into()],
        ..Default::default()
    };
    let out = autoroute(&d, &opt).unwrap();
    assert!(out.tracks.iter().all(|t| t.net != "A"));
}

#[test]
fn crossing_bus_is_negotiated_clean() {
    // Two SOIC-8s side by side. Pin k of U1 connects to pin k of U2. The nets
    // from the far row of U1 must cross the other nets. Thus, the router needs
    // vias and negotiation to finish without shorts.
    let mut d = ProjectData::new("x");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    let fp = from_package("SOIC-8", &Options::default(), None).unwrap();
    d.footprints.insert(fp.id.clone(), fp);
    for (r, x) in [("U1", 8.0), ("U2", 22.0)] {
        d.parts.parts.insert(r.into(), Part::new("SOIC-8"));
        d.placement
            .placements
            .insert(r.into(), Placement::at(x, 10.0));
    }
    for k in 1..=8 {
        d.nets.nets.insert(
            format!("N{k}"),
            Net {
                pads: vec![
                    format!("U1.{k}").parse().unwrap(),
                    format!("U2.{k}").parse().unwrap(),
                ],
                class: None,
            },
        );
    }
    let out = autoroute(&d, &RouteOptions::default()).unwrap();
    assert!(out.failed.is_empty(), "{:?}", out.failed);
    assert!(!out.vias.is_empty());
    d.routing.tracks.extend(out.tracks);
    d.routing.vias.extend(out.vias);
    let a = lamina_drc::analyze(&d);
    assert!(a.ratsnest.is_empty(), "{:?}", a.ratsnest);
    let errors: Vec<_> = a
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn vias_respect_copper_on_layers_the_router_may_not_use() {
    // 4 layers. The router uses only top and bottom. Another net covers In1,
    // so a through via will cause a short there. The net must fail, not short.
    let mut d = ProjectData::new("x");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    d.board.stackup.copper_layers = 4;
    let fp = from_package("R_0603", &Options::default(), None).unwrap();
    d.footprints.insert(fp.id.clone(), fp);
    d.parts.parts.insert("R1".into(), Part::new("R_0603"));
    d.parts.parts.insert("R2".into(), Part::new("R_0603"));
    d.placement
        .placements
        .insert("R1".into(), Placement::at(5.0, 10.0));
    let mut back = Placement::at(25.0, 10.0);
    back.side = Side::Bottom;
    d.placement.placements.insert("R2".into(), back);
    let net = |pads: &[&str]| Net {
        pads: pads.iter().map(|p| p.parse().unwrap()).collect(),
        class: None,
    };
    d.nets.nets.insert("A".into(), net(&["R1.1", "R2.1"]));
    d.nets.nets.insert("B".into(), net(&[]));
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "B".into(),
        layer: Layer::Copper(Copper::from_index(1, 4)),
        width: 19.0,
        points: vec![Point::new(0.5, 10.0), Point::new(29.5, 10.0)],
    });
    let opt = RouteOptions {
        layers: Some(vec![Layer::TOP, Layer::BOTTOM]),
        nets: Some(vec!["A".into()]),
        time_limit: Some(10.0),
        ..Default::default()
    };
    let out = autoroute(&d, &opt).unwrap();
    assert!(out.vias.is_empty(), "{:?}", out.vias);
    assert_eq!(out.failed.len(), 1);
}

#[test]
fn walkaround_goes_around_copper_of_other_nets() {
    let mut d = board();
    // U1 is between R1.2 and R2.1. A straight track would cross its pads.
    let pad = |l: &str| {
        let pr: PadRef = l.parse().unwrap();
        let pl = d.placement.placements[&pr.part];
        let fp = &d.footprints[&d.parts.parts[&pr.part].footprint];
        pl.transform(fp.pad(&pr.pad).unwrap().at)
    };
    let (from, to) = (pad("R1.2"), pad("R2.1"));
    let w = Walk {
        net: "A".into(),
        layer: Layer::Copper(Copper::Top),
        width: d.board.rules.track_width,
        from,
        to,
        grid: 0.1,
    };
    let pts = walkaround(&d, &w).expect("a path");
    assert_eq!(pts.first(), Some(&from));
    assert_eq!(pts.last(), Some(&to));
    assert!(pts.len() > 2, "the path must turn around U1: {pts:?}");
    // All segments are at 0, 45 or 90 degrees, also the end segments.
    for w in pts.windows(2) {
        let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
        assert!(
            dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6,
            "{w:?} is not octilinear"
        );
    }
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "A".into(),
        layer: Layer::Copper(Copper::Top),
        width: w.width,
        points: pts,
    });
    let a = lamina_drc::analyze(&d);
    let errors: Vec<_> = a
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn walkaround_to_the_same_point_does_not_fail() {
    let d = board();
    let p = Point::new(10.0, 3.0);
    let w = Walk {
        net: "A".into(),
        layer: Layer::Copper(Copper::Top),
        width: 0.25,
        from: p,
        to: Point::new(10.01, 3.0),
        grid: 0.1,
    };
    let pts = walkaround(&d, &w).expect("a path");
    assert_eq!(pts.first(), Some(&w.from));
    assert_eq!(pts.last(), Some(&w.to));
}

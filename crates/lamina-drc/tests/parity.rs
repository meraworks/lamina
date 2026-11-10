//! The checks of `parity.rs`.

use lamina_drc::*;
use lamina_footprint::{Options, from_package};
use lamina_model::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("t");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    let fp = from_package("R_0603", &Options::default(), None).unwrap();
    d.footprints.insert(fp.id.clone(), fp);
    for (r, x) in [("R1", 10.0), ("R2", 20.0)] {
        d.parts.parts.insert(r.into(), Part::new("R_0603"));
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

fn pad(d: &ProjectData, part: &str, pad: &str) -> Point {
    d.placement.placements[part].transform(d.footprints["R_0603"].pad(pad).unwrap().at)
}

fn track(d: &mut ProjectData, points: Vec<Point>) {
    let id = format!("t{}", d.routing.tracks.len());
    d.routing.tracks.push(Track {
        id,
        net: "A".into(),
        layer: Layer::TOP,
        width: 0.25,
        points,
    });
}

#[test]
fn board_holes_have_drill_and_ring_limits() {
    let mut d = board();
    d.board.holes.push(Hole {
        id: "h1".into(),
        at: Point::new(5.0, 5.0),
        drill: Drill::Round { diameter: 0.2 },
        plated: true,
        pad_diameter: Some(0.3),
        net: None,
        rotation: 0.0,
    });
    assert_eq!(count(&d, "drill_size"), 1);
    assert_eq!(count(&d, "annular_ring"), 1);
    d.board.holes[0].drill = Drill::Round { diameter: 7.0 };
    d.board.holes[0].pad_diameter = Some(9.0);
    assert_eq!(count(&d, "drill_size"), 1, "above the largest drill");
    d.board.holes[0].drill = Drill::Slot {
        width: 0.4,
        height: 2.0,
    };
    d.board.holes[0].pad_diameter = Some(4.0);
    assert_eq!(count(&d, "drill_size"), 1, "a slot that is too narrow");
    d.board.holes[0].drill = Drill::Slot {
        width: 0.5,
        height: 2.0,
    };
    assert_eq!(count(&d, "drill_size"), 0, "exactly at the limit");
}

#[test]
fn via_count_uses_the_limit_of_the_net() {
    let mut d = board();
    for (i, x) in [12.0, 14.0].iter().enumerate() {
        d.routing.vias.push(Via {
            id: format!("v{i}"),
            net: "A".into(),
            at: Point::new(*x, 15.0),
            diameter: 0.6,
            drill: 0.3,
            from: None,
            to: None,
        });
    }
    assert_eq!(count(&d, "via_count"), 0);
    d.nets.nets.get_mut("A").unwrap().max_vias = Some(1);
    assert_eq!(count(&d, "via_count"), 1);
    d.nets.nets.get_mut("A").unwrap().max_vias = Some(2);
    assert_eq!(count(&d, "via_count"), 0);
}

#[test]
fn courtyard_missing_open_and_clearance() {
    let mut d = board();
    assert_eq!(count(&d, "courtyard_missing"), 0);
    assert_eq!(count(&d, "courtyard_open"), 0);
    // 0603 courtyards at x 10 and 20 are far apart.
    d.board.rules.courtyard_clearance = 1.0;
    assert_eq!(count(&d, "courtyard_overlap"), 0);
    d.board.rules.courtyard_clearance = 50.0;
    assert_eq!(count(&d, "courtyard_overlap"), 1);
    d.board.rules.courtyard_clearance = 0.0;
    let fp = d.footprints.get_mut("R_0603").unwrap();
    fp.graphics.retain(|g| g.layer != FpLayer::Courtyard);
    assert_eq!(count(&d, "courtyard_missing"), 2);
    // One loose line is not a loop.
    let fp = d.footprints.get_mut("R_0603").unwrap();
    fp.graphics.push(FpGraphic {
        layer: FpLayer::Courtyard,
        shape: Shape::Line {
            a: Point::new(-1.0, -1.0),
            b: Point::new(1.0, -1.0),
        },
        width: 0.05,
        filled: false,
    });
    assert_eq!(count(&d, "courtyard_missing"), 0);
    assert_eq!(count(&d, "courtyard_open"), 2);
}

#[test]
fn tracks_that_only_touch_are_a_weak_connection() {
    let mut d = board();
    let (a, b) = (pad(&d, "R1", "2"), pad(&d, "R2", "1"));
    track(&mut d, vec![a, b]);
    assert_eq!(count(&d, "weak_connection"), 0);
    assert_eq!(count(&d, "unrouted"), 0);
    // Two tracks that meet end to end with almost no overlap.
    let m = Point::new((a.x + b.x) / 2.0, a.y);
    d.routing.tracks.clear();
    track(&mut d, vec![a, Point::new(m.x - 0.125, m.y)]);
    track(&mut d, vec![Point::new(m.x + 0.1249, m.y), b]);
    assert_eq!(count(&d, "unrouted"), 0, "they touch, so the net is routed");
    assert_eq!(count(&d, "weak_connection"), 1);
    // With a real overlap, the join is good.
    d.routing.tracks[1].points[0] = Point::new(m.x, m.y);
    assert_eq!(count(&d, "weak_connection"), 0);
}

#[test]
fn zero_length_segment_and_sharp_corner() {
    let mut d = board();
    let (a, b) = (pad(&d, "R1", "2"), pad(&d, "R2", "1"));
    track(&mut d, vec![a, Point::new(a.x + 0.005, a.y), b]);
    assert_eq!(count(&d, "track_zero_length"), 1);
    d.routing.tracks[0].points = vec![a, Point::new(15.0, 12.0), Point::new(14.0, 10.0), b];
    assert_eq!(count(&d, "track_zero_length"), 0);
    // The angle check is off by default.
    assert_eq!(count(&d, "track_angle"), 0);
    d.board
        .rules
        .severities
        .insert("track_angle".into(), RuleSeverity::Warning);
    assert_eq!(count(&d, "track_angle"), 1);
}

#[test]
fn zones_with_the_same_priority_must_not_overlap() {
    let mut d = board();
    d.nets.nets.insert("B".into(), Net::default());
    for (id, net, x) in [("z1", "A", 1.0), ("z2", "B", 5.0)] {
        d.routing.zones.push(Zone {
            id: id.into(),
            net: Some(net.into()),
            layers: vec![Layer::TOP],
            outline: Contour::rect(x, 1.0, 6.0, 6.0),
            priority: 0,
            clearance: None,
            min_width: None,
            pad_connection: PadConnection::Thermal,
            thermal_gap: 0.3,
            thermal_spoke: 0.3,
        });
    }
    assert_eq!(count(&d, "zone_overlap"), 1);
    d.routing.zones[1].priority = 1;
    assert_eq!(count(&d, "zone_overlap"), 0);
}

fn text(d: &mut ProjectData, at: Point, s: &str) {
    let id = format!("x{}", d.routing.texts.len());
    d.routing.texts.push(BoardText {
        id,
        layer: Layer::Silk(Side::Top),
        text: s.into(),
        at,
        size: 1.0,
        rotation: 0.0,
    });
}

#[test]
fn silk_outside_on_a_hole_thin_and_with_a_variable() {
    let mut d = board();
    text(&mut d, Point::new(15.0, 16.0), "OK");
    assert_eq!(count(&d, "silk_outside_board"), 0);
    text(&mut d, Point::new(40.0, 16.0), "OUT");
    assert_eq!(count(&d, "silk_outside_board"), 1);
    d.routing.texts.pop();
    d.board.holes.push(Hole {
        id: "h1".into(),
        at: Point::new(15.0, 16.0),
        drill: Drill::Round { diameter: 1.0 },
        plated: false,
        pad_diameter: None,
        net: None,
        rotation: 0.0,
    });
    assert_eq!(count(&d, "silk_on_hole"), 1);
    d.board.holes.clear();
    text(&mut d, Point::new(15.0, 4.0), "${rev}");
    assert_eq!(count(&d, "text_variable"), 1);
    d.routing.graphics.push(BoardGraphic {
        id: "g1".into(),
        layer: Layer::Silk(Side::Top),
        shape: Shape::Line {
            a: Point::new(2.0, 2.0),
            b: Point::new(6.0, 2.0),
        },
        width: 0.05,
        filled: false,
    });
    assert_eq!(count(&d, "silk_line_width"), 1);
}

#[test]
fn each_reported_rule_is_in_the_registry() {
    let d = board();
    let a = analyze(&d);
    for v in a.violations.iter().chain(&a.release_only) {
        assert!(
            registry::get(&v.rule).is_some(),
            "{} is not registered",
            v.rule
        );
    }
}

#[test]
fn an_open_import_note_is_a_loss_until_it_is_resolved() {
    let mut d = board();
    d.notes.notes.push(Note {
        id: "n1".into(),
        text: "skipped 2 footprint zones".into(),
        at: None,
        refdes: None,
        author: "import".into(),
        created: 0,
        resolved: false,
    });
    let a = analyze(&d);
    assert_eq!(count(&d, "import_loss"), 1);
    let release = release_view(&a, &d.board.rules);
    assert!(
        release
            .iter()
            .any(|v| v.rule == "import_loss" && v.severity == Severity::Error)
    );
    d.notes.notes[0].resolved = true;
    assert_eq!(count(&d, "import_loss"), 0);
}

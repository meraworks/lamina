//! The release view, the export gate, presets and strict mode.

use lamina_core::model::*;
use lamina_core::*;
use lamina_footprint::{Options, from_package};

/// Two resistors. Net A joins R1.2 and R2.1 and is not routed.
fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "test", Ports::default()).unwrap();
    let d = &mut p.data;
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
    (dir, p)
}

fn export(p: &Project, force: bool) -> String {
    p.export(Export {
        dir: None,
        force,
        fabricator: None,
        paste_for_dnp: false,
    })
    .unwrap_err()
    .to_string()
}

#[test]
fn export_refuses_open_connections_unless_forced() {
    let (_t, p) = project();
    assert_eq!(p.status().errors, 0);
    assert_eq!(p.status().release_errors, 1);
    let e = export(&p, false);
    assert!(e.contains("unrouted connection"), "{e}");
    // With force, the gate passes. The test project has no fabricator.
    let e = export(&p, true);
    assert!(e.contains("fabricator"), "{e}");
}

#[test]
fn use_fab_preset_keeps_custom_rules_exclusions_and_severities() {
    let (_t, mut p) = project();
    let update = serde_json::from_value(serde_json::json!({"rules": {
        "severities": {"silk_overlap": "error"},
        "custom": [{"name": "wide", "when": {"net": "A"}, "clearance": 0.4}]
    }}))
    .unwrap();
    p.set_rules(update).unwrap();
    p.exclude_violation(ExcludeViolation {
        rule: "via_dangling".into(),
        items: vec!["v1".into()],
        reason: "Test point".into(),
    })
    .unwrap();
    p.use_fab_preset(UseFabPreset {
        name: "relaxed".into(),
        strict: false,
    })
    .unwrap();
    let r = &p.data.board.rules;
    assert_eq!(r.clearance, 0.2);
    assert_eq!(r.custom.len(), 1);
    assert_eq!(r.exclusions.len(), 1);
    assert_eq!(r.severities.len(), 1);
}

#[test]
fn second_free_end_of_one_track_is_a_new_violation() {
    let (_t, mut p) = project();
    let free = |d: &ProjectData| {
        lamina_drc::analyze(d)
            .violations
            .iter()
            .filter(|v| v.rule == "track_dangling")
            .count()
    };
    let r1 = p.data.placement.placements["R1"];
    let pad = p.data.footprints["R_0603"].pad("2").unwrap().at;
    let start = r1.transform(pad);
    // One end on the pad, one end free.
    let mut d = p.data.clone();
    d.routing.tracks.push(Track {
        id: "t1".into(),
        net: "A".into(),
        layer: Layer::TOP,
        width: 0.25,
        points: vec![start, Point::new(15.0, 14.0)],
    });
    assert_eq!(free(&d), 1);
    p.commit(d, Touched::file(FileKind::Routing), false, "one end")
        .unwrap();
    // Now the two ends are free. The key is the same, the count is not.
    let mut d = p.data.clone();
    d.routing.tracks[0].points[0] = Point::new(12.0, 14.0);
    assert_eq!(free(&d), 2);
    let r = p
        .commit(d, Touched::file(FileKind::Routing), false, "two ends")
        .unwrap();
    assert_eq!(
        r.new_violations
            .iter()
            .filter(|v| v.rule == "track_dangling")
            .count(),
        1
    );
}

#[test]
fn model_rules_accept_a_severity_but_data_errors_do_not() {
    let (_t, mut p) = project();
    let ok =
        serde_json::from_value(serde_json::json!({"rules": {"severities": {"pin_role": "error"}}}))
            .unwrap();
    p.set_rules(ok).unwrap();
    let bad = serde_json::from_value(
        serde_json::json!({"rules": {"severities": {"unknown_net": "ignore"}}}),
    )
    .unwrap();
    assert!(p.set_rules(bad).is_err());
    assert!(p.checks().iter().any(|c| c.id == "hole_to_copper"));
}

#[test]
fn apply_fixes_corrects_a_small_text_and_refuses_nothing_on_a_dry_run() {
    use lamina_core::ops::ApplyFixes;
    let dir = tempfile::tempdir().unwrap();
    let src =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/yoink-main-board");
    for f in [
        "board.json",
        "parts.json",
        "nets.json",
        "placement.json",
        "routing.json",
    ] {
        std::fs::copy(src.join(f), dir.path().join(f)).unwrap();
    }
    let fp = dir.path().join("cache/footprints");
    std::fs::create_dir_all(&fp).unwrap();
    for e in std::fs::read_dir(src.join("cache/footprints")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), fp.join(e.file_name())).unwrap();
    }
    let mut p = lamina_core::Project::open_with(dir.path(), Ports::default()).unwrap();
    let mut d = p.data.clone();
    let min = d.board.rules.min_silk_text_height;
    d.routing.texts[0].size = min / 2.0;
    let id = d.routing.texts[0].id.clone();
    p.apply(d, "small text").unwrap();

    let dry = p
        .apply_fixes(ApplyFixes {
            dry_run: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(dry.applied.len(), 1, "{:?}", dry.refused);
    assert!(dry.result.is_none());
    assert!(
        p.data.routing.texts[0].size < min,
        "a dry run changes nothing"
    );

    let done = p.apply_fixes(ApplyFixes::default()).unwrap();
    assert_eq!(done.applied.len(), 1);
    assert_eq!(done.applied[0].items, vec![id]);
    assert!((p.data.routing.texts[0].size - min).abs() < 1e-9);
    assert!(
        p.apply_fixes(ApplyFixes::default())
            .unwrap()
            .applied
            .is_empty()
    );
}

#[test]
fn review_gives_not_ready_for_open_connections_and_writes_the_file() {
    let (dir, p) = project();
    let r = p.review(Review::default()).unwrap();
    assert_eq!(r.verdict, Verdict::NotReady);
    assert!(r.errors > 0);
    assert!(r.first_errors.iter().any(|e| e.starts_with("unrouted")));
    assert_eq!(r.fabs.len(), 2, "JLCPCB and PCBWay");
    let text = std::fs::read_to_string(dir.path().join("output/review.md")).unwrap();
    assert!(text.contains("**Verdict: NOT READY**"), "{text}");
    assert!(text.contains("## Footprint evidence"));

    let quiet = p.review(Review { no_file: true }).unwrap();
    assert!(quiet.file.is_none());
}

#[test]
fn diff_reports_moved_parts_and_new_violations() {
    use lamina_core::ops::review::{adds_error, diff};
    let (_dir, p) = project();
    let a = p.data.clone();
    assert!(diff(&a, &a).is_empty());
    let mut b = a.clone();
    // Put R2 on R1: the courtyards overlap.
    let r1 = b.placement.placements["R1"];
    *b.placement.placements.get_mut("R2").unwrap() = r1;
    b.nets.nets.insert("B".into(), Net::default());
    let d = diff(&a, &b);
    assert_eq!(d.parts_moved, vec!["R2"]);
    assert_eq!(d.nets_added, vec!["B"]);
    assert!(
        d.violations_added
            .iter()
            .any(|v| v.starts_with("courtyard_overlap")),
        "{:?}",
        d.violations_added
    );
    assert!(adds_error(&a, &b));
    assert!(!adds_error(&b, &b));
}

#[test]
fn a_model_error_has_a_position_and_stops_the_autorouter() {
    let (_dir, mut p) = project();
    p.data.routing.tracks.push(Track {
        id: "t9".into(),
        net: "NOPE".into(),
        layer: Layer::TOP,
        width: 0.2,
        points: vec![Point::new(3.0, 4.0), Point::new(6.0, 4.0)],
    });
    let v = p.check();
    let hit = v.iter().find(|x| x.rule == "unknown_net").unwrap();
    assert_eq!(hit.at, Some(Point::new(3.0, 4.0)));
    let e = p
        .autoroute(serde_json::from_value(serde_json::json!({})).unwrap())
        .unwrap_err()
        .to_string();
    assert!(e.contains("Correct these errors first"), "{e}");
    assert!(e.contains("unknown net NOPE"), "{e}");
}

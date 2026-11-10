//! Footprint evidence: verify, audit, and the rules that use the records.

use lamina_core::model::*;
use lamina_core::*;
use lamina_footprint::{Options, from_package};

/// A board with a header from the generator and a resistor that looks as if
/// it came from a KiCad library.
fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "test", Ports::default()).unwrap();
    let d = &mut p.data;
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    let mut r = from_package("R_0603", &Options::default(), Some("R_0603_1608Metric")).unwrap();
    r.provenance = Provenance::Manual {
        note: "from a library".into(),
    };
    d.footprints.insert(r.id.clone(), r);
    let h = from_package("PinHeader_1x03_P2.54", &Options::default(), Some("HDR")).unwrap();
    d.footprints.insert(h.id.clone(), h);
    d.parts
        .parts
        .insert("R1".into(), Part::new("R_0603_1608Metric"));
    d.parts.parts.insert("J1".into(), Part::new("HDR"));
    d.placement
        .placements
        .insert("R1".into(), Placement::at(10.0, 10.0));
    d.placement
        .placements
        .insert("J1".into(), Placement::at(20.0, 10.0));
    (dir, p)
}

fn rules_of(v: &[Violation]) -> Vec<&str> {
    v.iter().map(|x| x.rule.as_str()).collect()
}

#[test]
fn a_footprint_with_no_evidence_stops_the_release_and_audit_clears_it() {
    let (_t, mut p) = project();
    assert!(!rules_of(&p.check()).contains(&"footprint_unverified"));
    let release = p.check_release();
    let hits: Vec<&Violation> = release
        .iter()
        .filter(|v| v.rule == "footprint_unverified")
        .collect();
    assert_eq!(hits.len(), 1, "the generator footprint has evidence");
    assert_eq!(hits[0].severity, Severity::Error);
    assert_eq!(p.status().footprint_evidence.unverified, 1);

    let audit = p.audit_footprints(Default::default()).unwrap();
    let row = audit
        .rows
        .iter()
        .find(|r| r.footprint == "R_0603_1608Metric")
        .unwrap();
    assert_eq!(row.result, "verified", "{:?}", row.findings);
    assert!(!rules_of(&p.check_release()).contains(&"footprint_unverified"));
    assert_eq!(p.status().footprint_evidence.unverified, 0);
}

#[test]
fn a_changed_pad_makes_the_evidence_stale() {
    let (_t, mut p) = project();
    p.audit_footprints(Default::default()).unwrap();
    let mut d = p.data.clone();
    d.footprints.get_mut("R_0603_1608Metric").unwrap().pads[0]
        .at
        .x += 0.1;
    let mut touched = Touched::default();
    touched.footprints.insert("R_0603_1608Metric".into());
    p.commit(d, touched, false, "edit").unwrap();
    assert!(rules_of(&p.check()).contains(&"footprint_stale"));
}

#[test]
fn verify_with_datasheet_pads_finds_a_wrong_pitch() {
    let (_t, mut p) = project();
    // The datasheet land pattern of a 0603 with the pads 0.2 mm farther apart.
    let mut pads = p.data.footprints["R_0603_1608Metric"].pads.clone();
    let good = p
        .verify_footprint(VerifyFootprint {
            footprint: "R_0603_1608Metric".into(),
            document: Some("Datasheet rev 1".into()),
            page: Some("12".into()),
            url: None,
            package: None,
            pads: Some(pads.clone()),
            leads: None,
            accept: None,
        })
        .unwrap();
    assert!(good.pass, "{:?}", good.findings);
    pads[0].at.x -= 0.1;
    pads[1].at.x += 0.1;
    let bad = p
        .verify_footprint(VerifyFootprint {
            footprint: "R_0603_1608Metric".into(),
            document: Some("Datasheet rev 1".into()),
            page: None,
            url: None,
            package: None,
            pads: Some(pads),
            leads: None,
            accept: None,
        })
        .unwrap();
    assert!(!bad.pass);
    let v = p.check();
    let hit = v.iter().find(|x| x.rule == "footprint_mismatch").unwrap();
    assert_eq!(hit.severity, Severity::Error);
}

#[test]
fn verify_with_leads_finds_a_hole_that_is_too_small() {
    let (_t, mut p) = project();
    let verify = |p: &mut Project, size: f64| {
        p.verify_footprint(VerifyFootprint {
            footprint: "HDR".into(),
            document: Some("Header drawing".into()),
            page: None,
            url: None,
            package: None,
            pads: None,
            leads: Some(vec![Lead {
                pad: "1".into(),
                size,
            }]),
            accept: None,
        })
        .unwrap()
    };
    let hole = match &p.data.footprints["HDR"].pads[0].kind {
        PadKind::Through { drill, .. } => drill.min_size(),
        PadKind::Smd => panic!("a header has holes"),
    };
    assert!(verify(&mut p, hole - 0.2).pass);
    let tight = verify(&mut p, hole - 0.05);
    assert!(!tight.pass, "{:?}", tight.findings);
}

#[test]
fn accept_needs_a_reason_and_one_method_only() {
    let (_t, mut p) = project();
    let base = VerifyFootprint {
        footprint: "R_0603_1608Metric".into(),
        document: None,
        page: None,
        url: None,
        package: None,
        pads: None,
        leads: None,
        accept: None,
    };
    assert!(p.verify_footprint(base.clone()).is_err(), "no method");
    let mut blank = base.clone();
    blank.accept = Some(" ".into());
    assert!(p.verify_footprint(blank).is_err());
    let mut ok = base;
    ok.accept = Some("The same footprint is on three boards that work".into());
    assert!(p.verify_footprint(ok).unwrap().pass);
    let fp = &p.data.footprints["R_0603_1608Metric"];
    assert_eq!(fp.evidence.len(), 1);
    assert!(fp.source_hash.is_some());
}

#[test]
fn compare_two_footprints_of_the_project() {
    let (_t, p) = project();
    let c = p
        .compare_footprints(CompareFootprints {
            a: "R_0603_1608Metric".into(),
            b: "HDR".into(),
        })
        .unwrap();
    assert!(!c.same);
    assert!(
        p.compare_footprints(CompareFootprints {
            a: "R_0603_1608Metric".into(),
            b: "none".into(),
        })
        .is_err()
    );
}

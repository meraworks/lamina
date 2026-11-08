use lamina_drc::analyze;
use lamina_model::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("exclusions");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 20.0, 20.0));
    d.routing.vias.push(Via {
        id: "v1".into(),
        net: "A".into(),
        at: Point::new(5.0, 5.0),
        diameter: 0.6,
        drill: 0.3,
        from: None,
        to: None,
    });
    d
}

#[test]
fn exclusion_tracks_item_identity_after_a_move() {
    let mut d = board();
    assert!(
        analyze(&d)
            .violations
            .iter()
            .any(|v| v.rule == "via_dangling")
    );
    d.board.rules.exclusions.push(
        Exclusion::new(
            "via_dangling".into(),
            vec!["v1".into()],
            "Test point".into(),
        )
        .unwrap(),
    );
    for at in [Point::new(5.0, 5.0), Point::new(10.0, 10.0)] {
        d.routing.vias[0].at = at;
        let a = analyze(&d);
        assert!(!a.violations.iter().any(|v| v.rule == "via_dangling"));
        assert_eq!(a.excluded.len(), 1);
        assert_eq!(a.excluded[0].at, Some(at));
    }
    d.routing.vias.clear();
    let a = analyze(&d);
    assert!(a.excluded.is_empty());
    assert_eq!(
        a.violations
            .iter()
            .filter(|v| v.rule == "stale_exclusion")
            .count(),
        1
    );
}

#[test]
fn ignored_rule_does_not_make_an_exclusion_stale() {
    let mut d = board();
    d.board
        .rules
        .severities
        .insert("via_dangling".into(), RuleSeverity::Ignore);
    d.board.rules.exclusions.push(
        Exclusion::new(
            "via_dangling".into(),
            vec!["v1".into()],
            "Test point".into(),
        )
        .unwrap(),
    );
    let a = analyze(&d);
    assert!(!a.violations.iter().any(|v| v.rule == "stale_exclusion"));
    assert_eq!(a.excluded.len(), 1);
}

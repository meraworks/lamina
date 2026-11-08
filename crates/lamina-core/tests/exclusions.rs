use lamina_core::*;

#[test]
fn exclude_and_include_round_trip_with_a_required_reason() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(dir.path(), "test", Ports::default()).unwrap();
    let params = |reason: &str| ExcludeViolation {
        rule: "via_dangling".into(),
        items: vec!["v1".into()],
        reason: reason.into(),
    };
    assert!(p.exclude_violation(params(" ")).is_err());
    assert!(p.data.board.rules.exclusions.is_empty());
    let mut unknown = params("Test point");
    unknown.rule = "not_a_rule".into();
    assert!(p.exclude_violation(unknown).is_err());
    assert!(p.data.board.rules.exclusions.is_empty());
    p.exclude_violation(params("Test point")).unwrap();
    assert_eq!(p.data.board.rules.exclusions.len(), 1);
    p.exclude_violation(params("Updated reason")).unwrap();
    assert_eq!(p.data.board.rules.exclusions.len(), 1);
    let reopened = Project::open_shared_with(dir.path(), Ports::default()).unwrap();
    assert_eq!(
        reopened.data.board.rules.exclusions[0].reason,
        "Updated reason"
    );
    p.include_violation(IncludeViolation {
        rule: "via_dangling".into(),
        items: vec!["v1".into()],
    })
    .unwrap();
    assert!(p.data.board.rules.exclusions.is_empty());
}

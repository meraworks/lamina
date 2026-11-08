use lamina_core::*;

#[test]
fn review_export_preserves_unrelated_libraries_and_refuses_conflicts_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let p = Project::create_with(
        dir.path(),
        "test",
        Ports {
            formats: vec![std::sync::Arc::new(lamina_kicad::KicadFormat)],
            ..Default::default()
        },
    )
    .unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let table = out.join("fp-lib-table");
    std::fs::write(
        &table,
        "(fp_lib_table (lib (name userlib) (type KiCad) (uri user.pretty)))",
    )
    .unwrap();
    let board = out.join("board.kicad_pcb");
    let export = |path: &std::path::Path| {
        p.export_board(ExportBoard {
            format: "kicad".into(),
            path: Some(path.to_string_lossy().into_owned()),
        })
    };
    export(&board).unwrap();
    let text = std::fs::read_to_string(&table).unwrap();
    assert!(text.contains("userlib"));
    assert!(text.contains("lamina"));
    export(&board).unwrap();
    let other = out.join("other.kicad_pcb");
    assert!(export(&other).is_err());
    assert!(!other.exists());
    assert_eq!(std::fs::read_to_string(&table).unwrap(), text);
    std::fs::write(&table, "not a valid table").unwrap();
    std::fs::write(&board, "untouched").unwrap();
    assert!(export(&board).is_err());
    assert_eq!(std::fs::read_to_string(&board).unwrap(), "untouched");
}

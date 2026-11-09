mod common;

use std::process::Command;

use serde_json::json;

/// A small OBJ model: one triangle.
const OBJ: &str = "v -0.5 -0.5 0\nv 0.5 -0.5 0\nv 0 0.5 2\nf 1 2 3\n";

#[test]
fn status_reports_missing_models_and_restore_models_brings_them_back() {
    let t = tempfile::tempdir().unwrap();
    let board = t.path().join("board");
    let lib = t.path().join("lib");
    let src = t.path().join("body.obj");
    std::fs::write(&src, OBJ).unwrap();
    let env = [("LAMINA_LIB", lib.as_path())];

    let mut c = common::Client::start_with_env(&env);
    assert!(c.tool_names().contains(&"restore_models".to_string()));
    let (err, msg) = c.call("restore_models", json!({}));
    assert!(err && msg.as_str().unwrap().contains("No project is open"));

    c.ok("open", json!({"path": board, "create": true}));
    c.ok("footprint_from_package", json!({"package": "R_0603"}));
    c.ok("set_model", json!({"footprint": "R_0603", "file": src}));
    c.ok("add_part", json!({"refdes": "R1", "footprint": "R_0603"}));
    c.ok("place", json!({"refdes": "R1", "x": 1, "y": 1}));
    let s = c.ok("status", json!({}));
    assert!(s.get("missing_models").is_none(), "{s}");

    let cached = board.join("cache/models/user/body.obj");
    std::fs::remove_file(&cached).unwrap();
    let footprint = std::fs::read(board.join("cache/footprints/R_0603.json")).unwrap();
    let placement = std::fs::read(board.join("placement.json")).unwrap();
    let s = c.ok("status", json!({}));
    assert_eq!(
        s["missing_models"],
        json!([{"footprint": "R_0603", "file": "user/body.obj"}]),
        "{s}"
    );

    // Not in the library, and the footprint is not from a provider.
    let r = c.ok("restore_models", json!({"download": false}));
    assert_eq!(r["missing"].as_array().unwrap().len(), 1, "{r}");
    assert!(
        r["missing"][0]
            .as_str()
            .unwrap()
            .contains("user/body.obj (R_0603)"),
        "{r}"
    );
    assert!(!cached.exists());

    // With a copy in the library, the tool restores it.
    std::fs::create_dir_all(lib.join("models/user")).unwrap();
    std::fs::write(lib.join("models/user/body.obj"), OBJ).unwrap();
    let r = c.ok("restore_models", json!({}));
    assert_eq!(r["from_library"], json!(["user/body.obj (R_0603)"]), "{r}");
    assert_eq!(r["missing"], json!([]), "{r}");
    assert_eq!(std::fs::read_to_string(&cached).unwrap(), OBJ);
    let s = c.ok("status", json!({}));
    assert!(s.get("missing_models").is_none(), "{s}");
    let r = c.ok("restore_models", json!({}));
    assert_eq!(r["present"], 1, "{r}");
    assert_eq!(r["from_library"], json!([]), "{r}");
    assert_eq!(
        std::fs::read(board.join("cache/footprints/R_0603.json")).unwrap(),
        footprint
    );
    assert_eq!(
        std::fs::read(board.join("placement.json")).unwrap(),
        placement
    );
    drop(c);

    // The command line does the same.
    std::fs::remove_file(&cached).unwrap();
    std::fs::remove_file(lib.join("models/user/body.obj")).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_lamina"))
            .args(args)
            .env("LAMINA_LIB", &lib)
            .output()
            .unwrap()
    };
    let b = board.to_string_lossy().to_string();
    let out = run(&["status", &b]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"missing_models\""));
    let out = run(&["restore-models", &b, "--no-download"]);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("user/body.obj (R_0603)"), "{text}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("1 model(s) are still missing"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::write(lib.join("models/user/body.obj"), OBJ).unwrap();
    let out = run(&["restore-models", &b]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["from_library"], json!(["user/body.obj (R_0603)"]), "{r}");
    assert!(cached.is_file());
    let out = run(&["status", &b]);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("\"missing_models\""));
}

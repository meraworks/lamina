mod common;

use serde_json::json;

#[test]
fn unplace_is_repeatable_and_reports_skipped_parts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unplace");
    let mut c = common::Client::start();
    c.ok("open", json!({"path": path, "create": true}));
    c.ok("footprint_from_package", json!({"package": "R_0603"}));
    c.ok(
        "add_parts",
        json!({"parts": [
            {"refdes": "R1", "footprint": "R_0603"},
            {"refdes": "R2", "footprint": "R_0603"}
        ]}),
    );
    c.ok("place", json!({"refdes": "R1", "x": 5, "y": 5}));
    let placement = path.join("placement.json");
    let before = std::fs::read(&placement).unwrap();
    let (is_error, error) = c.call("unplace", json!({"refdes": ["R1", "R2", "unknown"]}));
    assert!(
        is_error && error.as_str().unwrap().contains("unknown does not exist"),
        "{error}"
    );
    assert_eq!(std::fs::read(&placement).unwrap(), before);

    let result = c.ok("unplace", json!({"refdes": ["R1", "R2"]}));
    assert_eq!(result["already_unplaced"], json!(["R2"]));
    assert_eq!(result["changed"], json!(["placement.json"]));
    assert_eq!(c.ok("status", json!({}))["unplaced"], json!(["R1", "R2"]));

    let result = c.ok("unplace", json!({"refdes": ["R1", "R2"]}));
    assert_eq!(result["already_unplaced"], json!(["R1", "R2"]));
    assert_eq!(result["changed"], json!([]));
}

#[test]
fn place_route_pour_check() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lay").to_string_lossy().to_string();
    let mut c = common::Client::start();
    c.ok("open", json!({"path": path, "create": true}));
    c.ok("use_fab_preset", json!({"name": "jlcpcb-2layer"}));
    c.ok(
        "set_outline",
        json!({"rect": {"width": 30, "height": 20, "corner_radius": 1}}),
    );
    c.ok("footprint_from_package", json!({"package": "R_0603"}));
    c.ok("footprint_from_package", json!({"package": "SOIC-8"}));
    c.ok(
        "add_parts",
        json!({"parts": [
            {"refdes": "R1", "footprint": "R_0603", "value": "10k"},
            {"refdes": "R2", "footprint": "R_0603", "value": "10k"},
            {"refdes": "U1", "footprint": "SOIC-8", "value": "NE555"}
        ]}),
    );
    let s = c.ok("status", json!({}));
    assert_eq!(s["unplaced"].as_array().unwrap().len(), 3);
    c.ok(
        "place_many",
        json!({"placements": {
            "R1": {"x": 6, "y": 10, "rotation": 90},
            "R2": {"x": 24, "y": 10},
            "U1": {"x": 15, "y": 10}
        }}),
    );
    c.ok(
        "set_nets",
        json!({"nets": {
            "SIG": {"pads": ["R1.2", "U1.2"]},
            "OUT": {"pads": ["U1.3", "R2.1"]},
            "GND": {"pads": ["U1.1", "R1.1", "R2.2"]}
        }}),
    );
    let rn = c.ok("ratsnest", json!({}));
    assert!(rn.as_array().unwrap().len() >= 4, "{rn}");

    let m = c.ok("measure", json!({"from": "U1.1", "to": "U1.8"}));
    assert!(m["distance"].as_f64().unwrap() > 4.0, "{m}");

    // Route one connection by hand. Autoroute all other nets except GND.
    let (err, msg) = c.call(
        "route_trace",
        json!({"layer": "bottom", "points": ["R1.2", "U1.2"]}),
    );
    assert!(
        err && msg.as_str().unwrap().contains("no copper on bottom"),
        "{msg}"
    );
    let r = c.ok("autoroute", json!({"skip": ["GND"]}));
    assert!(r["summary"].as_str().unwrap().contains("0 failed"), "{r}");

    c.ok(
        "add_zone",
        json!({"net": "GND", "layers": ["top", "bottom"]}),
    );
    let chk = c.ok("check", json!({"warnings": false}));
    assert_eq!(chk["total"], 0, "errors after routing: {chk}");
    let rn = c.ok("ratsnest", json!({}));
    assert!(
        rn.as_array().unwrap().is_empty(),
        "the GND pour must connect all pads: {rn}"
    );
}

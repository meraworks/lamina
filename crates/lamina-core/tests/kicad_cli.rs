//! A second opinion from KiCad. The test runs only if the variable
//! `KICAD_CLI` has the path of `kicad-cli`. It exports a board with known
//! violations, runs the KiCad DRC and compares the two reports.
//!
//! Differences with an explanation: KiCad reports one `unconnected_items`
//! line for each missing connection, and Lamina one `unrouted` line for each
//! net. Lamina reports the two free ends of a track; KiCad reports the track.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use lamina_core::model::*;
use lamina_core::ports::format::BoardFormat;
use lamina_footprint::{Options, from_package};

fn board() -> ProjectData {
    let mut d = ProjectData::new("cli");
    d.board.outline = Some(Contour::rect(0.0, 0.0, 40.0, 30.0));
    let fp = from_package("R_0603", &Options::default(), None).unwrap();
    d.footprints.insert(fp.id.clone(), fp);
    for (r, x) in [("R1", 10.0), ("R2", 30.0)] {
        d.parts.parts.insert(r.into(), Part::new("R_0603"));
        d.placement
            .placements
            .insert(r.into(), Placement::at(x, 5.0));
    }
    for (name, pads) in [
        ("SIG", vec!["R1.2", "R2.1"]),
        ("GND", vec!["R1.1"]),
        ("OTHER", vec!["R2.2"]),
    ] {
        d.nets.nets.insert(
            name.into(),
            Net {
                pads: pads.iter().map(|p| p.parse().unwrap()).collect(),
                ..Default::default()
            },
        );
    }
    let r = &mut d.board.rules;
    r.clearance = 0.2;
    r.min_track_width = 0.15;
    r.min_annular_ring = 0.13;
    r.min_via_drill = 0.2;
    // A smaller ring is permitted on GND. KiCad gets this from the rules file.
    r.custom.push(CustomRule {
        name: "gnd-ring".into(),
        when: RuleMatch {
            net: Some("GND".into()),
            class: None,
            layer: None,
            ..Default::default()
        },
        min_annular_ring: Some(0.04),
        ..Default::default()
    });
    let mut track = |id: &str, net: &str, width: f64, a: (f64, f64), b: (f64, f64)| {
        d.routing.tracks.push(Track {
            id: id.into(),
            net: net.into(),
            layer: Layer::TOP,
            width,
            points: vec![Point::new(a.0, a.1), Point::new(b.0, b.1)],
        });
    };
    // Two tracks of different nets, 0.05 mm apart: `clearance`.
    track("t1", "SIG", 0.2, (5.0, 15.0), (15.0, 15.0));
    track("t2", "OTHER", 0.2, (5.0, 15.25), (15.0, 15.25));
    // A track that is too narrow: `track_width`.
    track("t3", "SIG", 0.08, (20.0, 20.0), (30.0, 20.0));
    let mut via = |id: &str, net: &str, x: f64| {
        d.routing.vias.push(Via {
            id: id.into(),
            net: net.into(),
            at: Point::new(x, 25.0),
            diameter: 0.4,
            drill: 0.3,
            from: None,
            to: None,
        });
    };
    // A ring of 0.05 mm: an error on SIG, permitted on GND.
    via("v1", "SIG", 10.0);
    via("v2", "GND", 20.0);
    d
}

#[test]
fn kicad_and_lamina_agree_on_a_board_with_known_violations() {
    let Ok(cli) = std::env::var("KICAD_CLI") else {
        eprintln!("KICAD_CLI is not set. The comparison with KiCad did not run.");
        return;
    };
    let d = board();
    let lamina: BTreeMap<String, usize> =
        lamina_drc::release_view(&lamina_drc::analyze(&d), &d.board.rules)
            .iter()
            .filter_map(|v| lamina_kicad::kicad_rule_name(&v.rule))
            .fold(BTreeMap::new(), |mut m, k| {
                *m.entry(k.to_string()).or_default() += 1;
                m
            });
    for rule in [
        "clearance",
        "track_width",
        "annular_width",
        "unconnected_items",
    ] {
        assert!(
            lamina.contains_key(rule),
            "Lamina does not report {rule}: {lamina:?}"
        );
    }
    assert_eq!(
        lamina["annular_width"], 1,
        "the custom rule permits the GND via"
    );

    let dir = tempfile::tempdir().unwrap();
    let pcb = dir.path().join("cli.kicad_pcb");
    let format = lamina_kicad::KicadFormat;
    let text = format.write(&d, &dir.path().join("models")).unwrap();
    std::fs::write(&pcb, text).unwrap();
    for (path, text) in format.companions(&d, &pcb) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }
    assert!(pcb.with_extension("kicad_dru").exists());
    let report = dir.path().join("drc.json");
    let out = Command::new(&cli)
        .args([
            "pcb",
            "drc",
            "--format",
            "json",
            "--severity-all",
            "--output",
        ])
        .arg(&report)
        .arg(&pcb)
        .output()
        .expect("run kicad-cli");
    assert!(
        Path::new(&report).exists(),
        "kicad-cli wrote no report: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let mut kicad: BTreeMap<String, usize> = BTreeMap::new();
    for v in json["violations"].as_array().into_iter().flatten() {
        *kicad
            .entry(v["type"].as_str().unwrap_or("").to_string())
            .or_default() += 1;
    }
    let open = json["unconnected_items"].as_array().map_or(0, Vec::len);
    if open > 0 {
        kicad.insert("unconnected_items".into(), open);
    }
    eprintln!("lamina: {lamina:?}\nkicad:  {kicad:?}");
    for rule in [
        "clearance",
        "track_width",
        "annular_width",
        "unconnected_items",
    ] {
        assert!(
            kicad.contains_key(rule),
            "KiCad does not report {rule}: {kicad:?}"
        );
    }
    assert_eq!(
        kicad["annular_width"], lamina["annular_width"],
        "the rules file gives KiCad the same custom rule"
    );
    assert_eq!(kicad["track_width"], lamina["track_width"]);
    assert_eq!(kicad["clearance"], lamina["clearance"]);
}

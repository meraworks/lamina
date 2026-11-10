//! A record of what the checks report on the real boards in `examples`.
//! A change to the check engine must not change these files, if the task
//! does not say so. Run with `LAMINA_BLESS=1` to write the files again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lamina_core::model::*;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(board: &str) -> ProjectData {
    ProjectData::load(&root().join("examples").join(board)).expect("load the example board")
}

/// Limits that make many rules report.
fn tight(d: &mut ProjectData) {
    let r = &mut d.board.rules;
    r.exclusions.clear();
    r.clearance = 0.3;
    r.min_track_width = 0.3;
    r.min_annular_ring = 0.3;
    r.hole_to_hole = 1.0;
    r.copper_to_edge = 1.0;
    r.silk_to_pad = 0.3;
    r.silk_to_edge = 0.5;
    r.min_silk_text_height = 1.5;
    r.min_mask_web = 0.2;
    r.min_thermal_spokes = 4;
}

fn line(v: &Violation) -> String {
    let at =
        v.at.map(|p| format!("{:.3},{:.3}", p.x, p.y))
            .unwrap_or_default();
    let mut items = v.items.clone();
    items.sort();
    format!(
        "{:?} | {} | {} | {} | {} | {}",
        v.severity,
        v.rule,
        v.layer.map(|l| l.to_string()).unwrap_or_default(),
        items.join(","),
        v.message,
        at
    )
}

fn snapshot(d: &ProjectData) -> String {
    let a = lamina_drc::analyze(d);
    let mut v: Vec<String> = a.violations.iter().map(line).collect();
    v.sort();
    let mut x: Vec<String> = a.excluded.iter().map(line).collect();
    x.sort();
    let mut r: Vec<String> = a
        .ratsnest
        .iter()
        .map(|r| format!("{} | {} | {}", r.net, r.from, r.to))
        .collect();
    r.sort();
    // The number of copper islands for each net.
    let mut islands: BTreeMap<String, std::collections::BTreeSet<usize>> = BTreeMap::new();
    for (it, isl) in a.geometry.copper.iter().zip(&a.island) {
        if let Some(n) = &it.net {
            islands.entry(n.clone()).or_default().insert(*isl);
        }
    }
    let isl: Vec<String> = islands
        .iter()
        .map(|(n, s)| format!("{n} {}", s.len()))
        .collect();
    format!(
        "# violations\n{}\n# excluded\n{}\n# ratsnest\n{}\n# islands\n{}\n",
        v.join("\n"),
        x.join("\n"),
        r.join("\n"),
        isl.join("\n")
    )
}

fn check(name: &str, text: &str) {
    let path = root()
        .join("crates/lamina-core/tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("LAMINA_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing. Run with LAMINA_BLESS=1.", path.display()))
        .replace("\r\n", "\n");
    if want != text {
        let old: Vec<&str> = want.lines().collect();
        let new: Vec<&str> = text.lines().collect();
        let gone: Vec<&&str> = old.iter().filter(|l| !new.contains(l)).take(20).collect();
        let added: Vec<&&str> = new.iter().filter(|l| !old.contains(l)).take(20).collect();
        panic!("{name} changed.\nGone: {gone:#?}\nNew: {added:#?}");
    }
}

#[test]
fn golden_tit() {
    check("tit", &snapshot(&load("tit-flight-computer")));
}

#[test]
fn golden_yoink() {
    check("yoink", &snapshot(&load("yoink-main-board")));
}

#[test]
fn analysis_is_deterministic() {
    let d = load("yoink-main-board");
    assert_eq!(snapshot(&d), snapshot(&d));
}

#[test]
#[ignore = "slow in a debug build"]
fn golden_tit_tight() {
    let mut d = load("tit-flight-computer");
    tight(&mut d);
    check("tit-tight", &snapshot(&d));
}

#[test]
#[ignore = "slow in a debug build"]
fn golden_yoink_tight() {
    let mut d = load("yoink-main-board");
    tight(&mut d);
    check("yoink-tight", &snapshot(&d));
}

#[test]
fn connectivity_gives_the_islands_and_ratsnest_of_analyze() {
    for board in ["tit-flight-computer", "yoink-main-board"] {
        let mut d = load(board);
        // Open some connections, so that the ratsnest is not empty.
        d.routing.tracks.truncate(d.routing.tracks.len() / 2);
        let a = lamina_drc::analyze(&d);
        let c = lamina_drc::connectivity(&d);
        assert!(!a.ratsnest.is_empty(), "{board}");
        assert_eq!(a.ratsnest, c.ratsnest, "{board}");
        assert_eq!(a.island, c.island, "{board}");
        assert_eq!(a.fill_island, c.fill_island, "{board}");
        assert_eq!(a.geometry.copper.len(), c.geometry.copper.len(), "{board}");
        assert_eq!(a.fills, c.fills, "{board}");
    }
}

#[test]
fn each_golden_line_has_a_triage_line() {
    let dir = root().join("crates/lamina-core/tests/golden");
    let triage = std::fs::read_to_string(dir.join("triage.txt")).unwrap();
    let mut known = std::collections::BTreeSet::new();
    for line in triage
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let f: Vec<&str> = line.split(" | ").collect();
        assert!(f.len() == 5, "a triage line has 5 fields: {line}");
        assert!(f[3] == "true" || f[3] == "false", "{line}");
        assert!(!f[4].trim().is_empty(), "{line}");
        known.insert(format!("{} | {} | {}", f[0], f[1], f[2]));
    }
    for board in ["tit", "yoink"] {
        let text = std::fs::read_to_string(dir.join(format!("{board}.txt"))).unwrap();
        let mut section = "";
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("# ") {
                section = name;
                continue;
            }
            if !(section == "violations" || section == "excluded") || line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split(" | ").collect();
            let key = format!("{board} | {} | {}", f[1], f[3]);
            assert!(known.contains(&key), "Add a line to triage.txt for: {key}");
        }
    }
}

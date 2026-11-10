//! Recall: each test puts one real defect back into a copy of an example
//! board and expects the rule that must find it. The label defects (pin
//! labels in the reverse order, a plus mark at the ground screw, a label
//! far from its part) are in `crates/lamina-drc/tests/labels.rs`. The
//! thermal spoke in a hole is in `crates/lamina-geom/tests/fill_trust.rs`.

use std::path::Path;

use lamina_core::model::*;

fn load(board: &str) -> ProjectData {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    ProjectData::load(&root.join(board)).unwrap()
}

fn rules(d: &ProjectData, rule: &str) -> Vec<Violation> {
    lamina_drc::analyze(d)
        .violations
        .into_iter()
        .filter(|v| v.rule == rule)
        .collect()
}

#[test]
fn copper_text_on_a_track_is_found() {
    // A text on a copper layer went to the Gerber files, but no check saw it.
    let mut d = load("tit-flight-computer");
    assert!(rules(&d, "short").is_empty() && rules(&d, "clearance").is_empty());
    let t = d
        .routing
        .tracks
        .iter()
        .find(|t| t.layer == Layer::TOP && t.points.len() >= 2)
        .unwrap()
        .clone();
    let at = Point::new(
        (t.points[0].x + t.points[1].x) / 2.0,
        (t.points[0].y + t.points[1].y) / 2.0,
    );
    d.routing.texts.push(BoardText {
        id: "bad".into(),
        layer: Layer::TOP,
        text: "REV A".into(),
        at,
        size: 1.5,
        rotation: 0.0,
    });
    let found: Vec<Violation> = lamina_drc::analyze(&d)
        .violations
        .into_iter()
        .filter(|v| {
            (v.rule == "short" || v.rule == "clearance") && v.items.contains(&"bad".to_string())
        })
        .collect();
    assert!(
        !found.is_empty(),
        "the copper text on {} is not found",
        t.id
    );
}

#[test]
fn track_through_a_mounting_hole_is_found() {
    // No check compared a drilled hole with copper.
    let mut d = load("tit-flight-computer");
    let (h, pl) = d
        .placement
        .placements
        .iter()
        .find(|(r, _)| r.starts_with('H'))
        .map(|(r, pl)| (r.clone(), *pl))
        .expect("the board has mounting holes");
    assert!(rules(&d, "hole_to_copper").is_empty());
    let net = d.nets.nets.keys().next().unwrap().clone();
    d.routing.tracks.push(Track {
        id: "bad".into(),
        net,
        layer: Layer::BOTTOM,
        width: 0.2,
        points: vec![Point::new(pl.x - 3.0, pl.y), Point::new(pl.x + 3.0, pl.y)],
    });
    let v = rules(&d, "hole_to_copper");
    assert!(
        v.iter().any(|x| x.items.contains(&"bad".to_string())),
        "the track through {h} is not found: {v:?}"
    );
}

#[test]
fn a_pad_moved_after_the_check_is_found() {
    // A footprint with evidence, then one pad moves by 0.1 mm.
    let mut d = load("tit-flight-computer");
    let id = d
        .footprints
        .iter()
        .find(|(id, fp)| {
            fp.current_evidence().is_some() && d.parts.parts.values().any(|p| &p.footprint == *id)
        })
        .map(|(id, _)| id.clone())
        .expect("a footprint in use has evidence");
    assert!(rules(&d, "footprint_stale").is_empty());
    d.footprints.get_mut(&id).unwrap().pads[0].at.x += 0.1;
    let v = rules(&d, "footprint_stale");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, vec![id]);
}

#[test]
fn a_pad_with_no_paste_in_a_part_with_paste_is_found() {
    // The first import of this board lost the paste of the module ground pad.
    let mut d = load("tit-flight-computer");
    assert!(rules(&d, "paste_missing").is_empty());
    let fp = d.footprints.get_mut("ESP32-S3-WROOM-1").unwrap();
    fp.graphics.retain(|g| g.layer != FpLayer::Paste);
    let v = rules(&d, "paste_missing");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, vec!["U1"]);
}

#[test]
fn a_pour_in_the_antenna_area_is_found() {
    // The first import of this board lost the keep-out below the antenna.
    let mut d = load("tit-flight-computer");
    assert!(rules(&d, "keepout").is_empty());
    let before: f64 = pour_area(&d);
    let fp = d.footprints.get_mut("ESP32-S3-WROOM-1").unwrap();
    assert!(!fp.keepouts.is_empty(), "the footprint has its keep-out");
    fp.keepouts.clear();
    let after = pour_area(&d);
    assert!(
        after > before + 20.0,
        "with no keep-out the pour fills the antenna area: {before:.1} to {after:.1} mm2"
    );
}

fn pour_area(d: &ProjectData) -> f64 {
    lamina_drc::analyze(d)
        .fills
        .iter()
        .flat_map(|f| f.polygons.iter())
        .map(|p| {
            let ring = |r: &[Point]| {
                let mut a = 0.0;
                for i in 0..r.len() {
                    let (p, q) = (r[i], r[(i + 1) % r.len()]);
                    a += p.x * q.y - q.x * p.y;
                }
                (a / 2.0).abs()
            };
            ring(&p.outer) - p.holes.iter().map(|h| ring(h)).sum::<f64>()
        })
        .sum()
}

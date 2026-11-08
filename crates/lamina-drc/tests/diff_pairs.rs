use lamina_model::*;

fn board() -> ProjectData {
    let mut d = ProjectData::new("pair");
    for (net, y) in [("P", 5.0), ("N", 5.45)] {
        d.nets.nets.insert(net.into(), Net::default());
        d.routing.tracks.push(Track {
            id: net.into(),
            net: net.into(),
            layer: Layer::TOP,
            width: 0.25,
            points: vec![Point::new(5.0, y), Point::new(15.0, y)],
        });
    }
    d.nets.diff_pairs.insert(
        "data".into(),
        DiffPair {
            positive: "P".into(),
            negative: "N".into(),
            gap: 0.2,
            width: 0.25,
        },
    );
    d
}

fn count(d: &ProjectData, rule: &str) -> usize {
    lamina_drc::analyze(d)
        .violations
        .iter()
        .filter(|v| v.rule == rule)
        .count()
}

#[test]
fn matched_pair_and_wrong_gap() {
    let mut d = board();
    assert_eq!(count(&d, "diff_pair_gap"), 0);
    assert_eq!(count(&d, "diff_pair_uncoupled"), 0);
    for p in &mut d.routing.tracks[1].points {
        p.y += 0.3;
    }
    assert!(count(&d, "diff_pair_gap") > 0);
}

#[test]
fn missing_partial_or_wrong_layer_partner_warns() {
    let mut d = board();
    d.routing.tracks[1].layer = Layer::BOTTOM;
    assert!(count(&d, "diff_pair_uncoupled") > 0);
    d.routing.tracks[1].layer = Layer::TOP;
    d.routing.tracks[1].points[1].x = 10.0;
    assert!(count(&d, "diff_pair_uncoupled") > 0);
    d.routing.tracks.pop();
    assert!(count(&d, "diff_pair_uncoupled") > 0);
}

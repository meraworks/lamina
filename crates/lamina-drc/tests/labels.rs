//! The label checks, with the real defects put back into a real board.

use lamina_drc::*;
use lamina_model::*;

fn yoink() -> ProjectData {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    ProjectData::load(&root.join("yoink-main-board")).unwrap()
}

fn text(d: &mut ProjectData, id: &str, s: &str, at: Point, rotation: f64) {
    d.routing.texts.push(BoardText {
        id: id.into(),
        layer: Layer::Silk(Side::Top),
        text: s.into(),
        at,
        size: 0.8,
        rotation,
    });
}

fn hits<'a>(a: &'a Analysis, rule: &str) -> Vec<&'a Violation> {
    a.violations.iter().filter(|v| v.rule == rule).collect()
}

#[test]
fn the_corrected_board_has_no_label_warning() {
    let a = analyze(&yoink());
    for rule in ["label_far", "label_pin", "polarity_mark", "silk_under_part"] {
        assert!(hits(&a, rule).is_empty(), "{rule}: {:?}", hits(&a, rule));
    }
}

#[test]
fn pin_labels_in_the_reverse_order_are_found() {
    // The first version of this board had one text for the five pins of J11
    // (GND, GND, SCL, SDA, 3V3 from the top), turned 90 degrees. It reads
    // from the bottom to the top, so VCC was at the GND pin.
    let mut d = yoink();
    d.routing
        .texts
        .retain(|t| !["GND", "OE", "SCL", "SDA", "VCC", "PCA9685"].contains(&t.text.as_str()));
    text(
        &mut d,
        "bad",
        "PCA9685 GND OE SCL SDA VCC",
        Point::new(6.0, 14.9),
        90.0,
    );
    let a = analyze(&d);
    let v = hits(&a, "label_pin");
    assert_eq!(v.len(), 1, "{:?}", a.violations);
    assert!(v[0].message.contains("reverse order"), "{}", v[0].message);
    assert!(v[0].items.contains(&"J11".to_string()));
    assert_eq!(
        v[0].fix,
        vec![Fix::TurnText {
            id: "bad".into(),
            by: 180.0
        }]
    );
    // The same text turned the other way is correct.
    d.routing.texts.last_mut().unwrap().rotation = 270.0;
    assert!(hits(&analyze(&d), "label_pin").is_empty());
}

#[test]
fn a_plus_mark_above_the_ground_screw_is_found() {
    // "VBAT + -" centred above the terminal J1: the + is above the GND screw.
    let mut d = yoink();
    d.routing
        .texts
        .retain(|t| !(t.at.y > 56.0 && t.at.x > 68.0));
    text(&mut d, "bad", "VBAT + -", Point::new(72.5, 56.9), 0.0);
    let a = analyze(&d);
    let v = hits(&a, "polarity_mark");
    assert_eq!(v.len(), 1, "{:?}", a.violations);
    assert!(v[0].message.contains("J1"), "{}", v[0].message);
    // With the + above the VBAT screw and the - above the GND screw, it is correct.
    d.routing.texts.pop();
    text(&mut d, "p", "+", Point::new(70.0, 56.9), 0.0);
    text(&mut d, "m", "-", Point::new(75.0, 56.9), 0.0);
    assert!(hits(&analyze(&d), "polarity_mark").is_empty());
}

#[test]
fn a_label_far_from_its_part_is_found() {
    // On the collar board, "U1 nRF54L15" was 10 mm from U1, next to a capacitor.
    let mut d = yoink();
    // A label 1 mm above the courtyard of U1.
    let fp = &d.footprints[&d.parts.parts["U1"].footprint];
    let c = lamina_geom::courtyard(fp, &d.placement.placements["U1"]).unwrap();
    let b = Rect::from_points(c).unwrap();
    text(
        &mut d,
        "near",
        "U1 TA6586",
        Point::new((b.min.x + b.max.x) / 2.0, b.max.y + 1.4),
        0.0,
    );
    assert!(
        hits(&analyze(&d), "label_far").is_empty(),
        "a label at the part"
    );
    d.routing.texts.pop();
    let c5 = d.placement.placements["C5"];
    text(
        &mut d,
        "far",
        "U1 TA6586",
        Point::new(c5.x, c5.y - 7.0),
        0.0,
    );
    let a = analyze(&d);
    let v = hits(&a, "label_far");
    assert_eq!(v.len(), 1, "{:?}", a.violations);
    assert!(v[0].items.contains(&"U1".to_string()));
}

#[test]
fn a_text_below_a_part_body_is_found() {
    let mut d = yoink();
    let u1 = d.placement.placements["U1"];
    // DIP-8: the body is between the two pin rows.
    text(
        &mut d,
        "under",
        "X",
        Point::new(u1.x + 3.8, u1.y - 3.8),
        0.0,
    );
    let a = analyze(&d);
    assert_eq!(hits(&a, "silk_under_part").len(), 1, "{:?}", a.violations);
}

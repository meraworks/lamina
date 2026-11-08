//! Design rules and net classes from a `.kicad_pro` file. KiCad 6+ keeps
//! them there, not in the board file.

use lamina_model::*;
use serde_json::{Value, json};

fn r6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// The KiCad net class pattern: `*` and `?` wildcards, or an exact name.
fn wildcard(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

/// Apply the rules, net classes and class assignments of a `.kicad_pro`
/// to an imported board. Returns notes about the data that it used.
pub fn apply_project(d: &mut ProjectData, pro: &str) -> Result<Vec<String>, String> {
    let v: Value =
        serde_json::from_str(pro).map_err(|e| format!("the .kicad_pro file is not valid: {e}"))?;
    let rules = &mut d.board.rules;
    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64).filter(|x| *x > 0.0);

    let ds = &v["board"]["design_settings"]["rules"];
    if let Some(x) = num(ds, "min_track_width") {
        rules.min_track_width = r6(x);
    }
    if let Some(x) = num(ds, "min_through_hole_diameter") {
        rules.min_drill = r6(x);
        rules.min_via_drill = r6(x);
    }
    if let Some(x) = num(ds, "min_via_annular_width") {
        rules.min_annular_ring = r6(x);
    }
    if let Some(x) = num(ds, "min_hole_to_hole") {
        rules.hole_to_hole = r6(x);
    }
    if let Some(x) = num(ds, "min_copper_edge_clearance") {
        rules.copper_to_edge = r6(x);
    }

    let ns = &v["net_settings"];
    let classes = ns["classes"].as_array().cloned().unwrap_or_default();
    let mut named = 0;
    // KiCad 6/7 lists the nets of a class in the class.
    let mut members: Vec<(String, String)> = Vec::new();
    for c in &classes {
        let Some(name) = c["name"].as_str() else {
            continue;
        };
        let class = NetClass {
            clearance: num(c, "clearance").map(r6),
            track_width: num(c, "track_width").map(r6),
            via_diameter: num(c, "via_diameter").map(r6),
            via_drill: num(c, "via_drill").map(r6),
        };
        if name == "Default" {
            if let Some(x) = class.clearance {
                rules.clearance = x;
            }
            if let Some(x) = class.track_width {
                rules.track_width = x;
            }
            if let Some(x) = class.via_diameter {
                rules.via_diameter = x;
            }
            if let Some(x) = class.via_drill {
                rules.via_drill = x;
            }
            continue;
        }
        for n in c["nets"].as_array().into_iter().flatten() {
            if let Some(n) = n.as_str() {
                members.push((n.to_string(), name.to_string()));
            }
        }
        rules.net_classes.insert(name.to_string(), class);
        named += 1;
    }
    rules.preset = None;

    // KiCad 8+: explicit assignments, then patterns in order.
    if let Some(a) = ns["netclass_assignments"].as_object() {
        for (net, c) in a {
            let c = c
                .as_str()
                .or_else(|| c.as_array().and_then(|l| l.first()).and_then(Value::as_str));
            if let Some(c) = c {
                members.push((net.clone(), c.to_string()));
            }
        }
    }
    let patterns: Vec<(String, String)> = ns["netclass_patterns"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p["pattern"].as_str()?.into(),
                p["netclass"].as_str()?.into(),
            ))
        })
        .collect();

    let mut assigned = 0;
    for (name, net) in d.nets.nets.iter_mut() {
        let class = members
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c)
            .or_else(|| {
                patterns
                    .iter()
                    .find(|(p, _)| wildcard(p, name))
                    .map(|(_, c)| c)
            });
        if let Some(c) = class.filter(|c| rules.net_classes.contains_key(*c)) {
            net.class = Some(c.clone());
            assigned += 1;
        }
    }
    Ok(vec![format!(
        "rules from the KiCad project: {named} net classes, {assigned} nets assigned"
    )])
}

/// The `.kicad_pro` text for a board.
pub fn export_project(d: &ProjectData, name: &str) -> String {
    let r = &d.board.rules;
    let class = |name: &str, c: &NetClass| {
        json!({
            "name": name,
            "clearance": c.clearance.unwrap_or(r.clearance),
            "track_width": c.track_width.unwrap_or(r.track_width),
            "via_diameter": c.via_diameter.unwrap_or(r.via_diameter),
            "via_drill": c.via_drill.unwrap_or(r.via_drill),
        })
    };
    let mut classes = vec![class("Default", &NetClass::default())];
    classes.extend(r.net_classes.iter().map(|(n, c)| class(n, c)));
    let assigned: serde_json::Map<String, Value> = d
        .nets
        .nets
        .iter()
        .filter_map(|(n, net)| Some((n.clone(), json!(net.class.clone()?))))
        .collect();
    let min_clearance = r
        .net_classes
        .values()
        .filter_map(|c| c.clearance)
        .fold(r.clearance, f64::min);
    let text = json!({
        "board": {"design_settings": {"rules": {
            "min_clearance": min_clearance,
            "min_track_width": r.min_track_width,
            "min_through_hole_diameter": r.min_drill.min(r.min_via_drill),
            "min_via_annular_width": r.min_annular_ring,
            "min_via_diameter": r.min_via_drill + 2.0 * r.min_annular_ring,
            "min_hole_to_hole": r.hole_to_hole,
            "min_copper_edge_clearance": r.copper_to_edge,
        }}},
        "meta": {"filename": format!("{name}.kicad_pro"), "version": 3},
        "net_settings": {
            "classes": classes,
            "meta": {"version": 4},
            "netclass_assignments": assigned,
        },
    });
    serde_json::to_string_pretty(&text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard("GND", "GND"));
        assert!(!wildcard("GND", "AGND"));
        assert!(wildcard("*GND", "AGND"));
        assert!(wildcard("V?US", "VBUS"));
        assert!(wildcard("PWM*", "PWM12"));
        assert!(!wildcard("PWM?", "PWM12"));
    }

    #[test]
    fn rules_classes_and_patterns() {
        let mut d = ProjectData::new("x");
        for n in ["GND", "VBAT", "SDA", "3V3"] {
            d.nets.nets.insert(
                n.into(),
                Net {
                    pads: vec![],
                    class: None,
                },
            );
        }
        let pro = r#"{
          "board": {"design_settings": {"rules": {
            "min_hole_to_hole": 0.25, "min_track_width": 0.2, "min_clearance": 0.0,
            "min_through_hole_diameter": 0.3, "min_via_annular_width": 0.1,
            "min_copper_edge_clearance": 0.3}}},
          "net_settings": {
            "classes": [
              {"name": "Default", "clearance": 0.15, "track_width": 0.25, "via_diameter": 0.6, "via_drill": 0.3},
              {"name": "Power", "clearance": 0.25, "track_width": 0.7, "via_diameter": 0.8, "via_drill": 0.4},
              {"name": "Old", "clearance": 0.2, "nets": ["3V3"]}
            ],
            "netclass_assignments": {"GND": "Power"},
            "netclass_patterns": [{"netclass": "Power", "pattern": "VB*"}, {"netclass": "Nope", "pattern": "SDA"}]
          }}"#;
        let notes = apply_project(&mut d, pro).unwrap();
        let r = &d.board.rules;
        assert_eq!(r.hole_to_hole, 0.25);
        assert_eq!(r.min_track_width, 0.2);
        assert_eq!(r.min_annular_ring, 0.1);
        assert_eq!(r.clearance, 0.15);
        assert_eq!(r.net_classes["Power"].track_width, Some(0.7));
        assert_eq!(d.nets.nets["GND"].class.as_deref(), Some("Power"));
        assert_eq!(d.nets.nets["VBAT"].class.as_deref(), Some("Power"));
        assert_eq!(d.nets.nets["3V3"].class.as_deref(), Some("Old"));
        assert_eq!(d.nets.nets["SDA"].class, None);
        assert!(notes[0].contains("2 net classes, 3 nets"), "{notes:?}");
    }
}

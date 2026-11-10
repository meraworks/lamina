//! Design rules and net classes from a `.kicad_pro` file. KiCad 6+ keeps
//! them there, not in the board file.

use lamina_model::*;
use serde_json::{Value, json};

fn r6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
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
    if let Some(x) = num(ds, "min_hole_clearance") {
        rules.hole_to_copper = r6(x);
    }
    if let Some(x) = num(ds, "min_text_height") {
        rules.min_silk_text_height = r6(x);
    }
    if let Some(x) = ds
        .get("min_resolved_spokes")
        .and_then(Value::as_u64)
        .filter(|x| *x <= 4)
    {
        rules.min_thermal_spokes = x as u8;
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
            reference_plane: false,
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
    // The board minimum of KiCad is a floor for the clearance.
    if let Some(x) = num(ds, "min_clearance") {
        rules.clearance = rules.clearance.max(r6(x));
    }

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
        .chain(r.custom.iter().filter_map(|c| c.clearance))
        .fold(r.clearance, f64::min);
    let least = |base: f64, pick: &dyn Fn(&CustomRule) -> Option<f64>| {
        r.custom.iter().filter_map(pick).fold(base, f64::min)
    };
    let min_track = least(r.min_track_width, &|c| c.min_track_width);
    let min_ring = least(r.min_annular_ring, &|c| c.min_annular_ring);
    let min_via_drill = least(r.min_via_drill, &|c| c.min_via_drill);
    let text = json!({
        "board": {"design_settings": {"rules": {
            "min_clearance": min_clearance,
            "min_track_width": min_track,
            "min_through_hole_diameter": r.min_drill.min(min_via_drill),
            "min_via_annular_width": min_ring,
            "min_via_diameter": min_via_drill + 2.0 * min_ring,
            "min_hole_to_hole": r.hole_to_hole,
            "min_hole_clearance": r.hole_to_copper,
            "min_text_height": r.min_silk_text_height,
            "min_resolved_spokes": r.min_thermal_spokes,
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

/// The custom rules of the board as a KiCad rules file (`.kicad_dru`).
/// The result is `None` if the board has no custom rule.
///
/// KiCad uses the last rule that matches, as Lamina does, so the rules keep
/// their order.
pub fn export_rules(d: &ProjectData) -> Option<String> {
    let r = &d.board.rules;
    if r.custom.is_empty() {
        return None;
    }
    let quote = |s: &str| s.replace(['\\', '\'', '"'], "");
    let mut out = String::from("(version 1)\n");
    // The project file has each minimum as the smallest value that a custom
    // rule permits. These first rules give the limits of the board. A custom
    // rule after them wins where it matches.
    let base = |v: f64| format!("{}mm", (v * 1e6).round() / 1e6);
    out.push_str(&format!(
        "\n(rule \"board limits\"\n  (constraint track_width (min {}))\n  (constraint annular_width (min {})))\n",
        base(r.min_track_width),
        base(r.min_annular_ring)
    ));
    out.push_str(&format!(
        "\n(rule \"board limits (via drill)\"\n  (condition \"A.Type == 'Via'\")\n  (constraint hole_size (min {})))\n",
        base(r.min_via_drill)
    ));
    for rule in &r.custom {
        let mut when: Vec<String> = Vec::new();
        if let Some(net) = &rule.when.net {
            when.push(format!("A.NetName == '{}'", quote(net)));
        }
        if let Some(class) = &rule.when.class {
            when.push(format!("A.NetClass == '{}'", quote(class)));
        }
        let layer = rule
            .when
            .layer
            .map(|l| format!("\n  (layer \"{}\")", crate::layers::layer_to_kicad(l)))
            .unwrap_or_default();
        let mm = |v: f64| format!("{}mm", (v * 1e6).round() / 1e6);
        let mut write = |suffix: &str, extra: Option<&str>, constraints: &[String]| {
            if constraints.is_empty() {
                return;
            }
            let mut all = when.clone();
            all.extend(extra.map(str::to_string));
            out.push_str(&format!("\n(rule \"{}{suffix}\"{layer}", quote(&rule.name)));
            if !all.is_empty() {
                out.push_str(&format!("\n  (condition \"{}\")", all.join(" && ")));
            }
            for c in constraints {
                out.push_str(&format!("\n  {c}"));
            }
            out.push_str(")\n");
        };
        let mut general = Vec::new();
        if let Some(v) = rule.clearance {
            general.push(format!("(constraint clearance (min {}))", mm(v)));
        }
        if let Some(v) = rule.min_track_width {
            general.push(format!("(constraint track_width (min {}))", mm(v)));
        }
        if let Some(v) = rule.min_annular_ring {
            general.push(format!("(constraint annular_width (min {}))", mm(v)));
        }
        write("", None, &general);
        // The via drill limit is only for vias.
        if let Some(v) = rule.min_via_drill {
            write(
                " (via drill)",
                Some("A.Type == 'Via'"),
                &[format!("(constraint hole_size (min {}))", mm(v))],
            );
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_rules_are_in_the_dru_file() {
        let mut d = ProjectData::new("rules");
        assert_eq!(export_rules(&d), None);
        d.board.rules.min_annular_ring = 0.15;
        d.board.rules.custom.push(CustomRule {
            name: "via-ring".into(),
            when: RuleMatch {
                net: Some("GND*".into()),
                class: None,
                layer: None,
                ..Default::default()
            },
            min_annular_ring: Some(0.1),
            min_via_drill: Some(0.2),
            ..Default::default()
        });
        d.board.rules.custom.push(CustomRule {
            name: "hv".into(),
            when: RuleMatch {
                net: None,
                class: Some("HV".into()),
                layer: Some(Layer::TOP),
                ..Default::default()
            },
            clearance: Some(0.6),
            ..Default::default()
        });
        let text = export_rules(&d).unwrap();
        assert!(text.starts_with("(version 1)\n"));
        assert!(text.contains(
            "(rule \"via-ring\"\n  (condition \"A.NetName == 'GND*'\")\n  (constraint annular_width (min 0.1mm)))"
        ), "{text}");
        assert!(text.contains(
            "(rule \"via-ring (via drill)\"\n  (condition \"A.NetName == 'GND*' && A.Type == 'Via'\")\n  (constraint hole_size (min 0.2mm)))"
        ), "{text}");
        assert!(text.contains(
            "(rule \"hv\"\n  (layer \"F.Cu\")\n  (condition \"A.NetClass == 'HV'\")\n  (constraint clearance (min 0.6mm)))"
        ), "{text}");
        assert!(text.find("via-ring").unwrap() < text.find("\"hv\"").unwrap());
        assert!(text.contains(
            "(rule \"board limits\"\n  (constraint track_width (min 0.15mm))\n  (constraint annular_width (min 0.15mm)))"
        ), "{text}");
        assert!(text.find("board limits").unwrap() < text.find("via-ring").unwrap());
        // The board minimum does not hide the custom rule.
        let value: Value = serde_json::from_str(&export_project(&d, "rules")).unwrap();
        assert_eq!(
            value["board"]["design_settings"]["rules"]["min_via_annular_width"],
            0.1
        );
    }

    #[test]
    fn exported_hole_clearance_uses_the_hole_rule() {
        let mut d = ProjectData::new("rules");
        d.board.rules.clearance = 0.3;
        d.board.rules.hole_to_copper = 0.127;
        let value: Value = serde_json::from_str(&export_project(&d, "rules")).unwrap();
        assert_eq!(
            value["board"]["design_settings"]["rules"]["min_hole_clearance"],
            0.127
        );
    }

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
                    ..Default::default()
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

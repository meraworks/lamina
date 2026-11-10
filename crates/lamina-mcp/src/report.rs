//! The output formats of `lamina check` for people and for CI programs.

use lamina_core::model::{Severity, Violation};
use serde_json::json;

pub const FORMATS: [&str; 4] = ["text", "json", "sarif", "junit"];

/// The violations in the format `format`. `board` is the path of the board.
pub fn render(v: &[Violation], format: &str, board: &str) -> Option<String> {
    match format {
        "json" => serde_json::to_string_pretty(v).ok(),
        "text" => Some(text(v)),
        "sarif" => serde_json::to_string_pretty(&sarif(v, board)).ok(),
        "junit" => Some(junit(v, board)),
        _ => None,
    }
}

fn level(v: &Violation) -> &'static str {
    if v.severity == Severity::Error {
        "error"
    } else {
        "warning"
    }
}

fn place(v: &Violation) -> String {
    let mut s = String::new();
    if let Some(at) = v.at {
        s = format!(" at ({:.3}, {:.3})", at.x, at.y);
    }
    if let Some(layer) = v.layer {
        s.push_str(&format!(" on {layer}"));
    }
    s
}

fn text(v: &[Violation]) -> String {
    let mut out = String::new();
    for x in v {
        out.push_str(&format!(
            "{}: {}: {}{}\n",
            level(x),
            x.rule,
            x.message,
            place(x)
        ));
    }
    let errors = v.iter().filter(|x| x.severity == Severity::Error).count();
    out.push_str(&format!(
        "{} error(s), {} warning(s)\n",
        errors,
        v.len() - errors
    ));
    out
}

fn sarif(v: &[Violation], board: &str) -> serde_json::Value {
    let mut ids: Vec<&str> = v.iter().map(|x| x.rule.as_str()).collect();
    ids.sort();
    ids.dedup();
    let rules: Vec<serde_json::Value> = ids
        .iter()
        .map(|id| {
            let info = lamina_drc_info(id);
            json!({
                "id": id,
                "shortDescription": { "text": info.0 },
                "help": { "text": info.1 },
            })
        })
        .collect();
    let results: Vec<serde_json::Value> = v
        .iter()
        .map(|x| {
            json!({
                "ruleId": x.rule,
                "level": level(x),
                "message": { "text": format!("{}{}", x.message, place(x)) },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": format!("{}/routing.json", board.replace('\\', "/")) }
                    },
                    "logicalLocations": x.items.iter().map(|i| json!({ "name": i })).collect::<Vec<_>>(),
                }],
                "partialFingerprints": { "laminaKey": x.key() },
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "lamina",
                "version": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/meraworks/lamina",
                "rules": rules,
            }},
            "results": results,
        }],
    })
}

/// The summary and the reason of a rule.
fn lamina_drc_info(id: &str) -> (String, String) {
    lamina_core::rule_info(id).unwrap_or((id.to_string(), String::new()))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// One test case for each violation. A clean board has one test case that passes.
fn junit(v: &[Violation], board: &str) -> String {
    let errors = v.iter().filter(|x| x.severity == Severity::Error).count();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<testsuite name=\"lamina check\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">\n",
        v.len().max(1),
        errors,
        v.len() - errors
    ));
    if v.is_empty() {
        out.push_str(&format!(
            "  <testcase classname=\"lamina\" name=\"{}\"/>\n",
            xml(board)
        ));
    }
    for x in v {
        let name = format!("{} {}", x.rule, x.items.join(" "));
        out.push_str(&format!(
            "  <testcase classname=\"lamina.{}\" name=\"{}\">\n",
            xml(&x.rule),
            xml(name.trim())
        ));
        let body = xml(&format!("{}{}", x.message, place(x)));
        if x.severity == Severity::Error {
            out.push_str(&format!(
                "    <failure message=\"{}\" type=\"{}\"/>\n",
                body,
                xml(&x.rule)
            ));
        } else {
            out.push_str(&format!("    <skipped message=\"{body}\"/>\n"));
        }
        out.push_str("  </testcase>\n");
    }
    out.push_str("</testsuite>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Violation> {
        vec![
            Violation::error("clearance", "A <is> near \"B\" & C").items(["t1".to_string()]),
            Violation::warning("silk_text_size", "The text is small").items(["x1".to_string()]),
        ]
    }

    #[test]
    fn each_format_renders_and_an_unknown_format_does_not() {
        for f in FORMATS {
            assert!(render(&sample(), f, "board").is_some(), "{f}");
        }
        assert!(render(&sample(), "yaml", "board").is_none());
    }

    #[test]
    fn text_has_one_line_for_each_violation_and_a_total() {
        let t = render(&sample(), "text", "b").unwrap();
        assert_eq!(t.lines().count(), 3);
        assert!(t.ends_with("1 error(s), 1 warning(s)\n"));
    }

    #[test]
    fn sarif_has_the_rules_and_the_results() {
        let s: serde_json::Value =
            serde_json::from_str(&render(&sample(), "sarif", "b").unwrap()).unwrap();
        assert_eq!(s["version"], "2.1.0");
        let run = &s["runs"][0];
        assert_eq!(run["tool"]["driver"]["rules"].as_array().unwrap().len(), 2);
        assert_eq!(run["results"][0]["level"], "error");
        assert_eq!(run["results"][1]["level"], "warning");
        assert!(
            !run["tool"]["driver"]["rules"][0]["shortDescription"]["text"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn junit_escapes_text_and_counts_failures() {
        let j = render(&sample(), "junit", "b").unwrap();
        assert!(j.contains("tests=\"2\" failures=\"1\" skipped=\"1\""));
        assert!(j.contains("A &lt;is&gt; near &quot;B&quot; &amp; C"));
        let clean = render(&[], "junit", "b").unwrap();
        assert!(clean.contains("tests=\"1\" failures=\"0\""));
    }
}

//! The last step of the analysis: exclusions and severities.

use lamina_model::*;

use crate::registry;

/// The violations, the excluded violations, and the violations of rules that
/// are off in design and on for release.
pub(crate) fn run(
    mut v: Vec<Violation>,
    rules: &Rules,
) -> (Vec<Violation>, Vec<Violation>, Vec<Violation>) {
    let mut excluded = Vec::new();
    let mut matched = vec![false; rules.exclusions.len()];
    v.retain(|violation| {
        let mut items = violation.items.clone();
        items.sort();
        let mut found = false;
        for (i, entry) in rules.exclusions.iter().enumerate() {
            let mut expected = entry.items.clone();
            expected.sort();
            if entry.rule == violation.rule && expected == items {
                matched[i] = true;
                found = true;
            }
        }
        if found {
            excluded.push(violation.clone());
        }
        !found
    });
    for (entry, matched) in rules.exclusions.iter().zip(matched) {
        if !matched {
            v.push(
                Violation::warning(
                    "stale_exclusion",
                    format!(
                        "The exclusion for {} matches no violation: {}",
                        entry.rule, entry.reason
                    ),
                )
                .items(entry.items.clone()),
            );
        }
    }
    let mut release_only = Vec::new();
    v.retain_mut(|violation| {
        match rules.severities.get(&violation.rule) {
            Some(RuleSeverity::Ignore) => return false,
            Some(RuleSeverity::Error) => violation.severity = Severity::Error,
            Some(RuleSeverity::Warning) => violation.severity = Severity::Warning,
            None => {
                // A rule that is off by default. It can be on for release.
                if let Some(c) = registry::get(&violation.rule) {
                    if c.default == RuleSeverity::Ignore {
                        match c.release {
                            Some(RuleSeverity::Error) => {
                                violation.severity = Severity::Error;
                                release_only.push(violation.clone());
                            }
                            Some(RuleSeverity::Warning) => {
                                violation.severity = Severity::Warning;
                                release_only.push(violation.clone());
                            }
                            _ => {}
                        }
                        return false;
                    }
                }
            }
        }
        true
    });
    (v, excluded, release_only)
}

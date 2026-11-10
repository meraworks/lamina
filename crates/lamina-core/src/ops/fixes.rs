//! Apply the fixes that the checks propose.

use std::collections::BTreeMap;

use crate::project::{OpResult, Project, Touched};
use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct ApplyFixes {
    /// Apply only the fixes of these rules. With no value, all rules.
    #[serde(default)]
    pub rules: Option<Vec<String>>,
    /// Apply only the fixes of violations that name one of these items.
    #[serde(default)]
    pub items: Option<Vec<String>>,
    /// Report the fixes and change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FixReport {
    pub rule: String,
    pub items: Vec<String>,
    pub fix: Fix,
    /// Why the fix was not applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FixesApplied {
    pub applied: Vec<FixReport>,
    pub refused: Vec<FixReport>,
    pub dry_run: bool,
    /// The result of the change. Absent for a dry run and when no fix applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<OpResult>,
}

/// Apply one fix to the data. The error is the reason that it cannot apply.
fn apply(d: &mut ProjectData, fix: &Fix) -> std::result::Result<(), String> {
    match fix {
        Fix::MoveText { id, to } => {
            let t = text(d, id)?;
            t.at = *to;
        }
        Fix::TurnText { id, by } => {
            let t = text(d, id)?;
            t.rotation = (t.rotation + by).rem_euclid(360.0);
        }
        Fix::SetTextSize { id, size } => {
            let t = text(d, id)?;
            t.size = *size;
        }
        Fix::SetViaSize {
            id,
            diameter,
            drill,
        } => {
            let v = d
                .routing
                .vias
                .iter_mut()
                .find(|v| &v.id == id)
                .ok_or(format!("The via {id} does not exist."))?;
            v.diameter = *diameter;
            v.drill = *drill;
        }
        Fix::SetTrackWidth { id, width } => {
            let t = d
                .routing
                .tracks
                .iter_mut()
                .find(|t| &t.id == id)
                .ok_or(format!("The track {id} does not exist."))?;
            t.width = *width;
        }
    }
    Ok(())
}

fn text<'a>(d: &'a mut ProjectData, id: &str) -> std::result::Result<&'a mut BoardText, String> {
    d.routing
        .texts
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or(format!("The text {id} does not exist."))
}

/// The errors of `after` that `before` does not have, with their count.
fn new_errors(before: &[Violation], after: &[Violation]) -> Vec<String> {
    let mut left: BTreeMap<String, usize> = BTreeMap::new();
    for v in before {
        *left.entry(v.key()).or_default() += 1;
    }
    after
        .iter()
        .filter(|v| match left.get_mut(&v.key()) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .filter(|v| v.severity == Severity::Error)
        .map(|v| format!("{}: {}", v.rule, v.message))
        .collect()
}

impl Project {
    pub fn apply_fixes(&mut self, p: ApplyFixes) -> Result<FixesApplied> {
        if let Some(rules) = &p.rules {
            for r in rules {
                if lamina_drc::registry::get(r).is_none() {
                    return Err(Error::invalid(format!("{r} is not a rule name.")));
                }
            }
        }
        let mut current = self.check_data(&self.data);
        let wanted: Vec<Violation> = current
            .iter()
            .filter(|v| !v.fix.is_empty())
            .filter(|v| p.rules.as_ref().is_none_or(|r| r.contains(&v.rule)))
            .filter(|v| {
                p.items
                    .as_ref()
                    .is_none_or(|i| v.items.iter().any(|x| i.contains(x)))
            })
            .cloned()
            .collect();
        let mut d = self.data.clone();
        let mut applied = Vec::new();
        let mut refused = Vec::new();
        for v in wanted {
            for fix in &v.fix {
                let mut report = FixReport {
                    rule: v.rule.clone(),
                    items: v.items.clone(),
                    fix: fix.clone(),
                    reason: None,
                };
                let mut next = d.clone();
                if let Err(why) = apply(&mut next, fix) {
                    report.reason = Some(why);
                    refused.push(report);
                    continue;
                }
                // Each fix is a strict change: it must add no error.
                let after = self.check_data(&next);
                let errors = new_errors(&current, &after);
                if !errors.is_empty() {
                    report.reason = Some(format!(
                        "The fix adds {} error(s): {}",
                        errors.len(),
                        errors
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                    refused.push(report);
                    continue;
                }
                if after.iter().any(|a| a.key() == v.key()) {
                    report.reason = Some("The violation stays after the fix.".into());
                    refused.push(report);
                    continue;
                }
                d = next;
                current = after;
                applied.push(report);
            }
        }
        let result = if p.dry_run || applied.is_empty() {
            None
        } else {
            let n = applied.len();
            Some(self.commit(
                d,
                Touched::file(FileKind::Routing),
                false,
                format!("Applied {n} fix(es)"),
            )?)
        };
        Ok(FixesApplied {
            applied,
            refused,
            dry_run: p.dry_run,
            result,
        })
    }
}

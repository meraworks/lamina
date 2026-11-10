//! Checks of the footprints in use: their evidence and their symmetry.

use std::collections::BTreeMap;

use lamina_model::*;

/// The state of the evidence of a footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceState {
    /// The current pads have a record that passed, or the IPC generator made them.
    Verified,
    /// The last record of the current pads is a fail.
    Mismatch,
    /// The records are for other pads: the footprint changed after the check.
    Stale,
    /// No record.
    Unverified,
}

pub fn evidence_state(fp: &Footprint) -> EvidenceState {
    match fp.current_evidence() {
        Some(e) if e.pass => EvidenceState::Verified,
        Some(_) => EvidenceState::Mismatch,
        None if !fp.evidence.is_empty() => EvidenceState::Stale,
        None if matches!(fp.provenance, Provenance::Ipc7351 { .. }) => EvidenceState::Verified,
        None => EvidenceState::Unverified,
    }
}

pub(crate) fn check(d: &ProjectData) -> Vec<Violation> {
    let mut used: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (r, part) in &d.parts.parts {
        used.entry(part.footprint.as_str())
            .or_default()
            .push(r.as_str());
    }
    let mut v = Vec::new();
    for (id, parts) in used {
        let Some(fp) = d.footprints.get(id) else {
            continue;
        };
        let who = parts.join(", ");
        match evidence_state(fp) {
            EvidenceState::Verified => {}
            EvidenceState::Mismatch => {
                let e = fp.current_evidence().expect("a record");
                v.push(
                    Violation::error(
                        "footprint_mismatch",
                        format!(
                            "The footprint {id} ({who}) does not agree with {}: {}",
                            e.document,
                            e.findings.join(" ")
                        ),
                    )
                    .items([id.to_string()]),
                );
            }
            EvidenceState::Stale => v.push(
                Violation::warning(
                    "footprint_stale",
                    format!(
                        "The pads of the footprint {id} ({who}) changed after its check. Verify it again"
                    ),
                )
                .items([id.to_string()]),
            ),
            EvidenceState::Unverified => {
                if fp
                    .source_hash
                    .as_ref()
                    .is_some_and(|h| *h != fp.pads_hash())
                {
                    v.push(
                        Violation::warning(
                            "footprint_changed",
                            format!(
                                "The pads of the footprint {id} ({who}) differ from its source, and no check covers them"
                            ),
                        )
                        .items([id.to_string()]),
                    );
                }
                v.push(
                    Violation::warning(
                        "footprint_unverified",
                        format!(
                            "The footprint {id} ({who}) has no evidence. Use `verify_footprint` or `audit_footprints`"
                        ),
                    )
                    .items([id.to_string()]),
                );
            }
        }
        let near = lamina_footprint::compare::near_symmetry(fp);
        if !near.is_empty() {
            let list: Vec<String> = near
                .iter()
                .take(6)
                .map(|(n, d)| format!("{n} ({d} mm)"))
                .collect();
            v.push(
                Violation::warning(
                    "footprint_symmetry",
                    format!(
                        "The footprint {id} ({who}) is almost symmetric: these pads are off their mirror position: {}",
                        list.join(", ")
                    ),
                )
                .items([id.to_string()]),
            );
        }
    }
    v
}

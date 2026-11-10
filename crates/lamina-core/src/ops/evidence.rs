//! Footprint evidence: compare a footprint with the numbers of a datasheet
//! or with a second source, and keep the result in the footprint file.

use lamina_footprint::compare::{Comparison, Tolerance, compare};
use lamina_footprint::{Density, Options, PackageSpec};
use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::{OpResult, Project, Touched};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CompareFootprints {
    /// The id of a footprint in the project.
    pub a: String,
    /// The id of a second footprint in the project.
    pub b: String,
}

/// The size of a lead of a through-hole part.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Lead {
    /// The pad number.
    pub pad: String,
    /// The largest dimension of the lead across the hole, in mm: the
    /// diameter of a round lead, the diagonal of a square or flat lead.
    pub size: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VerifyFootprint {
    /// The id of the footprint in the project.
    pub footprint: String,
    /// The datasheet or drawing that the numbers come from.
    #[serde(default)]
    pub document: Option<String>,
    #[serde(default)]
    pub page: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// The package dimensions of the datasheet. Lamina makes the IPC-7351
    /// land pattern from them and compares it with the footprint.
    #[serde(default)]
    pub package: Option<PackageSpec>,
    /// The land pattern that the datasheet recommends: each pad with its
    /// number, position, shape and drill. Lamina compares pad by pad.
    #[serde(default)]
    pub pads: Option<Vec<Pad>>,
    /// The lead sizes of a through-hole part. Each hole must be 0.15 mm or
    /// more larger than its lead.
    #[serde(default)]
    pub leads: Option<Vec<Lead>>,
    /// A reason to trust the footprint with no comparison.
    #[serde(default)]
    pub accept: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Verified {
    pub footprint: String,
    pub pass: bool,
    pub method: EvidenceMethod,
    /// The differences. Empty if the footprint agrees.
    pub findings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<Comparison>,
    pub result: OpResult,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AuditRow {
    pub footprint: String,
    /// The parts that use the footprint.
    pub parts: Vec<String>,
    /// `verified`, `differs`, `has_evidence` or `no_source`. Only `verified`
    /// stores evidence.
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct AuditFootprints {
    /// Get the second source of a part from its provider on the network, if
    /// it is not in the project cache or in the part library.
    #[serde(default)]
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Audit {
    pub rows: Vec<AuditRow>,
    pub result: OpResult,
}

/// Compare a footprint with a package pattern. The pattern has three
/// densities (least, nominal and most material). The footprint is good if it
/// agrees with one of them: a library can use any of the three. If it agrees
/// with none, the result is the comparison with the nominal pattern.
fn compare_band(
    fp: &Footprint,
    make: &dyn Fn(Density) -> std::result::Result<Footprint, String>,
) -> std::result::Result<Comparison, String> {
    let nominal = compare(fp, &make(Density::Nominal)?, &Tolerance::SOURCE);
    if nominal.same {
        return Ok(nominal);
    }
    for density in [Density::Most, Density::Least] {
        if let Ok(want) = make(density) {
            let c = compare(fp, &want, &Tolerance::SOURCE);
            if c.same {
                return Ok(c);
            }
        }
    }
    Ok(nominal)
}

/// The standard package of a footprint id, if the name has one: `R_0603`
/// for `R_0603_1608Metric`.
fn preset_of(id: &str) -> Option<String> {
    let parts: Vec<&str> = id.split('_').collect();
    for n in (1..=parts.len()).rev() {
        let name = parts[..n].join("_");
        if lamina_footprint::from_package(&name, &Options::default(), Some("preset")).is_ok() {
            return Some(name);
        }
    }
    None
}

impl Project {
    fn footprint(&self, id: &str) -> Result<&Footprint> {
        self.data.footprints.get(id).ok_or_else(|| {
            Error::invalid(format!(
                "There is no footprint `{id}` in the project. Call `list_footprints` for the ids."
            ))
        })
    }

    /// Compare the pads of two footprints of the project.
    pub fn compare_footprints(&self, p: CompareFootprints) -> Result<Comparison> {
        Ok(compare(
            self.footprint(&p.a)?,
            self.footprint(&p.b)?,
            &Tolerance::SOURCE,
        ))
    }

    fn record(
        &mut self,
        id: &str,
        method: EvidenceMethod,
        document: String,
        page: Option<String>,
        url: Option<String>,
        findings: Vec<String>,
        pass: bool,
    ) -> Result<OpResult> {
        let mut d = self.data.clone();
        let fp = d.footprints.get_mut(id).expect("the footprint exists");
        let hash = fp.pads_hash();
        // One record for each method and pad set: a new check replaces the old one.
        fp.evidence
            .retain(|e| !(e.pads_hash == hash && e.method == method));
        fp.evidence.push(Evidence {
            pads_hash: hash,
            method,
            document,
            page,
            url,
            checked: lamina_model::time::today(),
            pass,
            findings,
        });
        let mut touched = Touched::default();
        touched.footprints.insert(id.to_string());
        self.commit(d, touched, false, format!("evidence for {id}"))
    }

    /// Check a footprint against datasheet numbers and keep the result.
    pub fn verify_footprint(&mut self, p: VerifyFootprint) -> Result<Verified> {
        let fp = self.footprint(&p.footprint)?.clone();
        let given = [
            p.package.is_some(),
            p.pads.is_some(),
            p.leads.is_some(),
            p.accept.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        if given != 1 {
            return Err(Error::invalid(
                "Give one of `package`, `pads`, `leads` or `accept`.",
            ));
        }
        let document = p.document.clone().unwrap_or_default();
        if p.accept.is_none() && document.trim().is_empty() {
            return Err(Error::invalid(
                "Give `document`: the datasheet or drawing that the numbers come from.",
            ));
        }
        let mut findings = Vec::new();
        let mut comparison = None;
        let (method, doc) = if let Some(reason) = &p.accept {
            if reason.trim().is_empty() {
                return Err(Error::invalid("The reason in `accept` is empty."));
            }
            (EvidenceMethod::Accepted, reason.clone())
        } else if let Some(spec) = &p.package {
            let c = compare_band(&fp, &|density| {
                lamina_footprint::generate(
                    "datasheet",
                    "datasheet",
                    spec,
                    &Options {
                        density,
                        ..Options::default()
                    },
                )
            })
            .map_err(Error::invalid)?;
            findings = c.findings.clone();
            if c.same && (c.turn != 0.0 || c.mirrored) {
                // A turn is only a note for a package comparison.
            }
            if !c.same {
                comparison = Some(c);
            } else {
                findings.clear();
            }
            (EvidenceMethod::Package, document)
        } else if let Some(pads) = &p.pads {
            if pads.is_empty() {
                return Err(Error::invalid("`pads` is empty."));
            }
            let mut want = Footprint::new(
                "datasheet",
                Provenance::Manual {
                    note: "datasheet land pattern".into(),
                },
            );
            want.pads = pads.clone();
            let c = compare(&fp, &want, &Tolerance::EXACT);
            findings = c.findings.clone();
            if !c.same || c.turn != 0.0 {
                if c.same {
                    findings.push(format!(
                        "The footprint is turned by {} degrees against the datasheet pattern.",
                        c.turn
                    ));
                }
                comparison = Some(c);
            } else {
                findings.clear();
            }
            (EvidenceMethod::LandPattern, document)
        } else {
            for lead in p.leads.as_deref().unwrap_or_default() {
                let holes: Vec<f64> = fp
                    .pads
                    .iter()
                    .filter(|x| x.number == lead.pad)
                    .filter_map(|x| match &x.kind {
                        PadKind::Through { drill, .. } => Some(drill.min_size()),
                        PadKind::Smd => None,
                    })
                    .collect();
                if holes.is_empty() {
                    findings.push(format!("The pad {} has no hole.", lead.pad));
                }
                for hole in holes {
                    let slack = hole - lead.size;
                    if slack + 1e-9 < 0.15 {
                        findings.push(format!(
                            "The hole of pad {} is {hole} mm for a lead of {} mm. It must be {:.2} mm or more.",
                            lead.pad,
                            lead.size,
                            lead.size + 0.15
                        ));
                    }
                }
            }
            (EvidenceMethod::LandPattern, document)
        };
        let pass = findings.is_empty();
        let result = self.record(
            &p.footprint,
            method,
            doc,
            p.page,
            p.url,
            findings.clone(),
            pass,
        )?;
        Ok(Verified {
            footprint: p.footprint,
            pass,
            method,
            findings,
            comparison,
            result,
        })
    }

    /// For each footprint in use that has no evidence: compare it with the
    /// standard package of its name, and keep the result.
    pub fn audit_footprints(&mut self, p: AuditFootprints) -> Result<Audit> {
        let mut used: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for (r, part) in &self.data.parts.parts {
            used.entry(part.footprint.clone())
                .or_default()
                .push(r.clone());
        }
        let mut rows = Vec::new();
        let mut d = self.data.clone();
        let mut touched = Touched::default();
        for (id, parts) in used {
            let Some(fp) = d.footprints.get_mut(&id) else {
                continue;
            };
            let mut row = AuditRow {
                footprint: id.clone(),
                parts,
                result: "no_source".into(),
                source: None,
                findings: Vec::new(),
            };
            if fp.current_evidence().is_some()
                || matches!(fp.provenance, Provenance::Ipc7351 { .. })
            {
                row.result = "has_evidence".into();
                rows.push(row);
                continue;
            }
            if let Some(name) = preset_of(&id) {
                let c = compare_band(fp, &|density| {
                    lamina_footprint::from_package(
                        &name,
                        &Options {
                            density,
                            ..Options::default()
                        },
                        Some("preset"),
                    )
                })
                .map_err(Error::invalid)?;
                row.source = Some(format!("IPC-7351 package {name}"));
                // The package pattern is not the datasheet. A difference is a
                // reason to look at the datasheet, not a failed check, so only
                // a pass is stored.
                row.result = if c.same { "verified" } else { "differs" }.into();
                row.findings = if c.same { Vec::new() } else { c.findings };
                if c.same {
                    let hash = fp.pads_hash();
                    fp.evidence
                        .retain(|e| !(e.pads_hash == hash && e.method == EvidenceMethod::Preset));
                    fp.evidence.push(Evidence {
                        pads_hash: hash,
                        method: EvidenceMethod::Preset,
                        document: format!("IPC-7351 package {name}"),
                        page: None,
                        url: None,
                        checked: lamina_model::time::today(),
                        pass: true,
                        findings: Vec::new(),
                    });
                    touched.footprints.insert(id.clone());
                }
            }
            // A second source: the same part from its provider. A footprint
            // that came from that provider is not a second source.
            if row.result != "verified" && !matches!(fp.provenance, Provenance::Lcsc { .. }) {
                let numbers: std::collections::BTreeSet<&str> = row
                    .parts
                    .iter()
                    .filter_map(|r| self.data.parts.parts.get(r))
                    .filter_map(|part| part.lcsc.as_deref())
                    .collect();
                for number in numbers {
                    let Some(other) = self.source_footprint("lcsc", number, p.online) else {
                        continue;
                    };
                    let c = compare(fp, &other, &Tolerance::SOURCE);
                    row.source = Some(format!("lcsc:{number}"));
                    if c.same {
                        row.result = "verified".into();
                        row.findings = Vec::new();
                        if c.turn != 0.0 {
                            row.findings.push(format!(
                                "The footprint is turned by {} degrees against lcsc:{number}. If the assembler uses that library, set the field cpl_rotation of the parts.",
                                c.turn
                            ));
                        }
                        let hash = fp.pads_hash();
                        fp.evidence.push(Evidence {
                            pads_hash: hash,
                            method: EvidenceMethod::SecondSource,
                            document: format!("lcsc:{number}"),
                            page: None,
                            url: None,
                            checked: lamina_model::time::today(),
                            pass: true,
                            findings: Vec::new(),
                        });
                        touched.footprints.insert(id.clone());
                        break;
                    }
                    row.result = "differs".into();
                    row.findings = c.findings;
                }
            }
            rows.push(row);
        }
        let result = self.commit(d, touched, false, "footprint audit")?;
        Ok(Audit { rows, result })
    }
}

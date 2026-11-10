//! The review: one verdict for the board, with the evidence, and the
//! comparison of two boards.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::ops::FabCompat;
use crate::ops::status::FootprintEvidence;
use crate::project::Project;
use lamina_drc::registry::{self, Family};
use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct Review {
    /// Do not write `output/review.md`. Only return the result.
    #[serde(default)]
    pub no_file: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Ready,
    ReadyWithWarnings,
    NotReady,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FamilyCount {
    pub family: Family,
    pub errors: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FabVerdict {
    pub profile: String,
    pub tier: String,
    pub fits: bool,
    /// The limits that the board does not meet, as text.
    pub fails: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Reviewed {
    pub verdict: Verdict,
    /// The errors and warnings of the release check.
    pub errors: usize,
    pub warnings: usize,
    pub families: Vec<FamilyCount>,
    pub fabs: Vec<FabVerdict>,
    pub footprints: FootprintEvidence,
    /// The BOM lines with no part number.
    pub bom_lines_without_number: usize,
    /// The open notes from an import about data that was dropped.
    pub import_losses: Vec<String>,
    pub exclusions: Vec<Exclusion>,
    pub open_notes: Vec<String>,
    /// The parts with two or more pads and no pin names.
    pub parts_without_pin_names: Vec<String>,
    /// The first errors, as text.
    pub first_errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The images next to the file: the top side, the bottom side, and the
    /// layers with a marker for each violation.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

impl Project {
    pub fn review(&self, p: Review) -> Result<Reviewed> {
        let d = &self.data;
        let v = self.check_release();
        let errors = v.iter().filter(|x| x.severity == Severity::Error).count();
        let warnings = v.len() - errors;
        let mut by: Vec<FamilyCount> = Vec::new();
        for x in &v {
            let family = registry::get(&x.rule)
                .map(|c| c.family)
                .unwrap_or(Family::Model);
            let row = match by.iter_mut().find(|r| r.family == family) {
                Some(r) => r,
                None => {
                    by.push(FamilyCount {
                        family,
                        errors: 0,
                        warnings: 0,
                    });
                    by.last_mut().expect("just pushed")
                }
            };
            if x.severity == Severity::Error {
                row.errors += 1;
            } else {
                row.warnings += 1;
            }
        }
        let fabs = self
            .fab_compat(FabCompat { profiles: None })?
            .into_iter()
            .map(|f| FabVerdict {
                fails: f
                    .limits
                    .iter()
                    .filter(|l| !l.fits)
                    .map(|l| format!("{}: {} (limit {})", l.what, l.value, l.limit))
                    .collect(),
                profile: f.profile,
                tier: f.tier,
                fits: f.fits,
            })
            .collect();
        let import_losses: Vec<String> = d
            .notes
            .notes
            .iter()
            .filter(|n| !n.resolved && n.author == "import")
            .map(|n| format!("{}: {}", n.id, n.text))
            .collect();
        let open_notes: Vec<String> = d
            .notes
            .notes
            .iter()
            .filter(|n| !n.resolved && n.author != "import")
            .map(|n| format!("{}: {}", n.id, n.text))
            .collect();
        let parts_without_pin_names: Vec<String> = d
            .parts
            .parts
            .iter()
            .filter(|(_, part)| {
                d.footprints.get(&part.footprint).is_some_and(|fp| {
                    fp.pads.iter().filter(|x| !x.number.is_empty()).count() > 2
                        && fp.pads.iter().all(|x| x.name.is_none())
                })
            })
            .map(|(r, _)| r.clone())
            .collect();
        let verdict = if errors > 0 {
            Verdict::NotReady
        } else if warnings > 0 || !import_losses.is_empty() {
            Verdict::ReadyWithWarnings
        } else {
            Verdict::Ready
        };
        let mut r = Reviewed {
            verdict,
            errors,
            warnings,
            families: by,
            fabs,
            footprints: self.status().footprint_evidence,
            bom_lines_without_number: v.iter().filter(|x| x.rule == "bom_part_number").count(),
            import_losses,
            exclusions: d.board.rules.exclusions.clone(),
            open_notes,
            parts_without_pin_names,
            first_errors: v
                .iter()
                .filter(|x| x.severity == Severity::Error)
                .take(20)
                .map(|x| format!("{}: {}", x.rule, x.message))
                .collect(),
            file: None,
            images: Vec::new(),
        };
        if !p.no_file {
            let dir = self.root().join("output");
            std::fs::create_dir_all(&dir).map_err(|e| Error::Io {
                path: dir.clone(),
                source: e,
            })?;
            // The images, if the program has a renderer.
            if self.ports.renderer.is_some() {
                use crate::ops::{Render, RenderView};
                let views = [
                    ("review-top.png", RenderView::Top, None),
                    ("review-bottom.png", RenderView::Bottom, None),
                    (
                        "review-checks.png",
                        RenderView::Layers,
                        Some(vec![
                            "top".to_string(),
                            "bottom".to_string(),
                            "outline".to_string(),
                            "drc".to_string(),
                        ]),
                    ),
                ];
                for (name, view, layers) in views {
                    let image = self.render(Render {
                        view,
                        layers,
                        width: Some(1200),
                        height: Some(900),
                        name: Some(name.trim_end_matches(".png").to_string()),
                        ..Default::default()
                    });
                    if let Ok(image) = image {
                        if std::fs::write(dir.join(name), &image.png).is_ok() {
                            r.images.push(format!("output/{name}"));
                        }
                    }
                }
            }
            let path = dir.join("review.md");
            std::fs::write(&path, markdown(&d.board.name, &r, &v)).map_err(|e| Error::Io {
                path: path.clone(),
                source: e,
            })?;
            r.file = Some("output/review.md".into());
        }
        Ok(r)
    }
}

/// The review as a file for the person who places the order.
pub fn markdown(name: &str, r: &Reviewed, v: &[Violation]) -> String {
    let mut s = String::new();
    let verdict = match r.verdict {
        Verdict::Ready => "READY",
        Verdict::ReadyWithWarnings => "READY, WITH WARNINGS",
        Verdict::NotReady => "NOT READY",
    };
    let _ = writeln!(s, "# Review of {name}\n");
    let _ = writeln!(s, "**Verdict: {verdict}**\n");
    let _ = writeln!(
        s,
        "The release check has {} error(s) and {} warning(s).\n",
        r.errors, r.warnings
    );
    if !r.images.is_empty() {
        for image in &r.images {
            let file = image.trim_start_matches("output/");
            let title = match file {
                "review-top.png" => "The top side",
                "review-bottom.png" => "The bottom side",
                _ => "The layers, with a marker for each violation",
            };
            let _ = writeln!(s, "![{title}]({file})\n");
        }
    }
    let _ = writeln!(s, "## Release check\n");
    if r.families.is_empty() {
        let _ = writeln!(s, "No violations.\n");
    } else {
        let _ = writeln!(s, "| Family | Errors | Warnings |\n|---|---|---|");
        for f in &r.families {
            let _ = writeln!(
                s,
                "| {} | {} | {} |",
                format!("{:?}", f.family).to_lowercase(),
                f.errors,
                f.warnings
            );
        }
        let _ = writeln!(s);
        for x in v {
            let level = if x.severity == Severity::Error {
                "error"
            } else {
                "warning"
            };
            let _ = writeln!(s, "- {level}, `{}`: {}", x.rule, x.message);
        }
        let _ = writeln!(s);
    }
    let _ = writeln!(s, "## Fabricators\n");
    for f in &r.fabs {
        if f.fits {
            let _ = writeln!(s, "- {} ({}): the board fits.", f.profile, f.tier);
        } else {
            let _ = writeln!(
                s,
                "- {} ({}): the board does not fit. {}",
                f.profile,
                f.tier,
                f.fails.join("; ")
            );
        }
    }
    let e = &r.footprints;
    let _ = writeln!(s, "\n## Footprint evidence\n");
    let _ = writeln!(
        s,
        "- Verified: {}\n- Mismatch: {}\n- Stale: {}\n- No evidence: {}\n",
        e.verified, e.mismatch, e.stale, e.unverified
    );
    let _ = writeln!(s, "## BOM\n");
    let _ = writeln!(
        s,
        "BOM lines with no part number: {}.\n",
        r.bom_lines_without_number
    );
    let mut list = |title: &str, empty: &str, rows: &[String]| {
        let _ = writeln!(s, "## {title}\n");
        if rows.is_empty() {
            let _ = writeln!(s, "{empty}\n");
        } else {
            for row in rows {
                let _ = writeln!(s, "- {row}");
            }
            let _ = writeln!(s);
        }
    };
    list(
        "Data that an import dropped",
        "None is open.",
        &r.import_losses,
    );
    let exclusions: Vec<String> = r
        .exclusions
        .iter()
        .map(|x| format!("`{}` {}: {}", x.rule, x.items.join(", "), x.reason))
        .collect();
    list("Accepted violations", "None.", &exclusions);
    list("Open notes", "None.", &r.open_notes);
    list(
        "Parts with no pin names",
        "None. The pin checks see all parts.",
        &r.parts_without_pin_names,
    );
    s
}

/// What is different between two boards.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct BoardDiff {
    pub parts_added: Vec<String>,
    pub parts_removed: Vec<String>,
    /// Parts with a different value, footprint or part number.
    pub parts_changed: Vec<String>,
    pub parts_moved: Vec<String>,
    pub nets_added: Vec<String>,
    pub nets_removed: Vec<String>,
    /// Nets with different pads.
    pub nets_changed: Vec<String>,
    pub rules_changed: bool,
    pub outline_changed: bool,
    /// Tracks, vias and zones: the count in the first and in the second board.
    pub tracks: (usize, usize),
    pub vias: (usize, usize),
    pub zones: (usize, usize),
    pub violations_added: Vec<String>,
    pub violations_fixed: Vec<String>,
}

impl BoardDiff {
    /// True if the two boards are the same for this comparison.
    pub fn is_empty(&self) -> bool {
        self.parts_added.is_empty()
            && self.parts_removed.is_empty()
            && self.parts_changed.is_empty()
            && self.parts_moved.is_empty()
            && self.nets_added.is_empty()
            && self.nets_removed.is_empty()
            && self.nets_changed.is_empty()
            && !self.rules_changed
            && !self.outline_changed
            && self.tracks.0 == self.tracks.1
            && self.vias.0 == self.vias.1
            && self.zones.0 == self.zones.1
            && self.violations_added.is_empty()
            && self.violations_fixed.is_empty()
    }
}

/// True if the board `b` has an error that the board `a` does not have.
pub fn adds_error(a: &ProjectData, b: &ProjectData) -> bool {
    let before: BTreeSet<String> = lamina_drc::analyze(a)
        .violations
        .iter()
        .map(|v| v.key())
        .collect();
    lamina_drc::analyze(b)
        .violations
        .iter()
        .any(|v| v.severity == Severity::Error && !before.contains(&v.key()))
}

/// Compare the board `a` (before) with the board `b` (after).
pub fn diff(a: &ProjectData, b: &ProjectData) -> BoardDiff {
    let mut out = BoardDiff::default();
    for (r, part) in &b.parts.parts {
        match a.parts.parts.get(r) {
            None => out.parts_added.push(r.clone()),
            Some(old) if old != part => out.parts_changed.push(r.clone()),
            _ => {}
        }
    }
    for r in a.parts.parts.keys() {
        if !b.parts.parts.contains_key(r) {
            out.parts_removed.push(r.clone());
        }
    }
    for (r, pl) in &b.placement.placements {
        if a.placement.placements.get(r).is_some_and(|old| old != pl) {
            out.parts_moved.push(r.clone());
        }
    }
    for (n, net) in &b.nets.nets {
        match a.nets.nets.get(n) {
            None => out.nets_added.push(n.clone()),
            Some(old) => {
                let x: BTreeSet<String> = old.pads.iter().map(|p| p.to_string()).collect();
                let y: BTreeSet<String> = net.pads.iter().map(|p| p.to_string()).collect();
                if x != y {
                    out.nets_changed.push(n.clone());
                }
            }
        }
    }
    for n in a.nets.nets.keys() {
        if !b.nets.nets.contains_key(n) {
            out.nets_removed.push(n.clone());
        }
    }
    out.rules_changed = a.board.rules != b.board.rules;
    out.outline_changed = a.board.outline != b.board.outline || a.board.holes != b.board.holes;
    out.tracks = (a.routing.tracks.len(), b.routing.tracks.len());
    out.vias = (a.routing.vias.len(), b.routing.vias.len());
    out.zones = (a.routing.zones.len(), b.routing.zones.len());

    let va = lamina_drc::analyze(a).violations;
    let vb = lamina_drc::analyze(b).violations;
    let count = |v: &[Violation]| {
        let mut m: BTreeMap<String, (usize, String)> = BTreeMap::new();
        for x in v {
            let e = m
                .entry(x.key())
                .or_insert((0, format!("{}: {}", x.rule, x.message)));
            e.0 += 1;
        }
        m
    };
    let (ca, cb) = (count(&va), count(&vb));
    for (k, (n, text)) in &cb {
        let before = ca.get(k).map(|x| x.0).unwrap_or(0);
        for _ in before..*n {
            out.violations_added.push(text.clone());
        }
    }
    for (k, (n, text)) in &ca {
        let after = cb.get(k).map(|x| x.0).unwrap_or(0);
        for _ in after..*n {
            out.violations_fixed.push(text.clone());
        }
    }
    out
}

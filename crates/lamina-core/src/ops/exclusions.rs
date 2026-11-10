use crate::project::{OpResult, Project, Touched};
use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ExcludeViolation {
    pub rule: String,
    pub items: Vec<String>,
    /// Why this violation is accepted. Must not be blank.
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct IncludeViolation {
    pub rule: String,
    pub items: Vec<String>,
}

impl Project {
    pub fn excluded_violations(&self) -> Vec<Violation> {
        lamina_drc::analyze(&self.data).excluded
    }

    pub fn exclude_violation(&mut self, p: ExcludeViolation) -> Result<OpResult> {
        if !lamina_drc::registry::get(&p.rule).is_some_and(|c| c.excludable)
            || p.rule == "stale_exclusion"
        {
            return Err(Error::invalid(format!(
                "Cannot exclude {}. Use a DRC rule name from check.",
                p.rule
            )));
        }
        let entry = Exclusion::new(p.rule, p.items, p.reason).map_err(Error::invalid)?;
        let mut d = self.data.clone();
        d.board.rules.exclusions.retain(|e| {
            let mut items = e.items.clone();
            items.sort();
            e.rule != entry.rule || items != entry.items
        });
        d.board.rules.exclusions.push(entry);
        self.commit(
            d,
            Touched::file(FileKind::Board),
            false,
            "violation excluded",
        )
    }

    pub fn include_violation(&mut self, mut p: IncludeViolation) -> Result<OpResult> {
        p.items.sort();
        let mut d = self.data.clone();
        d.board.rules.exclusions.retain(|e| {
            let mut items = e.items.clone();
            items.sort();
            e.rule != p.rule || items != p.items
        });
        self.commit(
            d,
            Touched::file(FileKind::Board),
            false,
            "violation included",
        )
    }
}

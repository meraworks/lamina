use crate::project::{OpResult, Project, Touched};
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SetDiffPairs {
    /// Replace all named pairs. Each has positive/negative net names, edge gap
    /// and track width in mm. An empty map removes all pair definitions.
    pub pairs: BTreeMap<String, DiffPair>,
}

pub(crate) fn check_pairs(nets: &NetsFile) -> Result<(), Error> {
    let mut used = BTreeSet::new();
    for (name, pair) in &nets.diff_pairs {
        if name.trim().is_empty() || pair.positive == pair.negative {
            return Err(Error::invalid(
                "A pair needs a non-empty name and two distinct nets.",
            ));
        }
        for net in [&pair.positive, &pair.negative] {
            if !nets.nets.contains_key(net) {
                return Err(Error::invalid(format!(
                    "Pair {name}: net `{net}` does not exist. Remove the pair or add its net."
                )));
            }
            if !used.insert(net) {
                return Err(Error::invalid(format!(
                    "Net `{net}` belongs to more than one pair."
                )));
            }
        }
        if !pair.gap.is_finite() || pair.gap <= 0.0 || !pair.width.is_finite() || pair.width <= 0.0
        {
            return Err(Error::invalid(format!(
                "Pair {name}: gap and width must be finite and more than 0 mm."
            )));
        }
    }
    Ok(())
}

impl Project {
    pub fn set_diff_pairs(&mut self, p: SetDiffPairs) -> Result<OpResult, Error> {
        let mut d = self.data.clone();
        d.nets.diff_pairs = p.pairs;
        check_pairs(&d.nets)?;
        self.commit(
            d,
            Touched::file(FileKind::Nets),
            false,
            "Differential pairs set",
        )
    }
}

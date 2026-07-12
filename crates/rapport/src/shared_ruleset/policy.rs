//! Context-facing shared Ruleset projection.

use super::boundary;
use super::catalog::Catalog;
use super::repository::Store;
use super::{Error, RulesetId};
use rapport_files::{FileSystem, Utf8Path};
use sha2::{Digest, Sha256};

pub(crate) struct SharedRulesets {
    summaries: Vec<SharedRulesetSummary>,
}

impl SharedRulesets {
    pub(crate) fn load(fs: &mut impl FileSystem, repo_root: &Utf8Path) -> Result<Self, Error> {
        let catalog = Catalog::load()?;
        let snapshot = Store::new(fs, repo_root, &catalog).load()?;
        let mut summaries = Vec::new();
        for stored in snapshot.entries() {
            let closure = snapshot.closure(stored.ruleset().id())?;
            let rules = snapshot.resolved_rules(stored.ruleset().id())?;
            let contents = boundary::render(stored.ruleset())?;
            let digest = format!("{:x}", Sha256::digest(contents.as_bytes()));
            summaries.push(SharedRulesetSummary {
                id: stored.ruleset().id().clone(),
                purpose: stored.ruleset().purpose().to_owned(),
                source: stored.source().to_string(),
                transitive: closure
                    .into_iter()
                    .filter(|candidate| candidate.ruleset().id() != stored.ruleset().id())
                    .map(|candidate| candidate.ruleset().id().clone())
                    .collect(),
                rule_count: rules.len(),
                digest,
            });
        }
        summaries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(Self { summaries })
    }

    pub(crate) fn get(&self, id: &RulesetId) -> Option<&SharedRulesetSummary> {
        self.summaries.iter().find(|summary| &summary.id == id)
    }

    pub(crate) fn require(&self, id: &RulesetId) -> Result<&SharedRulesetSummary, Error> {
        self.get(id)
            .ok_or_else(|| Error::UnknownRuleset(id.to_string()))
    }
}

impl std::fmt::Debug for SharedRulesets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedRulesets")
            .field("summary_count", &self.summaries.len())
            .finish()
    }
}

pub(crate) struct SharedRulesetSummary {
    id: RulesetId,
    purpose: String,
    source: String,
    transitive: Vec<RulesetId>,
    rule_count: usize,
    digest: String,
}

impl SharedRulesetSummary {
    pub(crate) fn id(&self) -> &RulesetId {
        &self.id
    }

    pub(crate) fn purpose(&self) -> &str {
        &self.purpose
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn transitive(&self) -> &[RulesetId] {
        &self.transitive
    }

    pub(crate) fn rule_count(&self) -> usize {
        self.rule_count
    }

    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
}

impl std::fmt::Debug for SharedRulesetSummary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedRulesetSummary")
            .field("id", &self.id)
            .field("purpose_length", &self.purpose.len())
            .field("source", &self.source)
            .field("transitive_count", &self.transitive.len())
            .field("rule_count", &self.rule_count)
            .field("digest", &"[redacted]")
            .finish()
    }
}

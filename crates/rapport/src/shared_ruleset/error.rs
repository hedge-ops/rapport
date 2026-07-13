//! Ruleset failures.
//!
//! This module owns the primary failure contract shared by catalog, repository, domain, and command boundaries.

use rapport_files::Utf8PathBuf;
use std::io;

/// A failure to validate, resolve, or persist a shared Ruleset.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid Ruleset ID; use uppercase words separated by single underscores")]
    InvalidRulesetId,
    #[error("invalid Rule ID for Ruleset `{ruleset}`; use `{ruleset}_NNN`")]
    InvalidRuleId { ruleset: String },
    #[error("{field} must not be empty")]
    EmptyText { field: &'static str },
    #[error("unsupported example language; use a documented Phase 1 language")]
    UnsupportedLanguage,
    #[error("reference must use Markdown-link syntax, such as `[label](target)`")]
    InvalidReference,
    #[error("Ruleset `{0}` was not found")]
    UnknownRuleset(String),
    #[error("Rule `{0}` was not found")]
    UnknownRule(String),
    #[error("Ruleset `{0}` already exists")]
    DuplicateRuleset(String),
    #[error("Rule `{0}` already exists")]
    DuplicateRule(String),
    #[error("Ruleset `{owner}` includes missing Ruleset `{included}`")]
    MissingInclude { owner: String, included: String },
    #[error("Ruleset composition cycle: {}", .0.join(" -> "))]
    IncludeCycle(Vec<String>),
    #[error("Rule `{rule}` conflicts between Rulesets `{first}` and `{second}`")]
    ConflictingRule {
        rule: String,
        first: String,
        second: String,
    },
    #[error("Ruleset `{owner}` still composes `{used}`")]
    RulesetUses { owner: String, used: String },
    #[error("Context `{path}` still composes Ruleset `{ruleset}`")]
    ContextUses { path: Utf8PathBuf, ruleset: String },
    #[error("catalog-owned Ruleset `{0}` cannot be changed as repository content")]
    CatalogOwned(String),
    #[error("catalog Ruleset `{0}` is not installed")]
    NotInstalled(String),
    #[error("installed catalog Ruleset `{0}` has local modifications")]
    ModifiedCatalogRuleset(String),
    #[error("catalog Ruleset `{0}` has an invalid or duplicate lock entry")]
    InvalidCatalogLock(String),
    #[error("Ruleset file `{0}` already exists without a matching catalog lock")]
    FileConflict(Utf8PathBuf),
    #[error("unsupported Ruleset schema version `{version}` in `{path}`")]
    SchemaVersion { path: Utf8PathBuf, version: u16 },
    #[error("unsupported Ruleset lock version `{0}`")]
    LockVersion(u16),
    #[error("could not read or write `{path}`")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not decode `{path}`: {source}")]
    Decode {
        path: Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not encode Ruleset data")]
    Encode(#[source] toml_edit::ser::Error),
}

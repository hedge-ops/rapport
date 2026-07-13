//! Context policy failures.
//!
//! This module owns the primary failure contract for Context domain, persistence, workflows, and commands.

use rapport_files::Utf8PathBuf;
use std::io;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Ruleset(#[from] crate::shared_ruleset::Error),
    #[error("Context path must remain inside the repository")]
    InvalidPath,
    #[error("Context ID is not canonical")]
    InvalidContextId,
    #[error("Context entry ID is invalid for `{0}`")]
    InvalidEntryId(String),
    #[error("Context text must not be empty")]
    EmptyText,
    #[error("Context `{0}` already exists")]
    DuplicateContext(String),
    #[error("no Context governs `{0}`")]
    MissingContext(Utf8PathBuf),
    #[error("Context entry `{0}` was not found")]
    MissingEntry(String),
    #[error("Context `{context}` references unknown neighboring owner `{owner}`")]
    UnknownBoundaryOwner { context: String, owner: String },
    #[error("minimum Review grade `{requested}` cannot lower inherited grade `{inherited}`")]
    LowerReviewGrade {
        requested: String,
        inherited: String,
    },
    #[error("Review grade must be A+ through D- or F")]
    InvalidGrade,
    #[error("Just target is invalid or unavailable from the Context directory")]
    InvalidTarget,
    #[error("signoff `{0}` already exists")]
    DuplicateSignoff(String),
    #[error("signoff `{0}` was not found")]
    MissingSignoff(String),
    #[error("machine resource group is invalid")]
    InvalidResourceGroup,
    #[error("included signoff path is duplicate, equivalent, or outside the repository")]
    InvalidIncludedPath,
    #[error("generated signoff workflow `{0}` is missing or drifted")]
    WorkflowDrift(Utf8PathBuf),
    #[error("unsupported Context schema version `{version}` in `{path}`")]
    SchemaVersion { path: Utf8PathBuf, version: u16 },
    #[error("could not read or write `{path}`")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not decode `{path}`")]
    Decode {
        path: Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not encode Context data")]
    Encode(#[source] toml_edit::ser::Error),
}

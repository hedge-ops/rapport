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
    #[error("`{path}` must declare exactly one of `namespace` or legacy `id`")]
    SchemaIdentity { path: Utf8PathBuf },
    #[error("Context ID is not canonical")]
    InvalidContextId,
    #[error("Context entry ID is invalid for `{0}`")]
    InvalidEntryId(String),
    #[error("Context text must not be empty")]
    EmptyText,
    #[error("Context `{0}` already exists")]
    DuplicateContext(String),
    #[error(
        "no Context governs `{0}`; create context.toml with namespace and purpose, or run `rapport context init <path> --purpose <text>`"
    )]
    MissingContext(Utf8PathBuf),
    #[error(
        "`{path}` includes unresolved standards pack `{included}`; install it with `rapport ruleset catalog install {included}` or define it under .rapport/rules"
    )]
    UnresolvedInclude { path: Utf8PathBuf, included: String },
    #[error("Context entry `{0}` was not found")]
    MissingEntry(String),
    #[error("Context `{context}` references unknown neighboring owner `{owner}`")]
    UnknownBoundaryOwner { context: String, owner: String },
    #[error(
        "lifecycle field `{field}` in `{path}` is no longer supported; remove it and move build/acceptance policy to repository tooling (see docs/lifecycle-migration.md)"
    )]
    LifecycleField {
        path: Utf8PathBuf,
        field: &'static str,
    },
    #[error("unsupported Context schema version `{version}` in `{path}`")]
    SchemaVersion { path: Utf8PathBuf, version: u16 },
    #[error(
        "legacy Context schema in `{path}`; migrate every legacy `context.toml` to named architecture and rule declarations (`rule_includes`, array ownership/boundaries, `kind` signoffs, and `[[rules]]` are unsupported)"
    )]
    LegacySchema { path: Utf8PathBuf },
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
    #[error("could not encode Context data")]
    Encode(#[source] toml_edit::ser::Error),
}

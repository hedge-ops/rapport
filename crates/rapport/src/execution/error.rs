//! Failures at component execution boundaries.

use rapport_files::Utf8PathBuf;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Context(#[from] crate::policy_context::Error),
    #[error(transparent)]
    Graph(#[from] super::graph::Error),
    #[error("unsupported component operation: `{component}` type `{kind}`")]
    UnsupportedComponent {
        component: Utf8PathBuf,
        kind: String,
    },
    #[error("unsupported generator `{tool}` target `{target}`")]
    UnsupportedGenerator { tool: String, target: String },
    #[error("missing execution.generators.{output} in `{component}/context.toml`")]
    MissingGenerator {
        component: Utf8PathBuf,
        output: String,
    },
    #[error("invalid execution declaration: {0}")]
    Declaration(String),
    #[error("path must remain inside repository: `{0}`")]
    Path(Utf8PathBuf),
    #[error("operation requires {required}; current host is {actual}")]
    Platform {
        required: &'static str,
        actual: &'static str,
    },
    #[error("cannot read or write `{path}`: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid TOML in `{path}`: {source}")]
    Toml {
        path: Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("cannot execute `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("`{program}` failed with exit code {code:?}")]
    Failed { program: String, code: Option<i32> },
    #[error("generation did not produce required output `{0}`")]
    MissingOutput(Utf8PathBuf),
    #[error(
        "committed generated output is stale: `{0}`; run explicit generation and review the change"
    )]
    Stale(String),
    #[error("invalid input glob: {0}")]
    Pattern(#[from] glob::PatternError),
    #[error("cannot enumerate generation inputs: {0}")]
    Glob(#[from] glob::GlobError),
    #[error("cannot serialize generation receipt: {0}")]
    Json(#[from] serde_json::Error),
}

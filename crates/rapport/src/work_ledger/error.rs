//! Work ledger failures.

use rapport_files::Utf8PathBuf;
use std::io;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("active Work already exists")]
    ActiveWorkExists,
    #[error("no active Work exists")]
    MissingWork,
    #[error("exactly one ticket, plan, or ad-hoc request is required")]
    InvalidRequestSource,
    #[error("Work fields must not be empty")]
    EmptyField,
    #[error("the plan must be a repository-relative file")]
    InvalidPlan,
    #[error("Work requires a clean worktree")]
    DirtyWorktree,
    #[error("Work requires a checked-out source branch")]
    DetachedHead,
    #[error("source branch and target branch must be different")]
    SourceIsTarget,
    #[error("Work source branch is `{expected}`, but the worktree is on `{actual}`")]
    SourceBranchChanged { expected: String, actual: String },
    #[error("Git operation is already active")]
    SourceOperationActive,
    #[error("Task ID space is exhausted")]
    TaskIdExhausted,
    #[error("Task `{0}` was not found")]
    MissingTask(String),
    #[error("Task `{0}` is not a pending Develop Action Task")]
    TaskNotPending(String),
    #[error("Task `{0}` is not a running Develop Action Task")]
    TaskNotRunning(String),
    #[error("another Develop Action Task is already running")]
    DevelopTaskRunning,
    #[error("Work cannot complete while Develop is incomplete")]
    DevelopIncomplete,
    #[error("acceptance Build requires Develop to be complete")]
    BuildDevelopIncomplete,
    #[error("Work cannot complete while Build proof is missing or stale")]
    BuildIncomplete,
    #[error("Build target `{0}` is interactive or long-running")]
    InteractiveBuildTarget(String),
    #[error("Build path must stay inside the repository")]
    InvalidBuildPath,
    #[error("Build operation `{0}` could not run")]
    BuildExecution(String),
    #[error("Build Task `{0}` failed; inspect it with `rapport build status {0}`")]
    BuildFailed(String),
    #[error("ad hoc Build failed; no proof was recorded")]
    AdHocBuildFailed,
    #[error("acceptance Review requires current Develop and Build proof")]
    ReviewPrerequisite,
    #[error("another Review Task is already running or awaiting decision")]
    ActiveReview,
    #[error("no current Review Task is awaiting this operation")]
    MissingReview,
    #[error("review result is invalid: {0}")]
    InvalidReviewResult(String),
    #[error("Review path must stay inside the repository")]
    InvalidReviewPath,
    #[error("review input changed; cancel or restart Review")]
    StaleReview,
    #[error("review finding `{0}` was not found or is already reconciled")]
    MissingFinding(String),
    #[error("quality override is unavailable for this Review result")]
    OverrideUnavailable,
    #[error("Work cannot complete while Review proof is missing or stale")]
    ReviewIncomplete,
    #[error("could not decode review result `{path}`")]
    ReviewDecode {
        path: Utf8PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("a title or description update is required")]
    EmptyTaskUpdate,
    #[error("the before/after position must name a Develop Action Task")]
    InvalidTaskPosition,
    #[error("Task filter `{0}` is invalid")]
    InvalidTaskFilter(String),
    #[error("another {0} Task is already active")]
    ActiveTask(String),
    #[error("checkpoint has no staged changes")]
    EmptyCheckpoint,
    #[error(
        "source HEAD does not descend from the latest Work checkpoint; restore an unambiguous descendant or abandon and restart Work"
    )]
    AmbiguousCheckpoint,
    #[error("files changed after checkpoint start: {0}")]
    ConcurrentChanges(String),
    #[error("Work cannot end while nonterminal Tasks remain")]
    NonterminalTasks,
    #[error("Work cannot end before source HEAD equals its latest checkpoint")]
    UncheckpointedHead,
    #[error("the global Work history directory is unavailable")]
    MissingHistoryDirectory,
    #[error("path is not valid UTF-8")]
    NonUtf8Path,
    #[error("could not access `{path}`")]
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
    #[error("unsupported {kind} schema version in `{path}`")]
    SchemaVersion {
        kind: &'static str,
        path: Utf8PathBuf,
    },
    #[error("could not encode Work state")]
    Encode(#[from] toml::ser::Error),
    #[error(transparent)]
    Git(#[from] rapport_git::GitError),
    #[error(transparent)]
    Revision(#[from] rapport_git::InvalidRevision),
    #[error(transparent)]
    Context(#[from] crate::policy_context::Error),
}

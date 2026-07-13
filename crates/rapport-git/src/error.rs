//! Failures produced while invoking or interpreting Git.
//!
//! This module owns the crate's primary error and the output-parsing helpers
//! that consistently translate command failures into it.

use rapport_command::CommandOutcome;
use rapport_files::Utf8PathBuf;
use std::collections::BTreeSet;
use std::io;
use std::str::Utf8Error;

/// A revision that is unsafe or ambiguous as a command argument.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Git revision: {0:?}")]
pub struct InvalidRevision(String);

impl InvalidRevision {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }
}

/// A failure while invoking or interpreting Git.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error(transparent)]
    InvalidRevision(#[from] InvalidRevision),
    #[error("could not {operation}: {source}")]
    Invocation {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("could not {operation}: Git exited {exit_code:?}: {}", stderr.trim())]
    CommandFailed {
        operation: &'static str,
        exit_code: Option<i32>,
        stderr: String,
    },
    #[error("could not {operation}: Git returned non-UTF-8 data: {source}")]
    InvalidUtf8 {
        operation: &'static str,
        #[source]
        source: Utf8Error,
    },
    #[error("could not {0}: Git returned no output")]
    MissingOutput(&'static str),
    #[error("Git returned an invalid object identifier: {0:?}")]
    InvalidObjectId(String),
}

pub(crate) fn command_failed(operation: &'static str, outcome: &CommandOutcome) -> GitError {
    GitError::CommandFailed {
        operation,
        exit_code: outcome.exit_code(),
        stderr: outcome.stderr_lossy(),
    }
}

pub(crate) fn single_line(
    outcome: &CommandOutcome,
    operation: &'static str,
) -> Result<String, GitError> {
    let value = std::str::from_utf8(outcome.stdout())
        .map_err(|source| GitError::InvalidUtf8 { operation, source })?
        .trim();
    if value.is_empty() {
        Err(GitError::MissingOutput(operation))
    } else {
        Ok(value.to_owned())
    }
}

pub(crate) fn zero_delimited_paths(
    output: &[u8],
    operation: &'static str,
) -> Result<BTreeSet<Utf8PathBuf>, GitError> {
    output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(Utf8PathBuf::from)
                .map_err(|source| GitError::InvalidUtf8 { operation, source })
        })
        .collect()
}

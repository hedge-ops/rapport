//! Failures produced while invoking or interpreting Git.
//!
//! This module owns the crate's primary error and the output-parsing helpers
//! that consistently translate command failures into it.

use crate::InvalidRevision;
use rapport_command::CommandOutcome;
use rapport_files::Utf8PathBuf;
use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::str::Utf8Error;

/// A failure while invoking or interpreting Git.
#[derive(Debug)]
pub enum GitError {
    InvalidRevision(InvalidRevision),
    Invocation {
        operation: &'static str,
        source: io::Error,
    },
    CommandFailed {
        operation: &'static str,
        exit_code: Option<i32>,
        stderr: String,
    },
    InvalidUtf8 {
        operation: &'static str,
        source: Utf8Error,
    },
    MissingOutput(&'static str),
    InvalidObjectId(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRevision(source) => source.fmt(formatter),
            Self::Invocation { operation, source } => {
                write!(formatter, "could not {operation}: {source}")
            }
            Self::CommandFailed {
                operation,
                exit_code,
                stderr,
            } => write!(
                formatter,
                "could not {operation}: Git exited {exit_code:?}: {}",
                stderr.trim()
            ),
            Self::InvalidUtf8 { operation, source } => {
                write!(
                    formatter,
                    "could not {operation}: Git returned non-UTF-8 data: {source}"
                )
            }
            Self::MissingOutput(operation) => {
                write!(formatter, "could not {operation}: Git returned no output")
            }
            Self::InvalidObjectId(value) => {
                write!(
                    formatter,
                    "Git returned an invalid object identifier: {value:?}"
                )
            }
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRevision(source) => Some(source),
            Self::Invocation { source, .. } => Some(source),
            Self::InvalidUtf8 { source, .. } => Some(source),
            Self::CommandFailed { .. } | Self::MissingOutput(_) | Self::InvalidObjectId(_) => None,
        }
    }
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

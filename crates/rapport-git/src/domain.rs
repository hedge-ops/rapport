//! Validated values returned to and accepted from Git operations.
//!
//! This module owns repository, revision, object, worktree, and operation state;
//! command execution remains a boundary concern.

use crate::GitError;
use rapport_files::{Utf8Path, Utf8PathBuf};
use std::collections::BTreeSet;
use std::fmt;

/// A discovered Git worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub(crate) root: Utf8PathBuf,
}

impl Repository {
    #[must_use]
    pub fn root(&self) -> &Utf8Path {
        &self.root
    }
}

/// A Git revision supplied by a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision(String);

impl Revision {
    /// Validate a revision before passing it to Git.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidRevision`] when the revision is empty, begins with `-`,
    /// or contains whitespace or a NUL character.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidRevision> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('-')
            || value.chars().any(char::is_whitespace)
            || value.contains('\0')
        {
            return Err(InvalidRevision(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A revision that is unsafe or ambiguous as a command argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidRevision(String);

impl fmt::Display for InvalidRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid Git revision: {:?}", self.0)
    }
}

impl std::error::Error for InvalidRevision {}

/// A Git object identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(String);

impl ObjectId {
    pub(crate) fn parse(value: String) -> Result<Self, GitError> {
        if value.len() < 4 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(GitError::InvalidObjectId(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Staged, unstaged, and untracked paths in a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeStatus {
    pub(crate) head: ObjectId,
    pub(crate) branch: Option<String>,
    pub(crate) staged: BTreeSet<Utf8PathBuf>,
    pub(crate) unstaged: BTreeSet<Utf8PathBuf>,
    pub(crate) untracked: BTreeSet<Utf8PathBuf>,
    pub(crate) conflicted: BTreeSet<Utf8PathBuf>,
}

impl WorktreeStatus {
    #[must_use]
    pub fn head(&self) -> &ObjectId {
        &self.head
    }

    #[must_use]
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    #[must_use]
    pub fn staged(&self) -> &BTreeSet<Utf8PathBuf> {
        &self.staged
    }

    #[must_use]
    pub fn unstaged(&self) -> &BTreeSet<Utf8PathBuf> {
        &self.unstaged
    }

    #[must_use]
    pub fn untracked(&self) -> &BTreeSet<Utf8PathBuf> {
        &self.untracked
    }

    #[must_use]
    pub fn conflicted(&self) -> &BTreeSet<Utf8PathBuf> {
        &self.conflicted
    }

    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty()
            && self.unstaged.is_empty()
            && self.untracked.is_empty()
            && self.conflicted.is_empty()
    }

    #[must_use]
    pub fn all_changed_paths(&self) -> BTreeSet<Utf8PathBuf> {
        self.staged
            .iter()
            .chain(&self.unstaged)
            .chain(&self.untracked)
            .chain(&self.conflicted)
            .cloned()
            .collect()
    }
}

/// Paths changed on the source side of a target revision, including local
/// staged, unstaged, and untracked work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSideChanges {
    pub(crate) paths: BTreeSet<Utf8PathBuf>,
}

/// A source-control operation currently owned by Git.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Rebase,
    Merge,
    CherryPick,
}

/// Result of starting or continuing a rebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebaseOutcome {
    Completed,
    Conflicts,
}

impl SourceSideChanges {
    #[must_use]
    pub fn paths(&self) -> &BTreeSet<Utf8PathBuf> {
        &self.paths
    }

    #[must_use]
    pub fn contains(&self, path: impl AsRef<Utf8Path>) -> bool {
        self.paths.contains(path.as_ref())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.paths.len()
    }
}

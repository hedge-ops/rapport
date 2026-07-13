//! Validated values returned to and accepted from Git operations.
//!
//! This module owns repository, revision, object, worktree, and operation state;
//! command execution remains a boundary concern.

use crate::{InvalidBranchName, InvalidObjectId, InvalidRevision};
use rapport_files::{Utf8Path, Utf8PathBuf};
use std::collections::BTreeSet;

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

/// A validated local or remote Git branch name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
pub struct BranchName(String);

impl BranchName {
    /// Validate a branch name using Git ref-format restrictions.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidBranchName`] when the value is reserved, option-like,
    /// or cannot form a `refs/heads/*` reference.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidBranchName> {
        let value = value.into();
        let invalid_character = value.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        });
        let invalid_component = value.split('/').any(|component| {
            component.is_empty()
                || component.starts_with('.')
                || component.as_bytes().ends_with(b".lock")
        });
        if value.is_empty()
            || value == "HEAD"
            || value.starts_with('-')
            || value.ends_with('.')
            || value.contains("..")
            || value.contains("@{")
            || invalid_character
            || invalid_component
        {
            return Err(InvalidBranchName::new(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A Git revision expression supplied only where arbitrary revspecs are valid.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
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
            return Err(InvalidRevision::new(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn local_branch(branch: &BranchName) -> Self {
        Self(format!("refs/heads/{}", branch.as_str()))
    }

    pub(crate) fn remote_tracking(remote: &str, branch: &BranchName) -> Self {
        Self(format!("refs/remotes/{remote}/{}", branch.as_str()))
    }
}

/// A Git object identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
pub struct ObjectId(String);

impl ObjectId {
    /// Validate a hexadecimal Git object identifier.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidObjectId`] when the value is shorter than Git's minimum
    /// abbreviation or contains non-hexadecimal characters.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidObjectId> {
        let value = value.into();
        if value.len() < 4 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(InvalidObjectId::new(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A resolved local branch and its current head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalBranch {
    pub(crate) name: BranchName,
    pub(crate) head: ObjectId,
}

impl LocalBranch {
    #[must_use]
    pub fn name(&self) -> &BranchName {
        &self.name
    }

    #[must_use]
    pub fn head(&self) -> &ObjectId {
        &self.head
    }

    #[must_use]
    pub fn revision(&self) -> Revision {
        Revision::local_branch(&self.name)
    }
}

/// A resolved remote-tracking branch and its current head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTrackingBranch {
    pub(crate) remote: String,
    pub(crate) name: BranchName,
    pub(crate) head: ObjectId,
}

impl RemoteTrackingBranch {
    #[must_use]
    pub fn remote(&self) -> &str {
        &self.remote
    }

    #[must_use]
    pub fn name(&self) -> &BranchName {
        &self.name
    }

    #[must_use]
    pub fn head(&self) -> &ObjectId {
        &self.head
    }

    #[must_use]
    pub fn revision(&self) -> Revision {
        Revision::remote_tracking(&self.remote, &self.name)
    }
}

/// Staged, unstaged, and untracked paths in a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeStatus {
    pub(crate) head: ObjectId,
    pub(crate) branch: Option<LocalBranch>,
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
    pub fn branch(&self) -> Option<&BranchName> {
        self.branch.as_ref().map(LocalBranch::name)
    }

    #[must_use]
    pub fn local_branch(&self) -> Option<&LocalBranch> {
        self.branch.as_ref()
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

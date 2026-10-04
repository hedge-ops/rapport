//! Read-only changeset and policy snapshot primitives.

use crate::error::{single_line, zero_delimited_paths};
use crate::{Git, GitError, ObjectId, Repository};
use rapport_command::Runner;
use rapport_files::{Utf8Path, Utf8PathBuf};
use std::collections::BTreeSet;

impl<R: Runner> Git<R> {
    /// Resolve the common ancestor of two selected commits.
    ///
    /// # Errors
    /// Returns an error when the histories have no merge base or Git fails.
    pub fn merge_base_between(
        &self,
        repository: &Repository,
        base: &ObjectId,
        head: &ObjectId,
    ) -> Result<ObjectId, GitError> {
        Ok(ObjectId::new(single_line(
            &self.run_in(
                repository,
                ["merge-base", base.as_str(), head.as_str()],
                "find merge base",
            )?,
            "find merge base",
        )?)?)
    }

    /// List both sides of moves as independent deleted and added paths.
    ///
    /// # Errors
    /// Returns an error if Git cannot compare the commits or paths are not UTF-8.
    pub fn changed_paths(
        &self,
        repository: &Repository,
        before: &ObjectId,
        after: &ObjectId,
    ) -> Result<BTreeSet<Utf8PathBuf>, GitError> {
        zero_delimited_paths(
            self.run_in(
                repository,
                [
                    "diff",
                    "--no-ext-diff",
                    "--no-renames",
                    "--name-only",
                    "-z",
                    before.as_str(),
                    after.as_str(),
                    "--",
                ],
                "read committed changes",
            )?
            .stdout(),
            "read committed changes",
        )
    }

    /// List staged or unstaged paths without losing the old side of renames.
    ///
    /// # Errors
    /// Returns an error if Git fails or paths are not UTF-8.
    pub fn pending_paths(
        &self,
        repository: &Repository,
        staged: bool,
    ) -> Result<BTreeSet<Utf8PathBuf>, GitError> {
        let outcome = if staged {
            self.run_in(
                repository,
                [
                    "diff",
                    "--cached",
                    "--no-ext-diff",
                    "--no-renames",
                    "--name-only",
                    "-z",
                    "--",
                ],
                "read staged changes",
            )?
        } else {
            self.run_in(
                repository,
                [
                    "diff",
                    "--no-ext-diff",
                    "--no-renames",
                    "--name-only",
                    "-z",
                    "--",
                ],
                "read unstaged changes",
            )?
        };
        zero_delimited_paths(outcome.stdout(), "read pending changes")
    }

    /// List paths in a commit tree or, with no commit, the index.
    ///
    /// # Errors
    /// Returns an error if Git fails or paths are not UTF-8.
    pub fn snapshot_paths(
        &self,
        repository: &Repository,
        commit: Option<&ObjectId>,
    ) -> Result<BTreeSet<Utf8PathBuf>, GitError> {
        let outcome = if let Some(commit) = commit {
            self.run_in(
                repository,
                ["ls-tree", "-r", "--name-only", "-z", commit.as_str(), "--"],
                "list tree paths",
            )?
        } else {
            self.run_in(
                repository,
                ["ls-files", "--cached", "-z", "--"],
                "list index paths",
            )?
        };
        zero_delimited_paths(outcome.stdout(), "list snapshot paths")
    }

    /// Read a UTF-8 blob from a commit tree or the index without checking it out.
    ///
    /// # Errors
    /// Returns an error for missing blobs, invalid UTF-8, or Git failures.
    pub fn snapshot_file(
        &self,
        repository: &Repository,
        commit: Option<&ObjectId>,
        path: &Utf8Path,
    ) -> Result<String, GitError> {
        let object = format!("{}:{path}", commit.map_or("", ObjectId::as_str));
        let outcome = self.run_in(repository, ["show", &object], "read snapshot file")?;
        std::str::from_utf8(outcome.stdout())
            .map(str::to_owned)
            .map_err(|source| GitError::InvalidUtf8 {
                operation: "read snapshot file",
                source,
            })
    }
}

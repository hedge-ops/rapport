//! Git repository operations backed by an injected command runner.
//!
//! This module owns command construction and interpretation while returning
//! the validated domain values exposed by the crate.

use crate::error::{command_failed, single_line, zero_delimited_paths};
use crate::{
    BranchName, GitError, LocalBranch, ObjectId, Operation, RebaseOutcome, RemoteTrackingBranch,
    Repository, Revision, SourceSideChanges, WorktreeStatus,
};
use rapport_command::{CommandOutcome, CommandSpec, Runner, SystemRunner};
use rapport_files::{Utf8Path, Utf8PathBuf};

/// Git operations backed by an injected command runner.
#[derive(Debug, Clone)]
pub struct Git<R = SystemRunner> {
    runner: R,
}

impl Default for Git<SystemRunner> {
    fn default() -> Self {
        Self {
            runner: SystemRunner,
        }
    }
}

impl<R: Runner> Git<R> {
    #[must_use]
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Discover the containing worktree from any path inside it.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot be invoked, the path is not inside a
    /// worktree, or Git returns a non-UTF-8 root path.
    pub fn discover(&self, start: impl AsRef<Utf8Path>) -> Result<Repository, GitError> {
        let outcome = self.run(
            &CommandSpec::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .current_dir(start.as_ref()),
            "discover repository",
        )?;
        let root = single_line(&outcome, "discover repository")?;
        Ok(Repository {
            root: Utf8PathBuf::from(root),
        })
    }

    /// Read the current commit, branch, and local worktree changes.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot inspect the repository or emits a
    /// non-UTF-8 path.
    pub fn status(&self, repository: &Repository) -> Result<WorktreeStatus, GitError> {
        let head = ObjectId::new(single_line(
            &self.run_in(repository, ["rev-parse", "HEAD"], "read HEAD")?,
            "read HEAD",
        )?)?;

        let branch_outcome = self.run_allowing_failure(
            &CommandSpec::new("git")
                .args(["symbolic-ref", "--short", "-q", "HEAD"])
                .current_dir(repository.root()),
            "read branch",
        )?;
        let branch = if branch_outcome.success() {
            let name = BranchName::new(single_line(&branch_outcome, "read branch")?)?;
            Some(LocalBranch {
                name,
                head: head.clone(),
            })
        } else if branch_outcome.exit_code() == Some(1) {
            None
        } else {
            return Err(command_failed("read branch", &branch_outcome));
        };

        let staged = zero_delimited_paths(
            self.run_in(
                repository,
                ["diff", "--cached", "--name-only", "-z", "--"],
                "read staged paths",
            )?
            .stdout(),
            "read staged paths",
        )?;
        let unstaged = zero_delimited_paths(
            self.run_in(
                repository,
                ["diff", "--name-only", "-z", "--"],
                "read unstaged paths",
            )?
            .stdout(),
            "read unstaged paths",
        )?;
        let untracked = zero_delimited_paths(
            self.run_in(
                repository,
                ["ls-files", "--others", "--exclude-standard", "-z", "--"],
                "read untracked paths",
            )?
            .stdout(),
            "read untracked paths",
        )?;
        let conflicted = zero_delimited_paths(
            self.run_in(
                repository,
                ["diff", "--name-only", "--diff-filter=U", "-z", "--"],
                "read conflicted paths",
            )?
            .stdout(),
            "read conflicted paths",
        )?;

        Ok(WorktreeStatus {
            head,
            branch,
            staged,
            unstaged,
            untracked,
            conflicted,
        })
    }

    /// List tracked files and non-ignored untracked files in the current worktree.
    ///
    /// This uses Git's standard exclusion rules, including repository ignores,
    /// `.git/info/exclude`, and configured global excludes. Tracked paths remain
    /// present even when a matching ignore pattern exists.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot inspect the repository or emits a
    /// non-UTF-8 path.
    pub fn working_tree_files(
        &self,
        repository: &Repository,
    ) -> Result<std::collections::BTreeSet<Utf8PathBuf>, GitError> {
        zero_delimited_paths(
            self.run_in(
                repository,
                [
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                    "--",
                ],
                "list working-tree files",
            )?
            .stdout(),
            "list working-tree files",
        )
    }

    /// Resolve a branch, tag, or other validated revision to a commit.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot resolve the revision to a commit.
    pub fn resolve(
        &self,
        repository: &Repository,
        revision: &Revision,
    ) -> Result<ObjectId, GitError> {
        let commit = format!("{}^{{commit}}", revision.as_str());
        Ok(ObjectId::new(single_line(
            &self.run_args(
                repository,
                ["rev-parse", "--verify", &commit],
                "resolve revision",
            )?,
            "resolve revision",
        )?)?)
    }

    /// Resolve the conventional default target branch without network access.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when neither `origin/HEAD`, `main`, nor `master` resolves.
    pub fn default_target(&self, repository: &Repository) -> Result<BranchName, GitError> {
        let remote = self.run_allowing_failure(
            &CommandSpec::new("git")
                .args(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
                .current_dir(repository.root()),
            "read default branch",
        )?;
        if remote.success() {
            let branch = single_line(&remote, "read default branch")?;
            return BranchName::new(branch.strip_prefix("origin/").unwrap_or(&branch).to_owned())
                .map_err(GitError::from);
        }
        for candidate in ["main", "master"] {
            let branch = BranchName::new(candidate)?;
            if self.local_branch(repository, &branch).is_ok() {
                return Ok(branch);
            }
        }
        Err(GitError::MissingOutput("read default branch"))
    }

    /// Report whether the target commit is already contained in source `HEAD`.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot compare the revisions.
    pub fn contains(&self, repository: &Repository, target: &Revision) -> Result<bool, GitError> {
        let outcome = self.run_allowing_failure(
            &CommandSpec::new("git")
                .args(["merge-base", "--is-ancestor", target.as_str(), "HEAD"])
                .current_dir(repository.root()),
            "compare source and target",
        )?;
        match outcome.exit_code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(command_failed("compare source and target", &outcome)),
        }
    }

    /// List source-side commits, oldest first, that are not reachable from a target.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot compare the target with source `HEAD`
    /// or emits an invalid object identifier.
    pub fn source_commits(
        &self,
        repository: &Repository,
        target: &Revision,
    ) -> Result<Vec<ObjectId>, GitError> {
        let range = format!("{}..HEAD", target.as_str());
        let outcome = self.run_in(
            repository,
            ["rev-list", "--reverse", &range],
            "list source-side commits",
        )?;
        let output =
            std::str::from_utf8(outcome.stdout()).map_err(|source| GitError::InvalidUtf8 {
                operation: "list source-side commits",
                source,
            })?;
        output
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| ObjectId::new(line.to_owned()).map_err(GitError::from))
            .collect()
    }

    /// Return the current tracked patch, including staged and unstaged changes.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot render the working patch.
    pub fn working_patch(&self, repository: &Repository) -> Result<Vec<u8>, GitError> {
        Ok(self
            .run_args(
                repository,
                ["diff", "--binary", "--no-ext-diff", "HEAD", "--"],
                "snapshot working changes",
            )?
            .stdout()
            .to_vec())
    }

    /// Commit exactly the currently staged changes.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git or a commit hook rejects the commit.
    pub fn commit(
        &self,
        repository: &Repository,
        summary: &str,
        description: Option<&str>,
    ) -> Result<ObjectId, GitError> {
        let mut spec = CommandSpec::new("git")
            .args(["commit", "-m", summary])
            .current_dir(repository.root());
        if let Some(description) = description {
            spec = spec.args(["-m", description]);
        }
        self.run(&spec, "create checkpoint commit")?;
        self.status(repository).map(|status| status.head().clone())
    }

    /// Resolve a named local branch to its current head.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot resolve the branch to a commit.
    pub fn local_branch(
        &self,
        repository: &Repository,
        name: &BranchName,
    ) -> Result<LocalBranch, GitError> {
        let revision = Revision::local_branch(name);
        let head = self.resolve(repository, &revision)?;
        Ok(LocalBranch {
            name: name.clone(),
            head,
        })
    }

    /// Resolve the named `origin` remote-tracking branch to its current head.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot resolve the branch to a commit.
    pub fn remote_tracking_branch(
        &self,
        repository: &Repository,
        name: &BranchName,
    ) -> Result<RemoteTrackingBranch, GitError> {
        let remote = "origin".to_owned();
        let revision = Revision::remote_tracking(&remote, name);
        let head = self.resolve(repository, &revision)?;
        Ok(RemoteTrackingBranch {
            remote,
            name: name.clone(),
            head,
        })
    }

    /// Publish `HEAD` to the named same-repository branch without force.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when the branch is unsafe or the remote rejects the
    /// non-force update.
    pub fn push_branch(
        &self,
        repository: &Repository,
        branch: &BranchName,
    ) -> Result<(), GitError> {
        let destination = format!("HEAD:refs/heads/{}", branch.as_str());
        self.run_args(
            repository,
            ["push", "--set-upstream", "origin", &destination],
            "publish source branch",
        )?;
        Ok(())
    }

    /// Delete a same-repository remote branch, succeeding when it is absent.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when the branch is unsafe or the remote cannot be
    /// inspected or updated.
    pub fn delete_remote_branch(
        &self,
        repository: &Repository,
        branch: &BranchName,
    ) -> Result<(), GitError> {
        let reference = format!("refs/heads/{}", branch.as_str());
        let exists = self.run_allowing_failure(
            &CommandSpec::new("git")
                .args(["ls-remote", "--exit-code", "--heads", "origin", &reference])
                .current_dir(repository.root()),
            "inspect remote source branch",
        )?;
        match exists.exit_code() {
            Some(0) => {
                let deleted = self.run_allowing_failure(
                    &CommandSpec::new("git")
                        .args(["push", "origin", "--delete", branch.as_str()])
                        .current_dir(repository.root()),
                    "delete remote source branch",
                )?;
                if deleted.success() {
                    return Ok(());
                }
                let remains = self.run_allowing_failure(
                    &CommandSpec::new("git")
                        .args(["ls-remote", "--exit-code", "--heads", "origin", &reference])
                        .current_dir(repository.root()),
                    "inspect remote source branch after failed deletion",
                )?;
                match remains.exit_code() {
                    Some(2) => Ok(()),
                    Some(0) => Err(command_failed("delete remote source branch", &deleted)),
                    _ => Err(command_failed(
                        "inspect remote source branch after failed deletion",
                        &remains,
                    )),
                }
            }
            Some(2) => Ok(()),
            _ => Err(command_failed("inspect remote source branch", &exists)),
        }
    }

    /// Fetch the target branch and resolve its remote-tracking commit.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when fetch or target resolution fails.
    pub fn fetch_target(
        &self,
        repository: &Repository,
        target: &BranchName,
    ) -> Result<RemoteTrackingBranch, GitError> {
        self.run_args(
            repository,
            ["fetch", "origin", target.as_str()],
            "update target branch",
        )?;
        self.remote_tracking_branch(repository, target)
    }

    /// Rebase source `HEAD` onto a target revision.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] for failures other than a reported conflict.
    pub fn rebase_start(
        &self,
        repository: &Repository,
        target: &Revision,
    ) -> Result<RebaseOutcome, GitError> {
        self.rebase_command(
            repository,
            &CommandSpec::new("git")
                .args(["rebase", target.as_str()])
                .current_dir(repository.root()),
            "rebase source branch",
        )
    }

    /// Continue the active rebase without opening an editor.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] for failures other than another reported conflict.
    pub fn rebase_continue(&self, repository: &Repository) -> Result<RebaseOutcome, GitError> {
        self.rebase_command(
            repository,
            &CommandSpec::new("git")
                .args(["rebase", "--continue"])
                .env("GIT_EDITOR", "true")
                .current_dir(repository.root()),
            "continue rebase",
        )
    }

    /// Abort the active rebase.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot restore the pre-rebase state.
    pub fn rebase_abort(&self, repository: &Repository) -> Result<(), GitError> {
        self.run_args(repository, ["rebase", "--abort"], "abort rebase")?;
        Ok(())
    }

    /// Detect an active rebase, merge, or cherry-pick operation.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot resolve its operation marker paths.
    pub fn operation(&self, repository: &Repository) -> Result<Option<Operation>, GitError> {
        for (marker, operation) in [
            ("rebase-merge", Operation::Rebase),
            ("rebase-apply", Operation::Rebase),
            ("MERGE_HEAD", Operation::Merge),
            ("CHERRY_PICK_HEAD", Operation::CherryPick),
        ] {
            let outcome = self.run_args(
                repository,
                ["rev-parse", "--git-path", marker],
                "inspect source-control operation",
            )?;
            let path = single_line(&outcome, "inspect source-control operation")?;
            let path = Utf8PathBuf::from(path);
            let absolute = if path.is_absolute() {
                path
            } else {
                repository.root().join(path)
            };
            if absolute.exists() {
                return Ok(Some(operation));
            }
        }
        Ok(None)
    }

    /// Find the merge base between a target revision and `HEAD`.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot resolve both histories or produces an
    /// invalid object identifier.
    pub fn merge_base(
        &self,
        repository: &Repository,
        target: &Revision,
    ) -> Result<ObjectId, GitError> {
        Ok(ObjectId::new(single_line(
            &self.run_in(
                repository,
                ["merge-base", target.as_str(), "HEAD"],
                "find merge base",
            )?,
            "find merge base",
        )?)?)
    }

    /// Collect committed source-side differences plus staged, unstaged, and
    /// untracked local paths.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot resolve the target or inspect local
    /// changes, or when a changed path is not UTF-8.
    pub fn source_side_changes(
        &self,
        repository: &Repository,
        target: &Revision,
    ) -> Result<SourceSideChanges, GitError> {
        let range = format!("{}...HEAD", target.as_str());
        let committed = self.run_in(
            repository,
            ["diff", "--name-only", "-z", &range, "--"],
            "read committed source-side paths",
        )?;
        let mut paths =
            zero_delimited_paths(committed.stdout(), "read committed source-side paths")?;
        paths.extend(self.status(repository)?.all_changed_paths());
        Ok(SourceSideChanges { paths })
    }

    fn run_in<const N: usize>(
        &self,
        repository: &Repository,
        args: [&str; N],
        operation: &'static str,
    ) -> Result<CommandOutcome, GitError> {
        self.run(
            &CommandSpec::new("git")
                .args(args)
                .current_dir(repository.root()),
            operation,
        )
    }

    fn run_args<I, S>(
        &self,
        repository: &Repository,
        args: I,
        operation: &'static str,
    ) -> Result<CommandOutcome, GitError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.run(
            &CommandSpec::new("git")
                .args(args)
                .current_dir(repository.root()),
            operation,
        )
    }

    fn rebase_command(
        &self,
        repository: &Repository,
        spec: &CommandSpec,
        operation: &'static str,
    ) -> Result<RebaseOutcome, GitError> {
        let outcome = self.run_allowing_failure(spec, operation)?;
        if outcome.success() {
            return Ok(RebaseOutcome::Completed);
        }
        if !self.status(repository)?.conflicted().is_empty() {
            return Ok(RebaseOutcome::Conflicts);
        }
        Err(command_failed(operation, &outcome))
    }

    fn run(&self, spec: &CommandSpec, operation: &'static str) -> Result<CommandOutcome, GitError> {
        let outcome = self.run_allowing_failure(spec, operation)?;
        if outcome.success() {
            Ok(outcome)
        } else {
            Err(command_failed(operation, &outcome))
        }
    }

    fn run_allowing_failure(
        &self,
        spec: &CommandSpec,
        operation: &'static str,
    ) -> Result<CommandOutcome, GitError> {
        self.runner
            .run(spec)
            .map_err(|source| GitError::Invocation { operation, source })
    }
}

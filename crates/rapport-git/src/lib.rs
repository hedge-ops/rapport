//! Concrete Git repository semantics for Rapport.

use rapport_command::{CommandOutcome, CommandSpec, Runner, SystemRunner};
use rapport_files::{Utf8Path, Utf8PathBuf};
use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::str::Utf8Error;

/// A discovered Git worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    root: Utf8PathBuf,
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
    fn parse(value: String) -> Result<Self, GitError> {
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
    head: ObjectId,
    branch: Option<String>,
    staged: BTreeSet<Utf8PathBuf>,
    unstaged: BTreeSet<Utf8PathBuf>,
    untracked: BTreeSet<Utf8PathBuf>,
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
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.untracked.is_empty()
    }

    #[must_use]
    pub fn all_changed_paths(&self) -> BTreeSet<Utf8PathBuf> {
        self.staged
            .iter()
            .chain(&self.unstaged)
            .chain(&self.untracked)
            .cloned()
            .collect()
    }
}

/// Paths changed on the source side of a target revision, including local
/// staged, unstaged, and untracked work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSideChanges {
    paths: BTreeSet<Utf8PathBuf>,
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
        let head = ObjectId::parse(single_line(
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
            Some(single_line(&branch_outcome, "read branch")?)
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

        Ok(WorktreeStatus {
            head,
            branch,
            staged,
            unstaged,
            untracked,
        })
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
        ObjectId::parse(single_line(
            &self.run_in(
                repository,
                ["merge-base", target.as_str(), "HEAD"],
                "find merge base",
            )?,
            "find merge base",
        )?)
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

/// A failure while invoking or interpreting Git.
#[derive(Debug)]
pub enum GitError {
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
            Self::Invocation { source, .. } => Some(source),
            Self::InvalidUtf8 { source, .. } => Some(source),
            Self::CommandFailed { .. } | Self::MissingOutput(_) | Self::InvalidObjectId(_) => None,
        }
    }
}

fn command_failed(operation: &'static str, outcome: &CommandOutcome) -> GitError {
    GitError::CommandFailed {
        operation,
        exit_code: outcome.exit_code(),
        stderr: outcome.stderr_lossy(),
    }
}

fn single_line(outcome: &CommandOutcome, operation: &'static str) -> Result<String, GitError> {
    let value = std::str::from_utf8(outcome.stdout())
        .map_err(|source| GitError::InvalidUtf8 { operation, source })?
        .trim();
    if value.is_empty() {
        Err(GitError::MissingOutput(operation))
    } else {
        Ok(value.to_owned())
    }
}

fn zero_delimited_paths(
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

#[cfg(test)]
mod tests {
    use super::{Git, Revision};
    use rapport_files::{Utf8Path, Utf8PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct TemporaryRepository {
        root: Utf8PathBuf,
    }

    impl TemporaryRepository {
        fn new() -> Self {
            let sequence = NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rapport-git-test-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create test repository");
            let canonical_path = std::fs::canonicalize(path).expect("canonicalize test repository");
            let root = Utf8PathBuf::from_path_buf(canonical_path).expect("test path is UTF-8");
            let repository = Self { root };
            repository.git(["init", "-q", "-b", "main"]);
            repository.git(["config", "user.name", "Rapport Test"]);
            repository.git(["config", "user.email", "rapport@example.invalid"]);
            repository
        }

        fn root(&self) -> &Utf8Path {
            &self.root
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.root.join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create test file parent");
            }
            std::fs::write(path, contents).expect("write test file");
        }

        fn git<const N: usize>(&self, args: [&str; N]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .expect("invoke Git in test");
            assert!(
                output.status.success(),
                "Git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout)
                .expect("Git test output is UTF-8")
                .trim()
                .to_owned()
        }
    }

    impl Drop for TemporaryRepository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn discovers_status_and_complete_source_side_changes() {
        let temporary = TemporaryRepository::new();
        temporary.write("base.txt", "base\n");
        temporary.git(["add", "base.txt"]);
        temporary.git(["commit", "-q", "-m", "base"]);
        let base = temporary.git(["rev-parse", "HEAD"]);

        temporary.git(["switch", "-q", "-c", "feature"]);
        temporary.write("committed.txt", "committed\n");
        temporary.git(["add", "committed.txt"]);
        temporary.git(["commit", "-q", "-m", "source commit"]);
        temporary.write("base.txt", "unstaged\n");
        temporary.write("staged file.txt", "staged\n");
        temporary.git(["add", "staged file.txt"]);
        temporary.write("untracked.txt", "untracked\n");
        std::fs::create_dir_all(temporary.root().join("nested"))
            .expect("create nested test directory");

        let git = Git::default();
        let repository = git
            .discover(temporary.root().join("nested"))
            .expect("discover temporary repository");
        let status = git.status(&repository).expect("read repository status");
        let target = Revision::new(&base).expect("valid base revision");
        let changes = git
            .source_side_changes(&repository, &target)
            .expect("read source-side changes");

        assert_eq!(repository.root(), temporary.root());
        assert_eq!(status.branch(), Some("feature"));
        assert!(!status.is_clean());
        assert!(status.unstaged().contains(Utf8Path::new("base.txt")));
        assert!(status.staged().contains(Utf8Path::new("staged file.txt")));
        assert!(status.untracked().contains(Utf8Path::new("untracked.txt")));
        assert_eq!(changes.len(), 4);
        assert!(changes.contains("base.txt"));
        assert!(changes.contains("committed.txt"));
        assert!(changes.contains("staged file.txt"));
        assert!(changes.contains("untracked.txt"));
        assert_eq!(
            git.merge_base(&repository, &target)
                .expect("find merge base")
                .as_str(),
            base
        );
    }

    #[test]
    fn rejects_revision_arguments_that_could_be_options() {
        assert!(Revision::new("main").is_ok());
        assert!(Revision::new("--output=/tmp/surprise").is_err());
        assert!(Revision::new("contains spaces").is_err());
    }
}

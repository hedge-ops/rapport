//! End-to-end Git repository behavior tests.

use super::{BranchName, Git, ObjectId, Revision};
use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
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
        assert_ok!(std::fs::create_dir_all(&path));
        let canonical_path = assert_ok!(std::fs::canonicalize(path));
        let root = assert_ok!(Utf8PathBuf::from_path_buf(canonical_path));
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
            assert_ok!(std::fs::create_dir_all(parent));
        }
        assert_ok!(std::fs::write(path, contents));
    }

    fn git<const N: usize>(&self, args: [&str; N]) -> String {
        let output = assert_ok!(
            Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
        );
        assert!(
            output.status.success(),
            "expecting Git command to succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

impl Drop for TemporaryRepository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
/// When a worktree is inspected, its attached branch and complete source-side changes are reported.
fn status_should_report_repository_and_source_side_changes() {
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
    assert_ok!(std::fs::create_dir_all(temporary.root().join("nested")));

    let git = Git::default();
    let repository = assert_ok!(git.discover(temporary.root().join("nested")));
    let status = assert_ok!(git.status(&repository));
    let target = assert_ok!(Revision::new(&base));
    let changes = assert_ok!(git.source_side_changes(&repository, &target));
    let source_commits = assert_ok!(git.source_commits(&repository, &target));

    assert_eq!(repository.root(), temporary.root());
    assert_eq!(status.branch().map(BranchName::as_str), Some("feature"));
    let local = assert_some!(status.local_branch());
    assert_eq!(local.name().as_str(), "feature");
    assert_eq!(local.head(), status.head());
    assert_eq!(local.revision().as_str(), "refs/heads/feature");
    assert!(!status.is_clean());
    assert!(status.unstaged().contains(Utf8Path::new("base.txt")));
    assert!(status.staged().contains(Utf8Path::new("staged file.txt")));
    assert!(status.untracked().contains(Utf8Path::new("untracked.txt")));
    assert_eq!(changes.len(), 4);
    assert!(changes.contains("base.txt"));
    assert!(changes.contains("committed.txt"));
    assert!(changes.contains("staged file.txt"));
    assert!(changes.contains("untracked.txt"));
    assert_eq!(source_commits.len(), 1);
    assert_eq!(
        source_commits[0].as_str(),
        temporary.git(["rev-parse", "HEAD"])
    );
    assert_eq!(
        assert_ok!(git.merge_base(&repository, &target)).as_str(),
        base
    );
}

#[test]
fn revision_should_reject_option_like_arguments() {
    assert!(Revision::new("main").is_ok());
    assert!(Revision::new("--output=/tmp/surprise").is_err());
    assert!(Revision::new("contains spaces").is_err());
}

#[test]
fn branch_name_should_reject_values_that_are_only_valid_as_revisions() {
    for valid in ["main", "work/106-rust-rule-cleanup", "release-0.3"] {
        assert!(
            BranchName::new(valid).is_ok(),
            "expecting {valid:?} to be a valid branch name"
        );
    }
    for invalid in [
        "",
        "HEAD",
        "-option",
        "contains spaces",
        "feature..other",
        "feature@{upstream}",
        "feature/.hidden",
        "feature.lock",
        "feature//nested",
        "feature?query",
        "feature\\windows",
        "trailing.",
    ] {
        assert!(
            BranchName::new(invalid).is_err(),
            "expecting {invalid:?} to be rejected as a branch name"
        );
    }
    assert!(Revision::new("HEAD~1").is_ok());
    assert!(BranchName::new("HEAD~1").is_err());
}

#[test]
fn object_id_should_reject_non_hexadecimal_values() {
    assert!(ObjectId::new("deadbeef").is_ok());
    assert!(ObjectId::new("abc").is_err());
    assert!(ObjectId::new("not-an-object").is_err());
}

#[test]
/// When a branch is published, its tracking state is observable and deletion is idempotent.
fn push_branch_should_publish_and_delete_remote_branch_idempotently() {
    let temporary = TemporaryRepository::new();
    temporary.write("base.txt", "base\n");
    temporary.git(["add", "base.txt"]);
    temporary.git(["commit", "-q", "-m", "base"]);
    temporary.git(["switch", "-q", "-c", "feature"]);
    let remote = Utf8PathBuf::from(format!("{}-remote.git", temporary.root()));
    let initialized = assert_ok!(
        Command::new("git")
            .args(["init", "--bare", "-q", remote.as_str()])
            .output()
    );
    assert!(initialized.status.success());
    temporary.git(["remote", "add", "origin", remote.as_str()]);
    let git = Git::default();
    let repository = assert_ok!(git.discover(temporary.root()));
    let branch = assert_ok!(BranchName::new("feature"));

    assert_ok!(git.push_branch(&repository, &branch));
    assert!(
        !temporary
            .git(["ls-remote", "--heads", "origin", "feature"])
            .is_empty()
    );
    let tracking = assert_ok!(git.fetch_target(&repository, &branch));
    assert_eq!(tracking.remote(), "origin");
    assert_eq!(tracking.name(), &branch);
    assert_eq!(tracking.head(), assert_ok!(git.status(&repository)).head());
    assert_eq!(tracking.revision().as_str(), "refs/remotes/origin/feature");
    assert_ok!(git.delete_remote_branch(&repository, &branch));
    assert_ok!(git.delete_remote_branch(&repository, &branch));
    assert!(
        temporary
            .git(["ls-remote", "--heads", "origin", "feature"])
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(remote);
}

//! End-to-end Git repository behavior tests.

use super::{Git, Revision};
use claims::assert_ok;
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
        let root = Utf8PathBuf::from_path_buf(canonical_path)
            .unwrap_or_else(|path| panic!("test path is not UTF-8: {}", path.display()));
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
            "Git failed: {}",
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
    assert_ok!(std::fs::create_dir_all(temporary.root().join("nested")));

    let git = Git::default();
    let repository = assert_ok!(git.discover(temporary.root().join("nested")));
    let status = assert_ok!(git.status(&repository));
    let target = assert_ok!(Revision::new(&base));
    let changes = assert_ok!(git.source_side_changes(&repository, &target));
    let source_commits = assert_ok!(git.source_commits(&repository, &target));

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
fn rejects_revision_arguments_that_could_be_options() {
    assert!(Revision::new("main").is_ok());
    assert!(Revision::new("--output=/tmp/surprise").is_err());
    assert!(Revision::new("contains spaces").is_err());
}

#[test]
fn publishes_and_idempotently_deletes_a_remote_branch() {
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

    assert_ok!(git.push_branch(&repository, "feature"));
    assert!(
        !temporary
            .git(["ls-remote", "--heads", "origin", "feature"])
            .is_empty()
    );
    assert_ok!(git.delete_remote_branch(&repository, "feature"));
    assert_ok!(git.delete_remote_branch(&repository, "feature"));
    assert!(
        temporary
            .git(["ls-remote", "--heads", "origin", "feature"])
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(remote);
}

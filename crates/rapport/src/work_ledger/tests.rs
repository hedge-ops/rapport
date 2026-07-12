use super::domain::{RequestKind, RequestSource, Task, TaskStatus, Work, Workflow};
use super::repository::Store;
use crate::{Clock, CommandOutcome, CommandRunner, CommandSpec, run_with_environment};
use claims::assert_ok;
use rapport_files::{FileSystem, InMemoryFileSystem, RealFileSystem, Utf8Path, Utf8PathBuf};
use std::io;
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-07-12T23:00:00Z".to_owned()
    }
}

#[derive(Debug)]
struct NeverRunner;

impl CommandRunner for NeverRunner {
    fn run(&self, _spec: &CommandSpec, _cwd: &Utf8Path) -> io::Result<CommandOutcome> {
        panic!("the Phase 3 Work boundary should use rapport-git")
    }
}

#[derive(Debug)]
struct TemporaryRepository {
    root: Utf8PathBuf,
}

impl TemporaryRepository {
    fn new() -> Self {
        let sequence = NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rapport-work-ledger-test-{}-{sequence}",
            std::process::id()
        ));
        assert_ok!(std::fs::create_dir_all(&path));
        let canonical = assert_ok!(std::fs::canonicalize(path));
        let root = Utf8PathBuf::from_path_buf(canonical)
            .unwrap_or_else(|path| panic!("test path is not UTF-8: {}", path.display()));
        let repository = Self { root };
        repository.git(["init", "-q", "-b", "main"]);
        repository.git(["config", "user.name", "Rapport Test"]);
        repository.git(["config", "user.email", "rapport@example.invalid"]);
        repository.git(["remote", "add", "origin", repository.root.as_str()]);
        repository.write(
            ".gitignore",
            ".rapport/**\n!.rapport/\n!.rapport/rules/\n!.rapport/rules/**\n!.rapport/rules.lock\n",
        );
        repository.write("request.md", "# Request\n\nBuild the Work ledger.\n");
        repository.write(
            "context.toml",
            "version = 1\nid = \"ROOT\"\npurpose = \"Repository policy.\"\nnext_ownership = 1\nnext_boundary = 1\n\n[ruleset]\nincludes = []\n",
        );
        repository.git(["add", ".gitignore", "request.md", "context.toml"]);
        repository.git(["commit", "-q", "-m", "base"]);
        repository.git(["switch", "-q", "-c", "feature"]);
        repository
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

    fn run(&self, args: &[&str]) -> (ExitCode, String, String) {
        let mut fs = RealFileSystem;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_environment(
            args.iter().map(|argument| (*argument).to_owned()),
            &NeverRunner,
            &mut fs,
            &FixedClock,
            self.root.clone(),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    fn succeeds(&self, args: &[&str]) -> String {
        let (code, out, err) = self.run(args);
        assert_eq!(code, ExitCode::SUCCESS, "{args:?}: {err}");
        assert!(err.is_empty(), "{args:?}: {err}");
        out
    }
}

impl Drop for TemporaryRepository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
// WRK-001–WRK-005: the Phase 3 golden path keeps request, Git, Tasks, and proof readiness aligned.
fn phase_three_work_checkpoint_and_rebase_lifecycle() {
    let repository = TemporaryRepository::new();

    let started = repository.succeeds(&[
        "work",
        "start",
        "--plan",
        "request.md",
        "--title",
        "Build Work ledger",
        "--description",
        "Persist the request, Tasks, Git identities, and next action.",
        "--target",
        "main",
    ]);
    assert!(started.contains("Build Work ledger"));
    assert!(repository.root.join(".rapport/work.toml").is_file());

    let (duplicate_code, _, duplicate_error) = repository.run(&[
        "work",
        "start",
        "--ad-hoc",
        "duplicate",
        "--title",
        "Duplicate",
        "--target",
        "main",
    ]);
    assert_eq!(duplicate_code, ExitCode::from(2));
    assert!(duplicate_error.contains("active Work already exists"));

    repository.write("src/new.rs", "pub fn phase_three() {}\n");
    let checkpoint = repository.succeeds(&["work", "checkpoint", "start"]);
    assert!(checkpoint.contains("TASK_001"));
    assert!(checkpoint.contains("src/new.rs"));
    repository.git(["add", "src/new.rs"]);
    let completed = repository.succeeds(&[
        "work",
        "checkpoint",
        "complete",
        "Add Work ledger source",
        "--description",
        "Record the first coherent candidate.",
    ]);
    assert!(completed.contains("status") || completed.contains("commit"));
    assert!(completed.contains("TASK_001"));

    let status = repository.succeeds(&["work", "status"]);
    assert!(status.contains("candidate files` — src/new.rs"));
    assert!(status.contains("latest checkpoint"));
    assert!(status.contains("required signoffs` — none"));
    let tasks = repository.succeeds(&["work", "task", "list", "--all"]);
    assert!(tasks.contains("TASK_001"));
    assert!(tasks.contains("passed"));
    let shown = repository.succeeds(&["work", "task", "show", "TASK_001"]);
    assert!(shown.contains("Add Work ledger source"));
    assert!(shown.contains("duration_seconds"));
    let next = repository.succeeds(&["work", "task", "next"]);
    assert!(next.contains("rapport build"));

    repository.git(["switch", "-q", "main"]);
    repository.write("target.txt", "target advanced\n");
    repository.git(["add", "target.txt"]);
    repository.git(["commit", "-q", "-m", "advance target"]);
    repository.git(["switch", "-q", "feature"]);
    let rebased = repository.succeeds(&["work", "rebase", "start"]);
    assert!(rebased.contains("status` — passed"));
    assert!(rebased.contains("TASK_002"));
    let rebase_task = repository.succeeds(&["work", "task", "show", "TASK_002"]);
    assert!(rebase_task.contains("prior_source_commits"));
    assert!(rebase_task.contains("resulting_source_commits"));
    assert!(rebase_task.contains("duration_seconds"));

    let after_rebase = repository.succeeds(&["work", "status"]);
    assert!(after_rebase.contains("contains target` — true"));
    assert!(after_rebase.contains("tasks` — 2"));
}

#[test]
// WRK-004: dirty rebase preparation is durable corrective Work, not an implicit stash.
fn dirty_rebase_creates_one_corrective_action_and_resumes() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Exercise dirty rebase recovery.",
        "--title",
        "Recover rebase",
        "--target",
        "main",
    ]);
    repository.write("dirty.txt", "not checkpointed\n");

    let (code, _, error) = repository.run(&["work", "rebase", "start"]);
    assert_eq!(code, ExitCode::from(2));
    assert!(error.contains("requires a clean worktree"), "{error}");
    let blocked = repository.succeeds(&["work", "task", "list", "--all"]);
    assert!(blocked.contains("TASK_001"));
    assert!(blocked.contains("TASK_002"));
    assert!(blocked.contains("blocked"));
    assert!(blocked.contains("pending"));

    assert_ok!(std::fs::remove_file(repository.root.join("dirty.txt")));
    let resumed = repository.succeeds(&["work", "rebase", "start"]);
    assert!(resumed.contains("TASK_001"));
    assert!(resumed.contains("passed"));
    let completed = repository.succeeds(&["work", "task", "list", "--all"]);
    assert_eq!(completed.matches("TASK_001").count(), 1);
    assert_eq!(completed.matches("TASK_002").count(), 1);
    assert!(!completed.contains("blocked"));
    assert!(!completed.contains("pending"));
}

#[test]
// WRK-003: a checkpoint rejects content that changed after reconciliation began.
fn checkpoint_refuses_content_changed_after_reconciliation_started() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Detect concurrent edits.",
        "--title",
        "Protect checkpoint",
        "--target",
        "main",
    ]);
    repository.write("candidate.txt", "first version\n");
    repository.succeeds(&["work", "checkpoint", "start"]);
    repository.write("candidate.txt", "changed after start\n");
    repository.git(["add", "candidate.txt"]);

    let (code, _, error) = repository.run(&["work", "checkpoint", "complete", "Unsafe checkpoint"]);

    assert_eq!(code, ExitCode::from(2));
    assert!(error.contains("files changed after checkpoint start"));
    assert_eq!(repository.git(["rev-list", "--count", "HEAD"]), "1");
    let task = repository.succeeds(&["work", "task", "show", "TASK_001"]);
    assert!(task.contains("running"));
}

#[test]
// WRK-003: Git remains authoritative when a clean descendant commit is unambiguous.
fn checkpoint_adopts_an_unambiguous_commit_created_directly_with_git() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Adopt direct Git history.",
        "--title",
        "Adopt checkpoint",
        "--target",
        "main",
    ]);
    repository.write("direct.txt", "committed directly\n");
    repository.git(["add", "direct.txt"]);
    repository.git(["commit", "-q", "-m", "direct commit"]);
    let next = repository.succeeds(&["work", "task", "next"]);
    assert!(next.contains("rapport work checkpoint start"));

    let adopted = repository.succeeds(&["work", "checkpoint", "start"]);

    assert!(adopted.contains("status` — passed"));
    assert!(adopted.contains("direct.txt"));
    let status = repository.succeeds(&["work", "status"]);
    assert!(status.contains("latest checkpoint"));
    assert!(status.contains("TASK_001 passed checkpoint"));
    let task = repository.succeeds(&["work", "task", "show", "TASK_001"]);
    assert!(task.contains("adopted checkpoint"));
    assert!(task.contains("committed_files=direct.txt"));
}

#[test]
// WRK-001, WRK-002: finalized Work preserves its request and complete Task ledger.
fn archive_writes_global_history_before_removing_local_state() {
    let mut fs = InMemoryFileSystem::default();
    let store = Store::new("/repository");
    let mut work = assert_ok!(Work::new(
        "Archived Work".to_owned(),
        "Preserve the complete local ledger.".to_owned(),
        RequestSource {
            kind: RequestKind::Ticket,
            value: "#106".to_owned(),
        },
        "feature".to_owned(),
        "main".to_owned(),
        "1111".to_owned(),
        "1111".to_owned(),
        "2026-07-12T23:00:00Z".to_owned(),
    ));
    let mut task = Task::new(
        assert_ok!(work.allocate_task_id()),
        "checkpoint",
        Workflow::Develop,
        "Checkpoint",
        "Persist a coherent change.",
        "rapport work checkpoint start",
        TaskStatus::Running,
        "1111",
        "2026-07-12T23:00:00Z",
        Some("rapport work checkpoint complete <SUMMARY>".to_owned()),
    );
    task.finish(
        TaskStatus::Passed,
        "2026-07-12T23:00:08Z".to_owned(),
        "created checkpoint 2222".to_owned(),
        None,
    );
    assert_ok!(store.save_work_and_task(&mut fs, &work, &task));

    let history = assert_ok!(store.archive(&mut fs, &work, std::slice::from_ref(&task)));

    assert!(fs.is_file(history.join("work.toml")));
    assert!(fs.is_file(history.join("tasks/TASK_001.toml")));
    assert!(!fs.is_file("/repository/.rapport/work.toml"));
    assert!(!fs.is_file("/repository/.rapport/tasks/TASK_001.toml"));
    let archived_task = assert_ok!(fs.read_to_string(history.join("tasks/TASK_001.toml")));
    assert!(archived_task.contains("duration_seconds = \"8\""));
}

#[test]
// WRK-001: Work begins from exactly one durable request source.
fn work_start_requires_exactly_one_request_source() {
    let repository = TemporaryRepository::new();

    let (missing_code, _, missing_error) = repository.run(&[
        "work",
        "start",
        "--title",
        "Missing request",
        "--description",
        "No durable source.",
        "--target",
        "main",
    ]);
    assert_eq!(missing_code, ExitCode::from(2));
    assert!(missing_error.contains("required arguments were not provided"));

    let (multiple_code, _, multiple_error) = repository.run(&[
        "work",
        "start",
        "--ticket",
        "#106",
        "--plan",
        "request.md",
        "--title",
        "Too many requests",
        "--description",
        "Ambiguous durable source.",
        "--target",
        "main",
    ]);
    assert_eq!(multiple_code, ExitCode::from(2));
    assert!(multiple_error.contains("cannot be used with"));
}

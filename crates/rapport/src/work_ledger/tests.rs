use super::domain::{RequestKind, RequestSource, Task, TaskStatus, Work, Workflow};
use super::repository::Store;
use crate::{Clock, CommandOutcome, CommandRunner, CommandSpec, run_with_environment};
use claims::assert_ok;
use rapport_files::{FileSystem, InMemoryFileSystem, RealFileSystem, Utf8Path, Utf8PathBuf};
use std::collections::VecDeque;
use std::io;
use std::process::{Command, ExitCode};
use std::sync::Mutex;
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
struct QueueRunner {
    outcomes: Mutex<VecDeque<CommandOutcome>>,
    calls: Mutex<Vec<(CommandSpec, Utf8PathBuf)>>,
}

impl QueueRunner {
    fn new(outcomes: impl IntoIterator<Item = CommandOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(CommandSpec, Utf8PathBuf)> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl CommandRunner for QueueRunner {
    fn run(&self, spec: &CommandSpec, cwd: &Utf8Path) -> io::Result<CommandOutcome> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((spec.clone(), cwd.to_path_buf()));
        self.outcomes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .ok_or_else(|| io::Error::other("missing queued outcome"))
    }
}

#[derive(Debug)]
struct MutatingRunner;

impl CommandRunner for MutatingRunner {
    fn run(&self, _spec: &CommandSpec, cwd: &Utf8Path) -> io::Result<CommandOutcome> {
        std::fs::write(cwd.join("generated.txt"), "generated during build\n")?;
        Ok(successful("generated output\n"))
    }
}

fn successful(stdout: &str) -> CommandOutcome {
    CommandOutcome {
        success: true,
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

fn failing(stderr: &str) -> CommandOutcome {
    CommandOutcome {
        success: false,
        stdout: String::new(),
        stderr: stderr.to_owned(),
    }
}

fn review_result(checksum: &str, grade: &str, action: bool) -> String {
    let categories = [
        "Intent and correctness",
        "Architecture and boundaries",
        "Rules and code quality",
        "Tests and reliability",
        "Security and privacy",
        "Documentation and operability",
        "Compatibility and dependencies",
    ]
    .into_iter()
    .map(|category| serde_json::json!({"category":category,"grade":grade,"explanation":"Concrete inspection found no unresolved category risk."}))
    .collect::<Vec<_>>();
    let actions = if action {
        vec![serde_json::json!({
            "title":"Clarify behavior", "explanation":"The behavior needs a clearer contract.",
            "rule_ids":[], "evidence":[{"path":"src/lib.rs","line":1,"description":"The public boundary is ambiguous."}],
            "impact":"Users may misunderstand the behavior.", "recommended_correction":"Document and test the contract."
        })]
    } else {
        Vec::new()
    };
    assert_ok!(serde_json::to_string_pretty(&serde_json::json!({
        "input_checksum":checksum,"overall_grade":grade,"overall_explanation":"The grade follows the concrete findings.",
        "categories":categories,"proposed_actions":actions,"suggested_rule_improvements":[]
    })))
}

fn request_checksum(request: &str) -> &str {
    let checksum = request
        .split("input_checksum `")
        .nth(1)
        .and_then(|value| value.split('`').next());
    let Some(checksum) = checksum else {
        panic!("request omitted its checksum")
    };
    checksum
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
        self.run_with(args, &NeverRunner)
    }

    fn run_with(&self, args: &[&str], runner: &dyn CommandRunner) -> (ExitCode, String, String) {
        let mut fs = RealFileSystem;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_environment(
            args.iter().map(|argument| (*argument).to_owned()),
            runner,
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

    fn succeeds_with(&self, args: &[&str], runner: &dyn CommandRunner) -> String {
        let (code, out, err) = self.run_with(args, runner);
        assert_eq!(code, ExitCode::SUCCESS, "{args:?}: {err}");
        assert!(err.is_empty(), "{args:?}: {err}");
        out
    }

    fn add_signoff(&self, target: &str, stage: u32) {
        let targets = QueueRunner::new([successful(&format!("dev {target}\n"))]);
        self.succeeds_with(
            &[
                "context",
                "signoff",
                "add",
                ".",
                "--target",
                target,
                "--stage",
                &stage.to_string(),
            ],
            &targets,
        );
    }
}

impl Drop for TemporaryRepository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
/// When Phase 3 runs end to end, request, Git, Tasks, and proof readiness remain aligned (WRK-001–WRK-005).
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
/// When rebase starts dirty, corrective Work is durable and Git is never implicitly stashed (WRK-004).
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
/// When content changes during reconciliation, checkpoint completion refuses it (WRK-003).
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
/// When Git contains an unambiguous clean descendant, checkpoint adopts it as authoritative (WRK-003).
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
/// When Work is archived, its request and complete Task ledger are preserved (WRK-001, WRK-002).
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
/// When Work starts, exactly one durable request source is required (WRK-001).
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

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "WRK-005 acceptance keeps ordering, transitions, checkpoint linkage, failure, and causal correction in one coherent journey"
)]
/// When Develop processes a request, stable IDs retain explicit order and causal correction (WRK-005, BLD-002, REV-001).
fn develop_should_process_ordered_sequence_and_causal_correction() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Implement the ordered Develop workflow.",
        "--title",
        "Develop Work",
        "--target",
        "main",
    ]);

    let first = repository.succeeds(&[
        "develop",
        "task",
        "add",
        "--title",
        "Implement domain",
        "--description",
        "Add the domain behavior.",
    ]);
    assert!(first.contains("TASK_001"));
    let second = repository.succeeds(&[
        "develop",
        "task",
        "add",
        "--title",
        "Add tests",
        "--description",
        "Cover the behavior.",
    ]);
    assert!(second.contains("TASK_002"));
    let inserted = repository.succeeds(&[
        "develop",
        "task",
        "add",
        "--title",
        "Wire command",
        "--description",
        "Expose the workflow through the CLI.",
        "--before",
        "TASK_002",
    ]);
    assert!(inserted.contains("TASK_003"));
    repository.succeeds(&[
        "develop",
        "task",
        "update",
        "TASK_002",
        "--title",
        "Add acceptance tests",
    ]);
    repository.succeeds(&[
        "develop", "task", "move", "TASK_002", "--before", "TASK_001",
    ]);

    let ordered = repository.succeeds(&["develop", "task", "list"]);
    let second_at = ordered.find("TASK_002").unwrap_or(usize::MAX);
    let first_at = ordered.find("TASK_001").unwrap_or(usize::MAX);
    let third_at = ordered.find("TASK_003").unwrap_or(usize::MAX);
    assert!(second_at < first_at && first_at < third_at);
    let next = repository.succeeds(&["work", "task", "next"]);
    assert!(next.contains("TASK_002"));
    assert!(next.contains("rapport develop task start TASK_002"));

    repository.succeeds(&["develop", "task", "start", "TASK_002"]);
    let (parallel_code, _, parallel_error) =
        repository.run(&["develop", "task", "start", "TASK_001"]);
    assert_eq!(parallel_code, ExitCode::from(2));
    assert!(parallel_error.contains("already running"));
    let no_file_result = repository.succeeds(&[
        "develop",
        "task",
        "complete",
        "TASK_002",
        "--result",
        "The existing coverage already proves the behavior.",
    ]);
    assert!(no_file_result.contains("status` — passed"));

    repository.succeeds(&["develop", "task", "start", "TASK_001"]);
    repository.write("src/develop.rs", "pub fn develop() {}\n");
    let (dirty_code, _, dirty_error) = repository.run(&[
        "develop",
        "task",
        "complete",
        "TASK_001",
        "--result",
        "Not checkpointed yet.",
    ]);
    assert_eq!(dirty_code, ExitCode::from(2));
    assert!(dirty_error.contains("clean worktree"));
    repository.succeeds(&["work", "checkpoint", "start"]);
    repository.git(["add", "src/develop.rs"]);
    repository.succeeds(&["work", "checkpoint", "complete", "Implement Develop domain"]);
    let completed = repository.succeeds(&[
        "develop",
        "task",
        "complete",
        "TASK_001",
        "--result",
        "Implemented the domain and checkpointed it.",
    ]);
    assert!(completed.contains("TASK_004"));
    let action = repository.succeeds(&["develop", "task", "show", "TASK_001"]);
    assert!(action.contains("TASK_004"));
    assert!(action.contains("initial_head"));
    assert!(action.contains("final_head"));

    repository.succeeds(&["develop", "task", "start", "TASK_003"]);
    let failed = repository.succeeds(&[
        "develop",
        "task",
        "fail",
        "TASK_003",
        "--result",
        "The command boundary needs a correction.",
    ]);
    assert!(failed.contains("status` — failed"));
    let correction_hint = repository.succeeds(&["work", "task", "next"]);
    assert!(correction_hint.contains("--caused-by TASK_003"));

    let correction = repository.succeeds(&[
        "develop",
        "task",
        "add",
        "--title",
        "Correct command boundary",
        "--description",
        "Address the failed command wiring.",
        "--caused-by",
        "TASK_003",
    ]);
    assert!(correction.contains("TASK_005"));
    let caused = repository.succeeds(&["develop", "task", "show", "TASK_005"]);
    assert!(caused.contains("TASK_003 failed action"));
    assert!(caused.contains("The command boundary needs a correction."));
    repository.succeeds(&["develop", "task", "start", "TASK_005"]);
    repository.succeeds(&[
        "develop",
        "task",
        "complete",
        "TASK_005",
        "--result",
        "The existing checkpoint contains the correction.",
    ]);

    repository.succeeds(&[
        "develop",
        "task",
        "add",
        "--title",
        "Obsolete follow-up",
        "--description",
        "This is no longer necessary.",
    ]);
    repository.succeeds(&[
        "develop",
        "task",
        "cancel",
        "TASK_006",
        "--reason",
        "Superseded by the correction.",
    ]);
    let status = repository.succeeds(&["work", "status"]);
    assert!(status.contains("Develop` — complete"));
    assert!(status.contains("next` — `rapport build`"));

    let (immutable_code, _, immutable_error) = repository.run(&[
        "develop",
        "task",
        "update",
        "TASK_001",
        "--title",
        "Rewrite history",
    ]);
    assert_eq!(immutable_code, ExitCode::from(2));
    assert!(immutable_error.contains("not a pending Develop Action Task"));
}

#[test]
/// When an Action fails, its immutable record blocks completion until causal Work is resolved (WRK-005).
fn develop_should_preserve_failed_task_until_explicit_resolution() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Preserve failed development history.",
        "--title",
        "Failed Work",
        "--target",
        "main",
    ]);
    repository.succeeds(&[
        "develop",
        "task",
        "add",
        "--title",
        "Attempt risky change",
        "--description",
        "Record failure without rewriting it.",
    ]);
    repository.succeeds(&["develop", "task", "start", "TASK_001"]);
    repository.succeeds(&[
        "develop",
        "task",
        "fail",
        "TASK_001",
        "--result",
        "The approach is not viable.",
    ]);
    let failed_path = repository.root.join(".rapport/tasks/TASK_001.toml");
    let before = assert_ok!(std::fs::read_to_string(&failed_path));

    let (complete_code, _, complete_error) =
        repository.run(&["work", "complete", "--result", "Should remain blocked."]);
    assert_eq!(complete_code, ExitCode::from(2));
    assert!(complete_error.contains("Develop is incomplete"));

    repository.succeeds(&[
        "develop",
        "task",
        "add",
        "--title",
        "Alternative correction",
        "--description",
        "Try a safer approach.",
        "--caused-by",
        "TASK_001",
    ]);
    let after = assert_ok!(std::fs::read_to_string(&failed_path));
    assert_eq!(after, before);
    repository.succeeds(&[
        "develop",
        "task",
        "cancel",
        "TASK_002",
        "--reason",
        "The request no longer needs this change.",
    ]);
    let status = repository.succeeds(&["work", "status"]);
    assert!(status.contains("Develop` — complete"));
}

#[test]
/// A clean candidate with no required operations still receives exact empty acceptance proof (BLD-002).
fn acceptance_build_passes_when_no_signoffs_are_required() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Prove an empty signoff set.",
        "--title",
        "Empty acceptance Build",
        "--target",
        "main",
    ]);

    let built = repository.succeeds(&["build"]);

    assert!(built.contains("status` — passed"), "{built}");
    assert!(built.contains("operations` — 0"), "{built}");
    let status = repository.succeeds(&["build", "status"]);
    assert!(status.contains("Build` — complete"), "{status}");
    assert!(status.contains("proof` — current"), "{status}");
    let next = repository.succeeds(&["work", "task", "next"]);
    assert!(next.contains("rapport review start"), "{next}");
    let task = repository.succeeds(&["build", "status", "TASK_001"]);
    assert!(task.contains("mode` — acceptance"), "{task}");
    assert!(task.contains("proof` — true"), "{task}");

    repository.write("later.txt", "candidate changed\n");
    repository.git(["add", "later.txt"]);
    repository.git(["commit", "-q", "-m", "change candidate after proof"]);
    let stale = repository.succeeds(&["build", "status"]);
    assert!(stale.contains("Build` — incomplete"), "{stale}");
    assert!(stale.contains("proof` — missing or stale"), "{stale}");
}

#[test]
/// Without Work, the conventional development target runs directly and creates no ledger (BLD-001).
fn build_without_work_runs_dev_without_inventing_state() {
    let repository = TemporaryRepository::new();
    let runner = QueueRunner::new([successful("development feedback\n")]);

    let output = repository.succeeds_with(&["build"], &runner);

    assert!(output.contains("mode` — feedback"), "{output}");
    assert!(output.contains("target` — dev"), "{output}");
    assert!(output.contains("proof` — none"), "{output}");
    assert!(!repository.root.join(".rapport/work.toml").is_file());
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, CommandSpec::new("just", ["dev"]));

    let (code, _, error) = repository.run_with(&["build", "../outside"], &runner);
    assert_eq!(code, ExitCode::from(2));
    assert!(error.contains("must stay inside the repository"), "{error}");
    assert_eq!(runner.calls().len(), 1);
}

#[test]
/// A failed stage blocks later stages and creates one corrective Develop Task per failed signoff (BLD-002, WRK-005).
fn failed_acceptance_stage_blocks_later_work_and_reopens_develop() {
    let repository = TemporaryRepository::new();
    repository.add_signoff("ci-fast", 0);
    repository.add_signoff("ci-later", 1);
    repository.git(["add", "-A"]);
    repository.git(["commit", "-q", "-m", "declare staged signoffs"]);
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Exercise staged acceptance failure.",
        "--title",
        "Stage Build operations",
        "--target",
        "main",
    ]);
    let runner = QueueRunner::new([failing("ci-fast failed\n")]);

    let (code, _, error) = repository.run_with(&["build"], &runner);

    assert_eq!(code, ExitCode::from(2));
    assert!(error.contains("Build Task `TASK_001` failed"), "{error}");
    assert_eq!(runner.calls().len(), 1);
    let task = repository.succeeds(&["build", "status", "TASK_001"]);
    assert!(task.contains("ROOT_SIGNOFF_CI_FAST"), "{task}");
    assert!(task.contains("status failed"), "{task}");
    assert!(task.contains("ROOT_SIGNOFF_CI_LATER"), "{task}");
    assert!(task.contains("status blocked"), "{task}");
    let develop = repository.succeeds(&["develop", "task", "list"]);
    assert_eq!(develop.matches("Repair failed Build signoff").count(), 1);
    assert!(develop.contains("TASK_002"), "{develop}");
}

#[test]
/// Ad hoc feedback records failure and dirty Git state without manufacturing corrective work (BLD-001).
fn ad_hoc_failure_remains_feedback_without_corrective_develop_task() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Collect development feedback.",
        "--title",
        "Ad hoc Build",
        "--target",
        "main",
    ]);
    repository.write("dirty.rs", "fn dirty() {}\n");
    let runner = QueueRunner::new([failing("feedback failed\n")]);

    let (code, _, error) = repository.run_with(&["build", ".", "--target", "ci"], &runner);

    assert_eq!(code, ExitCode::from(2));
    assert!(error.contains("Build Task `TASK_001` failed"), "{error}");
    let shown = repository.succeeds(&["build", "status", "TASK_001"]);
    assert!(shown.contains("mode` — feedback"), "{shown}");
    assert!(shown.contains("untracked dirty.rs"), "{shown}");
    assert!(shown.contains("proof` — false"), "{shown}");
    let tasks = repository.succeeds(&["work", "task", "list", "--all"]);
    assert_eq!(tasks.matches("TASK_").count(), 1, "{tasks}");
}

#[test]
/// Build-generated source changes invalidate passing operations and become explicit corrective work (BLD-002).
fn acceptance_build_generated_changes_invalidate_proof() {
    let repository = TemporaryRepository::new();
    repository.add_signoff("ci", 0);
    repository.git(["add", "-A"]);
    repository.git(["commit", "-q", "-m", "declare signoff"]);
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Reject build-generated changes.",
        "--title",
        "Clean acceptance Build",
        "--target",
        "main",
    ]);

    let (code, _, error) = repository.run_with(&["build"], &MutatingRunner);

    assert_eq!(code, ExitCode::from(2));
    assert!(error.contains("Build Task `TASK_001` failed"), "{error}");
    let shown = repository.succeeds(&["build", "status", "TASK_001"]);
    assert!(shown.contains("untracked generated.txt"), "{shown}");
    assert!(shown.contains("proof` — false"), "{shown}");
    let develop = repository.succeeds(&["develop", "task", "list"]);
    assert!(
        develop.contains("Reconcile build-generated changes"),
        "{develop}"
    );
}

#[test]
/// Acceptance Review keeps the minimum private and binds passing proof to the exact candidate (REV-001, WRK-005).
fn acceptance_review_passes_and_routes_work_to_integrate() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Review the candidate.",
        "--title",
        "Acceptance Review",
        "--target",
        "main",
    ]);
    repository.succeeds(&["build"]);

    let request = repository.succeeds(&["review", "start"]);
    assert!(request.contains("Rapport Independent Review"), "{request}");
    assert!(!request.contains("effective review minimum"), "{request}");
    let result_path = std::env::temp_dir().join(format!(
        "rapport-review-result-{}.json",
        NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
    ));
    assert_ok!(std::fs::write(
        &result_path,
        review_result(request_checksum(&request), "A", false)
    ));
    let result_path_string = result_path.to_string_lossy().into_owned();
    let completed = repository.succeeds(&["review", "complete", "--result", &result_path_string]);
    assert!(completed.contains("status` — passed"), "{completed}");
    assert!(completed.contains("proof` — true"), "{completed}");
    let next = repository.succeeds(&["work", "task", "next"]);
    assert!(next.contains("rapport integrate start"), "{next}");
    let _ = std::fs::remove_file(result_path);
}

#[test]
/// Review findings receive durable IDs and require an explicit risk or corrective-work decision (REV-001).
fn review_finding_dismissal_records_reason_and_completes_policy() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Review findings.",
        "--title",
        "Finding Review",
        "--target",
        "main",
    ]);
    repository.succeeds(&["build"]);
    let request = repository.succeeds(&["review", "start"]);
    let result_path = std::env::temp_dir().join(format!(
        "rapport-review-finding-{}.json",
        NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
    ));
    assert_ok!(std::fs::write(
        &result_path,
        review_result(request_checksum(&request), "B", true)
    ));
    let result_path_string = result_path.to_string_lossy().into_owned();
    let blocked = repository.succeeds(&["review", "complete", "--result", &result_path_string]);
    assert!(blocked.contains("REV_001"), "{blocked}");
    assert!(blocked.contains("status` — blocked"), "{blocked}");
    let dismissed = repository.succeeds(&[
        "review",
        "reconcile",
        "REV_001",
        "--dismiss",
        "--reason",
        "The current behavior is intentionally narrow.",
    ]);
    assert!(dismissed.contains("status` — passed"), "{dismissed}");
    assert!(dismissed.contains("proof` — true"), "{dismissed}");
    let _ = std::fs::remove_file(result_path);
}

use super::Error;
use super::domain::{
    RequestKind, RequestSource, Task, TaskStatus, Work, WorkOutcomeKind, Workflow,
};
use super::history::HistoryStore;
use super::repository::Store;
use crate::{Clock, CommandOutcome, CommandRunner, CommandSpec, run_with_environment};
use claims::{assert_err, assert_ok, assert_some};
use rapport_files::{FileSystem, InMemoryFileSystem, RealFileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::{BranchName, ObjectId};
use rstest::rstest;
use std::collections::VecDeque;
use std::io;
use std::process::{Command, ExitCode};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);
const INITIAL_OBJECT_ID: &str = "1111111111111111111111111111111111111111";
const CHECKPOINT_OBJECT_ID: &str = "2222222222222222222222222222222222222222";
const MERGE_OBJECT_ID: &str = "feedface1234feedface1234feedface1234feed";

fn branch(value: &str) -> BranchName {
    assert_ok!(BranchName::new(value))
}

fn object_id(value: &str) -> ObjectId {
    assert_ok!(ObjectId::new(value))
}

fn stored_work() -> Work {
    assert_ok!(Work::new(
        "Stored Work".to_owned(),
        "Reject invalid persisted Git identities.".to_owned(),
        RequestSource {
            kind: RequestKind::AdHoc,
            value: "Exercise the persistence boundary.".to_owned(),
        },
        "/repository".to_owned(),
        branch("feature"),
        branch("main"),
        object_id(INITIAL_OBJECT_ID),
        object_id(CHECKPOINT_OBJECT_ID),
        "2026-07-12T23:00:00Z".to_owned(),
    ))
}

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

fn accepted_work(repository: &TemporaryRepository) -> Work {
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Integrate one exact accepted candidate.",
        "--title",
        "Integrate accepted Work",
        "--target",
        "main",
    ]);
    repository.write("candidate.txt", "accepted candidate\n");
    repository.succeeds(&["work", "checkpoint", "start"]);
    repository.git(["add", "candidate.txt"]);
    repository.succeeds(&["work", "checkpoint", "complete", "Add accepted candidate"]);
    repository.succeeds(&["build"]);
    let request = repository.succeeds(&["review", "start"]);
    let result_path = std::env::temp_dir().join(format!(
        "rapport-integration-review-{}-{}.json",
        std::process::id(),
        NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed)
    ));
    assert_ok!(std::fs::write(
        &result_path,
        review_result(request_checksum(&request), "A", false)
    ));
    let result_path = result_path.to_string_lossy().into_owned();
    repository.succeeds(&["review", "complete", "--result", &result_path]);
    let _ = std::fs::remove_file(result_path);
    let fs = RealFileSystem;
    assert_ok!(Store::new(&repository.root).require_work(&fs))
}

fn integration_pull_request(repository: &TemporaryRepository, work: &Work, head: &str) -> String {
    serde_json::json!({
        "number": 115,
        "url": "https://github.com/hedge-ops/rapport/pull/115",
        "body": format!("Evidence\n\n<!-- Rapport-Work: {} -->", work.id),
        "headRefOid": head,
        "headRefName": "feature",
        "baseRefOid": repository.git(["rev-parse", "main"]),
        "baseRefName": "main",
        "isCrossRepository": false,
        "state": "OPEN",
        "mergeable": "MERGEABLE",
        "mergeStateStatus": "CLEAN",
        "reviewDecision": "",
        "reviewRequests": [],
        "statusCheckRollup": [
            {
                "__typename": "StatusContext",
                "context": "Rapport Build",
                "state": "SUCCESS"
            },
            {
                "__typename": "CheckRun",
                "name": "CI",
                "status": "COMPLETED",
                "conclusion": "SUCCESS"
            }
        ],
        "mergeCommit": null
    })
    .to_string()
}

fn github_target(repository: &TemporaryRepository) -> CommandOutcome {
    successful(&format!("{}\n", repository.git(["rev-parse", "main"])))
}

fn integration_start_runner(repository: &TemporaryRepository, pull_request: &str) -> QueueRunner {
    QueueRunner::new([
        successful(r#"{"nameWithOwner":"hedge-ops/rapport"}"#),
        successful("[]"),
        github_target(repository),
        successful("{}"),
        successful("[]"),
        successful("https://github.com/hedge-ops/rapport/pull/115\n"),
        successful(pull_request),
    ])
}

#[derive(Debug)]
struct TemporaryRepository {
    root: Utf8PathBuf,
    remote: Option<Utf8PathBuf>,
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
        let repository = Self { root, remote: None };
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

    fn use_bare_origin(&mut self) {
        let remote = Utf8PathBuf::from(format!("{}-remote.git", self.root));
        let output = assert_ok!(
            Command::new("git")
                .args(["init", "--bare", "-q", remote.as_str()])
                .output()
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        self.git(["remote", "set-url", "origin", remote.as_str()]);
        self.git(["push", "-q", "origin", "main"]);
        self.git(["push", "-q", "-u", "origin", "feature"]);
        self.remote = Some(remote);
    }
}

impl Drop for TemporaryRepository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        if let Some(remote) = &self.remote {
            let _ = std::fs::remove_dir_all(remote);
        }
    }
}

#[test]
/// When persisted Work contains an invalid branch, loading fails instead of using a fallback.
fn load_work_should_reject_invalid_branch_names_without_fallback() {
    let mut fs = InMemoryFileSystem::default();
    let store = Store::new("/repository");
    assert_ok!(store.save_work(&mut fs, &stored_work()));
    let path = Utf8Path::new("/repository/.rapport/work.toml");
    let valid = assert_ok!(fs.read_to_string(path));

    for invalid in [
        "-option",
        "feature name",
        "feature..other",
        "feature@{upstream}",
        "feature/.hidden",
        "feature.lock",
    ] {
        let corrupted = valid.replace(
            "source_branch = \"feature\"",
            &format!("source_branch = \"{invalid}\""),
        );
        assert_ok!(fs.write_string(path, corrupted));
        let error = assert_err!(store.load_work(&fs));
        assert!(
            matches!(error, Error::BranchName(_)),
            "expecting invalid stored branch {invalid:?} to fail explicitly, got {error:?}"
        );
    }
}

#[test]
/// When persisted Work contains an invalid object ID, loading fails before returning domain state.
fn load_work_should_reject_invalid_object_identifiers() {
    let mut fs = InMemoryFileSystem::default();
    let store = Store::new("/repository");
    assert_ok!(store.save_work(&mut fs, &stored_work()));
    let path = Utf8Path::new("/repository/.rapport/work.toml");
    let valid = assert_ok!(fs.read_to_string(path));
    let corrupted = valid.replace(
        &format!("starting_source = \"{INITIAL_OBJECT_ID}\""),
        "starting_source = \"not-an-oid\"",
    );
    assert_ok!(fs.write_string(path, corrupted));

    let error = assert_err!(store.load_work(&fs));

    assert!(
        matches!(error, Error::ObjectId(_)),
        "expecting an invalid stored object identifier to fail explicitly, got {error:?}"
    );
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
        "/repository".to_owned(),
        branch("feature"),
        branch("main"),
        object_id(INITIAL_OBJECT_ID),
        object_id(INITIAL_OBJECT_ID),
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
    assert_ok!(work.finish(
        WorkOutcomeKind::Completed,
        "2026-07-12T23:00:08Z".to_owned(),
        "Preserved the complete local ledger.".to_owned(),
        object_id(CHECKPOINT_OBJECT_ID),
        object_id(INITIAL_OBJECT_ID),
    ));
    assert_ok!(store.save_work_and_task(&mut fs, &work, &task));

    let history_store = assert_ok!(HistoryStore::new(Utf8Path::new("/repository")));
    let history =
        assert_ok!(history_store.archive(&mut fs, &store, &work, std::slice::from_ref(&task)));

    assert!(fs.is_file(history.join("work.toml")));
    assert!(fs.is_file(history.join("tasks/TASK_001.toml")));
    assert!(!fs.is_file("/repository/.rapport/work.toml"));
    assert!(!fs.is_file("/repository/.rapport/tasks/TASK_001.toml"));
    let archived_task = assert_ok!(fs.read_to_string(history.join("tasks/TASK_001.toml")));
    assert!(archived_task.contains("duration_seconds = \"8\""));
}

#[test]
/// When finalized Work is inspected or removed, history stays global and destructive actions require confirmation (WRK-006).
fn work_history_should_list_show_and_remove_finalized_work() {
    let repository = TemporaryRepository::new();
    repository.succeeds(&[
        "work",
        "start",
        "--ad-hoc",
        "Record an abandoned experiment.",
        "--title",
        "Abandoned experiment",
        "--target",
        "main",
    ]);
    let fs = RealFileSystem;
    let first = assert_ok!(Store::new(&repository.root).require_work(&fs));
    repository.succeeds(&[
        "work",
        "abandon",
        "--reason",
        "The experiment did not support the product direction.",
    ]);

    let listed = repository.succeeds(&["work", "history", "list"]);
    let first_prefix = first.id.to_string().chars().take(6).collect::<String>();
    assert!(
        listed.contains(&first_prefix),
        "expecting list to use the six-character Work prefix: {listed}"
    );
    assert!(
        listed.contains("Abandoned experiment"),
        "expecting list to preserve the human title: {listed}"
    );
    let shown = repository.succeeds(&["work", "history", "show", &first.id.to_string()]);
    assert!(
        shown.contains("outcome` — abandoned"),
        "expecting show to identify the final outcome: {shown}"
    );
    assert!(
        shown.contains("The experiment did not support the product direction."),
        "expecting show to preserve the human reason: {shown}"
    );
    assert!(
        shown.contains(repository.root.as_str()),
        "expecting show to identify the source repository and raw archive: {shown}"
    );

    let preview = repository.succeeds(&["work", "history", "remove", &first.id.to_string()]);
    assert!(
        preview.contains("removed` — false"),
        "expecting remove to preview before permanent deletion: {preview}"
    );
    assert!(
        repository
            .root
            .join(format!(".rapport/test-history/work/{}/work.toml", first.id))
            .is_file(),
        "expecting preview to preserve the historical record"
    );
    repository.succeeds(&[
        "work",
        "history",
        "remove",
        &first.id.to_string(),
        "--confirm",
    ]);
    let empty = repository.succeeds(&["work", "history", "list"]);
    assert!(
        empty.contains("none"),
        "expecting confirmed removal to leave no records: {empty}"
    );
    assert_eq!(
        repository.git(["status", "--short"]),
        "",
        "expecting history removal to leave repository and Git state unchanged"
    );
}

#[test]
/// When all Work History is cleared, Rapport reports the count and requires explicit confirmation (WRK-006).
fn work_history_clear_should_preview_and_remove_every_record() {
    let repository = TemporaryRepository::new();
    for title in ["First retained Work", "Second retained Work"] {
        repository.succeeds(&[
            "work",
            "start",
            "--ad-hoc",
            "Retain this result until history is cleared.",
            "--title",
            title,
            "--target",
            "main",
        ]);
        repository.succeeds(&[
            "work",
            "abandon",
            "--reason",
            "Recorded for the clear operation.",
        ]);
    }
    let clear_preview = repository.succeeds(&["work", "history", "clear"]);
    assert!(
        clear_preview.contains("records` — 2"),
        "expecting clear to report the number of permanent removals: {clear_preview}"
    );
    assert!(
        clear_preview.contains("removed` — false"),
        "expecting clear to require explicit confirmation: {clear_preview}"
    );
    repository.succeeds(&["work", "history", "clear", "--confirm"]);
    let cleared = repository.succeeds(&["work", "history", "list"]);
    assert!(
        cleared.contains("none"),
        "expecting confirmed clear to remove all Work History: {cleared}"
    );
    assert_eq!(
        repository.git(["status", "--short"]),
        "",
        "expecting history removal to leave repository and Git state unchanged"
    );
}

#[test]
/// When completed Work is shown, its exact candidate, Build proof, Review grade, and Task prose remain inspectable (WRK-006).
fn work_history_show_should_render_complete_build_and_review_evidence() {
    let repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    let candidate = assert_some!(work.latest_checkpoint.clone());

    repository.succeeds(&[
        "work",
        "complete",
        "--result",
        "The accepted local-only candidate is complete.",
    ]);
    let shown = repository.succeeds(&["work", "history", "show", &work.id.to_string()]);

    assert!(
        shown.contains("outcome` — completed"),
        "expecting the explicit completion outcome: {shown}"
    );
    assert!(
        shown.contains(&format!("final source` — {candidate}")),
        "expecting the exact final candidate identity: {shown}"
    );
    assert!(
        shown.contains("### Build evidence"),
        "expecting the complete Build Task evidence: {shown}"
    );
    assert!(
        shown.contains("proof` — true"),
        "expecting accepted Build and Review proof: {shown}"
    );
    assert!(
        shown.contains("### Review evidence"),
        "expecting the complete Review Task evidence: {shown}"
    );
    assert!(
        shown.contains("grade` — A"),
        "expecting the independent Review grade: {shown}"
    );
    assert!(
        shown.contains("The accepted local-only candidate is complete."),
        "expecting the human completion prose: {shown}"
    );
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

    let started = repository.succeeds(&["develop", "task", "start", "TASK_001"]);
    assert!(
        started.contains("if repository state changes, checkpoint it"),
        "expecting started source work to require a checkpoint only for repository changes"
    );
    assert!(
        started.contains("rapport develop task complete TASK_001 --result <RESULT>"),
        "expecting started source work to show its exact completion command"
    );
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
    let next = repository.succeeds(&["work", "task", "next"]);
    assert!(
        next.contains("appropriate engineering correction"),
        "expecting Build repair guidance to allow any engineering correction: {next}"
    );
    assert!(
        next.contains("If repository state changes"),
        "expecting Build repair guidance to make checkpointing conditional: {next}"
    );
    assert!(
        next.contains("rapport develop task start TASK_002"),
        "expecting a pending Build repair to show its exact start command: {next}"
    );

    let started = repository.succeeds(&["develop", "task", "start", "TASK_002"]);
    assert!(
        started.contains("if repository state changes, checkpoint it"),
        "expecting an environmental repair to permit completion without a checkpoint"
    );
    assert!(
        started.contains("rapport develop task complete TASK_002 --result <RESULT>"),
        "expecting a started Build repair to show its exact completion command"
    );
    let completed = repository.succeeds(&[
        "develop",
        "task",
        "complete",
        "TASK_002",
        "--result",
        "Corrected the execution environment and reran ci-fast successfully.",
    ]);
    assert!(
        completed.contains("checkpoints` — none"),
        "expecting an environmental repair to complete without a checkpoint: {completed}"
    );
    assert!(
        completed.contains("next` — `rapport build`"),
        "expecting the completed Build repair to return to acceptance Build: {completed}"
    );
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
    assert!(
        request.contains("Delegate this request to a fresh independent review agent"),
        "{request}"
    );
    assert!(
        request.contains("The implementing agent must not review or certify its own candidate"),
        "{request}"
    );
    assert!(!request.contains("effective review minimum"), "{request}");
    let result_path = std::env::temp_dir().join(format!(
        "rapport-review-result-{}-{}.json",
        std::process::id(),
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
        "rapport-review-finding-{}-{}.json",
        std::process::id(),
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

#[test]
/// Start publishes exact Build proof and creates the aggregate Review-carrying pull request (INT-001, REV-002).
fn integration_start_publishes_the_accepted_candidate() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let pull_request = integration_pull_request(&repository, &work, &head);
    let runner = QueueRunner::new([
        successful(r#"{"nameWithOwner":"hedge-ops/rapport"}"#),
        successful("[]"),
        github_target(&repository),
        successful("{}"),
        successful("[]"),
        successful("https://github.com/hedge-ops/rapport/pull/115\n"),
        successful(&pull_request),
    ]);

    let output = repository.succeeds_with(&["integrate", "start"], &runner);

    assert!(output.contains("stage` — Published"), "{output}");
    assert!(output.contains("blockers` — none"), "{output}");
    assert!(output.contains("rapport integrate complete"), "{output}");
    let fs = RealFileSystem;
    let tasks = assert_ok!(Store::new(&repository.root).load_tasks(&fs));
    let Some(task) = tasks.last() else {
        panic!("Integration Task was not recorded")
    };
    let Some(integration) = task.integration.as_ref() else {
        panic!("typed Integration payload was not recorded")
    };
    assert_eq!(integration.candidate, head);
    assert!(integration.pushed);
    assert!(integration.aggregate_build_published);
    assert_eq!(integration.pull_request_number, Some(115));
    assert_eq!(integration.review_task, "TASK_003");
    let calls = runner.calls();
    let Some(create) = calls.iter().find(|(spec, _)| {
        spec.args
            .starts_with(&["pr".to_owned(), "create".to_owned()])
    }) else {
        panic!("pull request creation was not recorded")
    };
    let Some(body) = create.0.args.last() else {
        panic!("pull request body was not recorded")
    };
    assert!(body.contains("Independent Review"));
    assert!(body.contains("Rapport-Work"));
    assert!(!body.contains("Rapport Review status"));
    assert!(!calls.iter().any(|(spec, _)| {
        spec.args
            .iter()
            .any(|argument| argument.contains("/rules/branches/"))
    }));
}

#[test]
/// Start waits for remote checks to appear instead of treating local status publication as CI (INT-001).
fn integration_start_should_block_when_no_remote_checks_are_observed() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let mut pull_request: serde_json::Value = assert_ok!(serde_json::from_str(
        &integration_pull_request(&repository, &work, &head)
    ));
    pull_request["statusCheckRollup"] = serde_json::json!([{
        "__typename": "StatusContext",
        "context": "Rapport Build",
        "state": "SUCCESS"
    }]);
    let runner = integration_start_runner(&repository, &pull_request.to_string());

    let output = repository.succeeds_with(&["integrate", "start"], &runner);

    assert!(
        output.contains("no remote checks observed"),
        "expecting integration to wait for a remote check run: {output}"
    );
}

#[rstest]
#[case::pending("IN_PROGRESS", "", "remote check(s) pending")]
#[case::failed("COMPLETED", "FAILURE", "remote check(s) failed")]
/// Start waits for every observed remote check to finish without failure (INT-001).
fn integration_start_should_block_nonpassing_remote_checks(
    #[case] status: &str,
    #[case] conclusion: &str,
    #[case] blocker: &str,
) {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let mut pull_request: serde_json::Value = assert_ok!(serde_json::from_str(
        &integration_pull_request(&repository, &work, &head)
    ));
    pull_request["statusCheckRollup"][1]["status"] = serde_json::Value::String(status.to_owned());
    pull_request["statusCheckRollup"][1]["conclusion"] =
        serde_json::Value::String(conclusion.to_owned());
    let runner = integration_start_runner(&repository, &pull_request.to_string());

    let output = repository.succeeds_with(&["integrate", "start"], &runner);

    assert!(
        output.contains(blocker),
        "expecting a nonpassing remote check to block integration: {output}"
    );
}

#[test]
/// GitHub policy does not choose optional review or target freshness for Rapport (INT-001, INT-002).
fn integration_start_should_ignore_policy_only_review_and_merge_blocks() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let mut pull_request: serde_json::Value = assert_ok!(serde_json::from_str(
        &integration_pull_request(&repository, &work, &head)
    ));
    pull_request["baseRefOid"] = serde_json::Value::String("target-advanced".to_owned());
    pull_request["reviewDecision"] = serde_json::Value::String("REVIEW_REQUIRED".to_owned());
    pull_request["mergeStateStatus"] = serde_json::Value::String("BLOCKED".to_owned());
    let runner = integration_start_runner(&repository, &pull_request.to_string());

    let output = repository.succeeds_with(&["integrate", "start"], &runner);

    assert!(output.contains("target advanced` — true"), "{output}");
    assert!(output.contains("blockers` — none"), "{output}");
}

#[test]
/// An explicit reviewer request for changes remains a blocker even without an approval policy (INT-001).
fn integration_start_should_block_explicit_requested_changes() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let mut pull_request: serde_json::Value = assert_ok!(serde_json::from_str(
        &integration_pull_request(&repository, &work, &head)
    ));
    pull_request["reviewDecision"] = serde_json::Value::String("CHANGES_REQUESTED".to_owned());
    let runner = integration_start_runner(&repository, &pull_request.to_string());

    let output = repository.succeeds_with(&["integrate", "start"], &runner);

    assert!(
        output.contains("changes are requested"),
        "expecting explicit human review feedback to block integration: {output}"
    );
}

#[test]
/// A review explicitly requested by the developer remains pending without a repository approval rule (INT-001).
fn integration_start_should_block_explicit_pending_review() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let mut pull_request: serde_json::Value = assert_ok!(serde_json::from_str(
        &integration_pull_request(&repository, &work, &head)
    ));
    pull_request["reviewDecision"] = serde_json::Value::String("REVIEW_REQUIRED".to_owned());
    pull_request["reviewRequests"] = serde_json::json!([{"login": "reviewer"}]);
    let runner = integration_start_runner(&repository, &pull_request.to_string());

    let output = repository.succeeds_with(&["integrate", "start"], &runner);

    assert!(
        output.contains("1 requested review(s) pending"),
        "expecting a developer-requested review to block integration: {output}"
    );
}

#[test]
/// Retrying interrupted publication adopts the owned pull request without repeating durable side effects (INT-002).
fn integration_start_resumes_without_duplicate_publication() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let pull_request = integration_pull_request(&repository, &work, &head);
    let first = QueueRunner::new([
        successful(r#"{"nameWithOwner":"hedge-ops/rapport"}"#),
        successful("[]"),
        github_target(&repository),
        successful("{}"),
        successful("[]"),
        failing("connection closed after request creation"),
    ]);

    let (code, _, error) = repository.run_with(&["integrate", "start"], &first);
    assert_eq!(code, ExitCode::from(2));
    assert!(error.contains("connection closed"), "{error}");

    let second = QueueRunner::new([
        successful(r#"{"nameWithOwner":"hedge-ops/rapport"}"#),
        successful(&format!("[{pull_request}]")),
    ]);
    let output = repository.succeeds_with(&["integrate", "start"], &second);

    assert!(output.contains("pull/115"), "{output}");
    let calls = second.calls();
    assert_eq!(calls.len(), 2);
    assert!(!calls.iter().any(|(spec, _)| {
        spec.args.iter().any(|argument| argument == "state=success")
            || spec
                .args
                .starts_with(&["pr".to_owned(), "create".to_owned()])
    }));
}

#[test]
/// Status reports a changed remote head without mutating GitHub or local Work (INT-001, INT-002).
fn integration_status_is_read_only_and_reports_changed_head() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let pull_request = integration_pull_request(&repository, &work, &head);
    let start = QueueRunner::new([
        successful(r#"{"nameWithOwner":"hedge-ops/rapport"}"#),
        successful("[]"),
        github_target(&repository),
        successful("{}"),
        successful("[]"),
        successful("https://github.com/hedge-ops/rapport/pull/115\n"),
        successful(&pull_request),
    ]);
    repository.succeeds_with(&["integrate", "start"], &start);
    let changed = integration_pull_request(&repository, &work, "deadbeef");
    let status = QueueRunner::new([successful(&changed)]);

    let output = repository.succeeds_with(&["integrate", "status"], &status);

    assert!(output.contains("pull-request head changed"), "{output}");
    assert_eq!(status.calls().len(), 1);
    let fs = RealFileSystem;
    assert!(assert_ok!(Store::new(&repository.root).load_work(&fs)).is_some());
}

#[test]
/// Cancellation closes only the owned PR, deletes its remote branch, and preserves the local Work candidate (INT-002).
fn integration_cancel_preserves_local_work() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let pull_request = integration_pull_request(&repository, &work, &head);
    let start = QueueRunner::new([
        successful(r#"{"nameWithOwner":"hedge-ops/rapport"}"#),
        successful("[]"),
        github_target(&repository),
        successful("{}"),
        successful("[]"),
        successful("https://github.com/hedge-ops/rapport/pull/115\n"),
        successful(&pull_request),
    ]);
    repository.succeeds_with(&["integrate", "start"], &start);
    let cancel = QueueRunner::new([successful(&pull_request), successful("")]);

    let output = repository.succeeds_with(
        &[
            "integrate",
            "cancel",
            "--reason",
            "The candidate needs another product decision.",
        ],
        &cancel,
    );

    assert!(output.contains("local Work preserved` — true"), "{output}");
    assert_eq!(repository.git(["branch", "--show-current"]), "feature");
    assert!(repository.root.join(".rapport/work.toml").is_file());
    assert!(
        repository
            .git(["ls-remote", "--heads", "origin", "feature"])
            .is_empty()
    );
}

#[test]
/// Completion revalidates, squash-merges, archives Work, and leaves the local branch checked out (INT-001, INT-002).
fn integration_complete_archives_without_switching_local_git() {
    let mut repository = TemporaryRepository::new();
    let work = accepted_work(&repository);
    repository.use_bare_origin();
    let head = repository.git(["rev-parse", "HEAD"]);
    let pull_request = integration_pull_request(&repository, &work, &head);
    let start = QueueRunner::new([
        successful(r#"{"nameWithOwner":"hedge-ops/rapport"}"#),
        successful("[]"),
        github_target(&repository),
        successful("{}"),
        successful("[]"),
        successful("https://github.com/hedge-ops/rapport/pull/115\n"),
        successful(&pull_request),
    ]);
    repository.succeeds_with(&["integrate", "start"], &start);
    let mut merged: serde_json::Value = assert_ok!(serde_json::from_str(&pull_request));
    merged["state"] = serde_json::Value::String("MERGED".to_owned());
    merged["mergeCommit"] = serde_json::json!({"oid": MERGE_OBJECT_ID});
    let complete = QueueRunner::new([
        successful(&pull_request),
        successful(""),
        successful(&merged.to_string()),
    ]);

    let output = repository.succeeds_with(&["integrate", "complete"], &complete);

    assert!(
        output.contains(MERGE_OBJECT_ID),
        "expecting completion to report the full merge object ID: {output}"
    );
    assert!(!repository.root.join(".rapport/work.toml").is_file());
    assert!(
        repository
            .root
            .join(format!(".rapport/test-history/work/{}/work.toml", work.id))
            .is_file()
    );
    assert_eq!(repository.git(["branch", "--show-current"]), "feature");
    assert!(
        repository
            .git(["ls-remote", "--heads", "origin", "feature"])
            .is_empty()
    );
    let history = repository.succeeds(&["work", "history", "show", &work.id.to_string()]);
    assert!(
        history.contains("outcome` — integrated"),
        "expecting Integration to finalize Work as integrated: {history}"
    );
    assert!(
        history.contains(&format!("final target` — {MERGE_OBJECT_ID}")),
        "expecting history to retain the confirmed squash commit: {history}"
    );
    assert!(
        history.contains("pull request` — #115"),
        "expecting history to retain the Integration identity: {history}"
    );
}

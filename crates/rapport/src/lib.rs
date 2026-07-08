mod build;
mod cli;
mod context;
mod init;
mod integrate;
mod paths;
mod rules;
mod runner;
mod state;
mod telemetry;
mod view;
mod work;

pub use context::{Clock, CommandContext, SystemClock, find_repo_root};
pub use paths::RapportPaths;
pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};
pub use state::{
    WORK_STATE_SCHEMA_VERSION, WorkFact, WorkStage, WorkState, WorkStateError, WorkStateStore,
    WorkStatus,
};
pub use telemetry::{
    CommandEvent, CommandEventOutcome, EVENT_SCHEMA_VERSION, TelemetryError, TelemetryWriter,
};
pub use view::{Outcome, RunHint, View, ViewBuilder};

use clap::{CommandFactory, Parser, error::ErrorKind};
use cli::{Cli, Command, WorkCommand, WorkRulesCommand};
use rapport_files::{FileSystem, RealFileSystem, Utf8PathBuf};
use std::io::Write;
use std::process::ExitCode;

/// Run the current `rapport` binary entrypoint.
pub fn run<I, O, E>(argv: I, runner: &dyn CommandRunner, out: &mut O, err: &mut E) -> ExitCode
where
    I: IntoIterator<Item = String>,
    O: Write,
    E: Write,
{
    let cwd = match current_utf8_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            let _ = writeln!(err, "{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut fs = RealFileSystem;
    let clock = SystemClock;
    run_with_environment(argv, runner, &mut fs, &clock, cwd, out, err)
}

fn run_with_environment<I, F, C, O, E>(
    argv: I,
    runner: &dyn CommandRunner,
    fs: &mut F,
    clock: &C,
    cwd: Utf8PathBuf,
    out: &mut O,
    err: &mut E,
) -> ExitCode
where
    I: IntoIterator<Item = String>,
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let arguments: Vec<String> = argv.into_iter().collect();
    if arguments.is_empty() {
        let _ = write!(out, "{}", Cli::command().render_help());
        return ExitCode::SUCCESS;
    }
    match Cli::try_parse_from(std::iter::once(String::from("rapport")).chain(arguments.clone())) {
        Ok(cli) => {
            let mut context = CommandContext::new(cwd, fs, clock, runner, out, err);
            execute_command(&cli, arguments, &mut context)
        }
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            let _ = write!(out, "{error}");
            ExitCode::SUCCESS
        }
        Err(error) if error.kind() == ErrorKind::DisplayVersion => {
            let _ = write!(out, "{error}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = write!(err, "{error}");
            ExitCode::from(2)
        }
    }
}

fn current_utf8_dir() -> Result<Utf8PathBuf, String> {
    let cwd =
        std::env::current_dir().map_err(|error| format!("cannot read current dir: {error}"))?;
    Utf8PathBuf::from_path_buf(cwd)
        .map_err(|path| format!("current dir is not valid UTF-8: {}", path.to_string_lossy()))
}

fn execute_command<F, C, O, E>(
    cli: &Cli,
    argv: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match &cli.command {
        Command::Init => init::run(argv, context),
        Command::Work(work_args) => match &work_args.command {
            WorkCommand::Status => work::status(argv, context),
            WorkCommand::Start(start_args) => work::start(start_args, argv, context),
            WorkCommand::Complete(complete_args) => work::complete(complete_args, argv, context),
            WorkCommand::Rules(rules_args) => match &rules_args.command {
                WorkRulesCommand::List { path } => rules::list(path.as_ref(), argv, context),
                WorkRulesCommand::Show { id } => rules::show(id, argv, context),
            },
            WorkCommand::Add(add_args) => work::add(&add_args.command, argv, context),
        },
        Command::Build(build_args) => build::run(build_args, argv, context),
        Command::Integrate(integrate_args) => integrate::run(integrate_args, argv, context),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::io;

    #[derive(Debug)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn now_rfc3339(&self) -> String {
            String::from("2026-07-07T23:00:00Z")
        }
    }

    #[derive(Debug)]
    struct NeverRunner;

    impl CommandRunner for NeverRunner {
        fn run(
            &self,
            _spec: &CommandSpec,
            _cwd: &rapport_files::Utf8Path,
        ) -> io::Result<CommandOutcome> {
            panic!("placeholder CLI must not run external commands");
        }
    }

    fn run_with(args: &[&str]) -> (ExitCode, String, String) {
        let mut fs = InMemoryFileSystem::default();
        run_with_fs(args, &mut fs)
    }

    fn run_with_fs(args: &[&str], fs: &mut InMemoryFileSystem) -> (ExitCode, String, String) {
        run_with_runner(args, fs, &NeverRunner)
    }

    fn run_with_runner(
        args: &[&str],
        fs: &mut InMemoryFileSystem,
        runner: &dyn CommandRunner,
    ) -> (ExitCode, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        fs.add_directory("/repo/.git");
        let code = run_with_environment(
            args.iter().map(|arg| (*arg).to_string()),
            runner,
            fs,
            &FixedClock,
            Utf8PathBuf::from("/repo"),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[derive(Debug)]
    struct FakeRunner {
        outcomes: RefCell<VecDeque<io::Result<CommandOutcome>>>,
        calls: RefCell<Vec<(CommandSpec, Utf8PathBuf)>>,
    }

    impl FakeRunner {
        fn with_outcomes(outcomes: impl IntoIterator<Item = io::Result<CommandOutcome>>) -> Self {
            Self {
                outcomes: RefCell::new(outcomes.into_iter().collect()),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn successful(stdout: &str) -> Self {
            Self::with_outcomes([Ok(CommandOutcome {
                success: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
            })])
        }

        fn failing(stderr: &str) -> Self {
            Self::with_outcomes([Ok(CommandOutcome {
                success: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
            })])
        }

        fn calls(&self) -> Vec<(CommandSpec, Utf8PathBuf)> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            spec: &CommandSpec,
            cwd: &rapport_files::Utf8Path,
        ) -> io::Result<CommandOutcome> {
            self.calls
                .borrow_mut()
                .push((spec.clone(), cwd.to_path_buf()));
            self.outcomes.borrow_mut().pop_front().unwrap()
        }
    }

    #[test]
    fn no_args_renders_root_help() {
        let (code, out, err) = run_with(&[]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("repository rapport for human-directed agent work"));
        assert!(out.contains("work -> build -> integrate"));
        assert!(out.contains("work"));
        assert_eq!(err, "");
    }

    #[test]
    fn help_flag_renders_root_help() {
        let (code, out, err) = run_with(&["--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Rapport keeps human-directed agent work grounded"));
        assert!(out.contains("work -> build -> integrate"));
        assert_eq!(err, "");
    }

    #[test]
    fn work_help_exists() {
        let (code, out, err) = run_with(&["work", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Manage active local work state"));
        assert!(out.contains("start"));
        assert!(out.contains("status"));
        assert!(out.contains("complete"));
        assert_eq!(err, "");
    }

    #[test]
    fn work_complete_help_exists() {
        let (code, out, err) = run_with(&["work", "complete", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Archive and clear completed local work"));
        assert!(out.contains("--summary"));
        assert!(out.contains("--without-integrate"));
        assert_eq!(err, "");
    }

    #[test]
    fn build_help_exists() {
        let (code, out, err) = run_with(&["build", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Validate active work"));
        assert!(out.contains("[PATH]"));
        assert_eq!(err, "");
    }

    #[test]
    fn init_help_exists() {
        let (code, out, err) = run_with(&["init", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Record Rapport usage"));
        assert_eq!(err, "");
    }

    #[test]
    fn integrate_help_exists() {
        let (code, out, err) = run_with(&["integrate", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Git/GitHub integration"));
        assert!(out.contains("--summary"));
        assert_eq!(err, "");
    }

    #[test]
    fn integrate_requires_active_work() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::successful("must not run");

        let (code, out, err) = run_with_runner(
            &[
                "integrate",
                "--summary",
                "PW-356: Do the thing",
                "--message",
                "Do the thing",
            ],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("No active work state found"));
        assert!(runner.calls().is_empty());
        let event = first_event(&fs);

        assert_eq!(event.command, "integrate");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn init_creates_root_agents_file() {
        let mut fs = InMemoryFileSystem::default();

        let (code, out, err) = run_with_fs(&["init"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("status` — created"));
        assert!(out.contains("AGENTS.md"));
        assert_eq!(err, "");
        let agents = fs.read_to_string("/repo/AGENTS.md").unwrap();

        assert!(agents.contains("## Rapport"));
        assert!(agents.contains("rapport work start"));
        let event = first_event(&fs);

        assert_eq!(event.command, "init");
        assert_eq!(event.outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn init_updates_existing_agents_file_idempotently() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/AGENTS.md",
            "# Agent Notes\n\nKeep local context current.\n",
        )
        .unwrap();

        let (first_code, first_out, first_err) = run_with_fs(&["init"], &mut fs);
        let first_agents = fs.read_to_string("/repo/AGENTS.md").unwrap();
        let (second_code, second_out, second_err) = run_with_fs(&["init"], &mut fs);
        let second_agents = fs.read_to_string("/repo/AGENTS.md").unwrap();

        assert_eq!(first_code, ExitCode::SUCCESS);
        assert!(first_out.contains("status` — updated"));
        assert_eq!(first_err, "");
        assert_eq!(second_code, ExitCode::SUCCESS);
        assert!(second_out.contains("status` — updated"));
        assert_eq!(second_err, "");
        assert_eq!(first_agents, second_agents);
        assert!(second_agents.contains("# Agent Notes"));
        assert_eq!(
            second_agents.matches("<!-- rapport:init:start -->").count(),
            1
        );
        assert_eq!(events(&fs).len(), 2);
    }

    #[test]
    fn work_status_reports_no_active_work() {
        let mut fs = InMemoryFileSystem::default();
        let (code, out, err) = run_with_fs(&["work", "status"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("No active work state found"));
        assert!(out.contains("rapport work start"));
        assert_eq!(err, "");
        let event = first_event(&fs);

        assert_eq!(event.command, "work status");
        assert_eq!(event.outcome, CommandEventOutcome::Success);
        assert_eq!(event.exit_code, 0);
    }

    #[test]
    fn work_status_reports_active_work() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/.rapport/work.toml",
            r#"
schema_version = 1
title = "Do the thing"
objective = "Make it real"
ticket = "PW-123"
paths = ["app/api"]
stage = "development"
status = "active"
created_at = "2026-07-07T23:00:00Z"
updated_at = "2026-07-07T23:00:00Z"
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_fs(&["work", "status"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Do the thing"));
        assert!(out.contains("Make it real"));
        assert!(out.contains("app/api"));
        assert!(out.contains("rapport build"));
        assert_eq!(err, "");
    }

    #[test]
    fn work_status_reports_invalid_state() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/.rapport/work.toml", "schema_version =")
            .unwrap();

        let (code, out, err) = run_with_fs(&["work", "status"], &mut fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Could not read active work state"));
        assert!(err.contains("work state parse error"));
        let event = first_event(&fs);

        assert_eq!(event.command, "work status");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn work_start_creates_minimal_state() {
        let mut fs = InMemoryFileSystem::default();
        let (code, out, err) = run_with_fs(&["work", "start", "--title", "Do the thing"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Do the thing"));
        assert!(out.contains("No paths added yet."));
        assert_eq!(err, "");
        let state = load_state(&fs);

        assert_eq!(state.title, "Do the thing");
        assert_eq!(state.stage, WorkStage::Development);
        assert_eq!(state.status, WorkStatus::Active);
        assert_eq!(state.created_at, "2026-07-07T23:00:00Z");
        assert_eq!(state.updated_at, "2026-07-07T23:00:00Z");
        assert!(state.paths.is_empty());
        let event = first_event(&fs);

        assert_eq!(event.command, "work start");
        assert_eq!(event.outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn work_start_supports_ticket_objective_plan_and_multiple_paths() {
        let mut fs = InMemoryFileSystem::default();
        let (code, out, err) = run_with_fs(
            &[
                "work",
                "start",
                "--title",
                "Do the thing",
                "--ticket",
                "PW-123",
                "--plan",
                "plan-7",
                "--objective",
                "Make it real",
                "--path",
                "app/api",
                "--path",
                "app/core",
            ],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("PW-123"));
        assert!(out.contains("app/api"));
        assert!(out.contains("app/core"));
        assert_eq!(err, "");
        let state = load_state(&fs);

        assert_eq!(state.ticket.as_deref(), Some("PW-123"));
        assert_eq!(state.plan.as_deref(), Some("plan-7"));
        assert_eq!(state.objective.as_deref(), Some("Make it real"));
        assert_eq!(state.paths, vec!["app/api", "app/core"]);
    }

    #[test]
    fn work_start_rejects_existing_active_work() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/.rapport/work.toml",
            r#"
schema_version = 1
title = "Existing work"
paths = ["app/api"]
stage = "development"
status = "active"
created_at = "2026-07-07T23:00:00Z"
updated_at = "2026-07-07T23:00:00Z"
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_fs(&["work", "start", "--title", "New work"], &mut fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Active work already exists"));
        assert!(err.contains("Existing work"));
        let state = load_state(&fs);

        assert_eq!(state.title, "Existing work");
        let event = first_event(&fs);

        assert_eq!(event.command, "work start");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn work_complete_requires_active_work() {
        let mut fs = InMemoryFileSystem::default();

        let (code, out, err) = run_with_fs(
            &["work", "complete", "--summary", "Merged pull request"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("No active work state found"));
        let event = first_event(&fs);

        assert_eq!(event.command, "work complete");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn work_complete_rejects_non_integrated_work_without_flag() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);

        let (code, out, err) =
            run_with_fs(&["work", "complete", "--summary", "Done locally"], &mut fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("has not recorded a successful integration"));
        assert!(err.contains("--without-integrate"));
        assert_eq!(load_state(&fs).status, WorkStatus::Active);
        assert!(!fs.is_file("/repo/.rapport/history/2026-07-07T23-00-00Z-do-the-thing.toml"));
        let event = first_event(&fs);

        assert_eq!(event.command, "work complete");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn work_complete_archives_integrated_work_and_clears_active_state() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);

        let (code, out, err) =
            run_with_fs(&["work", "complete", "--summary", "Merged PR #70"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("status` — complete"));
        assert!(out.contains("Merged PR #70"));
        assert!(out.contains(".rapport/history/2026-07-07T23-00-00Z-do-the-thing.toml"));
        assert_eq!(err, "");
        assert_eq!(
            WorkStateStore::new(RapportPaths::new("/repo"))
                .load(&fs)
                .unwrap(),
            None
        );
        let archived = archived_state(&fs, "2026-07-07T23-00-00Z-do-the-thing.toml");

        assert_eq!(archived.status, WorkStatus::Complete);
        assert_eq!(archived.updated_at, "2026-07-07T23:00:00Z");
        assert_eq!(
            archived.complete.unwrap().summary.as_deref(),
            Some("Merged PR #70")
        );
        assert_eq!(archived.integrate.unwrap().status, "pr_created");
        let event = first_event(&fs);

        assert_eq!(event.command, "work complete");
        assert_eq!(event.outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn work_complete_allows_local_only_with_without_integrate() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);

        let (code, out, err) = run_with_fs(
            &[
                "work",
                "complete",
                "--summary",
                "Closed local experiment",
                "--without-integrate",
            ],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Closed local experiment"));
        assert_eq!(err, "");
        assert_eq!(
            WorkStateStore::new(RapportPaths::new("/repo"))
                .load(&fs)
                .unwrap(),
            None
        );
        let archived = archived_state(&fs, "2026-07-07T23-00-00Z-do-the-thing.toml");

        assert_eq!(archived.status, WorkStatus::Complete);
        assert!(archived.integrate.is_none());
        assert_eq!(
            archived.complete.unwrap().summary.as_deref(),
            Some("Closed local experiment")
        );
    }

    #[test]
    fn work_rules_list_reports_requested_path_rules() {
        let mut fs = InMemoryFileSystem::default();
        add_rule_owner(
            &mut fs,
            r#"
includes = ["/rules/rust.toml"]
"#,
        );
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
[[rules]]
id = "RUST-ORG-003"
text = "Keep lib.rs small."
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_fs(
            &["work", "rules", "list", "crates/rapport/src/lib.rs"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("RUST-ORG-003"));
        assert!(out.contains("Keep lib.rs small."));
        assert!(out.contains("rules.toml"));
        assert_eq!(err, "");
        let event = first_event(&fs);

        assert_eq!(event.command, "work rules list");
        assert_eq!(event.outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn work_rules_list_uses_active_work_paths() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_rule_owner(
            &mut fs,
            r#"
includes = ["/rules/testing.toml"]
"#,
        );
        fs.write_string(
            "/repo/rules/testing.toml",
            r#"
[[rules]]
id = "TEST-CORE-001"
text = "Treat tests as specifications."
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_fs(&["work", "rules", "list"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("crates/rapport/src/lib.rs"));
        assert!(out.contains("TEST-CORE-001"));
        assert_eq!(err, "");
    }

    #[test]
    fn work_rules_list_reports_unresolved_paths() {
        let mut fs = InMemoryFileSystem::default();

        let (code, out, err) = run_with_fs(
            &["work", "rules", "list", "crates/rapport/src/lib.rs"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("unresolved: no rules owner found"));
        assert_eq!(err, "");
    }

    #[test]
    fn work_rules_show_finds_rule_applicable_to_current_work() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_rule_owner(
            &mut fs,
            r#"
includes = ["/rules/rust.toml"]
"#,
        );
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
[[rules]]
id = "RUST-ORG-003"
text = "Keep lib.rs small."
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_fs(&["work", "rules", "show", "RUST-ORG-003"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("RUST-ORG-003"));
        assert!(out.contains("Keep lib.rs small."));
        assert_eq!(err, "");
    }

    #[test]
    fn work_rules_show_requires_applicable_rule() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_rule_owner(&mut fs, "");

        let (code, out, err) = run_with_fs(&["work", "rules", "show", "RUST-ORG-404"], &mut fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Rule `RUST-ORG-404` is not applicable"));
        let event = first_event(&fs);

        assert_eq!(event.command, "work rules show");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn work_add_path_updates_state_and_reports_rules() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.add_file("/repo/crates/rapport/src/work.rs");
        add_rule_owner(
            &mut fs,
            r#"
includes = ["/rules/rust.toml"]
"#,
        );
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
[[rules]]
id = "RUST-ORG-003"
text = "Keep lib.rs small."
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_fs(
            &["work", "add", "path", "crates/rapport/src/work.rs"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("status` — added"));
        assert!(out.contains("crates/rapport/src/lib.rs"));
        assert!(out.contains("crates/rapport/src/work.rs"));
        assert!(out.contains("owner `rules.toml`"));
        assert!(out.contains("RUST-ORG-003"));
        assert_eq!(err, "");
        let state = load_state(&fs);

        assert_eq!(
            state.paths,
            vec!["crates/rapport/src/lib.rs", "crates/rapport/src/work.rs"]
        );
        assert_eq!(state.updated_at, "2026-07-07T23:00:00Z");
        let event = first_event(&fs);

        assert_eq!(event.command, "work add path");
        assert_eq!(event.outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn work_add_path_rejects_duplicate_paths_without_mutating_state() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.add_file("/repo/crates/rapport/src/lib.rs");

        let (code, out, err) = run_with_fs(
            &["work", "add", "path", "crates/rapport/src/lib.rs"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("already present"));
        assert_eq!(load_state(&fs).paths, vec!["crates/rapport/src/lib.rs"]);
        let event = first_event(&fs);

        assert_eq!(event.command, "work add path");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn work_add_path_requires_active_work() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_file("/repo/crates/rapport/src/lib.rs");

        let (code, out, err) = run_with_fs(
            &["work", "add", "path", "crates/rapport/src/lib.rs"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("No active work state found"));
        let event = first_event(&fs);

        assert_eq!(event.command, "work add path");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn work_add_path_rejects_missing_paths_without_mutating_state() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);

        let (code, out, err) = run_with_fs(
            &["work", "add", "path", "crates/rapport/src/missing.rs"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("does not exist"));
        assert_eq!(load_state(&fs).paths, vec!["crates/rapport/src/lib.rs"]);
    }

    #[test]
    fn work_add_path_rejects_outside_repo_paths() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.add_file("/outside/work.rs");

        let (code, out, err) = run_with_fs(&["work", "add", "path", "/outside/work.rs"], &mut fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("outside the repository"));
        assert_eq!(load_state(&fs).paths, vec!["crates/rapport/src/lib.rs"]);
    }

    #[test]
    fn work_add_path_reports_unresolved_rules_for_paths_without_owner() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.add_file("/repo/crates/rapport/src/work.rs");

        let (code, out, err) = run_with_fs(
            &["work", "add", "path", "crates/rapport/src/work.rs"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("unresolved: no rules owner found"));
        assert_eq!(err, "");
        assert_eq!(
            load_state(&fs).paths,
            vec!["crates/rapport/src/lib.rs", "crates/rapport/src/work.rs"]
        );
    }

    #[test]
    fn build_runs_for_all_work_paths_and_updates_state() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(
            &mut fs,
            &["crates/rapport/src/lib.rs", "crates/rapport/src/work.rs"],
        );
        let runner = FakeRunner::successful("checked\n");

        let (code, out, err) = run_with_runner(&["build"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("status` — pass"));
        assert!(out.contains("command` — just ci"));
        assert!(out.contains("crates/rapport/src/lib.rs"));
        assert!(out.contains("crates/rapport/src/work.rs"));
        assert!(out.contains("checked"));
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![(CommandSpec::new("just", ["ci"]), Utf8PathBuf::from("/repo"))]
        );
        let state = load_state(&fs);
        let build = state.build.unwrap();

        assert_eq!(build.status, "pass");
        assert_eq!(build.at.as_deref(), Some("2026-07-07T23:00:00Z"));
        assert_eq!(
            build.summary.as_deref(),
            Some("`just ci` for crates/rapport/src/lib.rs, crates/rapport/src/work.rs")
        );
        let events = events(&fs);

        assert_eq!(events[0].command, "build start");
        assert_eq!(events[0].outcome, CommandEventOutcome::Success);
        assert_eq!(events[1].command, "build");
        assert_eq!(events[1].outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn build_runs_for_targeted_paths_inside_current_work() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(
            &mut fs,
            &["crates/rapport/src/lib.rs", "crates/rapport/src/work.rs"],
        );
        let runner = FakeRunner::successful("checked targeted path\n");

        let (code, out, err) =
            run_with_runner(&["build", "crates/rapport/src/work.rs"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("crates/rapport/src/work.rs"));
        assert!(!out.contains("crates/rapport/src/lib.rs"));
        assert_eq!(err, "");
        let build = load_state(&fs).build.unwrap();

        assert_eq!(
            build.summary.as_deref(),
            Some("`just ci` for crates/rapport/src/work.rs")
        );
    }

    #[test]
    fn build_rejects_paths_outside_current_work() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let runner = FakeRunner::successful("must not run");

        let (code, out, err) =
            run_with_runner(&["build", "crates/rapport/src/work.rs"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("not part of the current work"));
        assert!(err.contains("rapport work add path crates/rapport/src/work.rs"));
        assert!(load_state(&fs).build.is_none());
        assert!(runner.calls().is_empty());
        let event = first_event(&fs);

        assert_eq!(event.command, "build");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn build_records_command_failure() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let runner = FakeRunner::failing("tests failed\n");

        let (code, out, err) = run_with_runner(&["build"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("status` — fail"));
        assert!(err.contains("tests failed"));
        let build = load_state(&fs).build.unwrap();

        assert_eq!(build.status, "fail");
        assert_eq!(
            build.summary.as_deref(),
            Some("`just ci` for crates/rapport/src/lib.rs")
        );
        let events = events(&fs);

        assert_eq!(events[0].command, "build start");
        assert_eq!(events[1].command, "build");
        assert_eq!(events[1].outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn build_requires_active_work() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::successful("must not run");

        let (code, out, err) = run_with_runner(&["build"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("No active work state found"));
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn integrate_requires_passing_build_context() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let runner = FakeRunner::successful("must not run");

        let (code, out, err) = run_with_runner(
            &[
                "integrate",
                "--summary",
                "PW-356: Do the thing",
                "--message",
                "Do the thing",
            ],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("has not passed build validation"));
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn integrate_rejects_no_active_work_diff() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let runner = FakeRunner::with_outcomes([Ok(CommandOutcome {
            success: true,
            stdout: String::from(" M .rapport/work.toml\n"),
            stderr: String::new(),
        })]);

        let (code, out, err) = run_with_runner(
            &[
                "integrate",
                "--summary",
                "PW-356: Do the thing",
                "--message",
                "Do the thing",
            ],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("No active-work changes found"));
        assert!(err.contains(".rapport/work.toml"));
        assert_eq!(
            runner.calls(),
            vec![(
                CommandSpec::new("git", ["status", "--porcelain=v1"]),
                Utf8PathBuf::from("/repo")
            )]
        );
    }

    #[test]
    fn integrate_commits_creates_pr_and_reports_signoffs() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.write_string(
            "/repo/signoffs.toml",
            r#"
[[signoffs]]
id = "local-check"
description = "Local check"
command = ["just", "check"]

[[signoffs]]
id = "review"
description = "Human review"
"#,
        )
        .unwrap();
        let runner = successful_integrate_runner();

        let (code, out, err) = run_with_runner(
            &[
                "integrate",
                "--summary",
                "PW-356: Do the thing",
                "--message",
                "Do the thing",
            ],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("pr_created"));
        assert!(out.contains("https://github.com/hedge-ops/rapport/pull/70"));
        assert!(out.contains("pending `review: Human review`"));
        assert_eq!(runner.calls(), successful_integrate_calls());
        let state = load_state(&fs);
        let integrate = state.integrate.unwrap();
        let signoff = state.signoff.unwrap();

        assert_eq!(integrate.status, "pr_created");
        assert_eq!(integrate.branch.as_deref(), Some("work/issue-57-integrate"));
        assert_eq!(integrate.commit.as_deref(), Some("abc123"));
        assert_eq!(
            integrate.pr_url.as_deref(),
            Some("https://github.com/hedge-ops/rapport/pull/70")
        );
        assert_eq!(signoff.status, "pending");
        assert_eq!(signoff.passed, vec![String::from("local-check")]);
        assert_eq!(signoff.pending, vec![String::from("review: Human review")]);
        let events = events(&fs);
        let commands = events
            .iter()
            .map(|event| event.command.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            commands,
            vec![
                "integrate start",
                "integrate inspect",
                "integrate commit",
                "integrate pr",
                "integrate signoff",
                "integrate",
            ]
        );
    }

    #[test]
    fn integrate_records_failed_local_signoff() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.write_string(
            "/repo/signoffs.toml",
            r#"
[[signoffs]]
id = "local-check"
description = "Local check"
command = ["just", "check"]
"#,
        )
        .unwrap();
        let runner = FakeRunner::with_outcomes([
            Ok(CommandOutcome {
                success: true,
                stdout: String::from(" M crates/rapport/src/lib.rs\n"),
                stderr: String::new(),
            }),
            Ok(CommandOutcome {
                success: true,
                stdout: String::from("work/issue-57-integrate\n"),
                stderr: String::new(),
            }),
            Ok(CommandOutcome {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            }),
            Ok(CommandOutcome {
                success: true,
                stdout: String::from("[work/issue-57-integrate abc123] PW-356\n"),
                stderr: String::new(),
            }),
            Ok(CommandOutcome {
                success: true,
                stdout: String::from("abc123\n"),
                stderr: String::new(),
            }),
            Ok(CommandOutcome {
                success: true,
                stdout: String::from("https://github.com/hedge-ops/rapport/pull/70\n"),
                stderr: String::new(),
            }),
            Ok(CommandOutcome {
                success: false,
                stdout: String::new(),
                stderr: String::from("clippy failed\n"),
            }),
        ]);

        let (code, out, err) = run_with_runner(
            &[
                "integrate",
                "--summary",
                "PW-356: Do the thing",
                "--message",
                "Do the thing",
            ],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("failed `local-check"));
        assert!(err.contains("clippy failed"));
        let signoff = load_state(&fs).signoff.unwrap();

        assert_eq!(signoff.status, "fail");
        assert_eq!(signoff.failed.len(), 1);
        assert!(signoff.failed[0].contains("local-check"));
        let events = events(&fs);

        assert_eq!(events[4].command, "integrate signoff");
        assert_eq!(events[4].outcome, CommandEventOutcome::Failure);
        assert_eq!(events[5].command, "integrate");
        assert_eq!(events[5].outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn help_does_not_write_telemetry() {
        let mut fs = InMemoryFileSystem::default();
        let (code, _out, _err) = run_with_fs(&["work", "--help"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(!fs.is_file("/repo/.rapport/events.jsonl"));
    }

    fn first_event(fs: &InMemoryFileSystem) -> CommandEvent {
        events(fs).into_iter().next().unwrap()
    }

    fn events(fs: &InMemoryFileSystem) -> Vec<CommandEvent> {
        let events = fs.read_to_string("/repo/.rapport/events.jsonl").unwrap();
        events
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn load_state(fs: &InMemoryFileSystem) -> WorkState {
        WorkStateStore::new(RapportPaths::new("/repo"))
            .load(fs)
            .unwrap()
            .unwrap()
    }

    fn archived_state(fs: &InMemoryFileSystem, filename: &str) -> WorkState {
        let path = Utf8PathBuf::from(format!("/repo/.rapport/history/{filename}"));
        let contents = fs.read_to_string(path).unwrap();
        toml::from_str(&contents).unwrap()
    }

    fn add_rule_owner(fs: &mut InMemoryFileSystem, contents: &str) {
        fs.write_string("/repo/rules.toml", contents).unwrap();
    }

    fn add_active_work_with_paths(fs: &mut InMemoryFileSystem, paths: &[&str]) {
        let rendered_paths = paths
            .iter()
            .map(|path| format!("\"{path}\""))
            .collect::<Vec<_>>()
            .join(", ");
        fs.write_string(
            "/repo/.rapport/work.toml",
            format!(
                r#"
schema_version = 1
title = "Do the thing"
paths = [{rendered_paths}]
stage = "development"
status = "active"
created_at = "2026-07-07T23:00:00Z"
updated_at = "2026-07-07T23:00:00Z"
"#
            ),
        )
        .unwrap();
    }

    fn add_built_active_work_with_paths(fs: &mut InMemoryFileSystem, paths: &[&str]) {
        let rendered_paths = paths
            .iter()
            .map(|path| format!("\"{path}\""))
            .collect::<Vec<_>>()
            .join(", ");
        fs.write_string(
            "/repo/.rapport/work.toml",
            format!(
                r#"
schema_version = 1
title = "Do the thing"
paths = [{rendered_paths}]
stage = "development"
status = "active"
created_at = "2026-07-07T23:00:00Z"
updated_at = "2026-07-07T23:00:00Z"

[build]
status = "pass"
at = "2026-07-07T23:00:00Z"
summary = "`just ci` for crates/rapport/src/lib.rs"
"#
            ),
        )
        .unwrap();
    }

    fn add_integrated_active_work_with_paths(fs: &mut InMemoryFileSystem, paths: &[&str]) {
        let rendered_paths = paths
            .iter()
            .map(|path| format!("\"{path}\""))
            .collect::<Vec<_>>()
            .join(", ");
        fs.write_string(
            "/repo/.rapport/work.toml",
            format!(
                r#"
schema_version = 1
title = "Do the thing"
paths = [{rendered_paths}]
stage = "development"
status = "active"
created_at = "2026-07-07T23:00:00Z"
updated_at = "2026-07-07T23:00:00Z"

[build]
status = "pass"
at = "2026-07-07T23:00:00Z"
summary = "`just ci` for crates/rapport/src/lib.rs"

[integrate]
status = "pr_created"
at = "2026-07-07T23:00:00Z"
summary = "Issue #70"
commit = "abc123"
branch = "work/issue-70-complete"
pr_url = "https://github.com/hedge-ops/rapport/pull/70"
"#
            ),
        )
        .unwrap();
    }

    fn successful_integrate_runner() -> FakeRunner {
        FakeRunner::with_outcomes([
            Ok(successful_outcome(
                " M crates/rapport/src/lib.rs\n M .rapport/work.toml\n",
            )),
            Ok(successful_outcome("work/issue-57-integrate\n")),
            Ok(successful_outcome("")),
            Ok(successful_outcome(
                "[work/issue-57-integrate abc123] PW-356\n",
            )),
            Ok(successful_outcome("abc123\n")),
            Ok(successful_outcome(
                "https://github.com/hedge-ops/rapport/pull/70\n",
            )),
            Ok(successful_outcome("checked\n")),
        ])
    }

    fn successful_outcome(stdout: &str) -> CommandOutcome {
        CommandOutcome {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn successful_integrate_calls() -> Vec<(CommandSpec, Utf8PathBuf)> {
        vec![
            (
                CommandSpec::new("git", ["status", "--porcelain=v1"]),
                Utf8PathBuf::from("/repo"),
            ),
            (
                CommandSpec::new("git", ["branch", "--show-current"]),
                Utf8PathBuf::from("/repo"),
            ),
            (
                CommandSpec::new("git", ["add", "--", "crates/rapport/src/lib.rs"]),
                Utf8PathBuf::from("/repo"),
            ),
            (
                CommandSpec::new(
                    "git",
                    ["commit", "-m", "PW-356: Do the thing", "-m", "Do the thing"],
                ),
                Utf8PathBuf::from("/repo"),
            ),
            (
                CommandSpec::new("git", ["rev-parse", "HEAD"]),
                Utf8PathBuf::from("/repo"),
            ),
            (
                CommandSpec::new(
                    "gh",
                    [
                        "pr",
                        "create",
                        "--title",
                        "PW-356: Do the thing",
                        "--body",
                        "Do the thing\n\n## Rapport\n- Work: Do the thing\n- Paths: crates/rapport/src/lib.rs\n- Build: pass\n- Commit: abc123",
                    ],
                ),
                Utf8PathBuf::from("/repo"),
            ),
            (
                CommandSpec::new("just", ["check"]),
                Utf8PathBuf::from("/repo"),
            ),
        ]
    }
}

mod build;
mod cli;
mod context;
mod doctor;
mod init;
mod integrate;
mod paths;
mod prime;
mod project_context;
mod repository_files;
mod review;
mod rules;
mod runner;
mod signoff_contract;
mod snapshot;
mod state;
mod telemetry;
mod view;
mod work;

pub use context::{Clock, CommandContext, SystemClock, find_repo_root};
pub use paths::RapportPaths;
pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};
pub use state::{
    BuildState, OperationStatus, ReviewAction, ReviewActionStatus, ReviewAttempt, ReviewGrade,
    ReviewGradeError, ReviewState, WORK_STATE_SCHEMA_VERSION, WorkFact, WorkStage, WorkState,
    WorkStateError, WorkStateStore, WorkStatus,
};
pub use telemetry::{
    CommandEvent, CommandEventOutcome, EVENT_SCHEMA_VERSION, TelemetryError, TelemetryWriter,
};
pub use view::{Outcome, RunHint, View, ViewBuilder};

use clap::{CommandFactory, Parser, error::ErrorKind};
use cli::{Cli, Command, ReviewCommand, WorkCommand, WorkRulesCommand};
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
        Command::Prime => prime::run(argv, context),
        Command::Doctor => doctor::run(argv, context),
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
            WorkCommand::Task(task_args) => work::task(&task_args.command, argv, context),
        },
        Command::Context(context_args) => {
            project_context::run(&context_args.command, argv, context)
        }
        Command::Build(build_args) => build::run(build_args, argv, context),
        Command::Review(review_args) => match &review_args.command {
            ReviewCommand::Start(start_args) => review::start(start_args, argv, context),
            ReviewCommand::Complete(complete_args) => {
                review::complete(complete_args, argv, context)
            }
        },
        Command::Integrate(integrate_args) => integrate::run(integrate_args, argv, context),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::{InMemoryFileSystem, Utf8Path};
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
        assert!(out.contains(
            "prime -> doctor -> work -> context -> build -> review -> integrate -> work complete"
        ));
        assert!(out.contains("prime"));
        assert!(out.contains("doctor"));
        assert!(out.contains("work"));
        assert_eq!(err, "");
    }

    #[test]
    fn help_flag_renders_root_help() {
        let (code, out, err) = run_with(&["--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Rapport keeps human-directed agent work grounded"));
        assert!(out.contains(
            "prime -> doctor -> work -> context -> build -> review -> integrate -> work complete"
        ));
        assert_eq!(err, "");
    }

    #[test]
    fn prime_help_exists() {
        let (code, out, err) = run_with(&["prime", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Show how agents should use Rapport"));
        assert_eq!(err, "");
    }

    #[test]
    fn prime_renders_workflow_and_records_telemetry() {
        let mut fs = InMemoryFileSystem::default();

        let (code, out, err) = run_with_fs(&["prime"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("rapport prime"));
        assert!(out.contains("planning, coding, testing, building, reviewing"));
        assert!(out.contains("rapport work start"));
        assert!(out.contains("rapport context show"));
        assert!(out.contains("rapport work rules list"));
        assert!(out.contains("rapport doctor"));
        assert!(out.contains("rapport build"));
        assert!(out.contains("rapport integrate"));
        assert!(out.contains("rapport work complete"));
        assert_eq!(err, "");
        let event = first_event(&fs);

        assert_eq!(event.command, "prime");
        assert_eq!(event.outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn doctor_help_exists() {
        let (code, out, err) = run_with(&["doctor", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Check repository prerequisites"));
        assert_eq!(err, "");
    }

    #[test]
    fn doctor_reports_github_origin_success() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("git repository"));
        assert!(out.contains("origin remote"));
        assert!(out.contains("GitHub origin"));
        assert!(out.contains("rapport integrate"));
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![(
                CommandSpec::new("git", ["remote", "get-url", "origin"]),
                Utf8PathBuf::from("/repo")
            )]
        );
        let event = first_event(&fs);

        assert_eq!(event.command, "doctor");
        assert_eq!(event.outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn doctor_rejects_missing_origin() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::failing("error: No such remote 'origin'\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("origin remote"));
        assert!(err.contains("No such remote"));
        assert!(err.contains("configure GitHub origin"));
        let event = first_event(&fs);

        assert_eq!(event.command, "doctor");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn doctor_rejects_non_github_origin() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::successful("https://gitlab.com/hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("GitHub origin"));
        assert!(err.contains("does not point at GitHub"));
        assert!(err.contains("https://gitlab.com/hedge-ops/rapport.git"));
        let event = first_event(&fs);

        assert_eq!(event.command, "doctor");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn doctor_reports_project_context_success() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/rules.toml",
            r#"
version = 1

includes = ["/rules/rust.toml"]
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

[[rules]]
id = "RUST-001"
text = "Use rustfmt."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/app/context.toml",
            r#"
version = 1
purpose = "App purpose"
rule_includes = ["/rules/rust.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Project Context"));
        assert!(out.contains(
            "validated 1 context.toml file, 0 signoff declarations, and 1 rules.toml file"
        ));
        assert_eq!(err, "");
    }

    #[test]
    fn doctor_rejects_malformed_context_toml() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/app/context.toml", "version =")
            .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Project Context"));
        assert!(err.contains("context parse error"));
        assert!(err.contains("/repo/app/context.toml"));
    }

    #[test]
    fn doctor_rejects_unsupported_context_version() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/app/context.toml",
            r#"
version = 2
purpose = "App purpose"
rule_includes = []

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("unsupported context schema version `2`"));
        assert!(err.contains("supported version is `1`"));
        assert!(err.contains("/repo/app/context.toml"));
    }

    #[test]
    fn doctor_rejects_missing_context_rule_include() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/app/context.toml",
            r#"
version = 1
purpose = "App purpose"
rule_includes = ["/rules/missing.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("rule include `/rules/missing.toml`"));
        assert!(err.contains("does not exist"));
        assert!(err.contains("/repo/app/context.toml"));
    }

    #[test]
    fn doctor_rejects_unsupported_included_rule_library_version() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/app/context.toml",
            r#"
version = 1
purpose = "App purpose"
rule_includes = ["/rules/rust.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 2

[[rules]]
id = "RUST-001"
text = "Use rustfmt."
"#,
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("unsupported rules schema version `2`"));
        assert!(err.contains("supported version is `1`"));
        assert!(err.contains("/repo/rules/rust.toml"));
    }

    #[test]
    fn doctor_rejects_malformed_included_rule_library() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/app/context.toml",
            r#"
version = 1
purpose = "App purpose"
rule_includes = ["/rules/rust.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        fs.write_string("/repo/rules/rust.toml", "version =")
            .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("rules parse error"));
        assert!(err.contains("/repo/rules/rust.toml"));
    }

    #[test]
    fn doctor_rejects_missing_nested_rule_include() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/app/context.toml",
            r#"
version = 1
purpose = "App purpose"
rule_includes = ["/rules/rust.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1
includes = ["testing.toml"]
"#,
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("rule include `testing.toml`"));
        assert!(err.contains("does not exist"));
        assert!(err.contains("/repo/rules/rust.toml"));
    }

    #[test]
    fn doctor_rejects_duplicate_effective_context_rule_ids() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/app/context.toml",
            r#"
version = 1
purpose = "App purpose"
rule_includes = ["/rules/rust.toml", "/rules/testing.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

[[rules]]
id = "DUP-001"
text = "First rule."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/testing.toml",
            r#"
version = 1

[[rules]]
id = "DUP-001"
text = "Second rule."
"#,
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("duplicate rule id `DUP-001`"));
        assert!(err.contains("/repo/rules/rust.toml"));
        assert!(err.contains("/repo/rules/testing.toml"));
    }

    #[test]
    fn doctor_rejects_unsupported_rules_toml_version() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/rules.toml", "version = 2\n")
            .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("unsupported rules schema version `2`"));
        assert!(err.contains("/repo/rules.toml"));
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
    fn context_help_explains_project_context_intent() {
        let (code, out, err) = run_with(&["context", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("what a project area is about"));
        assert!(out.contains("Ownership records what belongs"));
        assert!(out.contains("numbered, reviewable benchmarks"));
        assert!(out.contains("context.toml"));
        assert_eq!(err, "");
    }

    #[test]
    fn context_init_creates_context_toml() {
        let mut fs = InMemoryFileSystem::default();

        let (code, out, err) = run_with_fs(
            &[
                "context",
                "init",
                "app/core/domain",
                "--purpose",
                "Owns workspace business rules.",
            ],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("status` — created"));
        assert!(out.contains("app/core/domain/context.toml"));
        assert_eq!(err, "");
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();

        assert!(context.contains("version = 1"));
        assert!(context.contains("purpose = \"Owns workspace business rules.\""));
    }

    #[test]
    fn context_editing_commands_update_context_toml() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);

        let (purpose_code, _, purpose_err) = run_with_fs(
            &[
                "context",
                "purpose",
                "set",
                "app/core/domain",
                "Owns workspace business rules.",
            ],
            &mut fs,
        );
        let (owns_code, _, owns_err) = run_with_fs(
            &[
                "context",
                "ownership",
                "owns",
                "add",
                "app/core/domain",
                "Workspace invariants",
            ],
            &mut fs,
        );
        let (boundary_code, _, boundary_err) = run_with_fs(
            &[
                "context",
                "ownership",
                "boundary",
                "add",
                "app/core/domain",
                "Persistence document shape belongs in app/shared/documents.",
            ],
            &mut fs,
        );
        let (include_code, _, include_err) = run_with_fs(
            &[
                "context",
                "rule",
                "include",
                "add",
                "app/core/domain",
                "/rules/rust.toml",
            ],
            &mut fs,
        );
        let (rule_code, _, rule_err) = run_with_fs(
            &[
                "context",
                "rule",
                "add",
                "app/core/domain",
                "--id",
                "DOMAIN-WORKSPACE-001",
                "--text",
                "Treat documents::WorkspaceDocument as the persistence aggregate.",
                "--rationale",
                "The domain owns relationship and work invariants.",
                "--reference",
                "https://github.com/hedge-ops/rapport/issues/78",
            ],
            &mut fs,
        );

        assert_eq!(purpose_code, ExitCode::SUCCESS);
        assert_eq!(owns_code, ExitCode::SUCCESS);
        assert_eq!(boundary_code, ExitCode::SUCCESS);
        assert_eq!(include_code, ExitCode::SUCCESS);
        assert_eq!(rule_code, ExitCode::SUCCESS);
        assert_eq!(purpose_err, "");
        assert_eq!(owns_err, "");
        assert_eq!(boundary_err, "");
        assert_eq!(include_err, "");
        assert_eq!(rule_err, "");
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();

        assert!(context.contains("purpose = \"Owns workspace business rules.\""));
        assert!(context.contains("\"Workspace invariants\""));
        assert!(
            context.contains("\"Persistence document shape belongs in app/shared/documents.\"")
        );
        assert!(context.contains("\"/rules/rust.toml\""));
        assert!(context.contains("id = \"DOMAIN-WORKSPACE-001\""));
        assert!(
            context.contains("rationale = \"The domain owns relationship and work invariants.\"")
        );
        assert!(context.contains("\"https://github.com/hedge-ops/rapport/issues/78\""));
    }

    #[test]
    fn context_signoff_add_generates_exact_github_request_contract() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);

        let (code, out, err) = run_with_fs(
            &[
                "context",
                "signoff",
                "add",
                "app/core/domain",
                "build",
                "ci",
            ],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("signoff: app-core-domain-build-ci"));
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();
        assert!(context.contains("[[signoffs]]"));
        assert!(context.contains("kind = \"build\""));
        assert!(context.contains("target = \"ci\""));
        let shared = fs
            .read_to_string("/repo/.github/workflows/rapport-signoff.yml")
            .unwrap();
        assert!(shared.contains("context=signoff: ${TARGET}"));
        let request = fs
            .read_to_string("/repo/.github/workflows/rapport-app-core-domain-build-ci.yml")
            .unwrap();
        assert!(request.contains("- \"app/core/domain/**\""));
        assert!(request.contains("target: app-core-domain-build-ci"));
        assert!(!request.contains("runs-on:"));
    }

    #[test]
    fn context_signoff_review_has_no_target_or_profile() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);

        let (code, out, err) = run_with_fs(
            &[
                "context",
                "signoff",
                "add",
                "app/core/domain",
                "review",
                "--minimum-grade",
                "A-",
            ],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("signoff: app-core-domain-review"));
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();
        assert!(context.contains("kind = \"review\""));
        assert!(context.contains("minimum_grade = \"A-\""));
        assert!(!context.contains("target ="));
        let workflow = fs
            .read_to_string("/repo/.github/workflows/rapport-app-core-domain-review.yml")
            .unwrap();
        assert!(workflow.contains("target: app-core-domain-review"));
        assert!(workflow.contains("signoff repair app/core/domain review`"));

        fs.write_string(
            "/repo/.github/workflows/rapport-app-core-domain-review.yml",
            "drifted\n",
        )
        .unwrap();
        let (repair_code, repair_out, repair_err) = run_with_fs(
            &["context", "signoff", "repair", "app/core/domain", "review"],
            &mut fs,
        );
        assert_eq!(repair_code, ExitCode::SUCCESS);
        assert_eq!(repair_err, "");
        assert!(repair_out.contains("signoff: app-core-domain-review"));

        let (remove_code, remove_out, remove_err) = run_with_fs(
            &["context", "signoff", "remove", "app/core/domain", "review"],
            &mut fs,
        );
        assert_eq!(remove_code, ExitCode::SUCCESS);
        assert_eq!(remove_err, "");
        assert!(remove_out.contains("signoff: app-core-domain-review"));
        assert!(!fs.is_file("/repo/.github/workflows/rapport-app-core-domain-review.yml"));
        assert!(
            !fs.read_to_string("/repo/app/core/domain/context.toml")
                .unwrap()
                .contains("kind = \"review\"")
        );
    }

    #[test]
    fn context_signoff_rejects_review_profile_and_missing_build_target() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);

        let (review_code, _, review_err) = run_with_fs(
            &[
                "context",
                "signoff",
                "add",
                "app/core/domain",
                "review",
                "friendly",
            ],
            &mut fs,
        );
        let (build_code, _, build_err) = run_with_fs(
            &["context", "signoff", "add", "app/core/domain", "build"],
            &mut fs,
        );

        assert_eq!(review_code, ExitCode::from(2));
        assert!(review_err.contains("do not accept a target or profile"));
        assert_eq!(build_code, ExitCode::from(2));
        assert!(build_err.contains("build signoffs require a target"));
        assert!(!fs.is_dir("/repo/.github"));
    }

    #[test]
    fn context_signoff_rejects_readable_identity_collision_before_mutation() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/app/apple/context.toml",
            "version = 1\npurpose = \"Nested\"\nsignoffs = []\n",
        )
        .unwrap();
        fs.write_string(
            "/repo/app-apple/context.toml",
            "version = 1\npurpose = \"Flat\"\nsignoffs = []\n",
        )
        .unwrap();
        let (first_code, _, first_err) = run_with_fs(
            &["context", "signoff", "add", "app/apple", "build", "ci"],
            &mut fs,
        );
        let original_workflow = fs
            .read_to_string("/repo/.github/workflows/rapport-app-apple-build-ci.yml")
            .unwrap();

        let (second_code, second_out, second_err) = run_with_fs(
            &["context", "signoff", "add", "app-apple", "build", "ci"],
            &mut fs,
        );

        assert_eq!(first_code, ExitCode::SUCCESS);
        assert_eq!(first_err, "");
        assert_eq!(second_code, ExitCode::from(2));
        assert_eq!(second_out, "");
        assert!(second_err.contains("signoff identity `app-apple-build-ci`"));
        assert_eq!(
            fs.read_to_string("/repo/app-apple/context.toml").unwrap(),
            "version = 1\npurpose = \"Flat\"\nsignoffs = []\n"
        );
        assert_eq!(
            fs.read_to_string("/repo/.github/workflows/rapport-app-apple-build-ci.yml")
                .unwrap(),
            original_workflow
        );
    }

    #[test]
    fn context_signoff_repair_and_remove_own_generated_workflow() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);
        let request_path = "/repo/.github/workflows/rapport-app-core-domain-build-ci.yml";
        let _ = run_with_fs(
            &[
                "context",
                "signoff",
                "add",
                "app/core/domain",
                "build",
                "ci",
            ],
            &mut fs,
        );
        fs.write_string(request_path, "changed\n").unwrap();

        let (repair_code, _, repair_err) = run_with_fs(
            &[
                "context",
                "signoff",
                "repair",
                "app/core/domain",
                "build",
                "ci",
            ],
            &mut fs,
        );
        let repaired = fs.read_to_string(request_path).unwrap();
        let (remove_code, _, remove_err) = run_with_fs(
            &[
                "context",
                "signoff",
                "remove",
                "app/core/domain",
                "build",
                "ci",
            ],
            &mut fs,
        );

        assert_eq!(repair_code, ExitCode::SUCCESS);
        assert_eq!(repair_err, "");
        assert!(repaired.contains("target: app-core-domain-build-ci"));
        assert_eq!(remove_code, ExitCode::SUCCESS);
        assert_eq!(remove_err, "");
        assert!(!fs.is_file(request_path));
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();
        assert!(context.contains("signoffs = []"));
    }

    #[test]
    fn context_edit_migrates_legacy_build_and_removes_legacy_workflow() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);
        fs.write_string(
            "/repo/app/core/domain/context.toml",
            "version = 1\npurpose = \"Domain\"\nsignoffs = [\"ci\"]\n",
        )
        .unwrap();
        fs.write_string(
            "/repo/.github/workflows/rapport-app-core-domain-ci.yml",
            "legacy workflow\n",
        )
        .unwrap();

        let (code, _, err) = run_with_fs(
            &[
                "context",
                "signoff",
                "repair",
                "app/core/domain",
                "build",
                "ci",
            ],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(!fs.is_file("/repo/.github/workflows/rapport-app-core-domain-ci.yml"));
        assert!(fs.is_file("/repo/.github/workflows/rapport-app-core-domain-build-ci.yml"));
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();
        assert!(context.contains("[[signoffs]]"));
        assert!(context.contains("kind = \"build\""));

        let validation = project_context::validate_repository(&fs, Utf8Path::new("/repo"));
        assert_eq!(validation.signoff_count(), 1);
        assert_eq!(
            validation.problem_details().collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn context_edit_rejects_legacy_migration_collision_before_any_mutation() {
        let mut fs = InMemoryFileSystem::default();
        let legacy_context = "version = 1\npurpose = \"Nested\"\nsignoffs = [\"ci\"]\n";
        fs.write_string("/repo/app/apple/context.toml", legacy_context)
            .unwrap();
        fs.write_string(
            "/repo/app-apple/context.toml",
            r#"version = 1
purpose = "Flat"

[[signoffs]]
kind = "build"
target = "ci"
"#,
        )
        .unwrap();
        let existing = signoff_contract::SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo/app-apple"),
            signoff_contract::SignoffKind::Build,
            "ci",
            None,
        )
        .unwrap();
        signoff_contract::write_shared(&mut fs, Utf8Path::new("/repo")).unwrap();
        signoff_contract::write_request(&mut fs, Utf8Path::new("/repo"), &existing).unwrap();
        let workflow_before = fs.read_to_string(existing.workflow_path()).unwrap();

        let (code, out, err) = run_with_fs(
            &["context", "purpose", "set", "app/apple", "Updated purpose"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("collides with a declaration"), "{err}");
        assert_eq!(
            fs.read_to_string("/repo/app/apple/context.toml").unwrap(),
            legacy_context
        );
        assert_eq!(
            fs.read_to_string(existing.workflow_path()).unwrap(),
            workflow_before
        );
    }

    #[test]
    fn context_edit_rejects_duplicate_legacy_identities_in_one_file_before_mutation() {
        let mut fs = InMemoryFileSystem::default();
        let duplicate_context =
            "version = 1\npurpose = \"Duplicate\"\nsignoffs = [\"ci\", \"ci\"]\n";
        fs.write_string("/repo/app/context.toml", duplicate_context)
            .unwrap();
        let request = signoff_contract::SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo/app"),
            signoff_contract::SignoffKind::Build,
            "ci",
            None,
        )
        .unwrap();
        signoff_contract::write_shared(&mut fs, Utf8Path::new("/repo")).unwrap();
        signoff_contract::write_request(&mut fs, Utf8Path::new("/repo"), &request).unwrap();
        let workflow_before = fs.read_to_string(request.workflow_path()).unwrap();

        let (code, out, err) = run_with_fs(
            &["context", "purpose", "set", "app", "Updated purpose"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("collides with a declaration"), "{err}");
        assert_eq!(
            fs.read_to_string("/repo/app/context.toml").unwrap(),
            duplicate_context
        );
        assert_eq!(
            fs.read_to_string(request.workflow_path()).unwrap(),
            workflow_before
        );
    }

    #[test]
    fn legacy_cleanup_preserves_a_distinct_typed_workflow_with_the_same_path() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/app/apple/context.toml",
            "version = 1\npurpose = \"Nested\"\nsignoffs = [\"review\"]\n",
        )
        .unwrap();
        fs.write_string(
            "/repo/app-apple/context.toml",
            r#"version = 1
purpose = "Flat"

[[signoffs]]
kind = "review"
minimum_grade = "A-"
"#,
        )
        .unwrap();
        let typed_review = signoff_contract::SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo/app-apple"),
            signoff_contract::SignoffKind::Review,
            "review",
            Some("A-".parse().unwrap()),
        )
        .unwrap();
        signoff_contract::write_shared(&mut fs, Utf8Path::new("/repo")).unwrap();
        signoff_contract::write_request(&mut fs, Utf8Path::new("/repo"), &typed_review).unwrap();
        let typed_workflow_before = fs.read_to_string(typed_review.workflow_path()).unwrap();

        let (code, _, err) = run_with_fs(
            &["context", "purpose", "set", "app/apple", "Updated purpose"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS, "{err}");
        assert_eq!(err, "");
        assert_eq!(
            fs.read_to_string(typed_review.workflow_path()).unwrap(),
            typed_workflow_before
        );
        assert!(fs.is_file("/repo/.github/workflows/rapport-app-apple-build-review.yml"));
    }

    #[test]
    fn context_signoff_add_rejects_invalid_target_before_writing() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);

        let (code, out, err) = run_with_fs(
            &[
                "context",
                "signoff",
                "add",
                "app/core/domain",
                "build",
                "Not Valid",
            ],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("use lowercase kebab-case"));
        assert!(!fs.is_dir("/repo/.github"));
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();
        assert!(!context.contains("Not Valid"));
    }

    #[test]
    fn context_signoff_add_rejects_oversized_identity_before_mutation() {
        let mut fs = InMemoryFileSystem::default();
        let folder = "a".repeat(130);
        let context_path = format!("/repo/{folder}/context.toml");
        let original = "version = 1\npurpose = \"Long folder\"\nsignoffs = []\n";
        fs.write_string(&context_path, original).unwrap();

        let (code, out, err) = run_with_fs(
            &["context", "signoff", "add", &folder, "build", "ci"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("GitHub status contexts support at most 140 bytes"));
        assert_eq!(fs.read_to_string(context_path).unwrap(), original);
        assert!(!fs.is_dir("/repo/.github"));
    }

    #[test]
    fn doctor_rejects_drifted_signoff_request_workflow() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);
        let _ = run_with_fs(
            &[
                "context",
                "signoff",
                "add",
                "app/core/domain",
                "build",
                "ci",
            ],
            &mut fs,
        );
        fs.write_string(
            "/repo/.github/workflows/rapport-app-core-domain-build-ci.yml",
            "changed\n",
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("has drifted from its generated content"));
        assert!(err.contains("rapport-app-core-domain-build-ci.yml"));
    }

    #[test]
    fn doctor_rejects_readable_signoff_identity_collisions() {
        let mut fs = InMemoryFileSystem::default();
        for context_path in [
            "/repo/app/apple/context.toml",
            "/repo/app-apple/context.toml",
        ] {
            fs.write_string(
                context_path,
                r#"version = 1
purpose = "Review owner"

[[signoffs]]
kind = "review"
minimum_grade = "A-"
"#,
            )
            .unwrap();
        }
        signoff_contract::write_shared(&mut fs, Utf8Path::new("/repo")).unwrap();
        let request = signoff_contract::SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo/app/apple"),
            signoff_contract::SignoffKind::Review,
            "review",
            None,
        )
        .unwrap();
        signoff_contract::write_request(&mut fs, Utf8Path::new("/repo"), &request).unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("signoff identity collision `app-apple-review`"));
        assert!(err.contains("declaring contexts"));
        assert!(err.contains("`app/apple`"));
        assert!(err.contains("`app-apple`"));
    }

    #[test]
    fn doctor_rejects_signoff_folders_whose_readable_component_collapses() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/---/context.toml",
            r#"version = 1
purpose = "Collapsed owner"

[[signoffs]]
kind = "review"
minimum_grade = "A-"
"#,
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("must contain an ASCII letter or digit"));
        assert!(!fs.is_dir("/repo/.github"));
    }

    #[test]
    fn context_show_prints_effective_context_and_benchmarks() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

[[rules]]
id = "RUST-001"
text = "Use rustfmt."
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/context.toml",
            r#"
version = 1
purpose = "Root purpose"
rule_includes = ["/rules/rust.toml"]

[ownership]
owns = ["Root ownership"]
boundaries = []
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/app/core/context.toml",
            r#"
version = 1
purpose = "Core purpose"
rule_includes = []

[ownership]
owns = ["Core ownership"]
boundaries = ["Persistence lives elsewhere."]

[[rules]]
id = "CORE-001"
text = "Keep core boring."
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_fs(&["context", "show", "app/core/domain"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Core purpose"));
        assert!(out.contains("Root ownership"));
        assert!(out.contains("Persistence lives elsewhere."));
        assert!(out.contains("RUST-001"));
        assert!(out.contains("CORE-001"));
        assert_eq!(err, "");
    }

    #[test]
    fn context_doctor_reports_missing_includes() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/context.toml",
            r#"
version = 1
purpose = "Root purpose"
rule_includes = ["/rules/missing.toml"]

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_fs(&["context", "doctor", "."], &mut fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Context validation failed"));
        assert!(err.contains("/rules/missing.toml"));
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
        assert!(out.contains(".github/workflows/rapport-signoff.yml"));
        assert_eq!(err, "");
        let agents = fs.read_to_string("/repo/AGENTS.md").unwrap();

        assert!(agents.contains("## Software Factory"));
        assert!(agents.contains("rapport prime"));
        assert!(!agents.contains("rapport work start"));
        let signoff = fs
            .read_to_string("/repo/.github/workflows/rapport-signoff.yml")
            .unwrap();
        assert!(signoff.contains("context=signoff: ${TARGET}"));
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
        assert!(out.contains("rapport integrate"));
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
    fn work_complete_rejects_pending_signoffs() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let mut state = load_state(&fs);
        let mut signoff = WorkFact::new("pending");
        signoff.required = vec![String::from("root-ci")];
        signoff.pending = vec![String::from("root-ci")];
        state.signoff = Some(signoff);
        WorkStateStore::new(RapportPaths::new("/repo"))
            .save(&mut fs, &state)
            .unwrap();

        let (code, out, err) = run_with_fs(
            &["work", "complete", "--summary", "Not actually done"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("Required signoffs are still pending"));
        assert_eq!(load_state(&fs).status, WorkStatus::Active);
    }

    #[test]
    fn work_complete_archives_integrated_work_and_clears_active_state() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let runner = FakeRunner::successful("abc123\n");

        let (code, out, err) = run_with_runner(
            &["work", "complete", "--summary", "Merged PR #70"],
            &mut fs,
            &runner,
        );

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
    fn work_complete_rejects_head_that_differs_from_integrated_pr() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let runner = FakeRunner::successful("new-empty-commit\n");

        let (code, out, err) = run_with_runner(
            &["work", "complete", "--summary", "Not the integrated SHA"],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("current HEAD does not match integrated PR head"));
        assert!(!err.contains("abc123"));
        assert!(!err.contains("new-empty-commit"));
        assert_eq!(load_state(&fs).status, WorkStatus::Active);
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
version = 1

includes = ["/rules/rust.toml"]
"#,
        );
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

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
version = 1

includes = ["/rules/testing.toml"]
"#,
        );
        fs.write_string(
            "/repo/rules/testing.toml",
            r#"
version = 1

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
version = 1

includes = ["/rules/rust.toml"]
"#,
        );
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

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
        add_rule_owner(&mut fs, "version = 1\n");

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
version = 1

includes = ["/rules/rust.toml"]
"#,
        );
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

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
        add_root_signoff(&mut fs, "ci");
        let runner = build_runner(successful_outcome("checked\n"));

        let (code, out, err) = run_with_runner(&["build"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("status `pass`"));
        assert!(out.contains("`just ci`"));
        assert!(out.contains("crates/rapport/src/lib.rs"));
        assert!(out.contains("crates/rapport/src/work.rs"));
        assert!(out.contains("stdout: 8 bytes"));
        assert!(!out.contains("checked"));
        assert!(out.contains("rapport integrate"));
        assert!(!out.contains("rapport review"));
        assert_eq!(err, "");
        assert_eq!(
            runner.calls().last().cloned(),
            Some((CommandSpec::new("just", ["ci"]), Utf8PathBuf::from("/repo")))
        );
        let state = load_state(&fs);
        let build = state.build.unwrap();

        assert_eq!(build.status, "pass");
        assert_eq!(build.at.as_deref(), Some("2026-07-07T23:00:00Z"));
        assert_eq!(build.summary.as_deref(), Some("1 typed build operation(s)"));
        assert_eq!(state.builds["root-build-ci"].status, OperationStatus::Pass);
        let events = events(&fs);

        assert_eq!(events[0].command, "build");
        assert_eq!(events[0].outcome, CommandEventOutcome::Success);
    }

    #[test]
    fn build_runs_for_targeted_paths_inside_current_work() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(
            &mut fs,
            &["crates/rapport/src/lib.rs", "crates/rapport/src/work.rs"],
        );
        add_root_signoff(&mut fs, "ci");
        let runner = build_runner(successful_outcome("checked targeted path\n"));

        let (code, out, err) =
            run_with_runner(&["build", "crates/rapport/src/work.rs"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("crates/rapport/src/work.rs"));
        assert!(!out.contains("crates/rapport/src/lib.rs"));
        assert_eq!(err, "");
        let build = load_state(&fs).build.unwrap();

        assert_eq!(build.summary.as_deref(), Some("1 typed build operation(s)"));
    }

    #[test]
    fn work_status_and_completion_reject_stale_required_build() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_root_signoff(&mut fs, "ci");
        let initial = build_runner(successful_outcome("checked\n"));
        let (build_code, _, _) = run_with_runner(&["build"], &mut fs, &initial);
        assert_eq!(build_code, ExitCode::SUCCESS);

        let mut state = load_state(&fs);
        let mut integration = WorkFact::new("pr_created");
        integration.commit = Some(String::from("head123"));
        integration.branch = Some(String::from("work/stale-build"));
        integration.pr_url = Some(String::from("https://github.com/hedge-ops/rapport/pull/87"));
        state.integrate = Some(integration);
        state.signoff = Some(WorkFact::new("pass"));
        WorkStateStore::new(RapportPaths::new("/repo"))
            .save(&mut fs, &state)
            .unwrap();

        let status_runner = review_result_runner("diff-v2");
        let (status_code, status_out, status_err) =
            run_with_runner(&["work", "status"], &mut fs, &status_runner);
        assert_eq!(status_code, ExitCode::SUCCESS);
        assert_eq!(status_err, "");
        assert!(status_out.contains("`root-build-ci` stale"));

        let complete_runner = review_result_runner("diff-v2");
        let (complete_code, complete_out, complete_err) = run_with_runner(
            &["work", "complete", "--summary", "Not current"],
            &mut fs,
            &complete_runner,
        );
        assert_eq!(complete_code, ExitCode::from(2));
        assert_eq!(complete_out, "");
        assert!(complete_err.contains("required build `root-build-ci` is stale"));
        assert_eq!(load_state(&fs).status, WorkStatus::Active);
    }

    #[test]
    fn work_status_marks_changed_failing_build_stale() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_root_signoff(&mut fs, "ci");
        let failed = build_runner(CommandOutcome {
            success: false,
            stdout: String::new(),
            stderr: String::from("failed\n"),
        });
        let (build_code, _, _) = run_with_runner(&["build"], &mut fs, &failed);
        assert_eq!(build_code, ExitCode::from(2));
        assert_eq!(
            load_state(&fs).builds["root-build-ci"].status,
            OperationStatus::Fail
        );

        let changed = review_result_runner("changed-diff");
        let (status_code, status_out, status_err) =
            run_with_runner(&["work", "status"], &mut fs, &changed);

        assert_eq!(status_code, ExitCode::SUCCESS);
        assert_eq!(status_err, "");
        assert!(status_out.contains("`root-build-ci` stale"));
        let build = &load_state(&fs).builds["root-build-ci"];
        assert_eq!(build.status, OperationStatus::Stale);
        assert_eq!(build.result_status, Some(OperationStatus::Fail));
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
        assert!(err.contains("rapport work add path <path>"));
        assert!(load_state(&fs).build.is_none());
        assert!(runner.calls().is_empty());
        let event = first_event(&fs);

        assert_eq!(event.command, "build");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn build_rejects_parent_traversal_and_scope_widening() {
        for requested in ["app/../other", "app"] {
            let mut fs = InMemoryFileSystem::default();
            add_active_work_with_paths(&mut fs, &["app/one.rs"]);
            add_root_signoff(&mut fs, "ci");
            let runner = FakeRunner::successful("must not run");

            let (code, out, err) = run_with_runner(&["build", requested], &mut fs, &runner);

            assert_eq!(code, ExitCode::from(2));
            assert_eq!(out, "");
            assert!(
                err.contains("outside the repository")
                    || err.contains("not part of the current work"),
                "{err}"
            );
            assert!(runner.calls().is_empty());
            assert!(load_state(&fs).builds.is_empty());
        }
    }

    #[test]
    fn build_records_command_failure() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_root_signoff(&mut fs, "ci");
        let runner = build_runner(CommandOutcome {
            success: false,
            stdout: String::new(),
            stderr: String::from("tests failed\n"),
        });

        let (code, out, err) = run_with_runner(&["build"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("status `fail`"));
        assert!(err.contains("stderr: 13 bytes"));
        assert!(!err.contains("tests failed"));
        let build = load_state(&fs).build.unwrap();

        assert_eq!(build.status, "fail");
        assert_eq!(build.summary.as_deref(), Some("1 typed build operation(s)"));
        let events = events(&fs);

        assert_eq!(events[0].command, "build");
        assert_eq!(events[0].outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn successful_build_guides_to_a_required_review() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.write_string(
            "/repo/context.toml",
            r#"version = 1
purpose = "Repository"

[[signoffs]]
kind = "build"
target = "ci"

[[signoffs]]
kind = "review"
minimum_grade = "A-"
"#,
        )
        .unwrap();

        let (code, out, err) = run_with_runner(
            &["build"],
            &mut fs,
            &build_runner(successful_outcome("checked\n")),
        );

        assert_eq!(code, ExitCode::SUCCESS, "{err}");
        assert_eq!(err, "");
        assert!(out.contains("rapport review"));
        assert!(!out.contains("rapport integrate"));
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
    fn review_start_defaults_to_markdown_without_exposing_the_passing_threshold() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.add_file_with_contents("/repo/app/file.rs", "fn changed() {}\n");
        add_review_context(&mut fs);

        let (code, out, err) = run_with_runner(
            &["review", "start"],
            &mut fs,
            &review_request_runner("diff-v1"),
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.starts_with("# Rapport adversarial review"));
        assert!(out.contains("```json"));
        assert!(out.contains("\"grade\": \"A through F with optional + or -\""));
        assert!(!out.contains("minimum_grade"));
        assert!(!out.contains("minimum passing grade"));
        assert!(!out.contains("\"status\": \"pass|fail\""));
    }

    #[test]
    fn review_start_uses_head_when_origin_default_is_not_fetched() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.add_file_with_contents("/repo/app/file.rs", "fn changed() {}\n");
        add_review_context(&mut fs);
        let runner = FakeRunner::with_outcomes([
            successful_result("head123\n"),
            Ok(CommandOutcome {
                success: false,
                stdout: String::new(),
                stderr: String::new(),
            }),
            successful_result("diff-v1"),
            successful_result(""),
        ]);

        let (code, out, err) = run_with_runner(&["review", "start", "--json"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS, "{err}");
        assert_eq!(err, "");
        let request: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(request[0]["snapshot"]["base_sha"], "head123");
        assert!(runner.calls().iter().any(|(spec, _)| {
            spec.program == "git"
                && spec.args
                    == [
                        "symbolic-ref",
                        "--quiet",
                        "--short",
                        "refs/remotes/origin/HEAD",
                    ]
        }));
        assert!(!runner.calls().iter().any(|(spec, _)| {
            spec.program == "git" && spec.args.first().is_some_and(|arg| arg == "merge-base")
        }));
    }

    #[test]
    fn review_start_markdown_defines_one_array_for_multiple_requirements() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.add_file_with_contents("/repo/app/file.rs", "fn changed() {}\n");
        add_nested_review_contexts(&mut fs);

        let (code, out, err) =
            run_with_runner(&["review", "start"], &mut fs, &two_review_request_runner());

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.starts_with("# Rapport adversarial review set"));
        assert!(out.contains("Return only one JSON array"));
        let packet_json = out
            .split("```json\n")
            .nth(1)
            .unwrap()
            .split("\n```")
            .next()
            .unwrap();
        let packets: serde_json::Value = serde_json::from_str(packet_json).unwrap();
        assert_eq!(packets.as_array().unwrap().len(), 2);
    }

    #[test]
    fn review_complete_rejects_a_result_file_inside_reviewed_content() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["."]);
        add_review_context(&mut fs);
        let (request_code, request_json, request_err) = run_with_runner(
            &["review", "start", "--json"],
            &mut fs,
            &review_request_runner("diff-v1"),
        );
        assert_eq!(request_code, ExitCode::SUCCESS);
        assert_eq!(request_err, "");
        let request: serde_json::Value = serde_json::from_str(&request_json).unwrap();
        fs.write_string(
            "/repo/review-result.json",
            serde_json::json!({
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": request[0]["snapshot"]["input_checksum"],
                "grade": "A",
                "description": "No findings.",
                "actions": []
            })
            .to_string(),
        )
        .unwrap();
        let runner = FakeRunner::successful("must not run");

        let (code, out, err) = run_with_runner(
            &["review", "complete", "--result", "review-result.json"],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("must be outside the reviewed content"));
        assert!(runner.calls().is_empty());
        assert_eq!(
            load_state(&fs).reviews["root-review"].status,
            OperationStatus::Pending
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the end-to-end scenario intentionally keeps request, stale result, action retention, rereview, resolution, and exact reuse in one behavioral specification"
    )]
    fn review_tracks_uncommitted_staleness_and_reconciles_actions() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.add_file_with_contents("/repo/app/file.rs", "fn changed() {}\n");
        add_review_context(&mut fs);

        let first_request = review_request_runner("diff-v1");
        let (request_code, request_json, request_err) =
            run_with_runner(&["review", "start", "--json"], &mut fs, &first_request);

        assert_eq!(request_code, ExitCode::SUCCESS);
        assert_eq!(request_err, "");
        let request: serde_json::Value = serde_json::from_str(&request_json).unwrap();
        assert!(request[0]["requirement"].get("target").is_none());
        assert!(request[0]["requirement"].get("minimum_grade").is_none());
        assert_eq!(request[0]["requirement"]["requirement_id"], "root-review");
        assert!(
            request[0]["instructions"]
                .as_str()
                .unwrap()
                .contains("safety and security")
        );
        assert!(
            !request[0]["instructions"]
                .as_str()
                .unwrap()
                .contains("minimum passing grade")
        );
        assert!(request[0]["result_contract"].get("status").is_none());
        assert_eq!(
            request[0]["reconciliation"]["prior_actions"],
            serde_json::json!([])
        );
        let input_checksum = request[0]["snapshot"]["input_checksum"]
            .as_str()
            .unwrap()
            .to_string();
        fs.write_string(
            "/repo/review-result.json",
            serde_json::json!({
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": input_checksum,
                "grade": "B+",
                "description": "One substantive action remains.",
                "actions": [{
                    "prior_task_id": null,
                    "title": "Preserve the invariant",
                    "rule_ids": ["APP-001"],
                    "evidence": "app/file.rs:1: changed() omits the invariant"
                }]
            })
            .to_string(),
        )
        .unwrap();
        let first_result = review_result_runner("diff-v1");

        let (result_code, _, result_err) = run_with_runner(
            &["review", "complete", "--result", "review-result.json"],
            &mut fs,
            &first_result,
        );

        assert_eq!(result_code, ExitCode::SUCCESS);
        assert_eq!(result_err, "");
        let failed = &load_state(&fs).reviews["root-review"];
        assert_eq!(failed.grade.unwrap().to_string(), "B+");
        assert_eq!(failed.actions[0].id, "REV-001");

        let stale_runner = review_result_runner("diff-v2");
        let (status_code, status_out, status_err) =
            run_with_runner(&["work", "status"], &mut fs, &stale_runner);

        assert_eq!(status_code, ExitCode::SUCCESS);
        assert_eq!(status_err, "");
        assert!(status_out.contains("`root-review` stale"));
        assert!(status_out.contains("task `REV-001` open"));

        let (address_code, address_out, address_err) = run_with_fs(
            &[
                "work",
                "task",
                "address",
                "REV-001",
                "--summary",
                "Restored the invariant",
            ],
            &mut fs,
        );
        assert_eq!(address_code, ExitCode::SUCCESS);
        assert_eq!(address_err, "");
        assert!(address_out.contains("addressed"));

        let second_request = review_request_runner("diff-v2");
        let (request_code, request_json, _) =
            run_with_runner(&["review", "start", "--json"], &mut fs, &second_request);
        assert_eq!(request_code, ExitCode::SUCCESS);
        let request: serde_json::Value = serde_json::from_str(&request_json).unwrap();
        assert_eq!(
            request[0]["reconciliation"]["prior_actions"][0]["id"],
            "REV-001"
        );
        assert_eq!(
            request[0]["reconciliation"]["prior_actions"][0]["status"],
            "addressed"
        );
        let input_checksum = request[0]["snapshot"]["input_checksum"]
            .as_str()
            .unwrap()
            .to_string();
        fs.write_string(
            "/repo/review-result.json",
            serde_json::json!({
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": input_checksum,
                "grade": "B+",
                "description": "The prior action still remains.",
                "actions": [{
                    "prior_task_id": "REV-001",
                    "title": "Preserve the invariant",
                    "rule_ids": ["APP-001"],
                    "evidence": "app/file.rs:1: the attempted fix still omits the invariant"
                }]
            })
            .to_string(),
        )
        .unwrap();
        let second_result = review_result_runner("diff-v2");

        let (result_code, result_out, result_err) = run_with_runner(
            &["review", "complete", "--result", "review-result.json"],
            &mut fs,
            &second_result,
        );

        assert_eq!(result_code, ExitCode::SUCCESS);
        assert_eq!(result_err, "");
        assert!(result_out.contains("`root-review`: fail"));
        assert!(result_out.contains("`REV-001` open"));
        assert_eq!(
            load_state(&fs).reviews["root-review"].actions[0].status,
            ReviewActionStatus::Open
        );

        let (address_code, _, address_err) = run_with_fs(
            &[
                "work",
                "task",
                "address",
                "REV-001",
                "--summary",
                "Corrected the remaining invariant gap",
            ],
            &mut fs,
        );
        assert_eq!(address_code, ExitCode::SUCCESS);
        assert_eq!(address_err, "");

        let third_request = review_request_runner("diff-v2");
        let (request_code, request_json, request_err) =
            run_with_runner(&["review", "start", "--json"], &mut fs, &third_request);
        assert_eq!(request_code, ExitCode::SUCCESS);
        assert_eq!(request_err, "");
        let request: serde_json::Value = serde_json::from_str(&request_json).unwrap();
        let input_checksum = request[0]["snapshot"]["input_checksum"].as_str().unwrap();
        fs.write_string(
            "/repo/review-result.json",
            serde_json::json!({
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": input_checksum,
                "grade": "A-",
                "description": "The prior action is resolved and no current findings remain.",
                "actions": []
            })
            .to_string(),
        )
        .unwrap();
        let third_result = review_result_runner("diff-v2");
        let (result_code, result_out, result_err) = run_with_runner(
            &["review", "complete", "--result", "review-result.json"],
            &mut fs,
            &third_result,
        );
        assert_eq!(result_code, ExitCode::SUCCESS);
        assert_eq!(result_err, "");
        assert!(result_out.contains("`root-review`: pass"));
        let state = load_state(&fs);
        let passed = &state.reviews["root-review"];
        assert_eq!(passed.status, OperationStatus::Pass);
        assert_eq!(passed.actions.len(), 1);
        assert_eq!(passed.actions[0].status, ReviewActionStatus::Resolved);
        assert_eq!(passed.attempts.len(), 3);
        assert_eq!(passed.attempts[2].resolved_action_ids, vec!["REV-001"]);

        let committed_status = FakeRunner::with_outcomes([
            successful_result("committed-head\n"),
            successful_result("diff-v2"),
            successful_result(""),
        ]);
        let (status_code, status_out, status_err) =
            run_with_runner(&["work", "status"], &mut fs, &committed_status);
        assert_eq!(status_code, ExitCode::SUCCESS);
        assert_eq!(status_err, "");
        assert!(status_out.contains("current pass"));
        assert!(status_out.contains("head `committed-head`"));
        assert!(status_out.contains("rapport integrate"));
        assert_eq!(
            load_state(&fs).reviews["root-review"].head_sha.as_deref(),
            Some("committed-head")
        );

        let requirements = project_context::required_signoff_requirements_for_paths(
            &fs,
            Utf8Path::new("/repo"),
            &[String::from("app/file.rs")],
        )
        .unwrap();
        let exact_runner = review_result_runner("diff-v2");
        let mut state = load_state(&fs);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut context = CommandContext::new(
            Utf8PathBuf::from("/repo"),
            &mut fs,
            &FixedClock,
            &exact_runner,
            &mut out,
            &mut err,
        );
        let (status, packet) =
            review::evaluate_requirement(&mut context, &mut state, &requirements[0], "base123")
                .unwrap();
        assert_eq!(status, OperationStatus::Pass);
        assert!(packet.is_none());

        let new_request_runner = review_request_runner("diff-v3");
        let (request_code, _, request_err) =
            run_with_runner(&["review", "start"], &mut fs, &new_request_runner);
        assert_eq!(request_code, ExitCode::SUCCESS);
        assert_eq!(request_err, "");
        let mut pending_state = load_state(&fs);
        assert_eq!(pending_state.reviews["root-review"].grade, None);
        let repeated_runner = review_result_runner("diff-v3");
        let mut repeated_out = Vec::new();
        let mut repeated_err = Vec::new();
        let mut repeated_context = CommandContext::new(
            Utf8PathBuf::from("/repo"),
            &mut fs,
            &FixedClock,
            &repeated_runner,
            &mut repeated_out,
            &mut repeated_err,
        );
        let (pending_status, repeated_packet) = review::evaluate_requirement(
            &mut repeated_context,
            &mut pending_state,
            &requirements[0],
            "base123",
        )
        .unwrap();
        assert_eq!(pending_status, OperationStatus::Pending);
        assert!(repeated_packet.is_some());
    }

    #[test]
    fn review_explicit_paths_scope_shared_requirements_and_checksums() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/one.rs", "app/two.rs"]);
        fs.add_file_with_contents("/repo/app/one.rs", "fn one() {}\n");
        fs.add_file_with_contents("/repo/app/two.rs", "fn two() {}\n");
        add_review_context(&mut fs);

        let (code, request_json, err) = run_with_runner(
            &["review", "start", "--json", "app/one.rs"],
            &mut fs,
            &review_request_runner("diff-one"),
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        let requests: serde_json::Value = serde_json::from_str(&request_json).unwrap();
        assert_eq!(
            requests[0]["requirement"]["reviewed_paths"],
            serde_json::json!(["app/one.rs"])
        );
        assert_eq!(
            load_state(&fs).reviews["root-review"].reviewed_paths,
            vec!["app/one.rs"]
        );
        assert!(!request_json.contains("app/two.rs"));

        let checksum = requests[0]["snapshot"]["input_checksum"].as_str().unwrap();
        fs.write_string(
            "/repo/review-result.json",
            serde_json::json!({
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": checksum,
                "grade": "A-",
                "description": "The scoped review passes.",
                "actions": []
            })
            .to_string(),
        )
        .unwrap();

        let (result_code, result_out, result_err) = run_with_runner(
            &["review", "complete", "--result", "review-result.json"],
            &mut fs,
            &review_result_runner("diff-one"),
        );

        assert_eq!(result_code, ExitCode::SUCCESS, "{result_err}");
        assert!(result_out.contains("`root-review`: pass"));
        assert_eq!(
            load_state(&fs).reviews["root-review"].reviewed_paths,
            vec!["app/one.rs"]
        );
    }

    #[test]
    fn review_explicit_paths_reject_parent_traversal_and_scope_widening() {
        for requested in ["app/../other", "app"] {
            let mut fs = InMemoryFileSystem::default();
            add_active_work_with_paths(&mut fs, &["app/one.rs"]);
            add_review_context(&mut fs);
            let runner = FakeRunner::successful("must not run");

            let (code, out, err) =
                run_with_runner(&["review", "start", requested], &mut fs, &runner);

            assert_eq!(code, ExitCode::from(2));
            assert_eq!(out, "");
            assert!(
                err.contains("outside the repository") || err.contains("outside active work"),
                "{err}"
            );
            assert!(runner.calls().is_empty());
            assert!(load_state(&fs).reviews.is_empty());
        }
    }

    #[test]
    fn review_result_batch_is_atomic_when_a_later_result_is_invalid() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.add_file_with_contents("/repo/app/file.rs", "fn changed() {}\n");
        add_nested_review_contexts(&mut fs);
        let (request_code, request_json, request_err) = run_with_runner(
            &["review", "start", "--json"],
            &mut fs,
            &two_review_request_runner(),
        );
        assert_eq!(request_code, ExitCode::SUCCESS);
        assert_eq!(request_err, "");
        let requests: serde_json::Value = serde_json::from_str(&request_json).unwrap();
        let checksum = |id: &str| {
            requests
                .as_array()
                .unwrap()
                .iter()
                .find(|request| request["requirement"]["requirement_id"] == id)
                .unwrap()["snapshot"]["input_checksum"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let before = load_state(&fs);
        fs.write_string(
            "/repo/review-results.json",
            serde_json::json!([{
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": checksum("root-review"),
                "grade": "A-",
                "description": "Root review passes.",
                "actions": []
            }, {
                "schema_version": 2,
                "requirement_id": "app-review",
                "input_checksum": "not-the-pending-checksum",
                "grade": "A-",
                "description": "This result is invalid.",
                "actions": []
            }])
            .to_string(),
        )
        .unwrap();

        let (code, out, err) = run_with_runner(
            &["review", "complete", "--result", "review-results.json"],
            &mut fs,
            &review_result_runner(""),
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("invalid review result; details were redacted"));
        assert_eq!(load_state(&fs), before);
    }

    #[test]
    fn review_result_batch_rejects_duplicate_requirement_ids_before_evaluation() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.add_file_with_contents("/repo/app/file.rs", "fn changed() {}\n");
        add_nested_review_contexts(&mut fs);
        let (request_code, request_json, request_err) = run_with_runner(
            &["review", "start", "--json"],
            &mut fs,
            &two_review_request_runner(),
        );
        assert_eq!(request_code, ExitCode::SUCCESS);
        assert_eq!(request_err, "");
        let requests: serde_json::Value = serde_json::from_str(&request_json).unwrap();
        let checksum = requests
            .as_array()
            .unwrap()
            .iter()
            .find(|request| request["requirement"]["requirement_id"] == "root-review")
            .unwrap()["snapshot"]["input_checksum"]
            .as_str()
            .unwrap();
        fs.write_string(
            "/repo/review-results.json",
            serde_json::json!([{
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": checksum,
                "grade": "A-",
                "description": "First duplicate.",
                "actions": []
            }, {
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": checksum,
                "grade": "A-",
                "description": "Second duplicate.",
                "actions": []
            }])
            .to_string(),
        )
        .unwrap();
        let before = load_state(&fs);

        let (code, out, err) = run_with_fs(
            &["review", "complete", "--result", "review-results.json"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("invalid review result; details were redacted"));
        assert_eq!(load_state(&fs), before);
    }

    #[test]
    fn review_complete_assigns_work_global_task_ids_across_requirements() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.add_file_with_contents("/repo/app/file.rs", "fn changed() {}\n");
        add_nested_review_contexts(&mut fs);
        let (request_code, request_json, request_err) = run_with_runner(
            &["review", "start", "--json"],
            &mut fs,
            &two_review_request_runner(),
        );
        assert_eq!(request_code, ExitCode::SUCCESS);
        assert_eq!(request_err, "");
        let requests: serde_json::Value = serde_json::from_str(&request_json).unwrap();
        let checksum = |id: &str| {
            requests
                .as_array()
                .unwrap()
                .iter()
                .find(|request| request["requirement"]["requirement_id"] == id)
                .unwrap()["snapshot"]["input_checksum"]
                .as_str()
                .unwrap()
                .to_string()
        };
        fs.write_string(
            "/repo/review-results.json",
            serde_json::json!([{
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": checksum("root-review"),
                "grade": "B+",
                "description": "Root finding.",
                "actions": [{
                    "prior_task_id": null,
                    "title": "Root task",
                    "rule_ids": ["ROOT-001"],
                    "evidence": "app/file.rs:1: root evidence"
                }]
            }, {
                "schema_version": 2,
                "requirement_id": "app-review",
                "input_checksum": checksum("app-review"),
                "grade": "B+",
                "description": "Application finding.",
                "actions": [{
                    "prior_task_id": null,
                    "title": "Application task",
                    "rule_ids": ["APP-001"],
                    "evidence": "app/file.rs:1: application evidence"
                }]
            }])
            .to_string(),
        )
        .unwrap();
        let runner = FakeRunner::with_outcomes([
            successful_result("abc123\n"),
            successful_result(""),
            successful_result(""),
            successful_result("abc123\n"),
            successful_result(""),
            successful_result(""),
        ]);

        let (code, out, err) = run_with_runner(
            &["review", "complete", "--result", "review-results.json"],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("`REV-001` open"));
        assert!(out.contains("`REV-002` open"));
        let state = load_state(&fs);
        assert_eq!(state.reviews["root-review"].actions[0].id, "REV-001");
        assert_eq!(state.reviews["app-review"].actions[0].id, "REV-002");
    }

    #[test]
    fn build_service_reuses_only_an_exact_matching_result() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["app/file.rs"]);
        fs.add_file_with_contents("/repo/app/file.rs", "fn changed() {}\n");
        fs.write_string(
            "/repo/context.toml",
            r#"version = 1
purpose = "Repository"
signoffs = ["ci"]
"#,
        )
        .unwrap();
        let requirements = project_context::required_signoff_requirements_for_paths(
            &fs,
            Utf8Path::new("/repo"),
            &[String::from("app/file.rs")],
        )
        .unwrap();
        let mut state = load_state(&fs);
        let first_runner = build_signoff_runner("", successful_outcome("checked\n"));
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut first_context = CommandContext::new(
            Utf8PathBuf::from("/repo"),
            &mut fs,
            &FixedClock,
            &first_runner,
            &mut out,
            &mut err,
        );
        let first = build::evaluate_requirement(
            &mut first_context,
            &mut state,
            &requirements[0],
            "base123",
        )
        .unwrap();
        assert!(!first.reused);
        drop(first_context);

        let exact_runner = review_result_runner("");
        let mut exact_context = CommandContext::new(
            Utf8PathBuf::from("/repo"),
            &mut fs,
            &FixedClock,
            &exact_runner,
            &mut out,
            &mut err,
        );
        let exact = build::evaluate_requirement(
            &mut exact_context,
            &mut state,
            &requirements[0],
            "base123",
        )
        .unwrap();

        assert!(exact.reused);
        assert!(
            exact_runner
                .calls()
                .iter()
                .all(|(spec, _)| spec.program == "git")
        );
        drop(exact_context);

        let changed_runner =
            build_signoff_runner("changed-diff", successful_outcome("checked again\n"));
        let mut changed_context = CommandContext::new(
            Utf8PathBuf::from("/repo"),
            &mut fs,
            &FixedClock,
            &changed_runner,
            &mut out,
            &mut err,
        );
        let changed = build::evaluate_requirement(
            &mut changed_context,
            &mut state,
            &requirements[0],
            "base123",
        )
        .unwrap();

        assert!(!changed.reused);
        assert!(
            changed_runner
                .calls()
                .iter()
                .any(|(spec, _)| spec == &CommandSpec::new("just", ["ci"]))
        );
    }

    #[test]
    fn integrate_requires_active_work_paths() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &[]);
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
        assert!(err.contains("Active work has no paths to integrate"));
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
    fn integrate_commits_creates_pr_and_reports_context_signoffs() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.write_string(
            "/repo/context.toml",
            r#"
version = 1
purpose = "Repository"
signoffs = ["shared", "review"]
"#,
        )
        .unwrap();
        add_generated_signoff_contract(&mut fs, "/repo", &["shared", "review"]);
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

        assert_eq!(code, ExitCode::SUCCESS, "{err}");
        assert_eq!(err, "");
        assert!(out.contains("pr_created"));
        assert!(out.contains("https://github.com/hedge-ops/rapport/pull/70"));
        assert!(out.contains("passed `root-build-shared`"));
        assert!(out.contains("passed `root-build-review`"));
        let calls = runner.calls();
        assert!(calls.iter().any(|(spec, _)| {
            spec == &CommandSpec::new(
                "git",
                [
                    "push",
                    "--set-upstream",
                    "origin",
                    "work/issue-57-integrate",
                ],
            )
        }));
        assert!(calls.iter().any(|(spec, cwd)| {
            spec == &CommandSpec::new("just", ["shared"]) && cwd == Utf8Path::new("/repo")
        }));
        assert!(calls.iter().any(|(spec, cwd)| {
            spec == &CommandSpec::new("just", ["review"]) && cwd == Utf8Path::new("/repo")
        }));
        assert!(calls.iter().any(|(spec, _)| {
            spec == &CommandSpec::new("git", ["merge-base", "abc123", "base123"])
        }));
        assert!(calls.iter().any(|(spec, _)| {
            spec == &CommandSpec::new("git", ["fetch", "--no-tags", "origin", "base123"])
        }));
        let state = load_state(&fs);
        assert!(
            state
                .builds
                .values()
                .all(|build| build.base_sha.as_deref() == Some("merge123"))
        );
        let integrate = state.integrate.unwrap();
        let signoff = state.signoff.unwrap();

        assert_eq!(integrate.status, "pr_created");
        assert_eq!(integrate.branch.as_deref(), Some("work/issue-57-integrate"));
        assert_eq!(integrate.commit.as_deref(), Some("abc123"));
        assert_eq!(
            integrate.pr_url.as_deref(),
            Some("https://github.com/hedge-ops/rapport/pull/70")
        );
        assert_eq!(signoff.status, "pass");
        assert_eq!(
            signoff.required,
            vec!["root-build-shared", "root-build-review"]
        );
        assert_eq!(
            signoff.passed,
            vec!["root-build-shared", "root-build-review"]
        );
        assert!(signoff.pending.is_empty());
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
    #[expect(
        clippy::too_many_lines,
        reason = "the resumable integration scenario includes missing, pending, exact pass, and remote-success revalidation phases"
    )]
    fn integrate_records_pr_before_signoff_and_resumes_same_pr() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.write_string(
            "/repo/context.toml",
            "version = 1\npurpose = \"Repository\"\nsignoffs = [\"ci\"]\n",
        )
        .unwrap();
        add_generated_signoff_contract(&mut fs, "/repo", &["ci"]);
        let branch = "work/resumable";
        let mut first_outcomes = vec![
            successful_result(" M crates/rapport/src/lib.rs\n M .rapport/work.toml\n"),
            successful_result(&format!("{branch}\n")),
            successful_result("base123\n"),
            successful_result(""),
            successful_result(""),
            successful_result("abc123\n"),
            successful_result(""),
        ];
        push_local_identity(&mut first_outcomes, branch);
        first_outcomes.extend([
            successful_result(""),
            successful_result("[]"),
            successful_result("https://github.com/hedge-ops/rapport/pull/70\n"),
        ]);
        push_pr_identity(&mut first_outcomes, branch);
        first_outcomes.push(successful_result(""));
        push_pr_identity(&mut first_outcomes, branch);
        first_outcomes.push(successful_result(r#"{"statuses":[]}"#));
        let first = FakeRunner::with_outcomes(first_outcomes);

        let (first_code, _, first_err) = run_with_runner(
            &[
                "integrate",
                "--summary",
                "Resumable integration",
                "--message",
                "Exercise two phases",
            ],
            &mut fs,
            &first,
        );

        assert_eq!(first_code, ExitCode::from(2));
        assert!(first_err.contains("PR signoff statuses do not match context"));
        let pending = load_state(&fs);
        let integration = pending.integrate.unwrap();
        let signoff = pending.signoff.unwrap();
        assert_eq!(integration.status, "pr_created");
        assert_eq!(integration.commit.as_deref(), Some("abc123"));
        assert_eq!(
            integration.pr_url.as_deref(),
            Some("https://github.com/hedge-ops/rapport/pull/70")
        );
        assert_eq!(signoff.status, "pending");
        assert_eq!(signoff.pending, vec!["root-build-ci"]);

        let mut second_outcomes = vec![successful_result("")];
        push_pr_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(""));
        push_pr_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-build-ci","state":"pending"}]}"#,
        ));
        push_successful_build_signoff(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(""));
        push_pr_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-build-ci","state":"success"}]}"#,
        ));
        let second = FakeRunner::with_outcomes(second_outcomes);

        let (second_code, second_out, second_err) =
            run_with_runner(&["integrate"], &mut fs, &second);

        assert_eq!(second_code, ExitCode::SUCCESS);
        assert_eq!(second_err, "");
        assert!(second_out.contains("passed `root-build-ci`"));
        assert!(
            second
                .calls()
                .iter()
                .any(|(spec, _)| spec == &CommandSpec::new("just", ["ci"]))
        );
        let completed = load_state(&fs);
        assert_eq!(completed.signoff.unwrap().status, "pass");

        let mut third_outcomes = vec![successful_result("")];
        push_pr_identity(&mut third_outcomes, branch);
        third_outcomes.push(successful_result(""));
        push_pr_identity(&mut third_outcomes, branch);
        third_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-build-ci","state":"success"}]}"#,
        ));
        push_successful_build_signoff_with_diff(&mut third_outcomes, branch, "changed-diff");
        third_outcomes.push(successful_result(""));
        push_pr_identity(&mut third_outcomes, branch);
        third_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-build-ci","state":"success"}]}"#,
        ));
        let third = FakeRunner::with_outcomes(third_outcomes);

        let (third_code, _, third_err) = run_with_runner(&["integrate"], &mut fs, &third);

        assert_eq!(third_code, ExitCode::SUCCESS);
        assert_eq!(third_err, "");
        assert!(
            third
                .calls()
                .iter()
                .any(|(spec, _)| spec == &CommandSpec::new("just", ["ci"])),
            "a remote success must not bypass a changed local exact-input proof"
        );
    }

    #[test]
    fn integrate_refuses_to_sign_off_a_dirty_worktree() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let runner = FakeRunner::successful(" M crates/rapport/src/lib.rs\n");

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("worktree must be completely clean"));
        assert_eq!(
            runner.calls(),
            vec![(
                CommandSpec::new("git", ["status", "--porcelain"]),
                Utf8PathBuf::from("/repo")
            )]
        );
    }

    #[test]
    fn typed_review_integration_stays_pending_and_resumes_after_result() {
        let mut fs = InMemoryFileSystem::default();
        add_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_review_context(&mut fs);
        let request = signoff_contract::SignoffRequest::new(
            Utf8Path::new("/repo"),
            Utf8Path::new("/repo"),
            signoff_contract::SignoffKind::Review,
            "review",
            Some("A-".parse().unwrap()),
        )
        .unwrap();
        signoff_contract::write_shared(&mut fs, Utf8Path::new("/repo")).unwrap();
        signoff_contract::write_request(&mut fs, Utf8Path::new("/repo"), &request).unwrap();

        let branch = "work/review-resume";
        let mut first_outcomes = vec![
            successful_result(" M crates/rapport/src/lib.rs\n M .rapport/work.toml\n"),
            successful_result(&format!("{branch}\n")),
            successful_result("base123\n"),
            successful_result(""),
            successful_result("[work/review-resume abc123] review\n"),
            successful_result("abc123\n"),
            successful_result(""),
        ];
        push_local_identity(&mut first_outcomes, branch);
        first_outcomes.extend([
            successful_result(""),
            successful_result("[]"),
            successful_result("https://github.com/hedge-ops/rapport/pull/87\n"),
        ]);
        push_pr_identity(&mut first_outcomes, branch);
        first_outcomes.push(successful_result(""));
        push_pr_identity(&mut first_outcomes, branch);
        first_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-review","state":"pending"}]}"#,
        ));
        push_pending_review_signoff(&mut first_outcomes, branch);
        first_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-review","state":"pending"}]}"#,
        ));
        let first = FakeRunner::with_outcomes(first_outcomes);

        let (first_code, _, first_err) = run_with_runner(
            &[
                "integrate",
                "--summary",
                "Typed review",
                "--message",
                "Exercise review resume",
            ],
            &mut fs,
            &first,
        );
        assert_eq!(first_code, ExitCode::from(2));
        assert!(first_err.contains("requires an independent structured result"));
        let input_checksum = load_state(&fs).reviews["root-review"]
            .input_checksum
            .clone();
        fs.write_string(
            "/repo/review-result.json",
            serde_json::json!({
                "schema_version": 2,
                "requirement_id": "root-review",
                "input_checksum": input_checksum,
                "grade": "A-",
                "description": "No current actions.",
                "actions": []
            })
            .to_string(),
        )
        .unwrap();
        let result_runner = review_result_runner("");
        let (result_code, _, result_err) = run_with_runner(
            &["review", "complete", "--result", "review-result.json"],
            &mut fs,
            &result_runner,
        );
        assert_eq!(result_code, ExitCode::SUCCESS);
        assert_eq!(result_err, "");

        let mut second_outcomes = vec![successful_result("")];
        push_pr_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(""));
        push_pr_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-review","state":"pending"}]}"#,
        ));
        push_passing_review_signoff(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(""));
        push_pr_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-review","state":"success"}]}"#,
        ));
        let second = FakeRunner::with_outcomes(second_outcomes);

        let (second_code, second_out, second_err) =
            run_with_runner(&["integrate"], &mut fs, &second);
        assert_eq!(second_code, ExitCode::SUCCESS);
        assert_eq!(second_err, "");
        assert!(second_out.contains("passed `root-review`"));
        assert_eq!(load_state(&fs).signoff.unwrap().status, "pass");
    }

    #[test]
    fn integrate_refuses_success_when_target_dirties_worktree() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_root_signoff(&mut fs, "ci");
        let branch = "work/issue-70-complete";
        let mut outcomes = vec![successful_result("")];
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(""));
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-build-ci","state":"pending"}]}"#,
        ));
        outcomes.push(successful_result(""));
        push_local_identity(&mut outcomes, branch);
        outcomes.push(successful_result("abc123\n"));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(" M crates/rapport/src/lib.rs\n"));
        outcomes.push(successful_result(""));
        let runner = FakeRunner::with_outcomes(outcomes);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("worktree must be completely clean"));
        let post = runner.calls().last().unwrap().0.clone();
        assert!(post.args.iter().any(|arg| arg == "state=failure"));
        assert!(!post.args.iter().any(|arg| arg == "state=success"));
    }

    #[test]
    fn integrate_rejects_a_pr_base_advance_during_signoff() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_root_signoff(&mut fs, "ci");
        let branch = "work/issue-70-complete";
        let mut outcomes = vec![successful_result("")];
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(""));
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-build-ci","state":"pending"}]}"#,
        ));
        outcomes.push(successful_result(""));
        push_local_identity(&mut outcomes, branch);
        outcomes.push(successful_result("abc123\n"));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        push_pr_identity_with_base(&mut outcomes, branch, "base456", "merge456");
        outcomes.push(successful_result(""));
        let runner = FakeRunner::with_outcomes(outcomes);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("PR base changed while signoff was running"));
        let post = runner.calls().last().unwrap().0.clone();
        assert!(post.args.iter().any(|arg| arg == "state=failure"));
    }

    #[test]
    fn integrate_resumes_publishing_and_reuses_existing_pr() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        record_publishing_integration(&mut fs, "work/resumable");
        let pr_url = "https://github.com/hedge-ops/rapport/pull/70";
        let branch = "work/resumable";
        let mut outcomes = vec![successful_result(""), successful_result("")];
        push_local_identity(&mut outcomes, branch);
        outcomes.extend([
            successful_result(""),
            successful_result(&format!(r#"[{{"url":"{pr_url}"}}]"#)),
            successful_result(""),
        ]);
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(""));
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(r#"{"statuses":[]}"#));
        outcomes.push(successful_result(""));
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(r#"{"statuses":[]}"#));
        let runner = FakeRunner::with_outcomes(outcomes);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(pr_url));
        let calls = runner.calls();
        assert!(
            calls.iter().any(|(spec, _)| spec.program == "gh"
                && spec.args.get(1).is_some_and(|arg| arg == "edit"))
        );
        assert!(!calls.iter().any(|(spec, _)| spec.program == "git"
            && spec.args.first().is_some_and(|arg| arg == "commit")));
        let state = load_state(&fs);
        assert_eq!(state.integrate.unwrap().status, "pr_created");
        assert_eq!(state.signoff.unwrap().status, "none");
    }

    #[test]
    fn integrate_validates_publishing_identity_before_push() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        record_publishing_integration(&mut fs, "work/resumable");
        let runner = FakeRunner::with_outcomes([
            successful_result(""),
            successful_result(""),
            successful_result("different123\n"),
            successful_result("work/resumable\n"),
        ]);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("does not match integrated commit"));
        assert!(
            !runner
                .calls()
                .iter()
                .any(|(spec, _)| spec.args.first().is_some_and(|arg| arg == "push"))
        );
    }

    #[test]
    fn integrate_recovers_commit_created_before_publication_was_saved() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        record_commit_intent(&mut fs, "work/recover-commit");
        let mut outcomes = vec![
            successful_result("work/recover-commit\n"),
            successful_result("abc123\n"),
            successful_result(""),
            successful_result("base123\n"),
            successful_result("Recover commit\n\nPersist before commit"),
            successful_result(""),
        ];
        push_local_identity(&mut outcomes, "work/recover-commit");
        outcomes.push(Ok(CommandOutcome {
            success: false,
            stdout: String::new(),
            stderr: String::from("push rejected"),
        }));
        let runner = FakeRunner::with_outcomes(outcomes);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("`git push` failed"));
        assert!(!err.contains("push rejected"));
        let integration = load_state(&fs).integrate.unwrap();
        assert_eq!(integration.status, "publishing");
        assert_eq!(integration.commit.as_deref(), Some("abc123"));
    }

    #[test]
    fn integrate_records_publishing_before_push_failure() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let mut outcomes = vec![
            successful_result(" M crates/rapport/src/lib.rs\n"),
            successful_result("work/recover-push\n"),
            successful_result("base123\n"),
            successful_result(""),
            successful_result(""),
            successful_result("abc123\n"),
            successful_result(""),
        ];
        push_local_identity(&mut outcomes, "work/recover-push");
        outcomes.push(Ok(CommandOutcome {
            success: false,
            stdout: String::new(),
            stderr: String::from("push rejected"),
        }));
        let runner = FakeRunner::with_outcomes(outcomes);

        let (code, out, err) = run_with_runner(
            &[
                "integrate",
                "--summary",
                "Recover push",
                "--message",
                "Persist before remote side effects",
            ],
            &mut fs,
            &runner,
        );

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("`git push` failed"));
        assert!(!err.contains("push rejected"));
        let integration = load_state(&fs).integrate.unwrap();
        assert_eq!(integration.status, "publishing");
        assert_eq!(integration.branch.as_deref(), Some("work/recover-push"));
        assert_eq!(integration.commit.as_deref(), Some("abc123"));
        assert_eq!(
            integration.message.as_deref(),
            Some("Persist before remote side effects")
        );
    }

    #[test]
    fn integrate_rejects_closed_pr_on_resume() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let mut state = load_state(&fs);
        state.signoff = Some(WorkFact::new("pass"));
        WorkStateStore::new(RapportPaths::new("/repo"))
            .save(&mut fs, &state)
            .unwrap();
        let runner = FakeRunner::with_outcomes([
            Ok(successful_outcome("")),
            Ok(successful_outcome("abc123\n")),
            Ok(successful_outcome("work/issue-70-complete\n")),
            Ok(successful_outcome(&pull_request_json(
                "work/issue-70-complete",
                "CLOSED",
                false,
            ))),
        ]);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("signoff requires an open PR"));
        assert_eq!(load_state(&fs).signoff.unwrap().status, "pass");
    }

    #[test]
    fn integrate_rejects_fork_pr_on_resume() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        let runner = FakeRunner::with_outcomes([
            Ok(successful_outcome("")),
            Ok(successful_outcome("abc123\n")),
            Ok(successful_outcome("work/issue-70-complete\n")),
            Ok(successful_outcome(&pull_request_json(
                "work/issue-70-complete",
                "OPEN",
                true,
            ))),
        ]);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("fork pull requests are not supported"));
    }

    #[test]
    fn integrate_rejects_multiple_prs_for_publishing_branch() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        record_publishing_integration(&mut fs, "work/ambiguous");
        let mut outcomes = vec![successful_result(""), successful_result("")];
        push_local_identity(&mut outcomes, "work/ambiguous");
        outcomes.extend([
            successful_result(""),
            successful_result(
                r#"[{"url":"https://github.com/hedge-ops/rapport/pull/70"},{"url":"https://github.com/hedge-ops/rapport/pull/71"}]"#,
            ),
        ]);
        let runner = FakeRunner::with_outcomes(outcomes);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("`gh pr list` returned unexpected output"));
        assert_eq!(load_state(&fs).integrate.unwrap().status, "publishing");
    }

    #[test]
    fn integrate_rejects_unexpected_status_added_during_signoff() {
        let mut fs = InMemoryFileSystem::default();
        add_integrated_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        add_root_signoff(&mut fs, "ci");
        let branch = "work/issue-70-complete";
        let mut outcomes = vec![successful_result("")];
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(""));
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-build-ci","state":"pending"}]}"#,
        ));
        push_successful_build_signoff(&mut outcomes, branch);
        outcomes.push(successful_result(""));
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(
                r#"{"statuses":[{"context":"signoff: root-build-ci","state":"success"},{"context":"signoff: unexpected","state":"pending"}]}"#,
        ));
        let runner = FakeRunner::with_outcomes(outcomes);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("PR signoff statuses do not match context"));
        assert_eq!(load_state(&fs).signoff.unwrap().status, "pending");
    }

    #[test]
    fn integrate_records_context_signoff_resolution_failure() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.write_string("/repo/context.toml", "version =").unwrap();
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
        assert!(err.contains("Could not evaluate signoff requirements"));
        assert!(err.contains("invalid signoff contract"));
        assert!(!err.contains("context parse error"));
        assert!(load_state(&fs).signoff.is_none());
        assert!(runner.calls().is_empty());
        let events = events(&fs);

        assert_eq!(events[0].command, "integrate signoff");
        assert_eq!(events[0].outcome, CommandEventOutcome::Failure);
        assert_eq!(events[1].command, "integrate");
        assert_eq!(events[1].outcome, CommandEventOutcome::Failure);
    }

    #[test]
    fn integrate_fails_before_side_effects_when_signoff_workflow_is_missing() {
        let mut fs = InMemoryFileSystem::default();
        add_built_active_work_with_paths(&mut fs, &["crates/rapport/src/lib.rs"]);
        fs.write_string(
            "/repo/context.toml",
            "version = 1\npurpose = \"Repository\"\nsignoffs = [\"ci\"]\n",
        )
        .unwrap();
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
        assert!(err.contains("invalid signoff contract"));
        assert!(!err.contains("rapport-signoff.yml"));
        assert!(runner.calls().is_empty());
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

    fn add_editable_context(fs: &mut InMemoryFileSystem) {
        fs.write_string(
            "/repo/app/core/domain/context.toml",
            r#"
version = 1
purpose = "Old purpose"
rule_includes = []

[ownership]
owns = []
boundaries = []
"#,
        )
        .unwrap();
        fs.write_string(
            "/repo/rules/rust.toml",
            r#"
version = 1

[[rules]]
id = "RUST-001"
text = "Use rustfmt."
"#,
        )
        .unwrap();
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

    fn add_generated_signoff_contract(
        fs: &mut InMemoryFileSystem,
        context_directory: &str,
        targets: &[&str],
    ) {
        signoff_contract::write_shared(fs, Utf8Path::new("/repo")).unwrap();
        for target in targets {
            let request = signoff_contract::SignoffRequest::new(
                Utf8Path::new("/repo"),
                Utf8Path::new(context_directory),
                signoff_contract::SignoffKind::Build,
                target,
                None,
            )
            .unwrap();
            signoff_contract::write_request(fs, Utf8Path::new("/repo"), &request).unwrap();
        }
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

[signoff]
status = "none"
at = "2026-07-07T23:00:00Z"
summary = "no signoffs configured"
"#
            ),
        )
        .unwrap();
    }

    fn record_publishing_integration(fs: &mut InMemoryFileSystem, branch: &str) {
        let mut state = load_state(fs);
        let mut integration = WorkFact::new("publishing").summary("Resume publication");
        integration.message = Some(String::from("Persisted PR body"));
        integration.branch = Some(branch.to_string());
        integration.commit = Some(String::from("abc123"));
        state.integrate = Some(integration);
        state.signoff = None;
        WorkStateStore::new(RapportPaths::new("/repo"))
            .save(fs, &state)
            .unwrap();
    }

    fn record_commit_intent(fs: &mut InMemoryFileSystem, branch: &str) {
        let mut state = load_state(fs);
        let mut integration = WorkFact::new("committing").summary("Recover commit");
        integration.message = Some(String::from("Persist before commit"));
        integration.branch = Some(branch.to_string());
        integration.commit = Some(String::from("base123"));
        state.integrate = Some(integration);
        state.signoff = None;
        WorkStateStore::new(RapportPaths::new("/repo"))
            .save(fs, &state)
            .unwrap();
    }

    fn add_root_signoff(fs: &mut InMemoryFileSystem, target: &str) {
        fs.write_string(
            "/repo/context.toml",
            format!("version = 1\npurpose = \"Repository\"\nsignoffs = [\"{target}\"]\n"),
        )
        .unwrap();
        add_generated_signoff_contract(fs, "/repo", &[target]);
    }

    fn pull_request_json(branch: &str, state: &str, is_cross_repository: bool) -> String {
        pull_request_json_with_base(branch, state, is_cross_repository, "base123")
    }

    fn pull_request_json_with_base(
        branch: &str,
        state: &str,
        is_cross_repository: bool,
        base_sha: &str,
    ) -> String {
        format!(
            r#"{{"baseRefOid":"{base_sha}","headRefOid":"abc123","headRefName":"{branch}","isCrossRepository":{is_cross_repository},"state":"{state}","url":"https://github.com/hedge-ops/rapport/pull/70"}}"#
        )
    }

    type FakeOutcome = io::Result<CommandOutcome>;

    #[expect(
        clippy::unnecessary_wraps,
        reason = "test helper builds io::Result queues consumed by the fake runner"
    )]
    fn successful_result(stdout: &str) -> FakeOutcome {
        Ok(successful_outcome(stdout))
    }

    fn push_local_identity(outcomes: &mut Vec<FakeOutcome>, branch: &str) {
        outcomes.push(successful_result("abc123\n"));
        outcomes.push(successful_result(&format!("{branch}\n")));
    }

    fn push_pr_identity(outcomes: &mut Vec<FakeOutcome>, branch: &str) {
        push_pr_identity_with_base(outcomes, branch, "base123", "merge123");
    }

    fn push_pr_identity_with_base(
        outcomes: &mut Vec<FakeOutcome>,
        branch: &str,
        base_sha: &str,
        merge_base_sha: &str,
    ) {
        push_local_identity(outcomes, branch);
        outcomes.push(successful_result(&pull_request_json_with_base(
            branch, "OPEN", false, base_sha,
        )));
        outcomes.push(successful_result("hedge-ops/rapport\n"));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(&format!("{merge_base_sha}\n")));
    }

    fn successful_integrate_runner() -> FakeRunner {
        let branch = "work/issue-57-integrate";
        let mut outcomes = vec![
            successful_result(" M crates/rapport/src/lib.rs\n M .rapport/work.toml\n"),
            successful_result(&format!("{branch}\n")),
            successful_result("base123\n"),
            successful_result(""),
            successful_result("[work/issue-57-integrate abc123] PW-356\n"),
            successful_result("abc123\n"),
            successful_result(""),
        ];
        push_local_identity(&mut outcomes, branch);
        outcomes.extend([
            successful_result(""),
            successful_result("[]"),
            successful_result("https://github.com/hedge-ops/rapport/pull/70\n"),
        ]);
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(""));
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(
                r#"{"statuses":[{"context":"signoff: root-build-shared","state":"pending"},{"context":"signoff: root-build-review","state":"pending"}]}"#,
        ));
        for _target in ["shared", "review"] {
            push_successful_build_signoff(&mut outcomes, branch);
        }
        outcomes.push(successful_result(""));
        push_pr_identity(&mut outcomes, branch);
        outcomes.push(successful_result(
                r#"{"statuses":[{"context":"signoff: root-build-shared","state":"success"},{"context":"signoff: root-build-review","state":"success"}]}"#,
        ));
        FakeRunner::with_outcomes(outcomes)
    }

    fn build_runner(outcome: CommandOutcome) -> FakeRunner {
        FakeRunner::with_outcomes([
            successful_result("head123\n"),
            successful_result("origin/main\n"),
            successful_result("base123\n"),
            successful_result(""),
            successful_result(""),
            Ok(outcome),
        ])
    }

    fn build_signoff_runner(diff: &str, outcome: CommandOutcome) -> FakeRunner {
        FakeRunner::with_outcomes([
            successful_result("abc123\n"),
            successful_result(diff),
            successful_result(""),
            Ok(outcome),
        ])
    }

    fn review_request_runner(diff: &str) -> FakeRunner {
        FakeRunner::with_outcomes([
            successful_result("head123\n"),
            successful_result("origin/main\n"),
            successful_result("base123\n"),
            successful_result(diff),
            successful_result(""),
        ])
    }

    fn review_result_runner(diff: &str) -> FakeRunner {
        FakeRunner::with_outcomes([
            successful_result("abc123\n"),
            successful_result(diff),
            successful_result(""),
        ])
    }

    fn add_review_context(fs: &mut InMemoryFileSystem) {
        fs.write_string(
            "/repo/context.toml",
            r#"version = 1
purpose = "Repository"
rule_includes = []

[ownership]
owns = []
boundaries = []

[[signoffs]]
kind = "review"
minimum_grade = "A-"

[[rules]]
id = "APP-001"
text = "Preserve the application invariant."
references = []
"#,
        )
        .unwrap();
    }

    fn add_nested_review_contexts(fs: &mut InMemoryFileSystem) {
        for (path, purpose, rule_id) in [
            ("/repo/context.toml", "Repository", "ROOT-001"),
            ("/repo/app/context.toml", "Application", "APP-001"),
        ] {
            fs.write_string(
                path,
                format!(
                    r#"version = 1
purpose = "{purpose}"

[[signoffs]]
kind = "review"
minimum_grade = "A-"

[[rules]]
id = "{rule_id}"
text = "Review {purpose} behavior."
references = []
"#
                ),
            )
            .unwrap();
        }
    }

    fn two_review_request_runner() -> FakeRunner {
        FakeRunner::with_outcomes([
            successful_result("head123\n"),
            successful_result("origin/main\n"),
            successful_result("base123\n"),
            successful_result(""),
            successful_result(""),
            successful_result("head123\n"),
            successful_result("origin/main\n"),
            successful_result("base123\n"),
            successful_result(""),
            successful_result(""),
        ])
    }

    fn push_successful_build_signoff(outcomes: &mut Vec<FakeOutcome>, branch: &str) {
        push_successful_build_signoff_with_diff(outcomes, branch, "");
    }

    fn push_successful_build_signoff_with_diff(
        outcomes: &mut Vec<FakeOutcome>,
        branch: &str,
        diff: &str,
    ) {
        outcomes.push(successful_result(""));
        push_local_identity(outcomes, branch);
        outcomes.push(successful_result("abc123\n"));
        outcomes.push(successful_result(diff));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        push_pr_identity(outcomes, branch);
        outcomes.push(successful_result(""));
    }

    fn push_pending_review_signoff(outcomes: &mut Vec<FakeOutcome>, branch: &str) {
        outcomes.push(successful_result(""));
        push_local_identity(outcomes, branch);
        outcomes.push(successful_result("abc123\n"));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
    }

    fn push_passing_review_signoff(outcomes: &mut Vec<FakeOutcome>, branch: &str) {
        outcomes.push(successful_result(""));
        push_local_identity(outcomes, branch);
        outcomes.push(successful_result("abc123\n"));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        push_pr_identity(outcomes, branch);
        outcomes.push(successful_result(""));
    }

    fn successful_outcome(stdout: &str) -> CommandOutcome {
        CommandOutcome {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }
}

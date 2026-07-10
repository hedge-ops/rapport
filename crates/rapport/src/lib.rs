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
mod rules;
mod runner;
mod signoff_contract;
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
        },
        Command::Context(context_args) => {
            project_context::run(&context_args.command, argv, context)
        }
        Command::Build(build_args) => build::run(build_args, argv, context),
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
        assert!(
            out.contains(
                "prime -> doctor -> work -> context -> build -> integrate -> work complete"
            )
        );
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
        assert!(
            out.contains(
                "prime -> doctor -> work -> context -> build -> integrate -> work complete"
            )
        );
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
        assert!(
            out.contains("validated 1 context.toml file, 0 signoff targets, and 1 rules.toml file")
        );
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
            &["context", "signoff", "add", "app/core/domain", "ci"],
            &mut fs,
        );

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("signoff: app-core-domain-ci"));
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();
        assert!(context.contains("signoffs = [\n  \"ci\","));
        let shared = fs
            .read_to_string("/repo/.github/workflows/rapport-signoff.yml")
            .unwrap();
        assert!(shared.contains("context=signoff: ${TARGET}"));
        let request = fs
            .read_to_string("/repo/.github/workflows/rapport-app-core-domain-ci.yml")
            .unwrap();
        assert!(request.contains("- \"app/core/domain/**\""));
        assert!(request.contains("target: app-core-domain-ci"));
        assert!(!request.contains("runs-on:"));
    }

    #[test]
    fn context_signoff_repair_and_remove_own_generated_workflow() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);
        let request_path = "/repo/.github/workflows/rapport-app-core-domain-ci.yml";
        let _ = run_with_fs(
            &["context", "signoff", "add", "app/core/domain", "ci"],
            &mut fs,
        );
        fs.write_string(request_path, "changed\n").unwrap();

        let (repair_code, _, repair_err) = run_with_fs(
            &["context", "signoff", "repair", "app/core/domain", "ci"],
            &mut fs,
        );
        let repaired = fs.read_to_string(request_path).unwrap();
        let (remove_code, _, remove_err) = run_with_fs(
            &["context", "signoff", "remove", "app/core/domain", "ci"],
            &mut fs,
        );

        assert_eq!(repair_code, ExitCode::SUCCESS);
        assert_eq!(repair_err, "");
        assert!(repaired.contains("target: app-core-domain-ci"));
        assert_eq!(remove_code, ExitCode::SUCCESS);
        assert_eq!(remove_err, "");
        assert!(!fs.is_file(request_path));
        let context = fs
            .read_to_string("/repo/app/core/domain/context.toml")
            .unwrap();
        assert!(context.contains("signoffs = []"));
    }

    #[test]
    fn context_signoff_add_rejects_invalid_target_before_writing() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);

        let (code, out, err) = run_with_fs(
            &["context", "signoff", "add", "app/core/domain", "Not Valid"],
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
    fn doctor_rejects_drifted_signoff_request_workflow() {
        let mut fs = InMemoryFileSystem::default();
        add_editable_context(&mut fs);
        let _ = run_with_fs(
            &["context", "signoff", "add", "app/core/domain", "ci"],
            &mut fs,
        );
        fs.write_string(
            "/repo/.github/workflows/rapport-app-core-domain-ci.yml",
            "changed\n",
        )
        .unwrap();
        let runner = FakeRunner::successful("git@github.com:hedge-ops/rapport.git\n");

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("has drifted from its generated content"));
        assert!(err.contains("rapport-app-core-domain-ci.yml"));
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

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("pr_created"));
        assert!(out.contains("https://github.com/hedge-ops/rapport/pull/70"));
        assert!(out.contains("passed `root-shared`"));
        assert!(out.contains("passed `root-review`"));
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
        assert_eq!(signoff.status, "pass");
        assert_eq!(signoff.required, vec!["root-shared", "root-review"]);
        assert_eq!(signoff.passed, vec!["root-shared", "root-review"]);
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
        assert!(first_err.contains("missing [signoff: root-ci]"));
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
        assert_eq!(signoff.pending, vec!["root-ci"]);

        let mut second_outcomes = vec![successful_result("")];
        push_pr_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(""));
        push_pr_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-ci","state":"pending"}]}"#,
        ));
        second_outcomes.push(successful_result(""));
        push_local_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(""));
        second_outcomes.push(successful_result(""));
        push_local_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(""));
        second_outcomes.push(successful_result(""));
        push_local_identity(&mut second_outcomes, branch);
        second_outcomes.push(successful_result(
            r#"{"statuses":[{"context":"signoff: root-ci","state":"success"}]}"#,
        ));
        let second = FakeRunner::with_outcomes(second_outcomes);

        let (second_code, second_out, second_err) =
            run_with_runner(&["integrate"], &mut fs, &second);

        assert_eq!(second_code, ExitCode::SUCCESS);
        assert_eq!(second_err, "");
        assert!(second_out.contains("passed `root-ci`"));
        assert!(
            second
                .calls()
                .iter()
                .any(|(spec, _)| spec == &CommandSpec::new("just", ["ci"]))
        );
        let completed = load_state(&fs);
        assert_eq!(completed.signoff.unwrap().status, "pass");
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
            r#"{"statuses":[{"context":"signoff: root-ci","state":"pending"}]}"#,
        ));
        outcomes.push(successful_result(""));
        push_local_identity(&mut outcomes, branch);
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
        push_local_identity(&mut outcomes, branch);
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
        assert!(err.contains("push rejected"));
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
        assert!(err.contains("push rejected"));
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
        assert!(err.contains("multiple PRs per branch are unsupported"));
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
            r#"{"statuses":[{"context":"signoff: root-ci","state":"pending"}]}"#,
        ));
        outcomes.push(successful_result(""));
        push_local_identity(&mut outcomes, branch);
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        push_local_identity(&mut outcomes, branch);
        outcomes.push(successful_result(""));
        outcomes.push(successful_result(""));
        push_local_identity(&mut outcomes, branch);
        outcomes.push(successful_result(
                r#"{"statuses":[{"context":"signoff: root-ci","state":"success"},{"context":"signoff: unexpected","state":"pending"}]}"#,
        ));
        let runner = FakeRunner::with_outcomes(outcomes);

        let (code, out, err) = run_with_runner(&["integrate"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("unexpected [signoff: unexpected]"));
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
        assert!(err.contains("context parse error"));
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
        assert!(err.contains("missing Rapport-owned signoff workflow"));
        assert!(err.contains("rapport-signoff.yml"));
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
                target,
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

    fn open_pr_json(branch: &str, is_cross_repository: bool) -> String {
        pull_request_json(branch, "OPEN", is_cross_repository)
    }

    fn pull_request_json(branch: &str, state: &str, is_cross_repository: bool) -> String {
        format!(
            r#"{{"headRefOid":"abc123","headRefName":"{branch}","isCrossRepository":{is_cross_repository},"state":"{state}","url":"https://github.com/hedge-ops/rapport/pull/70"}}"#
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
        push_local_identity(outcomes, branch);
        outcomes.push(successful_result(&open_pr_json(branch, false)));
        outcomes.push(successful_result("hedge-ops/rapport\n"));
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
                r#"{"statuses":[{"context":"signoff: root-shared","state":"pending"},{"context":"signoff: root-review","state":"pending"}]}"#,
        ));
        for _target in ["shared", "review"] {
            outcomes.push(successful_result(""));
            push_local_identity(&mut outcomes, branch);
            outcomes.push(successful_result(""));
            outcomes.push(successful_result(""));
            push_local_identity(&mut outcomes, branch);
            outcomes.push(successful_result(""));
        }
        outcomes.push(successful_result(""));
        push_local_identity(&mut outcomes, branch);
        outcomes.push(successful_result(
                r#"{"statuses":[{"context":"signoff: root-shared","state":"success"},{"context":"signoff: root-review","state":"success"}]}"#,
        ));
        FakeRunner::with_outcomes(outcomes)
    }

    fn successful_outcome(stdout: &str) -> CommandOutcome {
        CommandOutcome {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }
}

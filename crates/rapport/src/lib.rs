#[allow(
    dead_code,
    reason = "later phases still consume legacy Build proof views during the ledger rewrite"
)]
mod build;
#[allow(
    dead_code,
    reason = "the Phase 1 catalog still contains compatibility helpers until Init is rebuilt"
)]
mod builtin_rules;
mod cli;
mod context;
mod doctor;
mod github;
mod init;
mod paths;
mod policy_context;
mod prime;
#[allow(
    dead_code,
    reason = "later phases still consume a narrow read-only slice of the replaced Context model"
)]
mod project_context;
mod repository_files;
#[allow(
    dead_code,
    reason = "Phase 6 will reconnect Review helpers to the new Work Task ledger"
)]
mod review;
#[allow(
    dead_code,
    reason = "Phase 6 will replace the legacy Work Rules projection with policy snapshots"
)]
mod rules;
#[allow(
    dead_code,
    reason = "later phase consumers still use the legacy Rules projection during the rewrite"
)]
mod ruleset;
mod runner;
mod shared_ruleset;
mod signoff_contract;
mod snapshot;
mod state;
mod telemetry;
mod view;
#[allow(
    dead_code,
    reason = "later phases still consume legacy Work views until their Task migrations"
)]
mod work;
mod work_ledger;

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
use cli::{Cli, Command};
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
        Command::Github(github_args) => github::run(github_args, context),
        Command::Init => init::run(argv, context),
        Command::Rules(rules_args) => ruleset::run(&rules_args.command, argv, context),
        Command::Ruleset(ruleset_args) => shared_ruleset::run(ruleset_args, context),
        Command::Work(work_args) => work_ledger::run(work_args, context),
        Command::Develop(develop_args) => work_ledger::run_develop(develop_args, context),
        Command::Context(context_args) => policy_context::run(context_args, context),
        Command::Build(build_args) => work_ledger::run_build(build_args, context),
        Command::Review(review_args) => work_ledger::run_review(review_args, context),
        Command::Integrate(integrate_args) => work_ledger::run_integrate(integrate_args, context),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rapport_files::{InMemoryFileSystem, Utf8Path};
    use std::collections::VecDeque;
    use std::io;
    use std::sync::Mutex;

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
        outcomes: Mutex<VecDeque<io::Result<CommandOutcome>>>,
        calls: Mutex<Vec<(CommandSpec, Utf8PathBuf)>>,
    }

    impl FakeRunner {
        fn with_outcomes(outcomes: impl IntoIterator<Item = io::Result<CommandOutcome>>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
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
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            spec: &CommandSpec,
            cwd: &rapport_files::Utf8Path,
        ) -> io::Result<CommandOutcome> {
            self.calls
                .lock()
                .unwrap()
                .push((spec.clone(), cwd.to_path_buf()));
            self.outcomes.lock().unwrap().pop_front().unwrap()
        }
    }

    #[test]
    fn no_args_renders_root_help() {
        let (code, out, err) = run_with(&[]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("repository rapport for human-directed agent work"));
        assert!(out.contains("prime -> doctor -> work -> develop -> build -> review -> integrate"));
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
        assert!(out.contains("prime -> doctor -> work -> develop -> build -> review -> integrate"));
        assert_eq!(err, "");
    }

    #[test]
    fn version_flag_renders_package_version() {
        let (code, out, err) = run_with(&["--version"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(out, format!("rapport {}\n", env!("CARGO_PKG_VERSION")));
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
        assert!(out.contains("rapport develop task next"));
        assert!(out.contains("rapport work checkpoint start"));
        assert!(out.contains("rapport doctor"));
        assert!(out.contains("rapport build"));
        assert!(out.contains("rapport integrate"));
        assert!(out.contains("rapport integrate complete"));
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
    fn github_help_exposes_confirmed_setup() {
        let (code, out, err) = run_with(&["github", "setup", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("--confirm"));
        assert_eq!(err, "");
    }

    #[test]
    fn doctor_reports_github_origin_success() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::with_outcomes([
            successful_result("git@github.com:hedge-ops/rapport.git\n"),
            successful_result("authenticated\n"),
            successful_result(
                r#"{"nameWithOwner":"hedge-ops/rapport","defaultBranchRef":{"name":"main"},"squashMergeAllowed":true,"deleteBranchOnMerge":true,"viewerPermission":"ADMIN"}"#,
            ),
            successful_result(
                r#"[{"type":"pull_request"},{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Rapport Build"}],"strict_required_status_checks_policy":false}}]"#,
            ),
        ]);

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("git repository"));
        assert!(out.contains("origin remote"));
        assert!(out.contains("GitHub origin"));
        assert!(out.contains("GitHub integration"));
        assert!(out.contains("rapport integrate"));
        assert_eq!(err, "");
        assert_eq!(runner.calls().len(), 4);
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

    /// Phase 1 manages catalog and repository Rulesets through the public CLI grammar.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the Phase 1 CLI acceptance test preserves one complete sequential lifecycle"
    )]
    fn shared_ruleset_cli_should_complete_the_phase_one_lifecycle() {
        let mut fs = InMemoryFileSystem::default();

        let (catalog_list_code, catalog_list, catalog_list_error) =
            run_with_fs(&["ruleset", "catalog", "list"], &mut fs);
        assert_eq!(catalog_list_code, ExitCode::SUCCESS);
        assert!(
            catalog_list.contains("`RUST_CRATE`"),
            "expecting catalog list to include the Rust aggregate"
        );
        assert!(
            catalog_list_error.is_empty(),
            "expecting catalog list not to emit an error"
        );

        assert_eq!(
            run_with_fs(&["ruleset", "catalog", "install", "RUST_CRATE"], &mut fs).0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(&["ruleset", "catalog", "update", "RUST_CRATE"], &mut fs).0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "catalog",
                    "show",
                    "RUST_CRATE",
                    "--rule",
                    "RUST_CODING_001"
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );

        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "init",
                    "CODE",
                    "--purpose",
                    "Shared coding expectations."
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "rule",
                    "add",
                    "CODE",
                    "--id",
                    "CODE_001",
                    "--text",
                    "Prefer explicit names.",
                    "--rationale",
                    "Names retain intent.",
                    "--avoid-example",
                    "let x = value;",
                    "--avoid-language",
                    "rust",
                    "--prefer-example",
                    "let person_count = value;",
                    "--prefer-language",
                    "rust",
                    "--reference",
                    "[Naming](https://example.com/naming)"
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "rule",
                    "update",
                    "CODE",
                    "--rule",
                    "CODE_001",
                    "--text",
                    "Use explicit names.",
                    "--clear-reference"
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "purpose",
                    "set",
                    "CODE",
                    "--purpose",
                    "Repository coding expectations."
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &[
                    "ruleset",
                    "init",
                    "APP",
                    "--purpose",
                    "Application expectations."
                ],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &["ruleset", "compose", "add", "APP", "--ruleset", "CODE"],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );

        let (_, composition, _) = run_with_fs(&["ruleset", "compose", "list", "APP"], &mut fs);
        let (_, shown_rule, _) =
            run_with_fs(&["ruleset", "show", "APP", "--rule", "CODE_001"], &mut fs);
        let (_, listed, _) = run_with_fs(&["ruleset", "list"], &mut fs);
        assert!(
            composition.contains("`CODE`"),
            "expecting composition status to show the direct Ruleset"
        );
        assert!(
            shown_rule.contains("Use explicit names."),
            "expecting show to resolve a composed Rule"
        );
        assert!(
            listed.contains("Repository coding expectations."),
            "expecting list to show the updated Ruleset purpose"
        );

        assert_eq!(
            run_with_fs(
                &["ruleset", "compose", "remove", "APP", "--ruleset", "CODE"],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(
                &["ruleset", "rule", "remove", "CODE", "--rule", "CODE_001"],
                &mut fs
            )
            .0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(&["ruleset", "remove", "CODE"], &mut fs).0,
            ExitCode::SUCCESS
        );
        assert_eq!(
            run_with_fs(&["ruleset", "remove", "APP"], &mut fs).0,
            ExitCode::SUCCESS
        );
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
        assert!(out.contains("start"));
        assert!(out.contains("status"));
        assert!(out.contains("cancel"));
        assert!(out.contains("complete"));
        assert_eq!(err, "");
    }

    #[test]
    fn integrate_requires_active_work() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::successful("must not run");

        let (code, out, err) = run_with_runner(&["integrate", "start"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("no active Work exists"));
        assert!(runner.calls().is_empty());
        assert!(!fs.is_file("/repo/.rapport/events.jsonl"));
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
    #[ignore = "legacy multi-requirement Review protocol replaced by Phase 6"]
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
avoid = { language = "rust", text = "avoid" }
prefer = { language = "rust", text = "prefer" }
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
avoid = {{ language = "rust", text = "avoid" }}
prefer = {{ language = "rust", text = "prefer" }}
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

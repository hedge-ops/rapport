//! Rapport repository workflow library.
//!
//! This crate root exposes the intentional embedding API and delegates CLI,
//! policy, work-ledger, filesystem, and presentation behavior to owned modules.

mod cli;
mod context;
mod doctor;
mod github;
mod init;
mod paths;
mod policy_context;
mod prime;
mod repository_files;
mod runner;
mod shared_ruleset;
mod view;
mod work_ledger;

pub use context::{Clock, CommandContext, SystemClock, find_repo_root};
pub use paths::RapportPaths;
pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};
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
    match Cli::try_parse_from(std::iter::once(String::from("rapport")).chain(arguments)) {
        Ok(cli) => {
            let mut context = CommandContext::new(cwd, fs, clock, runner, out, err);
            execute_command(&cli, &mut context)
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

fn execute_command<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match &cli.command {
        Command::Prime => prime::run(context),
        Command::Doctor => doctor::run(context),
        Command::Github(github_args) => github::run(github_args, context),
        Command::Init => init::run(context),
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
#[expect(
    clippy::unwrap_used,
    reason = "CLI acceptance tests unwrap deterministic in-memory files and fake-runner queues"
)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rapport_files::InMemoryFileSystem;
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
    fn prime_renders_workflow() {
        let (code, out, err) = run_with(&["prime"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("rapport prime"));
        assert!(out.contains("planning, coding, testing, building, reviewing"));
        assert!(out.contains("rapport work start"));
        assert!(out.contains("rapport context show"));
        assert!(out.contains("rapport work task next"));
        assert!(out.contains("rapport work checkpoint start"));
        assert!(out.contains("rapport doctor"));
        assert!(out.contains("rapport build"));
        assert!(out.contains("rapport integrate"));
        assert!(out.contains("rapport integrate complete"));
        assert_eq!(err, "");
    }

    #[test]
    fn doctor_help_exists() {
        let (code, out, err) = run_with(&["doctor", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Check repository prerequisites"));
        assert_eq!(err, "");
    }

    #[test]
    fn github_help_documents_applying_setup_and_dry_run() {
        let (code, out, err) = run_with(&["github", "setup", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Apply Rapport's target-branch integration ruleset"));
        assert!(out.contains("--dry-run"));
        assert!(!out.contains("--confirm"));
        assert_eq!(err, "");
    }

    #[test]
    fn github_setup_applies_by_default_and_accepts_legacy_confirm() {
        for arguments in [
            &["github", "setup"][..],
            &["github", "setup", "--confirm"][..],
        ] {
            let mut fs = InMemoryFileSystem::default();
            let runner = github_setup_runner();

            let (code, out, err) = run_with_runner(arguments, &mut fs, &runner);

            assert_eq!(code, ExitCode::SUCCESS);
            assert!(out.contains("`applied` — true"));
            assert_eq!(err, "");
            let calls = runner.calls();
            assert_eq!(calls.len(), 5);
            assert_eq!(calls[3].0.args[0..3], ["api", "--method", "POST"]);
            assert_eq!(calls[4].0.args[0..2], ["repo", "edit"]);
        }
    }

    #[test]
    fn github_setup_dry_run_shows_changes_without_mutating_github() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::with_outcomes([
            successful_result("authenticated\n"),
            successful_result(github_repository_identity()),
            successful_result("[]"),
        ]);

        let (code, out, err) = run_with_runner(&["github", "setup", "--dry-run"], &mut fs, &runner);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("`ruleset` — Rapport Integration (main) (create)"));
        assert!(out.contains("`pull requests required` — true"));
        assert!(out.contains("`required status` — Rapport Build"));
        assert!(out.contains("`squash merge` — enabled"));
        assert!(out.contains("`delete merged branches` — enabled"));
        assert!(out.contains("`applied` — false"));
        assert_eq!(err, "");
        assert_eq!(runner.calls().len(), 3);
    }

    fn github_setup_runner() -> FakeRunner {
        FakeRunner::with_outcomes([
            successful_result("authenticated\n"),
            successful_result(github_repository_identity()),
            successful_result("[]"),
            successful_result("{}"),
            successful_result(""),
        ])
    }

    fn github_repository_identity() -> &'static str {
        r#"{"nameWithOwner":"hedge-ops/rapport","defaultBranchRef":{"name":"main"},"squashMergeAllowed":true,"deleteBranchOnMerge":true,"viewerPermission":"ADMIN"}"#
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
    }

    #[test]
    fn doctor_recommends_applying_github_setup_when_configuration_is_missing() {
        let mut fs = InMemoryFileSystem::default();
        let runner = FakeRunner::with_outcomes([
            successful_result("git@github.com:hedge-ops/rapport.git\n"),
            successful_result("authenticated\n"),
            successful_result(github_repository_identity()),
            successful_result("[]"),
        ]);

        let (code, out, err) = run_with_runner(&["doctor"], &mut fs, &runner);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("run rapport github setup, then rapport doctor"));
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
        assert!(signoff.contains("workflow_call:"));
        assert!(signoff.contains("context=${IDENTITY}"));
        assert!(signoff.contains("context=${AGGREGATE}"));
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
    }

    type FakeOutcome = io::Result<CommandOutcome>;

    #[expect(
        clippy::unnecessary_wraps,
        reason = "test helper builds io::Result queues consumed by the fake runner"
    )]
    fn successful_result(stdout: &str) -> FakeOutcome {
        Ok(successful_outcome(stdout))
    }
    fn successful_outcome(stdout: &str) -> CommandOutcome {
        CommandOutcome {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }
}

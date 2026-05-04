mod project;
mod runner;
mod view;

pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};

use camino::Utf8Path;
use nonempty::{NonEmpty, nonempty};
use project::{CargoProjectMatcher, discover_project};
use rapport_cli::files::{FileSystem, RealFileSystem};
use rapport_cli::{
    HelpTarget, Invocation, ParseError, Parser as _, RepositoryPath, parse_validated,
};
use std::fmt::Display;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Instant;
use strum::IntoEnumIterator;
use view::{Outcome, RunHint, ViewBuilder};

const USAGE: &str = "usage: rapport <fix|lint|build|test|validate|audit> <path>";

const FMT: CommandSpec = CommandSpec {
    program: "cargo",
    args: &["fmt"],
};
const FMT_CHECK: CommandSpec = CommandSpec {
    program: "cargo",
    args: &["fmt", "--", "--check"],
};
const CLIPPY: CommandSpec = CommandSpec {
    program: "cargo",
    args: &["clippy", "--all-targets", "--", "-D", "warnings"],
};
const CHECK: CommandSpec = CommandSpec {
    program: "cargo",
    args: &["check"],
};
const TEST: CommandSpec = CommandSpec {
    program: "cargo",
    args: &["test"],
};
const BUILD_RELEASE: CommandSpec = CommandSpec {
    program: "cargo",
    args: &["build", "--release"],
};
const DOC: CommandSpec = CommandSpec {
    program: "cargo",
    args: &["doc", "--no-deps"],
};

#[derive(
    Debug, Clone, Copy, strum::Display, strum::EnumString, strum::EnumIter, strum::AsRefStr,
)]
#[strum(serialize_all = "lowercase")]
enum Verb {
    Fix,
    Lint,
    Build,
    Test,
    Validate,
    Audit,
}

impl Verb {
    fn about(self) -> &'static str {
        match self {
            Self::Fix => "Auto-fix issues (modifies code)",
            Self::Lint => "Check style and conventions (read-only)",
            Self::Build => "Verify the code compiles",
            Self::Test => "Run the test suite",
            Self::Validate => "Pre-commit check (lint + build + test)",
            Self::Audit => "Pre-release check (validate + release-mode compile + docs)",
        }
    }

    fn steps(self) -> &'static [CommandSpec] {
        match self {
            Self::Fix => &[FMT],
            Self::Lint => &[FMT_CHECK, CLIPPY],
            Self::Build => &[CHECK],
            Self::Test => &[TEST],
            Self::Validate => &[FMT_CHECK, CLIPPY, CHECK, TEST],
            Self::Audit => &[FMT_CHECK, CLIPPY, CHECK, TEST, BUILD_RELEASE, DOC],
        }
    }

    fn hints(self, outcome: Outcome, path: &Utf8Path) -> NonEmpty<RunHint> {
        let p = path.as_str();
        let cmd = |verb: &str| RunHint::new(format!("rapport {verb} {p}"));
        match (self, outcome) {
            (Self::Fix, Outcome::Pass) | (Self::Build, Outcome::Fail) => nonempty![cmd("lint")],
            (Self::Fix | Self::Lint, Outcome::Fail) => nonempty![cmd("fix")],
            (Self::Lint, Outcome::Pass) => nonempty![cmd("build")],
            (Self::Build, Outcome::Pass) | (Self::Test, Outcome::Fail) => nonempty![cmd("test")],
            (Self::Test, Outcome::Pass) | (Self::Audit, Outcome::Fail) => {
                nonempty![cmd("validate")]
            }
            (Self::Validate, Outcome::Pass) => nonempty![cmd("audit")],
            (Self::Validate, Outcome::Fail) => {
                nonempty![cmd("lint"), cmd("build"), cmd("test")]
            }
            (Self::Audit, Outcome::Pass) => nonempty![RunHint::new("git push")],
        }
    }
}

#[derive(Debug)]
enum Command {
    Fix { path: RepositoryPath },
    Lint { path: RepositoryPath },
    Build { path: RepositoryPath },
    Test { path: RepositoryPath },
    Validate { path: RepositoryPath },
    Audit { path: RepositoryPath },
}

impl Command {
    #[must_use]
    fn verb(&self) -> Verb {
        match self {
            Self::Fix { .. } => Verb::Fix,
            Self::Lint { .. } => Verb::Lint,
            Self::Build { .. } => Verb::Build,
            Self::Test { .. } => Verb::Test,
            Self::Validate { .. } => Verb::Validate,
            Self::Audit { .. } => Verb::Audit,
        }
    }

    #[must_use]
    fn path(&self) -> &RepositoryPath {
        match self {
            Self::Fix { path }
            | Self::Lint { path }
            | Self::Build { path }
            | Self::Test { path }
            | Self::Validate { path }
            | Self::Audit { path } => path,
        }
    }

    fn from_argv_with_file_system(
        verb: Verb,
        rest: &[String],
        fs: &impl FileSystem,
    ) -> Result<Self, ParseError> {
        let [p] = rest else {
            return Err(ParseError::MissingArg {
                verb: verb.to_string(),
                expected: "path",
            });
        };
        let path: RepositoryPath = parse_validated(verb.as_ref(), p, fs)?;
        let project =
            discover_project(path.as_path(), &CargoProjectMatcher, fs).map_err(|reason| {
                ParseError::InvalidArg {
                    verb: verb.as_ref().into(),
                    value: p.into(),
                    reason,
                }
            })?;
        let path = RepositoryPath::new(project);
        Ok(match verb {
            Verb::Fix => Self::Fix { path },
            Verb::Lint => Self::Lint { path },
            Verb::Build => Self::Build { path },
            Verb::Test => Self::Test { path },
            Verb::Validate => Self::Validate { path },
            Verb::Audit => Self::Audit { path },
        })
    }
}

impl rapport_cli::Parser for Command {
    type Verb = Verb;

    fn parse_verb(name: &str) -> Result<Verb, ParseError> {
        name.parse()
            .map_err(|_| ParseError::UnknownVerb(name.into()))
    }

    fn from_argv(verb: Verb, rest: &[String]) -> Result<Self, ParseError> {
        Self::from_argv_with_file_system(verb, rest, &RealFileSystem)
    }
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.verb().fmt(f)
    }
}

pub fn run<I, O, E>(argv: I, runner: &dyn CommandRunner, out: &mut O, err: &mut E) -> ExitCode
where
    I: IntoIterator<Item = String>,
    O: Write,
    E: Write,
{
    run_with_file_system(argv, runner, &RealFileSystem, out, err)
}

fn run_with_file_system<I, O, E>(
    argv: I,
    runner: &dyn CommandRunner,
    fs: &impl FileSystem,
    out: &mut O,
    err: &mut E,
) -> ExitCode
where
    I: IntoIterator<Item = String>,
    O: Write,
    E: Write,
{
    match parse_with_file_system(argv, fs) {
        Ok(Invocation::Run(command)) => run_command(&command, runner, out, err),
        Ok(Invocation::Help(target)) => {
            let _ = writeln!(out, "{}", render_help(&target));
            ExitCode::SUCCESS
        }
        Err(parse_err) => {
            let _ = writeln!(err, "{}", render_error(&parse_err));
            ExitCode::from(2)
        }
    }
}

fn parse_with_file_system<I>(
    argv: I,
    fs: &impl FileSystem,
) -> Result<Invocation<Command>, ParseError>
where
    I: IntoIterator<Item = String>,
{
    let argv: Vec<String> = argv.into_iter().collect();
    match argv.as_slice() {
        [] => Err(ParseError::NoVerb),
        [a] if is_help_flag(a) || a == "help" => Ok(Invocation::Help(HelpTarget::Top)),
        [first, verb_name] if first == "help" => {
            let verb = Command::parse_verb(verb_name)?;
            Ok(Invocation::Help(HelpTarget::Verb(verb)))
        }
        [name, rest @ ..] => {
            let verb = Command::parse_verb(name)?;
            if rest.iter().any(|a| is_help_flag(a)) {
                Ok(Invocation::Help(HelpTarget::Verb(verb)))
            } else {
                Command::from_argv_with_file_system(verb, rest, fs).map(Invocation::Run)
            }
        }
    }
}

fn is_help_flag(s: &str) -> bool {
    s == "-h" || s == "--help"
}

fn render_help(target: &HelpTarget<Verb>) -> String {
    match target {
        HelpTarget::Top => render_help_top(),
        HelpTarget::Verb(v) => render_help_verb(*v),
    }
}

fn render_help_top() -> String {
    ViewBuilder::new()
        .title("rapport — workspace command runner")
        .section("Usage", |b| {
            b.usage(["rapport <verb> <path>", "rapport help [<verb>]"])
        })
        .section("Verbs", |b| b.entries(Verb::iter().map(|v| (v, v.about()))))
        .next_actions(nonempty![RunHint::new("rapport help build")])
        .build()
}

fn render_help_verb(verb: Verb) -> String {
    ViewBuilder::new()
        .title(format!("rapport {verb} — {}", verb.about()))
        .section("Usage", |b| b.usage([format!("rapport {verb} <path>")]))
        .section("Args", |b| {
            b.entries([("<path>", "Repository directory to operate on")])
        })
        .next_actions(nonempty![RunHint::new(format!("rapport {verb} ."))])
        .build()
}

fn render_error(err: &ParseError) -> String {
    let vb = ViewBuilder::new();
    let (vb, hints) = match err {
        ParseError::NoVerb => (vb.paragraph(USAGE), nonempty![RunHint::new("rapport help")]),
        ParseError::UnknownVerb(v) => (
            vb.paragraph(format!("'{v}' is not a recognized verb."))
                .paragraph(USAGE),
            nonempty![RunHint::new("rapport help")],
        ),
        ParseError::MissingArg { verb, expected } => (
            vb.paragraph(format!("rapport {verb} requires a {expected} argument."))
                .paragraph(USAGE),
            nonempty![RunHint::new(format!("rapport help {verb}"))],
        ),
        ParseError::InvalidArg {
            verb,
            value,
            reason,
        } => (
            vb.paragraph(format!("You ran: rapport {verb} {value}"))
                .paragraph(format!("{value} {reason}.")),
            nonempty![RunHint::new(format!("rapport help {verb}"))],
        ),
    };
    vb.next_actions(hints).build()
}

fn render_pass(started: Instant, hints: NonEmpty<RunHint>) -> String {
    ViewBuilder::new()
        .status(Outcome::Pass, started.elapsed())
        .next_actions(hints)
        .build()
}

fn render_step_failure(
    outcome: &CommandOutcome,
    started: Instant,
    hints: NonEmpty<RunHint>,
) -> String {
    let combined = combined_output(outcome);
    let mut vb = ViewBuilder::new();
    if !combined.is_empty() {
        vb = vb.section("Output", |b| b.captured(combined));
    }
    vb.status(Outcome::Fail, started.elapsed())
        .next_actions(hints)
        .build()
}

fn render_invoke_failure(command: &Command, path: &RepositoryPath, err: &io::Error) -> String {
    ViewBuilder::new()
        .paragraph(format!("You ran: rapport {command} {path}"))
        .paragraph(format!("Failed to invoke cargo: {err}"))
        .next_actions(nonempty![RunHint::new("which cargo")])
        .build()
}

fn combined_output(outcome: &CommandOutcome) -> String {
    let stderr = outcome.stderr.trim();
    let stdout = outcome.stdout.trim();
    match (stderr.is_empty(), stdout.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stderr.to_owned(),
        (true, false) => stdout.to_owned(),
        (false, false) => format!("{stderr}\n\n{stdout}"),
    }
}

fn run_command<O, E>(
    command: &Command,
    runner: &dyn CommandRunner,
    out: &mut O,
    err: &mut E,
) -> ExitCode
where
    O: Write,
    E: Write,
{
    let path = command.path();
    let started = Instant::now();
    for spec in command.verb().steps() {
        let outcome = match runner.run(spec, path.as_path()) {
            Ok(o) => o,
            Err(io_err) => {
                let _ = writeln!(err, "{}", render_invoke_failure(command, path, &io_err));
                return ExitCode::from(2);
            }
        };
        if !outcome.success {
            let hints = command.verb().hints(Outcome::Fail, path.as_path());
            let _ = writeln!(err, "{}", render_step_failure(&outcome, started, hints));
            return ExitCode::from(1);
        }
    }
    let hints = command.verb().hints(Outcome::Pass, path.as_path());
    let _ = writeln!(out, "{}", render_pass(started, hints));
    ExitCode::SUCCESS
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};
    use rapport_cli::files::InMemoryFileSystem;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    const ROOT: &str = "/work/repo";
    const ROOT_GIT_MARKER: &str = "/work/repo/.git";
    const ROOT_MANIFEST: &str = "/work/repo/Cargo.toml";
    const ROOT_CHILD: &str = "/work/repo/src/deep";
    const CRATE_DIR: &str = "/work/repo/crates/app";
    const CRATE_CHILD: &str = "/work/repo/crates/app/src";
    const CRATE_MANIFEST: &str = "/work/repo/crates/app/Cargo.toml";
    const MISSING: &str = "/work/missing";
    const OUTSIDE_REPO: &str = "/work/outside";
    const OUTSIDE_MANIFEST: &str = "/work/outside/Cargo.toml";

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedCommand {
        program: String,
        args: Vec<String>,
        cwd: Utf8PathBuf,
    }

    #[derive(Debug)]
    struct FakeRunner {
        outcomes: RefCell<VecDeque<io::Result<CommandOutcome>>>,
        calls: RefCell<Vec<RecordedCommand>>,
    }

    impl FakeRunner {
        fn new(outcomes: Vec<io::Result<CommandOutcome>>) -> Self {
            Self {
                outcomes: RefCell::new(outcomes.into()),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn all_pass(count: usize) -> Self {
            let outcomes = (0..count).map(|_| Ok(pass())).collect();
            Self::new(outcomes)
        }

        fn calls(&self) -> Vec<RecordedCommand> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, spec: &CommandSpec, cwd: &Utf8Path) -> io::Result<CommandOutcome> {
            self.calls.borrow_mut().push(RecordedCommand {
                program: spec.program.to_owned(),
                args: spec.args.iter().map(|arg| (*arg).to_owned()).collect(),
                cwd: cwd.to_owned(),
            });
            self.outcomes
                .borrow_mut()
                .pop_front()
                .expect("fake runner should have an outcome for each command")
        }
    }

    fn pass() -> CommandOutcome {
        CommandOutcome {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    fn fail(stdout: &str, stderr: &str) -> CommandOutcome {
        CommandOutcome {
            success: false,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn cargo_project_fs() -> InMemoryFileSystem {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory(ROOT);
        fs.add_file(ROOT_GIT_MARKER);
        fs.add_file(ROOT_MANIFEST);
        fs
    }

    fn run_with(
        args: &[&str],
        runner: &dyn CommandRunner,
        fs: &impl FileSystem,
    ) -> (ExitCode, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_file_system(
            args.iter().map(|arg| (*arg).to_owned()),
            runner,
            fs,
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn build_runs_cargo_check_in_the_given_directory() {
        let fs = cargo_project_fs();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", ROOT], &runner, &fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains("status: pass"));
        assert!(out.contains(&format!("└ run rapport test {ROOT}")));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into()],
                cwd: Utf8PathBuf::from(ROOT),
            }]
        );
    }

    #[test]
    fn validate_runs_lint_build_test_pipeline() {
        let fs = cargo_project_fs();
        let runner = FakeRunner::all_pass(4);

        let (code, out, err) = run_with(&["validate", ROOT], &runner, &fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport audit {ROOT}")));
        assert_eq!(
            runner.calls(),
            vec![
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["fmt".into(), "--".into(), "--check".into()],
                    cwd: Utf8PathBuf::from(ROOT),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec![
                        "clippy".into(),
                        "--all-targets".into(),
                        "--".into(),
                        "-D".into(),
                        "warnings".into(),
                    ],
                    cwd: Utf8PathBuf::from(ROOT),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["check".into()],
                    cwd: Utf8PathBuf::from(ROOT),
                },
                RecordedCommand {
                    program: "cargo".into(),
                    args: vec!["test".into()],
                    cwd: Utf8PathBuf::from(ROOT),
                },
            ]
        );
    }

    #[test]
    fn child_directory_runs_nearest_parent_cargo_project() {
        let mut fs = cargo_project_fs();
        fs.add_directory(CRATE_DIR);
        fs.add_file(CRATE_MANIFEST);
        fs.add_directory(CRATE_CHILD);
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", CRATE_CHILD], &runner, &fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert!(out.contains(&format!("└ run rapport test {CRATE_DIR}")));
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into()],
                cwd: Utf8PathBuf::from(CRATE_DIR),
            }]
        );
    }

    #[test]
    fn git_root_is_used_when_it_is_the_only_cargo_project() {
        let mut fs = cargo_project_fs();
        fs.add_directory(ROOT_CHILD);
        let runner = FakeRunner::all_pass(1);

        let (code, _out, err) = run_with(&["build", ROOT_CHILD], &runner, &fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(err, "");
        assert_eq!(
            runner.calls(),
            vec![RecordedCommand {
                program: "cargo".into(),
                args: vec!["check".into()],
                cwd: Utf8PathBuf::from(ROOT),
            }]
        );
    }

    #[test]
    fn step_failure_stops_pipeline_and_reports_captured_output() {
        let fs = cargo_project_fs();
        let runner = FakeRunner::new(vec![
            Ok(pass()),
            Ok(fail("stdout details", "stderr details")),
        ]);

        let (code, out, err) = run_with(&["lint", ROOT], &runner, &fs);

        assert_eq!(code, ExitCode::from(1));
        assert_eq!(out, "");
        assert!(err.contains("## Output"));
        assert!(err.contains("stderr details"));
        assert!(err.contains("stdout details"));
        assert!(err.contains("status: FAIL"));
        assert!(err.contains(&format!("└ run rapport fix {ROOT}")));
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn invoke_failure_reports_recovery_hint() {
        let fs = cargo_project_fs();
        let runner = FakeRunner::new(vec![Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing cargo",
        ))]);

        let (code, out, err) = run_with(&["build", ROOT], &runner, &fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains(&format!("You ran: rapport build {ROOT}")));
        assert!(err.contains("Failed to invoke cargo: missing cargo"));
        assert!(err.contains("└ run which cargo"));
    }

    #[test]
    fn missing_path_errors_before_running_any_commands() {
        let fs = InMemoryFileSystem::default();
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", MISSING], &runner, &fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains(&format!("You ran: rapport build {MISSING}")));
        assert!(err.contains("does not exist or is not a directory"));
        assert!(err.contains("└ run rapport help build"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn path_outside_git_repository_errors_before_running_any_commands() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory(OUTSIDE_REPO);
        fs.add_file(OUTSIDE_MANIFEST);
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", OUTSIDE_REPO], &runner, &fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains(&format!("You ran: rapport build {OUTSIDE_REPO}")));
        assert!(err.contains("is not inside a git repository"));
        assert_eq!(runner.calls(), Vec::new());
    }

    #[test]
    fn git_repository_without_supported_project_errors_before_running_any_commands() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory(ROOT);
        fs.add_file(ROOT_GIT_MARKER);
        fs.add_directory(ROOT_CHILD);
        let runner = FakeRunner::all_pass(1);

        let (code, out, err) = run_with(&["build", ROOT_CHILD], &runner, &fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains(&format!("You ran: rapport build {ROOT_CHILD}")));
        assert!(err.contains(&format!(
            "has no supported project between it and git root {ROOT}"
        )));
        assert_eq!(runner.calls(), Vec::new());
    }
}

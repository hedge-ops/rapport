mod runner;
mod view;

pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};

use camino::Utf8Path;
use nonempty::{NonEmpty, nonempty};
use rapport_cli::{
    HelpTarget, Invocation, ParseError, Parser as _, RealFileSystem, RepositoryPath,
    parse_validated,
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
            (Self::Fix, Outcome::Pass) => nonempty![cmd("lint")],
            (Self::Fix, Outcome::Fail) => nonempty![cmd("fix")],
            (Self::Lint, Outcome::Pass) => nonempty![cmd("build")],
            (Self::Lint, Outcome::Fail) => nonempty![cmd("fix")],
            (Self::Build, Outcome::Pass) => nonempty![cmd("test")],
            (Self::Build, Outcome::Fail) => nonempty![cmd("lint")],
            (Self::Test, Outcome::Pass) => nonempty![cmd("validate")],
            (Self::Test, Outcome::Fail) => nonempty![cmd("test")],
            (Self::Validate, Outcome::Pass) => nonempty![cmd("audit")],
            (Self::Validate, Outcome::Fail) => {
                nonempty![cmd("lint"), cmd("build"), cmd("test")]
            }
            (Self::Audit, Outcome::Pass) => nonempty![RunHint::new("git push")],
            (Self::Audit, Outcome::Fail) => nonempty![cmd("validate")],
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
}

impl rapport_cli::Parser for Command {
    type Verb = Verb;

    fn parse_verb(name: &str) -> Result<Verb, ParseError> {
        name.parse()
            .map_err(|_| ParseError::UnknownVerb(name.into()))
    }

    fn from_argv(verb: Verb, rest: &[String]) -> Result<Self, ParseError> {
        let [p] = rest else {
            return Err(ParseError::MissingArg {
                verb: verb.to_string(),
                expected: "path",
            });
        };
        let path: RepositoryPath = parse_validated(verb.as_ref(), p, &RealFileSystem)?;
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
    match Command::parse(argv) {
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
        .section("Verbs", |b| {
            b.entries(Verb::iter().map(|v| (v, v.about())))
        })
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

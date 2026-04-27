mod runner;

pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};

use rapport_cli::{
    HelpTarget, Invocation, ParseError, Parser as _, RealFileSystem, RepositoryPath,
    parse_validated,
};
use rapport_prose::{Column, OutputBuilder, ReportTable};
use std::fmt::Display;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Instant;
use strum::IntoEnumIterator;

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
    let mut verbs = ReportTable::new(vec![
        Column::new("Verb", 10),
        Column::new("Description", 60),
    ]);
    for v in Verb::iter() {
        verbs.push_row(vec![v.as_ref().to_string(), v.about().to_string()]);
    }
    OutputBuilder::new()
        .h1("rapport — workspace command runner")
        .h2("Usage")
        .text("```")
        .text("rapport <verb> <path>")
        .text("rapport help [<verb>]")
        .text("```")
        .blank()
        .h2("Verbs")
        .text(verbs.render())
        .blank()
        .text("Run `rapport help <verb>` for verb-specific details.")
        .build()
}

fn render_help_verb(verb: Verb) -> String {
    OutputBuilder::new()
        .h1(format!("rapport {verb} — {}", verb.about()))
        .h2("Usage")
        .text("```")
        .text(format!("rapport {verb} <path>"))
        .text("```")
        .blank()
        .h2("Args")
        .text("- `<path>` — Repository directory to operate on")
        .build()
}

fn render_error(err: &ParseError) -> String {
    let b = OutputBuilder::new();
    match err {
        ParseError::NoVerb => b.text(USAGE),
        ParseError::UnknownVerb(v) => b
            .text(format!("'{v}' is not a recognized verb."))
            .text(USAGE),
        ParseError::MissingArg { verb, expected } => b
            .text(format!("rapport {verb} requires a {expected} argument."))
            .text(USAGE),
        ParseError::InvalidArg {
            verb,
            value,
            reason,
        } => b
            .text(format!("You ran: rapport {verb} {value}"))
            .text(format!("{value} {reason}.")),
    }
    .build()
}

fn render_pass(started: Instant) -> String {
    OutputBuilder::new()
        .field("status", "pass")
        .field("duration", format!("{:.2}s", started.elapsed().as_secs_f64()))
        .build()
}

fn render_step_failure(outcome: &CommandOutcome, started: Instant) -> String {
    let mut b = OutputBuilder::new();
    let has_stdout = !outcome.stdout.trim().is_empty();
    let has_stderr = !outcome.stderr.trim().is_empty();
    if has_stdout {
        b = b.text(&outcome.stdout);
    }
    if has_stderr {
        b = b.text(&outcome.stderr);
    }
    if has_stdout || has_stderr {
        b = b.blank();
    }
    b.field("status", "FAIL")
        .field("duration", format!("{:.2}s", started.elapsed().as_secs_f64()))
        .build()
}

fn render_invoke_failure(command: &Command, path: &RepositoryPath, err: &io::Error) -> String {
    OutputBuilder::new()
        .text(format!("You ran: rapport {command} {path}"))
        .text(format!("Failed to invoke cargo: {err}"))
        .build()
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
            let _ = writeln!(err, "{}", render_step_failure(&outcome, started));
            return ExitCode::from(1);
        }
    }
    let _ = writeln!(out, "{}", render_pass(started));
    ExitCode::SUCCESS
}

use rapport_cli::{
    HelpTarget, Invocation, ParseError, Parser as _, RealFileSystem, RepositoryPath,
    parse_validated,
};
use std::fmt::Display;
use std::process::ExitCode;
use std::time::Instant;
use strum::IntoEnumIterator;

const USAGE: &str = "usage: rapport <fix|lint|build|test|validate|audit> <path>";

const FMT: &[&str] = &["fmt"];
const FMT_CHECK: &[&str] = &["fmt", "--", "--check"];
const CLIPPY: &[&str] = &["clippy", "--all-targets", "--", "-D", "warnings"];
const CHECK: &[&str] = &["check"];
const TEST: &[&str] = &["test"];
const BUILD_RELEASE: &[&str] = &["build", "--release"];
const DOC: &[&str] = &["doc", "--no-deps"];

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

    fn steps(self) -> &'static [&'static [&'static str]] {
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

pub fn run<I>(argv: I) -> ExitCode
where
    I: IntoIterator<Item = String>,
{
    match Command::parse(argv) {
        Ok(Invocation::Run(command)) => run_command(&command),
        Ok(Invocation::Help(target)) => {
            print_help(&target);
            ExitCode::SUCCESS
        }
        Err(err) => report_error(&err),
    }
}

fn print_help(target: &HelpTarget<Verb>) {
    match target {
        HelpTarget::Top => print_help_top(),
        HelpTarget::Verb(v) => print_help_verb(*v),
    }
}

fn print_help_top() {
    println!("rapport — workspace command runner");
    println!();
    println!("USAGE:");
    println!("    rapport <verb> <path>");
    println!("    rapport help [<verb>]");
    println!();
    println!("VERBS:");
    for verb in Verb::iter() {
        println!("    {:<10} {}", verb.as_ref(), verb.about());
    }
    println!();
    println!("Run `rapport help <verb>` for verb-specific details.");
}

fn print_help_verb(verb: Verb) {
    println!("rapport {verb} — {}", verb.about());
    println!();
    println!("USAGE:");
    println!("    rapport {verb} <path>");
    println!();
    println!("ARGS:");
    println!("    <path>    Repository directory to operate on");
}

fn report_error(err: &ParseError) -> ExitCode {
    match err {
        ParseError::NoVerb => {
            eprintln!("{USAGE}");
        }
        ParseError::UnknownVerb(v) => {
            eprintln!("'{v}' is not a recognized verb.");
            eprintln!("{USAGE}");
        }
        ParseError::MissingArg { verb, expected } => {
            eprintln!("rapport {verb} requires a {expected} argument.");
            eprintln!("{USAGE}");
        }
        ParseError::InvalidArg {
            verb,
            value,
            reason,
        } => {
            eprintln!("You ran: rapport {verb} {value}");
            eprintln!("{value} {reason}.");
        }
    }
    ExitCode::from(2)
}

fn run_command(command: &Command) -> ExitCode {
    let path = command.path();
    let verb = command.verb();
    let started = Instant::now();
    for step in verb.steps() {
        let output = std::process::Command::new("cargo")
            .args(*step)
            .current_dir(path.as_path())
            .output();
        let output = match output {
            Ok(o) => o,
            Err(err) => {
                eprintln!("You ran: rapport {command} {path}");
                eprintln!("Failed to invoke cargo: {err}");
                return ExitCode::from(2);
            }
        };

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() {
                eprint!("{stdout}");
            }
            if !stderr.trim().is_empty() {
                eprint!("{stderr}");
            }
            eprintln!();
            eprintln!("status: FAIL");
            eprintln!("duration: {:.2}s", started.elapsed().as_secs_f64());
            return ExitCode::from(1);
        }
    }

    println!("status: pass");
    println!("duration: {:.2}s", started.elapsed().as_secs_f64());
    ExitCode::SUCCESS
}

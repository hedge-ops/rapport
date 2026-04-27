use rapport_cli::{Argument as _, ParseError, Parser as _, RepositoryPath};
use std::fmt::Display;
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "usage: rapport <fix|lint|build|test|validate|audit> <path>";

const FMT: &[&str] = &["fmt"];
const FMT_CHECK: &[&str] = &["fmt", "--", "--check"];
const CLIPPY: &[&str] = &["clippy", "--all-targets", "--", "-D", "warnings"];
const CHECK: &[&str] = &["check"];
const TEST: &[&str] = &["test"];
const BUILD_RELEASE: &[&str] = &["build", "--release"];
const DOC: &[&str] = &["doc", "--no-deps"];

#[derive(Debug)]
enum Command {
    Fix { path: RepositoryPath },
    Lint { path: RepositoryPath },
    Build { path: RepositoryPath },
    Test { path: RepositoryPath },
    Validate { path: RepositoryPath },
    Audit { path: RepositoryPath },
}

impl rapport_cli::Parser for Command {
    fn from_argv(verb: &str, rest: &[String]) -> Result<Self, ParseError> {
        let [p] = rest else {
            return Err(ParseError::MissingArg {
                verb: verb.into(),
                expected: "path",
            });
        };
        let path = RepositoryPath::parse(p).map_err(|reason| ParseError::InvalidArg {
            verb: verb.into(),
            value: p.clone(),
            reason,
        })?;
        Ok(match verb {
            "fix" => Self::Fix { path },
            "lint" => Self::Lint { path },
            "build" => Self::Build { path },
            "test" => Self::Test { path },
            "validate" => Self::Validate { path },
            "audit" => Self::Audit { path },
            _ => return Err(ParseError::UnknownVerb(verb.into())),
        })
    }
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Fix { .. } => "fix",
            Self::Lint { .. } => "lint",
            Self::Build { .. } => "build",
            Self::Test { .. } => "test",
            Self::Validate { .. } => "validate",
            Self::Audit { .. } => "audit",
        })
    }
}

impl Command {
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

    #[must_use]
    fn steps(&self) -> &'static [&'static [&'static str]] {
        match self {
            Self::Fix { .. } => &[FMT],
            Self::Lint { .. } => &[FMT_CHECK, CLIPPY],
            Self::Build { .. } => &[CHECK],
            Self::Test { .. } => &[TEST],
            Self::Validate { .. } => &[FMT_CHECK, CLIPPY, CHECK, TEST],
            Self::Audit { .. } => &[FMT_CHECK, CLIPPY, CHECK, TEST, BUILD_RELEASE, DOC],
        }
    }
}

fn main() -> ExitCode {
    let command = match Command::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(ParseError::NoVerb) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        Err(ParseError::UnknownVerb(v)) => {
            eprintln!("'{v}' is not a recognized verb.");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        Err(ParseError::MissingArg { verb, expected }) => {
            eprintln!("rapport {verb} requires a {expected} argument.");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        Err(ParseError::InvalidArg {
            verb,
            value,
            reason,
        }) => {
            eprintln!("You ran: rapport {verb} {value}");
            eprintln!("{value} {reason}.");
            return ExitCode::from(2);
        }
    };

    run_command(&command)
}

fn run_command(command: &Command) -> ExitCode {
    let path = command.path();
    let started = Instant::now();
    for step in command.steps() {
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

mod runner;
mod view;

pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};
pub use view::{Outcome, RunHint, View, ViewBuilder};

use clap::{Parser, error::ErrorKind};
use nonempty::nonempty;
use std::io::Write;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "rapport", about = "repository workflow cli")]
struct Cli {
    #[arg(
        value_name = "COMMAND",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    command: Vec<String>,
}

/// Run the current `rapport` binary entrypoint.
///
/// The builder-era lifecycle runner has been removed. The replacement workflow
/// CLI is intentionally left to the Clap foundation work so this cleanup does
/// not lock in a temporary command contract.
pub fn run<I, O, E>(argv: I, _runner: &dyn CommandRunner, out: &mut O, err: &mut E) -> ExitCode
where
    I: IntoIterator<Item = String>,
    O: Write,
    E: Write,
{
    let arguments: Vec<String> = argv.into_iter().collect();
    if arguments.is_empty() || arguments.iter().any(|arg| arg == "-h" || arg == "--help") {
        let _ = writeln!(out, "{}", render_help());
        ExitCode::SUCCESS
    } else if let Err(error) =
        Cli::try_parse_from(std::iter::once(String::from("rapport")).chain(arguments))
    {
        if error.kind() == ErrorKind::DisplayHelp {
            let _ = writeln!(out, "{}", render_help());
            ExitCode::SUCCESS
        } else {
            let _ = write!(err, "{error}");
            ExitCode::from(2)
        }
    } else {
        let _ = writeln!(err, "{}", render_pending_cli());
        ExitCode::from(2)
    }
}

fn render_help() -> String {
    ViewBuilder::new()
        .title("rapport - repository workflow cli")
        .section("Loop", |b| b.usage(["work -> build -> integrate"]))
        .section("Planned Commands", |b| {
            b.usage([
                "rapport work start",
                "rapport work status",
                "rapport work add path <path>",
                "rapport work rules list",
                "rapport work rules show <id>",
                "rapport build [path...]",
                "rapport integrate --summary \"...\" --message \"...\"",
            ])
        })
        .next_actions(nonempty![RunHint::new(
            "follow #51 for the Clap CLI foundation"
        )])
        .build()
}

fn render_pending_cli() -> String {
    ViewBuilder::new()
        .paragraph("The builder-era lifecycle runner has been removed.")
        .paragraph("The workflow CLI will be added by the Clap foundation work.")
        .next_actions(nonempty![RunHint::new("rapport --help")])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

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
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            args.iter().map(|arg| (*arg).to_string()),
            &NeverRunner,
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[test]
    fn no_args_renders_planned_workflow_help() {
        let (code, out, err) = run_with(&[]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("work -> build -> integrate"));
        assert!(out.contains("rapport work start"));
        assert_eq!(err, "");
    }

    #[test]
    fn help_flag_renders_planned_workflow_help() {
        let (code, out, err) = run_with(&["--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("rapport integrate"));
        assert_eq!(err, "");
    }

    #[test]
    fn old_lifecycle_verbs_are_not_supported() {
        let (code, out, err) = run_with(&["build", "."]);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("builder-era lifecycle runner has been removed"));
        assert!(err.contains("rapport --help"));
    }
}

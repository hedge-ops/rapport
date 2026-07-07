mod cli;
mod context;
mod paths;
mod runner;
mod state;
mod telemetry;
mod view;

pub use context::{Clock, CommandContext, SystemClock, find_repo_root};
pub use paths::RapportPaths;
pub use runner::{CommandOutcome, CommandRunner, CommandSpec, RealCommandRunner};
pub use state::{WORK_STATE_SCHEMA_VERSION, WorkState, WorkStateError, WorkStateStore};
pub use telemetry::{
    CommandEvent, CommandEventOutcome, EVENT_SCHEMA_VERSION, TelemetryError, TelemetryWriter,
};
pub use view::{Outcome, RunHint, View, ViewBuilder};

use clap::{CommandFactory, Parser, error::ErrorKind};
use cli::Cli;
use nonempty::nonempty;
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
            execute_foundation_command(&cli, arguments, &mut context)
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

fn execute_foundation_command<F, C, O, E>(
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
    let exit_code = 2;
    let command = cli.command_path();
    let pending_issue = cli.pending_issue();
    let _ = writeln!(
        context.err,
        "{}",
        render_pending_command(command, pending_issue)
    );
    let event = CommandEvent::new(
        context.clock.now_rfc3339(),
        argv,
        command,
        CommandEventOutcome::Failure,
        exit_code,
    );
    match TelemetryWriter::new(context.paths.clone()).append(context.fs, &event) {
        Ok(()) => ExitCode::from(exit_code),
        Err(error) => {
            let _ = writeln!(context.err, "{error}");
            ExitCode::FAILURE
        }
    }
}

fn render_pending_command(command: &str, pending_issue: &str) -> String {
    ViewBuilder::new()
        .paragraph(format!(
            "`rapport {command}` is defined, but its workflow behavior lands in {pending_issue}."
        ))
        .paragraph(
            "This foundation only establishes parsing, local paths, state plumbing, and telemetry.",
        )
        .next_actions(nonempty![RunHint::new("rapport --help")])
        .build()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;
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
        let mut out = Vec::new();
        let mut err = Vec::new();
        fs.add_directory("/repo/.git");
        let code = run_with_environment(
            args.iter().map(|arg| (*arg).to_string()),
            &NeverRunner,
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

    #[test]
    fn no_args_renders_root_help() {
        let (code, out, err) = run_with(&[]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("repository rapport for human-directed agent work"));
        assert!(out.contains("work -> build -> integrate"));
        assert!(out.contains("work"));
        assert_eq!(err, "");
    }

    #[test]
    fn help_flag_renders_root_help() {
        let (code, out, err) = run_with(&["--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Rapport keeps human-directed agent work grounded"));
        assert!(out.contains("work -> build -> integrate"));
        assert_eq!(err, "");
    }

    #[test]
    fn work_help_exists() {
        let (code, out, err) = run_with(&["work", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Manage active local work state"));
        assert!(out.contains("start"));
        assert!(out.contains("status"));
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
    fn integrate_help_exists() {
        let (code, out, err) = run_with(&["integrate", "--help"]);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("Git/GitHub integration"));
        assert!(out.contains("--summary"));
        assert_eq!(err, "");
    }

    #[test]
    fn valid_pending_command_writes_failure_event() {
        let mut fs = InMemoryFileSystem::default();
        let (code, out, err) = run_with_fs(&["build", "crates/rapport"], &mut fs);

        assert_eq!(code, ExitCode::from(2));
        assert_eq!(out, "");
        assert!(err.contains("workflow behavior lands in #56"));
        assert!(err.contains("rapport --help"));
        let events = fs.read_to_string("/repo/.rapport/events.jsonl").unwrap();
        let event: CommandEvent = serde_json::from_str(events.lines().next().unwrap()).unwrap();

        assert_eq!(event.timestamp, "2026-07-07T23:00:00Z");
        assert_eq!(
            event.argv,
            vec![String::from("build"), String::from("crates/rapport")]
        );
        assert_eq!(event.command, "build");
        assert_eq!(event.outcome, CommandEventOutcome::Failure);
        assert_eq!(event.exit_code, 2);
    }

    #[test]
    fn help_does_not_write_telemetry() {
        let mut fs = InMemoryFileSystem::default();
        let (code, _out, _err) = run_with_fs(&["work", "--help"], &mut fs);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(!fs.is_file("/repo/.rapport/events.jsonl"));
    }
}

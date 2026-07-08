use crate::cli::BuildArgs;
use crate::context::{Clock, CommandContext};
use crate::runner::{CommandOutcome, CommandSpec};
use crate::state::{WorkFact, WorkState, WorkStateError, WorkStateStore};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::error::Error;
use std::fmt;
use std::io;
use std::io::Write;
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;
const JUST_TARGET: &str = "ci";

pub fn run<F, C, O, E>(
    build_args: &BuildArgs,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = WorkStateStore::new(context.paths.clone());
    let result = match store.load(context.fs) {
        Ok(Some(state)) => {
            match select_build_paths(build_args, &state, &context.repo_root, &context.cwd) {
                Ok(paths) if paths.is_empty() => {
                    let _ = writeln!(context.err, "{}", render_no_build_paths());
                    CommandResult::failure()
                }
                Ok(paths) => run_validation(arguments.clone(), context, &store, state, &paths),
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_build_path_error(&error));
                    CommandResult::failure()
                }
            }
        }
        Ok(None) => {
            let _ = writeln!(context.err, "{}", render_missing_work());
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            CommandResult::failure()
        }
    };
    finish("build", arguments, context, result)
}

fn run_validation<F, C, O, E>(
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    mut state: WorkState,
    paths: &[String],
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if let Err(error) = record_event("build start", arguments, context, CommandResult::success()) {
        let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
        return CommandResult::failure();
    }

    let spec = validation_command();
    match context.runner.run(&spec, context.paths.repo_root()) {
        Ok(outcome) if outcome.success => {
            let now = context.clock.now_rfc3339();
            state.build = Some(build_fact("pass", &now, paths));
            state.updated_at = now;
            match store.save(context.fs, &state) {
                Ok(()) => {
                    let _ = writeln!(context.out, "{}", render_build_pass(paths, &outcome));
                    CommandResult::success()
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_state_error(&error));
                    CommandResult::failure()
                }
            }
        }
        Ok(outcome) => {
            let now = context.clock.now_rfc3339();
            state.build = Some(build_fact("fail", &now, paths));
            state.updated_at = now;
            match store.save(context.fs, &state) {
                Ok(()) => {
                    let _ = writeln!(context.err, "{}", render_build_fail(paths, &outcome));
                    CommandResult::failure()
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_state_error(&error));
                    CommandResult::failure()
                }
            }
        }
        Err(error) => {
            let now = context.clock.now_rfc3339();
            state.build = Some(build_fact("error", &now, paths));
            state.updated_at = now;
            match store.save(context.fs, &state) {
                Ok(()) => {
                    let _ = writeln!(context.err, "{}", render_command_error(paths, &error));
                    CommandResult::failure()
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_state_error(&error));
                    CommandResult::failure()
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BuildPathError {
    OutsideRepository { path: Utf8PathBuf },
    OutsideWork { path: Utf8PathBuf },
}

impl fmt::Display for BuildPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideRepository { path } => {
                write!(f, "`{path}` is outside the repository.")
            }
            Self::OutsideWork { path } => write!(f, "`{path}` is not part of the current work."),
        }
    }
}

impl Error for BuildPathError {}

#[derive(Debug, Clone, Copy)]
struct CommandResult {
    outcome: CommandEventOutcome,
    exit_code: u8,
}

impl CommandResult {
    fn success() -> Self {
        Self {
            outcome: CommandEventOutcome::Success,
            exit_code: SUCCESS,
        }
    }

    fn failure() -> Self {
        Self {
            outcome: CommandEventOutcome::Failure,
            exit_code: FAILURE,
        }
    }
}

fn select_build_paths(
    build_args: &BuildArgs,
    state: &WorkState,
    repo_root: &Utf8Path,
    cwd: &Utf8Path,
) -> Result<Vec<String>, BuildPathError> {
    if build_args.paths.is_empty() {
        return Ok(state.paths.clone());
    }

    let mut selected = Vec::new();
    for path in &build_args.paths {
        let normalized = normalize_requested_path(repo_root, cwd, path)?;
        if !state
            .paths
            .iter()
            .any(|work_path| work_path == normalized.as_str())
        {
            return Err(BuildPathError::OutsideWork { path: normalized });
        }
        selected.push(normalized.to_string());
    }
    Ok(selected)
}

fn normalize_requested_path(
    repo_root: &Utf8Path,
    cwd: &Utf8Path,
    path: &Utf8Path,
) -> Result<Utf8PathBuf, BuildPathError> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let relative_path =
        absolute_path
            .strip_prefix(repo_root)
            .map_err(|_| BuildPathError::OutsideRepository {
                path: absolute_path.clone(),
            })?;
    if relative_path.as_str().is_empty() {
        Ok(Utf8PathBuf::from("."))
    } else {
        Ok(relative_path.to_path_buf())
    }
}

fn validation_command() -> CommandSpec {
    CommandSpec::new("just", [JUST_TARGET])
}

fn build_fact(status: &str, timestamp: &str, paths: &[String]) -> WorkFact {
    WorkFact::new(status)
        .at(timestamp)
        .summary(format!("`just {JUST_TARGET}` for {}", paths.join(", ")))
}

fn finish<F, C, O, E>(
    command: &'static str,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
    result: CommandResult,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match record_event(command, arguments, context, result) {
        Ok(()) => ExitCode::from(result.exit_code),
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
            ExitCode::FAILURE
        }
    }
}

fn record_event<F, C, O, E>(
    command: &'static str,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
    result: CommandResult,
) -> Result<(), TelemetryError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let event = CommandEvent::new(
        context.clock.now_rfc3339(),
        arguments,
        command,
        result.outcome,
        result.exit_code,
    );
    TelemetryWriter::new(context.paths.clone()).append(context.fs, &event)
}

fn render_build_pass(paths: &[String], outcome: &CommandOutcome) -> String {
    let mut builder = ViewBuilder::new()
        .title("rapport build")
        .section("Validation", |b| {
            b.entries([
                ("status", String::from("pass")),
                ("command", format!("just {JUST_TARGET}")),
            ])
        })
        .section("Paths", |b| b.items(paths.to_vec()));
    let output = captured_output(outcome);
    if !output.is_empty() {
        builder = builder.section("Output", |b| b.captured(output));
    }
    builder
        .next_actions(nonempty![RunHint::new("rapport integrate")])
        .build()
}

fn render_build_fail(paths: &[String], outcome: &CommandOutcome) -> String {
    let mut builder = ViewBuilder::new()
        .title("rapport build")
        .section("Validation", |b| {
            b.entries([
                ("status", String::from("fail")),
                ("command", format!("just {JUST_TARGET}")),
            ])
        })
        .section("Paths", |b| b.items(paths.to_vec()));
    let output = captured_output(outcome);
    if !output.is_empty() {
        builder = builder.section("Output", |b| b.captured(output));
    }
    builder
        .next_actions(nonempty![RunHint::new(
            "fix validation, then run rapport build"
        )])
        .build()
}

fn captured_output(outcome: &CommandOutcome) -> String {
    let mut parts = Vec::new();
    if !outcome.stdout.trim().is_empty() {
        parts.push(format!("stdout:\n{}", outcome.stdout.trim()));
    }
    if !outcome.stderr.trim().is_empty() {
        parts.push(format!("stderr:\n{}", outcome.stderr.trim()));
    }
    parts.join("\n\n")
}

fn render_missing_work() -> String {
    ViewBuilder::new()
        .title("rapport build")
        .paragraph("No active work state found.")
        .paragraph("Start work before running build validation.")
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

fn render_no_build_paths() -> String {
    ViewBuilder::new()
        .title("rapport build")
        .paragraph("Active work has no paths to validate.")
        .next_actions(nonempty![RunHint::new("rapport work add path <path>")])
        .build()
}

fn render_build_path_error(error: &BuildPathError) -> String {
    let next = match error {
        BuildPathError::OutsideRepository { .. } => "rapport work status".to_string(),
        BuildPathError::OutsideWork { path } => format!("rapport work add path {path}"),
    };
    ViewBuilder::new()
        .title("rapport build")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new(next)])
        .build()
}

fn render_command_error(paths: &[String], error: &io::Error) -> String {
    ViewBuilder::new()
        .title("rapport build")
        .paragraph(format!("Could not run `just {JUST_TARGET}`."))
        .paragraph(error)
        .section("Paths", |b| b.items(paths.to_vec()))
        .next_actions(nonempty![RunHint::new(
            "install Just, then run rapport build"
        )])
        .build()
}

fn render_state_error(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport build")
        .paragraph("Could not update active work state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_telemetry_error(error: &TelemetryError) -> String {
    ViewBuilder::new()
        .title("rapport telemetry")
        .paragraph("Command completed, but telemetry could not be written.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

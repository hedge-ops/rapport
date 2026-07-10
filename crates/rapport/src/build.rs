use crate::cli::BuildArgs;
use crate::context::{Clock, CommandContext};
use crate::project_context::resolved_rules_for_paths;
use crate::project_context::{SignoffRequirement, required_signoff_requirements_for_paths};
use crate::runner::{CommandOutcome, CommandSpec};
use crate::signoff_contract::SignoffKind;
use crate::snapshot::{self, SnapshotError};
use crate::state::{
    BuildState, OperationStatus, WorkFact, WorkState, WorkStateError, WorkStateStore,
};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::error::Error;
use std::fmt;
use std::io::Write;
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;

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
        Ok(Some(mut state)) => {
            match select_build_paths(build_args, &state, &context.repo_root, &context.cwd) {
                Ok(paths) if paths.is_empty() => {
                    let _ = writeln!(context.err, "{}", render_no_build_paths());
                    CommandResult::failure()
                }
                Ok(paths) => run_builds(context, &store, &mut state, &paths),
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_build_error(&error));
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

fn run_builds<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    selected_paths: &[String],
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let all_requirements = match required_signoff_requirements_for_paths(
        context.fs,
        &context.repo_root,
        selected_paths,
    ) {
        Ok(requirements) => requirements,
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_build_error(&BuildError::from(error))
            );
            return CommandResult::failure();
        }
    };
    let has_reviews = all_requirements
        .iter()
        .any(|requirement| requirement.request.kind() == SignoffKind::Review);
    let requirements = all_requirements
        .into_iter()
        .filter(|requirement| requirement.request.kind() == SignoffKind::Build)
        .collect::<Vec<_>>();
    if requirements.is_empty() {
        let _ = writeln!(context.err, "{}", render_no_builds());
        return CommandResult::failure();
    }

    let mut lines = Vec::new();
    let mut all_passed = true;
    for requirement in &requirements {
        match execute_requirement(context, state, requirement, None) {
            Ok(execution) => {
                all_passed &= execution.status == OperationStatus::Pass;
                lines.push(render_execution(requirement, &execution));
            }
            Err(error) => {
                all_passed = false;
                lines.push(format!(
                    "`{}` error: {error}",
                    requirement.request.qualified_target()
                ));
                break;
            }
        }
        state.updated_at = context.clock.now_rfc3339();
        if let Err(error) = store.save(context.fs, state) {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            return CommandResult::failure();
        }
    }

    let now = context.clock.now_rfc3339();
    let status = if all_passed { "pass" } else { "fail" };
    state.build = Some(
        WorkFact::new(status)
            .at(&now)
            .summary(format!("{} typed build operation(s)", requirements.len())),
    );
    state.updated_at = now;
    if let Err(error) = store.save(context.fs, state) {
        let _ = writeln!(context.err, "{}", render_state_error(&error));
        return CommandResult::failure();
    }

    let rendered = render_build_results(status, lines, has_reviews);
    if all_passed {
        let _ = writeln!(context.out, "{rendered}");
        CommandResult::success()
    } else {
        let _ = writeln!(context.err, "{rendered}");
        CommandResult::failure()
    }
}

pub(crate) fn evaluate_requirement<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
    requirement: &SignoffRequirement,
    explicit_base_sha: &str,
) -> Result<BuildExecution, BuildError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let snapshot = capture_requirement(context, requirement, Some(explicit_base_sha))?;
    let id = requirement.request.qualified_target();
    if state.builds.get(id).is_some_and(|build| {
        build.status == OperationStatus::Pass && build.input_checksum == snapshot.input_checksum
    }) {
        if let Some(build) = state.builds.get_mut(id) {
            build.base_sha = Some(snapshot.base_sha);
            build.head_sha = Some(snapshot.head_sha);
        }
        return Ok(BuildExecution {
            status: OperationStatus::Pass,
            reused: true,
            outcome: None,
        });
    }
    execute_with_snapshot(context, state, requirement, snapshot)
}

pub(crate) fn refresh<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
) -> Result<(), BuildError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let requirements = build_requirements(context.fs, &context.repo_root, &state.paths)?;
    for requirement in requirements {
        let id = requirement.request.qualified_target().to_string();
        let Some(existing) = state.builds.get(&id) else {
            continue;
        };
        let base = existing.base_sha.clone();
        let snapshot = capture_requirement(context, &requirement, base.as_deref())?;
        if let Some(build) = state.builds.get_mut(&id) {
            if build.result_status.is_none() && build.status != OperationStatus::Stale {
                build.result_status = Some(build.status);
            }
            if build.input_checksum == snapshot.input_checksum {
                build.base_sha = Some(snapshot.base_sha);
                build.head_sha = Some(snapshot.head_sha);
                if let Some(result_status) = build.result_status {
                    build.status = result_status;
                }
            } else {
                build.status = OperationStatus::Stale;
            }
        }
    }
    Ok(())
}

pub(crate) fn status_lines<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
) -> Result<Vec<String>, BuildError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    refresh(context, state)?;
    let requirements = build_requirements(context.fs, &context.repo_root, &state.paths)?;
    Ok(requirements
        .into_iter()
        .map(|requirement| {
            let id = requirement.request.qualified_target();
            state.builds.get(id).map_or_else(
                || {
                    format!(
                        "`{id}` missing; command `just {}`; context `{}`; paths [{}]",
                        requirement.request.target(),
                        requirement.request.declaring_context(),
                        requirement.paths.join(", ")
                    )
                },
                |build| {
                    let head = build.head_sha.as_deref().unwrap_or("uncommitted");
                    let status = if build.status == OperationStatus::Stale {
                        String::from("stale")
                    } else {
                        format!("current {}", build.status)
                    };
                    format!(
                        "`{id}` {status}; head `{head}`; input `{}`; command `{}`; paths [{}]",
                        build.input_checksum,
                        build.command,
                        build.paths.join(", ")
                    )
                },
            )
        })
        .collect())
}

pub(crate) fn completion_problems<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
) -> Result<Vec<String>, BuildError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    refresh(context, state)?;
    let requirements = build_requirements(context.fs, &context.repo_root, &state.paths)?;
    Ok(requirements
        .into_iter()
        .filter_map(|requirement| {
            let id = requirement.request.qualified_target();
            match state.builds.get(id) {
                None => Some(format!("required build `{id}` is missing")),
                Some(build) if build.status != OperationStatus::Pass => {
                    Some(format!("required build `{id}` is {}", build.status))
                }
                Some(_) => None,
            }
        })
        .collect())
}

fn execute_requirement<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
    requirement: &SignoffRequirement,
    explicit_base_sha: Option<&str>,
) -> Result<BuildExecution, BuildError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let snapshot = capture_requirement(context, requirement, explicit_base_sha)?;
    execute_with_snapshot(context, state, requirement, snapshot)
}

fn execute_with_snapshot<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
    requirement: &SignoffRequirement,
    operation_snapshot: snapshot::OperationSnapshot,
) -> Result<BuildExecution, BuildError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let target = requirement.request.target();
    let command = format!("just {target}");
    let outcome = context
        .runner
        .run(
            &CommandSpec::new("just", [target]),
            requirement.request.context_directory(),
        )
        .map_err(BuildError::Invoke)?;
    let status = if outcome.success {
        OperationStatus::Pass
    } else {
        OperationStatus::Fail
    };
    let description = captured_output(&outcome);
    state.builds.insert(
        requirement.request.qualified_target().to_string(),
        BuildState {
            status,
            result_status: Some(status),
            target: target.to_string(),
            declaring_context: requirement.request.declaring_context().to_string(),
            paths: requirement.paths.clone(),
            at: context.clock.now_rfc3339(),
            base_sha: Some(operation_snapshot.base_sha),
            head_sha: Some(operation_snapshot.head_sha),
            content_checksum: operation_snapshot.content_checksum,
            instructions_checksum: operation_snapshot.instructions_checksum,
            input_checksum: operation_snapshot.input_checksum,
            command,
            description,
        },
    );
    Ok(BuildExecution {
        status,
        reused: false,
        outcome: Some(outcome),
    })
}

fn capture_requirement<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    requirement: &SignoffRequirement,
    explicit_base_sha: Option<&str>,
) -> Result<snapshot::OperationSnapshot, BuildError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let instruction = format!(
        "build {} by running just {} in {} for [{}]",
        requirement.request.qualified_target(),
        requirement.request.target(),
        requirement.request.declaring_context(),
        requirement.paths.join(", ")
    );
    let instructions_checksum = snapshot::checksum([instruction.as_str()]);
    let rules = resolved_rules_for_paths(context.fs, &context.repo_root, &requirement.paths)?;
    let canonical_rules = serde_json::to_string(&rules).map_err(BuildError::EncodeRules)?;
    let rules_checksum = snapshot::checksum([canonical_rules.as_str()]);
    snapshot::capture(
        context.fs,
        context.runner,
        &context.repo_root,
        requirement.request.qualified_target(),
        &requirement.paths,
        explicit_base_sha,
        &rules_checksum,
        &instructions_checksum,
    )
    .map_err(BuildError::Snapshot)
}

fn build_requirements(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    paths: &[String],
) -> Result<Vec<SignoffRequirement>, BuildError> {
    Ok(
        required_signoff_requirements_for_paths(fs, repo_root, paths)?
            .into_iter()
            .filter(|requirement| requirement.request.kind() == SignoffKind::Build)
            .collect(),
    )
}

#[derive(Debug)]
pub(crate) struct BuildExecution {
    pub(crate) status: OperationStatus,
    pub(crate) reused: bool,
    outcome: Option<CommandOutcome>,
}

fn render_execution(requirement: &SignoffRequirement, execution: &BuildExecution) -> String {
    let reuse = if execution.reused { " (reused)" } else { "" };
    let mut line = format!(
        "`{}` {}{reuse}: `just {}` in `{}` for [{}]",
        requirement.request.qualified_target(),
        execution.status,
        requirement.request.target(),
        requirement.request.declaring_context(),
        requirement.paths.join(", ")
    );
    if let Some(outcome) = &execution.outcome {
        let output = captured_output(outcome);
        if !output.is_empty() {
            line.push('\n');
            line.push_str(&output);
        }
    }
    line
}

fn captured_output(outcome: &CommandOutcome) -> String {
    let mut parts = Vec::new();
    if !outcome.stdout.trim().is_empty() {
        parts.push(format!("stdout: {} bytes", outcome.stdout.len()));
    }
    if !outcome.stderr.trim().is_empty() {
        parts.push(format!("stderr: {} bytes", outcome.stderr.len()));
    }
    parts.join("\n\n")
}

fn select_build_paths(
    build_args: &BuildArgs,
    state: &WorkState,
    repo_root: &Utf8Path,
    cwd: &Utf8Path,
) -> Result<Vec<String>, BuildError> {
    if build_args.paths.is_empty() {
        return Ok(state.paths.clone());
    }
    let mut selected = Vec::new();
    for path in &build_args.paths {
        let normalized = normalize_requested_path(repo_root, cwd, path)?;
        if !state
            .paths
            .iter()
            .any(|work_path| path_is_within(normalized.as_str(), work_path))
        {
            return Err(BuildError::OutsideWork(normalized));
        }
        selected.push(normalized.to_string());
    }
    Ok(selected)
}

fn normalize_requested_path(
    repo_root: &Utf8Path,
    cwd: &Utf8Path,
    path: &Utf8Path,
) -> Result<Utf8PathBuf, BuildError> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let relative_path = absolute_path
        .strip_prefix(repo_root)
        .map_err(|_| BuildError::OutsideRepository(absolute_path.clone()))?;
    let portable = relative_path.as_str().replace('\\', "/");
    let components = portable
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>();
    if components.contains(&"..") {
        return Err(BuildError::OutsideRepository(absolute_path));
    }
    if components.is_empty() {
        Ok(Utf8PathBuf::from("."))
    } else {
        Ok(Utf8PathBuf::from(components.join("/")))
    }
}

fn path_is_within(selected: &str, work_path: &str) -> bool {
    work_path == "."
        || selected == work_path
        || selected
            .strip_prefix(work_path)
            .is_some_and(|remaining| remaining.starts_with('/'))
}

pub(crate) enum BuildError {
    Context(crate::project_context::ProjectContextError),
    Snapshot(SnapshotError),
    Invoke(std::io::Error),
    EncodeRules(serde_json::Error),
    OutsideRepository(Utf8PathBuf),
    OutsideWork(Utf8PathBuf),
}

impl fmt::Debug for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Context(_) => "context",
            Self::Snapshot(_) => "snapshot",
            Self::Invoke(_) => "invoke",
            Self::EncodeRules(_) => "encode_rules",
            Self::OutsideRepository(_) => "outside_repository",
            Self::OutsideWork(_) => "outside_work",
        };
        f.debug_struct("BuildError").field("kind", &kind).finish()
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Context(source) => write!(
                f,
                "could not resolve build context (detail {} bytes)",
                source.to_string().len()
            ),
            Self::Snapshot(source) => write!(f, "could not checksum build inputs: {source}"),
            Self::Invoke(source) => {
                write!(f, "could not run build operation ({:?})", source.kind())
            }
            Self::EncodeRules(source) => write!(
                f,
                "could not checksum resolved build rules (line {}, column {})",
                source.line(),
                source.column()
            ),
            Self::OutsideRepository(path) => write!(
                f,
                "build path is outside the repository ({} bytes)",
                path.as_str().len()
            ),
            Self::OutsideWork(path) => write!(
                f,
                "selected build path is not part of the current work ({} bytes)",
                path.as_str().len()
            ),
        }
    }
}

impl Error for BuildError {}

impl From<crate::project_context::ProjectContextError> for BuildError {
    fn from(source: crate::project_context::ProjectContextError) -> Self {
        Self::Context(source)
    }
}

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
    let event = CommandEvent::new(
        context.clock.now_rfc3339(),
        arguments,
        command,
        result.outcome,
        result.exit_code,
    );
    match TelemetryWriter::new(context.paths.clone()).append(context.fs, &event) {
        Ok(()) => ExitCode::from(result.exit_code),
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
            ExitCode::FAILURE
        }
    }
}

fn render_build_results(status: &str, lines: Vec<String>, has_reviews: bool) -> String {
    ViewBuilder::new()
        .title("rapport build")
        .section("Builds", |b| {
            b.items(std::iter::once(format!("status `{status}`")).chain(lines))
        })
        .next_actions(if status == "pass" && has_reviews {
            nonempty![RunHint::new("rapport review start")]
        } else if status == "pass" {
            nonempty![RunHint::new("rapport integrate")]
        } else {
            nonempty![RunHint::new("fix validation, then run rapport build")]
        })
        .build()
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

fn render_no_builds() -> String {
    ViewBuilder::new()
        .title("rapport build")
        .paragraph("No build signoff applies to the selected active-work paths.")
        .next_actions(nonempty![RunHint::new("rapport context show <path>")])
        .build()
}

fn render_build_error(error: &BuildError) -> String {
    let next = match error {
        BuildError::OutsideWork(_) => String::from("rapport work add path <path>"),
        _ => String::from("rapport work status"),
    };
    ViewBuilder::new()
        .title("rapport build")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new(next)])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_error_diagnostics_redact_paths_and_sources() {
        let path_error = BuildError::OutsideWork(Utf8PathBuf::from("PRIVATE/path.rs"));
        let invoke_error = BuildError::Invoke(std::io::Error::other("PRIVATE IO DETAIL"));

        let diagnostics = format!("{path_error:?} {path_error} {invoke_error:?} {invoke_error}");

        assert!(!diagnostics.contains("PRIVATE"));
        assert!(diagnostics.contains("outside_work"));
        assert!(diagnostics.contains("invoke"));
    }
}

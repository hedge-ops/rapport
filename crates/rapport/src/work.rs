use crate::build;
use crate::cli::{WorkAddCommand, WorkCompleteArgs, WorkStartArgs, WorkTaskCommand};
use crate::context::{Clock, CommandContext};
use crate::review;
use crate::rules::{PathRules, RuleResolver, RulesError};
use crate::runner::CommandSpec;
use crate::state::{
    OperationStatus, ReviewActionStatus, WorkFact, WorkState, WorkStateError, WorkStateStore,
    WorkStatus,
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

pub fn status<F, C, O, E>(
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
        Ok(Some(mut state)) => match build::status_lines(context, &mut state) {
            Ok(build_lines) => match review::status_lines(context, &mut state) {
                Ok(review_lines) => match store.save(context.fs, &state) {
                    Ok(()) => {
                        let _ = writeln!(
                            context.out,
                            "{}",
                            render_active_work_with_signoffs(&state, &build_lines, &review_lines)
                        );
                        CommandResult::success()
                    }
                    Err(error) => {
                        let _ = writeln!(context.err, "{}", render_invalid_work_state(&error));
                        CommandResult::failure()
                    }
                },
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_review_state_error(&error));
                    CommandResult::failure()
                }
            },
            Err(error) => {
                let _ = writeln!(context.err, "{}", render_build_state_error(&error));
                CommandResult::failure()
            }
        },
        Ok(None) => {
            let _ = writeln!(
                context.out,
                "{}",
                render_no_work(context.paths.work_state_file().as_str())
            );
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_invalid_work_state(&error));
            CommandResult::failure()
        }
    };
    finish("work status", arguments, context, result)
}

pub fn start<F, C, O, E>(
    start_args: &WorkStartArgs,
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
    match store.load(context.fs) {
        Ok(Some(existing)) => {
            let _ = writeln!(context.err, "{}", render_existing_work(&existing));
            finish("work start", arguments, context, CommandResult::failure())
        }
        Ok(None) => {
            let now = context.clock.now_rfc3339();
            let state = WorkState::new(start_args.title.clone(), now)
                .with_objective(start_args.objective.clone())
                .with_ticket(start_args.ticket.clone())
                .with_plan(start_args.plan.clone())
                .with_paths(start_args.paths.iter().map(ToString::to_string));
            match store.save(context.fs, &state) {
                Ok(()) => {
                    let _ = writeln!(context.out, "{}", render_active_work(&state));
                    finish("work start", arguments, context, CommandResult::success())
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_state_error(&error));
                    finish("work start", arguments, context, CommandResult::failure())
                }
            }
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            finish("work start", arguments, context, CommandResult::failure())
        }
    }
}

pub fn complete<F, C, O, E>(
    complete_args: &WorkCompleteArgs,
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
        Ok(Some(mut state)) => complete_active_work(complete_args, context, &store, &mut state),
        Ok(None) => {
            let _ = writeln!(context.err, "{}", render_missing_work_for_complete());
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_complete_state_error(&error));
            CommandResult::failure()
        }
    };
    finish("work complete", arguments, context, result)
}

fn complete_active_work<F, C, O, E>(
    complete_args: &WorkCompleteArgs,
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let summary = complete_args.summary.trim();
    if summary.is_empty() {
        let _ = writeln!(context.err, "{}", render_missing_summary_for_complete());
        return CommandResult::failure();
    }

    match build::completion_problems(context, state) {
        Ok(problems) if !problems.is_empty() => {
            let _ = writeln!(context.err, "{}", render_build_gate(&problems));
            return CommandResult::failure();
        }
        Ok(_) => {}
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_build_state_error(&error));
            return CommandResult::failure();
        }
    }

    match review::completion_problems(context, state) {
        Ok(problems) if !problems.is_empty() => {
            let _ = writeln!(context.err, "{}", render_review_gate(&problems));
            return CommandResult::failure();
        }
        Ok(_) => {}
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_review_state_error(&error));
            return CommandResult::failure();
        }
    }

    if !complete_args.without_integrate {
        if !has_successful_integration(state) {
            let _ = writeln!(
                context.err,
                "{}",
                render_unintegrated_work_for_complete(state)
            );
            return CommandResult::failure();
        }
        if let Err(error) = validate_completion_head(context, state) {
            let _ = writeln!(context.err, "{}", render_completion_identity_error(&error));
            return CommandResult::failure();
        }
    }

    let now = context.clock.now_rfc3339();
    state.status = WorkStatus::Complete;
    state.updated_at.clone_from(&now);
    state.complete = Some(WorkFact::new("complete").at(&now).summary(summary));

    let filename = archive_filename(state, &now);
    let archive_path = context.paths.history_file(&filename);
    let archive_display = display_repo_path(context.paths.repo_root(), &archive_path);

    match store.archive(context.fs, &filename, state) {
        Ok(()) => match store.clear(context.fs) {
            Ok(()) => {
                let _ = writeln!(
                    context.out,
                    "{}",
                    render_completed_work(state, summary, &archive_display)
                );
                CommandResult::success()
            }
            Err(error) => {
                let _ = writeln!(context.err, "{}", render_complete_state_error(&error));
                CommandResult::failure()
            }
        },
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_complete_state_error(&error));
            CommandResult::failure()
        }
    }
}

pub fn add<F, C, O, E>(
    add_command: &WorkAddCommand,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match add_command {
        WorkAddCommand::Path { path } => add_path(path, arguments, context),
    }
}

pub fn task<F, C, O, E>(
    task_command: &WorkTaskCommand,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match task_command {
        WorkTaskCommand::Address(args) => {
            address_review_task(&args.id, &args.summary, arguments, context)
        }
    }
}

fn address_review_task<F, C, O, E>(
    id: &str,
    summary: &str,
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
            address_review_task_in_state(id, summary, context, &store, &mut state)
        }
        Ok(None) => {
            let _ = writeln!(context.err, "{}", render_missing_work_for_add());
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            CommandResult::failure()
        }
    };
    finish("work task address", arguments, context, result)
}

fn address_review_task_in_state<F, C, O, E>(
    id: &str,
    summary: &str,
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if summary.trim().is_empty() {
        let rendered = ViewBuilder::new()
            .title("rapport work task address")
            .paragraph("An addressing summary is required.")
            .next_actions(nonempty![RunHint::new(format!(
                "rapport work task address {id} --summary \"what changed\""
            ))])
            .build();
        let _ = writeln!(context.err, "{rendered}");
        return CommandResult::failure();
    }

    let matches = state
        .reviews
        .values()
        .flat_map(|review| &review.actions)
        .filter(|action| action.id == id)
        .count();
    if matches == 1 {
        return update_review_task(id, summary, context, store, state);
    }

    let message = if matches == 0 {
        format!("Review task `{id}` does not exist in active work.")
    } else {
        format!("Review task `{id}` is ambiguous in active work.")
    };
    let rendered = ViewBuilder::new()
        .title("rapport work task address")
        .paragraph(message)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build();
    let _ = writeln!(context.err, "{rendered}");
    CommandResult::failure()
}

fn update_review_task<F, C, O, E>(
    id: &str,
    summary: &str,
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let now = context.clock.now_rfc3339();
    let mut prior_status = None;
    for review in state.reviews.values_mut() {
        let Some(action) = review.actions.iter_mut().find(|action| action.id == id) else {
            continue;
        };
        if action.status == ReviewActionStatus::Open {
            action.status = ReviewActionStatus::Addressed;
            action.addressed_at = Some(now.clone());
            action.addressed_summary = Some(summary.trim().to_string());
            if review.status == OperationStatus::Pending {
                review.status = OperationStatus::Stale;
            }
        } else {
            prior_status = Some(action.status);
        }
        break;
    }
    if let Some(status) = prior_status {
        let rendered = ViewBuilder::new()
            .title("rapport work task address")
            .paragraph(format!(
                "Review task `{id}` is already {status}; only open tasks can be addressed."
            ))
            .next_actions(nonempty![RunHint::new("rapport work status")])
            .build();
        let _ = writeln!(context.err, "{rendered}");
        return CommandResult::failure();
    }

    state.updated_at = now;
    if let Err(error) = store.save(context.fs, state) {
        let _ = writeln!(context.err, "{}", render_state_error(&error));
        return CommandResult::failure();
    }
    let rendered = ViewBuilder::new()
        .title("rapport work task address")
        .section("Task", |b| {
            b.entries(vec![
                ("id", id.to_string()),
                ("status", String::from("addressed")),
                ("summary", summary.trim().to_string()),
            ])
        })
        .next_actions(nonempty![RunHint::new("rapport review start")])
        .build();
    let _ = writeln!(context.out, "{rendered}");
    CommandResult::success()
}

fn add_path<F, C, O, E>(
    path: &Utf8Path,
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
            match resolve_existing_work_path(context.fs, &context.repo_root, &context.cwd, path) {
                Ok(path) if state.paths.iter().any(|existing| existing == path.as_str()) => {
                    let _ = writeln!(context.err, "{}", render_duplicate_path(&path, &state));
                    CommandResult::failure()
                }
                Ok(path) => {
                    let resolver = RuleResolver::new(context.paths.clone());
                    match resolver.resolve_path(context.fs, &path) {
                        Ok(path_rules) => {
                            state.paths.push(path.to_string());
                            state.build = None;
                            state.integrate = None;
                            state.signoff = None;
                            for build in state.builds.values_mut() {
                                if build.result_status.is_none()
                                    && build.status != OperationStatus::Stale
                                {
                                    build.result_status = Some(build.status);
                                }
                                build.status = OperationStatus::Stale;
                            }
                            for review in state.reviews.values_mut() {
                                review.status = OperationStatus::Stale;
                            }
                            state.updated_at = context.clock.now_rfc3339();
                            match store.save(context.fs, &state) {
                                Ok(()) => {
                                    let _ = writeln!(
                                        context.out,
                                        "{}",
                                        render_added_path(&resolver, &path, &state, &path_rules)
                                    );
                                    CommandResult::success()
                                }
                                Err(error) => {
                                    let _ =
                                        writeln!(context.err, "{}", render_add_state_error(&error));
                                    CommandResult::failure()
                                }
                            }
                        }
                        Err(error) => {
                            let _ = writeln!(context.err, "{}", render_add_rules_error(&error));
                            CommandResult::failure()
                        }
                    }
                }
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_path_error(&error));
                    CommandResult::failure()
                }
            }
        }
        Ok(None) => {
            let _ = writeln!(context.err, "{}", render_missing_work_for_add());
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_add_state_error(&error));
            CommandResult::failure()
        }
    };
    finish("work add path", arguments, context, result)
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

#[derive(Debug)]
enum AddPathError {
    OutsideRepository { path: Utf8PathBuf },
    Missing { path: Utf8PathBuf },
}

impl fmt::Display for AddPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideRepository { path } => {
                write!(f, "`{path}` is outside the repository.")
            }
            Self::Missing { path } => write!(f, "`{path}` does not exist."),
        }
    }
}

impl Error for AddPathError {}

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

fn resolve_existing_work_path(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    cwd: &Utf8Path,
    path: &Utf8Path,
) -> Result<Utf8PathBuf, AddPathError> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let relative_path =
        absolute_path
            .strip_prefix(repo_root)
            .map_err(|_| AddPathError::OutsideRepository {
                path: absolute_path.clone(),
            })?;
    if !fs.exists(&absolute_path) {
        return Err(AddPathError::Missing {
            path: absolute_path,
        });
    }
    if relative_path.as_str().is_empty() {
        Ok(Utf8PathBuf::from("."))
    } else {
        Ok(relative_path.to_path_buf())
    }
}

fn has_successful_integration(state: &WorkState) -> bool {
    let pr_created = state
        .integrate
        .as_ref()
        .is_some_and(|fact| fact.status == "pr_created");
    let signoff_complete = state
        .signoff
        .as_ref()
        .is_some_and(|fact| matches!(fact.status.as_str(), "pass" | "none"));
    pr_created && signoff_complete
}

fn validate_completion_head<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &WorkState,
) -> Result<(), CompletionIdentityError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let expected = state
        .integrate
        .as_ref()
        .and_then(|fact| fact.commit.as_deref())
        .ok_or(CompletionIdentityError::MissingIntegratedCommit)?;
    let outcome = context
        .runner
        .run(
            &CommandSpec::new("git", ["rev-parse", "HEAD"]),
            &context.repo_root,
        )
        .map_err(CompletionIdentityError::Invoke)?;
    if !outcome.success {
        return Err(CompletionIdentityError::CommandFailed(
            outcome.stderr.trim().to_string(),
        ));
    }
    let current = outcome.stdout.trim();
    if current != expected {
        return Err(CompletionIdentityError::Mismatch {
            expected: expected.to_string(),
            current: current.to_string(),
        });
    }
    Ok(())
}

enum CompletionIdentityError {
    MissingIntegratedCommit,
    Invoke(std::io::Error),
    CommandFailed(String),
    Mismatch { expected: String, current: String },
}

impl fmt::Debug for CompletionIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, detail_bytes) = match self {
            Self::MissingIntegratedCommit => ("missing_integrated_commit", 0),
            Self::Invoke(source) => ("invoke", source.to_string().len()),
            Self::CommandFailed(stderr) => ("command_failed", stderr.len()),
            Self::Mismatch { expected, current } => {
                ("mismatch", expected.len().saturating_add(current.len()))
            }
        };
        f.debug_struct("CompletionIdentityError")
            .field("kind", &kind)
            .field("detail_bytes", &detail_bytes)
            .finish()
    }
}

impl fmt::Display for CompletionIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingIntegratedCommit => {
                f.write_str("successful integration does not record its commit SHA")
            }
            Self::Invoke(source) => {
                write!(f, "could not read current HEAD ({:?})", source.kind())
            }
            Self::CommandFailed(stderr) => {
                write!(f, "`git rev-parse HEAD` failed ({} bytes)", stderr.len())
            }
            Self::Mismatch { expected, current } => write!(
                f,
                "current HEAD does not match integrated PR head (current {} bytes, expected {} bytes)",
                current.len(),
                expected.len()
            ),
        }
    }
}

impl Error for CompletionIdentityError {}

fn archive_filename(state: &WorkState, timestamp: &str) -> String {
    format!(
        "{}-{}.toml",
        timestamp.replace(':', "-"),
        safe_filename_part(&state.title)
    )
}

fn safe_filename_part(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_separator = true;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            slug.push('-');
            last_was_separator = true;
        }
    }

    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        String::from("work")
    } else {
        trimmed
    }
}

fn display_repo_path(repo_root: &Utf8Path, path: &Utf8Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string()
        .replace('\\', "/")
}

fn render_no_work(state_file: &str) -> String {
    ViewBuilder::new()
        .title("rapport work status")
        .paragraph(format!("No active work state found at `{state_file}`."))
        .paragraph("Start work to create local context for the current task.")
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

pub fn render_active_work(state: &WorkState) -> String {
    render_active_work_with_signoffs(state, &[], &[])
}

fn render_active_work_with_signoffs(
    state: &WorkState,
    build_lines: &[String],
    review_lines: &[String],
) -> String {
    let mut details = vec![("title", state.title.clone())];
    if let Some(ticket) = &state.ticket {
        details.push(("ticket", ticket.clone()));
    }
    if let Some(plan) = &state.plan {
        details.push(("plan", plan.clone()));
    }
    if let Some(objective) = &state.objective {
        details.push(("objective", objective.clone()));
    }
    details.extend([
        ("stage", state.stage.to_string()),
        ("status", state.status.to_string()),
        ("created", state.created_at.clone()),
        ("updated", state.updated_at.clone()),
    ]);

    let paths = if state.paths.is_empty() {
        vec![String::from("No paths added yet.")]
    } else {
        state.paths.clone()
    };

    let mut builder = ViewBuilder::new()
        .title("rapport work status")
        .section("Work", |b| b.entries(details))
        .section("Paths", |b| b.items(paths));

    let facts = recent_facts(state);
    if !facts.is_empty() {
        builder = builder.section("Recent", |b| b.entries(facts));
    }
    if !build_lines.is_empty() {
        builder = builder.section("Build Signoffs", |b| b.items(build_lines.to_vec()));
    }
    if !review_lines.is_empty() {
        builder = builder.section("Review Signoffs", |b| b.items(review_lines.to_vec()));
    }

    let open_task = first_open_review_task(review_lines);
    let next = if let Some(task_id) = open_task {
        format!("rapport work task address {task_id} --summary \"what changed\"")
    } else if !review_lines.is_empty()
        && review_lines.iter().any(|line| {
            [" missing", " pending", " stale", " fail"]
                .iter()
                .any(|status| line.contains(status))
        })
    {
        String::from("rapport review start")
    } else if build_lines.iter().any(|line| {
        [" missing", " stale", " fail"]
            .iter()
            .any(|status| line.contains(status))
    }) {
        String::from("rapport build")
    } else {
        String::from("rapport integrate")
    };
    builder.next_actions(nonempty![RunHint::new(next)]).build()
}

fn first_open_review_task(review_lines: &[String]) -> Option<String> {
    review_lines.iter().find_map(|line| {
        line.strip_prefix("task `")
            .and_then(|remaining| remaining.split_once("` open;"))
            .map(|(id, _)| id.to_string())
    })
}

fn render_build_gate(problems: &[String]) -> String {
    ViewBuilder::new()
        .title("rapport work complete")
        .paragraph("Required builds are not complete.")
        .section("Builds", |b| b.items(problems.to_vec()))
        .next_actions(nonempty![RunHint::new("rapport build")])
        .build()
}

fn render_review_gate(problems: &[String]) -> String {
    ViewBuilder::new()
        .title("rapport work complete")
        .paragraph("Required reviews are not complete.")
        .section("Reviews", |b| b.items(problems.to_vec()))
        .next_actions(nonempty![RunHint::new("rapport review start")])
        .build()
}

fn render_review_state_error(error: &review::ReviewError) -> String {
    ViewBuilder::new()
        .title("rapport work status")
        .paragraph("Could not evaluate required review state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport context doctor <path>")])
        .build()
}

fn render_completion_identity_error(error: &CompletionIdentityError) -> String {
    ViewBuilder::new()
        .title("rapport work complete")
        .paragraph("Current work no longer matches the integrated pull request head.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport integrate")])
        .build()
}

fn render_build_state_error(error: &build::BuildError) -> String {
    ViewBuilder::new()
        .title("rapport work status")
        .paragraph("Could not evaluate required build state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport context doctor <path>")])
        .build()
}

fn render_added_path(
    resolver: &RuleResolver,
    path: &Utf8Path,
    state: &WorkState,
    path_rules: &PathRules,
) -> String {
    ViewBuilder::new()
        .title("rapport work add path")
        .section("Path", |b| {
            b.entries([
                ("status", String::from("added")),
                ("path", path.to_string()),
            ])
        })
        .section("Current Work Paths", |b| b.items(state.paths.clone()))
        .section("Benchmarks", |b| {
            b.items(render_path_rules(resolver, path_rules))
        })
        .next_actions(nonempty![RunHint::new("rapport work rules list")])
        .build()
}

fn render_path_rules(resolver: &RuleResolver, path_rules: &PathRules) -> Vec<String> {
    if let Some(reason) = path_rules.unresolved {
        return vec![format!(
            "`{}` -- unresolved: {reason}",
            path_rules.requested_path
        )];
    }
    let mut lines = vec![format!("path `{}`", path_rules.requested_path)];
    lines.extend(path_rules.rules.iter().map(|rule| {
        format!(
            "`{}` -- {} ({})",
            rule.id,
            rule.text,
            resolver.display_path(&rule.source)
        )
    }));
    lines
}

fn recent_facts(state: &WorkState) -> Vec<(&'static str, String)> {
    [
        ("build", state.build.as_ref()),
        ("integrate", state.integrate.as_ref()),
        ("signoff", state.signoff.as_ref()),
        ("complete", state.complete.as_ref()),
    ]
    .into_iter()
    .filter_map(|(name, fact)| fact.map(|fact| (name, render_fact(fact))))
    .collect()
}

fn render_fact(fact: &WorkFact) -> String {
    let mut rendered = fact.status.clone();
    if let Some(at) = &fact.at {
        rendered.push_str(" at ");
        rendered.push_str(at);
    }
    if let Some(summary) = &fact.summary {
        rendered.push_str(": ");
        rendered.push_str(summary);
    }
    rendered
}

fn render_invalid_work_state(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport work status")
        .paragraph("Could not read active work state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new(
            "fix .rapport/work.toml or remove it before starting new work"
        )])
        .build()
}

fn render_existing_work(state: &WorkState) -> String {
    ViewBuilder::new()
        .title("rapport work start")
        .paragraph(format!("Active work already exists: `{}`.", state.title))
        .paragraph("Rapport will not overwrite `.rapport/work.toml`.")
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_duplicate_path(path: &Utf8Path, state: &WorkState) -> String {
    ViewBuilder::new()
        .title("rapport work add path")
        .paragraph(format!("Path `{path}` is already present in active work."))
        .section("Current Work Paths", |b| b.items(state.paths.clone()))
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_missing_work_for_add() -> String {
    ViewBuilder::new()
        .title("rapport work add path")
        .paragraph("No active work state found.")
        .paragraph("Start work before adding paths.")
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

fn render_missing_work_for_complete() -> String {
    ViewBuilder::new()
        .title("rapport work complete")
        .paragraph("No active work state found.")
        .paragraph("Start work before completing it.")
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

fn render_missing_summary_for_complete() -> String {
    ViewBuilder::new()
        .title("rapport work complete")
        .paragraph("Completion summary cannot be empty.")
        .next_actions(nonempty![RunHint::new(
            "rapport work complete --summary \"...\""
        )])
        .build()
}

fn render_unintegrated_work_for_complete(state: &WorkState) -> String {
    let problem = match state.signoff.as_ref().map(|fact| fact.status.as_str()) {
        Some("pending") => String::from("Required signoffs are still pending."),
        Some("fail") => String::from("Required signoffs are failing."),
        _ => format!(
            "Active work `{}` has not recorded a successful integration.",
            state.title
        ),
    };
    ViewBuilder::new()
        .title("rapport work complete")
        .paragraph(problem)
        .paragraph("Use the local-only flag only when this work should close without a PR.")
        .next_actions(nonempty![
            RunHint::new("rapport integrate"),
            RunHint::new("rapport work complete --summary \"...\" --without-integrate")
        ])
        .build()
}

fn render_completed_work(state: &WorkState, summary: &str, archive_path: &str) -> String {
    let mut work = vec![("title", state.title.clone())];
    if let Some(ticket) = &state.ticket {
        work.push(("ticket", ticket.clone()));
    }
    if let Some(integrate) = &state.integrate
        && let Some(pr_url) = &integrate.pr_url
    {
        work.push(("pr", pr_url.clone()));
    }

    ViewBuilder::new()
        .title("rapport work complete")
        .section("Completion", |b| {
            b.entries([
                ("status", state.status.to_string()),
                ("summary", summary.to_string()),
                ("archive", archive_path.to_string()),
            ])
        })
        .section("Work", |b| b.entries(work))
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

fn render_path_error(error: &AddPathError) -> String {
    ViewBuilder::new()
        .title("rapport work add path")
        .paragraph("Could not add path to active work.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_add_rules_error(error: &RulesError) -> String {
    ViewBuilder::new()
        .title("rapport work add path")
        .paragraph("Could not resolve repository rules for the added path.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work rules list <path>")])
        .build()
}

fn render_add_state_error(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport work add path")
        .paragraph("Could not update active work state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_complete_state_error(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport work complete")
        .paragraph("Could not complete active work state.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_state_error(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport work start")
        .paragraph("Could not write active work state.")
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
    use crate::state::{WorkStage, WorkStatus};

    #[test]
    fn active_work_view_includes_recent_facts_when_present() {
        let mut state = WorkState::new("Do the thing", "2026-07-07T23:00:00Z");
        state.paths = vec![String::from("app/api")];
        state.stage = WorkStage::Development;
        state.status = WorkStatus::Active;
        state.build = Some(
            WorkFact::new("pass")
                .at("2026-07-07T23:05:00Z")
                .summary("just ci"),
        );

        let view = render_active_work(&state);

        assert!(view.contains("Do the thing"));
        assert!(view.contains("app/api"));
        assert!(view.contains("just ci"));
    }

    #[test]
    fn next_action_uses_only_open_tasks_from_current_review_lines() {
        let state = WorkState::new("Do the thing", "2026-07-07T23:00:00Z");
        let current_pass = vec![String::from(
            "`root-review` current pass; grade A (minimum A-)",
        )];

        let view = render_active_work_with_signoffs(&state, &[], &current_pass);

        assert!(view.contains("rapport integrate"));
        assert!(!view.contains("rapport work task address"));

        let current_open = vec![
            String::from("`root-review` current fail; grade B (minimum A-)"),
            String::from("task `REV-123` open; fix the current review"),
        ];
        let view = render_active_work_with_signoffs(&state, &[], &current_open);
        assert!(view.contains("rapport work task address REV-123 --summary \"what changed\""));
    }

    #[test]
    fn completion_identity_diagnostics_redact_sources_output_and_shas() {
        let invoke = CompletionIdentityError::Invoke(std::io::Error::other("PRIVATE IO"));
        let failed = CompletionIdentityError::CommandFailed(String::from("PRIVATE STDERR"));
        let mismatch = CompletionIdentityError::Mismatch {
            expected: String::from("PRIVATE EXPECTED SHA"),
            current: String::from("PRIVATE CURRENT SHA"),
        };

        let diagnostics =
            format!("{invoke:?} {invoke} {failed:?} {failed} {mismatch:?} {mismatch}");

        assert!(!diagnostics.contains("PRIVATE"));
        assert!(diagnostics.contains("invoke"));
        assert!(diagnostics.contains("command_failed"));
        assert!(diagnostics.contains("mismatch"));
    }
}

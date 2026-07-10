use crate::cli::IntegrateArgs;
use crate::context::{Clock, CommandContext};
use crate::project_context::{self, SignoffRequirement};
use crate::runner::{CommandOutcome, CommandSpec};
use crate::signoff_contract::SignoffKind;
use crate::state::{OperationStatus, WorkFact, WorkState, WorkStateError, WorkStateStore};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder, build, review};
use nonempty::nonempty;
use rapport_files::FileSystem;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::io;
use std::io::Write;
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;

pub fn run<F, C, O, E>(
    integrate_args: &IntegrateArgs,
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
        Ok(Some(state)) => integrate(integrate_args, &arguments, context, &store, state),
        Ok(None) => {
            let _ = writeln!(context.err, "{}", render_missing_work());
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            CommandResult::failure()
        }
    };
    finish("integrate", arguments, context, result)
}

fn integrate<F, C, O, E>(
    integrate_args: &IntegrateArgs,
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    mut state: WorkState,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if let Err(error) = validate_work_context(&state) {
        let _ = writeln!(context.err, "{}", render_work_context_error(&error));
        return CommandResult::failure();
    }

    if let Some(intent) = RecordedCommitIntent::from_state(&state) {
        let resolved = match evaluate_signoffs(arguments, context, &state.paths) {
            Ok(resolved) => resolved,
            Err(result) => return result,
        };
        return resume_commit(arguments, context, store, &mut state, &intent, &resolved);
    }

    if let Some(publication) = RecordedPublication::from_state(&state) {
        if let Err(result) = require_clean_worktree(context) {
            return result;
        }
        let resolved = match evaluate_signoffs(arguments, context, &state.paths) {
            Ok(resolved) => resolved,
            Err(result) => return result,
        };
        return publish_and_sign(
            arguments,
            context,
            store,
            &mut state,
            &publication,
            &resolved,
        );
    }

    let request = IntegrationRequest::from_args(integrate_args);
    if request.is_none()
        && let Some(integration) = RecordedIntegration::from_state(&state)
    {
        if let Err(result) = require_clean_worktree(context) {
            return result;
        }
        let resolved = match evaluate_signoffs(arguments, context, &state.paths) {
            Ok(resolved) => resolved,
            Err(result) => return result,
        };
        return resume_signoff(
            arguments,
            context,
            store,
            &mut state,
            &integration,
            &resolved,
        );
    }
    let Some(request) = request else {
        let _ = writeln!(context.err, "{}", render_missing_summary_or_message());
        return CommandResult::failure();
    };
    let resolved_signoffs = match evaluate_signoffs(arguments, context, &state.paths) {
        Ok(resolved) => resolved,
        Err(result) => return result,
    };
    start_new_integration(
        arguments,
        context,
        store,
        &mut state,
        request,
        &resolved_signoffs,
    )
}

fn start_new_integration<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    request: IntegrationRequest<'_>,
    resolved_signoffs: &ResolvedSignoffs,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if let Err(error) = record_event(
        "integrate start",
        arguments.to_owned(),
        context,
        CommandResult::success(),
    ) {
        let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
        return CommandResult::failure();
    }

    let inspection = match inspect_active_changes(arguments, context, state) {
        Ok(inspection) => inspection,
        Err(result) => return result,
    };
    let branch = match current_branch(context) {
        Ok(branch) => branch,
        Err(result) => return result,
    };
    let base_commit = match current_head(context) {
        Ok(commit) => commit,
        Err(result) => return result,
    };
    let intent = RecordedCommitIntent {
        summary: request.summary.to_string(),
        message: request.message.to_string(),
        branch: branch.clone(),
        base_commit,
    };
    if let Err(result) = save_commit_intent(context, store, state, &intent) {
        return result;
    }

    let commit = match commit_active_changes(arguments, context, request, &inspection.commit_paths)
    {
        Ok(commit) => commit,
        Err(result) => return result,
    };

    let publication = RecordedPublication {
        summary: request.summary.to_string(),
        message: request.message.to_string(),
        branch,
        commit,
    };
    if let Err(result) = save_publication(context, store, state, &publication) {
        return result;
    }
    publish_and_sign(
        arguments,
        context,
        store,
        state,
        &publication,
        resolved_signoffs,
    )
}

#[derive(Debug, Clone, Copy)]
struct IntegrationRequest<'request> {
    summary: &'request str,
    message: &'request str,
}

impl<'request> IntegrationRequest<'request> {
    fn from_args(args: &'request IntegrateArgs) -> Option<Self> {
        let summary = args.summary.as_deref()?.trim();
        let message = args.message.as_deref()?.trim();
        if summary.is_empty() || message.is_empty() {
            None
        } else {
            Some(Self { summary, message })
        }
    }
}

#[derive(Debug, Clone)]
struct RecordedIntegration {
    summary: String,
    branch: String,
    commit: String,
    pr_url: String,
}

#[derive(Debug, Clone)]
struct RecordedCommitIntent {
    summary: String,
    message: String,
    branch: String,
    base_commit: String,
}

impl RecordedCommitIntent {
    fn from_state(state: &WorkState) -> Option<Self> {
        let fact = state.integrate.as_ref()?;
        if fact.status != "committing" {
            return None;
        }
        Some(Self {
            summary: fact.summary.clone()?,
            message: fact.message.clone()?,
            branch: fact.branch.clone()?,
            base_commit: fact.commit.clone()?,
        })
    }

    fn request(&self) -> IntegrationRequest<'_> {
        IntegrationRequest {
            summary: &self.summary,
            message: &self.message,
        }
    }
}

#[derive(Debug, Clone)]
struct RecordedPublication {
    summary: String,
    message: String,
    branch: String,
    commit: String,
}

impl RecordedPublication {
    fn from_state(state: &WorkState) -> Option<Self> {
        let fact = state.integrate.as_ref()?;
        if fact.status != "publishing" {
            return None;
        }
        Some(Self {
            summary: fact.summary.clone()?,
            message: fact.message.clone()?,
            branch: fact.branch.clone()?,
            commit: fact.commit.clone()?,
        })
    }
}

impl RecordedIntegration {
    fn from_state(state: &WorkState) -> Option<Self> {
        let fact = state.integrate.as_ref()?;
        if fact.status != "pr_created" {
            return None;
        }
        Some(Self {
            summary: fact.summary.clone()?,
            branch: fact.branch.clone()?,
            commit: fact.commit.clone()?,
            pr_url: fact.pr_url.clone()?,
        })
    }
}

fn inspect_active_changes<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &WorkState,
) -> Result<StatusInspection, CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let status = match run_step(
        context,
        &CommandSpec::new("git", ["status", "--porcelain=v1"]),
    ) {
        Ok(outcome) if outcome.success => outcome.stdout,
        Ok(outcome) => {
            record_best_effort(
                "integrate inspect",
                arguments.to_owned(),
                context,
                CommandResult::failure(),
            );
            let _ = writeln!(
                context.err,
                "{}",
                render_command_failure("git status", &outcome)
            );
            return Err(CommandResult::failure());
        }
        Err(error) => {
            record_best_effort(
                "integrate inspect",
                arguments.to_owned(),
                context,
                CommandResult::failure(),
            );
            let _ = writeln!(
                context.err,
                "{}",
                render_command_invoke_error("git status", &error)
            );
            return Err(CommandResult::failure());
        }
    };

    match inspect_status(&status, &state.paths) {
        Ok(inspection) => {
            record_best_effort(
                "integrate inspect",
                arguments.to_owned(),
                context,
                CommandResult::success(),
            );
            Ok(inspection)
        }
        Err(error) => {
            record_best_effort(
                "integrate inspect",
                arguments.to_owned(),
                context,
                CommandResult::failure(),
            );
            let _ = writeln!(context.err, "{}", render_status_error(&error));
            Err(CommandResult::failure())
        }
    }
}

fn current_branch<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match run_step(
        context,
        &CommandSpec::new("git", ["branch", "--show-current"]),
    ) {
        Ok(outcome) if outcome.success => Ok(present_or_unknown(outcome.stdout.trim())),
        Ok(outcome) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_command_failure("git branch --show-current", &outcome)
            );
            Err(CommandResult::failure())
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_command_invoke_error("git branch --show-current", &error)
            );
            Err(CommandResult::failure())
        }
    }
}

fn current_head<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match run_step(context, &CommandSpec::new("git", ["rev-parse", "HEAD"])) {
        Ok(outcome) if outcome.success => Ok(outcome.stdout.trim().to_string()),
        Ok(outcome) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_command_failure("git rev-parse HEAD", &outcome)
            );
            Err(CommandResult::failure())
        }
        Err(error) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_command_invoke_error("git rev-parse HEAD", &error)
            );
            Err(CommandResult::failure())
        }
    }
}

fn commit_active_changes<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    request: IntegrationRequest<'_>,
    commit_paths: &[String],
) -> Result<String, CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match create_commit(context, commit_paths, request.summary, request.message) {
        Ok(commit) => {
            record_best_effort(
                "integrate commit",
                arguments.to_owned(),
                context,
                CommandResult::success(),
            );
            Ok(commit)
        }
        Err(error) => {
            record_best_effort(
                "integrate commit",
                arguments.to_owned(),
                context,
                CommandResult::failure(),
            );
            let _ = writeln!(context.err, "{}", render_commit_error(&error));
            Err(CommandResult::failure())
        }
    }
}

fn open_pull_request<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    publication: &RecordedPublication,
    pr_body: &str,
) -> Result<String, CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match create_or_update_pr(context, &publication.branch, &publication.summary, pr_body) {
        Ok(pr_url) => {
            record_best_effort(
                "integrate pr",
                arguments.to_owned(),
                context,
                CommandResult::success(),
            );
            Ok(pr_url)
        }
        Err(error) => {
            record_best_effort(
                "integrate pr",
                arguments.to_owned(),
                context,
                CommandResult::failure(),
            );
            let _ = writeln!(context.err, "{}", render_pr_error(&error));
            Err(CommandResult::failure())
        }
    }
}

fn resume_commit<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    intent: &RecordedCommitIntent,
    resolved: &ResolvedSignoffs,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let branch = match current_branch(context) {
        Ok(branch) => branch,
        Err(result) => return result,
    };
    if branch != intent.branch {
        let error = SignoffError::execution(
            "local branch does not match committing branch",
            format!("current `{branch}`, expected `{}`", intent.branch),
        );
        let _ = writeln!(context.err, "{}", render_signoff_error(&error));
        return CommandResult::failure();
    }
    let head = match current_head(context) {
        Ok(head) => head,
        Err(result) => return result,
    };
    let commit = if head == intent.base_commit {
        let inspection = match inspect_active_changes(arguments, context, state) {
            Ok(inspection) => inspection,
            Err(result) => return result,
        };
        match commit_active_changes(
            arguments,
            context,
            intent.request(),
            &inspection.commit_paths,
        ) {
            Ok(commit) => commit,
            Err(result) => return result,
        }
    } else {
        if let Err(result) = require_clean_worktree(context) {
            return result;
        }
        let parent = match signoff_stdout(
            context,
            &CommandSpec::new("git", ["rev-parse", "HEAD^"]),
            "git rev-parse HEAD^",
        ) {
            Ok(parent) => parent,
            Err(error) => {
                let _ = writeln!(context.err, "{}", render_signoff_error(&error));
                return CommandResult::failure();
            }
        };
        let message = match signoff_stdout(
            context,
            &CommandSpec::new("git", ["log", "-1", "--format=%s%n%n%b"]),
            "git log -1",
        ) {
            Ok(message) => message,
            Err(error) => {
                let _ = writeln!(context.err, "{}", render_signoff_error(&error));
                return CommandResult::failure();
            }
        };
        let expected_message = format!("{}\n\n{}", intent.summary, intent.message);
        if parent != intent.base_commit || message != expected_message {
            let error = SignoffError::execution(
                "HEAD does not match the saved integration intent",
                String::from("parent or commit message differs"),
            );
            let _ = writeln!(context.err, "{}", render_signoff_error(&error));
            return CommandResult::failure();
        }
        head
    };
    let publication = RecordedPublication {
        summary: intent.summary.clone(),
        message: intent.message.clone(),
        branch,
        commit,
    };
    if let Err(result) = save_publication(context, store, state, &publication) {
        return result;
    }
    publish_and_sign(arguments, context, store, state, &publication, resolved)
}

fn publish_and_sign<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    publication: &RecordedPublication,
    resolved: &ResolvedSignoffs,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if let Err(result) = require_clean_worktree(context) {
        return result;
    }
    if let Err(error) = validate_local_identity(context, &publication.branch, &publication.commit) {
        let _ = writeln!(context.err, "{}", render_signoff_error(&error));
        return CommandResult::failure();
    }
    let body = pr_body(
        state,
        &publication.message,
        &publication.commit,
        &resolved.report.required,
    );
    let pr_url = match open_pull_request(arguments, context, publication, &body) {
        Ok(pr_url) => pr_url,
        Err(result) => return result,
    };
    let integration = RecordedIntegration {
        summary: publication.summary.clone(),
        branch: publication.branch.clone(),
        commit: publication.commit.clone(),
        pr_url,
    };
    if let Err(error) = validate_pr_identity(context, &integration) {
        let _ = writeln!(context.err, "{}", render_signoff_error(&error));
        return CommandResult::failure();
    }
    if let Err(result) =
        save_pending_integration(context, store, state, &integration, &resolved.report)
    {
        return result;
    }
    finish_signoff(arguments, context, store, state, &integration, resolved)
}

fn evaluate_signoffs<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    work_paths: &[String],
) -> Result<ResolvedSignoffs, CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match run_signoffs(context, work_paths) {
        Ok(report) => Ok(report),
        Err(error) => {
            record_best_effort(
                "integrate signoff",
                arguments.to_owned(),
                context,
                CommandResult::failure(),
            );
            let _ = writeln!(context.err, "{}", render_signoff_error(&error));
            Err(CommandResult::failure())
        }
    }
}

fn resume_signoff<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    integration: &RecordedIntegration,
    resolved: &ResolvedSignoffs,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if let Err(error) = validate_pr_identity(context, integration) {
        let _ = writeln!(context.err, "{}", render_signoff_error(&error));
        return CommandResult::failure();
    }
    if let Err(result) = save_pending_signoffs(context, store, state, &resolved.report) {
        return result;
    }
    finish_signoff(arguments, context, store, state, integration, resolved)
}

fn finish_signoff<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    integration: &RecordedIntegration,
    resolved: &ResolvedSignoffs,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let report = match execute_signoffs(context, store, state, integration, resolved) {
        Ok(report) => report,
        Err(error) => {
            record_best_effort(
                "integrate signoff",
                arguments.to_owned(),
                context,
                CommandResult::failure(),
            );
            let _ = writeln!(context.err, "{}", render_signoff_error(&error));
            return CommandResult::failure();
        }
    };
    let result = if matches!(report.status(), "pass" | "none") {
        CommandResult::success()
    } else {
        CommandResult::failure()
    };
    record_best_effort("integrate signoff", arguments.to_owned(), context, result);
    save_integration_result(context, store, state, integration, &report)
}

fn save_commit_intent<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    intent: &RecordedCommitIntent,
) -> Result<(), CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let now = context.clock.now_rfc3339();
    let mut fact = integration_fact(
        "committing",
        &now,
        &intent.summary,
        &intent.branch,
        &intent.base_commit,
        "",
    );
    fact.pr_url = None;
    fact.message = Some(intent.message.clone());
    state.integrate = Some(fact);
    state.signoff = None;
    state.updated_at = now;
    store.save(context.fs, state).map_err(|error| {
        let _ = writeln!(context.err, "{}", render_state_error(&error));
        CommandResult::failure()
    })
}

fn save_publication<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    publication: &RecordedPublication,
) -> Result<(), CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let now = context.clock.now_rfc3339();
    let mut fact = integration_fact(
        "publishing",
        &now,
        &publication.summary,
        &publication.branch,
        &publication.commit,
        "",
    );
    fact.pr_url = None;
    fact.message = Some(publication.message.clone());
    state.integrate = Some(fact);
    state.signoff = None;
    state.updated_at = now;
    store.save(context.fs, state).map_err(|error| {
        let _ = writeln!(context.err, "{}", render_state_error(&error));
        CommandResult::failure()
    })
}

fn save_pending_integration<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    integration: &RecordedIntegration,
    signoffs: &SignoffReport,
) -> Result<(), CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let now = context.clock.now_rfc3339();
    state.integrate = Some(integration_fact(
        "pr_created",
        &now,
        &integration.summary,
        &integration.branch,
        &integration.commit,
        &integration.pr_url,
    ));
    state.signoff = Some(signoffs.fact(&now));
    state.updated_at = now;
    store.save(context.fs, state).map_err(|error| {
        let _ = writeln!(context.err, "{}", render_state_error(&error));
        CommandResult::failure()
    })
}

fn save_pending_signoffs<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    signoffs: &SignoffReport,
) -> Result<(), CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let now = context.clock.now_rfc3339();
    state.signoff = Some(signoffs.fact(&now));
    state.updated_at = now;
    store.save(context.fs, state).map_err(|error| {
        let _ = writeln!(context.err, "{}", render_state_error(&error));
        CommandResult::failure()
    })
}

fn save_integration_result<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    integration: &RecordedIntegration,
    signoffs: &SignoffReport,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let now = context.clock.now_rfc3339();
    state.signoff = Some(signoffs.fact(&now));
    state.updated_at = now;

    match store.save(context.fs, state) {
        Ok(()) if matches!(signoffs.status(), "pass" | "none") => {
            let _ = writeln!(
                context.out,
                "{}",
                render_integrated(
                    &integration.summary,
                    &integration.branch,
                    &integration.commit,
                    &integration.pr_url,
                    signoffs,
                )
            );
            CommandResult::success()
        }
        Ok(()) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_integrated_with_failed_signoffs(
                    &integration.summary,
                    &integration.branch,
                    &integration.commit,
                    &integration.pr_url,
                    signoffs,
                )
            );
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            CommandResult::failure()
        }
    }
}

fn create_commit<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    commit_paths: &[String],
    summary: &str,
    message: &str,
) -> Result<String, IntegrationStepError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let mut add_args = vec![String::from("add"), String::from("--")];
    add_args.extend(commit_paths.iter().cloned());
    run_success(context, &CommandSpec::new("git", add_args), "git add")?;
    run_success(
        context,
        &CommandSpec::new("git", ["commit", "-m", summary, "-m", message]),
        "git commit",
    )?;
    let outcome = run_success(
        context,
        &CommandSpec::new("git", ["rev-parse", "HEAD"]),
        "git rev-parse HEAD",
    )?;
    Ok(outcome.stdout.trim().to_string())
}

fn create_or_update_pr<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    branch: &str,
    summary: &str,
    body: &str,
) -> Result<String, IntegrationStepError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    run_success(
        context,
        &CommandSpec::new("git", ["push", "--set-upstream", "origin", branch]),
        "git push",
    )?;
    let existing = run_success(
        context,
        &CommandSpec::new(
            "gh",
            [
                "pr", "list", "--head", branch, "--state", "open", "--json", "url",
            ],
        ),
        "gh pr list",
    )?;
    let candidates: Vec<PullRequestCandidate> =
        serde_json::from_str(&existing.stdout).map_err(|error| {
            IntegrationStepError::InvalidOutput {
                command: String::from("gh pr list"),
                message: format!("invalid JSON: {error}"),
            }
        })?;
    if candidates.len() > 1 {
        return Err(IntegrationStepError::InvalidOutput {
            command: String::from("gh pr list"),
            message: format!(
                "found {} open pull requests for branch `{branch}`; multiple PRs per branch are unsupported",
                candidates.len()
            ),
        });
    }
    if let Some(candidate) = candidates.first() {
        let pr_url = &candidate.url;
        run_success(
            context,
            &CommandSpec::new(
                "gh",
                ["pr", "edit", pr_url, "--title", summary, "--body", body],
            ),
            "gh pr edit",
        )?;
        return Ok(pr_url.clone());
    }
    let outcome = run_success(
        context,
        &CommandSpec::new(
            "gh",
            [
                "pr", "create", "--head", branch, "--title", summary, "--body", body,
            ],
        ),
        "gh pr create",
    )?;
    parse_pr_url(&outcome.stdout).ok_or_else(|| IntegrationStepError::InvalidOutput {
        command: String::from("gh pr create"),
        message: String::from("did not print a PR URL"),
    })
}

fn run_signoffs<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    work_paths: &[String],
) -> Result<ResolvedSignoffs, SignoffError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let validation = project_context::validate_repository(context.fs, context.paths.repo_root());
    let problems = validation.problem_details().collect::<Vec<_>>();
    if !problems.is_empty() {
        return Err(SignoffError::Contract {
            problems: problems.into_iter().map(ToString::to_string).collect(),
        });
    }
    let requirements = project_context::required_signoff_requirements_for_paths(
        context.fs,
        context.paths.repo_root(),
        work_paths,
    )
    .map_err(SignoffError::Context)?;
    let required = requirements
        .iter()
        .map(|requirement| requirement.request.qualified_target().to_string())
        .collect();
    Ok(ResolvedSignoffs {
        requirements,
        report: SignoffReport::from_required(required),
    })
}

fn require_clean_worktree<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<(), CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    ensure_clean_worktree(context).map_err(|error| {
        let _ = writeln!(context.err, "{}", render_signoff_error(&error));
        CommandResult::failure()
    })
}

fn ensure_clean_worktree<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<(), SignoffError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let status = signoff_stdout(
        context,
        &CommandSpec::new("git", ["status", "--porcelain"]),
        "git status --porcelain",
    )?;
    if status.is_empty() {
        Ok(())
    } else {
        Err(SignoffError::execution(
            "worktree must be completely clean before signoff",
            status,
        ))
    }
}

fn validate_pr_identity<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    integration: &RecordedIntegration,
) -> Result<ValidatedPullRequest, SignoffError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    validate_local_identity(context, &integration.branch, &integration.commit)?;
    let json = signoff_stdout(
        context,
        &CommandSpec::new(
            "gh",
            [
                "pr",
                "view",
                &integration.pr_url,
                "--json",
                "baseRefOid,headRefOid,headRefName,isCrossRepository,state,url",
            ],
        ),
        "gh pr view",
    )?;
    let pull_request: PullRequestInfo = serde_json::from_str(&json).map_err(|error| {
        SignoffError::execution("invalid pull request response", error.to_string())
    })?;
    if pull_request.state != "OPEN" {
        return Err(SignoffError::execution(
            "signoff requires an open PR",
            pull_request.state,
        ));
    }
    if pull_request.is_cross_repository {
        return Err(SignoffError::execution(
            "fork pull requests are not supported for Rapport signoff",
            String::new(),
        ));
    }
    if pull_request.head_ref_oid != integration.commit {
        return Err(SignoffError::execution(
            "PR HEAD does not match integrated commit",
            format!("{} != {}", pull_request.head_ref_oid, integration.commit),
        ));
    }
    if pull_request.head_ref_name != integration.branch {
        return Err(SignoffError::execution(
            "PR branch does not match integrated branch",
            format!("{} != {}", pull_request.head_ref_name, integration.branch),
        ));
    }
    let repository = signoff_stdout(
        context,
        &CommandSpec::new(
            "gh",
            [
                "repo",
                "view",
                "--json",
                "nameWithOwner",
                "--jq",
                ".nameWithOwner",
            ],
        ),
        "gh repo view",
    )?;
    let expected_prefix = format!("https://github.com/{repository}/pull/");
    if !pull_request.url.starts_with(&expected_prefix) {
        return Err(SignoffError::execution(
            "pull request does not belong to the current repository",
            format!("{} not under {repository}", pull_request.url),
        ));
    }
    signoff_stdout(
        context,
        &CommandSpec::new(
            "git",
            [
                "fetch",
                "--no-tags",
                "origin",
                pull_request.base_ref_oid.as_str(),
            ],
        ),
        "git fetch PR base",
    )?;
    let merge_base_sha = signoff_stdout(
        context,
        &CommandSpec::new(
            "git",
            [
                "merge-base",
                &pull_request.head_ref_oid,
                &pull_request.base_ref_oid,
            ],
        ),
        "git merge-base PR head and base",
    )?;
    Ok(ValidatedPullRequest {
        repository,
        base_ref_oid: pull_request.base_ref_oid,
        merge_base_sha,
    })
}

fn validate_local_identity<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    branch: &str,
    commit: &str,
) -> Result<(), SignoffError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let local_head = signoff_stdout(
        context,
        &CommandSpec::new("git", ["rev-parse", "HEAD"]),
        "git rev-parse HEAD",
    )?;
    if local_head != commit {
        return Err(SignoffError::execution(
            "local HEAD does not match integrated commit",
            format!("{local_head} != {commit}"),
        ));
    }
    let local_branch = signoff_stdout(
        context,
        &CommandSpec::new("git", ["branch", "--show-current"]),
        "git branch --show-current",
    )?;
    if local_branch != branch {
        return Err(SignoffError::execution(
            "local branch does not match integrated branch",
            format!("{local_branch} != {branch}"),
        ));
    }
    Ok(())
}

fn ensure_pr_base_unchanged(
    before: &ValidatedPullRequest,
    after: &ValidatedPullRequest,
) -> Result<(), SignoffError> {
    if before.repository == after.repository
        && before.base_ref_oid == after.base_ref_oid
        && before.merge_base_sha == after.merge_base_sha
    {
        return Ok(());
    }
    Err(SignoffError::execution(
        "PR base changed while signoff was running; rerun `rapport integrate`",
        format!(
            "base {} -> {}, merge-base {} -> {}",
            before.base_ref_oid, after.base_ref_oid, before.merge_base_sha, after.merge_base_sha
        ),
    ))
}

fn execute_signoffs<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    work_state: &mut WorkState,
    integration: &RecordedIntegration,
    resolved: &ResolvedSignoffs,
) -> Result<SignoffReport, SignoffError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    ensure_clean_worktree(context)?;
    let mut pull_request = validate_pr_identity(context, integration)?;
    let combined = fetch_statuses(context, &pull_request.repository, &integration.commit)?;
    verify_status_set(&resolved.requirements, &combined)?;

    let mut all_succeeded = true;
    for requirement in &resolved.requirements {
        ensure_clean_worktree(context)?;
        validate_local_identity(context, &integration.branch, &integration.commit)?;
        let request = &requirement.request;
        let qualified = request.qualified_target().to_string();
        // A remote success proves only that this context name was posted for
        // the SHA. Re-evaluate the shared local service so reuse is accepted
        // only when content, base, rules, and instructions still match.
        let (operation_status, review_packet) = match request.kind() {
            SignoffKind::Build => {
                let execution = build::evaluate_requirement(
                    context,
                    work_state,
                    requirement,
                    &pull_request.merge_base_sha,
                )
                .map_err(|error| {
                    SignoffError::execution("build signoff operation failed", error.to_string())
                })?;
                (execution.status, None)
            }
            SignoffKind::Review => review::evaluate_requirement(
                context,
                work_state,
                requirement,
                &pull_request.merge_base_sha,
            )
            .map_err(|error| {
                SignoffError::execution("review signoff operation failed", error.to_string())
            })?,
        };
        work_state.updated_at = context.clock.now_rfc3339();
        store.save(context.fs, work_state).map_err(|error| {
            SignoffError::execution("could not save signoff work state", error.to_string())
        })?;
        let status_state = match operation_status {
            OperationStatus::Pass => "success",
            OperationStatus::Fail | OperationStatus::Stale => "failure",
            OperationStatus::Pending => "pending",
        };
        if operation_status == OperationStatus::Pass {
            let refreshed = ensure_clean_worktree(context)
                .and_then(|()| validate_pr_identity(context, integration));
            let refreshed = match refreshed.and_then(|refreshed| {
                ensure_pr_base_unchanged(&pull_request, &refreshed)?;
                Ok(refreshed)
            }) {
                Ok(refreshed) => refreshed,
                Err(error) => {
                    let _ = post_signoff_status(
                        context,
                        &pull_request.repository,
                        &integration.commit,
                        &integration.pr_url,
                        &qualified,
                        "failure",
                    );
                    return Err(error);
                }
            };
            pull_request = refreshed;
        }
        post_signoff_status(
            context,
            &pull_request.repository,
            &integration.commit,
            &integration.pr_url,
            &qualified,
            status_state,
        )?;
        if let Some(packet) = review_packet {
            let json = serde_json::to_string_pretty(&packet).map_err(|error| {
                SignoffError::execution("could not encode review request", error.to_string())
            })?;
            let _ = writeln!(
                context.err,
                "Review `{qualified}` requires an independent structured result.\n{json}"
            );
        }
        if operation_status != OperationStatus::Pass {
            all_succeeded = false;
            break;
        }
    }
    if all_succeeded {
        ensure_clean_worktree(context)?;
        let final_pull_request = validate_pr_identity(context, integration)?;
        ensure_pr_base_unchanged(&pull_request, &final_pull_request)?;
        pull_request = final_pull_request;
    }
    let final_statuses = fetch_statuses(context, &pull_request.repository, &integration.commit)?;
    let final_states = verify_status_set(&resolved.requirements, &final_statuses)?;
    Ok(SignoffReport::from_states(
        &resolved.requirements,
        &final_states,
    ))
}

fn fetch_statuses<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    repository: &str,
    commit: &str,
) -> Result<CombinedStatus, SignoffError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let status_path = format!("repos/{repository}/commits/{commit}/status?per_page=100");
    let json = signoff_stdout(
        context,
        &CommandSpec::new("gh", ["api", status_path.as_str()]),
        "gh api commit status",
    )?;
    let combined: CombinedStatus = serde_json::from_str(&json).map_err(|error| {
        SignoffError::execution("invalid commit status response", error.to_string())
    })?;
    ensure_complete_status_page(&combined)?;
    Ok(combined)
}

fn ensure_complete_status_page(combined: &CombinedStatus) -> Result<(), SignoffError> {
    if combined
        .total_count
        .is_some_and(|count| count > combined.statuses.len())
    {
        Err(SignoffError::execution(
            "more than 100 status contexts; exact signoff reconciliation is unsupported",
            combined.statuses.len().to_string(),
        ))
    } else {
        Ok(())
    }
}

fn signoff_stdout<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    spec: &CommandSpec,
    display: &'static str,
) -> Result<String, SignoffError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    run_success(context, spec, display)
        .map(|outcome| outcome.stdout.trim().to_string())
        .map_err(|error| SignoffError::execution(display, error.to_string()))
}

fn verify_status_set(
    requirements: &[SignoffRequirement],
    combined: &CombinedStatus,
) -> Result<BTreeMap<String, String>, SignoffError> {
    let expected = requirements
        .iter()
        .map(|requirement| format!("signoff: {}", requirement.request.qualified_target()))
        .collect::<BTreeSet<_>>();
    let states = combined
        .statuses
        .iter()
        .filter(|status| status.context.starts_with("signoff: "))
        .map(|status| (status.context.clone(), status.state.clone()))
        .collect::<BTreeMap<_, _>>();
    let actual = states.keys().cloned().collect::<BTreeSet<_>>();
    if actual == expected {
        return Ok(states);
    }
    let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
    let unexpected = actual.difference(&expected).cloned().collect::<Vec<_>>();
    Err(SignoffError::execution(
        "PR signoff statuses do not match context",
        format!(
            "missing [{}], unexpected [{}]",
            missing.join(", "),
            unexpected.join(", ")
        ),
    ))
}

fn post_signoff_status<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    repository: &str,
    commit: &str,
    pr_url: &str,
    target: &str,
    state: &str,
) -> Result<(), SignoffError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let path = format!("repos/{repository}/statuses/{commit}");
    let args = vec![
        String::from("api"),
        String::from("-X"),
        String::from("POST"),
        path,
        String::from("-f"),
        format!("context=signoff: {target}"),
        String::from("-f"),
        format!("state={state}"),
        String::from("-f"),
        format!("description=local signoff {state}"),
        String::from("-f"),
        format!("target_url={pr_url}"),
    ];
    signoff_stdout(context, &CommandSpec::new("gh", args), "gh api post status").map(|_| ())
}

fn run_success<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    spec: &CommandSpec,
    display: &'static str,
) -> Result<CommandOutcome, IntegrationStepError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match run_step(context, spec) {
        Ok(outcome) if outcome.success => Ok(outcome),
        Ok(outcome) => Err(IntegrationStepError::CommandFailed {
            command: display.to_string(),
            outcome,
        }),
        Err(source) => Err(IntegrationStepError::Invoke {
            command: display.to_string(),
            source,
        }),
    }
}

fn run_step<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    spec: &CommandSpec,
) -> io::Result<CommandOutcome>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    context.runner.run(spec, context.paths.repo_root())
}

fn validate_work_context(state: &WorkState) -> Result<(), WorkContextError> {
    if state.paths.is_empty() {
        return Err(WorkContextError::NoPaths);
    }
    Ok(())
}

fn inspect_status(status: &str, work_paths: &[String]) -> Result<StatusInspection, StatusError> {
    let entries = status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(StatusEntry::parse)
        .collect::<Vec<_>>();
    let mut commit_paths = BTreeSet::new();
    let mut outside_work = Vec::new();
    let mut local_state = Vec::new();
    let mut staged_local_state = Vec::new();

    for entry in entries {
        if entry.paths.iter().any(|path| is_local_state_path(path)) {
            local_state.extend(entry.paths.iter().cloned());
            if entry.is_staged() {
                staged_local_state.extend(entry.paths.iter().cloned());
            }
            continue;
        }

        if entry
            .paths
            .iter()
            .any(|path| path_is_in_work(path, work_paths))
        {
            commit_paths.extend(entry.paths);
        } else {
            outside_work.extend(entry.paths);
        }
    }

    if !staged_local_state.is_empty() {
        return Err(StatusError::StagedLocalState {
            paths: staged_local_state,
        });
    }
    if !outside_work.is_empty() {
        return Err(StatusError::OutsideWork {
            paths: outside_work,
        });
    }
    if commit_paths.is_empty() {
        return Err(StatusError::NoWorkDiff {
            ignored_local_state: local_state,
        });
    }

    Ok(StatusInspection {
        commit_paths: commit_paths.into_iter().collect(),
        ignored_local_state: local_state,
    })
}

#[derive(Debug, Clone)]
struct StatusEntry {
    index_status: char,
    paths: Vec<String>,
}

impl StatusEntry {
    fn parse(line: &str) -> Self {
        let index_status = line.chars().next().unwrap_or(' ');
        let body = line.get(3..).unwrap_or("").trim();
        let paths = body
            .split(" -> ")
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        Self {
            index_status,
            paths,
        }
    }

    fn is_staged(&self) -> bool {
        self.index_status != ' ' && self.index_status != '?'
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusInspection {
    commit_paths: Vec<String>,
    ignored_local_state: Vec<String>,
}

enum WorkContextError {
    NoPaths,
}

impl fmt::Debug for WorkContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkContextError")
            .field("kind", &"no_paths")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
enum StatusError {
    NoWorkDiff { ignored_local_state: Vec<String> },
    OutsideWork { paths: Vec<String> },
    StagedLocalState { paths: Vec<String> },
}

impl fmt::Debug for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, path_count) = match self {
            Self::NoWorkDiff {
                ignored_local_state,
            } => ("no_work_diff", ignored_local_state.len()),
            Self::OutsideWork { paths } => ("outside_work", paths.len()),
            Self::StagedLocalState { paths } => ("staged_local_state", paths.len()),
        };
        f.debug_struct("StatusError")
            .field("kind", &kind)
            .field("path_count", &path_count)
            .finish()
    }
}

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWorkDiff { .. } => f.write_str("No active-work changes found to integrate."),
            Self::OutsideWork { paths } => {
                write!(
                    f,
                    "Git status includes changes outside active work: {}.",
                    paths.join(", ")
                )
            }
            Self::StagedLocalState { paths } => {
                write!(
                    f,
                    "Local Rapport state is already staged: {}.",
                    paths.join(", ")
                )
            }
        }
    }
}

impl Error for StatusError {}

enum IntegrationStepError {
    CommandFailed {
        command: String,
        outcome: CommandOutcome,
    },
    Invoke {
        command: String,
        source: io::Error,
    },
    InvalidOutput {
        command: String,
        message: String,
    },
}

impl fmt::Debug for IntegrationStepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::CommandFailed { .. } => "command_failed",
            Self::Invoke { .. } => "invoke",
            Self::InvalidOutput { .. } => "invalid_output",
        };
        f.debug_struct("IntegrationStepError")
            .field("kind", &kind)
            .finish()
    }
}

impl fmt::Display for IntegrationStepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed { command, outcome } => write!(
                f,
                "`{command}` failed (stdout {} bytes, stderr {} bytes)",
                outcome.stdout.len(),
                outcome.stderr.len()
            ),
            Self::Invoke { command, source } => {
                write!(f, "could not run `{command}` ({:?})", source.kind())
            }
            Self::InvalidOutput { command, message } => {
                write!(
                    f,
                    "`{command}` returned unexpected output ({} bytes)",
                    message.len()
                )
            }
        }
    }
}

impl Error for IntegrationStepError {}

enum SignoffError {
    Contract {
        problems: Vec<String>,
    },
    Context(project_context::ProjectContextError),
    Execution {
        summary: &'static str,
        detail: String,
    },
}

impl SignoffError {
    fn execution(summary: &'static str, detail: impl Into<String>) -> Self {
        Self::Execution {
            summary,
            detail: detail.into(),
        }
    }
}

impl fmt::Debug for SignoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Contract { .. } => "contract",
            Self::Context(_) => "context",
            Self::Execution { .. } => "execution",
        };
        f.debug_struct("SignoffError").field("kind", &kind).finish()
    }
}

impl fmt::Display for SignoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract { problems } => write!(
                f,
                "invalid signoff contract ({} problem(s))",
                problems.len()
            ),
            Self::Context(source) => write!(
                f,
                "could not resolve signoffs from project context (detail {} bytes)",
                source.to_string().len()
            ),
            Self::Execution { summary, detail } => {
                write!(f, "{summary} (detail {} bytes)", detail.len())
            }
        }
    }
}

impl Error for SignoffError {}

struct ResolvedSignoffs {
    requirements: Vec<SignoffRequirement>,
    report: SignoffReport,
}

#[derive(Deserialize)]
struct PullRequestCandidate {
    url: String,
}

#[derive(Deserialize)]
struct PullRequestInfo {
    #[serde(rename = "baseRefOid")]
    base_ref_oid: String,
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "isCrossRepository")]
    is_cross_repository: bool,
    state: String,
    url: String,
}

struct ValidatedPullRequest {
    repository: String,
    base_ref_oid: String,
    merge_base_sha: String,
}

#[derive(Deserialize)]
struct CombinedStatus {
    #[serde(default)]
    total_count: Option<usize>,
    statuses: Vec<CommitStatus>,
}

#[derive(Deserialize)]
struct CommitStatus {
    context: String,
    state: String,
}

#[derive(Default)]
struct SignoffReport {
    required: Vec<String>,
    passed: Vec<String>,
    failed: Vec<String>,
    pending: Vec<String>,
}

impl SignoffReport {
    fn from_required(required: Vec<String>) -> Self {
        Self {
            pending: required.clone(),
            required,
            ..Self::default()
        }
    }

    fn from_states(requirements: &[SignoffRequirement], states: &BTreeMap<String, String>) -> Self {
        let mut report = Self::default();
        for requirement in requirements {
            let request = &requirement.request;
            let target = request.qualified_target().to_string();
            let context = format!("signoff: {target}");
            report.required.push(target.clone());
            match states.get(&context).map(String::as_str) {
                Some("success") => report.passed.push(target),
                Some("failure" | "error") => report.failed.push(target),
                _ => report.pending.push(target),
            }
        }
        report
    }

    fn status(&self) -> &'static str {
        if !self.failed.is_empty() {
            "fail"
        } else if !self.pending.is_empty() {
            "pending"
        } else if self.required.is_empty() {
            "none"
        } else {
            "pass"
        }
    }

    fn fact(&self, timestamp: &str) -> WorkFact {
        let mut fact = WorkFact::new(self.status()).at(timestamp);
        fact.summary = Some(self.summary());
        fact.required.clone_from(&self.required);
        fact.passed.clone_from(&self.passed);
        fact.failed.clone_from(&self.failed);
        fact.pending.clone_from(&self.pending);
        fact
    }

    fn summary(&self) -> String {
        match self.status() {
            "none" => String::from("no signoffs configured"),
            "pass" => format!("{} required signoff(s) passed", self.required.len()),
            "pending" => format!("{} signoff(s) pending", self.pending.len()),
            "fail" => format!("{} signoff(s) failed", self.failed.len()),
            _ => String::from("unknown signoff state"),
        }
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

fn path_is_in_work(path: &str, work_paths: &[String]) -> bool {
    work_paths.iter().any(|work_path| {
        work_path == "."
            || path == work_path
            || path
                .strip_prefix(work_path)
                .is_some_and(|remaining| remaining.starts_with('/'))
    })
}

fn is_local_state_path(path: &str) -> bool {
    path == ".rapport" || path.starts_with(".rapport/")
}

fn present_or_unknown(value: &str) -> String {
    if value.is_empty() {
        String::from("unknown")
    } else {
        value.to_string()
    }
}

fn parse_pr_url(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("http://") || line.starts_with("https://"))
        .map(ToString::to_string)
}

fn pr_body(state: &WorkState, message: &str, commit: &str, signoffs: &[String]) -> String {
    let mut body = vec![
        message.to_string(),
        String::new(),
        String::from("## Rapport"),
        format!("- Work: {}", state.title),
    ];
    if let Some(ticket) = &state.ticket {
        body.push(format!("- Ticket: {ticket}"));
    }
    if let Some(objective) = &state.objective {
        body.push(format!("- Objective: {objective}"));
    }
    body.push(format!("- Paths: {}", state.paths.join(", ")));
    if let Some(build) = &state.build {
        body.push(format!("- Build: {}", build.status));
    }
    if !signoffs.is_empty() {
        body.push(format!(
            "- Signoffs: {}",
            signoffs
                .iter()
                .map(|target| format!("`signoff: {target}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    body.push(format!("- Commit: {commit}"));
    body.join("\n")
}

fn integration_fact(
    status: &str,
    timestamp: &str,
    summary: &str,
    branch: &str,
    commit: &str,
    pr_url: &str,
) -> WorkFact {
    let mut fact = WorkFact::new(status)
        .at(timestamp)
        .summary(summary.to_string());
    fact.branch = Some(branch.to_string());
    fact.commit = Some(commit.to_string());
    fact.pr_url = Some(pr_url.to_string());
    fact
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

fn record_best_effort<F, C, O, E>(
    command: &'static str,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
    result: CommandResult,
) where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let _ = record_event(command, arguments, context, result);
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

fn captured_output(outcome: &CommandOutcome) -> String {
    format!(
        "stdout: {} bytes\nstderr: {} bytes",
        outcome.stdout.len(),
        outcome.stderr.len()
    )
}

fn render_missing_work() -> String {
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph("No active work state found.")
        .paragraph("Start work before integrating.")
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

fn render_missing_summary_or_message() -> String {
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph("Integration needs both a summary and a message.")
        .next_actions(nonempty![RunHint::new(
            "rapport integrate --summary \"...\" --message \"...\""
        )])
        .build()
}

fn render_work_context_error(error: &WorkContextError) -> String {
    let WorkContextError::NoPaths = error;
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph("Active work has no paths to integrate.")
        .next_actions(nonempty![RunHint::new("rapport work add path <path>")])
        .build()
}

fn render_status_error(error: &StatusError) -> String {
    let next = match error {
        StatusError::NoWorkDiff { .. } => "make changes, then run rapport build",
        StatusError::OutsideWork { .. } => "rapport work add path <path>",
        StatusError::StagedLocalState { .. } => "git restore --staged .rapport",
    };
    let mut builder = ViewBuilder::new()
        .title("rapport integrate")
        .paragraph("Could not safely select changes to commit.")
        .paragraph(error);
    if let StatusError::NoWorkDiff {
        ignored_local_state,
    } = error
        && !ignored_local_state.is_empty()
    {
        builder = builder.section("Ignored Local State", |b| b.items(ignored_local_state));
    }
    builder.next_actions(nonempty![RunHint::new(next)]).build()
}

fn render_commit_error(error: &IntegrationStepError) -> String {
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph("Could not create the integration commit.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new(
            "fix Git state, then run rapport integrate"
        )])
        .build()
}

fn render_pr_error(error: &IntegrationStepError) -> String {
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph("Could not create a GitHub pull request.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new(
            "fix GitHub auth or branch publish state, then run rapport integrate"
        )])
        .build()
}

fn render_signoff_error(error: &SignoffError) -> String {
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph("Could not evaluate signoff requirements.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new(
            "fix signoffs in the applicable context.toml, then run rapport integrate"
        )])
        .build()
}

fn render_command_failure(command: &str, outcome: &CommandOutcome) -> String {
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph(format!("`{command}` failed."))
        .section("Output", |b| b.captured(captured_output(outcome)))
        .next_actions(nonempty![RunHint::new(
            "fix Git state, then run rapport integrate"
        )])
        .build()
}

fn render_command_invoke_error(command: &str, error: &io::Error) -> String {
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph(format!("Could not run `{command}`."))
        .paragraph(format!("Invocation failed ({:?}).", error.kind()))
        .next_actions(nonempty![RunHint::new(
            "install Git/GitHub CLI, then run rapport integrate"
        )])
        .build()
}

fn render_integrated(
    summary: &str,
    branch: &str,
    commit: &str,
    pr_url: &str,
    signoffs: &SignoffReport,
) -> String {
    let next = if signoffs.pending.is_empty() {
        format!("gh pr view {pr_url}")
    } else {
        String::from("complete pending signoffs")
    };
    ViewBuilder::new()
        .title("rapport integrate")
        .section("Integration", |b| {
            b.entries([
                ("status", String::from("pr_created")),
                ("summary", summary.to_string()),
                ("branch", branch.to_string()),
                ("commit", commit.to_string()),
                ("pr", pr_url.to_string()),
            ])
        })
        .section("Signoffs", |b| b.items(render_signoff_lines(signoffs)))
        .next_actions(nonempty![RunHint::new(next)])
        .build()
}

fn render_integrated_with_failed_signoffs(
    summary: &str,
    branch: &str,
    commit: &str,
    pr_url: &str,
    signoffs: &SignoffReport,
) -> String {
    ViewBuilder::new()
        .title("rapport integrate")
        .section("Integration", |b| {
            b.entries([
                ("status", String::from("pr_created")),
                ("summary", summary.to_string()),
                ("branch", branch.to_string()),
                ("commit", commit.to_string()),
                ("pr", pr_url.to_string()),
            ])
        })
        .section("Signoffs", |b| b.items(render_signoff_lines(signoffs)))
        .next_actions(nonempty![RunHint::new(
            "fix failed signoffs, then update the PR"
        )])
        .build()
}

fn render_signoff_lines(signoffs: &SignoffReport) -> Vec<String> {
    let mut lines = vec![format!("status `{}`", signoffs.status())];
    if signoffs.required.is_empty() {
        lines.push(String::from("no signoffs configured"));
    }
    lines.extend(signoffs.passed.iter().map(|id| format!("passed `{id}`")));
    lines.extend(signoffs.failed.iter().map(|id| format!("failed `{id}`")));
    lines.extend(signoffs.pending.iter().map(|id| format!("pending `{id}`")));
    lines
}

fn render_state_error(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport integrate")
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
    fn inspect_status_rejects_staged_local_state() {
        let error = match inspect_status("A  .rapport/work.toml\n M src/lib.rs\n", &["src".into()])
        {
            Ok(inspection) => panic!("expected staged local state error, got {inspection:?}"),
            Err(error) => error,
        };

        assert_eq!(
            error,
            StatusError::StagedLocalState {
                paths: vec![String::from(".rapport/work.toml")]
            }
        );
    }

    #[test]
    fn inspect_status_ignores_unstaged_local_state() {
        let inspection =
            match inspect_status(" M .rapport/work.toml\n M src/lib.rs\n", &["src".into()]) {
                Ok(inspection) => inspection,
                Err(error) => panic!("expected status inspection, got {error}"),
            };

        assert_eq!(
            inspection,
            StatusInspection {
                commit_paths: vec![String::from("src/lib.rs")],
                ignored_local_state: vec![String::from(".rapport/work.toml")]
            }
        );
    }

    #[test]
    fn status_reconciliation_rejects_incomplete_page() {
        let combined = CombinedStatus {
            total_count: Some(101),
            statuses: Vec::new(),
        };

        let error = match ensure_complete_status_page(&combined) {
            Ok(()) => panic!("expected incomplete status page to fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("more than 100 status contexts"));
    }

    #[test]
    fn integration_diagnostics_redact_command_output_and_signoff_details() {
        let outcome = CommandOutcome {
            success: false,
            stdout: String::from("PRIVATE STDOUT"),
            stderr: String::from("PRIVATE STDERR"),
        };
        let step = IntegrationStepError::CommandFailed {
            command: String::from("git command"),
            outcome: outcome.clone(),
        };
        let signoff = SignoffError::execution(
            "signoff command failed",
            String::from("PRIVATE SIGNOFF DETAIL"),
        );
        let contract = SignoffError::Contract {
            problems: vec![String::from("PRIVATE CONTRACT PROBLEM")],
        };

        let diagnostics = format!(
            "{step:?} {step} {signoff:?} {signoff} {contract:?} {contract} {}",
            captured_output(&outcome)
        );

        assert!(!diagnostics.contains("PRIVATE"));
        assert!(diagnostics.contains("stdout 14 bytes"));
        assert!(diagnostics.contains("detail 22 bytes"));
        assert!(diagnostics.contains("1 problem(s)"));
    }
}

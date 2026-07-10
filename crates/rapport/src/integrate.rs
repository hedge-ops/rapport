use crate::cli::IntegrateArgs;
use crate::context::{Clock, CommandContext};
use crate::project_context;
use crate::runner::{CommandOutcome, CommandSpec};
use crate::state::{WorkFact, WorkState, WorkStateError, WorkStateStore};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::FileSystem;
use std::collections::BTreeSet;
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
    let Some(request) = IntegrationRequest::from_args(integrate_args) else {
        let _ = writeln!(context.err, "{}", render_missing_summary_or_message());
        return CommandResult::failure();
    };

    if let Err(error) = validate_work_context(&state) {
        let _ = writeln!(context.err, "{}", render_work_context_error(&error));
        return CommandResult::failure();
    }

    let signoff_report = match evaluate_signoffs(arguments, context, &state.paths) {
        Ok(report) => report,
        Err(result) => return result,
    };
    record_best_effort(
        "integrate signoff",
        arguments.to_owned(),
        context,
        CommandResult::success(),
    );

    if let Err(error) = record_event(
        "integrate start",
        arguments.to_owned(),
        context,
        CommandResult::success(),
    ) {
        let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
        return CommandResult::failure();
    }

    let inspection = match inspect_active_changes(arguments, context, &state) {
        Ok(inspection) => inspection,
        Err(result) => return result,
    };
    let branch = match current_branch(context) {
        Ok(branch) => branch,
        Err(result) => return result,
    };

    let commit = match commit_active_changes(
        arguments,
        context,
        store,
        &mut state,
        request,
        &branch,
        &inspection.commit_paths,
    ) {
        Ok(commit) => commit,
        Err(result) => return result,
    };

    let pr_body = pr_body(&state, request.message, &commit, &signoff_report.required);
    let pr_url = match open_pull_request(
        arguments,
        context,
        store,
        &mut state,
        CreatedIntegration {
            request,
            branch: &branch,
            commit: &commit,
        },
        &pr_body,
    ) {
        Ok(pr_url) => pr_url,
        Err(result) => return result,
    };

    save_integration_result(
        context,
        store,
        &mut state,
        IntegrationOutput {
            request,
            branch: &branch,
            commit: &commit,
            pr_url: &pr_url,
            signoffs: &signoff_report,
        },
    )
}

#[derive(Debug, Clone, Copy)]
struct IntegrationRequest<'request> {
    summary: &'request str,
    message: &'request str,
}

impl<'request> IntegrationRequest<'request> {
    fn from_args(args: &'request IntegrateArgs) -> Option<Self> {
        let summary = args.summary.trim();
        let message = args.message.trim();
        if summary.is_empty() || message.is_empty() {
            None
        } else {
            Some(Self { summary, message })
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct IntegrationOutput<'output> {
    request: IntegrationRequest<'output>,
    branch: &'output str,
    commit: &'output str,
    pr_url: &'output str,
    signoffs: &'output SignoffReport,
}

#[derive(Debug, Clone, Copy)]
struct CreatedIntegration<'created> {
    request: IntegrationRequest<'created>,
    branch: &'created str,
    commit: &'created str,
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

fn commit_active_changes<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    request: IntegrationRequest<'_>,
    branch: &str,
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
            record_failed_integration(
                context,
                store,
                state,
                FailedIntegration::new("commit_failed", request.summary).branch(branch),
            );
            let _ = writeln!(context.err, "{}", render_commit_error(&error));
            Err(CommandResult::failure())
        }
    }
}

fn open_pull_request<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    integration: CreatedIntegration<'_>,
    pr_body: &str,
) -> Result<String, CommandResult>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match create_pr(context, integration.request.summary, pr_body) {
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
            record_failed_integration(
                context,
                store,
                state,
                FailedIntegration::new("pr_failed", integration.request.summary)
                    .branch(integration.branch)
                    .commit(integration.commit),
            );
            let _ = writeln!(context.err, "{}", render_pr_error(&error));
            Err(CommandResult::failure())
        }
    }
}

fn evaluate_signoffs<F, C, O, E>(
    arguments: &[String],
    context: &mut CommandContext<'_, F, C, O, E>,
    work_paths: &[String],
) -> Result<SignoffReport, CommandResult>
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

fn save_integration_result<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    output: IntegrationOutput<'_>,
) -> CommandResult
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
        output.request.summary,
        output.branch,
        output.commit,
        output.pr_url,
    ));
    state.signoff = Some(output.signoffs.fact(&now));
    state.updated_at = now;

    match store.save(context.fs, state) {
        Ok(()) if output.signoffs.failed.is_empty() => {
            let _ = writeln!(
                context.out,
                "{}",
                render_integrated(
                    output.request.summary,
                    output.branch,
                    output.commit,
                    output.pr_url,
                    output.signoffs,
                )
            );
            CommandResult::success()
        }
        Ok(()) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_integrated_with_failed_signoffs(
                    output.request.summary,
                    output.branch,
                    output.commit,
                    output.pr_url,
                    output.signoffs,
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

fn create_pr<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    summary: &str,
    body: &str,
) -> Result<String, IntegrationStepError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let outcome = run_success(
        context,
        &CommandSpec::new("gh", ["pr", "create", "--title", summary, "--body", body]),
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
) -> Result<SignoffReport, SignoffError>
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
    let required = project_context::required_signoffs_for_paths(
        context.fs,
        context.paths.repo_root(),
        work_paths,
    )
    .map_err(SignoffError::Context)?;
    Ok(SignoffReport::from_required(required))
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
    match &state.build {
        Some(build) if build.status == "pass" => Ok(()),
        Some(build) => Err(WorkContextError::BuildNotPassing {
            status: build.status.clone(),
        }),
        None => Err(WorkContextError::MissingBuild),
    }
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

#[derive(Debug)]
enum WorkContextError {
    NoPaths,
    MissingBuild,
    BuildNotPassing { status: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StatusError {
    NoWorkDiff { ignored_local_state: Vec<String> },
    OutsideWork { paths: Vec<String> },
    StagedLocalState { paths: Vec<String> },
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

#[derive(Debug)]
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

impl fmt::Display for IntegrationStepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed { command, outcome } => {
                write!(f, "`{command}` failed: {}", captured_output(outcome))
            }
            Self::Invoke { command, source } => write!(f, "could not run `{command}`: {source}"),
            Self::InvalidOutput { command, message } => {
                write!(f, "`{command}` returned unexpected output: {message}")
            }
        }
    }
}

impl Error for IntegrationStepError {}

#[derive(Debug)]
enum SignoffError {
    Contract { problems: Vec<String> },
    Context(project_context::ProjectContextError),
}

impl fmt::Display for SignoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract { problems } => {
                write!(f, "invalid signoff contract: {}", problems.join("; "))
            }
            Self::Context(source) => write!(
                f,
                "could not resolve signoffs from project context: {source}"
            ),
        }
    }
}

impl Error for SignoffError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Contract { .. } => None,
            Self::Context(source) => Some(source),
        }
    }
}

#[derive(Debug, Default)]
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

fn failed_integration_fact(
    status: &str,
    timestamp: &str,
    summary: &str,
    branch: Option<&str>,
    commit: Option<&str>,
    pr_url: Option<&str>,
) -> WorkFact {
    let mut fact = WorkFact::new(status)
        .at(timestamp)
        .summary(summary.to_string());
    fact.branch = branch.map(ToString::to_string);
    fact.commit = commit.map(ToString::to_string);
    fact.pr_url = pr_url.map(ToString::to_string);
    fact
}

#[derive(Debug, Clone, Copy)]
struct FailedIntegration<'failure> {
    status: &'failure str,
    summary: &'failure str,
    branch: Option<&'failure str>,
    commit: Option<&'failure str>,
    pr_url: Option<&'failure str>,
}

impl<'failure> FailedIntegration<'failure> {
    fn new(status: &'failure str, summary: &'failure str) -> Self {
        Self {
            status,
            summary,
            branch: None,
            commit: None,
            pr_url: None,
        }
    }

    fn branch(mut self, branch: &'failure str) -> Self {
        self.branch = Some(branch);
        self
    }

    fn commit(mut self, commit: &'failure str) -> Self {
        self.commit = Some(commit);
        self
    }
}

fn record_failed_integration<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
    failure: FailedIntegration<'_>,
) where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let now = context.clock.now_rfc3339();
    state.integrate = Some(failed_integration_fact(
        failure.status,
        &now,
        failure.summary,
        failure.branch,
        failure.commit,
        failure.pr_url,
    ));
    state.updated_at = now;
    let _ = store.save(context.fs, state);
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
    let mut parts = Vec::new();
    if !outcome.stdout.trim().is_empty() {
        parts.push(format!("stdout:\n{}", outcome.stdout.trim()));
    }
    if !outcome.stderr.trim().is_empty() {
        parts.push(format!("stderr:\n{}", outcome.stderr.trim()));
    }
    if parts.is_empty() {
        String::from("no output")
    } else {
        parts.join("\n\n")
    }
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
    let next = match error {
        WorkContextError::NoPaths => "rapport work add path <path>",
        WorkContextError::MissingBuild | WorkContextError::BuildNotPassing { .. } => {
            "rapport build"
        }
    };
    let message = match error {
        WorkContextError::NoPaths => String::from("Active work has no paths to integrate."),
        WorkContextError::MissingBuild => {
            String::from("Active work has not passed build validation yet.")
        }
        WorkContextError::BuildNotPassing { status } => {
            format!("Active work build status is `{status}`, not `pass`.")
        }
    };
    ViewBuilder::new()
        .title("rapport integrate")
        .paragraph(message)
        .next_actions(nonempty![RunHint::new(next)])
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
        .paragraph(error)
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
}

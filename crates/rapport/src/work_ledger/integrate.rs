//! Pull-request integration for an accepted Work candidate.
//!
//! Owns the durable Integration Task and GitHub side-effect reconciliation.
//! Candidate creation remains owned by Develop, Build, and Review.

use super::Error;
use super::build;
use super::command;
use super::develop;
use super::domain::{
    IntegrationStage, IntegrationTask, PublishedBuildStatus, ReviewMode, Task, TaskStatus, Work,
    WorkOutcomeKind, Workflow,
};
use super::history::HistoryStore;
use super::repository::Store;
use crate::{Clock, CommandContext, CommandSpec};
use clap::{Args, Subcommand};
use rapport_files::{FileSystem, Utf8PathBuf};
use rapport_git::{BranchName, Git, ObjectId, Repository};
use serde::Deserialize;
use std::io::Write;
use std::process::ExitCode;

const BUILD_AGGREGATE: &str = "Rapport Build";
const PULL_REQUEST_FIELDS: &str = "number,url,body,headRefOid,headRefName,baseRefOid,baseRefName,isCrossRepository,state,mergeable,mergeStateStatus,reviewDecision,reviewRequests,statusCheckRollup,mergeCommit";

#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Publish exact accepted Work and create its pull request.
    Start,
    /// Inspect local and GitHub integration state without changing it.
    Status,
    /// Close the owned pull request and preserve active Work.
    Cancel {
        #[arg(long)]
        reason: String,
    },
    /// Revalidate and squash-merge the owned pull request.
    Complete,
}

pub(crate) fn run<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result = match &cli.action {
        Action::Start => start(context),
        Action::Status => status(context),
        Action::Cancel { reason } => cancel(context, reason),
        Action::Complete => complete(context),
    };
    match result {
        Ok(output) => {
            let _ = writeln!(context.out, "{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "{error}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug)]
struct AcceptedCandidate {
    candidate: String,
    policy_digest: String,
    build_task: String,
    review_task: String,
}

fn accepted_candidate<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    work: &Work,
    tasks: &[Task],
    git: &Git,
    repository: &Repository,
) -> Result<AcceptedCandidate, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if git.operation(repository)?.is_some() {
        return Err(Error::SourceOperationActive);
    }
    let live = git.status(repository)?;
    if live.branch() != Some(&work.source_branch) {
        return Err(Error::SourceBranchChanged {
            expected: work.source_branch.as_str().to_owned(),
            actual: live
                .branch()
                .map_or("detached HEAD", BranchName::as_str)
                .to_owned(),
        });
    }
    if !live.is_clean() {
        return Err(Error::DirtyWorktree);
    }
    let checkpoint = work
        .latest_checkpoint
        .as_ref()
        .unwrap_or(&work.starting_source);
    if checkpoint != live.head() {
        return Err(Error::UncheckpointedHead);
    }
    if !develop::is_complete(work, tasks, &live, None)
        || tasks
            .iter()
            .any(|task| task.workflow != Workflow::Integrate && !task.status.is_terminal())
    {
        return Err(Error::IntegrationPrerequisite);
    }
    let (target, _) = command::target_revision(git, repository, &work.target_branch)?;
    let changes = git.source_side_changes(repository, &target)?;
    let changed_paths = changes.paths().iter().cloned().collect::<Vec<_>>();
    let policy_digest = crate::policy_context::effective_policy_digest_for_paths(
        context.fs,
        &context.repo_root,
        changed_paths.iter().map(Utf8PathBuf::as_path),
    )?;
    let required = crate::policy_context::required_signoffs_for_paths(
        context.fs,
        &context.repo_root,
        changed_paths.iter().map(Utf8PathBuf::as_path),
    )?;
    if !build::current_proof(tasks, live.head().as_str(), &policy_digest, &required) {
        return Err(Error::BuildIncomplete);
    }
    let build_task = tasks
        .iter()
        .rev()
        .find(|task| {
            task.status == TaskStatus::Passed
                && task.build.as_ref().is_some_and(|build| {
                    build.proof
                        && build.candidate == live.head().as_str()
                        && build.policy_digest.as_deref() == Some(policy_digest.as_str())
                })
        })
        .map(|task| task.id.clone())
        .ok_or(Error::BuildIncomplete)?;
    let review_task = tasks
        .iter()
        .rev()
        .find(|task| {
            task.status == TaskStatus::Passed
                && task.review.as_ref().is_some_and(|review| {
                    review.mode == ReviewMode::Acceptance
                        && review.proof
                        && review.candidate == live.head().as_str()
                        && review.policy_digest == policy_digest
                })
        })
        .map(|task| task.id.clone())
        .ok_or(Error::ReviewIncomplete)?;
    Ok(AcceptedCandidate {
        candidate: live.head().as_str().to_owned(),
        policy_digest,
        build_task,
        review_task,
    })
}

fn start<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let (git, repository) = git_repository(&context.repo_root)?;
    let accepted = accepted_candidate(context, &work, &tasks, &git, &repository)?;
    let repository_name = github_repository(context)?;

    if let Some(task) = current_integration(&tasks) {
        let integration = task.integration.as_ref().ok_or(Error::MissingIntegration)?;
        if integration.candidate != accepted.candidate
            || integration.policy_digest != accepted.policy_digest
        {
            return Err(Error::StaleIntegration);
        }
        return continue_start(
            context,
            &store,
            &work,
            task.clone(),
            &tasks,
            &git,
            &repository,
            &repository_name,
        );
    }

    if !open_pull_requests(context, work.source_branch.as_str())?.is_empty() {
        return Err(Error::ActiveIntegration);
    }
    let target_branch = work.target_branch.as_str();
    let target_commit = github_target_commit(context, &repository_name, target_branch)?;
    let review = tasks
        .iter()
        .find(|task| task.id == accepted.review_task)
        .and_then(|task| task.review.as_ref())
        .ok_or(Error::ReviewIncomplete)?;
    let task_id = work.allocate_task_id()?;
    let review_grade = review
        .result
        .as_ref()
        .map(|result| result.overall_grade.to_string())
        .ok_or(Error::ReviewIncomplete)?;
    let review_findings = review
        .findings
        .iter()
        .filter_map(|finding| finding.id.clone())
        .collect();
    let mut task = Task::new(
        task_id,
        "integration",
        Workflow::Integrate,
        "Integrate accepted Work",
        "Publish and merge the exact candidate through its owned pull request.",
        "rapport integrate start",
        TaskStatus::Running,
        &accepted.candidate,
        context.clock.now_rfc3339(),
        Some("rapport integrate status".to_owned()),
    );
    task.integration = Some(IntegrationTask {
        stage: IntegrationStage::Preparing,
        repository: Some(repository_name.clone()),
        source_branch: work.source_branch.as_str().to_owned(),
        target_branch: work.target_branch.as_str().to_owned(),
        candidate: accepted.candidate,
        target_commit,
        policy_digest: accepted.policy_digest,
        build_task: accepted.build_task,
        review_task: accepted.review_task,
        review_grade,
        quality_override: review.quality_override.clone(),
        review_findings,
        pushed: false,
        published_builds: Vec::new(),
        aggregate_build_published: false,
        pull_request_number: None,
        pull_request_url: None,
        pull_request_head: None,
        pull_request_base: None,
        pull_request_closed: false,
        remote_branch_deleted: false,
        merge_commit: None,
        cancellation_reason: None,
    });
    store.save_work_and_task(context.fs, &work, &task)?;
    continue_start(
        context,
        &store,
        &work,
        task,
        &tasks,
        &git,
        &repository,
        &repository_name,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "resumption carries the complete durable and observed integration boundary"
)]
fn continue_start<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &Store,
    work: &Work,
    mut task: Task,
    tasks: &[Task],
    git: &Git,
    repository: &Repository,
    repository_name: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if !task.integration.as_ref().is_some_and(|state| state.pushed) {
        let branch = task
            .integration
            .as_ref()
            .ok_or(Error::MissingIntegration)?
            .source_branch
            .clone();
        let branch = BranchName::new(branch)?;
        git.push_branch(repository, &branch)?;
        task.integration
            .as_mut()
            .ok_or(Error::MissingIntegration)?
            .pushed = true;
        store.save_task(context.fs, &task)?;
    }

    publish_build_statuses(context, store, work, &mut task, tasks, repository_name)?;

    let integration = task.integration.as_ref().ok_or(Error::MissingIntegration)?;
    let pull_request = if let Some(number) = integration.pull_request_number {
        pull_request(context, &number.to_string())?
    } else {
        let mut open = open_pull_requests(context, &integration.source_branch)?;
        if open.len() > 1 {
            return Err(Error::IntegrationOwnership);
        }
        if let Some(existing) = open.pop() {
            verify_pull_request(work, integration, &existing)?;
            existing
        } else {
            let body = pull_request_body(work, tasks, integration)?;
            let url = run_gh(
                context,
                [
                    "pr",
                    "create",
                    "--base",
                    &integration.target_branch,
                    "--head",
                    &integration.source_branch,
                    "--title",
                    &work.title,
                    "--body",
                    &body,
                ],
            )?;
            pull_request(context, url.trim())?
        }
    };
    verify_pull_request(
        work,
        task.integration.as_ref().ok_or(Error::MissingIntegration)?,
        &pull_request,
    )?;
    let integration = task.integration.as_mut().ok_or(Error::MissingIntegration)?;
    integration.pull_request_number = Some(pull_request.number);
    integration.pull_request_url = Some(pull_request.url.clone());
    integration.pull_request_head = Some(pull_request.head_ref_oid.clone());
    integration.pull_request_base = Some(pull_request.base_ref_oid.clone());
    integration.stage = IntegrationStage::Published;
    task.continuation = Some("rapport integrate status".to_owned());
    store.save_task(context.fs, &task)?;
    let blockers = integration_blockers(
        work,
        task.integration.as_ref().ok_or(Error::MissingIntegration)?,
        &pull_request,
    );
    render_status(&task, &pull_request, blockers)
}

fn publish_build_statuses<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &Store,
    work: &Work,
    task: &mut Task,
    tasks: &[Task],
    repository: &str,
) -> Result<(), Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let integration = task.integration.as_ref().ok_or(Error::MissingIntegration)?;
    let candidate = integration.candidate.clone();
    let integration_build_task = integration.build_task.clone();
    let build_task = tasks
        .iter()
        .find(|candidate| candidate.id == integration_build_task)
        .ok_or(Error::BuildIncomplete)?;
    let build = build_task.build.as_ref().ok_or(Error::BuildIncomplete)?;
    for operation in &build.operations {
        let Some(identity) = &operation.identity else {
            continue;
        };
        let digest = operation
            .contract_digest
            .as_deref()
            .ok_or(Error::BuildIncomplete)?;
        let already_published = task
            .integration
            .as_ref()
            .ok_or(Error::MissingIntegration)?
            .published_builds
            .iter()
            .any(|published| published.identity == *identity);
        if already_published {
            continue;
        }
        publish_status(
            context,
            repository,
            &candidate,
            identity,
            &format!(
                "Build proof {} for Work {}",
                build_task.id,
                short_uuid(work)
            ),
        )?;
        task.integration
            .as_mut()
            .ok_or(Error::MissingIntegration)?
            .published_builds
            .push(PublishedBuildStatus {
                identity: identity.clone(),
                build_task: build_task.id.clone(),
                contract_digest: digest.to_owned(),
            });
        store.save_task(context.fs, task)?;
    }
    if !task
        .integration
        .as_ref()
        .is_some_and(|state| state.aggregate_build_published)
    {
        publish_status(
            context,
            repository,
            &candidate,
            BUILD_AGGREGATE,
            &format!(
                "Build proof {} for Work {}",
                build_task.id,
                short_uuid(work)
            ),
        )?;
        task.integration
            .as_mut()
            .ok_or(Error::MissingIntegration)?
            .aggregate_build_published = true;
        store.save_task(context.fs, task)?;
    }
    Ok(())
}

fn publish_status<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    repository: &str,
    candidate: &str,
    identity: &str,
    description: &str,
) -> Result<(), Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let endpoint = format!("repos/{repository}/statuses/{candidate}");
    run_gh(
        context,
        [
            "api",
            "--method",
            "POST",
            &endpoint,
            "-f",
            "state=success",
            "-f",
            &format!("context={identity}"),
            "-f",
            &format!("description={description}"),
        ],
    )?;
    Ok(())
}

fn status<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let task = current_integration(&tasks).ok_or(Error::MissingIntegration)?;
    let integration = task.integration.as_ref().ok_or(Error::MissingIntegration)?;
    let number = integration
        .pull_request_number
        .ok_or(Error::MissingIntegration)?;
    let pull_request = pull_request(context, &number.to_string())?;
    let mut blockers = integration_blockers(&work, integration, &pull_request);
    let (git, repository) = git_repository(&context.repo_root)?;
    if accepted_candidate(context, &work, &tasks, &git, &repository).is_err() {
        blockers.push("local Develop, Build, or Review proof is stale".to_owned());
    }
    render_status(task, &pull_request, blockers)
}

fn cancel<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    reason: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let reason = required(reason)?;
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let mut task = current_integration(&tasks)
        .cloned()
        .ok_or(Error::MissingIntegration)?;
    let number = task
        .integration
        .as_ref()
        .and_then(|state| state.pull_request_number)
        .ok_or(Error::MissingIntegration)?;
    let pull_request = pull_request(context, &number.to_string())?;
    verify_pull_request(
        &work,
        task.integration.as_ref().ok_or(Error::MissingIntegration)?,
        &pull_request,
    )?;
    if pull_request.state == "MERGED" {
        return Err(Error::IntegrationBlocked(
            "the pull request is already merged; run rapport integrate complete".to_owned(),
        ));
    }
    if !task
        .integration
        .as_ref()
        .is_some_and(|state| state.pull_request_closed)
    {
        if pull_request.state == "OPEN" {
            run_gh(
                context,
                [
                    "pr",
                    "close",
                    &number.to_string(),
                    "--comment",
                    &format!("Rapport cancelled integration: {reason}"),
                ],
            )?;
        }
        let integration = task.integration.as_mut().ok_or(Error::MissingIntegration)?;
        integration.stage = IntegrationStage::Cancelling;
        integration.pull_request_closed = true;
        integration.cancellation_reason = Some(reason.clone());
        store.save_task(context.fs, &task)?;
    }
    if !task
        .integration
        .as_ref()
        .is_some_and(|state| state.remote_branch_deleted)
    {
        let (git, repository) = git_repository(&context.repo_root)?;
        let source = task
            .integration
            .as_ref()
            .ok_or(Error::MissingIntegration)?
            .source_branch
            .clone();
        let source = BranchName::new(source)?;
        git.delete_remote_branch(&repository, &source)?;
        task.integration
            .as_mut()
            .ok_or(Error::MissingIntegration)?
            .remote_branch_deleted = true;
        store.save_task(context.fs, &task)?;
    }
    task.integration
        .as_mut()
        .ok_or(Error::MissingIntegration)?
        .stage = IntegrationStage::Cancelled;
    task.finish(
        TaskStatus::Cancelled,
        context.clock.now_rfc3339(),
        reason,
        None,
    );
    store.save_task(context.fs, &task)?;
    Ok(format!(
        "# rapport integrate cancel\n\n- `task` — {}\n- `pull request` — {}\n- `status` — cancelled\n- `remote branch deleted` — true\n- `local Work preserved` — true\n- `next` — `rapport work status`",
        task.id, pull_request.url
    ))
}

fn complete<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    if let Some(outcome) = &work.outcome {
        if outcome.kind != WorkOutcomeKind::Integrated {
            return Err(Error::FinalizedWork(outcome.kind.to_string()));
        }
        let archive =
            HistoryStore::new(&context.repo_root)?.archive(context.fs, &store, &work, &tasks)?;
        return Ok(format!(
            "# rapport integrate complete\n\n- `status` — passed\n- `target commit` — {}\n- `remote branch deleted` — preserved in Work History\n- `Work` — archived {}",
            outcome.target_commit, archive
        ));
    }
    let index = current_integration_index(&tasks).ok_or(Error::MissingIntegration)?;
    let mut task = tasks[index].clone();
    let integration = task.integration.as_ref().ok_or(Error::MissingIntegration)?;
    let number = integration
        .pull_request_number
        .ok_or(Error::MissingIntegration)?;
    let mut observed_pull_request = pull_request(context, &number.to_string())?;
    verify_pull_request(&work, integration, &observed_pull_request)?;

    if observed_pull_request.state != "MERGED" {
        if integration.stage == IntegrationStage::Merging {
            return Err(Error::IntegrationBlocked(
                "GitHub has not confirmed the submitted merge; run rapport integrate status and retry after it merges"
                    .to_owned(),
            ));
        }
        let (git, repository) = git_repository(&context.repo_root)?;
        accepted_candidate(context, &work, &tasks, &git, &repository)?;
        let blockers = integration_blockers(&work, integration, &observed_pull_request);
        if !blockers.is_empty() {
            return Err(Error::IntegrationBlocked(blockers.join("; ")));
        }
        task.integration
            .as_mut()
            .ok_or(Error::MissingIntegration)?
            .stage = IntegrationStage::Merging;
        store.save_task(context.fs, &task)?;
        run_gh(context, ["pr", "merge", &number.to_string(), "--squash"])?;
        observed_pull_request = pull_request(context, &number.to_string())?;
        if observed_pull_request.state != "MERGED" {
            return Err(Error::IntegrationBlocked(
                "GitHub accepted the merge operation but has not confirmed the merge; run status and complete again"
                    .to_owned(),
            ));
        }
    }
    let merge_commit = observed_pull_request
        .merge_commit
        .as_ref()
        .map(|commit| commit.oid.clone())
        .filter(|commit| !commit.is_empty())
        .ok_or_else(|| Error::IntegrationBlocked("GitHub omitted the merge commit".to_owned()))?;
    let integration = task.integration.as_mut().ok_or(Error::MissingIntegration)?;
    integration.stage = IntegrationStage::Merged;
    integration.merge_commit = Some(merge_commit.clone());
    if !integration.remote_branch_deleted {
        let (git, repository) = git_repository(&context.repo_root)?;
        let source = BranchName::new(integration.source_branch.clone())?;
        git.delete_remote_branch(&repository, &source)?;
        integration.remote_branch_deleted = true;
    }
    let final_source = ObjectId::new(integration.candidate.clone())?;
    let final_target = ObjectId::new(merge_commit.clone())?;
    let completed_at = context.clock.now_rfc3339();
    task.finish(
        TaskStatus::Passed,
        completed_at.clone(),
        format!("squash-merged as {merge_commit}"),
        Some(observed_pull_request.url.clone()),
    );
    work.finish(
        WorkOutcomeKind::Integrated,
        completed_at,
        format!(
            "squash-merged pull request #{} as {merge_commit}",
            observed_pull_request.number
        ),
        final_source,
        final_target,
    )?;
    tasks[index] = task.clone();
    store.save_work_and_task(context.fs, &work, &task)?;
    let archive =
        HistoryStore::new(&context.repo_root)?.archive(context.fs, &store, &work, &tasks)?;
    Ok(format!(
        "# rapport integrate complete\n\n- `task` — {}\n- `pull request` — {}\n- `status` — passed\n- `target commit` — {}\n- `remote branch deleted` — true\n- `Work` — archived {}",
        task.id, observed_pull_request.url, merge_commit, archive
    ))
}

fn integration_blockers(
    work: &Work,
    integration: &IntegrationTask,
    pull_request: &PullRequest,
) -> Vec<String> {
    let mut blockers = Vec::new();
    if pull_request.state != "OPEN" {
        blockers.push(format!(
            "pull request is {}",
            pull_request.state.to_lowercase()
        ));
    }
    if pull_request.head_ref_oid != integration.candidate {
        blockers.push("pull-request head changed".to_owned());
    }
    if pull_request.base_ref_name != work.target_branch.as_str() {
        blockers.push("pull-request target changed".to_owned());
    }
    let checks = check_state(&pull_request.status_check_rollup);
    if !checks.aggregate_passed {
        blockers.push(format!("{BUILD_AGGREGATE} is not passing"));
    }
    for published in &integration.published_builds {
        if !status_passed(&pull_request.status_check_rollup, &published.identity) {
            blockers.push(format!("{} is not passing", published.identity));
        }
    }
    if checks.failed > 0 {
        blockers.push(format!("{} remote check(s) failed", checks.failed));
    }
    if checks.pending > 0 {
        blockers.push(format!("{} remote check(s) pending", checks.pending));
    }
    if checks.observed_remote == 0 {
        blockers.push("no remote checks observed".to_owned());
    }
    match pull_request.review_decision.as_deref() {
        Some("CHANGES_REQUESTED") => blockers.push("changes are requested".to_owned()),
        Some("APPROVED" | "REVIEW_REQUIRED" | "") | None => {}
        Some(other) => blockers.push(format!("review decision is {other}")),
    }
    if pull_request.mergeable != "MERGEABLE" {
        blockers.push(format!(
            "GitHub mergeability is {}",
            pull_request.mergeable.to_lowercase()
        ));
    }
    if pull_request.merge_state_status == "DIRTY" {
        blockers.push(format!(
            "GitHub merge state is {}",
            pull_request.merge_state_status.to_lowercase()
        ));
    }
    blockers
}

#[derive(Debug, Default)]
struct CheckState {
    aggregate_passed: bool,
    observed_remote: usize,
    passed: usize,
    pending: usize,
    failed: usize,
}

fn check_state(checks: &[StatusCheck]) -> CheckState {
    let mut state = CheckState::default();
    for check in checks {
        if check.kind.as_deref() == Some("CheckRun") {
            state.observed_remote += 1;
        }
        let name = check
            .name
            .as_deref()
            .or(check.context.as_deref())
            .unwrap_or("");
        let result = check_result(check);
        match result {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => state.passed += 1,
            "PENDING" | "QUEUED" | "IN_PROGRESS" | "EXPECTED" | "WAITING" | "REQUESTED" => {
                state.pending += 1;
            }
            _ => state.failed += 1,
        }
        if name == BUILD_AGGREGATE && result == "SUCCESS" {
            state.aggregate_passed = true;
        }
    }
    state
}

fn check_result(check: &StatusCheck) -> &str {
    if check.kind.as_deref() == Some("CheckRun") {
        match check.status.as_deref() {
            Some("COMPLETED") | None => check.conclusion.as_deref().unwrap_or("PENDING"),
            Some(status) => status,
        }
    } else {
        check.state.as_deref().unwrap_or("PENDING")
    }
}

fn status_passed(checks: &[StatusCheck], expected: &str) -> bool {
    checks.iter().any(|check| {
        check.name.as_deref().or(check.context.as_deref()) == Some(expected)
            && check.conclusion.as_deref().or(check.state.as_deref()) == Some("SUCCESS")
    })
}

fn render_status(
    task: &Task,
    pull_request: &PullRequest,
    mut blockers: Vec<String>,
) -> Result<String, Error> {
    let integration = task.integration.as_ref().ok_or(Error::MissingIntegration)?;
    blockers.sort();
    blockers.dedup();
    let checks = check_state(&pull_request.status_check_rollup);
    let target_advanced = pull_request.base_ref_oid != integration.target_commit;
    let blocker_text = if blockers.is_empty() {
        "none".to_owned()
    } else {
        blockers.join(", ")
    };
    let next = if pull_request.state == "MERGED" || blockers.is_empty() {
        "rapport integrate complete"
    } else {
        "address blockers, then rapport integrate status"
    };
    let review_proof = format!(
        "{} grade {}",
        integration.review_task, integration.review_grade
    );
    Ok(format!(
        "# rapport integrate status\n\n- `task` — {}\n- `stage` — {:?}\n- `pull request` — {}\n- `source` — {} @ {}\n- `target` — {} @ {}\n- `candidate` — {}\n- `target advanced` — {}\n- `remote checks observed` — {}\n- `checks` — {} passed, {} pending, {} failed\n- `Review proof` — {}\n- `Review findings` — {}\n- `quality override` — {}\n- `GitHub review decision` — {} (policy and requests informational; changes requested blocks)\n- `requested reviews` — {} (informational)\n- `mergeability` — {} / {}\n- `blockers` — {}\n- `next` — `{}`",
        task.id,
        integration.stage,
        pull_request.url,
        integration.source_branch,
        short(&pull_request.head_ref_oid),
        integration.target_branch,
        short(&pull_request.base_ref_oid),
        short(&integration.candidate),
        target_advanced,
        checks.observed_remote,
        checks.passed,
        checks.pending,
        checks.failed,
        review_proof,
        if integration.review_findings.is_empty() {
            "none".to_owned()
        } else {
            integration.review_findings.join(", ")
        },
        integration.quality_override.as_deref().unwrap_or("none"),
        pull_request.review_decision.as_deref().unwrap_or("none"),
        pull_request.review_requests.len(),
        pull_request.mergeable,
        pull_request.merge_state_status,
        blocker_text,
        next
    ))
}

fn pull_request_body(
    work: &Work,
    tasks: &[Task],
    integration: &IntegrationTask,
) -> Result<String, Error> {
    let build = tasks
        .iter()
        .find(|task| task.id == integration.build_task)
        .and_then(|task| task.build.as_ref())
        .ok_or(Error::BuildIncomplete)?;
    let review = tasks
        .iter()
        .find(|task| task.id == integration.review_task)
        .and_then(|task| task.review.as_ref())
        .ok_or(Error::ReviewIncomplete)?;
    let result = review.result.as_ref().ok_or(Error::ReviewIncomplete)?;
    let checkpoints = tasks
        .iter()
        .filter(|task| task.kind == "checkpoint" && task.status == TaskStatus::Passed)
        .map(|task| format!("- {} — {}", task.id, task.title))
        .collect::<Vec<_>>();
    let develop_tasks = tasks
        .iter()
        .filter(|task| {
            task.workflow == Workflow::Develop
                && task.kind != "checkpoint"
                && task.status == TaskStatus::Passed
        })
        .map(|task| format!("- {} — {}", task.id, task.title))
        .collect::<Vec<_>>();
    let operations = build
        .operations
        .iter()
        .map(|operation| {
            format!(
                "- {} — {} ({})",
                operation.id,
                operation.identity.as_deref().unwrap_or("local proof"),
                operation.status
            )
        })
        .collect::<Vec<_>>();
    let findings = if review.findings.is_empty() {
        vec!["- none".to_owned()]
    } else {
        review
            .findings
            .iter()
            .map(|finding| {
                format!(
                    "- {} — {} ({:?})",
                    finding.id.as_deref().unwrap_or("finding"),
                    finding.title,
                    finding.status
                )
            })
            .collect()
    };
    Ok(format!(
        "{}\n\n## Source\n\n- {}: {}\n- Candidate: `{}`\n- Target: `{}`\n- Work: `{}`\n\n## Develop Tasks\n\n{}\n\n## Checkpoints\n\n{}\n\n## Build proof\n\n- Task: `{}`\n{}\n\n## Independent Review\n\n- Task: `{}`\n- Grade: `{}`\n- Quality-policy override: {}\n\n### Findings\n\n{}\n\n<!-- Rapport-Work: {} -->",
        work.description,
        match work.request.kind {
            super::domain::RequestKind::Ticket => "Ticket",
            super::domain::RequestKind::Plan => "Plan",
            super::domain::RequestKind::AdHoc => "Ad hoc request",
        },
        work.request.value,
        integration.candidate,
        integration.target_branch,
        work.id,
        none(develop_tasks.join("\n")),
        none(checkpoints.join("\n")),
        integration.build_task,
        none(operations.join("\n")),
        integration.review_task,
        result.overall_grade,
        review.quality_override.as_deref().unwrap_or("none"),
        findings.join("\n"),
        work.id
    ))
}

fn current_integration(tasks: &[Task]) -> Option<&Task> {
    tasks.iter().rev().find(|task| {
        task.workflow == Workflow::Integrate
            && task
                .integration
                .as_ref()
                .is_some_and(|integration| integration.stage != IntegrationStage::Cancelled)
    })
}

fn current_integration_index(tasks: &[Task]) -> Option<usize> {
    tasks.iter().rposition(|task| {
        task.workflow == Workflow::Integrate
            && task
                .integration
                .as_ref()
                .is_some_and(|integration| integration.stage != IntegrationStage::Cancelled)
    })
}

fn verify_pull_request(
    work: &Work,
    integration: &IntegrationTask,
    pull_request: &PullRequest,
) -> Result<(), Error> {
    let marker = format!("Rapport-Work: {}", work.id);
    if pull_request.is_cross_repository
        || pull_request.head_ref_name != integration.source_branch
        || pull_request.base_ref_name != integration.target_branch
        || pull_request.head_ref_oid != integration.candidate
        || !pull_request.body.contains(&marker)
    {
        return Err(Error::IntegrationOwnership);
    }
    Ok(())
}

fn open_pull_requests<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    source_branch: &str,
) -> Result<Vec<PullRequest>, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let output = run_gh(
        context,
        [
            "pr",
            "list",
            "--head",
            source_branch,
            "--state",
            "open",
            "--json",
            PULL_REQUEST_FIELDS,
        ],
    )?;
    Ok(serde_json::from_str(&output)?)
}

fn pull_request<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    identifier: &str,
) -> Result<PullRequest, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let output = run_gh(
        context,
        ["pr", "view", identifier, "--json", PULL_REQUEST_FIELDS],
    )?;
    Ok(serde_json::from_str(&output)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
    #[serde(default)]
    number: u64,
    #[serde(default)]
    url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    head_ref_oid: String,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    base_ref_oid: String,
    #[serde(default)]
    base_ref_name: String,
    #[serde(default)]
    is_cross_repository: bool,
    #[serde(default)]
    state: String,
    #[serde(default)]
    mergeable: String,
    #[serde(default)]
    merge_state_status: String,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    review_requests: Vec<ReviewRequest>,
    #[serde(default)]
    status_check_rollup: Vec<StatusCheck>,
    #[serde(default)]
    merge_commit: Option<CommitIdentity>,
}

#[derive(Debug, Deserialize)]
struct ReviewRequest {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusCheck {
    #[serde(default, rename = "__typename")]
    kind: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommitIdentity {
    oid: String,
}

#[derive(Debug, Deserialize)]
struct RepositoryIdentity {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

fn github_repository<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let output = run_gh(context, ["repo", "view", "--json", "nameWithOwner"])?;
    let identity: RepositoryIdentity = serde_json::from_str(&output)?;
    required(&identity.name_with_owner)
}

fn github_target_commit<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    repository: &str,
    target: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let endpoint = format!("repos/{repository}/branches/{}", percent_encode(target));
    required(&run_gh(context, ["api", &endpoint, "--jq", ".commit.sha"])?)
}

fn run_gh<F, C, O, E, I, S>(
    context: &mut CommandContext<'_, F, C, O, E>,
    arguments: I,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let specification = CommandSpec::new("gh", arguments);
    let outcome = context
        .runner
        .run(&specification, &context.repo_root)
        .map_err(|error| Error::GitHub(error.to_string()))?;
    if outcome.success {
        Ok(outcome.stdout)
    } else {
        let detail = [outcome.stderr.trim(), outcome.stdout.trim()]
            .into_iter()
            .find(|value| !value.is_empty())
            .unwrap_or("gh exited unsuccessfully");
        Err(Error::GitHub(detail.to_owned()))
    }
}

fn git_repository(root: &rapport_files::Utf8Path) -> Result<(Git, Repository), Error> {
    let git = Git::default();
    let repository = git.discover(root)?;
    Ok((git, repository))
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn required(value: &str) -> Result<String, Error> {
    let value = value.trim();
    if value.is_empty() {
        Err(Error::EmptyField)
    } else {
        Ok(value.to_owned())
    }
}

fn short(value: &str) -> String {
    value.chars().take(12).collect()
}

fn short_uuid(work: &Work) -> String {
    work.id.to_string().chars().take(6).collect()
}

fn none(value: String) -> String {
    if value.is_empty() {
        "- none".to_owned()
    } else {
        value
    }
}

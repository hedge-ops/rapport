//! Derived Work status and Task inspection views.
//!
//! This module owns current-state aggregation, task filtering and rendering,
//! integration blockers, and next-workflow selection without mutating state.

use super::cli::TaskAction;
use super::command::{git_repository, none, operation_name, paths, short, target_revision};
use super::domain::{Task, TaskStatus, Work, Workflow};
use super::repository::Store;
use super::{Error, develop};
use crate::context::{Clock, CommandContext};
use rapport_files::{FileSystem, Utf8PathBuf};
use rapport_git::{Operation, WorktreeStatus};
use std::collections::BTreeSet;
use std::io::Write;
use std::str::FromStr;

pub(super) fn render<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let (git, repository) = git_repository(&context.repo_root)?;
    let live = git.status(&repository)?;
    let (target, target_head) = target_revision(&git, &repository, &work.target_branch)?;
    let changes = git.source_side_changes(&repository, &target)?;
    let signoffs = crate::policy_context::required_signoffs_for_paths(
        context.fs,
        &context.repo_root,
        changes.paths().iter().map(Utf8PathBuf::as_path),
    )?;
    let policy_digest = crate::policy_context::effective_policy_digest_for_paths(
        context.fs,
        &context.repo_root,
        changes.paths().iter().map(Utf8PathBuf::as_path),
    )?;
    let operation = git.operation(&repository)?;
    let develop_state = if develop::is_complete(&work, &tasks, &live, operation) {
        "complete"
    } else {
        "incomplete"
    };
    let build_proof =
        if super::build::current_proof(&tasks, live.head().as_str(), &policy_digest, &signoffs) {
            "current"
        } else {
            "missing or stale"
        };
    let review_proof = if super::review::has_candidate_proof(&tasks, live.head().as_str()) {
        "current"
    } else {
        "missing or stale"
    };
    let blockers = integration_blockers(&work, &tasks, &live, operation);
    let next = select_next(&work, &tasks).map_or_else(
        || next_workflow(&work, &tasks, &live, operation),
        |task| {
            task.continuation
                .clone()
                .unwrap_or_else(|| "inspect Task".to_owned())
        },
    );
    Ok(format!(
        "# rapport work status\n\n- `work` — {}\n- `title` — {}\n- `description` — {}\n- `request` — {:?} {}\n- `source` — {} @ {}\n- `current branch` — {}\n- `target` — {} @ {}\n- `starting source` — {}\n- `starting target` — {}\n- `latest checkpoint` — {}\n- `contains target` — {}\n- `staged` — {}\n- `unstaged` — {}\n- `untracked` — {}\n- `conflicted` — {}\n- `operation` — {}\n- `candidate files` — {}\n- `required signoffs` — {}\n- `tasks` — {}\n- `task state` — {}\n- `Develop` — {}\n- `Build proof` — {}\n- `Review proof` — {}\n- `integration blockers` — {}\n- `next` — `{}`",
        work.id,
        work.title,
        work.description,
        work.request.kind,
        work.request.value,
        work.source_branch,
        short(live.head().as_str()),
        live.branch().unwrap_or("detached"),
        work.target_branch,
        short(target_head.as_str()),
        short(&work.starting_source),
        short(&work.starting_target),
        work.latest_checkpoint
            .as_deref()
            .map_or("none".to_owned(), short),
        git.contains(&repository, &target)?,
        paths(live.staged()),
        paths(live.unstaged()),
        paths(live.untracked()),
        paths(live.conflicted()),
        operation.map_or("none".to_owned(), operation_name),
        paths(changes.paths()),
        none(
            &signoffs
                .iter()
                .map(|signoff| format!(
                    "{} ({}; just {}; stage {}; resource {}; trigger {})",
                    signoff.id,
                    signoff.source_context,
                    signoff.target,
                    signoff.stage,
                    signoff.resource_group.as_deref().unwrap_or("none"),
                    signoff.triggers.join(", ")
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        tasks.len(),
        task_state(&tasks),
        develop_state,
        build_proof,
        review_proof,
        blockers,
        next
    ))
}

pub(super) fn task<F, C, O, E>(
    action: &TaskAction,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    match action {
        TaskAction::List {
            status,
            task_type,
            workflow,
            related_to,
            since_checkpoint,
            all,
        } => list_tasks(
            &work,
            &tasks,
            status,
            task_type,
            workflow,
            related_to.as_deref(),
            *since_checkpoint,
            *all,
        ),
        TaskAction::Show { id } => tasks
            .iter()
            .find(|task| task.id == *id)
            .map(|task| render_task(&work, task))
            .ok_or_else(|| Error::MissingTask(id.clone())),
        TaskAction::Next => {
            if let Some(task) = select_next(&work, &tasks) {
                return Ok(render_task(&work, task));
            }
            let (git, repository) = git_repository(&context.repo_root)?;
            let live = git.status(&repository)?;
            let operation = git.operation(&repository)?;
            Ok(format!(
                "# rapport work task next\n\n- `work` — {}\n- `description` — {}\n- `next workflow` — `{}`",
                work.title,
                work.description,
                next_workflow(&work, &tasks, &live, operation)
            ))
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the public Task list grammar exposes seven independently composable filters"
)]
fn list_tasks(
    work: &Work,
    tasks: &[Task],
    statuses: &[String],
    types: &[String],
    workflows: &[String],
    related_to: Option<&str>,
    since_checkpoint: bool,
    all: bool,
) -> Result<String, Error> {
    let statuses = statuses
        .iter()
        .map(|value| TaskStatus::from_str(value))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let workflows = workflows
        .iter()
        .map(|value| Workflow::from_str(value))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let checkpoint = tasks
        .iter()
        .rfind(|task| task.kind == "checkpoint" && task.status == TaskStatus::Passed)
        .map(|task| task.completed_at.as_deref().unwrap_or(&task.created_at));
    let recent_terminal = tasks
        .iter()
        .rev()
        .filter(|task| task.status.is_terminal())
        .take(5)
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut selected = tasks
        .iter()
        .filter(|task| statuses.is_empty() || statuses.contains(&task.status))
        .filter(|task| types.is_empty() || types.iter().any(|value| value == &task.kind))
        .filter(|task| workflows.is_empty() || workflows.contains(&task.workflow))
        .filter(|task| related_to.is_none_or(|id| task.related.iter().any(|related| related == id)))
        .filter(|task| {
            !since_checkpoint
                || checkpoint.is_none_or(|at| {
                    task.created_at.as_str() >= at
                        || task.completed_at.as_deref().is_some_and(|done| done >= at)
                })
        })
        .filter(|task| {
            all || !task.status.is_terminal() || recent_terminal.contains(task.id.as_str())
        })
        .collect::<Vec<_>>();
    selected.sort_by_key(|task| (task.status.is_terminal(), task.id.as_str()));
    let lines = selected
        .iter()
        .map(|task| {
            format!(
                "- `{}` — {} — {} — {} — next {}",
                task.id,
                task.status,
                task.workflow,
                task.title,
                task.continuation.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "# rapport work task list\n\n- `work` — {}\n- `description` — {}\n\n{}",
        work.title,
        work.description,
        none(&lines)
    ))
}

fn render_task(work: &Work, task: &Task) -> String {
    format!(
        "# rapport work task show\n\n- `work` — {}\n- `work description` — {}\n- `task` — {}\n- `type` — {}\n- `workflow` — {}\n- `status` — {}\n- `title` — {}\n- `description` — {}\n- `origin` — {}\n- `related` — {}\n- `source commit` — {}\n- `created` — {}\n- `completed` — {}\n- `result` — {}\n- `output` — {}\n- `payload` — {}\n- `next` — {}",
        work.title,
        work.description,
        task.id,
        task.kind,
        task.workflow,
        task.status,
        task.title,
        task.description,
        task.origin,
        none(&task.related.join(", ")),
        short(&task.source_commit),
        task.created_at,
        task.completed_at.as_deref().unwrap_or("none"),
        task.result.as_deref().unwrap_or("none"),
        task.output.as_deref().unwrap_or("none"),
        none(
            &task
                .payload
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        task.continuation.as_deref().unwrap_or("none")
    )
}

fn select_next<'task>(work: &Work, tasks: &'task [Task]) -> Option<&'task Task> {
    tasks
        .iter()
        .filter(|task| !task.status.is_terminal())
        .min_by_key(|task| {
            let priority = match task.status {
                TaskStatus::Running => 0,
                TaskStatus::Blocked => 1,
                TaskStatus::Pending if task.kind == "action" => 2,
                TaskStatus::Pending => 3,
                TaskStatus::Passed | TaskStatus::Failed | TaskStatus::Cancelled => 4,
            };
            let order = if task.is_develop_action() {
                work.development_sequence
                    .iter()
                    .position(|id| id == &task.id)
                    .and_then(|index| u32::try_from(index).ok())
                    .unwrap_or(u32::MAX)
            } else {
                u32::MAX
            };
            (priority, order, task.id.as_str())
        })
}

fn next_workflow(
    work: &Work,
    tasks: &[Task],
    live: &WorktreeStatus,
    operation: Option<Operation>,
) -> String {
    if let Some(failed) = develop::unresolved_failure(work, tasks) {
        return format!(
            "rapport develop task add --caused-by {} --title <TITLE> --description <DESCRIPTION>",
            failed.id
        );
    }
    let checkpoint = work
        .latest_checkpoint
        .as_deref()
        .unwrap_or(&work.starting_source);
    if !live.all_changed_paths().is_empty() || checkpoint != live.head().as_str() {
        "rapport work checkpoint start".to_owned()
    } else if develop::is_complete(work, tasks, live, operation) {
        if super::build::has_candidate_proof(tasks, live.head().as_str()) {
            if super::review::has_candidate_proof(tasks, live.head().as_str()) {
                "rapport integrate start".to_owned()
            } else {
                "rapport review start".to_owned()
            }
        } else {
            "rapport build".to_owned()
        }
    } else {
        "make the requested changes, then rapport work checkpoint start".to_owned()
    }
}

fn task_state(tasks: &[Task]) -> String {
    none(
        &tasks
            .iter()
            .map(|task| format!("{} {} {}", task.id, task.status, task.kind))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

fn integration_blockers(
    work: &Work,
    tasks: &[Task],
    live: &WorktreeStatus,
    operation: Option<Operation>,
) -> String {
    let mut blockers = Vec::new();
    if live.branch() != Some(work.source_branch.as_str()) {
        blockers.push("source branch changed");
    }
    if !live.is_clean() {
        blockers.push("worktree is not clean");
    }
    if operation.is_some() {
        blockers.push("source-control operation active");
    }
    if effective_checkpoint(work) != live.head().as_str() {
        blockers.push("source HEAD is not the latest checkpoint");
    }
    if tasks.iter().any(|task| !task.status.is_terminal()) {
        blockers.push("nonterminal Tasks remain");
    }
    if !develop::is_complete(work, tasks, live, operation) {
        blockers.push("Develop incomplete");
    }
    if !tasks
        .iter()
        .any(|task| task.workflow == Workflow::Build && task.status == TaskStatus::Passed)
    {
        blockers.push("Build proof missing");
    }
    if !tasks
        .iter()
        .any(|task| task.workflow == Workflow::Review && task.status == TaskStatus::Passed)
    {
        blockers.push("Review proof missing");
    }
    none(&blockers.join("; "))
}

pub(super) fn effective_checkpoint(work: &Work) -> &str {
    work.latest_checkpoint
        .as_deref()
        .unwrap_or(&work.starting_source)
}

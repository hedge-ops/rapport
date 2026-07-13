//! Work checkpoint lifecycle.
//!
//! This module owns adoption, staging reconciliation, commit creation, and
//! checkpoint cancellation while the command boundary supplies shared guards.

use super::Error;
use super::cli::CheckpointAction;
use super::command::{
    active_task, change_snapshot, ensure_no_active, ensure_source, git_repository, none, paths,
    required, short, tasks_since_checkpoint,
};
use super::domain::{Task, TaskStatus, Work, Workflow};
use super::repository::Store;
use crate::context::{Clock, CommandContext};
use rapport_files::FileSystem;
use rapport_git::{Git, Repository, Revision, WorktreeStatus};
use std::io::Write;

pub(super) fn run<F, C, O, E>(
    action: &CheckpointAction,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match action {
        CheckpointAction::Start => checkpoint_start(context),
        CheckpointAction::Complete {
            summary,
            description,
        } => checkpoint_complete(context, summary, description.as_deref()),
        CheckpointAction::Cancel { reason } => checkpoint_cancel(context, reason),
    }
}

fn checkpoint_start<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    ensure_no_active(&tasks, "checkpoint")?;
    let (git, repository) = git_repository(&context.repo_root)?;
    if git.operation(&repository)?.is_some() {
        return Err(Error::SourceOperationActive);
    }
    let live = git.status(&repository)?;
    ensure_source(&work, &live)?;
    if live.is_clean() {
        return adopt_checkpoint(context, &store, &mut work, &tasks, &git, &repository, &live);
    }
    let mut task = Task::new(
        work.allocate_task_id()?,
        "checkpoint",
        Workflow::Develop,
        "Reconcile and stage the next coherent checkpoint.",
        "Inspect all current changes, stage only the intended files or hunks, then complete the checkpoint.",
        "rapport work checkpoint start",
        TaskStatus::Running,
        live.head().as_str(),
        context.clock.now_rfc3339(),
        Some("rapport work checkpoint complete <SUMMARY>".to_owned()),
    );
    task.payload.insert(
        "snapshot".to_owned(),
        change_snapshot(&repository, &live, context.fs)?,
    );
    task.payload
        .insert("changes".to_owned(), paths(&live.all_changed_paths()));
    task.payload.insert(
        "tasks_since_checkpoint".to_owned(),
        tasks_since_checkpoint(&tasks).join(", "),
    );
    store.save_work_and_task(context.fs, &work, &task)?;
    Ok(format!(
        "# rapport work checkpoint start\n\n- `task` — {}\n- `source` — {}\n- `staged` — {}\n- `unstaged` — {}\n- `untracked` — {}\n- `tasks since checkpoint` — {}\n- `next` — stage intended changes, then `rapport work checkpoint complete <SUMMARY>`",
        task.id,
        short(live.head().as_str()),
        paths(live.staged()),
        paths(live.unstaged()),
        paths(live.untracked()),
        none(
            task.payload
                .get("tasks_since_checkpoint")
                .map_or("", String::as_str)
        )
    ))
}

fn adopt_checkpoint<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &Store,
    work: &mut Work,
    tasks: &[Task],
    git: &Git,
    repository: &Repository,
    live: &WorktreeStatus,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let prior = work
        .latest_checkpoint
        .as_ref()
        .unwrap_or(&work.starting_source)
        .clone();
    if prior == *live.head() {
        return Err(Error::EmptyCheckpoint);
    }
    let prior_revision = Revision::new(prior.as_str())?;
    if !git.contains(repository, &prior_revision)? {
        return Err(Error::AmbiguousCheckpoint);
    }
    let changed = git.source_side_changes(repository, &prior_revision)?;
    let now = context.clock.now_rfc3339();
    let mut task = Task::new(
        work.allocate_task_id()?,
        "checkpoint",
        Workflow::Develop,
        "Adopt an existing Git checkpoint.",
        "Record a clean source commit created directly through Git.",
        "rapport work checkpoint start",
        TaskStatus::Running,
        live.head().as_str(),
        now.clone(),
        None,
    );
    task.payload
        .insert("prior_commit".to_owned(), prior.as_str().to_owned());
    task.payload.insert(
        "resulting_commit".to_owned(),
        live.head().as_str().to_owned(),
    );
    task.payload
        .insert("committed_files".to_owned(), paths(changed.paths()));
    task.payload.insert(
        "tasks_since_checkpoint".to_owned(),
        tasks_since_checkpoint(tasks).join(", "),
    );
    task.finish(
        TaskStatus::Passed,
        now,
        format!("adopted checkpoint {}", short(live.head().as_str())),
        None,
    );
    work.latest_checkpoint = Some(live.head().clone());
    store.save_work_and_task(context.fs, work, &task)?;
    Ok(format!(
        "# rapport work checkpoint start\n\n- `task` — {}\n- `status` — passed\n- `adopted commit` — {}\n- `files` — {}\n- `next` — `rapport work task next`",
        task.id,
        short(live.head().as_str()),
        paths(changed.paths())
    ))
}

#[expect(
    clippy::too_many_lines,
    reason = "checkpoint completion keeps commit, corrective Task, and atomic ledger transitions together"
)]
fn checkpoint_complete<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    summary: &str,
    description: Option<&str>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    required(summary.to_owned())?;
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = active_task(&tasks, "checkpoint")?;
    let (git, repository) = git_repository(&context.repo_root)?;
    if git.operation(&repository)?.is_some() {
        return Err(Error::SourceOperationActive);
    }
    let before = git.status(&repository)?;
    ensure_source(&work, &before)?;
    let expected = tasks[index]
        .payload
        .get("snapshot")
        .cloned()
        .unwrap_or_default();
    let actual = change_snapshot(&repository, &before, context.fs)?;
    if actual != expected {
        return Err(Error::ConcurrentChanges(paths(&before.all_changed_paths())));
    }
    if before.staged().is_empty() {
        return Err(Error::EmptyCheckpoint);
    }
    let prior = before.head().as_str().to_owned();
    let committed = before.staged().clone();
    match git.commit(&repository, summary, description) {
        Ok(head) => {
            let after = git.status(&repository)?;
            work.latest_checkpoint = Some(head.clone());
            let checkpoint_id = tasks[index].id.clone();
            for action in &mut tasks {
                if action.kind == "action"
                    && action
                        .related
                        .iter()
                        .any(|related| related == &checkpoint_id)
                    && !action.status.is_terminal()
                {
                    action.finish(
                        TaskStatus::Passed,
                        context.clock.now_rfc3339(),
                        "checkpoint commit failure resolved".to_owned(),
                        None,
                    );
                    store.save_task(context.fs, action)?;
                }
            }
            tasks[index]
                .payload
                .insert("prior_commit".to_owned(), prior);
            tasks[index]
                .payload
                .insert("resulting_commit".to_owned(), head.as_str().to_owned());
            tasks[index]
                .payload
                .insert("committed_files".to_owned(), paths(&committed));
            tasks[index]
                .payload
                .insert("summary".to_owned(), summary.to_owned());
            if let Some(description) = description {
                tasks[index]
                    .payload
                    .insert("description".to_owned(), description.to_owned());
            }
            tasks[index].finish(
                TaskStatus::Passed,
                context.clock.now_rfc3339(),
                format!("created checkpoint {}", short(head.as_str())),
                Some(format!(
                    "remaining changes: {}",
                    paths(&after.all_changed_paths())
                )),
            );
            store.save_work_and_task(context.fs, &work, &tasks[index])?;
            Ok(format!(
                "# rapport work checkpoint complete\n\n- `task` — {}\n- `commit` — {}\n- `files` — {}\n- `remaining` — {}\n- `next` — `rapport work task next`",
                tasks[index].id,
                short(head.as_str()),
                paths(&committed),
                paths(&after.all_changed_paths())
            ))
        }
        Err(error) => {
            tasks[index].status = TaskStatus::Blocked;
            tasks[index].result = Some(error.to_string());
            tasks[index].continuation = Some("resolve the Git or hook failure".to_owned());
            let mut action = Task::new(
                work.allocate_task_id()?,
                "action",
                Workflow::Develop,
                "Resolve checkpoint commit failure.",
                error.to_string(),
                tasks[index].id.clone(),
                TaskStatus::Pending,
                prior,
                context.clock.now_rfc3339(),
                Some("rapport work checkpoint complete <SUMMARY>".to_owned()),
            );
            action.related.push(tasks[index].id.clone());
            tasks[index].related.push(action.id.clone());
            work.development_sequence.push(action.id.clone());
            store.save_task(context.fs, &tasks[index])?;
            store.save_work_and_task(context.fs, &work, &action)?;
            Err(Error::Git(error))
        }
    }
}

fn checkpoint_cancel<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    reason: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let reason = required(reason.to_owned())?;
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = active_task(&tasks, "checkpoint")?;
    tasks[index].finish(
        TaskStatus::Cancelled,
        context.clock.now_rfc3339(),
        reason,
        None,
    );
    store.save_work_and_task(context.fs, &work, &tasks[index])?;
    Ok(format!(
        "# rapport work checkpoint cancel\n\n- `task` — {}\n- `status` — cancelled\n- `Git state changed` — false",
        tasks[index].id
    ))
}

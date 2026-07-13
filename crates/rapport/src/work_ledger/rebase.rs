//! Source-branch rebase lifecycle.
//!
//! This module owns clean-worktree preparation, Git rebase transitions,
//! conflict correction tasks, continuation, and abort behavior.

use super::Error;
use super::cli::RebaseAction;
use super::command::{
    active_task, ensure_source, git_repository, object_ids, paths, required, short,
};
use super::domain::{Task, TaskStatus, Work, Workflow};
use super::repository::Store;
use crate::context::{Clock, CommandContext};
use rapport_files::FileSystem;
use rapport_git::{Git, Operation, RebaseOutcome, Repository, Revision};
use std::io::Write;

pub(super) fn run<F, C, O, E>(
    action: &RebaseAction,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match action {
        RebaseAction::Start => rebase_start(context),
        RebaseAction::Continue => rebase_continue(context),
        RebaseAction::Abort { reason } => rebase_abort(context, reason),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "rebase start keeps clean-worktree correction and Git transition in one workflow boundary"
)]
fn rebase_start<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let existing = tasks
        .iter()
        .position(|task| task.kind == "rebase" && !task.status.is_terminal());
    let (git, repository) = git_repository(&context.repo_root)?;
    if git.operation(&repository)?.is_some() {
        return Err(Error::SourceOperationActive);
    }
    let before = git.status(&repository)?;
    ensure_source(&work, &before)?;
    if !before.is_clean() {
        if existing.is_some() {
            return Err(Error::DirtyWorktree);
        }
        let mut rebase = Task::new(
            work.allocate_task_id()?,
            "rebase",
            Workflow::Rebase,
            "Rebase source onto its target.",
            "A clean worktree is required before rebase can begin.",
            "rapport work rebase start",
            TaskStatus::Blocked,
            before.head().as_str(),
            context.clock.now_rfc3339(),
            Some("prepare a clean worktree".to_owned()),
        );
        rebase
            .payload
            .insert("awaiting_clean_worktree".to_owned(), "true".to_owned());
        let mut action = Task::new(
            work.allocate_task_id()?,
            "action",
            Workflow::Rebase,
            "Prepare a clean worktree.",
            format!("Current changes: {}", paths(&before.all_changed_paths())),
            rebase.id.clone(),
            TaskStatus::Pending,
            before.head().as_str(),
            context.clock.now_rfc3339(),
            Some("rapport work rebase start".to_owned()),
        );
        rebase.related.push(action.id.clone());
        action.related.push(rebase.id.clone());
        store.save_task(context.fs, &rebase)?;
        store.save_work_and_task(context.fs, &work, &action)?;
        return Err(Error::DirtyWorktree);
    }
    let (target, target_commit) = git.fetch_target(&repository, &work.target_branch)?;
    let mut task = if let Some(index) = existing {
        if tasks[index]
            .payload
            .get("awaiting_clean_worktree")
            .map(String::as_str)
            != Some("true")
        {
            return Err(Error::ActiveTask("rebase".to_owned()));
        }
        for action in &mut tasks {
            if action.kind == "action"
                && action.workflow == Workflow::Rebase
                && !action.status.is_terminal()
            {
                action.finish(
                    TaskStatus::Passed,
                    context.clock.now_rfc3339(),
                    "clean worktree prepared".to_owned(),
                    None,
                );
                store.save_task(context.fs, action)?;
            }
        }
        let mut task = tasks[index].clone();
        task.status = TaskStatus::Running;
        before.head().as_str().clone_into(&mut task.source_commit);
        task.continuation = Some("rapport work rebase continue".to_owned());
        task.payload.remove("awaiting_clean_worktree");
        task
    } else {
        Task::new(
            work.allocate_task_id()?,
            "rebase",
            Workflow::Rebase,
            "Rebase source onto its target.",
            format!("Rebase {} onto {}.", work.source_branch, work.target_branch),
            "rapport work rebase start",
            TaskStatus::Running,
            before.head().as_str(),
            context.clock.now_rfc3339(),
            Some("rapport work rebase continue".to_owned()),
        )
    };
    task.payload.insert(
        "prior_source_commit".to_owned(),
        before.head().as_str().to_owned(),
    );
    task.payload.insert(
        "target_commit".to_owned(),
        target_commit.as_str().to_owned(),
    );
    task.payload.insert(
        "prior_source_commits".to_owned(),
        object_ids(&git.source_commits(&repository, &target)?),
    );
    store.save_work_and_task(context.fs, &work, &task)?;
    finish_rebase(
        context,
        &store,
        &mut work,
        task,
        git.rebase_start(&repository, &target)?,
        &git,
        &repository,
    )
}

fn rebase_continue<F, C, O, E>(
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
    let mut tasks = store.load_tasks(context.fs)?;
    let index = active_task(&tasks, "rebase")?;
    let (git, repository) = git_repository(&context.repo_root)?;
    if git.operation(&repository)? != Some(Operation::Rebase) {
        return Err(Error::SourceOperationActive);
    }
    for task in &mut tasks {
        if task.kind == "action" && task.workflow == Workflow::Rebase && !task.status.is_terminal()
        {
            task.finish(
                TaskStatus::Passed,
                context.clock.now_rfc3339(),
                "conflicts resolved and staged".to_owned(),
                None,
            );
            store.save_task(context.fs, task)?;
        }
    }
    let outcome = git.rebase_continue(&repository)?;
    finish_rebase(
        context,
        &store,
        &mut work,
        tasks[index].clone(),
        outcome,
        &git,
        &repository,
    )
}

fn finish_rebase<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &Store,
    work: &mut Work,
    mut task: Task,
    outcome: RebaseOutcome,
    git: &Git,
    repository: &Repository,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match outcome {
        RebaseOutcome::Completed => {
            let live = git.status(repository)?;
            work.latest_checkpoint = Some(live.head().as_str().to_owned());
            task.payload.insert(
                "resulting_source_commit".to_owned(),
                live.head().as_str().to_owned(),
            );
            let target = Revision::new(
                task.payload
                    .get("target_commit")
                    .cloned()
                    .unwrap_or_default(),
            )?;
            task.payload.insert(
                "resulting_source_commits".to_owned(),
                object_ids(&git.source_commits(repository, &target)?),
            );
            task.finish(
                TaskStatus::Passed,
                context.clock.now_rfc3339(),
                format!("rebased onto {}", work.target_branch),
                None,
            );
            store.save_work_and_task(context.fs, work, &task)?;
            Ok(format!(
                "# rapport work rebase\n\n- `task` — {}\n- `status` — passed\n- `source` — {}\n- `target` — {}\n- `next` — `rapport work task next`",
                task.id,
                short(live.head().as_str()),
                short(task.payload.get("target_commit").map_or("", String::as_str))
            ))
        }
        RebaseOutcome::Conflicts => {
            let live = git.status(repository)?;
            task.status = TaskStatus::Blocked;
            task.continuation = Some("rapport work rebase continue".to_owned());
            let mut action = Task::new(
                work.allocate_task_id()?,
                "action",
                Workflow::Rebase,
                "Resolve and stage rebase conflicts.",
                format!("Conflicted files: {}", paths(live.conflicted())),
                task.id.clone(),
                TaskStatus::Pending,
                live.head().as_str(),
                context.clock.now_rfc3339(),
                Some("rapport work rebase continue".to_owned()),
            );
            action.related.push(task.id.clone());
            task.related.push(action.id.clone());
            store.save_task(context.fs, &task)?;
            store.save_work_and_task(context.fs, work, &action)?;
            Ok(format!(
                "# rapport work rebase\n\n- `task` — {}\n- `status` — blocked\n- `conflicts` — {}\n- `action` — {}\n- `next` — resolve and stage conflicts, then `rapport work rebase continue`",
                task.id,
                paths(live.conflicted()),
                action.id
            ))
        }
    }
}

fn rebase_abort<F, C, O, E>(
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
    let index = active_task(&tasks, "rebase")?;
    let (git, repository) = git_repository(&context.repo_root)?;
    if git.operation(&repository)? == Some(Operation::Rebase) {
        git.rebase_abort(&repository)?;
    } else {
        ensure_source(&work, &git.status(&repository)?)?;
    }
    for task in &mut tasks {
        if task.kind == "action" && task.workflow == Workflow::Rebase && !task.status.is_terminal()
        {
            task.finish(
                TaskStatus::Cancelled,
                context.clock.now_rfc3339(),
                format!("rebase aborted: {reason}"),
                None,
            );
            store.save_task(context.fs, task)?;
        }
    }
    tasks[index].finish(
        TaskStatus::Cancelled,
        context.clock.now_rfc3339(),
        reason,
        None,
    );
    store.save_work_and_task(context.fs, &work, &tasks[index])?;
    Ok(format!(
        "# rapport work rebase abort\n\n- `task` — {}\n- `status` — cancelled\n- `source restored` — {}",
        tasks[index].id,
        short(git.status(&repository)?.head().as_str())
    ))
}

//! Work CLI orchestration and status rendering.
//!
//! This module dispatches Work actions and derives cross-workflow status while
//! focused modules own persistence, Build, Review, and Integration behavior.

use super::Error;
use super::cli::{Action, Cli, StartArgs};
use super::develop;
use super::domain::{RequestKind, RequestSource, Task, TaskStatus, Work, WorkOutcomeKind};
use super::history::HistoryStore;
use super::repository::Store;
use crate::context::{Clock, CommandContext};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::{Git, ObjectId, Operation, Repository, Revision, WorktreeStatus};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Write;
use std::process::ExitCode;

pub(crate) fn run<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result = execute(&cli.command, context);
    match result {
        Ok(output) => {
            let _ = writeln!(context.out, "{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "# rapport work\n\n{error}");
            ExitCode::from(2)
        }
    }
}

fn execute<F, C, O, E>(
    action: &Action,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match action {
        Action::Start(args) => start(args, context),
        Action::Status => super::status::render(context),
        Action::Task(args) => super::status::task(&args.command, context),
        Action::Checkpoint(args) => super::checkpoint::run(&args.command, context),
        Action::Rebase(args) => super::rebase::run(&args.command, context),
        Action::Complete { result } => end_work(context, result, true),
        Action::Abandon { reason } => end_work(context, reason, false),
        Action::History(cli) => super::history::execute(cli, context),
    }
}

pub(super) fn git_repository(repo_root: &Utf8Path) -> Result<(Git, Repository), Error> {
    let git = Git::default();
    let repository = git.discover(repo_root)?;
    Ok((git, repository))
}

pub(super) fn target_revision(
    git: &Git,
    repository: &Repository,
    branch: &str,
) -> Result<(Revision, ObjectId), Error> {
    let remote = Revision::new(format!("refs/remotes/origin/{branch}"))?;
    if let Ok(commit) = git.resolve(repository, &remote) {
        return Ok((remote, commit));
    }
    let local = Revision::new(branch.to_owned())?;
    let commit = git.resolve(repository, &local)?;
    Ok((local, commit))
}

fn start<F, C, O, E>(
    args: &StartArgs,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    if store.load_work(context.fs)?.is_some() {
        return Err(Error::ActiveWorkExists);
    }
    let (git, repository) = git_repository(&context.repo_root)?;
    let status = git.status(&repository)?;
    if !status.is_clean() {
        return Err(Error::DirtyWorktree);
    }
    if git.operation(&repository)?.is_some() {
        return Err(Error::SourceOperationActive);
    }
    let source_branch = status.branch().ok_or(Error::DetachedHead)?.to_owned();
    let target_branch = args
        .target
        .clone()
        .map_or_else(|| git.default_target(&repository), Ok)?;
    if source_branch == target_branch {
        return Err(Error::SourceIsTarget);
    }
    let (_, target_commit) = target_revision(&git, &repository, &target_branch)?;
    let (request, description) = request(args, context.fs, &context.repo_root)?;
    let work = Work::new(
        args.title.clone(),
        description,
        request,
        context.repo_root.to_string(),
        source_branch,
        target_branch,
        status.head().as_str().to_owned(),
        target_commit.as_str().to_owned(),
        context.clock.now_rfc3339(),
    )?;
    store.save_work(context.fs, &work)?;
    Ok(format!(
        "# rapport work start\n\n- `work` — {}\n- `title` — {}\n- `source` — {} @ {}\n- `target` — {} @ {}\n- `next` — `rapport work task next`",
        work.id,
        work.title,
        work.source_branch,
        short(&work.starting_source),
        work.target_branch,
        short(&work.starting_target)
    ))
}

fn request(
    args: &StartArgs,
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
) -> Result<(RequestSource, String), Error> {
    match (&args.ticket, &args.plan, &args.ad_hoc) {
        (Some(ticket), None, None) => Ok((
            RequestSource {
                kind: RequestKind::Ticket,
                value: required(ticket.clone())?,
            },
            required(args.description.clone().ok_or(Error::EmptyField)?)?,
        )),
        (None, Some(plan), None) => {
            if plan.is_absolute()
                || plan
                    .components()
                    .any(|component| component.as_str() == "..")
                || !fs.is_file(repo_root.join(plan))
            {
                return Err(Error::InvalidPlan);
            }
            Ok((
                RequestSource {
                    kind: RequestKind::Plan,
                    value: plan.to_string(),
                },
                required(args.description.clone().ok_or(Error::EmptyField)?)?,
            ))
        }
        (None, None, Some(request)) => {
            let request = required(request.clone())?;
            Ok((
                RequestSource {
                    kind: RequestKind::AdHoc,
                    value: request.clone(),
                },
                request,
            ))
        }
        _ => Err(Error::InvalidRequestSource),
    }
}

fn end_work<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    outcome: &str,
    completed: bool,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let outcome = required(outcome.to_owned())?;
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let outcome_kind = if completed {
        WorkOutcomeKind::Completed
    } else {
        WorkOutcomeKind::Abandoned
    };
    if let Some(existing) = &work.outcome {
        if existing.kind != outcome_kind {
            return Err(Error::FinalizedWork(existing.kind.to_string()));
        }
        let history =
            HistoryStore::new(&context.repo_root)?.archive(context.fs, &store, &work, &tasks)?;
        return Ok(format!(
            "# rapport work {}\n\n- `work` — {}\n- `outcome` — {}\n- `remaining Git changes` — preserved from finalized Work\n- `history` — {}\n- `Git state changed` — false",
            if completed { "complete" } else { "abandon" },
            work.id,
            existing.summary,
            history
        ));
    }
    let (git, repository) = git_repository(&context.repo_root)?;
    if git.operation(&repository)?.is_some() {
        return Err(Error::SourceOperationActive);
    }
    let live = git.status(&repository)?;
    ensure_source(&work, &live)?;
    if completed {
        if tasks.iter().any(|task| !task.status.is_terminal()) {
            return Err(Error::NonterminalTasks);
        }
        if !live.is_clean() {
            return Err(Error::DirtyWorktree);
        }
        if super::status::effective_checkpoint(&work) != live.head().as_str() {
            return Err(Error::UncheckpointedHead);
        }
        if !develop::is_complete(&work, &tasks, &live, None) {
            return Err(Error::DevelopIncomplete);
        }
        let target = Revision::new(work.target_branch.clone())?;
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
        if !super::build::current_proof(&tasks, live.head().as_str(), &policy_digest, &signoffs) {
            return Err(Error::BuildIncomplete);
        }
        if !super::review::has_candidate_proof(&tasks, live.head().as_str()) {
            return Err(Error::ReviewIncomplete);
        }
    }
    let final_target = target_revision(&git, &repository, &work.target_branch).map_or_else(
        |_| work.starting_target.clone(),
        |(_, commit)| commit.as_str().to_owned(),
    );
    work.finish(
        outcome_kind,
        context.clock.now_rfc3339(),
        outcome.clone(),
        live.head().as_str().to_owned(),
        final_target,
    )?;
    store.save_work(context.fs, &work)?;
    let remaining = paths(&live.all_changed_paths());
    let history =
        HistoryStore::new(&context.repo_root)?.archive(context.fs, &store, &work, &tasks)?;
    Ok(format!(
        "# rapport work {}\n\n- `work` — {}\n- `outcome` — {}\n- `remaining Git changes` — {}\n- `history` — {}\n- `Git state changed` — false",
        if completed { "complete" } else { "abandon" },
        work.id,
        outcome,
        remaining,
        history
    ))
}

pub(super) fn ensure_no_active(tasks: &[Task], task_type: &str) -> Result<(), Error> {
    if tasks
        .iter()
        .any(|task| task.kind == task_type && !task.status.is_terminal())
    {
        Err(Error::ActiveTask(task_type.to_owned()))
    } else {
        Ok(())
    }
}

pub(super) fn ensure_source(work: &Work, status: &WorktreeStatus) -> Result<(), Error> {
    let actual = status.branch().unwrap_or("detached");
    if actual == work.source_branch {
        Ok(())
    } else {
        Err(Error::SourceBranchChanged {
            expected: work.source_branch.clone(),
            actual: actual.to_owned(),
        })
    }
}

pub(super) fn active_task(tasks: &[Task], task_type: &str) -> Result<usize, Error> {
    tasks
        .iter()
        .position(|task| task.kind == task_type && !task.status.is_terminal())
        .ok_or_else(|| Error::MissingTask(format!("active {task_type}")))
}

pub(super) fn change_snapshot(
    repository: &Repository,
    status: &WorktreeStatus,
    fs: &impl FileSystem,
) -> Result<String, Error> {
    let mut hasher = Sha256::new();
    for path in status.all_changed_paths() {
        hasher.update(path.as_str().as_bytes());
        let absolute = repository.root().join(&path);
        if fs.is_file(&absolute) {
            let mode = fs.git_file_mode(&absolute).map_err(|source| Error::Io {
                path: absolute.clone(),
                source,
            })?;
            hasher.update(mode.to_le_bytes());
            let contents = fs.read_bytes(&absolute).map_err(|source| Error::Io {
                path: absolute,
                source,
            })?;
            hasher.update(contents);
        } else {
            hasher.update(b"deleted");
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(super) fn tasks_since_checkpoint(tasks: &[Task]) -> Vec<String> {
    let checkpoint = tasks
        .iter()
        .rposition(|task| task.kind == "checkpoint" && task.status == TaskStatus::Passed)
        .map_or(0, |index| index + 1);
    tasks[checkpoint..]
        .iter()
        .map(|task| task.id.clone())
        .collect()
}

pub(super) fn object_ids(ids: &[ObjectId]) -> String {
    ids.iter()
        .map(ObjectId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn required(value: String) -> Result<String, Error> {
    if value.trim().is_empty() {
        Err(Error::EmptyField)
    } else {
        Ok(value)
    }
}

pub(super) fn operation_name(operation: Operation) -> String {
    match operation {
        Operation::Rebase => "rebase",
        Operation::Merge => "merge",
        Operation::CherryPick => "cherry-pick",
    }
    .to_owned()
}

pub(super) fn paths(paths: &BTreeSet<Utf8PathBuf>) -> String {
    none(
        &paths
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    )
}

pub(super) fn none(value: &str) -> String {
    if value.is_empty() {
        "none".to_owned()
    } else {
        value.to_owned()
    }
}

pub(super) fn short(value: &str) -> String {
    value.chars().take(12).collect()
}

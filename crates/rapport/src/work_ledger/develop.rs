//! Processes ordered Develop Action Tasks.
//!
//! Owns sequence changes, Task transitions, causal correction, and derived
//! Develop completion. Git and persistence remain shared Work-ledger concerns.

use super::Error;
use super::domain::{Task, TaskStatus, Work, Workflow};
use super::repository::Store;
use crate::context::{Clock, CommandContext};
use clap::{ArgGroup, Args, Subcommand};
use rapport_files::{FileSystem, Utf8PathBuf};
use rapport_git::{Git, Operation, WorktreeStatus};
use std::collections::BTreeSet;
use std::fmt;
use std::io::Write;
use std::process::ExitCode;

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Action,
}

impl fmt::Debug for Cli {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DevelopCli")
            .field("action", &"task")
            .finish()
    }
}

#[derive(Subcommand)]
enum Action {
    /// Manage the ordered sequence of development work.
    Task(TaskArgs),
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct TaskArgs {
    #[command(subcommand)]
    command: TaskAction,
}

#[derive(Subcommand)]
enum TaskAction {
    /// List Action Tasks in development order.
    List,
    /// Show a complete Action Task and its cause.
    Show { id: String },
    /// Add a pending Action Task.
    Add(AddArgs),
    /// Update a pending Action Task.
    Update(UpdateArgs),
    /// Move a pending Action Task without changing its ID.
    Move(MoveArgs),
    /// Cancel pending work that is no longer needed.
    Cancel {
        id: String,
        #[arg(long)]
        reason: String,
    },
    /// Start a pending Action Task.
    Start { id: String },
    /// Complete a running Action Task.
    Complete {
        id: String,
        #[arg(long)]
        result: String,
    },
    /// Record a running Action Task as failed.
    Fail {
        id: String,
        #[arg(long)]
        result: String,
    },
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("position")
        .multiple(false)
        .args(["before", "after"])
))]
struct AddArgs {
    #[arg(long)]
    title: String,
    #[arg(long)]
    description: String,
    #[arg(long)]
    caused_by: Option<String>,
    #[arg(long)]
    before: Option<String>,
    #[arg(long)]
    after: Option<String>,
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("change")
        .required(true)
        .multiple(true)
        .args(["title", "description"])
))]
struct UpdateArgs {
    id: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    description: Option<String>,
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("position")
        .required(true)
        .multiple(false)
        .args(["before", "after"])
))]
struct MoveArgs {
    id: String,
    #[arg(long)]
    before: Option<String>,
    #[arg(long)]
    after: Option<String>,
}

pub(crate) fn run<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result = match &cli.command {
        Action::Task(args) => execute(&args.command, context),
    };
    match result {
        Ok(output) => {
            let _ = writeln!(context.out, "{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "# rapport develop\n\n{error}");
            ExitCode::from(2)
        }
    }
}

fn execute<F, C, O, E>(
    action: &TaskAction,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    match action {
        TaskAction::List => list(context),
        TaskAction::Show { id } => show(context, id),
        TaskAction::Add(args) => add(context, args),
        TaskAction::Update(args) => update(context, args),
        TaskAction::Move(args) => move_task(context, args),
        TaskAction::Cancel { id, reason } => cancel(context, id, reason),
        TaskAction::Start { id } => start(context, id),
        TaskAction::Complete { id, result } => complete(context, id, result),
        TaskAction::Fail { id, result } => fail(context, id, result),
    }
}

fn list<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let lines = ordered_actions(&work, &tasks)
        .into_iter()
        .map(|task| {
            format!(
                "- `{}` — {} — {} — next {}",
                task.id,
                task.status,
                task.title,
                task.continuation.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "# rapport develop task list\n\n- `work` — {}\n- `description` — {}\n\n{}",
        work.title,
        work.description,
        none(&lines)
    ))
}

fn show<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>, id: &str) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let task = require_action(&tasks, id)?;
    let cause = task
        .payload
        .get("caused_by")
        .and_then(|cause| tasks.iter().find(|candidate| candidate.id == *cause))
        .map_or_else(
            || "none".to_owned(),
            |cause| {
                format!(
                    "{} {} {} — {}",
                    cause.id,
                    cause.status,
                    cause.kind,
                    cause.result.as_deref().unwrap_or("no result")
                )
            },
        );
    Ok(format!(
        "# rapport develop task show\n\n- `work` — {}\n- `task` — {}\n- `order` — {}\n- `status` — {}\n- `title` — {}\n- `description` — {}\n- `origin` — {}\n- `caused by` — {}\n- `related` — {}\n- `source commit` — {}\n- `created` — {}\n- `completed` — {}\n- `result` — {}\n- `output` — {}\n- `payload` — {}\n- `next` — {}",
        work.title,
        task.id,
        action_position(&work, &tasks, &task.id),
        task.status,
        task.title,
        task.description,
        task.origin,
        cause,
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
    ))
}

fn add<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    args: &AddArgs,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let title = required(&args.title)?;
    let description = required(&args.description)?;
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    let live = git.status(&repository)?;
    ensure_source(&work, &live)?;
    let id = work.allocate_task_id()?;
    let mut task = Task::new(
        id.clone(),
        "action",
        Workflow::Develop,
        title,
        description,
        "rapport develop task add",
        TaskStatus::Pending,
        live.head().as_str(),
        context.clock.now_rfc3339(),
        Some(format!("rapport develop task start {id}")),
    );
    let mut changed_cause = None;
    if let Some(cause) = &args.caused_by {
        let cause_index = tasks
            .iter()
            .position(|candidate| candidate.id == *cause)
            .ok_or_else(|| Error::MissingTask(cause.clone()))?;
        task.payload.insert("caused_by".to_owned(), cause.clone());
        task.related.push(cause.clone());
        if !tasks[cause_index].status.is_terminal() {
            tasks[cause_index].related.push(id.clone());
            changed_cause = Some(tasks[cause_index].clone());
        }
    }
    let mut sequence = ordered_action_ids(&work, &tasks);
    insert_at(
        &mut sequence,
        id.clone(),
        args.before.as_deref(),
        args.after.as_deref(),
    )?;
    work.development_sequence.clone_from(&sequence);
    let mut writes = vec![task.clone()];
    if let Some(cause) = changed_cause {
        writes.push(cause);
    }
    store.save_work_and_tasks(context.fs, &work, &writes)?;
    Ok(format!(
        "# rapport develop task add\n\n- `task` — {id}\n- `status` — pending\n- `position` — {}\n- `next` — `rapport develop task start {id}`",
        sequence
            .iter()
            .position(|candidate| candidate == &id)
            .map_or(0, |index| index + 1)
    ))
}

fn update<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    args: &UpdateArgs,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    if args.title.is_none() && args.description.is_none() {
        return Err(Error::EmptyTaskUpdate);
    }
    let store = Store::new(&context.repo_root);
    store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = pending_action_index(&tasks, &args.id)?;
    if let Some(title) = &args.title {
        tasks[index].title = required(title)?;
    }
    if let Some(description) = &args.description {
        tasks[index].description = required(description)?;
    }
    store.save_task(context.fs, &tasks[index])?;
    Ok(format!(
        "# rapport develop task update\n\n- `task` — {}\n- `status` — pending\n- `title` — {}\n- `description` — {}",
        tasks[index].id, tasks[index].title, tasks[index].description
    ))
}

fn move_task<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    args: &MoveArgs,
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
    pending_action_index(&tasks, &args.id)?;
    let mut sequence = ordered_action_ids(&work, &tasks);
    sequence.retain(|candidate| candidate != &args.id);
    insert_at(
        &mut sequence,
        args.id.clone(),
        args.before.as_deref(),
        args.after.as_deref(),
    )?;
    work.development_sequence.clone_from(&sequence);
    store.save_work(context.fs, &work)?;
    Ok(format!(
        "# rapport develop task move\n\n- `task` — {}\n- `position` — {}\n- `identity changed` — false",
        args.id,
        sequence
            .iter()
            .position(|candidate| candidate == &args.id)
            .map_or(0, |index| index + 1)
    ))
}

fn cancel<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    id: &str,
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
    store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = pending_action_index(&tasks, id)?;
    tasks[index].finish(
        TaskStatus::Cancelled,
        context.clock.now_rfc3339(),
        reason,
        None,
    );
    store.save_task(context.fs, &tasks[index])?;
    Ok(format!(
        "# rapport develop task cancel\n\n- `task` — {}\n- `status` — cancelled",
        tasks[index].id
    ))
}

fn start<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    id: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = pending_action_index(&tasks, id)?;
    if tasks
        .iter()
        .any(|task| task.is_develop_action() && task.status == TaskStatus::Running)
    {
        return Err(Error::DevelopTaskRunning);
    }
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    if git.operation(&repository)?.is_some() {
        return Err(Error::SourceOperationActive);
    }
    let live = git.status(&repository)?;
    ensure_source(&work, &live)?;
    tasks[index].status = TaskStatus::Running;
    tasks[index]
        .payload
        .insert("started_at".to_owned(), context.clock.now_rfc3339());
    tasks[index]
        .payload
        .insert("started_task_cursor".to_owned(), work.next_task.to_string());
    record_git_state(&mut tasks[index], "initial", &live, None);
    tasks[index].continuation = Some(format!(
        "rapport develop task complete {} --result <RESULT>",
        tasks[index].id
    ));
    store.save_task(context.fs, &tasks[index])?;
    Ok(format!(
        "# rapport develop task start\n\n- `task` — {}\n- `status` — running\n- `source` — {}\n- `changes` — {}\n- `next` — checkpoint changed files, then `rapport develop task complete {} --result <RESULT>`",
        tasks[index].id,
        short(live.head().as_str()),
        paths(&live.all_changed_paths()),
        tasks[index].id
    ))
}

fn complete<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    id: &str,
    result: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result = required(result)?;
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = running_action_index(&tasks, id)?;
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    if git.operation(&repository)?.is_some() {
        return Err(Error::SourceOperationActive);
    }
    let live = git.status(&repository)?;
    ensure_source(&work, &live)?;
    if !live.is_clean() {
        return Err(Error::DirtyWorktree);
    }
    if effective_checkpoint(&work) != live.head().as_str() {
        return Err(Error::UncheckpointedHead);
    }
    let started_at = tasks[index]
        .payload
        .get("started_at")
        .cloned()
        .unwrap_or_else(|| tasks[index].created_at.clone());
    let started_task_cursor = tasks[index]
        .payload
        .get("started_task_cursor")
        .and_then(|value| value.parse::<u32>().ok());
    let checkpoint_ids = tasks
        .iter()
        .filter(|task| {
            task.kind == "checkpoint"
                && task.status == TaskStatus::Passed
                && started_task_cursor.map_or_else(
                    || task.created_at >= started_at,
                    |cursor| task_number(&task.id).is_some_and(|number| number >= cursor),
                )
        })
        .map(|task| task.id.clone())
        .collect::<Vec<_>>();
    let action_id = tasks[index].id.clone();
    for checkpoint_id in &checkpoint_ids {
        if !tasks[index].related.contains(checkpoint_id) {
            tasks[index].related.push(checkpoint_id.clone());
        }
        if let Some(checkpoint) = tasks.iter_mut().find(|task| task.id == *checkpoint_id)
            && !checkpoint.related.contains(&action_id)
        {
            checkpoint.related.push(action_id.clone());
        }
    }
    record_git_state(&mut tasks[index], "final", &live, None);
    tasks[index].finish(
        TaskStatus::Passed,
        context.clock.now_rfc3339(),
        result,
        Some(format!("checkpoints: {}", none(&checkpoint_ids.join(", ")))),
    );
    let writes = tasks
        .iter()
        .filter(|task| task.id == action_id || checkpoint_ids.contains(&task.id))
        .cloned()
        .collect::<Vec<_>>();
    store.save_tasks(context.fs, &writes)?;
    Ok(format!(
        "# rapport develop task complete\n\n- `task` — {}\n- `status` — passed\n- `source` — {}\n- `checkpoints` — {}\n- `next` — `rapport work task next`",
        tasks[index].id,
        short(live.head().as_str()),
        none(&checkpoint_ids.join(", "))
    ))
}

fn fail<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    id: &str,
    result: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result = required(result)?;
    let store = Store::new(&context.repo_root);
    store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = running_action_index(&tasks, id)?;
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    let live = git.status(&repository)?;
    let operation = git.operation(&repository)?;
    record_git_state(&mut tasks[index], "final", &live, operation);
    tasks[index].finish(
        TaskStatus::Failed,
        context.clock.now_rfc3339(),
        result,
        None,
    );
    store.save_task(context.fs, &tasks[index])?;
    Ok(format!(
        "# rapport develop task fail\n\n- `task` — {}\n- `status` — failed\n- `next` — `rapport develop task add --caused-by {} --title <TITLE> --description <DESCRIPTION>`",
        tasks[index].id, tasks[index].id
    ))
}

pub(super) fn is_complete(
    work: &Work,
    tasks: &[Task],
    live: &WorktreeStatus,
    operation: Option<Operation>,
) -> bool {
    operation.is_none()
        && live.is_clean()
        && effective_checkpoint(work) == live.head().as_str()
        && !tasks.iter().any(|task| {
            task.is_develop_action()
                && matches!(
                    task.status,
                    TaskStatus::Pending | TaskStatus::Running | TaskStatus::Blocked
                )
        })
        && unresolved_failure(work, tasks).is_none()
}

pub(super) fn unresolved_failure<'task>(work: &Work, tasks: &'task [Task]) -> Option<&'task Task> {
    ordered_actions(work, tasks)
        .into_iter()
        .find(|task| task.status == TaskStatus::Failed && !failure_resolved(tasks, &task.id))
}

fn failure_resolved(tasks: &[Task], failed_id: &str) -> bool {
    tasks
        .iter()
        .filter(|task| {
            task.is_develop_action()
                && task.payload.get("caused_by").map(String::as_str) == Some(failed_id)
        })
        .any(|task| match task.status {
            TaskStatus::Passed | TaskStatus::Cancelled => true,
            TaskStatus::Failed => failure_resolved(tasks, &task.id),
            TaskStatus::Pending | TaskStatus::Running | TaskStatus::Blocked => false,
        })
}

fn ordered_actions<'task>(work: &Work, tasks: &'task [Task]) -> Vec<&'task Task> {
    ordered_action_ids(work, tasks)
        .iter()
        .filter_map(|id| tasks.iter().find(|task| task.id == *id))
        .collect()
}

fn ordered_action_ids(work: &Work, tasks: &[Task]) -> Vec<String> {
    let develop_ids = tasks
        .iter()
        .filter(|task| task.is_develop_action())
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut sequence = work
        .development_sequence
        .iter()
        .filter(|id| develop_ids.contains(id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let included = sequence.iter().cloned().collect::<BTreeSet<_>>();
    sequence.extend(
        develop_ids
            .into_iter()
            .filter(|id| !included.contains(*id))
            .map(str::to_owned),
    );
    sequence
}

fn insert_at(
    sequence: &mut Vec<String>,
    id: String,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<(), Error> {
    let position = if let Some(target) = before {
        sequence
            .iter()
            .position(|candidate| candidate == target)
            .ok_or(Error::InvalidTaskPosition)?
    } else if let Some(target) = after {
        sequence
            .iter()
            .position(|candidate| candidate == target)
            .map(|index| index + 1)
            .ok_or(Error::InvalidTaskPosition)?
    } else {
        sequence.len()
    };
    sequence.insert(position, id);
    Ok(())
}

fn action_position(work: &Work, tasks: &[Task], id: &str) -> usize {
    ordered_action_ids(work, tasks)
        .iter()
        .position(|candidate| candidate == id)
        .map_or(0, |index| index + 1)
}

fn task_number(id: &str) -> Option<u32> {
    id.strip_prefix("TASK_")?.parse().ok()
}

fn require_action<'task>(tasks: &'task [Task], id: &str) -> Result<&'task Task, Error> {
    tasks
        .iter()
        .find(|task| task.id == id && task.is_develop_action())
        .ok_or_else(|| Error::MissingTask(id.to_owned()))
}

fn pending_action_index(tasks: &[Task], id: &str) -> Result<usize, Error> {
    tasks
        .iter()
        .position(|task| {
            task.id == id && task.is_develop_action() && task.status == TaskStatus::Pending
        })
        .ok_or_else(|| Error::TaskNotPending(id.to_owned()))
}

fn running_action_index(tasks: &[Task], id: &str) -> Result<usize, Error> {
    tasks
        .iter()
        .position(|task| {
            task.id == id && task.is_develop_action() && task.status == TaskStatus::Running
        })
        .ok_or_else(|| Error::TaskNotRunning(id.to_owned()))
}

fn ensure_source(work: &Work, status: &WorktreeStatus) -> Result<(), Error> {
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

fn effective_checkpoint(work: &Work) -> &str {
    work.latest_checkpoint
        .as_deref()
        .unwrap_or(&work.starting_source)
}

fn record_git_state(
    task: &mut Task,
    prefix: &str,
    status: &WorktreeStatus,
    operation: Option<Operation>,
) {
    task.payload
        .insert(format!("{prefix}_head"), status.head().as_str().to_owned());
    task.payload
        .insert(format!("{prefix}_staged"), paths(status.staged()));
    task.payload
        .insert(format!("{prefix}_unstaged"), paths(status.unstaged()));
    task.payload
        .insert(format!("{prefix}_untracked"), paths(status.untracked()));
    task.payload
        .insert(format!("{prefix}_conflicted"), paths(status.conflicted()));
    if let Some(operation) = operation {
        task.payload.insert(
            format!("{prefix}_operation"),
            match operation {
                Operation::Rebase => "rebase",
                Operation::Merge => "merge",
                Operation::CherryPick => "cherry-pick",
            }
            .to_owned(),
        );
    }
}

fn required(value: &str) -> Result<String, Error> {
    if value.trim().is_empty() {
        Err(Error::EmptyField)
    } else {
        Ok(value.to_owned())
    }
}

fn paths(paths: &BTreeSet<Utf8PathBuf>) -> String {
    none(
        &paths
            .iter()
            .map(Utf8PathBuf::as_path)
            .map(rapport_files::Utf8Path::as_str)
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn none(value: &str) -> String {
    if value.is_empty() {
        "none".to_owned()
    } else {
        value.to_owned()
    }
}

fn short(value: &str) -> String {
    value.chars().take(12).collect()
}

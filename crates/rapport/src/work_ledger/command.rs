//! Phase 3 Work CLI and workflow operations.

use super::Error;
use super::domain::{RequestKind, RequestSource, Task, TaskStatus, Work, Workflow};
use super::repository::Store;
use crate::context::{Clock, CommandContext};
use clap::{ArgGroup, Args, Subcommand};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::{Git, ObjectId, Operation, RebaseOutcome, Repository, Revision, WorktreeStatus};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::io::Write;
use std::process::ExitCode;
use std::str::FromStr;

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Action,
}

impl fmt::Debug for Cli {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkCli")
            .field("action", &self.command.name())
            .finish()
    }
}

#[derive(Subcommand)]
enum Action {
    /// Start Work from exactly one durable request source.
    Start(StartArgs),
    /// Derive the complete current Work state.
    Status,
    /// Inspect the Task ledger.
    Task(TaskArgs),
    /// Commit intentionally staged changes as a checkpoint.
    Checkpoint(CheckpointArgs),
    /// Rebase the source branch onto its current target.
    Rebase(RebaseArgs),
    /// Complete Work without Integration.
    Complete {
        #[arg(long)]
        result: String,
    },
    /// Stop tracking Work without claiming completion.
    Abandon {
        #[arg(long)]
        reason: String,
    },
}

impl Action {
    fn name(&self) -> &'static str {
        match self {
            Self::Start(_) => "start",
            Self::Status => "status",
            Self::Task(_) => "task",
            Self::Checkpoint(_) => "checkpoint",
            Self::Rebase(_) => "rebase",
            Self::Complete { .. } => "complete",
            Self::Abandon { .. } => "abandon",
        }
    }
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("request")
        .required(true)
        .multiple(false)
        .args(["ticket", "plan", "ad_hoc"])
))]
struct StartArgs {
    #[arg(long)]
    ticket: Option<String>,
    #[arg(long)]
    plan: Option<Utf8PathBuf>,
    #[arg(long)]
    ad_hoc: Option<String>,
    #[arg(long)]
    title: String,
    #[arg(long, required_unless_present = "ad_hoc", conflicts_with = "ad_hoc")]
    description: Option<String>,
    #[arg(long)]
    target: Option<String>,
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct TaskArgs {
    #[command(subcommand)]
    command: TaskAction,
}

#[derive(Subcommand)]
enum TaskAction {
    /// List and filter Tasks.
    List {
        #[arg(long)]
        status: Vec<String>,
        #[arg(long = "type")]
        task_type: Vec<String>,
        #[arg(long)]
        workflow: Vec<String>,
        #[arg(long)]
        related_to: Option<String>,
        #[arg(long)]
        since_checkpoint: bool,
        #[arg(long)]
        all: bool,
    },
    /// Show one complete Task envelope.
    Show { id: String },
    /// Show the next action without executing it.
    Next,
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct CheckpointArgs {
    #[command(subcommand)]
    command: CheckpointAction,
}

#[derive(Subcommand)]
enum CheckpointAction {
    Start,
    Complete {
        summary: String,
        #[arg(long)]
        description: Option<String>,
    },
    Cancel {
        #[arg(long)]
        reason: String,
    },
}

#[derive(Args)]
#[command(arg_required_else_help = true)]
struct RebaseArgs {
    #[command(subcommand)]
    command: RebaseAction,
}

#[derive(Subcommand)]
enum RebaseAction {
    Start,
    Continue,
    Abort {
        #[arg(long)]
        reason: String,
    },
}

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
        Action::Status => status(context),
        Action::Task(args) => task(&args.command, context),
        Action::Checkpoint(args) => checkpoint(&args.command, context),
        Action::Rebase(args) => rebase(&args.command, context),
        Action::Complete { result } => end_work(context, result, true),
        Action::Abandon { reason } => end_work(context, reason, false),
    }
}

fn git_repository(repo_root: &Utf8Path) -> Result<(Git, Repository), Error> {
    let git = Git::default();
    let repository = git.discover(repo_root)?;
    Ok((git, repository))
}

fn target_revision(
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
    let (git, repository) = git_repository(&context.repo_root)?;
    let live = git.status(&repository)?;
    let (target, target_head) = target_revision(&git, &repository, &work.target_branch)?;
    let changes = git.source_side_changes(&repository, &target)?;
    let signoffs = crate::policy_context::required_signoffs_for_paths(
        context.fs,
        &context.repo_root,
        changes.paths().iter().map(Utf8PathBuf::as_path),
    )?;
    let operation = git.operation(&repository)?;
    let build_proof = workflow_state(&tasks, Workflow::Build);
    let review_proof = workflow_state(&tasks, Workflow::Review);
    let blockers = integration_blockers(&work, &tasks, &live, operation);
    let next = select_next(&tasks).map_or_else(
        || next_workflow(&work, &live),
        |task| {
            task.continuation
                .clone()
                .unwrap_or_else(|| "inspect Task".to_owned())
        },
    );
    Ok(format!(
        "# rapport work status\n\n- `work` — {}\n- `title` — {}\n- `description` — {}\n- `request` — {:?} {}\n- `source` — {} @ {}\n- `current branch` — {}\n- `target` — {} @ {}\n- `starting source` — {}\n- `starting target` — {}\n- `latest checkpoint` — {}\n- `contains target` — {}\n- `staged` — {}\n- `unstaged` — {}\n- `untracked` — {}\n- `conflicted` — {}\n- `operation` — {}\n- `candidate files` — {}\n- `required signoffs` — {}\n- `tasks` — {}\n- `task state` — {}\n- `Build proof` — {}\n- `Review proof` — {}\n- `integration blockers` — {}\n- `next` — `{}`",
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
        build_proof,
        review_proof,
        blockers,
        next
    ))
}

fn task<F, C, O, E>(
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
            if let Some(task) = select_next(&tasks) {
                return Ok(render_task(&work, task));
            }
            let (git, repository) = git_repository(&context.repo_root)?;
            let live = git.status(&repository)?;
            Ok(format!(
                "# rapport work task next\n\n- `work` — {}\n- `description` — {}\n- `next workflow` — `{}`",
                work.title,
                work.description,
                next_workflow(&work, &live)
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

fn checkpoint<F, C, O, E>(
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
        .as_deref()
        .unwrap_or(&work.starting_source)
        .to_owned();
    if prior == live.head().as_str() {
        return Err(Error::EmptyCheckpoint);
    }
    let prior_revision = Revision::new(prior.clone())?;
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
    task.payload.insert("prior_commit".to_owned(), prior);
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
    work.latest_checkpoint = Some(live.head().as_str().to_owned());
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
            work.latest_checkpoint = Some(head.as_str().to_owned());
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

fn rebase<F, C, O, E>(
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
        if work.latest_checkpoint.as_deref() != Some(live.head().as_str()) {
            return Err(Error::UncheckpointedHead);
        }
    }
    work.outcome = Some(format!(
        "{} at {}: {}",
        if completed { "completed" } else { "abandoned" },
        context.clock.now_rfc3339(),
        outcome
    ));
    let remaining = paths(&live.all_changed_paths());
    let history = store.archive(context.fs, &work, &tasks)?;
    Ok(format!(
        "# rapport work {}\n\n- `work` — {}\n- `outcome` — {}\n- `remaining Git changes` — {}\n- `history` — {}\n- `Git state changed` — false",
        if completed { "complete" } else { "abandon" },
        work.id,
        outcome,
        remaining,
        history
    ))
}

fn select_next(tasks: &[Task]) -> Option<&Task> {
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
            (priority, task.id.as_str())
        })
}

fn next_workflow(work: &Work, live: &WorktreeStatus) -> String {
    let checkpoint = work
        .latest_checkpoint
        .as_deref()
        .unwrap_or(&work.starting_source);
    if !live.all_changed_paths().is_empty() || checkpoint != live.head().as_str() {
        "rapport work checkpoint start".to_owned()
    } else if work.latest_checkpoint.as_deref() == Some(live.head().as_str()) {
        "rapport build".to_owned()
    } else {
        "make the requested changes, then rapport work checkpoint start".to_owned()
    }
}

fn ensure_no_active(tasks: &[Task], task_type: &str) -> Result<(), Error> {
    if tasks
        .iter()
        .any(|task| task.kind == task_type && !task.status.is_terminal())
    {
        Err(Error::ActiveTask(task_type.to_owned()))
    } else {
        Ok(())
    }
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

fn active_task(tasks: &[Task], task_type: &str) -> Result<usize, Error> {
    tasks
        .iter()
        .position(|task| task.kind == task_type && !task.status.is_terminal())
        .ok_or_else(|| Error::MissingTask(format!("active {task_type}")))
}

fn change_snapshot(
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

fn tasks_since_checkpoint(tasks: &[Task]) -> Vec<String> {
    let checkpoint = tasks
        .iter()
        .rposition(|task| task.kind == "checkpoint" && task.status == TaskStatus::Passed)
        .map_or(0, |index| index + 1);
    tasks[checkpoint..]
        .iter()
        .map(|task| task.id.clone())
        .collect()
}

fn object_ids(ids: &[ObjectId]) -> String {
    ids.iter()
        .map(ObjectId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
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

fn workflow_state(tasks: &[Task], workflow: Workflow) -> String {
    tasks
        .iter()
        .rev()
        .find(|task| task.workflow == workflow)
        .map_or_else(
            || "missing".to_owned(),
            |task| format!("{} ({})", task.status, task.id),
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
    if work.latest_checkpoint.as_deref() != Some(live.head().as_str()) {
        blockers.push("source HEAD is not the latest checkpoint");
    }
    if tasks.iter().any(|task| !task.status.is_terminal()) {
        blockers.push("nonterminal Tasks remain");
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

fn required(value: String) -> Result<String, Error> {
    if value.trim().is_empty() {
        Err(Error::EmptyField)
    } else {
        Ok(value)
    }
}

fn operation_name(operation: Operation) -> String {
    match operation {
        Operation::Rebase => "rebase",
        Operation::Merge => "merge",
        Operation::CherryPick => "cherry-pick",
    }
    .to_owned()
}

fn paths(paths: &BTreeSet<Utf8PathBuf>) -> String {
    none(
        &paths
            .iter()
            .map(|path| path.as_str())
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

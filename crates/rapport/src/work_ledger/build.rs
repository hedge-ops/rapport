//! Build feedback and acceptance proof recorded in the Work Task ledger.
//!
//! This module owns Build command execution and proof transitions; repository Just targets own build behavior.

use super::Error;
use super::develop;
use super::domain::{
    BuildMode, BuildOperation, BuildOperationStatus, BuildTask, GitState, Task, TaskStatus, Work,
    Workflow,
};
use super::repository::Store;
use crate::context::{Clock, CommandContext};
use crate::runner::{CommandRunner, CommandSpec as LegacyCommandSpec};
use clap::{Args, Subcommand};
use rapport_command::{
    BatchRunner, CommandOutcome, CommandSpec, Job, JobEvent, MachineResources, ResourceKey, Runner,
};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::{Git, WorktreeStatus};
use std::collections::BTreeSet;
use std::io;
use std::num::NonZeroUsize;
use std::process::ExitCode;
use std::time::Instant;

#[derive(Debug, Args)]
pub(crate) struct Cli {
    /// Inspect current or historical Build proof.
    #[command(subcommand)]
    command: Option<Action>,
    /// Repository path for ad hoc development feedback.
    #[arg(value_name = "PATH")]
    path: Option<Utf8PathBuf>,
    /// Finite Just target to run instead of `dev`.
    #[arg(long, requires = "path")]
    target: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Show aggregate Build completion or one Build Task.
    Status { task_id: Option<String> },
}

pub(crate) fn run<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let result = match &cli.command {
        Some(Action::Status { task_id }) => status(context, task_id.as_deref()),
        None if cli.path.is_some() || cli.target.is_some() => feedback(
            context,
            cli.path.as_deref(),
            cli.target.as_deref().unwrap_or("dev"),
        ),
        None => {
            let store = Store::new(&context.repo_root);
            match store.load_work(context.fs) {
                Ok(Some(_)) => acceptance(context),
                Ok(None) => feedback(context, None, "dev"),
                Err(error) => Err(error),
            }
        }
    };
    match result {
        Ok(output) => {
            let _ = writeln!(context.out, "{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "# rapport build\n\n{error}");
            ExitCode::from(2)
        }
    }
}

fn acceptance<F, C, O, E>(context: &mut CommandContext<'_, F, C, O, E>) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    let initial = git.status(&repository)?;
    ensure_source(&work, &initial)?;
    if super::integrate::published_candidate(&tasks) != Some(initial.head().as_str()) {
        return Err(Error::MissingIntegration);
    }
    let operation = git.operation(&repository)?;
    if !develop::is_complete(&work, &tasks, &initial, operation) {
        return Err(Error::BuildDevelopIncomplete);
    }
    if !initial.is_clean() {
        return Err(Error::DirtyWorktree);
    }

    let (target, _) = super::command::target_revision(&git, &repository, &work.target_branch)?;
    let changes = git.source_side_changes(&repository, &target)?;
    let changed_paths = changes.paths().iter().cloned().collect::<Vec<_>>();
    let policy_digest = crate::policy_context::effective_policy_digest_for_paths(
        context.fs,
        &context.repo_root,
        changed_paths.iter().map(Utf8PathBuf::as_path),
    )?;
    let signoffs = crate::policy_context::required_signoffs_for_paths(
        context.fs,
        &context.repo_root,
        changed_paths.iter().map(Utf8PathBuf::as_path),
    )?;
    let now = context.clock.now_rfc3339();
    let id = work.allocate_task_id()?;
    let operations = signoffs
        .into_iter()
        .map(|signoff| BuildOperation {
            id: signoff.id,
            context: Some(signoff.source_context),
            working_directory: signoff.working_directory,
            target: signoff.target,
            triggers: signoff.triggers,
            identity: Some(signoff.identity),
            stage: signoff.stage,
            resource_group: signoff.resource_group,
            contract_digest: Some(signoff.contract_digest),
            status: BuildOperationStatus::Waiting,
            started_at: None,
            completed_at: None,
            duration_seconds: None,
            exit_status: None,
            stdout: String::new(),
            stderr: String::new(),
            proof: false,
        })
        .collect::<Vec<_>>();
    let mut task = Task::new(
        id,
        "build",
        Workflow::Build,
        "Build acceptance proof",
        "Run every Context signoff required by the exact candidate.",
        "rapport build",
        TaskStatus::Running,
        initial.head().as_str(),
        &now,
        Some("rapport build status".to_owned()),
    );
    task.payload.insert("started_at".to_owned(), now);
    task.build = Some(BuildTask {
        mode: BuildMode::Acceptance,
        candidate: initial.head().as_str().to_owned(),
        policy_digest: Some(policy_digest),
        initial_git: git_state(&initial),
        final_git: None,
        operations,
        proof: false,
    });
    store.save_work_and_task(context.fs, &work, &task)?;

    run_acceptance_stages(context, &store, &mut task)?;

    let final_status = git.status(&repository)?;
    finalize_acceptance(
        context.fs,
        context.clock,
        &store,
        work,
        task,
        initial.head().as_str(),
        &final_status,
    )
}

fn finalize_acceptance(
    fs: &mut impl FileSystem,
    clock: &impl Clock,
    store: &Store,
    mut work: Work,
    mut task: Task,
    candidate: &str,
    final_status: &WorktreeStatus,
) -> Result<String, Error> {
    let candidate_changed = final_status.head().as_str() != candidate;
    let generated_changes = !final_status.is_clean() || candidate_changed;
    let (failed_operations, operation_count) = {
        let build = task
            .build
            .as_mut()
            .ok_or_else(|| Error::BuildExecution("Build Task lost its typed payload".to_owned()))?;
        build.final_git = Some(git_state(final_status));
        (
            build
                .operations
                .iter()
                .filter(|operation| operation.status == BuildOperationStatus::Failed)
                .map(|operation| (operation.id.clone(), operation.identity.clone()))
                .collect::<Vec<_>>(),
            build.operations.len(),
        )
    };
    let passed = failed_operations.is_empty() && !generated_changes;
    if passed {
        let build = task
            .build
            .as_mut()
            .ok_or_else(|| Error::BuildExecution("Build Task lost its typed payload".to_owned()))?;
        for operation in &mut build.operations {
            operation.proof = true;
        }
        build.proof = true;
        task.finish(
            TaskStatus::Passed,
            clock.now_rfc3339(),
            "all required signoffs passed for the exact candidate".to_owned(),
            None,
        );
        store.save_task(fs, &task)?;
        return Ok(format!(
            "# rapport build\n\n- `task` — {}\n- `mode` — acceptance\n- `candidate` — {}\n- `operations` — {}\n- `status` — passed\n- `proof` — current\n- `next` — `rapport review start`",
            task.id,
            short(candidate),
            operation_count
        ));
    }

    let build = task
        .build
        .as_mut()
        .ok_or_else(|| Error::BuildExecution("Build Task lost its typed payload".to_owned()))?;
    for operation in &mut build.operations {
        operation.proof = false;
    }
    build.proof = false;
    task.finish(
        TaskStatus::Failed,
        clock.now_rfc3339(),
        if generated_changes {
            "Build changed the candidate; Develop must reconcile generated changes".to_owned()
        } else {
            "one or more required signoffs failed".to_owned()
        },
        None,
    );
    let mut corrective = failed_operations
        .iter()
        .map(|(operation, identity)| {
            corrective_task(
                &mut work,
                &task,
                format!("Repair failed Build signoff {operation}"),
                format!(
                    "Make {} pass for the candidate using the appropriate engineering correction. If repository state changes, checkpoint the correction before completing this Task. Record the correction and passing evidence in the completion result.",
                    identity.as_deref().unwrap_or(operation)
                ),
                operation,
                clock.now_rfc3339(),
            )
        })
        .collect::<Result<Vec<_>, Error>>()?;
    if generated_changes {
        corrective.push(corrective_task(
            &mut work,
            &task,
            "Reconcile build-generated changes".to_owned(),
            generated_change_description(candidate, final_status),
            "generated_changes",
            clock.now_rfc3339(),
        )?);
    }
    if !corrective.is_empty() {
        work.develop_completed_checkpoint = None;
    }
    task.related
        .extend(corrective.iter().map(|corrective| corrective.id.clone()));
    let mut writes = vec![task.clone()];
    writes.extend(corrective);
    store.save_work_and_tasks(fs, &work, &writes)?;
    Err(Error::BuildFailed(task.id))
}

fn generated_change_description(candidate: &str, final_status: &WorktreeStatus) -> String {
    if final_status.head().as_str() == candidate {
        format!(
            "Inspect, checkpoint, or prevent generated changes: {}.",
            display_paths(&final_status.all_changed_paths())
        )
    } else {
        format!(
            "Inspect the Build-created candidate change from {} to {} and restore the intended checkpoint.",
            short(candidate),
            short(final_status.head().as_str())
        )
    }
}

fn run_acceptance_stages<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &Store,
    task: &mut Task,
) -> Result<(), Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let stages = task
        .build
        .as_ref()
        .map(|build| {
            build
                .operations
                .iter()
                .map(|operation| operation.stage)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let parallelism = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let batch = BatchRunner::new(parallelism, MachineResources::rapport_default());
    let adapter = LegacyRunner {
        inner: context.runner,
    };
    for stage in stages {
        let jobs = {
            let build = task.build.as_mut().ok_or_else(|| {
                Error::BuildExecution("Build Task lost its typed payload".to_owned())
            })?;
            build
                .operations
                .iter_mut()
                .filter(|operation| operation.stage == stage)
                .map(|operation| {
                    let command = CommandSpec::new("just")
                        .arg(&operation.target)
                        .current_dir(context.repo_root.join(&operation.working_directory));
                    let job = Job::new(&operation.id, command);
                    if let Some(resource) = &operation.resource_group {
                        ResourceKey::new(resource.clone())
                            .map(|key| job.requiring(key))
                            .map_err(|_| {
                                Error::BuildExecution(format!(
                                    "invalid resource group `{resource}`"
                                ))
                            })
                    } else {
                        Ok(job)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        store.save_task(context.fs, task)?;
        let mut persistence_error = None;
        let _ = batch.run_with_events(&adapter, jobs, |event| {
            update_operation(task, event, context.clock);
            if persistence_error.is_none()
                && let Err(error) = store.save_task(context.fs, task)
            {
                persistence_error = Some(error);
            }
        });
        if let Some(error) = persistence_error {
            return Err(error);
        }
        let stage_failed = task.build.as_ref().is_some_and(|build| {
            build.operations.iter().any(|operation| {
                operation.stage == stage && operation.status == BuildOperationStatus::Failed
            })
        });
        if stage_failed {
            if let Some(build) = task.build.as_mut() {
                for operation in &mut build.operations {
                    if operation.stage > stage && operation.status == BuildOperationStatus::Waiting
                    {
                        operation.status = BuildOperationStatus::Blocked;
                    }
                }
            }
            store.save_task(context.fs, task)?;
            break;
        }
    }
    Ok(())
}

fn update_operation(task: &mut Task, event: &JobEvent, clock: &impl Clock) {
    let Some(build) = task.build.as_mut() else {
        return;
    };
    let Some(operation) = build
        .operations
        .iter_mut()
        .find(|operation| operation.id == event.name())
    else {
        return;
    };
    let Some(outcome) = event.outcome() else {
        operation.status = BuildOperationStatus::Running;
        operation.started_at = Some(clock.now_rfc3339());
        return;
    };
    operation.completed_at = Some(clock.now_rfc3339());
    match outcome.result() {
        Ok(result) => {
            operation.status = if result.success() {
                BuildOperationStatus::Passed
            } else {
                BuildOperationStatus::Failed
            };
            operation.duration_seconds = Some(result.elapsed().as_secs());
            operation.exit_status = result.exit_code();
            operation.stdout = result.stdout_lossy();
            operation.stderr = result.stderr_lossy();
        }
        Err(error) => {
            operation.status = BuildOperationStatus::Failed;
            operation.stderr = error.to_string();
        }
    }
}

fn feedback<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    path: Option<&Utf8Path>,
    target: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    validate_finite_target(target)?;
    let directory = resolve_build_directory(&context.repo_root, &context.cwd, path)?;
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    let initial = git.status(&repository)?;
    let store = Store::new(&context.repo_root);
    let active = store.load_work(context.fs)?;
    let mut task_and_work = if let Some(mut work) = active {
        let relative = directory
            .strip_prefix(&context.repo_root)
            .unwrap_or(&directory);
        let policy_digest = crate::policy_context::effective_policy_digest_for_paths(
            context.fs,
            &context.repo_root,
            std::iter::once(relative),
        )?;
        let task = new_feedback_task(
            &mut work,
            &initial,
            &context.repo_root,
            &directory,
            target,
            policy_digest,
            context.clock,
        )?;
        store.save_work_and_task(context.fs, &work, &task)?;
        Some((work, task))
    } else {
        None
    };

    let started = Instant::now();
    let outcome = match context
        .runner
        .run(&LegacyCommandSpec::new("just", [target]), &directory)
    {
        Ok(outcome) => outcome,
        Err(error) if task_and_work.is_some() => crate::runner::CommandOutcome {
            success: false,
            stdout: String::new(),
            stderr: format!("could not invoke Just: {error}"),
        },
        Err(error) => return Err(Error::BuildExecution(error.to_string())),
    };
    let final_status = git.status(&repository)?;
    if let Some((_, task)) = task_and_work.as_mut() {
        let build = task
            .build
            .as_mut()
            .ok_or_else(|| Error::BuildExecution("Build Task lost its typed payload".to_owned()))?;
        let operation = build.operations.first_mut().ok_or_else(|| {
            Error::BuildExecution("feedback Build Task has no operation".to_owned())
        })?;
        operation.status = if outcome.success {
            BuildOperationStatus::Passed
        } else {
            BuildOperationStatus::Failed
        };
        operation.completed_at = Some(context.clock.now_rfc3339());
        operation.duration_seconds = Some(started.elapsed().as_secs());
        operation.exit_status = Some(i32::from(!outcome.success));
        operation.stdout.clone_from(&outcome.stdout);
        operation.stderr.clone_from(&outcome.stderr);
        build.final_git = Some(git_state(&final_status));
        task.finish(
            if outcome.success {
                TaskStatus::Passed
            } else {
                TaskStatus::Failed
            },
            context.clock.now_rfc3339(),
            format!(
                "development feedback {}",
                if outcome.success { "passed" } else { "failed" }
            ),
            Some(join_output(&outcome.stdout, &outcome.stderr)),
        );
        store.save_task(context.fs, task)?;
    }
    let report = format!(
        "# rapport build\n\n- `mode` — feedback\n- `directory` — {}\n- `target` — {}\n- `status` — {}\n- `proof` — none\n- `generated changes` — {}{}",
        display_relative(&context.repo_root, &directory),
        target,
        if outcome.success { "passed" } else { "failed" },
        display_paths(&final_status.all_changed_paths()),
        task_and_work
            .as_ref()
            .map_or_else(String::new, |(_, task)| format!("\n- `task` — {}", task.id))
    );
    if outcome.success {
        Ok(report)
    } else if let Some((_, task)) = task_and_work {
        Err(Error::BuildFailed(task.id))
    } else {
        Err(Error::AdHocBuildFailed)
    }
}

fn new_feedback_task(
    work: &mut Work,
    initial: &WorktreeStatus,
    repo_root: &Utf8Path,
    directory: &Utf8Path,
    target: &str,
    policy_digest: String,
    clock: &impl Clock,
) -> Result<Task, Error> {
    let id = work.allocate_task_id()?;
    let mut task = Task::new(
        id,
        "build",
        Workflow::Build,
        format!("Build feedback: just {target}"),
        "Run finite repository feedback without creating proof.",
        "rapport build",
        TaskStatus::Running,
        initial.head().as_str(),
        clock.now_rfc3339(),
        None,
    );
    task.build = Some(BuildTask {
        mode: BuildMode::Feedback,
        candidate: initial.head().as_str().to_owned(),
        policy_digest: Some(policy_digest),
        initial_git: git_state(initial),
        final_git: None,
        operations: vec![BuildOperation {
            id: "feedback".to_owned(),
            context: None,
            working_directory: display_relative(repo_root, directory),
            target: target.to_owned(),
            triggers: vec![display_relative(repo_root, directory)],
            identity: None,
            stage: 0,
            resource_group: None,
            contract_digest: None,
            status: BuildOperationStatus::Running,
            started_at: Some(clock.now_rfc3339()),
            completed_at: None,
            duration_seconds: None,
            exit_status: None,
            stdout: String::new(),
            stderr: String::new(),
            proof: false,
        }],
        proof: false,
    });
    Ok(task)
}

fn status<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    task_id: Option<&str>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let store = Store::new(&context.repo_root);
    let work = store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    if let Some(id) = task_id {
        let task = tasks
            .iter()
            .find(|task| task.id == id && task.workflow == Workflow::Build)
            .ok_or_else(|| Error::MissingTask(id.to_owned()))?;
        return Ok(render_task(task));
    }
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    let live = git.status(&repository)?;
    ensure_source(&work, &live)?;
    let (target, _) = super::command::target_revision(&git, &repository, &work.target_branch)?;
    let changes = git.source_side_changes(&repository, &target)?;
    let paths = changes.paths().iter().cloned().collect::<Vec<_>>();
    let digest = crate::policy_context::effective_policy_digest_for_paths(
        context.fs,
        &context.repo_root,
        paths.iter().map(Utf8PathBuf::as_path),
    )?;
    let required = crate::policy_context::required_signoffs_for_paths(
        context.fs,
        &context.repo_root,
        paths.iter().map(Utf8PathBuf::as_path),
    )?;
    let develop_complete = develop::is_complete(&work, &tasks, &live, git.operation(&repository)?);
    let complete = current_proof(&tasks, live.head().as_str(), &digest, &required);
    let latest = tasks
        .iter()
        .rev()
        .find(|task| task.workflow == Workflow::Build)
        .map_or("none", |task| task.id.as_str());
    Ok(format!(
        "# rapport build status\n\n- `candidate` — {}\n- `policy digest` — {}\n- `Develop` — {}\n- `required signoffs` — {}\n- `latest Build Task` — {}\n- `Build` — {}\n- `proof` — {}\n- `next` — `{}`",
        short(live.head().as_str()),
        &digest[..12.min(digest.len())],
        if develop_complete {
            "complete"
        } else {
            "incomplete"
        },
        none(
            &required
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        latest,
        if complete { "complete" } else { "incomplete" },
        if complete {
            "current"
        } else {
            "missing or stale"
        },
        if !develop_complete {
            "rapport work task next"
        } else if complete {
            "rapport review start"
        } else {
            "rapport build"
        }
    ))
}

pub(super) fn current_proof(
    tasks: &[Task],
    candidate: &str,
    policy_digest: &str,
    required: &[crate::policy_context::RequiredSignoff],
) -> bool {
    tasks.iter().rev().any(|task| {
        let Some(build) = &task.build else {
            return false;
        };
        task.workflow == Workflow::Build
            && task.status == TaskStatus::Passed
            && build.mode == BuildMode::Acceptance
            && build.proof
            && build.candidate == candidate
            && build.policy_digest.as_deref() == Some(policy_digest)
            && required.iter().all(|required| {
                build.operations.iter().any(|operation| {
                    operation.id == required.id
                        && operation.contract_digest.as_deref()
                            == Some(required.contract_digest.as_str())
                        && operation.proof
                        && operation.status == BuildOperationStatus::Passed
                })
            })
            && build.operations.len() == required.len()
    })
}

pub(super) fn has_candidate_proof(tasks: &[Task], candidate: &str) -> bool {
    tasks.iter().rev().any(|task| {
        task.status == TaskStatus::Passed
            && task.build.as_ref().is_some_and(|build| {
                build.mode == BuildMode::Acceptance && build.proof && build.candidate == candidate
            })
    })
}

fn render_task(task: &Task) -> String {
    let Some(build) = &task.build else {
        return format!(
            "# rapport build status\n\n- `task` — {}\n- `typed Build payload` — missing",
            task.id
        );
    };
    let operations = build
        .operations
        .iter()
        .map(|operation| {
            format!(
                "- `{}` — context {} — directory {} — just {} — identity {} — stage {} — resource {} — status {} — exit {} — proof {}\n  - triggers: {}\n  - timing: {} to {} ({}s)\n  - stdout: {}\n  - stderr: {}",
                operation.id,
                operation.context.as_deref().unwrap_or("none"),
                operation.working_directory,
                operation.target,
                operation.identity.as_deref().unwrap_or("none"),
                operation.stage,
                operation.resource_group.as_deref().unwrap_or("none"),
                operation.status,
                operation.exit_status.map_or_else(|| "none".to_owned(), |status| status.to_string()),
                operation.proof,
                none(&operation.triggers.join(", ")),
                operation.started_at.as_deref().unwrap_or("not started"),
                operation.completed_at.as_deref().unwrap_or("not completed"),
                operation.duration_seconds.unwrap_or(0),
                none(operation.stdout.trim()),
                none(operation.stderr.trim()),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# rapport build status\n\n- `task` — {}\n- `mode` — {}\n- `status` — {}\n- `candidate` — {}\n- `policy digest` — {}\n- `initial Git` — {}\n- `final Git` — {}\n- `proof` — {}\n\n## Operations\n\n{}",
        task.id,
        build.mode,
        task.status,
        short(&build.candidate),
        build.policy_digest.as_deref().unwrap_or("none"),
        render_git(&build.initial_git),
        build
            .final_git
            .as_ref()
            .map_or_else(|| "not captured".to_owned(), render_git),
        build.proof,
        none(&operations)
    )
}

fn corrective_task(
    work: &mut Work,
    build: &Task,
    title: String,
    description: String,
    cause: &str,
    created_at: String,
) -> Result<Task, Error> {
    let id = work.allocate_task_id()?;
    work.development_sequence.push(id.clone());
    let mut task = Task::new(
        id.clone(),
        "action",
        Workflow::Develop,
        title,
        description,
        "rapport build",
        TaskStatus::Pending,
        &build.source_commit,
        created_at,
        Some(format!("rapport develop task start {id}")),
    );
    task.related.push(build.id.clone());
    task.payload
        .insert("caused_by_build".to_owned(), build.id.clone());
    task.payload
        .insert("failed_operation".to_owned(), cause.to_owned());
    Ok(task)
}

fn git_state(status: &WorktreeStatus) -> GitState {
    GitState {
        head: status.head().as_str().to_owned(),
        staged: status.staged().iter().map(ToString::to_string).collect(),
        unstaged: status.unstaged().iter().map(ToString::to_string).collect(),
        untracked: status.untracked().iter().map(ToString::to_string).collect(),
        conflicted: status
            .conflicted()
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

fn ensure_source(work: &Work, status: &WorktreeStatus) -> Result<(), Error> {
    let actual = status.branch();
    if actual == Some(&work.source_branch) {
        Ok(())
    } else {
        Err(Error::SourceBranchChanged {
            expected: work.source_branch.as_str().to_owned(),
            actual: actual
                .map_or("detached", rapport_git::BranchName::as_str)
                .to_owned(),
        })
    }
}

fn resolve_build_directory(
    repo_root: &Utf8Path,
    cwd: &Utf8Path,
    path: Option<&Utf8Path>,
) -> Result<Utf8PathBuf, Error> {
    if path.is_some_and(|path| {
        path.components()
            .any(|component| component.as_str() == "..")
    }) {
        return Err(Error::InvalidBuildPath);
    }
    let directory = match path {
        None => cwd.to_path_buf(),
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => cwd.join(path),
    };
    if !directory.starts_with(repo_root) {
        return Err(Error::InvalidBuildPath);
    }
    Ok(directory)
}

fn validate_finite_target(target: &str) -> Result<(), Error> {
    if matches!(target, "serve" | "start" | "open" | "run") {
        return Err(Error::InteractiveBuildTarget(target.to_owned()));
    }
    if target.is_empty()
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(Error::BuildExecution(format!(
            "invalid Just target `{target}`"
        )));
    }
    Ok(())
}

fn display_relative(root: &Utf8Path, path: &Utf8Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.as_str().is_empty() {
        ".".to_owned()
    } else {
        relative.to_string()
    }
}

fn display_paths(paths: &BTreeSet<Utf8PathBuf>) -> String {
    none(
        &paths
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    )
    .to_owned()
}

fn render_git(state: &GitState) -> String {
    format!(
        "{}; staged {}; unstaged {}; untracked {}; conflicted {}",
        short(&state.head),
        none(&state.staged.join(", ")),
        none(&state.unstaged.join(", ")),
        none(&state.untracked.join(", ")),
        none(&state.conflicted.join(", "))
    )
}

fn join_output(stdout: &str, stderr: &str) -> String {
    [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn short(value: &str) -> &str {
    &value[..value.len().min(12)]
}

fn none(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

struct LegacyRunner<'runner> {
    inner: &'runner dyn CommandRunner,
}

impl Runner for LegacyRunner<'_> {
    fn run(&self, spec: &CommandSpec) -> io::Result<CommandOutcome> {
        let directory = spec.working_directory().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "missing working directory")
        })?;
        let directory = Utf8Path::from_path(directory).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "non-UTF-8 working directory")
        })?;
        let started = Instant::now();
        let outcome = self.inner.run(
            &LegacyCommandSpec::new(spec.program(), spec.arguments().iter().map(String::as_str)),
            directory,
        )?;
        Ok(CommandOutcome::new(
            outcome.success,
            Some(i32::from(!outcome.success)),
            outcome.stdout.into_bytes(),
            outcome.stderr.into_bytes(),
            started.elapsed(),
        ))
    }
}

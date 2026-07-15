//! Independent feedback and acceptance Review recorded in the Work ledger.
//!
//! This module owns Review requests, result validation, findings, corrective Tasks, and acceptance proof.

use super::domain::{
    FindingStatus, ReviewMode, ReviewResult, ReviewTask, ReviewUnit, Task, TaskStatus, Work,
    Workflow,
};
use super::grade::ReviewGrade;
use super::repository::Store;
use super::{Error, build, develop};
use crate::{Clock, CommandContext};
use clap::{ArgGroup, Args, Subcommand};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::{Git, Repository, WorktreeStatus};
use rapport_prose::OutputBuilder;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io;
use std::process::ExitCode;
use std::str::FromStr;

const CATEGORIES: [&str; 7] = [
    "Intent and correctness",
    "Architecture and boundaries",
    "Rules and code quality",
    "Tests and reliability",
    "Security and privacy",
    "Documentation and operability",
    "Compatibility and dependencies",
];

#[derive(Debug, Args)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    Start {
        path: Option<Utf8PathBuf>,
    },
    Complete {
        #[arg(long)]
        result: Utf8PathBuf,
    },
    #[command(group(ArgGroup::new("decision").required(true).args(["accept", "dismiss"])))]
    Reconcile {
        finding: String,
        #[arg(long)]
        accept: bool,
        #[arg(long)]
        dismiss: bool,
        #[arg(long, requires = "dismiss")]
        reason: Option<String>,
    },
    Override {
        #[arg(long)]
        reason: String,
    },
    Cancel {
        #[arg(long)]
        reason: String,
    },
    Status {
        task_id: Option<String>,
    },
}

pub(crate) fn run<F, C, O, E>(cli: &Cli, context: &mut CommandContext<'_, F, C, O, E>) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let result = match &cli.command {
        Action::Start { path } => start(context, path.as_deref()),
        Action::Complete { result } => complete(context, result),
        Action::Reconcile {
            finding,
            accept,
            reason,
            ..
        } => reconcile(context, finding, *accept, reason.as_deref()),
        Action::Override { reason } => override_quality(context, reason),
        Action::Cancel { reason } => cancel(context, reason),
        Action::Status { task_id } => status(context, task_id.as_deref()),
    };
    match result {
        Ok(output) => {
            let _ = writeln!(context.out, "{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(context.err, "# rapport review\n\n{error}");
            ExitCode::from(2)
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "Review start atomically binds request, policy, Git, Build proof, and ledger state"
)]
fn start<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    path: Option<&Utf8Path>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    if path.is_some_and(|path| {
        path.is_absolute()
            || path
                .components()
                .any(|component| component.as_str() == "..")
    }) {
        return Err(Error::InvalidReviewPath);
    }
    let store = Store::new(&context.repo_root);
    let active = store.load_work(context.fs)?;
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    let live = git.status(&repository)?;
    if let Some(mut work) = active {
        let tasks = store.load_tasks(context.fs)?;
        if tasks.iter().any(|task| {
            task.workflow == Workflow::Review
                && matches!(task.status, TaskStatus::Running | TaskStatus::Blocked)
        }) {
            return Err(Error::ActiveReview);
        }
        let mode = if path.is_some() {
            ReviewMode::Feedback
        } else {
            ReviewMode::Acceptance
        };
        let (base, changed_paths) = candidate_paths(&git, &repository, &work, path)?;
        let policy = crate::policy_context::review_policy_for_paths(
            context.fs,
            &context.repo_root,
            changed_paths.iter().map(Utf8PathBuf::as_path),
        )?;
        let digest = crate::policy_context::effective_policy_digest_for_paths(
            context.fs,
            &context.repo_root,
            changed_paths.iter().map(Utf8PathBuf::as_path),
        )?;
        let build_task = if mode == ReviewMode::Acceptance {
            ensure_acceptance_ready(
                context.fs,
                &git,
                &repository,
                &work,
                &tasks,
                &live,
                &digest,
                &changed_paths,
            )?
        } else {
            None
        };
        let request = render_request(
            Some(&work),
            &base,
            live.head().as_str(),
            &policy.markdown,
            build_task.as_deref(),
        );
        let checksum = checksum(&request);
        let request = with_contract(request, &checksum);
        let id = work.allocate_task_id()?;
        let mut task = Task::new(
            id,
            "review",
            Workflow::Review,
            "Independent Review",
            "Review the complete candidate against intent and policy.",
            "rapport review start",
            TaskStatus::Running,
            live.head().as_str(),
            context.clock.now_rfc3339(),
            Some("rapport review complete --result <FILE>".to_owned()),
        );
        if mode == ReviewMode::Feedback
            && let Some(action) = tasks
                .iter()
                .find(|task| task.is_develop_action() && task.status == TaskStatus::Running)
        {
            task.related.push(action.id.clone());
        }
        task.review = Some(ReviewTask {
            mode,
            base,
            candidate: live.head().as_str().to_owned(),
            policy_digest: digest,
            content_digest: super::command::change_snapshot(&repository, &live, context.fs)?,
            reviewed_paths: changed_paths.iter().map(ToString::to_string).collect(),
            build_task,
            minimum_grade: Some(
                ReviewGrade::from_str(&policy.minimum_grade)
                    .map_err(|_| Error::InvalidReviewResult("invalid policy grade".to_owned()))?,
            ),
            rule_ids: policy.rule_ids.into_iter().collect(),
            units: vec![ReviewUnit {
                id: "UNIT_001".to_owned(),
                input_checksum: checksum,
                request: request.clone(),
            }],
            result: None,
            findings: Vec::new(),
            quality_override: None,
            proof: false,
        });
        store.save_work_and_task(context.fs, &work, &task)?;
        Ok(request)
    } else {
        let relative = path.map_or_else(
            || {
                context
                    .cwd
                    .strip_prefix(&context.repo_root)
                    .unwrap_or(Utf8Path::new("."))
            },
            |path| path,
        );
        let policy = crate::policy_context::review_policy_for_paths(
            context.fs,
            &context.repo_root,
            std::iter::once(relative),
        )?;
        let target = git.default_target(&repository)?;
        let (base, _) = super::command::target_revision(&git, &repository, &target)?;
        let request = render_request(
            None,
            base.as_str(),
            live.head().as_str(),
            &policy.markdown,
            None,
        );
        let checksum = checksum(&request);
        Ok(with_contract(request, &checksum))
    }
}

fn complete<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    result_path: &Utf8Path,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let store = Store::new(&context.repo_root);
    if store.load_work(context.fs)?.is_none() {
        let path = if result_path.is_absolute() {
            result_path.to_path_buf()
        } else {
            context.cwd.join(result_path)
        };
        let contents = context
            .fs
            .read_to_string(&path)
            .map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
        let result: ReviewResult = serde_json::from_str(&contents)
            .map_err(|source| Error::ReviewDecode { path, source })?;
        validate_standalone_result(&result)?;
        return Ok(format!(
            "# rapport review complete\n\n- `mode` — feedback\n- `grade` — {}\n- `findings` — {}\n- `persisted` — no",
            result.overall_grade,
            result.proposed_actions.len()
        ));
    }
    let mut work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = current_review(&tasks)?;
    let path = if result_path.is_absolute() {
        result_path.to_path_buf()
    } else {
        context.cwd.join(result_path)
    };
    let contents = context
        .fs
        .read_to_string(&path)
        .map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
    let mut result: ReviewResult =
        serde_json::from_str(&contents).map_err(|source| Error::ReviewDecode { path, source })?;
    let review = tasks[index].review.as_ref().ok_or(Error::MissingReview)?;
    validate_result(&result, review)?;
    revalidate(context, &work, &tasks, review)?;
    if review.mode == ReviewMode::Acceptance {
        for finding in &mut result.proposed_actions {
            finding.id = Some(work.allocate_finding_id()?);
        }
    }
    let grade = result.overall_grade;
    let findings = result.proposed_actions.clone();
    let review = tasks[index].review.as_mut().ok_or(Error::MissingReview)?;
    review.findings = findings;
    review.result = Some(result);
    let c_minus =
        ReviewGrade::from_str("C-").map_err(|_| Error::InvalidReviewResult("grade".to_owned()))?;
    if review.mode == ReviewMode::Feedback {
        tasks[index].finish(
            TaskStatus::Passed,
            context.clock.now_rfc3339(),
            "feedback recorded".to_owned(),
            None,
        );
    } else if !grade.meets(c_minus) {
        tasks[index].finish(
            TaskStatus::Failed,
            context.clock.now_rfc3339(),
            "Review grade cannot be accepted".to_owned(),
            None,
        );
    } else if review.findings.is_empty() && grade.meets(review.minimum_grade.unwrap_or_default()) {
        review.proof = true;
        tasks[index].finish(
            TaskStatus::Passed,
            context.clock.now_rfc3339(),
            "Review passed".to_owned(),
            None,
        );
    } else {
        tasks[index].status = TaskStatus::Blocked;
        tasks[index].continuation =
            Some("rapport review reconcile <FINDING_ID> --accept|--dismiss".to_owned());
    }
    store.save_work_and_task(context.fs, &work, &tasks[index])?;
    Ok(render_task(&tasks[index]))
}

fn reconcile<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    finding_id: &str,
    accept: bool,
    reason: Option<&str>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let store = Store::new(&context.repo_root);
    let mut work = store.require_work(context.fs)?;
    let mut tasks = store.load_tasks(context.fs)?;
    let index = latest_review_with_finding(&tasks, finding_id)?;
    let finding_index = tasks[index]
        .review
        .as_ref()
        .and_then(|review| {
            review.findings.iter().position(|finding| {
                finding.id.as_deref() == Some(finding_id)
                    && finding.status == FindingStatus::Pending
            })
        })
        .ok_or_else(|| Error::MissingFinding(finding_id.to_owned()))?;
    if accept {
        let review_task_id = tasks[index].id.clone();
        let finding = tasks[index]
            .review
            .as_ref()
            .ok_or(Error::MissingReview)?
            .findings[finding_index]
            .clone();
        let id = work.allocate_task_id()?;
        work.development_sequence.push(id.clone());
        work.develop_completed_checkpoint = None;
        let mut corrective = Task::new(
            id.clone(),
            "action",
            Workflow::Develop,
            finding.title,
            finding.recommended_correction,
            "rapport review reconcile",
            TaskStatus::Pending,
            &tasks[index].source_commit,
            context.clock.now_rfc3339(),
            None,
        );
        corrective.related.push(review_task_id);
        corrective
            .payload
            .insert("caused_by_finding".to_owned(), finding_id.to_owned());
        let review = tasks[index].review.as_mut().ok_or(Error::MissingReview)?;
        review.findings[finding_index].status = FindingStatus::Accepted;
        review.findings[finding_index].corrective_task = Some(id);
        review.proof = false;
        tasks[index].status = TaskStatus::Failed;
        tasks[index].result = Some("finding accepted as corrective Develop work".to_owned());
        store.save_work_and_tasks(context.fs, &work, &[tasks[index].clone(), corrective])?;
    } else {
        let reason = required(reason.unwrap_or_default())?;
        let review = tasks[index].review.as_mut().ok_or(Error::MissingReview)?;
        review.findings[finding_index].status = FindingStatus::Dismissed;
        review.findings[finding_index].decision_reason = Some(reason);
        settle_review(&mut tasks[index], context.clock.now_rfc3339())?;
        store.save_task(context.fs, &tasks[index])?;
    }
    Ok(render_task(&tasks[index]))
}

fn override_quality<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    reason: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let reason = required(reason)?;
    let store = Store::new(&context.repo_root);
    let mut tasks = store.load_tasks(context.fs)?;
    let index = tasks
        .iter()
        .rposition(|task| task.workflow == Workflow::Review && task.status == TaskStatus::Blocked)
        .ok_or(Error::MissingReview)?;
    let review = tasks[index].review.as_mut().ok_or(Error::MissingReview)?;
    let result = review.result.as_ref().ok_or(Error::OverrideUnavailable)?;
    let c_minus = ReviewGrade::from_str("C-").map_err(|_| Error::OverrideUnavailable)?;
    if !result.overall_grade.meets(c_minus)
        || review
            .findings
            .iter()
            .any(|f| f.status != FindingStatus::Dismissed)
    {
        return Err(Error::OverrideUnavailable);
    }
    review.quality_override = Some(reason);
    review.proof = true;
    tasks[index].finish(
        TaskStatus::Passed,
        context.clock.now_rfc3339(),
        "passed with quality-policy override".to_owned(),
        None,
    );
    store.save_task(context.fs, &tasks[index])?;
    Ok(render_task(&tasks[index]))
}

fn cancel<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    reason: &str,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let reason = required(reason)?;
    let store = Store::new(&context.repo_root);
    let mut tasks = store.load_tasks(context.fs)?;
    let index = current_review(&tasks)?;
    tasks[index].finish(
        TaskStatus::Cancelled,
        context.clock.now_rfc3339(),
        reason,
        None,
    );
    store.save_task(context.fs, &tasks[index])?;
    Ok(render_task(&tasks[index]))
}

fn status<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    id: Option<&str>,
) -> Result<String, Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let store = Store::new(&context.repo_root);
    store.require_work(context.fs)?;
    let tasks = store.load_tasks(context.fs)?;
    let task = if let Some(id) = id {
        tasks
            .iter()
            .find(|t| t.id == id && t.workflow == Workflow::Review)
    } else {
        tasks.iter().rev().find(|t| t.workflow == Workflow::Review)
    }
    .ok_or(Error::MissingReview)?;
    Ok(render_task(task))
}

pub(super) fn has_candidate_proof(tasks: &[Task], candidate: &str) -> bool {
    tasks.iter().rev().any(|t| {
        t.status == TaskStatus::Passed
            && t.review.as_ref().is_some_and(|r| {
                r.mode == ReviewMode::Acceptance && r.proof && r.candidate == candidate
            })
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "acceptance validation compares one complete candidate boundary"
)]
fn ensure_acceptance_ready(
    fs: &mut impl FileSystem,
    git: &Git,
    repository: &Repository,
    work: &Work,
    tasks: &[Task],
    live: &WorktreeStatus,
    digest: &str,
    paths: &[Utf8PathBuf],
) -> Result<Option<String>, Error> {
    if super::integrate::published_candidate(tasks) != Some(live.head().as_str()) {
        return Err(Error::ReviewPrerequisite);
    }
    if !develop::is_complete(work, tasks, live, git.operation(repository)?)
        || !live.is_clean()
        || work
            .latest_checkpoint
            .as_ref()
            .unwrap_or(&work.starting_source)
            .as_str()
            != live.head().as_str()
    {
        return Err(Error::ReviewPrerequisite);
    }
    let required = crate::policy_context::required_signoffs_for_paths(
        fs,
        repository.root(),
        paths.iter().map(Utf8PathBuf::as_path),
    )?;
    if !build::current_proof(tasks, live.head().as_str(), digest, &required) {
        return Err(Error::ReviewPrerequisite);
    }
    Ok(tasks
        .iter()
        .rev()
        .find(|t| t.workflow == Workflow::Build && t.status == TaskStatus::Passed)
        .map(|t| t.id.clone()))
}

fn candidate_paths(
    git: &Git,
    repository: &Repository,
    work: &Work,
    path: Option<&Utf8Path>,
) -> Result<(String, Vec<Utf8PathBuf>), Error> {
    let (target, base) = super::command::target_revision(git, repository, &work.target_branch)?;
    if let Some(path) = path {
        return Ok((base.as_str().to_owned(), vec![path.to_path_buf()]));
    }
    let changes = git.source_side_changes(repository, &target)?;
    Ok((
        base.as_str().to_owned(),
        changes.paths().iter().cloned().collect(),
    ))
}

fn revalidate<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    work: &Work,
    tasks: &[Task],
    review: &ReviewTask,
) -> Result<(), Error>
where
    F: FileSystem,
    C: Clock,
    O: io::Write,
    E: io::Write,
{
    let git = Git::default();
    let repository = git.discover(&context.repo_root)?;
    let live = git.status(&repository)?;
    if live.head().as_str() != review.candidate
        || super::command::change_snapshot(&repository, &live, context.fs)? != review.content_digest
    {
        return Err(Error::StaleReview);
    }
    let paths = if review.mode == ReviewMode::Feedback {
        review
            .reviewed_paths
            .iter()
            .map(Utf8PathBuf::from)
            .collect::<Vec<_>>()
    } else {
        candidate_paths(&git, &repository, work, None)?.1
    };
    let digest = crate::policy_context::effective_policy_digest_for_paths(
        context.fs,
        &context.repo_root,
        paths.iter().map(Utf8PathBuf::as_path),
    )?;
    if digest != review.policy_digest {
        return Err(Error::StaleReview);
    }
    if review.mode == ReviewMode::Acceptance {
        ensure_acceptance_ready(
            context.fs,
            &git,
            &repository,
            work,
            tasks,
            &live,
            &digest,
            &paths,
        )?;
    }
    Ok(())
}

fn validate_result(result: &ReviewResult, review: &ReviewTask) -> Result<(), Error> {
    if review.units.first().map(|u| u.input_checksum.as_str())
        != Some(result.input_checksum.as_str())
    {
        return Err(Error::InvalidReviewResult(
            "input checksum does not match".to_owned(),
        ));
    }
    let categories = result
        .categories
        .iter()
        .map(|c| c.category.as_str())
        .collect::<BTreeSet<_>>();
    if categories != CATEGORIES.into_iter().collect() {
        return Err(Error::InvalidReviewResult(
            "all seven categories are required exactly once".to_owned(),
        ));
    }
    for category in &result.categories {
        if category.explanation.trim().is_empty()
            || (!category.not_applicable && category.grade.is_none())
        {
            return Err(Error::InvalidReviewResult(format!(
                "category `{}` is incomplete",
                category.category
            )));
        }
    }
    let a =
        ReviewGrade::from_str("A").map_err(|_| Error::InvalidReviewResult("grade".to_owned()))?;
    if !result.overall_grade.meets(a) && result.proposed_actions.is_empty() {
        return Err(Error::InvalidReviewResult(
            "a grade below A requires proposed actions".to_owned(),
        ));
    }
    for finding in &result.proposed_actions {
        if finding.id.is_some()
            || finding.title.trim().is_empty()
            || finding.explanation.trim().is_empty()
            || finding.impact.trim().is_empty()
            || finding.recommended_correction.trim().is_empty()
            || finding.evidence.is_empty()
        {
            return Err(Error::InvalidReviewResult(
                "every proposed action requires complete evidence and correction".to_owned(),
            ));
        }
        if finding
            .rule_ids
            .iter()
            .any(|id| !review.rule_ids.contains(id))
        {
            return Err(Error::InvalidReviewResult(
                "proposed action cites an unknown Rule".to_owned(),
            ));
        }
        if finding
            .evidence
            .iter()
            .any(|e| e.path.trim().is_empty() || e.line == 0 || e.description.trim().is_empty())
        {
            return Err(Error::InvalidReviewResult(
                "evidence requires path, positive line, and description".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_standalone_result(result: &ReviewResult) -> Result<(), Error> {
    if result.input_checksum.trim().is_empty() {
        return Err(Error::InvalidReviewResult(
            "input checksum is required".to_owned(),
        ));
    }
    let categories = result
        .categories
        .iter()
        .map(|category| category.category.as_str())
        .collect::<BTreeSet<_>>();
    if categories != CATEGORIES.into_iter().collect() {
        return Err(Error::InvalidReviewResult(
            "all seven categories are required exactly once".to_owned(),
        ));
    }
    Ok(())
}

fn settle_review(task: &mut Task, at: String) -> Result<(), Error> {
    let review = task.review.as_mut().ok_or(Error::MissingReview)?;
    if review
        .findings
        .iter()
        .any(|f| f.status == FindingStatus::Pending)
    {
        return Ok(());
    }
    if review
        .findings
        .iter()
        .any(|f| f.status == FindingStatus::Accepted)
    {
        task.status = TaskStatus::Failed;
        review.proof = false;
        return Ok(());
    }
    let grade = review
        .result
        .as_ref()
        .ok_or(Error::MissingReview)?
        .overall_grade;
    if grade.meets(review.minimum_grade.unwrap_or_default()) {
        review.proof = true;
        task.finish(
            TaskStatus::Passed,
            at,
            "Review passed after reconciliation".to_owned(),
            None,
        );
    }
    Ok(())
}
fn current_review(tasks: &[Task]) -> Result<usize, Error> {
    tasks
        .iter()
        .rposition(|t| {
            t.workflow == Workflow::Review
                && matches!(t.status, TaskStatus::Running | TaskStatus::Blocked)
        })
        .ok_or(Error::MissingReview)
}
fn latest_review_with_finding(tasks: &[Task], id: &str) -> Result<usize, Error> {
    tasks
        .iter()
        .rposition(|t| {
            t.review
                .as_ref()
                .is_some_and(|r| r.findings.iter().any(|f| f.id.as_deref() == Some(id)))
        })
        .ok_or_else(|| Error::MissingFinding(id.to_owned()))
}
fn render_request(
    work: Option<&Work>,
    base: &str,
    candidate: &str,
    policy: &str,
    build: Option<&str>,
) -> String {
    let intent = work.map_or("Ad hoc repository review".to_owned(), |w| {
        format!(
            "{}\n\n{}\n\nSource: {:?} {}",
            w.title, w.description, w.request.kind, w.request.value
        )
    });
    OutputBuilder::new()
        .h1("Rapport Independent Review")
        .h2("Intent")
        .text(intent)
        .blank()
        .h2("Candidate")
        .text(format!(
            "- Base: `{}`\n- Candidate: `{}`\n- Build proof: `{}`",
            short(base),
            short(candidate),
            build.unwrap_or("feedback only")
        ))
        .blank()
        .text(policy)
        .blank()
        .h2("Host Instruction")
        .text("Delegate this request to a fresh independent review agent. The implementing agent must not review or certify its own candidate. Save the returned JSON outside the repository, then run `rapport review complete --result <FILE>`.")
        .blank()
        .h2("Grading Rubric")
        .text("A is excellent and exemplary; B is good and releasable; C has meaningful weaknesses; D has serious release-blocking flaws; F is unacceptable. Grade from the highest-impact unresolved risk. Inspect relevant source, tests, manifests, build files, and documentation. Form findings independently, cite Rule IDs and concrete file/line evidence, and keep suggested Rule improvements separate.")
        .build()
}
fn with_contract(request: String, checksum: &str) -> String {
    OutputBuilder::new()
        .text(request)
        .blank()
        .h2("Result Contract")
        .text(format!(
            "Return JSON with input_checksum `{checksum}`, overall_grade, overall_explanation, categories (the seven named categories, grade or not_applicable, and explanation), proposed_actions (title, explanation, rule_ids, evidence objects with path/line/description, impact, recommended_correction), and suggested_rule_improvements. Do not assign finding IDs."
        ))
        .build()
}
fn render_task(task: &Task) -> String {
    let Some(r) = &task.review else {
        return "# rapport review status\n\nmissing typed Review payload".to_owned();
    };
    let grade = r
        .result
        .as_ref()
        .map_or("ungraded".to_owned(), |v| v.overall_grade.to_string());
    format!(
        "# rapport review status\n\n- `task` — {}\n- `mode` — {}\n- `status` — {}\n- `candidate` — {}\n- `grade` — {}\n- `minimum` — {}\n- `findings` — {}\n- `quality override` — {}\n- `proof` — {}",
        task.id,
        r.mode,
        task.status,
        short(&r.candidate),
        grade,
        r.minimum_grade.map_or("none".to_owned(), |g| g.to_string()),
        r.findings
            .iter()
            .map(|f| format!(
                "{} {:?} {}",
                f.id.as_deref().unwrap_or("feedback"),
                f.status,
                f.title
            ))
            .collect::<Vec<_>>()
            .join("; "),
        r.quality_override.as_deref().unwrap_or("none"),
        r.proof
    )
}
fn checksum(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn short(v: &str) -> &str {
    &v[..v.len().min(12)]
}
fn required(v: &str) -> Result<String, Error> {
    if v.trim().is_empty() {
        Err(Error::EmptyField)
    } else {
        Ok(v.to_owned())
    }
}

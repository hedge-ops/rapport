use crate::cli::{
    LegacyReviewCompleteArgs as ReviewCompleteArgs, LegacyReviewStartArgs as ReviewStartArgs,
};
use crate::context::{Clock, CommandContext};
use crate::project_context::{
    ResolvedContextRule, SignoffRequirement, required_signoff_requirements_for_paths,
    resolved_rules_for_paths,
};
use crate::signoff_contract::SignoffKind;
use crate::snapshot::{self, OperationSnapshot, SnapshotError};
use crate::state::{
    OperationStatus, ReviewAction, ReviewActionStatus, ReviewAttempt, ReviewGrade, ReviewState,
    WorkState, WorkStateError, WorkStateStore,
};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::io::Write;
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const FAILURE: u8 = 2;
const REVIEW_PROTOCOL_VERSION: u16 = 2;
const REVIEW_INSTRUCTIONS: &str = r"Work adversarially and independently. Review the whole change for correctness, safety and security, reliability, maintainability, tests, documentation, operability, and compatibility; do not narrow scope to a named specialty. Inspect the relevant source, tests, manifests, build files, and documentation. Resolve and apply every supplied repository rule. Cite applicable rule IDs and concrete file/line evidence for every action. Do not edit the work. Form and grade the current findings before consulting the prior-task reconciliation ledger. Then reconcile identities: set prior_task_id to the Rapport task ID for the same substantive action even if order or wording changed, leave prior_task_id null for a genuinely new action, and omit actions that the independent rereview no longer finds. Grade from concrete findings and residual risk, not an average. Do not try to infer or target Rapport's passing threshold. Return only the structured result contract, including the unchanged requirement_id and input_checksum, an overall A-F grade with optional +/- modifier, a concise description, and all currently outstanding actions. An action needs a title, cited rule IDs, and concrete evidence; Rapport assigns IDs to new tasks.";

pub fn start<F, C, O, E>(
    start_args: &ReviewStartArgs,
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
        Ok(Some(mut state)) => request_reviews(start_args, context, &store, &mut state),
        Ok(None) => {
            let _ = writeln!(context.err, "{}", render_missing_work());
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            CommandResult::failure()
        }
    };
    finish("review start", arguments, context, result)
}

pub fn complete<F, C, O, E>(
    complete_args: &ReviewCompleteArgs,
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
        Ok(Some(mut state)) => record_results(&complete_args.result, context, &store, &mut state),
        Ok(None) => {
            let _ = writeln!(context.err, "{}", render_missing_work());
            CommandResult::failure()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_state_error(&error));
            CommandResult::failure()
        }
    };
    finish("review complete", arguments, context, result)
}

fn request_reviews<F, C, O, E>(
    args: &ReviewStartArgs,
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let paths = match select_paths(&args.paths, state, &context.repo_root, &context.cwd) {
        Ok(paths) => paths,
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_error(&error));
            return CommandResult::failure();
        }
    };
    let selected = match review_requirements(context.fs, &context.repo_root, &paths) {
        Ok(requirements) => requirements,
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_error(&error));
            return CommandResult::failure();
        }
    };
    if selected.is_empty() {
        let _ = writeln!(context.err, "{}", render_no_reviews());
        return CommandResult::failure();
    }

    let mut packets = Vec::new();
    for requirement in &selected {
        match prepare_requirement(context, state, requirement, None) {
            Ok(packet) => packets.push(packet),
            Err(error) => {
                let _ = writeln!(context.err, "{}", render_error(&error));
                return CommandResult::failure();
            }
        }
    }
    state.updated_at = context.clock.now_rfc3339();
    if let Err(error) = store.save(context.fs, state) {
        let _ = writeln!(context.err, "{}", render_state_error(&error));
        return CommandResult::failure();
    }
    let rendered = if args.json {
        serde_json::to_string_pretty(&packets).map_err(ReviewError::Encode)
    } else {
        render_review_requests(&packets)
    };
    match rendered {
        Ok(rendered) => {
            let _ = writeln!(context.out, "{rendered}");
            CommandResult::success()
        }
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_error(&error));
            CommandResult::failure()
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the review-result boundary keeps parsing, validation, durable recording, and user-facing outcome ordering explicit"
)]
fn record_results<F, C, O, E>(
    path: &Utf8Path,
    context: &mut CommandContext<'_, F, C, O, E>,
    store: &WorkStateStore,
    state: &mut WorkState,
) -> CommandResult
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let result_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        context.cwd.join(path)
    };
    let contents = match context.fs.read_to_string(&result_path) {
        Ok(contents) => contents,
        Err(source) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_error(&ReviewError::ReadResult {
                    path: result_path,
                    source,
                })
            );
            return CommandResult::failure();
        }
    };
    let submissions = match serde_json::from_str::<ReviewResultEnvelope>(&contents) {
        Ok(ReviewResultEnvelope::One(result)) => vec![result],
        Ok(ReviewResultEnvelope::Many(results)) => results,
        Err(source) => {
            let _ = writeln!(
                context.err,
                "{}",
                render_error(&ReviewError::DecodeResult {
                    path: result_path,
                    source,
                })
            );
            return CommandResult::failure();
        }
    };
    if submissions.is_empty() {
        let _ = writeln!(
            context.err,
            "{}",
            render_error(&ReviewError::InvalidResult(String::from(
                "review result array cannot be empty"
            )))
        );
        return CommandResult::failure();
    }
    if result_path_is_reviewed(&result_path, &context.repo_root, &submissions, state) {
        let _ = writeln!(
            context.err,
            "{}",
            render_error(&ReviewError::ResultInsideReviewedContent)
        );
        return CommandResult::failure();
    }

    let mut seen_requirements = BTreeSet::new();
    let mut requirements = Vec::new();
    for submission in &submissions {
        if !seen_requirements.insert(submission.requirement_id.as_str()) {
            let _ = writeln!(
                context.err,
                "{}",
                render_error(&ReviewError::InvalidResult(format!(
                    "duplicate review requirement `{}` in one result envelope",
                    submission.requirement_id
                )))
            );
            return CommandResult::failure();
        }
        let Some(pending) = state.reviews.get(&submission.requirement_id) else {
            let _ = writeln!(
                context.err,
                "{}",
                render_error(&ReviewError::InvalidResult(format!(
                    "unknown review requirement `{}`",
                    submission.requirement_id
                )))
            );
            return CommandResult::failure();
        };
        let scoped =
            match review_requirements(context.fs, &context.repo_root, &pending.reviewed_paths) {
                Ok(requirements) => requirements,
                Err(error) => {
                    let _ = writeln!(context.err, "{}", render_error(&error));
                    return CommandResult::failure();
                }
            };
        let Some(requirement) = scoped.into_iter().find(|requirement| {
            requirement.request.qualified_target() == submission.requirement_id
        }) else {
            let _ = writeln!(
                context.err,
                "{}",
                render_error(&ReviewError::InvalidResult(format!(
                    "unknown review requirement `{}`",
                    submission.requirement_id
                )))
            );
            return CommandResult::failure();
        };
        requirements.push(requirement);
    }

    // Apply the complete envelope to a transaction clone. No accepted result
    // becomes durable unless every unique submission validates against its
    // exact pending checksum, current rules, and derived status.
    let mut candidate = state.clone();
    let mut processed_ids = Vec::new();
    for submission in submissions {
        let requirement_id = submission.requirement_id.clone();
        let Some(requirement) = requirements.iter().find(|requirement| {
            requirement.request.qualified_target() == submission.requirement_id
        }) else {
            let _ = writeln!(
                context.err,
                "{}",
                render_error(&ReviewError::InvalidResult(format!(
                    "review requirement `{}` changed while applying the result envelope",
                    submission.requirement_id
                )))
            );
            return CommandResult::failure();
        };
        match apply_result(context, &mut candidate, requirement, submission) {
            Ok(_) => processed_ids.push(requirement_id),
            Err(error) => {
                let _ = writeln!(context.err, "{}", render_error(&error));
                return CommandResult::failure();
            }
        }
    }
    candidate.updated_at = context.clock.now_rfc3339();
    if let Err(error) = store.save(context.fs, &candidate) {
        let _ = writeln!(context.err, "{}", render_state_error(&error));
        return CommandResult::failure();
    }
    *state = candidate;
    let mut results = Vec::new();
    let mut tasks = Vec::new();
    let mut resolved = Vec::new();
    let mut any_failed = false;
    let mut first_open_task = None;
    for id in processed_ids {
        let Some(review) = state.reviews.get(&id) else {
            continue;
        };
        any_failed |= review.status != OperationStatus::Pass;
        let grade = review
            .grade
            .map_or_else(|| String::from("ungraded"), |grade| grade.to_string());
        results.push(format!(
            "`{id}`: {}; grade {grade} (minimum {})",
            review.status, review.minimum_grade
        ));
        for action in review
            .actions
            .iter()
            .filter(|action| action.is_outstanding())
        {
            if first_open_task.is_none() && action.status == ReviewActionStatus::Open {
                first_open_task = Some(action.id.clone());
            }
            tasks.push(format!(
                "`{}` {} — {}; rules [{}]; evidence: {}",
                action.id,
                action.status,
                action.title,
                action.rule_ids.join(", "),
                action.evidence
            ));
        }
        if let Some(attempt) = review.attempts.last() {
            resolved.extend(
                attempt
                    .resolved_action_ids
                    .iter()
                    .map(|task_id| format!("`{task_id}` resolved by independent rereview")),
            );
        }
    }
    let mut builder = ViewBuilder::new()
        .title("rapport review complete")
        .section("Results", |b| b.items(results));
    if !tasks.is_empty() {
        builder = builder.section("Review Tasks", |b| b.items(tasks));
    }
    if !resolved.is_empty() {
        builder = builder.section("Resolved Tasks", |b| b.items(resolved));
    }
    let next = first_open_task.map_or_else(
        || {
            if any_failed {
                String::from("rapport review start")
            } else {
                String::from("rapport integrate")
            }
        },
        |task_id| format!("rapport work task address {task_id} --summary \"what changed\""),
    );
    let rendered = builder.next_actions(nonempty![RunHint::new(next)]).build();
    let _ = writeln!(context.out, "{rendered}");
    CommandResult::success()
}

pub(crate) fn prepare_requirement<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
    requirement: &SignoffRequirement,
    explicit_base_sha: Option<&str>,
) -> Result<ReviewRequestPacket, ReviewError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let (rules, snapshot, minimum_grade) =
        requirement_inputs(context, requirement, explicit_base_sha)?;
    Ok(prepare_requirement_with_inputs(
        context,
        state,
        requirement,
        rules,
        snapshot,
        minimum_grade,
    ))
}

fn prepare_requirement_with_inputs<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
    requirement: &SignoffRequirement,
    rules: Vec<ResolvedContextRule>,
    snapshot: OperationSnapshot,
    minimum_grade: ReviewGrade,
) -> ReviewRequestPacket
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let id = requirement.request.qualified_target().to_string();
    let now = context.clock.now_rfc3339();
    let previous = state.reviews.get(&id);
    let prior_actions = previous.map_or_else(Vec::new, |review| {
        review
            .actions
            .iter()
            .filter(|action| action.is_outstanding())
            .cloned()
            .collect()
    });
    let review_state = ReviewState {
        status: OperationStatus::Pending,
        minimum_grade,
        declaring_context: requirement.request.declaring_context().to_string(),
        reviewed_paths: requirement.paths.clone(),
        at: now,
        base_sha: Some(snapshot.base_sha.clone()),
        head_sha: Some(snapshot.head_sha.clone()),
        content_checksum: snapshot.content_checksum.clone(),
        rules_checksum: snapshot.rules_checksum.clone(),
        instructions_checksum: snapshot.instructions_checksum.clone(),
        input_checksum: snapshot.input_checksum.clone(),
        // A new checksum always requires a new accepted result. Prior actions
        // remain available for reconciliation, but their grade is not proof.
        grade: None,
        description: previous.map_or_else(String::new, |review| review.description.clone()),
        actions: previous.map_or_else(Vec::new, |review| review.actions.clone()),
        attempts: previous.map_or_else(Vec::new, |review| review.attempts.clone()),
    };
    state.reviews.insert(id.clone(), review_state);
    ReviewRequestPacket {
        schema_version: REVIEW_PROTOCOL_VERSION,
        requirement: ReviewRequirementPacket {
            requirement_id: id.clone(),
            kind: String::from("review"),
            declaring_context: requirement.request.declaring_context().to_string(),
            reviewed_paths: requirement.paths.clone(),
        },
        snapshot: ReviewSnapshotPacket::from_snapshot(&snapshot),
        instructions: review_instructions(requirement),
        rules,
        reconciliation: ReviewReconciliationPacket {
            instruction: String::from(
                "Consult only after forming current findings. Set prior_task_id to the matching Rapport task ID for the same substantive action, leave it null for a new action, and omit actions no longer present.",
            ),
            prior_actions,
        },
        result_contract: ReviewResultContract {
            schema_version: REVIEW_PROTOCOL_VERSION,
            requirement_id: id,
            input_checksum: snapshot.input_checksum,
            grade: String::from("A through F with optional + or -"),
            description: String::from("why the grade fits"),
            actions: vec![ReviewActionContract {
                prior_task_id: None,
                title: String::from("short action title"),
                rule_ids: vec![String::from("RULE-ID")],
                evidence: String::from("path/to/file.rs:42: concrete evidence"),
            }],
        },
    }
}

pub(crate) fn evaluate_requirement<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
    requirement: &SignoffRequirement,
    explicit_base_sha: &str,
) -> Result<(OperationStatus, Option<ReviewRequestPacket>), ReviewError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let (rules, snapshot, minimum_grade) =
        requirement_inputs(context, requirement, Some(explicit_base_sha))?;
    let id = requirement.request.qualified_target();
    if let Some(review) = state.reviews.get_mut(id) {
        if review.input_checksum == snapshot.input_checksum {
            review.minimum_grade = minimum_grade;
            review.base_sha = Some(snapshot.base_sha.clone());
            review.head_sha = Some(snapshot.head_sha.clone());
            if review.status != OperationStatus::Pending {
                review.status = current_status(review.grade, minimum_grade, &review.actions);
                return Ok((review.status, None));
            }
            return Ok((
                OperationStatus::Pending,
                Some(prepare_requirement_with_inputs(
                    context,
                    state,
                    requirement,
                    rules,
                    snapshot,
                    minimum_grade,
                )),
            ));
        }
        review.status = OperationStatus::Stale;
    }
    Ok((
        OperationStatus::Pending,
        Some(prepare_requirement_with_inputs(
            context,
            state,
            requirement,
            rules,
            snapshot,
            minimum_grade,
        )),
    ))
}

pub(crate) fn refresh<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
) -> Result<(), ReviewError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let requirements = review_requirements(context.fs, &context.repo_root, &state.paths)?;
    for requirement in requirements {
        let id = requirement.request.qualified_target().to_string();
        let Some(existing) = state.reviews.get(&id) else {
            continue;
        };
        let base = existing.base_sha.clone();
        let (_, snapshot, minimum_grade) =
            requirement_inputs(context, &requirement, base.as_deref())?;
        if let Some(review) = state.reviews.get_mut(&id) {
            let exact = review.input_checksum == snapshot.input_checksum;
            review.status = if exact {
                if review.status == OperationStatus::Pending {
                    OperationStatus::Pending
                } else {
                    current_status(review.grade, minimum_grade, &review.actions)
                }
            } else {
                OperationStatus::Stale
            };
            review.minimum_grade = minimum_grade;
            if exact {
                review.base_sha = Some(snapshot.base_sha);
                review.head_sha = Some(snapshot.head_sha);
                review.content_checksum = snapshot.content_checksum;
                review.rules_checksum = snapshot.rules_checksum;
                review.instructions_checksum = snapshot.instructions_checksum;
                review.input_checksum = snapshot.input_checksum;
                review.reviewed_paths = requirement.paths;
            }
        }
    }
    Ok(())
}

pub(crate) fn status_lines<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
) -> Result<Vec<String>, ReviewError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    refresh(context, state)?;
    let requirements = review_requirements(context.fs, &context.repo_root, &state.paths)?;
    let mut lines = Vec::new();
    for requirement in requirements {
        let id = requirement.request.qualified_target();
        let Some(review) = state.reviews.get(id) else {
            lines.push(format!(
                "`{id}` missing; minimum {}; context `{}`; paths [{}]",
                requirement.request.minimum_grade().unwrap_or_default(),
                requirement.request.declaring_context(),
                requirement.paths.join(", ")
            ));
            continue;
        };
        let grade = review
            .grade
            .map_or_else(|| String::from("ungraded"), |grade| grade.to_string());
        let head = review.head_sha.as_deref().unwrap_or("uncommitted");
        let status = if review.status == OperationStatus::Stale {
            String::from("stale")
        } else {
            format!("current {}", review.status)
        };
        lines.push(format!(
            "`{id}` {status}; grade {grade} (minimum {}); head `{head}`; input `{}`; paths [{}]",
            review.minimum_grade,
            review.input_checksum,
            review.reviewed_paths.join(", ")
        ));
        lines.extend(
            review
                .actions
                .iter()
                .filter(|action| action.is_outstanding())
                .map(|action| {
                    format!(
                        "task `{}` {}; {}; rules [{}]; evidence: {}",
                        action.id,
                        action.status,
                        action.title,
                        action.rule_ids.join(", "),
                        action.evidence
                    )
                }),
        );
    }
    Ok(lines)
}

pub(crate) fn completion_problems<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
) -> Result<Vec<String>, ReviewError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    refresh(context, state)?;
    let requirements = review_requirements(context.fs, &context.repo_root, &state.paths)?;
    let mut problems = Vec::new();
    for requirement in requirements {
        let id = requirement.request.qualified_target();
        match state.reviews.get(id) {
            None => problems.push(format!("required review `{id}` is missing")),
            Some(review) if review.status != OperationStatus::Pass => {
                problems.push(format!("required review `{id}` is {}", review.status));
            }
            Some(review) if review.actions.iter().any(ReviewAction::is_outstanding) => problems
                .push(format!(
                    "required review `{id}` has {} outstanding action(s)",
                    review
                        .actions
                        .iter()
                        .filter(|action| action.is_outstanding())
                        .count()
                )),
            Some(_) => {}
        }
    }
    Ok(problems)
}

fn apply_result<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    state: &mut WorkState,
    requirement: &SignoffRequirement,
    submission: ReviewResultInput,
) -> Result<OperationStatus, ReviewError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    validate_submission(&submission)?;
    let id = requirement.request.qualified_target().to_string();
    let Some(existing) = state.reviews.get(&id) else {
        return Err(ReviewError::InvalidResult(format!(
            "review `{id}` has no pending request; run `rapport review start` first"
        )));
    };
    if existing.status != OperationStatus::Pending {
        return Err(ReviewError::InvalidResult(format!(
            "review `{id}` has no current pending request; run `rapport review start` first"
        )));
    }
    if submission.input_checksum != existing.input_checksum {
        return Err(ReviewError::InvalidResult(format!(
            "review `{id}` result checksum does not match the pending request"
        )));
    }
    let base = existing.base_sha.clone();
    let (rules, snapshot, minimum_grade) =
        requirement_inputs(context, requirement, base.as_deref())?;
    if snapshot.input_checksum != submission.input_checksum {
        if let Some(review) = state.reviews.get_mut(&id) {
            review.status = OperationStatus::Stale;
        }
        return Err(ReviewError::InvalidResult(format!(
            "review `{id}` inputs changed after the request; the pending result is stale"
        )));
    }
    let now = context.clock.now_rfc3339();
    validate_action_rules(&submission.actions, &rules)?;
    let reconciled = reconcile_actions(state, &id, &submission.actions, &now)?;
    let derived = current_status(Some(submission.grade), minimum_grade, &reconciled.current);
    let Some(previous) = state.reviews.get(&id) else {
        return Err(ReviewError::InvalidResult(format!(
            "review `{id}` disappeared before its result could be recorded"
        )));
    };
    let attempt = ReviewAttempt {
        status: derived,
        at: now.clone(),
        input_checksum: snapshot.input_checksum.clone(),
        grade: submission.grade,
        description: submission.description.clone(),
        actions: reconciled.current,
        resolved_action_ids: reconciled.resolved_ids,
    };
    let mut attempts = previous.attempts.clone();
    attempts.push(attempt);
    state.reviews.insert(
        id,
        ReviewState {
            status: derived,
            minimum_grade,
            declaring_context: requirement.request.declaring_context().to_string(),
            reviewed_paths: requirement.paths.clone(),
            at: now,
            base_sha: Some(snapshot.base_sha),
            head_sha: Some(snapshot.head_sha),
            content_checksum: snapshot.content_checksum,
            rules_checksum: snapshot.rules_checksum,
            instructions_checksum: snapshot.instructions_checksum,
            input_checksum: snapshot.input_checksum,
            grade: Some(submission.grade),
            description: submission.description,
            actions: reconciled.ledger,
            attempts,
        },
    );
    Ok(derived)
}

struct ReconciledActions {
    ledger: Vec<ReviewAction>,
    current: Vec<ReviewAction>,
    resolved_ids: Vec<String>,
}

fn reconcile_actions(
    state: &WorkState,
    review_id: &str,
    submitted: &[ReviewActionInput],
    now: &str,
) -> Result<ReconciledActions, ReviewError> {
    let previous = state
        .reviews
        .get(review_id)
        .map_or_else(Vec::new, |review| review.actions.clone());
    let mut known_ids = all_review_task_ids(state);
    let mut next_number = next_review_task_number(&known_ids);
    let mut reused = BTreeSet::new();
    let mut current = Vec::new();

    for action in submitted {
        let (id, addressed_at, addressed_summary) = if let Some(prior_task_id) =
            &action.prior_task_id
        {
            if !reused.insert(prior_task_id.clone()) {
                return Err(ReviewError::InvalidResult(format!(
                    "review task `{prior_task_id}` was reconciled more than once"
                )));
            }
            let Some(prior) = previous
                .iter()
                .find(|prior| prior.id == *prior_task_id && prior.is_outstanding())
            else {
                return Err(ReviewError::InvalidResult(format!(
                    "prior_task_id `{prior_task_id}` is not an outstanding task for review `{review_id}`"
                )));
            };
            (
                prior.id.clone(),
                prior.addressed_at.clone(),
                prior.addressed_summary.clone(),
            )
        } else {
            let id = loop {
                let candidate = format!("REV-{next_number:03}");
                next_number = next_number.checked_add(1).unwrap_or(1);
                if known_ids.insert(candidate.clone()) {
                    break candidate;
                }
            };
            (id, None, None)
        };
        current.push(ReviewAction {
            id,
            status: ReviewActionStatus::Open,
            title: action.title.clone(),
            rule_ids: action.rule_ids.clone(),
            evidence: action.evidence.clone(),
            addressed_at,
            addressed_summary,
            resolved_at: None,
        });
    }

    let mut resolved_ids = Vec::new();
    let mut ledger = current.clone();
    for mut prior in previous {
        if prior.status == ReviewActionStatus::Resolved {
            ledger.push(prior);
        } else if !reused.contains(&prior.id) {
            prior.status = ReviewActionStatus::Resolved;
            prior.resolved_at = Some(now.to_string());
            resolved_ids.push(prior.id.clone());
            ledger.push(prior);
        }
    }
    Ok(ReconciledActions {
        ledger,
        current,
        resolved_ids,
    })
}

fn all_review_task_ids(state: &WorkState) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for review in state.reviews.values() {
        ids.extend(review.actions.iter().map(|action| action.id.clone()));
        for attempt in &review.attempts {
            ids.extend(attempt.actions.iter().map(|action| action.id.clone()));
            ids.extend(attempt.resolved_action_ids.iter().cloned());
        }
    }
    ids
}

fn next_review_task_number(ids: &BTreeSet<String>) -> u64 {
    ids.iter()
        .filter_map(|id| id.strip_prefix("REV-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .unwrap_or(1)
}

fn requirement_inputs<F, C, O, E>(
    context: &mut CommandContext<'_, F, C, O, E>,
    requirement: &SignoffRequirement,
    explicit_base_sha: Option<&str>,
) -> Result<(Vec<ResolvedContextRule>, OperationSnapshot, ReviewGrade), ReviewError>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let rules = resolved_rules_for_paths(context.fs, &context.repo_root, &requirement.paths)?;
    let canonical_rules = serde_json::to_string(&rules).map_err(ReviewError::Encode)?;
    let rules_checksum = snapshot::checksum([canonical_rules.as_str()]);
    let instructions = review_instructions(requirement);
    let instructions_checksum = snapshot::checksum([instructions.as_str()]);
    let snapshot = snapshot::capture(
        context.fs,
        context.runner,
        &context.repo_root,
        requirement.request.qualified_target(),
        &requirement.paths,
        explicit_base_sha,
        &rules_checksum,
        &instructions_checksum,
    )?;
    let minimum_grade = requirement.request.minimum_grade().unwrap_or_default();
    Ok((rules, snapshot, minimum_grade))
}

fn review_instructions(requirement: &SignoffRequirement) -> String {
    format!(
        "{REVIEW_INSTRUCTIONS}\n\nThis comprehensive review is `{}` declared by `{}` for paths [{}].",
        requirement.request.qualified_target(),
        requirement.request.declaring_context(),
        requirement.paths.join(", "),
    )
}

fn review_requirements(
    fs: &impl FileSystem,
    repo_root: &Utf8Path,
    paths: &[String],
) -> Result<Vec<SignoffRequirement>, ReviewError> {
    Ok(
        required_signoff_requirements_for_paths(fs, repo_root, paths)?
            .into_iter()
            .filter(|requirement| requirement.request.kind() == SignoffKind::Review)
            .collect(),
    )
}

fn current_status(
    grade: Option<ReviewGrade>,
    minimum: ReviewGrade,
    actions: &[ReviewAction],
) -> OperationStatus {
    match grade {
        None => OperationStatus::Pending,
        Some(grade)
            if grade.meets(minimum) && actions.iter().all(|action| !action.is_outstanding()) =>
        {
            OperationStatus::Pass
        }
        Some(_) => OperationStatus::Fail,
    }
}

fn validate_submission(result: &ReviewResultInput) -> Result<(), ReviewError> {
    if result.schema_version != REVIEW_PROTOCOL_VERSION {
        return Err(ReviewError::InvalidResult(format!(
            "unsupported review result schema {}; expected {}",
            result.schema_version, REVIEW_PROTOCOL_VERSION
        )));
    }
    if result.requirement_id.trim().is_empty()
        || result.input_checksum.trim().is_empty()
        || result.description.trim().is_empty()
    {
        return Err(ReviewError::InvalidResult(String::from(
            "requirement_id, input_checksum, and description are required",
        )));
    }
    for action in &result.actions {
        if action.title.trim().is_empty()
            || action.evidence.trim().is_empty()
            || action.rule_ids.is_empty()
        {
            return Err(ReviewError::InvalidResult(String::from(
                "every review action requires a title, cited rule IDs, and concrete evidence",
            )));
        }
        if action
            .prior_task_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err(ReviewError::InvalidResult(String::from(
                "prior_task_id must be null or a non-empty Rapport task ID",
            )));
        }
    }
    Ok(())
}

fn validate_action_rules(
    actions: &[ReviewActionInput],
    rules: &[ResolvedContextRule],
) -> Result<(), ReviewError> {
    let known = rules
        .iter()
        .map(|rule| rule.id.as_str())
        .collect::<BTreeSet<_>>();
    for action in actions {
        for rule_id in &action.rule_ids {
            if !known.contains(rule_id.as_str()) {
                return Err(ReviewError::InvalidResult(format!(
                    "review action `{}` cites unknown rule `{rule_id}`",
                    action.title
                )));
            }
        }
    }
    Ok(())
}

fn select_paths(
    requested: &[Utf8PathBuf],
    state: &WorkState,
    repo_root: &Utf8Path,
    cwd: &Utf8Path,
) -> Result<Vec<String>, ReviewError> {
    if requested.is_empty() {
        return Ok(state.paths.clone());
    }
    let mut selected = Vec::new();
    for path in requested {
        let absolute = if path.is_absolute() {
            path.clone()
        } else {
            cwd.join(path)
        };
        let relative = absolute
            .strip_prefix(repo_root)
            .map_err(|_| ReviewError::InvalidPath(absolute.clone()))?;
        let portable = relative.as_str().replace('\\', "/");
        let components = portable
            .split('/')
            .filter(|component| !component.is_empty() && *component != ".")
            .collect::<Vec<_>>();
        if components.contains(&"..") {
            return Err(ReviewError::InvalidPath(absolute));
        }
        let normalized = if components.is_empty() {
            String::from(".")
        } else {
            components.join("/")
        };
        if !state
            .paths
            .iter()
            .any(|work_path| path_is_within(&normalized, work_path))
        {
            return Err(ReviewError::OutsideWork(normalized));
        }
        selected.push(normalized);
    }
    Ok(selected)
}

fn path_is_within(selected: &str, work_path: &str) -> bool {
    work_path == "."
        || selected == work_path
        || selected
            .strip_prefix(work_path)
            .is_some_and(|remaining| remaining.starts_with('/'))
}

fn result_path_is_reviewed(
    result_path: &Utf8Path,
    repo_root: &Utf8Path,
    submissions: &[ReviewResultInput],
    state: &WorkState,
) -> bool {
    let Ok(relative) = result_path.strip_prefix(repo_root) else {
        return false;
    };
    let mut components = Vec::new();
    let portable = relative.as_str().replace('\\', "/");
    for component in portable.split('/') {
        if component == ".." {
            if components.pop().is_none() {
                return false;
            }
        } else if !component.is_empty() && component != "." {
            components.push(component);
        }
    }
    let normalized = components.join("/");
    submissions.iter().any(|submission| {
        state
            .reviews
            .get(&submission.requirement_id)
            .is_some_and(|review| {
                review
                    .reviewed_paths
                    .iter()
                    .any(|reviewed_path| path_is_within(&normalized, reviewed_path))
            })
    })
}

fn render_review_requests(packets: &[ReviewRequestPacket]) -> Result<String, ReviewError> {
    if let [packet] = packets {
        return render_review_request(packet);
    }
    let json = serde_json::to_string_pretty(packets).map_err(ReviewError::Encode)?;
    Ok(format!(
        "# Rapport adversarial review set\n\nPerform a fresh, independent review for each of the {} exact requests below. Inspect the current workspace as needed, follow every request's supplied instructions and rules, and do not edit the work. Return only one JSON array containing exactly one structured result object per request, in the same order. Do not return adjacent standalone objects or Markdown fences.\n\nDo not save this request or its result inside any reviewed path; keep protocol files outside the repository, such as under `/tmp`.\n\n```json\n{json}\n```",
        packets.len()
    ))
}

pub(crate) fn render_review_request(packet: &ReviewRequestPacket) -> Result<String, ReviewError> {
    let json = serde_json::to_string_pretty(packet).map_err(ReviewError::Encode)?;
    Ok(format!(
        "# Rapport adversarial review\n\nPerform a fresh, independent review of the exact request below. Inspect the current workspace as needed, follow every supplied instruction and rule, and do not edit the work. Return only the structured JSON result described by `result_contract`.\n\nDo not save this request or its result inside any reviewed path; keep protocol files outside the repository, such as under `/tmp`.\n\n```json\n{json}\n```"
    ))
}

#[derive(Serialize)]
pub(crate) struct ReviewRequestPacket {
    schema_version: u16,
    requirement: ReviewRequirementPacket,
    snapshot: ReviewSnapshotPacket,
    instructions: String,
    rules: Vec<ResolvedContextRule>,
    reconciliation: ReviewReconciliationPacket,
    result_contract: ReviewResultContract,
}

#[derive(Serialize)]
struct ReviewReconciliationPacket {
    instruction: String,
    prior_actions: Vec<ReviewAction>,
}

#[derive(Serialize)]
struct ReviewRequirementPacket {
    requirement_id: String,
    kind: String,
    declaring_context: String,
    reviewed_paths: Vec<String>,
}

#[derive(Serialize)]
struct ReviewSnapshotPacket {
    base_sha: String,
    head_sha: String,
    content_checksum: String,
    rules_checksum: String,
    instructions_checksum: String,
    input_checksum: String,
}

impl ReviewSnapshotPacket {
    fn from_snapshot(snapshot: &OperationSnapshot) -> Self {
        Self {
            base_sha: snapshot.base_sha.clone(),
            head_sha: snapshot.head_sha.clone(),
            content_checksum: snapshot.content_checksum.clone(),
            rules_checksum: snapshot.rules_checksum.clone(),
            instructions_checksum: snapshot.instructions_checksum.clone(),
            input_checksum: snapshot.input_checksum.clone(),
        }
    }
}

#[derive(Serialize)]
struct ReviewResultContract {
    schema_version: u16,
    requirement_id: String,
    input_checksum: String,
    grade: String,
    description: String,
    actions: Vec<ReviewActionContract>,
}

#[derive(Serialize)]
struct ReviewActionContract {
    prior_task_id: Option<String>,
    title: String,
    rule_ids: Vec<String>,
    evidence: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReviewResultEnvelope {
    One(ReviewResultInput),
    Many(Vec<ReviewResultInput>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewResultInput {
    schema_version: u16,
    requirement_id: String,
    input_checksum: String,
    grade: ReviewGrade,
    description: String,
    #[serde(default)]
    actions: Vec<ReviewActionInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewActionInput {
    #[serde(default)]
    prior_task_id: Option<String>,
    title: String,
    rule_ids: Vec<String>,
    evidence: String,
}

pub(crate) enum ReviewError {
    Context(crate::project_context::ProjectContextError),
    Snapshot(SnapshotError),
    Encode(serde_json::Error),
    ReadResult {
        path: Utf8PathBuf,
        source: std::io::Error,
    },
    DecodeResult {
        path: Utf8PathBuf,
        source: serde_json::Error,
    },
    ResultInsideReviewedContent,
    InvalidResult(String),
    InvalidPath(Utf8PathBuf),
    OutsideWork(String),
}

impl fmt::Debug for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Context(_) => "context",
            Self::Snapshot(_) => "snapshot",
            Self::Encode(_) => "encode",
            Self::ReadResult { .. } => "read_result",
            Self::DecodeResult { .. } => "decode_result",
            Self::ResultInsideReviewedContent => "result_inside_reviewed_content",
            Self::InvalidResult(_) => "invalid_result",
            Self::InvalidPath(_) => "invalid_path",
            Self::OutsideWork(_) => "outside_work",
        };
        f.debug_struct("ReviewError").field("kind", &kind).finish()
    }
}

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Context(source) => write!(
                f,
                "could not resolve review context (detail {} bytes)",
                source.to_string().len()
            ),
            Self::Snapshot(source) => write!(f, "could not checksum review inputs: {source}"),
            Self::Encode(source) => write!(
                f,
                "could not encode review protocol JSON (line {}, column {})",
                source.line(),
                source.column()
            ),
            Self::ReadResult { path, source } => {
                write!(
                    f,
                    "could not read review result file (path {} bytes; {:?})",
                    path.as_str().len(),
                    source.kind()
                )
            }
            Self::DecodeResult { path, source } => write!(
                f,
                "could not parse review result JSON (path {} bytes; line {}, column {})",
                path.as_str().len(),
                source.line(),
                source.column()
            ),
            Self::ResultInsideReviewedContent => f.write_str(
                "review result files must be outside the reviewed content; write the result under /tmp and retry",
            ),
            Self::InvalidResult(detail) => write!(
                f,
                "invalid review result; details were redacted ({} bytes)",
                detail.len()
            ),
            Self::InvalidPath(path) => write!(
                f,
                "review path is outside the repository ({} bytes)",
                path.as_str().len()
            ),
            Self::OutsideWork(path) => write!(
                f,
                "review path is outside active work ({} bytes)",
                path.len()
            ),
        }
    }
}

impl Error for ReviewError {}

impl From<crate::project_context::ProjectContextError> for ReviewError {
    fn from(source: crate::project_context::ProjectContextError) -> Self {
        Self::Context(source)
    }
}

impl From<SnapshotError> for ReviewError {
    fn from(source: SnapshotError) -> Self {
        Self::Snapshot(source)
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
    let event = CommandEvent::new(
        context.clock.now_rfc3339(),
        arguments,
        command,
        result.outcome,
        result.exit_code,
    );
    match TelemetryWriter::new(context.paths.clone()).append(context.fs, &event) {
        Ok(()) => ExitCode::from(result.exit_code),
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
            ExitCode::FAILURE
        }
    }
}

fn render_missing_work() -> String {
    ViewBuilder::new()
        .title("rapport review")
        .paragraph("No active work state found.")
        .next_actions(nonempty![RunHint::new(
            "rapport work start --title \"...\" --path <path>"
        )])
        .build()
}

fn render_no_reviews() -> String {
    ViewBuilder::new()
        .title("rapport review")
        .paragraph("No review signoff applies to the selected active-work paths.")
        .next_actions(nonempty![RunHint::new("rapport context show <path>")])
        .build()
}

fn render_error(error: &ReviewError) -> String {
    ViewBuilder::new()
        .title("rapport review")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_state_error(error: &WorkStateError) -> String {
    ViewBuilder::new()
        .title("rapport review")
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
    #[expect(
        clippy::expect_used,
        reason = "literal grade fixtures are part of this test's setup contract"
    )]
    fn current_status_should_require_threshold_and_no_actions() {
        let a_minus: ReviewGrade = "A-".parse().expect("valid fixture grade");
        let b_plus: ReviewGrade = "B+".parse().expect("valid fixture grade");
        let action = ReviewAction {
            id: String::from("REV-001"),
            status: ReviewActionStatus::Open,
            title: String::from("Fix it"),
            rule_ids: vec![String::from("RULE-001")],
            evidence: String::from("src/lib.rs:1"),
            addressed_at: None,
            addressed_summary: None,
            resolved_at: None,
        };

        assert_eq!(
            current_status(Some(a_minus), a_minus, &[]),
            OperationStatus::Pass
        );
        assert_eq!(
            current_status(Some(b_plus), a_minus, &[]),
            OperationStatus::Fail
        );
        assert_eq!(
            current_status(Some(a_minus), a_minus, &[action]),
            OperationStatus::Fail
        );

        let invalid = ReviewError::InvalidResult(String::from("PRIVATE REVIEW RESULT"));
        let read = ReviewError::ReadResult {
            path: Utf8PathBuf::from("PRIVATE/RESULT.json"),
            source: std::io::Error::other("PRIVATE IO DETAIL"),
        };
        let diagnostics = format!("{invalid:?} {invalid} {read:?} {read}");
        assert!(!diagnostics.contains("PRIVATE"));
        assert!(diagnostics.contains("invalid_result"));
    }
}

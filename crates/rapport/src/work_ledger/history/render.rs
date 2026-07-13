//! Human-readable rendering for complete historical Work records.

use super::super::Error;
use super::super::domain::{
    BuildTask, FindingStatus, IntegrationStage, IntegrationTask, RequestKind, ReviewTask, Task,
};
use super::repository::HistoryRecord;

pub(super) fn record(record: &HistoryRecord) -> Result<String, Error> {
    let work = &record.work;
    let outcome = work.outcome.as_ref().ok_or(Error::UnfinalizedHistory)?;
    let tasks = record
        .tasks
        .iter()
        .map(render_task)
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(format!(
        "# rapport work history show\n\n- `work` — {}\n- `title` — {}\n- `description` — {}\n- `request` — {} {}\n- `repository` — {}\n- `archive` — {}\n- `created` — {}\n- `source branch` — {}\n- `target branch` — {}\n- `starting source` — {}\n- `starting target` — {}\n- `final source` — {}\n- `final target` — {}\n- `outcome` — {}\n- `outcome recorded` — {}\n- `outcome summary` — {}\n- `Develop sequence` — {}\n- `tasks` — {}\n\n{}",
        work.id,
        work.title,
        work.description,
        request_kind(work.request.kind),
        work.request.value,
        work.repository,
        record.path,
        work.created_at,
        work.source_branch,
        work.target_branch,
        work.starting_source,
        work.starting_target,
        outcome.source_commit,
        outcome.target_commit,
        outcome.kind,
        outcome.at,
        outcome.summary,
        none(&work.development_sequence.join(", ")),
        record.tasks.len(),
        if tasks.is_empty() {
            "## Task ledger\n\nnone"
        } else {
            &tasks
        }
    ))
}

fn render_task(task: &Task) -> String {
    let payload = task
        .payload
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sections = vec![format!(
        "## {} — {}\n\n- `type` — {}\n- `workflow` — {}\n- `status` — {}\n- `title` — {}\n- `description` — {}\n- `origin` — {}\n- `related` — {}\n- `source commit` — {}\n- `created` — {}\n- `completed` — {}\n- `result` — {}\n- `output` — {}\n- `payload` — {}",
        task.id,
        task.workflow,
        task.kind,
        task.workflow,
        task.status,
        task.title,
        task.description,
        task.origin,
        none(&task.related.join(", ")),
        task.source_commit,
        task.created_at,
        task.completed_at.as_deref().unwrap_or("none"),
        task.result.as_deref().unwrap_or("none"),
        task.output.as_deref().unwrap_or("none"),
        none(&payload)
    )];
    if let Some(build) = &task.build {
        sections.push(render_build(build));
    }
    if let Some(review) = &task.review {
        sections.push(render_review(review));
    }
    if let Some(integration) = &task.integration {
        sections.push(render_integration(integration));
    }
    sections.join("\n\n")
}

fn render_build(build: &BuildTask) -> String {
    let operations = build
        .operations
        .iter()
        .map(|operation| {
            format!(
                "- `{}` — {} — target {} — context {} — stage {} — resource {} — proof {} — duration {} — exit {} — stdout {} — stderr {}",
                operation.id,
                operation.status,
                operation.target,
                operation.context.as_deref().unwrap_or("none"),
                operation.stage,
                operation.resource_group.as_deref().unwrap_or("none"),
                operation.proof,
                operation
                    .duration_seconds
                    .map_or_else(|| "none".to_owned(), |duration| duration.to_string()),
                operation
                    .exit_status
                    .map_or_else(|| "none".to_owned(), |status| status.to_string()),
                none(&operation.stdout),
                none(&operation.stderr)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let operation_section = if operations.is_empty() {
        "#### Build operations\n\nnone".to_owned()
    } else {
        format!("#### Build operations\n\n{operations}")
    };
    format!(
        "### Build evidence\n\n- `mode` — {}\n- `candidate` — {}\n- `policy digest` — {}\n- `proof` — {}\n- `initial head` — {}\n- `final head` — {}\n\n{}",
        build.mode,
        build.candidate,
        build.policy_digest.as_deref().unwrap_or("none"),
        build.proof,
        build.initial_git.head,
        build
            .final_git
            .as_ref()
            .map_or("none", |git| git.head.as_str()),
        operation_section
    )
}

fn render_review(review: &ReviewTask) -> String {
    let categories = review
        .result
        .as_ref()
        .map(|result| {
            result
                .categories
                .iter()
                .map(|category| {
                    format!(
                        "- {} — {} — {}",
                        category.category,
                        category
                            .grade
                            .map_or_else(|| "not applicable".to_owned(), |grade| grade.to_string()),
                        category.explanation
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let findings = review
        .findings
        .iter()
        .map(|finding| {
            let evidence = finding
                .evidence
                .iter()
                .map(|item| format!("{}:{} {}", item.path, item.line, item.description))
                .collect::<Vec<_>>()
                .join("; ");
            format!(
                "- `{}` — {} — {} — explanation {} — Rules {} — evidence {} — reason {} — corrective task {} — impact {} — correction {}",
                finding.id.as_deref().unwrap_or("unassigned"),
                finding_status(finding.status),
                finding.title,
                finding.explanation,
                none(&finding.rule_ids.join(", ")),
                none(&evidence),
                finding.decision_reason.as_deref().unwrap_or("none"),
                finding.corrective_task.as_deref().unwrap_or("none"),
                finding.impact,
                finding.recommended_correction
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "### Review evidence\n\n- `mode` — {}\n- `base` — {}\n- `candidate` — {}\n- `policy digest` — {}\n- `build task` — {}\n- `grade` — {}\n- `grade explanation` — {}\n- `minimum grade` — {}\n- `proof` — {}\n- `quality override` — {}\n\n#### Category grades\n\n{}\n\n#### Findings and reconciliation\n\n{}",
        review.mode,
        review.base,
        review.candidate,
        review.policy_digest,
        review.build_task.as_deref().unwrap_or("none"),
        review.result.as_ref().map_or_else(
            || "none".to_owned(),
            |result| result.overall_grade.to_string()
        ),
        review
            .result
            .as_ref()
            .map_or("none", |result| result.overall_explanation.as_str()),
        review
            .minimum_grade
            .map_or_else(|| "none".to_owned(), |grade| grade.to_string()),
        review.proof,
        review.quality_override.as_deref().unwrap_or("none"),
        none(&categories),
        none(&findings)
    )
}

fn render_integration(integration: &IntegrationTask) -> String {
    format!(
        "### Integration evidence\n\n- `stage` — {}\n- `repository` — {}\n- `source branch` — {}\n- `target branch` — {}\n- `candidate` — {}\n- `target commit` — {}\n- `freshness` — {}\n- `Build task` — {}\n- `Review task` — {}\n- `Review grade` — {}\n- `quality override` — {}\n- `pull request` — {} {}\n- `merge commit` — {}\n- `remote branch deleted` — {}\n- `cancellation` — {}",
        integration_stage(integration.stage),
        integration.repository.as_deref().unwrap_or("none"),
        integration.source_branch,
        integration.target_branch,
        integration.candidate,
        integration.target_commit,
        integration.freshness_policy,
        integration.build_task,
        integration.review_task,
        integration.review_grade,
        integration.quality_override.as_deref().unwrap_or("none"),
        integration
            .pull_request_number
            .map_or_else(|| "none".to_owned(), |number| format!("#{number}")),
        integration.pull_request_url.as_deref().unwrap_or(""),
        integration.merge_commit.as_deref().unwrap_or("none"),
        integration.remote_branch_deleted,
        integration.cancellation_reason.as_deref().unwrap_or("none")
    )
}

fn request_kind(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Ticket => "ticket",
        RequestKind::Plan => "plan",
        RequestKind::AdHoc => "ad hoc",
    }
}

fn finding_status(status: FindingStatus) -> &'static str {
    match status {
        FindingStatus::Pending => "pending",
        FindingStatus::Accepted => "accepted",
        FindingStatus::Dismissed => "dismissed",
    }
}

fn integration_stage(stage: IntegrationStage) -> &'static str {
    match stage {
        IntegrationStage::Preparing => "preparing",
        IntegrationStage::Published => "published",
        IntegrationStage::Merging => "merging",
        IntegrationStage::Merged => "merged",
        IntegrationStage::Cancelling => "cancelling",
        IntegrationStage::Cancelled => "cancelled",
    }
}

fn none(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

//! Phase 3 Work and Task domain values.
//!
//! This module owns durable Work, Task, Build, Review, and Integration state and their transition invariants.

use chrono::DateTime;
use rapport_git::{BranchName, ObjectId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

use super::Error;
use super::grade::ReviewGrade;

pub(super) const WORK_SCHEMA_VERSION: u16 = 2;
pub(super) const TASK_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RequestKind {
    Ticket,
    Plan,
    AdHoc,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RequestSource {
    pub(super) kind: RequestKind,
    pub(super) value: String,
}

impl fmt::Debug for RequestSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestSource")
            .field("kind", &self.kind)
            .field("value", &self.value)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Work {
    pub(super) version: u16,
    pub(super) id: Uuid,
    pub(super) repository: String,
    pub(super) title: String,
    pub(super) description: String,
    pub(super) request: RequestSource,
    pub(super) source_branch: BranchName,
    pub(super) target_branch: BranchName,
    pub(super) starting_source: ObjectId,
    pub(super) starting_target: ObjectId,
    pub(super) latest_checkpoint: Option<ObjectId>,
    pub(super) development_sequence: Vec<String>,
    pub(super) next_task: u32,
    pub(super) next_finding: u32,
    pub(super) created_at: String,
    pub(super) outcome: Option<WorkOutcome>,
}

impl Work {
    #[expect(
        clippy::too_many_arguments,
        reason = "Work start records one complete immutable Git and request identity"
    )]
    pub(super) fn new(
        title: String,
        description: String,
        request: RequestSource,
        repository: String,
        source_branch: BranchName,
        target_branch: BranchName,
        starting_source: ObjectId,
        starting_target: ObjectId,
        created_at: String,
    ) -> Result<Self, Error> {
        Ok(Self {
            version: WORK_SCHEMA_VERSION,
            id: Uuid::new_v4(),
            repository: required(repository)?,
            title: required(title)?,
            description: required(description)?,
            request,
            source_branch,
            target_branch,
            starting_source,
            starting_target,
            latest_checkpoint: None,
            development_sequence: Vec::new(),
            next_task: 1,
            next_finding: 1,
            created_at,
            outcome: None,
        })
    }

    pub(super) fn finish(
        &mut self,
        kind: WorkOutcomeKind,
        at: String,
        summary: String,
        source_commit: ObjectId,
        target_commit: ObjectId,
    ) -> Result<(), Error> {
        if let Some(outcome) = &self.outcome {
            if outcome.kind == kind {
                return Ok(());
            }
            return Err(Error::FinalizedWork(outcome.kind.to_string()));
        }
        self.outcome = Some(WorkOutcome {
            kind,
            at: required(at)?,
            summary: required(summary)?,
            source_commit,
            target_commit,
        });
        Ok(())
    }

    pub(super) fn allocate_task_id(&mut self) -> Result<String, Error> {
        let id = format!("TASK_{:03}", self.next_task);
        self.next_task = self
            .next_task
            .checked_add(1)
            .ok_or(Error::TaskIdExhausted)?;
        Ok(id)
    }

    pub(super) fn allocate_finding_id(&mut self) -> Result<String, Error> {
        let id = format!("REV_{:03}", self.next_finding);
        self.next_finding = self
            .next_finding
            .checked_add(1)
            .ok_or(Error::TaskIdExhausted)?;
        Ok(id)
    }
}

impl fmt::Debug for Work {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Work")
            .field("version", &self.version)
            .field("id", &self.id)
            .field("repository", &self.repository)
            .field("title", &self.title)
            .field("description", &self.description)
            .field("request", &self.request)
            .field("source_branch", &self.source_branch)
            .field("target_branch", &self.target_branch)
            .field("starting_source", &self.starting_source)
            .field("starting_target", &self.starting_target)
            .field("latest_checkpoint", &self.latest_checkpoint)
            .field("development_sequence", &self.development_sequence)
            .field("next_task", &self.next_task)
            .field("next_finding", &self.next_finding)
            .field("created_at", &self.created_at)
            .field("outcome", &self.outcome)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
#[serde(rename_all = "snake_case")]
pub(super) enum WorkOutcomeKind {
    #[display("integrated")]
    Integrated,
    #[display("completed")]
    Completed,
    #[display("abandoned")]
    Abandoned,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct WorkOutcome {
    pub(super) kind: WorkOutcomeKind,
    pub(super) at: String,
    pub(super) summary: String,
    pub(super) source_commit: ObjectId,
    pub(super) target_commit: ObjectId,
}

impl fmt::Debug for WorkOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkOutcome")
            .field("kind", &self.kind)
            .field("at", &self.at)
            .field("summary", &self.summary)
            .field("source_commit", &self.source_commit)
            .field("target_commit", &self.target_commit)
            .finish()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, derive_more::Display,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum TaskStatus {
    #[display("pending")]
    Pending,
    #[display("running")]
    Running,
    #[display("blocked")]
    Blocked,
    #[display("passed")]
    Passed,
    #[display("failed")]
    Failed,
    #[display("cancelled")]
    Cancelled,
}

impl TaskStatus {
    pub(super) fn is_terminal(self) -> bool {
        matches!(self, Self::Passed | Self::Failed | Self::Cancelled)
    }
}

impl FromStr for TaskStatus {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "blocked" => Ok(Self::Blocked),
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(Error::InvalidTaskFilter(value.to_owned())),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, derive_more::Display,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Workflow {
    #[display("develop")]
    Develop,
    #[display("build")]
    Build,
    #[display("review")]
    Review,
    #[display("rebase")]
    Rebase,
    #[display("integrate")]
    Integrate,
}

impl FromStr for Workflow {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "develop" => Ok(Self::Develop),
            "build" => Ok(Self::Build),
            "review" => Ok(Self::Review),
            "rebase" => Ok(Self::Rebase),
            "integrate" => Ok(Self::Integrate),
            _ => Err(Error::InvalidTaskFilter(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
#[serde(rename_all = "snake_case")]
pub(super) enum BuildMode {
    #[display("feedback")]
    Feedback,
    #[display("acceptance")]
    Acceptance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
#[serde(rename_all = "snake_case")]
pub(super) enum BuildOperationStatus {
    #[display("waiting")]
    Waiting,
    #[display("running")]
    Running,
    #[display("blocked")]
    Blocked,
    #[display("passed")]
    Passed,
    #[display("failed")]
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct GitState {
    pub(super) head: String,
    pub(super) staged: Vec<String>,
    pub(super) unstaged: Vec<String>,
    pub(super) untracked: Vec<String>,
    pub(super) conflicted: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BuildOperation {
    pub(super) id: String,
    pub(super) context: Option<String>,
    pub(super) working_directory: String,
    pub(super) target: String,
    pub(super) triggers: Vec<String>,
    pub(super) identity: Option<String>,
    pub(super) stage: u32,
    pub(super) resource_group: Option<String>,
    pub(super) contract_digest: Option<String>,
    pub(super) status: BuildOperationStatus,
    pub(super) started_at: Option<String>,
    pub(super) completed_at: Option<String>,
    pub(super) duration_seconds: Option<u64>,
    pub(super) exit_status: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) proof: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BuildTask {
    pub(super) mode: BuildMode,
    pub(super) candidate: String,
    pub(super) policy_digest: Option<String>,
    pub(super) initial_git: GitState,
    pub(super) final_git: Option<GitState>,
    pub(super) operations: Vec<BuildOperation>,
    pub(super) proof: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewMode {
    #[display("feedback")]
    Feedback,
    #[display("acceptance")]
    Acceptance,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FindingStatus {
    #[default]
    Pending,
    Accepted,
    Dismissed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReviewEvidence {
    pub(super) path: String,
    pub(super) line: u32,
    pub(super) description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReviewFinding {
    #[serde(default)]
    pub(super) id: Option<String>,
    pub(super) title: String,
    pub(super) explanation: String,
    pub(super) rule_ids: Vec<String>,
    pub(super) evidence: Vec<ReviewEvidence>,
    pub(super) impact: String,
    pub(super) recommended_correction: String,
    #[serde(default)]
    pub(super) status: FindingStatus,
    #[serde(default)]
    pub(super) decision_reason: Option<String>,
    #[serde(default)]
    pub(super) corrective_task: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReviewCategory {
    pub(super) category: String,
    pub(super) grade: Option<ReviewGrade>,
    #[serde(default)]
    pub(super) not_applicable: bool,
    pub(super) explanation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReviewResult {
    pub(super) input_checksum: String,
    pub(super) overall_grade: ReviewGrade,
    pub(super) overall_explanation: String,
    pub(super) categories: Vec<ReviewCategory>,
    #[serde(default)]
    pub(super) proposed_actions: Vec<ReviewFinding>,
    #[serde(default)]
    pub(super) suggested_rule_improvements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReviewUnit {
    pub(super) id: String,
    pub(super) input_checksum: String,
    pub(super) request: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReviewTask {
    pub(super) mode: ReviewMode,
    pub(super) base: String,
    pub(super) candidate: String,
    pub(super) policy_digest: String,
    pub(super) content_digest: String,
    pub(super) reviewed_paths: Vec<String>,
    pub(super) build_task: Option<String>,
    pub(super) minimum_grade: Option<ReviewGrade>,
    pub(super) rule_ids: Vec<String>,
    pub(super) units: Vec<ReviewUnit>,
    pub(super) result: Option<ReviewResult>,
    pub(super) findings: Vec<ReviewFinding>,
    pub(super) quality_override: Option<String>,
    pub(super) proof: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum IntegrationStage {
    Preparing,
    Published,
    Merging,
    Merged,
    Cancelling,
    Cancelled,
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum FreshnessPolicy {
    #[display("strict")]
    Strict,
    #[default]
    #[display("loose")]
    Loose,
    #[display("merge_queue")]
    MergeQueue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PublishedBuildStatus {
    pub(super) identity: String,
    pub(super) build_task: String,
    pub(super) contract_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "durable booleans record independently resumable external side effects"
)]
pub(super) struct IntegrationTask {
    pub(super) stage: IntegrationStage,
    pub(super) repository: Option<String>,
    pub(super) source_branch: String,
    pub(super) target_branch: String,
    pub(super) candidate: String,
    pub(super) target_commit: String,
    pub(super) policy_digest: String,
    #[serde(default)]
    pub(super) freshness_policy: FreshnessPolicy,
    pub(super) build_task: String,
    pub(super) review_task: String,
    pub(super) review_grade: String,
    pub(super) quality_override: Option<String>,
    #[serde(default)]
    pub(super) review_findings: Vec<String>,
    #[serde(default)]
    pub(super) pushed: bool,
    #[serde(default)]
    pub(super) published_builds: Vec<PublishedBuildStatus>,
    #[serde(default)]
    pub(super) aggregate_build_published: bool,
    pub(super) pull_request_number: Option<u64>,
    pub(super) pull_request_url: Option<String>,
    pub(super) pull_request_head: Option<String>,
    pub(super) pull_request_base: Option<String>,
    #[serde(default)]
    pub(super) pull_request_closed: bool,
    #[serde(default)]
    pub(super) remote_branch_deleted: bool,
    pub(super) merge_commit: Option<String>,
    pub(super) cancellation_reason: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Task {
    pub(super) version: u16,
    pub(super) id: String,
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) workflow: Workflow,
    pub(super) title: String,
    pub(super) description: String,
    pub(super) origin: String,
    #[serde(default)]
    pub(super) related: Vec<String>,
    pub(super) status: TaskStatus,
    pub(super) source_commit: String,
    pub(super) created_at: String,
    pub(super) completed_at: Option<String>,
    pub(super) result: Option<String>,
    pub(super) output: Option<String>,
    pub(super) continuation: Option<String>,
    #[serde(default)]
    pub(super) payload: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) build: Option<BuildTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) review: Option<ReviewTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) integration: Option<IntegrationTask>,
}

impl Task {
    #[expect(
        clippy::too_many_arguments,
        reason = "the common Task envelope is deliberately complete at creation"
    )]
    pub(super) fn new(
        id: String,
        task_type: impl Into<String>,
        workflow: Workflow,
        title: impl Into<String>,
        description: impl Into<String>,
        origin: impl Into<String>,
        status: TaskStatus,
        source_commit: impl Into<String>,
        created_at: impl Into<String>,
        continuation: Option<String>,
    ) -> Self {
        Self {
            version: TASK_SCHEMA_VERSION,
            id,
            kind: task_type.into(),
            workflow,
            title: title.into(),
            description: description.into(),
            origin: origin.into(),
            related: Vec::new(),
            status,
            source_commit: source_commit.into(),
            created_at: created_at.into(),
            completed_at: None,
            result: None,
            output: None,
            continuation,
            payload: BTreeMap::new(),
            build: None,
            review: None,
            integration: None,
        }
    }

    pub(super) fn finish(
        &mut self,
        status: TaskStatus,
        at: String,
        result: String,
        output: Option<String>,
    ) {
        let started_at = self.payload.get("started_at").unwrap_or(&self.created_at);
        if let (Ok(started), Ok(completed)) = (
            DateTime::parse_from_rfc3339(started_at),
            DateTime::parse_from_rfc3339(&at),
        ) {
            self.payload.insert(
                "duration_seconds".to_owned(),
                completed
                    .signed_duration_since(started)
                    .num_seconds()
                    .max(0)
                    .to_string(),
            );
        }
        self.status = status;
        self.completed_at = Some(at);
        self.result = Some(result);
        self.output = output;
        self.continuation = None;
    }

    pub(super) fn is_develop_action(&self) -> bool {
        self.kind == "action" && self.workflow == Workflow::Develop
    }
}

impl fmt::Debug for Task {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Task")
            .field("version", &self.version)
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("workflow", &self.workflow)
            .field("title", &self.title)
            .field("description", &self.description)
            .field("origin", &self.origin)
            .field("related", &self.related)
            .field("status", &self.status)
            .field("source_commit", &self.source_commit)
            .field("created_at", &self.created_at)
            .field("completed_at", &self.completed_at)
            .field("result", &self.result)
            .field("output", &self.output)
            .field("continuation", &self.continuation)
            .field("payload", &self.payload)
            .field("build", &self.build)
            .field("review", &self.review)
            .field("integration", &self.integration)
            .finish()
    }
}

fn required(value: String) -> Result<String, Error> {
    if value.trim().is_empty() {
        Err(Error::EmptyField)
    } else {
        Ok(value)
    }
}

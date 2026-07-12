//! Phase 3 Work and Task domain values.

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

use super::Error;

pub(super) const WORK_SCHEMA_VERSION: u16 = 1;
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
            .field("value_length", &self.value.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Work {
    pub(super) version: u16,
    pub(super) id: Uuid,
    pub(super) title: String,
    pub(super) description: String,
    pub(super) request: RequestSource,
    pub(super) source_branch: String,
    pub(super) target_branch: String,
    pub(super) starting_source: String,
    pub(super) starting_target: String,
    pub(super) latest_checkpoint: Option<String>,
    #[serde(default)]
    pub(super) development_sequence: Vec<String>,
    pub(super) next_task: u32,
    pub(super) created_at: String,
    pub(super) outcome: Option<String>,
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
        source_branch: String,
        target_branch: String,
        starting_source: String,
        starting_target: String,
        created_at: String,
    ) -> Result<Self, Error> {
        Ok(Self {
            version: WORK_SCHEMA_VERSION,
            id: Uuid::new_v4(),
            title: required(title)?,
            description: required(description)?,
            request,
            source_branch: required(source_branch)?,
            target_branch: required(target_branch)?,
            starting_source,
            starting_target,
            latest_checkpoint: None,
            development_sequence: Vec::new(),
            next_task: 1,
            created_at,
            outcome: None,
        })
    }

    pub(super) fn allocate_task_id(&mut self) -> Result<String, Error> {
        let id = format!("TASK_{:03}", self.next_task);
        self.next_task = self
            .next_task
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
            .field("title_length", &self.title.len())
            .field("description_length", &self.description.len())
            .field("request", &self.request)
            .field("source_branch", &self.source_branch)
            .field("target_branch", &self.target_branch)
            .field("starting_source", &"[redacted]")
            .field("starting_target", &"[redacted]")
            .field("has_checkpoint", &self.latest_checkpoint.is_some())
            .field("development_tasks", &self.development_sequence.len())
            .field("next_task", &self.next_task)
            .field("created_at", &self.created_at)
            .field("has_outcome", &self.outcome.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TaskStatus {
    Pending,
    Running,
    Blocked,
    Passed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub(super) fn is_terminal(self) -> bool {
        matches!(self, Self::Passed | Self::Failed | Self::Cancelled)
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Workflow {
    Develop,
    Build,
    Review,
    Rebase,
    Integrate,
}

impl fmt::Display for Workflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Develop => "develop",
            Self::Build => "build",
            Self::Review => "review",
            Self::Rebase => "rebase",
            Self::Integrate => "integrate",
        })
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum BuildMode {
    Feedback,
    Acceptance,
}

impl fmt::Display for BuildMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Feedback => "feedback",
            Self::Acceptance => "acceptance",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum BuildOperationStatus {
    Waiting,
    Running,
    Blocked,
    Passed,
    Failed,
}

impl fmt::Display for BuildOperationStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Passed => "passed",
            Self::Failed => "failed",
        })
    }
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
            .field("title_length", &self.title.len())
            .field("description_length", &self.description.len())
            .field("origin", &self.origin)
            .field("related_count", &self.related.len())
            .field("status", &self.status)
            .field("source_commit", &"[redacted]")
            .field("created_at", &self.created_at)
            .field("completed_at", &self.completed_at)
            .field("has_result", &self.result.is_some())
            .field("has_output", &self.output.is_some())
            .field("continuation", &self.continuation)
            .field("payload_keys", &self.payload.keys().collect::<Vec<_>>())
            .field("has_build", &self.build.is_some())
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

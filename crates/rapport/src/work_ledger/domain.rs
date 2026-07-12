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
        }
    }

    pub(super) fn finish(
        &mut self,
        status: TaskStatus,
        at: String,
        result: String,
        output: Option<String>,
    ) {
        if let (Ok(started), Ok(completed)) = (
            DateTime::parse_from_rfc3339(&self.created_at),
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

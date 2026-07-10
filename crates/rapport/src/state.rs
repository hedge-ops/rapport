use crate::paths::RapportPaths;
use rapport_files::FileSystem;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::io;
use std::str::FromStr;

pub const WORK_STATE_SCHEMA_VERSION: u16 = 3;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkState {
    pub schema_version: u16,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    pub stage: WorkStage,
    pub status: WorkStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<WorkFact>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub builds: BTreeMap<String, BuildState>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reviews: BTreeMap<String, ReviewState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrate: Option<WorkFact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signoff: Option<WorkFact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete: Option<WorkFact>,
}

impl fmt::Debug for WorkState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkState")
            .field("schema_version", &self.schema_version)
            .field("title", &RedactedText(&self.title))
            .field("has_objective", &self.objective.is_some())
            .field("has_ticket", &self.ticket.is_some())
            .field("has_plan", &self.plan.is_some())
            .field("path_count", &self.paths.len())
            .field("stage", &self.stage)
            .field("status", &self.status)
            .field("created_at", &RedactedText(&self.created_at))
            .field("updated_at", &RedactedText(&self.updated_at))
            .field("has_legacy_build", &self.build.is_some())
            .field("build_count", &self.builds.len())
            .field("review_count", &self.reviews.len())
            .field("has_integration", &self.integrate.is_some())
            .field("has_signoff", &self.signoff.is_some())
            .field("has_completion", &self.complete.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildState {
    pub status: OperationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_status: Option<OperationStatus>,
    pub target: String,
    pub declaring_context: String,
    pub paths: Vec<String>,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub content_checksum: String,
    pub instructions_checksum: String,
    pub input_checksum: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl fmt::Debug for BuildState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildState")
            .field("status", &self.status)
            .field("result_status", &self.result_status)
            .field("target", &RedactedText(&self.target))
            .field("declaring_context", &RedactedText(&self.declaring_context))
            .field("path_count", &self.paths.len())
            .field("at", &self.at)
            .field("has_base_sha", &self.base_sha.is_some())
            .field("has_head_sha", &self.head_sha.is_some())
            .field("content_checksum", &RedactedText(&self.content_checksum))
            .field(
                "instructions_checksum",
                &RedactedText(&self.instructions_checksum),
            )
            .field("input_checksum", &RedactedText(&self.input_checksum))
            .field("command", &RedactedText(&self.command))
            .field("description", &RedactedText(&self.description))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Pending,
    Pass,
    Fail,
    Stale,
}

impl fmt::Display for OperationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Pass => f.write_str("pass"),
            Self::Fail => f.write_str("fail"),
            Self::Stale => f.write_str("stale"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewState {
    pub status: OperationStatus,
    pub minimum_grade: ReviewGrade,
    pub declaring_context: String,
    pub reviewed_paths: Vec<String>,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub content_checksum: String,
    pub rules_checksum: String,
    pub instructions_checksum: String,
    pub input_checksum: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<ReviewGrade>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ReviewAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ReviewAttempt>,
}

impl fmt::Debug for ReviewState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReviewState")
            .field("status", &self.status)
            .field("minimum_grade", &self.minimum_grade)
            .field("declaring_context", &RedactedText(&self.declaring_context))
            .field("reviewed_path_count", &self.reviewed_paths.len())
            .field("at", &self.at)
            .field("has_base_sha", &self.base_sha.is_some())
            .field("has_head_sha", &self.head_sha.is_some())
            .field("content_checksum", &RedactedText(&self.content_checksum))
            .field("rules_checksum", &RedactedText(&self.rules_checksum))
            .field(
                "instructions_checksum",
                &RedactedText(&self.instructions_checksum),
            )
            .field("input_checksum", &RedactedText(&self.input_checksum))
            .field("grade", &self.grade)
            .field("description", &RedactedText(&self.description))
            .field("action_count", &self.actions.len())
            .field("attempt_count", &self.attempts.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAttempt {
    pub status: OperationStatus,
    pub at: String,
    pub input_checksum: String,
    pub grade: ReviewGrade,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ReviewAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_action_ids: Vec<String>,
}

impl fmt::Debug for ReviewAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReviewAttempt")
            .field("status", &self.status)
            .field("at", &self.at)
            .field("input_checksum", &RedactedText(&self.input_checksum))
            .field("grade", &self.grade)
            .field("description", &RedactedText(&self.description))
            .field("action_count", &self.actions.len())
            .field("resolved_action_count", &self.resolved_action_ids.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAction {
    pub id: String,
    #[serde(default)]
    pub status: ReviewActionStatus,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rule_ids: Vec<String>,
    pub evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressed_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

impl fmt::Debug for ReviewAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReviewAction")
            .field("id", &RedactedText(&self.id))
            .field("status", &self.status)
            .field("title", &RedactedText(&self.title))
            .field("rule_id_count", &self.rule_ids.len())
            .field("evidence", &RedactedText(&self.evidence))
            .field("has_addressed_at", &self.addressed_at.is_some())
            .field("has_addressed_summary", &self.addressed_summary.is_some())
            .field("has_resolved_at", &self.resolved_at.is_some())
            .finish()
    }
}

impl ReviewAction {
    #[must_use]
    pub fn is_outstanding(&self) -> bool {
        self.status != ReviewActionStatus::Resolved
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewActionStatus {
    #[default]
    Open,
    Addressed,
    Resolved,
}

impl fmt::Display for ReviewActionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::Addressed => f.write_str("addressed"),
            Self::Resolved => f.write_str("resolved"),
        }
    }
}

struct RedactedText<'a>(&'a str);

impl fmt::Debug for RedactedText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted; {} bytes>", self.0.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReviewGrade(u8);

impl ReviewGrade {
    pub const DEFAULT_MINIMUM: Self = Self(12);

    #[must_use]
    pub const fn meets(self, minimum: Self) -> bool {
        self.0 >= minimum.0
    }

    fn label(self) -> &'static str {
        match self.0 {
            14 => "A+",
            13 => "A",
            12 => "A-",
            11 => "B+",
            10 => "B",
            9 => "B-",
            8 => "C+",
            7 => "C",
            6 => "C-",
            5 => "D+",
            4 => "D",
            3 => "D-",
            2 => "F+",
            1 => "F",
            _ => "F-",
        }
    }
}

impl Default for ReviewGrade {
    fn default() -> Self {
        Self::DEFAULT_MINIMUM
    }
}

impl fmt::Display for ReviewGrade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl FromStr for ReviewGrade {
    type Err = ReviewGradeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_uppercase();
        let score = match normalized.as_str() {
            "A+" => 14,
            "A" => 13,
            "A-" => 12,
            "B+" => 11,
            "B" => 10,
            "B-" => 9,
            "C+" => 8,
            "C" => 7,
            "C-" => 6,
            "D+" => 5,
            "D" => 4,
            "D-" => 3,
            "F+" => 2,
            "F" => 1,
            "F-" => 0,
            _ => {
                return Err(ReviewGradeError {
                    value: value.to_string(),
                });
            }
        };
        Ok(Self(score))
    }
}

impl Serialize for ReviewGrade {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for ReviewGrade {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ReviewGradeError {
    value: String,
}

impl fmt::Debug for ReviewGradeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReviewGradeError")
            .field("value", &RedactedText(&self.value))
            .finish()
    }
}

impl fmt::Display for ReviewGradeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid review grade; expected A through F with optional + or -")
    }
}

impl Error for ReviewGradeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStage {
    Development,
}

impl fmt::Display for WorkStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Development => f.write_str("development"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Active,
    Complete,
}

impl fmt::Display for WorkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::Complete => f.write_str("complete"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkFact {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<String>,
}

impl fmt::Debug for WorkFact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkFact")
            .field("status", &RedactedText(&self.status))
            .field("has_at", &self.at.is_some())
            .field("has_summary", &self.summary.is_some())
            .field("has_message", &self.message.is_some())
            .field("has_commit", &self.commit.is_some())
            .field("has_branch", &self.branch.is_some())
            .field("has_pr_url", &self.pr_url.is_some())
            .field("required_count", &self.required.len())
            .field("passed_count", &self.passed.len())
            .field("failed_count", &self.failed.len())
            .field("pending_count", &self.pending.len())
            .finish()
    }
}

impl WorkFact {
    #[must_use]
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            at: None,
            summary: None,
            message: None,
            commit: None,
            branch: None,
            pr_url: None,
            required: Vec::new(),
            passed: Vec::new(),
            failed: Vec::new(),
            pending: Vec::new(),
        }
    }

    #[must_use]
    pub fn at(mut self, timestamp: impl Into<String>) -> Self {
        self.at = Some(timestamp.into());
        self
    }

    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }
}

impl WorkState {
    #[must_use]
    pub fn new(title: impl Into<String>, created_at: impl Into<String>) -> Self {
        let timestamp = created_at.into();
        Self {
            schema_version: WORK_STATE_SCHEMA_VERSION,
            title: title.into(),
            objective: None,
            ticket: None,
            plan: None,
            paths: Vec::new(),
            stage: WorkStage::Development,
            status: WorkStatus::Active,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            build: None,
            builds: BTreeMap::new(),
            reviews: BTreeMap::new(),
            integrate: None,
            signoff: None,
            complete: None,
        }
    }

    #[must_use]
    pub fn with_objective(mut self, objective: Option<String>) -> Self {
        self.objective = objective;
        self
    }

    #[must_use]
    pub fn with_ticket(mut self, ticket: Option<String>) -> Self {
        self.ticket = ticket;
        self
    }

    #[must_use]
    pub fn with_plan(mut self, plan: Option<String>) -> Self {
        self.plan = plan;
        self
    }

    #[must_use]
    pub fn with_paths(mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.paths = paths.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Debug, Clone)]
pub struct WorkStateStore {
    paths: RapportPaths,
}

impl WorkStateStore {
    #[must_use]
    pub fn new(paths: RapportPaths) -> Self {
        Self { paths }
    }

    /// Load the active work state when it exists.
    ///
    /// # Errors
    ///
    /// Returns [`WorkStateError`] when the state file cannot be read or parsed.
    pub fn load(&self, fs: &impl FileSystem) -> Result<Option<WorkState>, WorkStateError> {
        let path = self.paths.work_state_file();
        if !fs.is_file(&path) {
            return Ok(None);
        }
        let contents = fs.read_to_string(&path)?;
        let mut state: WorkState = toml::from_str(&contents)?;
        if state.schema_version > WORK_STATE_SCHEMA_VERSION {
            return Err(WorkStateError::UnsupportedSchemaVersion {
                version: state.schema_version,
            });
        }
        migrate_review_action_ids(&mut state);
        Ok(Some(state))
    }

    /// Save the active work state to `.rapport/work.toml`.
    ///
    /// # Errors
    ///
    /// Returns [`WorkStateError`] when the `.rapport` directory cannot be
    /// created, the state cannot be encoded, or the file cannot be written.
    pub fn save(&self, fs: &mut impl FileSystem, state: &WorkState) -> Result<(), WorkStateError> {
        fs.create_dir_all(self.paths.rapport_dir())?;
        let mut current = state.clone();
        current.schema_version = WORK_STATE_SCHEMA_VERSION;
        let contents = toml::to_string_pretty(&current)?;
        fs.write_string(self.paths.work_state_file(), contents)?;
        Ok(())
    }

    /// Archive a work state under `.rapport/history`.
    ///
    /// # Errors
    ///
    /// Returns [`WorkStateError`] when the history directory cannot be
    /// created, the state cannot be encoded, or the archive file cannot be
    /// written.
    pub fn archive(
        &self,
        fs: &mut impl FileSystem,
        filename: &str,
        state: &WorkState,
    ) -> Result<(), WorkStateError> {
        fs.create_dir_all(self.paths.history_dir())?;
        let mut current = state.clone();
        current.schema_version = WORK_STATE_SCHEMA_VERSION;
        let contents = toml::to_string_pretty(&current)?;
        fs.write_string(self.paths.history_file(filename), contents)?;
        Ok(())
    }

    /// Remove the active work state.
    ///
    /// # Errors
    ///
    /// Returns [`WorkStateError`] when `.rapport/work.toml` cannot be removed.
    pub fn clear(&self, fs: &mut impl FileSystem) -> Result<(), WorkStateError> {
        fs.remove_file(self.paths.work_state_file())?;
        Ok(())
    }
}

fn migrate_review_action_ids(state: &mut WorkState) {
    if state.schema_version >= 3 {
        return;
    }
    let mut next_number = state
        .reviews
        .values()
        .flat_map(review_action_ids)
        .filter_map(|id| id.strip_prefix("REV-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .unwrap_or(1);
    let mut used = BTreeSet::new();
    for review in state.reviews.values_mut() {
        let mut replacements = BTreeMap::new();
        for old_id in review_action_ids(review) {
            if replacements.contains_key(old_id) {
                continue;
            }
            let replacement = if used.insert(old_id.to_string()) {
                old_id.to_string()
            } else {
                loop {
                    let candidate = format!("REV-{next_number:03}");
                    next_number = next_number.checked_add(1).unwrap_or(1);
                    if used.insert(candidate.clone()) {
                        break candidate;
                    }
                }
            };
            replacements.insert(old_id.to_string(), replacement);
        }
        for action in &mut review.actions {
            replace_action_id(action, &replacements);
        }
        for attempt in &mut review.attempts {
            for action in &mut attempt.actions {
                replace_action_id(action, &replacements);
            }
            for id in &mut attempt.resolved_action_ids {
                if let Some(replacement) = replacements.get(id) {
                    id.clone_from(replacement);
                }
            }
        }
    }
}

fn review_action_ids(review: &ReviewState) -> impl Iterator<Item = &str> {
    review
        .actions
        .iter()
        .map(|action| action.id.as_str())
        .chain(
            review
                .attempts
                .iter()
                .flat_map(|attempt| attempt.actions.iter().map(|action| action.id.as_str())),
        )
        .chain(
            review
                .attempts
                .iter()
                .flat_map(|attempt| attempt.resolved_action_ids.iter().map(String::as_str)),
        )
}

fn replace_action_id(action: &mut ReviewAction, replacements: &BTreeMap<String, String>) {
    if let Some(replacement) = replacements.get(&action.id) {
        action.id.clone_from(replacement);
    }
}

#[derive(Debug)]
pub enum WorkStateError {
    Io(io::Error),
    Decode(toml::de::Error),
    Encode(toml::ser::Error),
    UnsupportedSchemaVersion { version: u16 },
}

impl fmt::Display for WorkStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "work state filesystem error: {error}"),
            Self::Decode(error) => write!(f, "work state parse error: {error}"),
            Self::Encode(error) => write!(f, "work state encode error: {error}"),
            Self::UnsupportedSchemaVersion { version } => write!(
                f,
                "unsupported work state schema version `{version}`; supported version is `{WORK_STATE_SCHEMA_VERSION}`"
            ),
        }
    }
}

impl Error for WorkStateError {}

impl From<io::Error> for WorkStateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<toml::de::Error> for WorkStateError {
    fn from(error: toml::de::Error) -> Self {
        Self::Decode(error)
    }
}

impl From<toml::ser::Error> for WorkStateError {
    fn from(error: toml::ser::Error) -> Self {
        Self::Encode(error)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;

    #[test]
    fn work_state_load_returns_none_when_missing() {
        let fs = InMemoryFileSystem::default();
        let store = WorkStateStore::new(RapportPaths::new("/repo"));

        assert_eq!(store.load(&fs).unwrap(), None);
    }

    #[test]
    fn work_state_saves_and_loads_minimal_state() {
        let mut fs = InMemoryFileSystem::default();
        let store = WorkStateStore::new(RapportPaths::new("/repo"));
        let state = WorkState::new("Do the thing", "2026-07-07T23:00:00Z");

        store.save(&mut fs, &state).unwrap();

        assert_eq!(store.load(&fs).unwrap(), Some(state));
        assert!(fs.is_dir("/repo/.rapport"));
    }

    #[test]
    fn work_state_archives_and_clears_active_state() {
        let mut fs = InMemoryFileSystem::default();
        let store = WorkStateStore::new(RapportPaths::new("/repo"));
        let state = WorkState::new("Do the thing", "2026-07-07T23:00:00Z");

        store.save(&mut fs, &state).unwrap();
        store
            .archive(&mut fs, "2026-07-07T23-00-00Z-do-the-thing.toml", &state)
            .unwrap();
        store.clear(&mut fs).unwrap();

        assert_eq!(store.load(&fs).unwrap(), None);
        assert!(fs.is_file("/repo/.rapport/history/2026-07-07T23-00-00Z-do-the-thing.toml"));
    }

    #[test]
    fn work_state_loads_active_work_fixture() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/.rapport/work.toml",
            r#"
schema_version = 1
title = "Do the thing"
objective = "Make it real"
ticket = "PW-123"
paths = ["app/api", "app/core"]
stage = "development"
status = "active"
created_at = "2026-07-07T23:00:00Z"
updated_at = "2026-07-07T23:00:00Z"
"#,
        )
        .unwrap();
        let store = WorkStateStore::new(RapportPaths::new("/repo"));

        let state = store.load(&fs).unwrap().unwrap();

        assert_eq!(state.title, "Do the thing");
        assert_eq!(state.objective.as_deref(), Some("Make it real"));
        assert_eq!(state.ticket.as_deref(), Some("PW-123"));
        assert_eq!(state.paths, vec!["app/api", "app/core"]);
    }

    #[test]
    fn work_state_save_should_migrate_older_schema_to_current_version() {
        let mut fs = InMemoryFileSystem::default();
        let store = WorkStateStore::new(RapportPaths::new("/repo"));
        let mut state = WorkState::new("Do the thing", "2026-07-07T23:00:00Z");
        state.schema_version = 1;

        store.save(&mut fs, &state).unwrap();

        let contents = fs.read_to_string("/repo/.rapport/work.toml").unwrap();
        assert!(contents.contains("schema_version = 3"));
    }

    #[test]
    fn work_state_load_should_make_legacy_review_action_ids_work_global() {
        let mut fs = InMemoryFileSystem::default();
        let store = WorkStateStore::new(RapportPaths::new("/repo"));
        let action = ReviewAction {
            id: String::from("REV-001"),
            status: ReviewActionStatus::Open,
            title: String::from("Legacy action"),
            rule_ids: vec![String::from("RULE-001")],
            evidence: String::from("src/lib.rs:1"),
            addressed_at: None,
            addressed_summary: None,
            resolved_at: None,
        };
        let attempt = ReviewAttempt {
            status: OperationStatus::Fail,
            at: String::from("2026-07-10T18:00:00Z"),
            input_checksum: String::from("checksum"),
            grade: "B+".parse().unwrap(),
            description: String::from("Legacy attempt"),
            actions: vec![action.clone()],
            resolved_action_ids: vec![action.id.clone()],
        };
        let review = ReviewState {
            status: OperationStatus::Fail,
            minimum_grade: ReviewGrade::DEFAULT_MINIMUM,
            declaring_context: String::from("."),
            reviewed_paths: vec![String::from(".")],
            at: String::from("2026-07-10T18:00:00Z"),
            base_sha: None,
            head_sha: None,
            content_checksum: String::from("content"),
            rules_checksum: String::from("rules"),
            instructions_checksum: String::from("instructions"),
            input_checksum: String::from("input"),
            grade: Some("B+".parse().unwrap()),
            description: String::from("Legacy review"),
            actions: vec![action],
            attempts: vec![attempt],
        };
        let mut state = WorkState::new("Legacy", "2026-07-10T18:00:00Z");
        state.schema_version = 2;
        state
            .reviews
            .insert(String::from("a-review"), review.clone());
        state.reviews.insert(String::from("b-review"), review);
        fs.write_string(
            "/repo/.rapport/work.toml",
            toml::to_string_pretty(&state).unwrap(),
        )
        .unwrap();

        let migrated = store.load(&fs).unwrap().unwrap();

        assert_eq!(migrated.reviews["a-review"].actions[0].id, "REV-001");
        assert_eq!(migrated.reviews["b-review"].actions[0].id, "REV-002");
        assert_eq!(
            migrated.reviews["b-review"].attempts[0].actions[0].id,
            "REV-002"
        );
        assert_eq!(
            migrated.reviews["b-review"].attempts[0].resolved_action_ids,
            vec!["REV-002"]
        );
    }

    #[test]
    fn work_state_load_should_reject_future_schema_versions() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string(
            "/repo/.rapport/work.toml",
            r#"schema_version = 999
title = "Future"
paths = []
stage = "development"
status = "active"
created_at = "2026-07-07T23:00:00Z"
updated_at = "2026-07-07T23:00:00Z"
"#,
        )
        .unwrap();

        let error = WorkStateStore::new(RapportPaths::new("/repo"))
            .load(&fs)
            .unwrap_err();

        assert!(error.to_string().contains("schema version `999`"));
    }

    #[test]
    fn work_state_reports_invalid_toml() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/.rapport/work.toml", "schema_version =")
            .unwrap();
        let store = WorkStateStore::new(RapportPaths::new("/repo"));

        let error = store.load(&fs).unwrap_err();

        assert!(error.to_string().contains("work state parse error"));
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one privacy regression fixture covers every nested work-state diagnostic type"
    )]
    fn operation_debug_output_redacts_privacy_bearing_text() {
        let grade_error = "PRIVATE-GRADE".parse::<ReviewGrade>().unwrap_err();
        let build = BuildState {
            status: OperationStatus::Fail,
            result_status: Some(OperationStatus::Fail),
            target: String::from("PRIVATE-BUILD-TARGET"),
            declaring_context: String::from("private/build-context"),
            paths: vec![String::from("private/build-file.rs")],
            at: String::from("2026-07-10T18:00:00Z"),
            base_sha: Some(String::from("private-build-base-sha")),
            head_sha: Some(String::from("private-build-head-sha")),
            content_checksum: String::from("private-build-content-checksum"),
            instructions_checksum: String::from("private-build-instructions-checksum"),
            input_checksum: String::from("private-build-input-checksum"),
            command: String::from("private build command"),
            description: String::from("private captured build output"),
        };
        let action = ReviewAction {
            id: String::from("PRIVATE-ACTION-ID"),
            status: ReviewActionStatus::Addressed,
            title: String::from("private action title"),
            rule_ids: vec![String::from("PRIVATE-RULE-ID")],
            evidence: String::from("private/file.rs:7 contains sensitive evidence"),
            addressed_at: Some(String::from("PRIVATE-ADDRESSED-TIME")),
            addressed_summary: Some(String::from("PRIVATE-ADDRESSED-SUMMARY")),
            resolved_at: Some(String::from("PRIVATE-RESOLVED-TIME")),
        };
        let attempt = ReviewAttempt {
            status: OperationStatus::Fail,
            at: String::from("2026-07-10T18:00:00Z"),
            input_checksum: String::from("private-input-checksum"),
            grade: "B+".parse().unwrap(),
            description: String::from("private attempt description"),
            actions: vec![action.clone()],
            resolved_action_ids: vec![String::from("PRIVATE-RESOLVED-ID")],
        };
        let state = ReviewState {
            status: OperationStatus::Fail,
            minimum_grade: ReviewGrade::DEFAULT_MINIMUM,
            declaring_context: String::from("private/component"),
            reviewed_paths: vec![String::from("private/file.rs")],
            at: String::from("2026-07-10T18:00:00Z"),
            base_sha: Some(String::from("private-base-sha")),
            head_sha: Some(String::from("private-head-sha")),
            content_checksum: String::from("private-content-checksum"),
            rules_checksum: String::from("private-rules-checksum"),
            instructions_checksum: String::from("private-instructions-checksum"),
            input_checksum: String::from("private-input-checksum"),
            grade: Some("B+".parse().unwrap()),
            description: String::from("private state description"),
            actions: vec![action.clone()],
            attempts: vec![attempt.clone()],
        };
        let fact = WorkFact {
            status: String::from("PRIVATE-FACT-STATUS"),
            at: Some(String::from("PRIVATE-FACT-TIME")),
            summary: Some(String::from("PRIVATE-FACT-SUMMARY")),
            message: Some(String::from("PRIVATE-FACT-MESSAGE")),
            commit: Some(String::from("PRIVATE-FACT-COMMIT")),
            branch: Some(String::from("PRIVATE-FACT-BRANCH")),
            pr_url: Some(String::from("PRIVATE-FACT-URL")),
            required: vec![String::from("PRIVATE-REQUIRED")],
            passed: vec![String::from("PRIVATE-PASSED")],
            failed: vec![String::from("PRIVATE-FAILED")],
            pending: vec![String::from("PRIVATE-PENDING")],
        };
        let mut work = WorkState::new("PRIVATE-WORK-TITLE", "PRIVATE-WORK-CREATED");
        work.objective = Some(String::from("PRIVATE-WORK-OBJECTIVE"));
        work.ticket = Some(String::from("PRIVATE-WORK-TICKET"));
        work.plan = Some(String::from("PRIVATE-WORK-PLAN"));
        work.paths = vec![String::from("PRIVATE-WORK-PATH")];
        work.updated_at = String::from("PRIVATE-WORK-UPDATED");
        work.integrate = Some(fact.clone());

        let debug = format!(
            "{build:?} {state:?} {attempt:?} {action:?} {grade_error:?} {grade_error} {fact:?} {work:?}"
        );

        for private_value in [
            "PRIVATE-BUILD-TARGET",
            "PRIVATE-GRADE",
            "private/build-context",
            "private/build-file.rs",
            "private-build-base-sha",
            "private-build-input-checksum",
            "private build command",
            "private captured build output",
            "PRIVATE-ACTION-ID",
            "PRIVATE-ADDRESSED-TIME",
            "PRIVATE-ADDRESSED-SUMMARY",
            "PRIVATE-RESOLVED-TIME",
            "private action title",
            "PRIVATE-RULE-ID",
            "sensitive evidence",
            "private attempt description",
            "PRIVATE-RESOLVED-ID",
            "private/component",
            "private/file.rs",
            "private-base-sha",
            "private-input-checksum",
            "private state description",
            "PRIVATE-FACT-STATUS",
            "PRIVATE-FACT-TIME",
            "PRIVATE-FACT-SUMMARY",
            "PRIVATE-FACT-MESSAGE",
            "PRIVATE-FACT-COMMIT",
            "PRIVATE-FACT-BRANCH",
            "PRIVATE-FACT-URL",
            "PRIVATE-REQUIRED",
            "PRIVATE-PASSED",
            "PRIVATE-FAILED",
            "PRIVATE-PENDING",
            "PRIVATE-WORK-TITLE",
            "PRIVATE-WORK-CREATED",
            "PRIVATE-WORK-OBJECTIVE",
            "PRIVATE-WORK-TICKET",
            "PRIVATE-WORK-PLAN",
            "PRIVATE-WORK-PATH",
            "PRIVATE-WORK-UPDATED",
        ] {
            assert!(!debug.contains(private_value));
        }
        assert!(debug.contains("<redacted;"));
        assert!(debug.contains("action_count: 1"));
        assert!(debug.contains("attempt_count: 1"));
    }
}

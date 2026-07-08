use crate::paths::RapportPaths;
use rapport_files::FileSystem;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::io;

pub const WORK_STATE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrate: Option<WorkFact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signoff: Option<WorkFact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete: Option<WorkFact>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkFact {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
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

impl WorkFact {
    #[must_use]
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            at: None,
            summary: None,
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
        let state = toml::from_str(&contents)?;
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
        let contents = toml::to_string_pretty(state)?;
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
        let contents = toml::to_string_pretty(state)?;
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

#[derive(Debug)]
pub enum WorkStateError {
    Io(io::Error),
    Decode(toml::de::Error),
    Encode(toml::ser::Error),
}

impl fmt::Display for WorkStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "work state filesystem error: {error}"),
            Self::Decode(error) => write!(f, "work state parse error: {error}"),
            Self::Encode(error) => write!(f, "work state encode error: {error}"),
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
    fn work_state_reports_invalid_toml() {
        let mut fs = InMemoryFileSystem::default();
        fs.write_string("/repo/.rapport/work.toml", "schema_version =")
            .unwrap();
        let store = WorkStateStore::new(RapportPaths::new("/repo"));

        let error = store.load(&fs).unwrap_err();

        assert!(error.to_string().contains("work state parse error"));
    }
}

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
}

impl WorkState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema_version: WORK_STATE_SCHEMA_VERSION,
        }
    }
}

impl Default for WorkState {
    fn default() -> Self {
        Self::new()
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

        store.save(&mut fs, &WorkState::new()).unwrap();

        assert_eq!(store.load(&fs).unwrap(), Some(WorkState::new()));
        assert!(fs.is_dir("/repo/.rapport"));
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

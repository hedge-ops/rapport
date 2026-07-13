//! Repository-local Rapport paths.
//!
//! This module owns canonical locations for active Work, Tasks, history, and
//! other local workflow state.

use rapport_files::{Utf8Path, Utf8PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RapportPaths {
    repo_root: Utf8PathBuf,
}

impl RapportPaths {
    #[must_use]
    pub fn new(repo_root: impl Into<Utf8PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    #[must_use]
    pub fn repo_root(&self) -> &Utf8Path {
        &self.repo_root
    }

    #[must_use]
    pub fn rapport_dir(&self) -> Utf8PathBuf {
        self.repo_root.join(".rapport")
    }

    #[must_use]
    pub fn work_state_file(&self) -> Utf8PathBuf {
        self.rapport_dir().join("work.toml")
    }

    #[must_use]
    pub fn history_dir(&self) -> Utf8PathBuf {
        self.rapport_dir().join("history")
    }

    #[must_use]
    pub fn history_file(&self, filename: &str) -> Utf8PathBuf {
        self.history_dir().join(filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapport_paths_resolve_local_state_files_under_repo_root() {
        let paths = RapportPaths::new("/repo");

        assert_eq!(paths.rapport_dir(), Utf8PathBuf::from("/repo/.rapport"));
        assert_eq!(
            paths.work_state_file(),
            Utf8PathBuf::from("/repo/.rapport/work.toml")
        );
        assert_eq!(
            paths.history_dir(),
            Utf8PathBuf::from("/repo/.rapport/history")
        );
        assert_eq!(
            paths.history_file("done.toml"),
            Utf8PathBuf::from("/repo/.rapport/history/done.toml")
        );
    }
}

//! Repository-local Rapport paths.
//!
//! Owns the repository root and directory used for shared standards.

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
}

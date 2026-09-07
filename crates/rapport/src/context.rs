//! Shared command execution context.
//!
//! Owns repository discovery and the filesystem and output boundaries used by
//! architecture and review commands.

use crate::paths::RapportPaths;
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::fmt;
use std::io::Write;

pub struct CommandContext<'context, F, O, E>
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    pub repo_root: Utf8PathBuf,
    pub cwd: Utf8PathBuf,
    pub paths: RapportPaths,
    pub fs: &'context mut F,
    pub out: &'context mut O,
    pub err: &'context mut E,
}

impl<'context, F, O, E> CommandContext<'context, F, O, E>
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    pub fn new(
        cwd: Utf8PathBuf,
        fs: &'context mut F,
        out: &'context mut O,
        err: &'context mut E,
    ) -> Self {
        let repo_root = find_repo_root(fs, &cwd);
        let paths = RapportPaths::new(repo_root.clone());
        Self {
            repo_root,
            cwd,
            paths,
            fs,
            out,
            err,
        }
    }
}

impl<F, O, E> fmt::Debug for CommandContext<'_, F, O, E>
where
    F: FileSystem,
    O: Write,
    E: Write,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandContext")
            .field("repo_root", &self.repo_root)
            .field("cwd", &self.cwd)
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

#[must_use]
pub fn find_repo_root(fs: &impl FileSystem, cwd: &Utf8Path) -> Utf8PathBuf {
    let mut current = cwd.to_path_buf();
    loop {
        let git_path = current.join(".git");
        if fs.is_dir(&git_path) || fs.is_file(&git_path) {
            return current;
        }
        if !current.pop() {
            return cwd.to_path_buf();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;

    #[test]
    fn find_repo_root_uses_nearest_git_directory() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/repo/.git");
        fs.add_directory("/repo/crates/rapport");

        assert_eq!(
            find_repo_root(&fs, Utf8Path::new("/repo/crates/rapport")),
            Utf8PathBuf::from("/repo")
        );
    }

    #[test]
    fn find_repo_root_falls_back_to_cwd_without_git_marker() {
        let fs = InMemoryFileSystem::default();

        assert_eq!(
            find_repo_root(&fs, Utf8Path::new("/repo/crates/rapport")),
            Utf8PathBuf::from("/repo/crates/rapport")
        );
    }
}

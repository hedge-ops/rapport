//! Shared command execution context.
//!
//! This module owns repository discovery, injected boundary collaborators, and
//! the clock contract used by command workflows.

use crate::paths::RapportPaths;
use crate::runner::CommandRunner;
use chrono::{SecondsFormat, Utc};
use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::fmt;
use std::io::Write;

pub trait Clock {
    fn now_rfc3339(&self) -> String;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_rfc3339(&self) -> String {
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
    }
}

pub struct CommandContext<'context, F, C, O, E>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    pub repo_root: Utf8PathBuf,
    pub cwd: Utf8PathBuf,
    pub paths: RapportPaths,
    pub fs: &'context mut F,
    pub clock: &'context C,
    pub runner: &'context dyn CommandRunner,
    pub out: &'context mut O,
    pub err: &'context mut E,
}

impl<'context, F, C, O, E> CommandContext<'context, F, C, O, E>
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    pub fn new(
        cwd: Utf8PathBuf,
        fs: &'context mut F,
        clock: &'context C,
        runner: &'context dyn CommandRunner,
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
            clock,
            runner,
            out,
            err,
        }
    }
}

impl<F, C, O, E> fmt::Debug for CommandContext<'_, F, C, O, E>
where
    F: FileSystem,
    C: Clock,
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

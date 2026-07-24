//! Repository file discovery helpers.
//!
//! This module owns Git-aware named-file discovery for repository policy scans.

use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use rapport_git::Git;
use std::io;
use std::time::Instant;

const SKIPPED_DIRECTORIES: &[&str] = &[".git", ".rapport", "target"];

pub(crate) fn find_named_files(
    fs: &impl FileSystem,
    root: &Utf8Path,
    file_name: &str,
) -> io::Result<Vec<Utf8PathBuf>> {
    if root.is_dir() {
        return find_git_named_files(fs, root, file_name);
    }

    // In-memory filesystem fixtures do not have a host worktree for Git to
    // inspect. Keep their focused traversal model so policy tests remain
    // independent of a process boundary; production repositories always use
    // Git-aware discovery above.
    let mut files = Vec::new();
    collect_named_files(fs, root, file_name, &mut files)?;
    files.sort();
    Ok(files)
}

fn find_git_named_files(
    fs: &impl FileSystem,
    root: &Utf8Path,
    file_name: &str,
) -> io::Result<Vec<Utf8PathBuf>> {
    eprintln!("rapport: discovering {file_name} from Git-tracked and non-ignored files");
    let started = Instant::now();
    let git = Git::default();
    let repository = git
        .discover(root)
        .map_err(|error| io::Error::other(format!("discover repository files: {error}")))?;
    let files = git
        .working_tree_files(&repository)
        .map_err(|error| io::Error::other(format!("list repository files: {error}")))?
        .into_iter()
        .filter(|path| path.file_name() == Some(file_name))
        .map(|path| root.join(path))
        .filter(|path| fs.is_file(path))
        .collect::<Vec<_>>();
    eprintln!(
        "rapport: discovered {} {file_name} file(s) in {} ms",
        files.len(),
        started.elapsed().as_millis()
    );
    Ok(files)
}

fn collect_named_files(
    fs: &impl FileSystem,
    directory: &Utf8Path,
    file_name: &str,
    files: &mut Vec<Utf8PathBuf>,
) -> io::Result<()> {
    for entry in fs.read_dir(directory)? {
        if fs.is_dir(&entry) {
            if should_skip_directory(&entry) {
                continue;
            }
            collect_named_files(fs, &entry, file_name, files)?;
        } else if entry.file_name() == Some(file_name) {
            files.push(entry);
        }
    }
    Ok(())
}

fn should_skip_directory(path: &Utf8Path) -> bool {
    path.file_name()
        .is_some_and(|name| SKIPPED_DIRECTORIES.contains(&name))
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "repository traversal tests unwrap paths created by their in-memory fixtures"
)]
mod tests {
    use super::*;
    use rapport_files::{InMemoryFileSystem, RealFileSystem};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

    struct TemporaryRepository {
        root: Utf8PathBuf,
    }

    impl TemporaryRepository {
        fn new() -> Self {
            let sequence = NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "rapport-repository-files-test-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&directory).unwrap();
            let root = Utf8PathBuf::from_path_buf(directory).unwrap();
            let repository = Self { root };
            repository.git(&["init", "-q", "-b", "main"]);
            repository.git(&["config", "user.name", "Rapport Test"]);
            repository.git(&["config", "user.email", "rapport@example.invalid"]);
            repository
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.root.join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }

        fn git(&self, args: &[&str]) {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "expecting Git command to succeed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    impl Drop for TemporaryRepository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn find_named_files_discovers_matching_files_recursively() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_directory("/repo/.git");
        fs.add_file("/repo/context.toml");
        fs.add_file("/repo/app/context.toml");
        fs.add_file("/repo/app/rules.toml");

        let files = find_named_files(&fs, Utf8Path::new("/repo"), "context.toml").unwrap();

        assert_eq!(
            files,
            vec![
                Utf8PathBuf::from("/repo/app/context.toml"),
                Utf8PathBuf::from("/repo/context.toml"),
            ]
        );
    }

    #[test]
    fn find_named_files_skips_local_work_and_build_directories() {
        let mut fs = InMemoryFileSystem::default();
        fs.add_file("/repo/context.toml");
        fs.add_file("/repo/.rapport/history/context.toml");
        fs.add_file("/repo/target/debug/context.toml");

        let files = find_named_files(&fs, Utf8Path::new("/repo"), "context.toml").unwrap();

        assert_eq!(files, vec![Utf8PathBuf::from("/repo/context.toml")]);
    }

    #[test]
    /// When a working tree contains a large ignored cache, policy discovery uses Git's file boundary.
    fn find_named_files_should_skip_ignored_paths_and_keep_tracked_or_untracked_files() {
        const IGNORED_CONTEXT_COUNT: u16 = 256;

        let repository = TemporaryRepository::new();
        repository.write(".gitignore", "build/\ntracked-output/\n");
        repository.write("context.toml", "root");
        repository.write("nested/context.toml", "nested");
        repository.write("tracked-output/context.toml", "tracked despite ignore");
        repository.git(&["add", ".gitignore", "context.toml", "nested/context.toml"]);
        repository.git(&["add", "-f", "tracked-output/context.toml"]);
        repository.git(&["commit", "-q", "-m", "add policy files"]);
        repository.write("local/context.toml", "untracked and discoverable");
        repository.write(".git/info/exclude", "excluded/\n");
        repository.write("excluded/context.toml", "ignored by info exclude");
        for index in 0..IGNORED_CONTEXT_COUNT {
            repository.write(
                &format!("build/cache-{index}/context.toml"),
                "ignored cache metadata",
            );
        }

        let files =
            find_named_files(&RealFileSystem, repository.root.as_path(), "context.toml").unwrap();

        assert_eq!(
            files,
            vec![
                repository.root.join("context.toml"),
                repository.root.join("local/context.toml"),
                repository.root.join("nested/context.toml"),
                repository.root.join("tracked-output/context.toml"),
            ]
        );
    }

    #[test]
    /// When ignored build metadata changes, the effective policy digest remains bound to Git-visible policy files.
    fn policy_digest_should_ignore_context_files_beneath_an_ignored_directory() {
        const ROOT_CONTEXT: &str = r#"version = 1
id = "ROOT"
purpose = "Repository policy."

[ruleset]
includes = []
"#;

        let repository = TemporaryRepository::new();
        repository.write(".gitignore", "build/\n");
        repository.write("context.toml", ROOT_CONTEXT);
        repository.write("src/lib.rs", "pub fn source() {}\n");
        repository.git(&["add", ".gitignore", "context.toml", "src/lib.rs"]);
        repository.git(&["commit", "-q", "-m", "add repository policy"]);
        let mut fs = RealFileSystem;
        let before = crate::policy_context::effective_policy_digest_for_paths(
            &mut fs,
            &repository.root,
            [Utf8Path::new("src/lib.rs")],
        )
        .unwrap();

        repository.write("build/cache/context.toml", ROOT_CONTEXT);
        repository.write("build/cache/metadata.txt", "updated build metadata");

        let after = crate::policy_context::effective_policy_digest_for_paths(
            &mut fs,
            &repository.root,
            [Utf8Path::new("src/lib.rs")],
        )
        .unwrap();

        assert_eq!(after, before);
    }
}

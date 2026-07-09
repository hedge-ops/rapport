use rapport_files::{FileSystem, Utf8Path, Utf8PathBuf};
use std::io;

const SKIPPED_DIRECTORIES: &[&str] = &[".git", ".rapport", "target"];

pub(crate) fn find_named_files(
    fs: &impl FileSystem,
    root: &Utf8Path,
    file_name: &str,
) -> io::Result<Vec<Utf8PathBuf>> {
    let mut files = Vec::new();
    collect_named_files(fs, root, file_name, &mut files)?;
    files.sort();
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rapport_files::InMemoryFileSystem;

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
}

use camino::{Utf8Path, Utf8PathBuf};
use rapport_cli::files::FileSystem;

pub(crate) trait ProjectMatcher {
    fn matches_project(&self, dir: &Utf8Path, fs: &impl FileSystem) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct CargoProjectMatcher;

impl ProjectMatcher for CargoProjectMatcher {
    fn matches_project(&self, dir: &Utf8Path, fs: &impl FileSystem) -> bool {
        fs.is_file(dir.join("Cargo.toml"))
    }
}

pub(crate) fn discover_project(
    start: &Utf8Path,
    matcher: &impl ProjectMatcher,
    fs: &impl FileSystem,
) -> Result<Utf8PathBuf, String> {
    let ancestors = ancestors_to_git_root(start, fs)?;
    let Some(git_root) = ancestors.last() else {
        return Err("could not inspect directory ancestry".into());
    };

    ancestors
        .iter()
        .find(|dir| matcher.matches_project(dir, fs))
        .cloned()
        .ok_or_else(|| format!("has no supported project between it and git root {git_root}"))
}

fn ancestors_to_git_root(
    start: &Utf8Path,
    fs: &impl FileSystem,
) -> Result<Vec<Utf8PathBuf>, String> {
    let mut dir = absolute_path(start)?;
    let mut ancestors = Vec::new();

    loop {
        ancestors.push(dir.clone());
        if has_git_marker(&dir, fs) {
            return Ok(ancestors);
        }
        if !dir.pop() {
            return Err("is not inside a git repository".into());
        }
    }
}

fn absolute_path(path: &Utf8Path) -> Result<Utf8PathBuf, String> {
    let absolute =
        std::path::absolute(path).map_err(|err| format!("could not resolve directory: {err}"))?;
    Utf8PathBuf::from_path_buf(absolute)
        .map_err(|path| format!("could not resolve directory as UTF-8: {}", path.display()))
}

fn has_git_marker(dir: &Utf8Path, fs: &impl FileSystem) -> bool {
    fs.exists(dir.join(".git"))
}

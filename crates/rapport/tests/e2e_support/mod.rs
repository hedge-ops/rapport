use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

pub(crate) type TestResult<T = ()> = Result<T, TestError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum TestError {
    #[error(transparent)]
    Cargo(#[from] assert_cmd::cargo::CargoError),
    #[error(transparent)]
    FromUtf8(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    JoinPaths(#[from] std::env::JoinPathsError),
}

#[derive(Debug)]
pub(crate) struct FixtureProject {
    pub(crate) root: TempDir,
    pub(crate) project: PathBuf,
}

impl FixtureProject {
    pub(crate) fn copy(convention: &str, relative: &str) -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(convention)
            .join(relative);
        let project = root.path().join("project");
        copy_dir(&source, &project)?;
        fs::write(project.join(".git"), "gitdir: test")?;

        Ok(Self { root, project })
    }
}

#[derive(Debug)]
pub(crate) struct RunResult {
    exit: String,
    stdout: String,
    stderr: String,
}

impl RunResult {
    pub(crate) fn snapshot(&self) -> String {
        format!(
            "exit: {}\nstdout:\n---\n{}stderr:\n---\n{}",
            self.exit,
            normalize_trailing_newline(&self.stdout),
            normalize_trailing_newline(&self.stderr),
        )
    }

    fn from_output(output: &Output) -> TestResult<Self> {
        Ok(Self {
            exit: output
                .status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
            stdout: String::from_utf8(output.stdout.clone())?,
            stderr: String::from_utf8(output.stderr.clone())?,
        })
    }
}

pub(crate) fn run_rapport(
    path: &Path,
    verb: &str,
    expected_exit: i32,
    configure: impl FnOnce(&mut Command),
) -> TestResult<RunResult> {
    let mut command = Command::cargo_bin("rapport")?;
    command.arg(verb).arg(path);
    configure(&mut command);

    let assertion = command.assert().code(expected_exit);
    RunResult::from_output(assertion.get_output())
}

pub(crate) fn snapshot_settings(
    project: &FixtureProject,
    extra_paths: &[(&Path, &'static str)],
    before_path_filters: &[(&str, &str)],
    after_path_filters: &[(&str, &str)],
) -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.add_filter(r"duration: [0-9]+(\.[0-9]+)?s", "duration: [duration]");

    for &(pattern, replacement) in before_path_filters {
        settings.add_filter(pattern, replacement);
    }

    let workspace_root = workspace_root();
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let base_paths = [
        (project.project.as_path(), "[project]"),
        (project.root.path(), "[tmp]"),
        (workspace_root.as_path(), "[repo]"),
        (crate_root, "[crate]"),
    ];
    let paths = extra_paths.iter().copied().chain(base_paths);
    for (path, replacement) in snapshot_paths(paths) {
        settings.add_filter(&regex_escape(&path.to_string_lossy()), replacement);
    }

    for &(pattern, replacement) in after_path_filters {
        settings.add_filter(pattern, replacement);
    }

    settings
}

fn copy_dir(source: &Path, destination: &Path) -> TestResult {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let destination_path = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &destination_path)?;
        } else {
            fs::copy(entry.path(), destination_path)?;
        }
    }
    Ok(())
}

fn normalize_trailing_newline(s: &str) -> String {
    if s.is_empty() {
        "(empty)\n".to_owned()
    } else if s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}

fn snapshot_paths<'a>(
    paths: impl IntoIterator<Item = (&'a Path, &'static str)>,
) -> Vec<(PathBuf, &'static str)> {
    let mut paths: Vec<_> = paths
        .into_iter()
        .flat_map(|(path, replacement)| {
            let canonical = fs::canonicalize(path).ok();
            std::iter::once((path.to_path_buf(), replacement)).chain(
                canonical
                    .filter(|canonical_path| canonical_path != path)
                    .map(|canonical_path| (canonical_path, replacement)),
            )
        })
        .collect();

    paths.sort_by(|(left, _), (right, _)| {
        right
            .to_string_lossy()
            .len()
            .cmp(&left.to_string_lossy().len())
    });
    paths
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            Path::to_path_buf,
        )
}

fn regex_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
            | '#' | '&' | '-' | '~' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

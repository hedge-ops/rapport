use assert_cmd::Command;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Debug)]
struct FixtureProject {
    root: TempDir,
    project: PathBuf,
    target: PathBuf,
    cargo_home: PathBuf,
}

#[derive(Debug)]
struct RunResult {
    exit: String,
    stdout: String,
    stderr: String,
}

impl RunResult {
    fn snapshot(&self) -> String {
        format!(
            "exit: {}\nstdout:\n---\n{}stderr:\n---\n{}",
            self.exit,
            normalize_trailing_newline(&self.stdout),
            normalize_trailing_newline(&self.stderr),
        )
    }
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

fn fixture(relative: &str) -> Result<FixtureProject, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cargo")
        .join(relative);
    let project = root.path().join("project");
    copy_dir(&source, &project)?;

    let target = root.path().join("target");
    let cargo_home = root.path().join("cargo-home");
    fs::create_dir_all(&target)?;
    fs::create_dir_all(&cargo_home)?;

    Ok(FixtureProject {
        root,
        project,
        target,
        cargo_home,
    })
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
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

fn run_rapport(
    project: &FixtureProject,
    verb: &str,
    expected_exit: i32,
) -> Result<RunResult, Box<dyn Error>> {
    run_rapport_with_path(project, verb, expected_exit, None::<&OsStr>)
}

fn run_rapport_with_path<P>(
    project: &FixtureProject,
    verb: &str,
    expected_exit: i32,
    path: Option<P>,
) -> Result<RunResult, Box<dyn Error>>
where
    P: AsRef<OsStr>,
{
    let mut command = Command::cargo_bin("rapport")?;
    command
        .arg(verb)
        .arg(&project.project)
        .env("CARGO_TARGET_DIR", &project.target)
        .env("CARGO_HOME", &project.cargo_home)
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS");

    if let Some(path) = path {
        command.env("PATH", path);
    }

    let _guard = cargo_command_lock()
        .lock()
        .map_err(|_| std::io::Error::other("cargo fixture command lock poisoned"))?;
    let assertion = command.assert().code(expected_exit);
    let output = assertion.get_output();
    Ok(RunResult {
        exit: output
            .status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        stdout: String::from_utf8(output.stdout.clone())?,
        stderr: String::from_utf8(output.stderr.clone())?,
    })
}

fn cargo_command_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn assert_snapshot(name: &str, project: &FixtureProject, result: &RunResult) {
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.add_filter(r"duration: [0-9]+(\.[0-9]+)?s", "duration: [duration]");
    settings.add_filter(r"in [0-9]+(\.[0-9]+)?s", "in [duration]");
    settings.add_filter(r"rust-[0-9]+\.[0-9]+\.[0-9]+", "rust-[version]");
    settings.add_filter(
        r"thread '([^']+)' \([0-9]+\) panicked",
        "thread '$1' panicked",
    );
    settings.add_filter(
        r"could not compile `([^`]+)` \((lib|lib test)\)",
        "could not compile `$1` ([cargo-target])",
    );
    settings.add_filter(
        r"/[^[:space:]`']*\.rustup/toolchains/[^[:space:]`']+",
        "[toolchain]",
    );

    for (path, replacement) in snapshot_paths(project) {
        settings.add_filter(&regex_escape(&path.to_string_lossy()), replacement);
    }
    settings.add_filter(
        r"\[target\]/debug/deps/([A-Za-z0-9_-]+)-[0-9a-f]+",
        "[target]/debug/deps/$1-[hash]",
    );

    settings.bind(|| {
        insta::assert_snapshot!(name, result.snapshot());
    });
}

fn snapshot_paths(project: &FixtureProject) -> Vec<(PathBuf, &'static str)> {
    let mut paths: Vec<_> = [
        (project.target.as_path(), "[target]"),
        (project.cargo_home.as_path(), "[cargo-home]"),
        (project.project.as_path(), "[project]"),
        (project.root.path(), "[tmp]"),
        (workspace_root().as_path(), "[repo]"),
        (Path::new(env!("CARGO_MANIFEST_DIR")), "[crate]"),
    ]
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

fn assert_fixture_snapshot(
    snapshot_name: &str,
    fixture_path: &str,
    verb: &str,
    expected_exit: i32,
) -> TestResult {
    let project = fixture(fixture_path)?;
    let result = run_rapport(&project, verb, expected_exit)?;
    assert_snapshot(snapshot_name, &project, &result);
    Ok(())
}

#[test]
fn basic_crate_fix() -> TestResult {
    assert_fixture_snapshot("cargo_ok_basic_crate_fix", "ok/basic-crate", "fix", 0)
}

#[test]
fn basic_crate_lint() -> TestResult {
    assert_fixture_snapshot("cargo_ok_basic_crate_lint", "ok/basic-crate", "lint", 0)
}

#[test]
fn basic_crate_build() -> TestResult {
    assert_fixture_snapshot("cargo_ok_basic_crate_build", "ok/basic-crate", "build", 0)
}

#[test]
fn basic_crate_test() -> TestResult {
    assert_fixture_snapshot("cargo_ok_basic_crate_test", "ok/basic-crate", "test", 0)
}

#[test]
fn basic_crate_validate() -> TestResult {
    assert_fixture_snapshot(
        "cargo_ok_basic_crate_validate",
        "ok/basic-crate",
        "validate",
        0,
    )
}

#[test]
fn basic_crate_audit() -> TestResult {
    assert_fixture_snapshot("cargo_ok_basic_crate_audit", "ok/basic-crate", "audit", 0)
}

#[test]
fn workspace_build() -> TestResult {
    assert_fixture_snapshot("cargo_ok_workspace_build", "ok/workspace", "build", 0)
}

#[test]
fn workspace_test() -> TestResult {
    assert_fixture_snapshot("cargo_ok_workspace_test", "ok/workspace", "test", 0)
}

#[test]
fn workspace_validate() -> TestResult {
    assert_fixture_snapshot("cargo_ok_workspace_validate", "ok/workspace", "validate", 0)
}

#[test]
fn missing_project_build_failure() -> TestResult {
    assert_fixture_snapshot(
        "cargo_fail_missing_project_build",
        "fail/missing-project",
        "build",
        1,
    )
}

#[test]
fn fmt_needed_lint_failure() -> TestResult {
    assert_fixture_snapshot("cargo_fail_fmt_needed_lint", "fail/fmt-needed", "lint", 1)
}

#[test]
fn fmt_needed_fix_success() -> TestResult {
    let project = fixture("fail/fmt-needed")?;
    let result = run_rapport(&project, "fix", 0)?;
    let source = fs::read_to_string(project.project.join("src/lib.rs"))?;

    assert!(source.contains("pub fn answer() -> u8 {\n    42\n}\n"));
    assert_snapshot("cargo_fail_fmt_needed_fix", &project, &result);
    Ok(())
}

#[test]
fn clippy_warning_lint_failure() -> TestResult {
    assert_fixture_snapshot(
        "cargo_fail_clippy_warning_lint",
        "fail/clippy-warning",
        "lint",
        1,
    )
}

#[test]
fn compile_error_build_failure() -> TestResult {
    assert_fixture_snapshot(
        "cargo_fail_compile_error_build",
        "fail/compile-error",
        "build",
        1,
    )
}

#[test]
fn compile_error_validate_failure() -> TestResult {
    assert_fixture_snapshot(
        "cargo_fail_compile_error_validate",
        "fail/compile-error",
        "validate",
        1,
    )
}

#[test]
fn compile_error_audit_failure() -> TestResult {
    assert_fixture_snapshot(
        "cargo_fail_compile_error_audit",
        "fail/compile-error",
        "audit",
        1,
    )
}

#[test]
fn failing_test_failure() -> TestResult {
    assert_fixture_snapshot(
        "cargo_fail_failing_test_test",
        "fail/failing-test",
        "test",
        1,
    )
}

#[test]
fn failing_test_validate_failure() -> TestResult {
    assert_fixture_snapshot(
        "cargo_fail_failing_test_validate",
        "fail/failing-test",
        "validate",
        1,
    )
}

#[test]
fn doc_error_audit_failure() -> TestResult {
    assert_fixture_snapshot("cargo_fail_doc_error_audit", "fail/doc-error", "audit", 1)
}

#[test]
fn composite_stops_at_first_failing_fmt_step() -> TestResult {
    assert_fixture_snapshot(
        "cargo_fail_fmt_needed_validate",
        "fail/fmt-needed",
        "validate",
        1,
    )
}

#[test]
fn missing_cargo_on_path_reports_invoke_hint() -> TestResult {
    let project = fixture("ok/basic-crate")?;
    let empty_path = tempfile::tempdir()?;
    let result = run_rapport_with_path(&project, "build", 2, Some(empty_path.path().as_os_str()))?;

    assert_snapshot("cargo_missing_cargo_on_path", &project, &result);
    Ok(())
}

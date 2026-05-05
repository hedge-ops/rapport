#![cfg(unix)]

mod e2e_support;

use e2e_support::{FixtureProject, RunResult, TestResult};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::TempDir;

#[derive(Debug)]
struct Toolchain {
    _root: TempDir,
    path: OsString,
}

#[derive(Debug, Clone, Copy)]
enum ToolchainMode {
    DriverFormatter,
    DirectFormatter,
    MissingFormatter,
    MissingSwift,
}

fn fixture(relative: &str) -> TestResult<FixtureProject> {
    FixtureProject::copy("swift", relative)
}

fn toolchain(mode: ToolchainMode) -> TestResult<Toolchain> {
    let root = tempfile::tempdir()?;
    match mode {
        ToolchainMode::DriverFormatter => {
            write_executable(&root.path().join("swift"), &swift_script(true))?;
        }
        ToolchainMode::DirectFormatter => {
            write_executable(&root.path().join("swift"), &swift_script(false))?;
            write_executable(&root.path().join("swift-format"), swift_format_script())?;
        }
        ToolchainMode::MissingFormatter => {
            write_executable(&root.path().join("swift"), &swift_script(false))?;
        }
        ToolchainMode::MissingSwift => {}
    }
    let path = std::env::join_paths([root.path()])?;
    Ok(Toolchain { _root: root, path })
}

fn write_executable(path: &Path, contents: &str) -> TestResult {
    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn swift_script(supports_driver_formatter: bool) -> String {
    let driver_format = if supports_driver_formatter {
        r#"
    shift
    format_tool "$@"
"#
    } else {
        r#"
    echo "error: unknown subcommand 'format'" >&2
    exit 64
"#
    };

    format!(
        r#"#!/bin/sh
set -u

has_format_issue() {{
    for candidate in Package.swift Sources Tests Plugins; do
        if [ -e "$candidate" ] && /usr/bin/grep -R "let answer=42" "$candidate" >/dev/null 2>&1; then
            return 0
        fi
    done
    return 1
}}

fix_format_issue() {{
    file="Sources/RapportFixture/Greeter.swift"
    if [ -f "$file" ]; then
        /usr/bin/awk '{{ gsub("let answer=42", "let answer = 42"); print }}' "$file" > "$file.tmp" && /bin/mv "$file.tmp" "$file"
    fi
}}

format_tool() {{
    subcommand="${{1:-}}"
    if [ "$subcommand" = "--version" ] || [ "$subcommand" = "-v" ]; then
        echo "6.3.0"
        exit 0
    fi
    if [ "$subcommand" = "format" ]; then
        fix_format_issue
        echo "swift-format formatted inputs"
        exit 0
    fi
    if [ "$subcommand" = "lint" ]; then
        if has_format_issue; then
            echo "Sources/RapportFixture/Greeter.swift:1:1: warning: source is not formatted" >&2
            exit 1
        fi
        echo "swift-format lint passed"
        exit 0
    fi
    echo "unexpected swift-format args: $*" >&2
    exit 2
}}

case "${{1:-}}" in
format)
{driver_format}
    ;;
build)
    if [ -e Sources ] && /usr/bin/grep -R "compile error" Sources >/dev/null 2>&1; then
        echo "error: simulated Swift compile failure" >&2
        exit 1
    fi
    if [ "${{2:-}}" = "-c" ] && [ "${{3:-}}" = "release" ]; then
        echo "Swift release build complete"
    else
        echo "Swift build complete"
    fi
    exit 0
    ;;
test)
    if [ -e Tests ] && /usr/bin/grep -R "XCTFail" Tests >/dev/null 2>&1; then
        echo "Test Suite 'RapportFixtureTests' failed" >&2
        exit 1
    fi
    echo "Swift tests passed"
    exit 0
    ;;
*)
    echo "unexpected swift args: $*" >&2
    exit 2
    ;;
esac
"#
    )
}

fn swift_format_script() -> &'static str {
    r#"#!/bin/sh
set -u

has_format_issue() {
    for candidate in Package.swift Sources Tests Plugins; do
        if [ -e "$candidate" ] && /usr/bin/grep -R "let answer=42" "$candidate" >/dev/null 2>&1; then
            return 0
        fi
    done
    return 1
}

fix_format_issue() {
    file="Sources/RapportFixture/Greeter.swift"
    if [ -f "$file" ]; then
        /usr/bin/awk '{ gsub("let answer=42", "let answer = 42"); print }' "$file" > "$file.tmp" && /bin/mv "$file.tmp" "$file"
    fi
}

subcommand="${1:-}"
if [ "$subcommand" = "--version" ] || [ "$subcommand" = "-v" ]; then
    echo "6.3.0"
    exit 0
fi
if [ "$subcommand" = "format" ]; then
    fix_format_issue
    echo "swift-format formatted inputs"
    exit 0
fi
if [ "$subcommand" = "lint" ]; then
    if has_format_issue; then
        echo "Sources/RapportFixture/Greeter.swift:1:1: warning: source is not formatted" >&2
        exit 1
    fi
    echo "swift-format lint passed"
    exit 0
fi
echo "unexpected swift-format args: $*" >&2
exit 2
"#
}

fn run_rapport(
    project: &FixtureProject,
    verb: &str,
    expected_exit: i32,
    tools: &Toolchain,
) -> TestResult<RunResult> {
    run_rapport_at_path(verb, expected_exit, &project.project, tools)
}

fn run_rapport_at_path(
    verb: &str,
    expected_exit: i32,
    path: &Path,
    tools: &Toolchain,
) -> TestResult<RunResult> {
    e2e_support::run_rapport(path, verb, expected_exit, |command| {
        command.env("PATH", &tools.path);
    })
}

fn assert_snapshot(name: &str, project: &FixtureProject, result: &RunResult) {
    let settings = e2e_support::snapshot_settings(project, &[], &[], &[]);

    settings.bind(|| {
        insta::assert_snapshot!(name, result.snapshot());
    });
}

fn assert_fixture_snapshot(
    snapshot_name: &str,
    fixture_path: &str,
    verb: &str,
    expected_exit: i32,
    mode: ToolchainMode,
) -> TestResult {
    let project = fixture(fixture_path)?;
    let tools = toolchain(mode)?;
    let result = run_rapport(&project, verb, expected_exit, &tools)?;
    assert_snapshot(snapshot_name, &project, &result);
    Ok(())
}

#[test]
fn basic_package_fix() -> TestResult {
    assert_fixture_snapshot(
        "swift_ok_basic_package_fix",
        "ok/basic-package",
        "fix",
        0,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn basic_package_lint() -> TestResult {
    assert_fixture_snapshot(
        "swift_ok_basic_package_lint",
        "ok/basic-package",
        "lint",
        0,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn basic_package_build() -> TestResult {
    assert_fixture_snapshot(
        "swift_ok_basic_package_build",
        "ok/basic-package",
        "build",
        0,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn basic_package_test() -> TestResult {
    assert_fixture_snapshot(
        "swift_ok_basic_package_test",
        "ok/basic-package",
        "test",
        0,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn basic_package_validate() -> TestResult {
    assert_fixture_snapshot(
        "swift_ok_basic_package_validate",
        "ok/basic-package",
        "validate",
        0,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn basic_package_audit() -> TestResult {
    assert_fixture_snapshot(
        "swift_ok_basic_package_audit",
        "ok/basic-package",
        "audit",
        0,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn with_format_config_uses_formatter_defaults() -> TestResult {
    assert_fixture_snapshot(
        "swift_ok_with_format_config_lint",
        "ok/with-format-config",
        "lint",
        0,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn no_format_config_uses_formatter_defaults() -> TestResult {
    assert_fixture_snapshot(
        "swift_ok_no_format_config_lint",
        "ok/no-format-config",
        "lint",
        0,
        ToolchainMode::DirectFormatter,
    )
}

#[test]
fn discovery_walks_up_from_child_path() -> TestResult {
    let project = fixture("ok/basic-package")?;
    let tools = toolchain(ToolchainMode::DriverFormatter)?;
    let child = project.project.join("Sources/RapportFixture");
    let result = run_rapport_at_path("build", 0, &child, &tools)?;

    assert_snapshot("swift_ok_basic_package_child_build", &project, &result);
    Ok(())
}

#[test]
fn missing_project_build_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_missing_project_build",
        "fail/missing-project",
        "build",
        2,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn invalid_tools_version_build_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_invalid_tools_version_build",
        "fail/invalid-tools-version",
        "build",
        2,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn missing_swift_tool_build_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_missing_swift_tool_build",
        "fail/missing-swift-tool",
        "build",
        2,
        ToolchainMode::MissingSwift,
    )
}

#[test]
fn missing_formatter_lint_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_missing_formatter_lint",
        "fail/missing-formatter",
        "lint",
        2,
        ToolchainMode::MissingFormatter,
    )
}

#[test]
fn missing_formatter_build_success() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_missing_formatter_build",
        "fail/missing-formatter",
        "build",
        0,
        ToolchainMode::MissingFormatter,
    )
}

#[test]
fn compile_error_build_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_compile_error_build",
        "fail/compile-error",
        "build",
        1,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn compile_error_validate_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_compile_error_validate",
        "fail/compile-error",
        "validate",
        1,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn compile_error_audit_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_compile_error_audit",
        "fail/compile-error",
        "audit",
        1,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn failing_test_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_failing_test_test",
        "fail/failing-test",
        "test",
        1,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn failing_test_validate_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_failing_test_validate",
        "fail/failing-test",
        "validate",
        1,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn failing_test_audit_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_failing_test_audit",
        "fail/failing-test",
        "audit",
        1,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn format_needed_lint_failure() -> TestResult {
    assert_fixture_snapshot(
        "swift_fail_format_needed_lint",
        "fail/format-needed",
        "lint",
        1,
        ToolchainMode::DriverFormatter,
    )
}

#[test]
fn format_needed_fix_success() -> TestResult {
    let project = fixture("fail/format-needed")?;
    let tools = toolchain(ToolchainMode::DriverFormatter)?;
    let result = run_rapport(&project, "fix", 0, &tools)?;
    let source = fs::read_to_string(project.project.join("Sources/RapportFixture/Greeter.swift"))?;

    assert!(source.contains("let answer = 42"));
    assert_snapshot("swift_fail_format_needed_fix", &project, &result);
    Ok(())
}

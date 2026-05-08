#!/usr/bin/env python3
from __future__ import annotations

import argparse
import difflib
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError as exc:  # pragma: no cover - exercised by old Python only
    raise SystemExit("rapport e2e requires Python 3.11+ for tomllib") from exc


REPO_ROOT = Path(__file__).resolve().parents[2]
CRATE_ROOT = REPO_ROOT / "crates" / "rapport"
FIXTURE_ROOT = CRATE_ROOT / "tests" / "fixtures"
CASE_ROOT = Path(__file__).resolve().parent / "cases"
SNAPSHOT_ROOT = Path(__file__).resolve().parent / "snapshots"

UNSET_CARGO_ENV = (
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_TARGET",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "RUSTFLAGS",
    "RUSTDOCFLAGS",
)


@dataclass(frozen=True)
class ContainsAssertion:
    path: str
    text: str
    root: str = "project"


@dataclass(frozen=True)
class AbsentAssertion:
    path: str
    root: str = "project"


@dataclass(frozen=True)
class Case:
    name: str
    convention: str
    verb: str
    expected_exit: int
    snapshot: str
    fixture: str | None = None
    source: str | None = None
    path: str = "."
    toolchain: str | None = None
    path_env: str | None = None
    assert_contains: tuple[ContainsAssertion, ...] = ()
    assert_absent: tuple[AbsentAssertion, ...] = ()

    @property
    def source_path(self) -> Path:
        if self.source is not None:
            return REPO_ROOT / self.source
        if self.fixture is None:
            raise ValueError(f"{self.name} must set fixture or source")
        return FIXTURE_ROOT / self.convention / self.fixture

    @property
    def snapshot_path(self) -> Path:
        return SNAPSHOT_ROOT / self.snapshot


@dataclass(frozen=True)
class RunContext:
    temp_root: Path
    project: Path
    target: Path | None = None
    cargo_home: Path | None = None
    toolchain: Path | None = None


def main() -> int:
    parser = argparse.ArgumentParser(description="Run rapport CLI e2e cases")
    parser.add_argument(
        "--convention",
        choices=("cargo", "bun", "swift", "fastlane", "gradle", "kustomize", "terraform", "zola"),
        help="run only cases for one project convention",
    )
    parser.add_argument(
        "--case",
        dest="case_names",
        action="append",
        default=[],
        help="run only a named case; may be passed more than once",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite expected snapshots with current normalized output",
    )
    args = parser.parse_args()

    selected = set(args.case_names)
    cases = [
        case
        for case in load_cases()
        if (args.convention is None or case.convention == args.convention)
        and (not selected or case.name in selected)
    ]
    if not cases:
        print("no e2e cases matched", file=sys.stderr)
        return 2

    rapport_bin = resolve_rapport_bin()
    failures: list[str] = []

    print("\n=== e2e ===", flush=True)
    suite_started = time.perf_counter()
    for case in cases:
        case_started = time.perf_counter()
        failure = run_case(case, rapport_bin, update=args.update)
        case_duration = format_duration(time.perf_counter() - case_started)
        if failure is None:
            print(f"ok   {case.name} ({case_duration})")
        else:
            print(f"FAIL {case.name} ({case_duration})")
            failures.append(failure)
    suite_duration = format_duration(time.perf_counter() - suite_started)

    print("\n=== summary ===")
    print(f"{len(cases)} case(s), {len(failures)} failure(s), {suite_duration} total")
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    return 0


def format_duration(seconds: float) -> str:
    return f"{seconds:.2f}s"


def load_cases() -> list[Case]:
    cases: list[Case] = []
    for path in sorted(CASE_ROOT.rglob("*.toml")):
        with path.open("rb") as f:
            data = tomllib.load(f)
        for raw in data.get("case", []):
            case = parse_case(raw)
            if case.source_path.exists():
                cases.append(case)
            else:
                raise SystemExit(f"{path}: fixture for {case.name} does not exist: {case.source_path}")

    names = [case.name for case in cases]
    duplicates = sorted({name for name in names if names.count(name) > 1})
    if duplicates:
        raise SystemExit(f"duplicate e2e case name(s): {', '.join(duplicates)}")
    return cases


def parse_case(raw: dict[str, Any]) -> Case:
    contains = tuple(
        ContainsAssertion(
            path=item["path"],
            text=item["text"],
            root=item.get("root", "project"),
        )
        for item in raw.get("assert_contains", [])
    )
    absent = tuple(
        AbsentAssertion(
            path=item["path"],
            root=item.get("root", "project"),
        )
        for item in raw.get("assert_absent", [])
    )
    return Case(
        name=raw["name"],
        convention=raw["convention"],
        fixture=raw.get("fixture"),
        source=raw.get("source"),
        path=raw.get("path", "."),
        verb=raw["verb"],
        expected_exit=int(raw["expected_exit"]),
        snapshot=raw["snapshot"],
        toolchain=raw.get("toolchain"),
        path_env=raw.get("path_env"),
        assert_contains=contains,
        assert_absent=absent,
    )


def resolve_rapport_bin() -> Path:
    override = os.environ.get("RAPPORT_BIN")
    if override:
        path = Path(override)
        if not path.is_absolute():
            path = REPO_ROOT / path
        if not os.access(path, os.X_OK):
            raise SystemExit(f"RAPPORT_BIN is not executable: {path}")
        return path

    print("=== build rapport ===", flush=True)
    subprocess.run(
        ["cargo", "build", "--manifest-path", str(REPO_ROOT / "Cargo.toml"), "-p", "rapport"],
        cwd=REPO_ROOT,
        check=True,
    )
    path = REPO_ROOT / "target" / "debug" / "rapport"
    if not os.access(path, os.X_OK):
        raise SystemExit(f"rapport binary is not executable: {path}")
    return path


def run_case(case: Case, rapport_bin: Path, *, update: bool) -> str | None:
    with tempfile.TemporaryDirectory(prefix=f"rapport-e2e-{safe_name(case.name)}.") as tmp:
        temp_root = Path(tmp)
        project = temp_root / "project"
        shutil.copytree(case.source_path, project, ignore=fixture_ignore)
        (project / ".git").write_text("gitdir: test\n")

        env = os.environ.copy()
        context = RunContext(temp_root=temp_root, project=project)
        if case.convention == "cargo":
            env, context = configure_cargo(env, context, case)
        elif case.convention == "bun":
            env, context = configure_bun(env, context, case)
        elif case.convention == "swift":
            env, context = configure_swift(env, context, case)
        elif case.convention == "fastlane":
            env, context = configure_fastlane(env, context, case)
        elif case.convention == "gradle":
            env, context = configure_gradle(env, context, case)
        elif case.convention == "kustomize":
            env, context = configure_kustomize(env, context, case)
        elif case.convention == "terraform":
            env, context = configure_terraform(env, context, case)
        elif case.convention == "zola":
            env, context = configure_zola(env, context, case)
        else:
            return f"\n--- {case.name} configuration error ---\nunsupported convention: {case.convention}"

        argument = project if case.path == "." else project / case.path
        result = subprocess.run(
            [str(rapport_bin), case.verb, str(argument)],
            cwd=REPO_ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        exit_failure = None
        if result.returncode != case.expected_exit:
            exit_failure = (
                f"expected exit {case.expected_exit}, got {result.returncode}\n"
                f"command: {rapport_bin} {case.verb} {argument}"
            )

        actual = normalize_snapshot(render_snapshot(result), context)
        assertion_failure = assert_case_state(case, context)
        snapshot_failure = None
        if exit_failure is None and assertion_failure is None:
            snapshot_failure = compare_or_update_snapshot(case, actual, update=update)

        failures = [f for f in (exit_failure, assertion_failure, snapshot_failure) if f]
        if failures:
            return f"\n--- {case.name} failure ---\n" + "\n".join(failures)
        return None


def fixture_ignore(_dir: str, names: list[str]) -> set[str]:
    ignored: set[str] = set()
    for name in names:
        if name == ".git":
            ignored.add(name)
        elif name in {"target", ".build", ".swiftpm"} and not (
            Path(_dir) / name / ".rapport-keep"
        ).exists():
            ignored.add(name)
    return ignored


def configure_cargo(env: dict[str, str], context: RunContext, case: Case) -> tuple[dict[str, str], RunContext]:
    for key in UNSET_CARGO_ENV:
        env.pop(key, None)

    target = context.temp_root / "target"
    cargo_home = context.temp_root / "cargo-home"
    target.mkdir()
    cargo_home.mkdir()
    env.update(
        {
            "CARGO_TARGET_DIR": str(target),
            "CARGO_HOME": str(cargo_home),
            "CARGO_BUILD_JOBS": "1",
            "CARGO_TERM_COLOR": "never",
            "CARGO_INCREMENTAL": "0",
        }
    )
    mode = case.toolchain or "host"
    if mode == "fake":
        tool_root = context.temp_root / "toolchain"
        tool_root.mkdir()
        write_executable(tool_root / "cargo", cargo_script())
        env["PATH"] = str(tool_root)
    elif mode != "host":
        raise ValueError(f"unsupported cargo toolchain mode: {mode}")

    if case.path_env == "empty":
        empty_path = context.temp_root / "empty-path"
        empty_path.mkdir()
        env["PATH"] = str(empty_path)

    return env, RunContext(
        temp_root=context.temp_root,
        project=context.project,
        target=target,
        cargo_home=cargo_home,
    )


def configure_bun(env: dict[str, str], context: RunContext, case: Case) -> tuple[dict[str, str], RunContext]:
    mode = case.toolchain or "full"
    tool_root = context.temp_root / "toolchain"
    tool_root.mkdir()

    if mode == "full":
        write_executable(tool_root / "bun", bun_script())
    elif mode == "missing_bun":
        pass
    else:
        raise ValueError(f"unsupported bun toolchain mode: {mode}")

    env["PATH"] = str(tool_root)
    return env, RunContext(
        temp_root=context.temp_root,
        project=context.project,
        toolchain=tool_root,
    )


def configure_swift(env: dict[str, str], context: RunContext, case: Case) -> tuple[dict[str, str], RunContext]:
    mode = case.toolchain or "driver_formatter"
    tool_root = context.temp_root / "toolchain"
    tool_root.mkdir()

    if mode == "driver_formatter":
        write_executable(tool_root / "swift", swift_script(supports_driver_formatter=True))
    elif mode == "direct_formatter":
        write_executable(tool_root / "swift", swift_script(supports_driver_formatter=False))
        write_executable(tool_root / "swift-format", swift_format_script())
    elif mode == "missing_formatter":
        write_executable(tool_root / "swift", swift_script(supports_driver_formatter=False))
    elif mode == "missing_swift":
        pass
    else:
        raise ValueError(f"unsupported swift toolchain mode: {mode}")

    env["PATH"] = str(tool_root)
    return env, RunContext(
        temp_root=context.temp_root,
        project=context.project,
        toolchain=tool_root,
    )


def configure_fastlane(env: dict[str, str], context: RunContext, case: Case) -> tuple[dict[str, str], RunContext]:
    mode = case.toolchain or "bundle"
    tool_root = context.temp_root / "toolchain"
    tool_root.mkdir()

    if mode == "bundle":
        write_executable(tool_root / "bundle", fastlane_bundle_script())
    elif mode == "missing_bundle":
        pass
    else:
        raise ValueError(f"unsupported fastlane toolchain mode: {mode}")

    env["PATH"] = str(tool_root)
    return env, RunContext(
        temp_root=context.temp_root,
        project=context.project,
        toolchain=tool_root,
    )


def configure_gradle(env: dict[str, str], context: RunContext, case: Case) -> tuple[dict[str, str], RunContext]:
    mode = case.toolchain or "full"
    tool_root = context.temp_root / "toolchain"
    tool_root.mkdir()

    if mode == "full":
        write_executable(tool_root / "java", java_script())
    elif mode == "missing_java":
        pass
    else:
        raise ValueError(f"unsupported gradle toolchain mode: {mode}")

    env["PATH"] = str(tool_root)
    return env, RunContext(
        temp_root=context.temp_root,
        project=context.project,
        toolchain=tool_root,
    )


def configure_kustomize(env: dict[str, str], context: RunContext, case: Case) -> tuple[dict[str, str], RunContext]:
    mode = case.toolchain or "full"
    tool_root = context.temp_root / "toolchain"
    tool_root.mkdir()

    if mode == "full":
        write_executable(tool_root / "kustomize", kustomize_script())
        write_executable(tool_root / "kubectl", kubectl_script())
        write_executable(tool_root / "kubeconform", kubeconform_script())
    elif mode == "kubectl":
        write_executable(tool_root / "kubectl", kubectl_script())
        write_executable(tool_root / "kubeconform", kubeconform_script())
    elif mode == "standalone":
        write_executable(tool_root / "kustomize", kustomize_script())
        write_executable(tool_root / "kubeconform", kubeconform_script())
    elif mode == "missing_renderer":
        write_executable(tool_root / "kubeconform", kubeconform_script())
    elif mode == "missing_validator":
        write_executable(tool_root / "kubectl", kubectl_script())
    else:
        raise ValueError(f"unsupported kustomize toolchain mode: {mode}")

    env["PATH"] = str(tool_root)
    return env, RunContext(
        temp_root=context.temp_root,
        project=context.project,
        toolchain=tool_root,
    )


def configure_terraform(env: dict[str, str], context: RunContext, case: Case) -> tuple[dict[str, str], RunContext]:
    mode = case.toolchain or "full"
    tool_root = context.temp_root / "toolchain"
    tool_root.mkdir()

    if mode == "full":
        write_executable(tool_root / "terraform", terraform_script())
        write_executable(tool_root / "tflint", tflint_script())
    elif mode == "full_with_cargo":
        write_executable(tool_root / "terraform", terraform_script())
        write_executable(tool_root / "tflint", tflint_script())
        write_executable(tool_root / "cargo", cargo_script())
    elif mode == "terraform_only":
        write_executable(tool_root / "terraform", terraform_script())
    elif mode == "missing_terraform":
        write_executable(tool_root / "tflint", tflint_script())
    elif mode == "missing_tflint":
        write_executable(tool_root / "terraform", terraform_script())
    else:
        raise ValueError(f"unsupported terraform toolchain mode: {mode}")

    env["PATH"] = str(tool_root)
    return env, RunContext(
        temp_root=context.temp_root,
        project=context.project,
        toolchain=tool_root,
    )


def configure_zola(env: dict[str, str], context: RunContext, case: Case) -> tuple[dict[str, str], RunContext]:
    mode = case.toolchain or "full"
    tool_root = context.temp_root / "toolchain"
    tool_root.mkdir()

    if mode == "full":
        write_executable(tool_root / "zola", zola_script())
        write_executable(tool_root / "bun", bun_script())
    elif mode == "missing_zola":
        write_executable(tool_root / "bun", bun_script())
    elif mode == "missing_bun":
        write_executable(tool_root / "zola", zola_script())
    else:
        raise ValueError(f"unsupported zola toolchain mode: {mode}")

    env["PATH"] = str(tool_root)
    return env, RunContext(
        temp_root=context.temp_root,
        project=context.project,
        toolchain=tool_root,
    )


def write_executable(path: Path, contents: str) -> None:
    path.write_text(contents)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def cargo_script() -> str:
    return """#!/bin/sh
set -u

case "${1:-}" in
--version)
    echo "cargo 1.89.0"
    exit 0
    ;;
fmt)
    if [ "${2:-}" = "--version" ]; then
        echo "rustfmt 1.8.0"
        exit 0
    fi
    echo "unexpected cargo fmt args: $*" >&2
    exit 2
    ;;
clippy)
    if [ "${2:-}" = "--version" ]; then
        echo "clippy 0.1.89"
        exit 0
    fi
    echo "unexpected cargo clippy args: $*" >&2
    exit 2
    ;;
check)
    echo "cargo check passed"
    exit 0
    ;;
*)
    echo "unexpected cargo args: $*" >&2
    exit 2
    ;;
esac
"""


def swift_script(*, supports_driver_formatter: bool) -> str:
    if supports_driver_formatter:
        format_case = """
    shift
    format_tool "$@"
"""
    else:
        format_case = """
    echo "error: unknown subcommand 'format'" >&2
    exit 64
"""

    return f"""#!/bin/sh
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
{format_case}
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
"""


def swift_format_script() -> str:
    return """#!/bin/sh
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
"""


def fastlane_bundle_script() -> str:
    return """#!/bin/sh
set -u

if [ "${1:-}" != "exec" ] || [ "${2:-}" != "fastlane" ]; then
    echo "unexpected bundle args: $*" >&2
    exit 2
fi

lane="${3:-}"
if [ -z "$lane" ]; then
    echo "missing fastlane lane" >&2
    exit 2
fi

echo "fastlane lane $lane started"

if [ "$lane" = "build" ] && /usr/bin/grep -q "UI.user_error!" fastlane/Fastfile; then
    echo "Error in lane '$lane': simulated Fastlane lane failure" >&2
    echo "fastlane lane $lane failed" >&2
    exit 1
fi

if /usr/bin/grep -q "xcodebuild" fastlane/Fastfile; then
    echo "fastlane lane $lane invoked xcodebuild wrapper"
else
    echo "fastlane lane $lane passed"
fi
"""


def java_script() -> str:
    return """#!/bin/sh
set -u

if [ "${1:-}" = "-version" ]; then
    echo 'openjdk version "21.0.3"' >&2
    exit 0
fi

echo "unexpected java args: $*" >&2
exit 2
"""


def bun_script() -> str:
    return """#!/bin/sh
set -u

if [ "${1:-}" = "--version" ]; then
    echo "1.2.20"
    exit 0
fi

if [ "${1:-}" != "run" ]; then
    echo "unexpected bun args: $*" >&2
    exit 2
fi

script="${2:-}"
if [ -z "$script" ]; then
    echo "missing bun script" >&2
    exit 2
fi

if /usr/bin/grep -q "fail-$script" package.json; then
    echo "src/index.ts:1:1 error: simulated Bun $script failure" >&2
    echo "Build failed with 1 error" >&2
    exit 1
fi

if [ "$script" = "lint" ] && [ -e src ] && /usr/bin/grep -R "lint_error" src >/dev/null 2>&1; then
    echo "src/index.ts:1:1 warning: simulated Bun lint finding" >&2
    exit 1
fi

if [ "$script" = "test" ] && [ -e src ] && /usr/bin/grep -R "test_failure" src >/dev/null 2>&1; then
    echo "index.test.ts:1:1 test failed: simulated Bun test failure" >&2
    exit 1
fi

if [ "$script" = "fix" ] && [ -f src/index.ts ]; then
    /usr/bin/awk '{ gsub("needsFix", "fixed"); print }' src/index.ts > src/index.ts.tmp && /bin/mv src/index.ts.tmp src/index.ts
fi

echo "bun script $script passed"
"""


def kubectl_script() -> str:
    return """#!/bin/sh
set -u

render_kustomize() {
    dir="${1:-.}"
    manifest="$dir/kustomization.yaml"
    if [ ! -f "$manifest" ]; then
        manifest="$dir/kustomization.yml"
    fi
    if [ ! -f "$manifest" ]; then
        echo "error: no kustomization.yaml or kustomization.yml found in $dir" >&2
        exit 1
    fi
    if /usr/bin/grep -q "malformed:" "$manifest"; then
        echo "error: accumulating resources: yaml: line 2: did not find expected node content" >&2
        exit 1
    fi
    if /usr/bin/grep -q "missing.yaml" "$manifest"; then
        echo "Error: accumulating resources: accumulation err='accumulating resources from missing.yaml: no such file or directory'" >&2
        exit 1
    fi
    kind="Deployment"
    if /usr/bin/grep -R "kind: TotallyInvalid" "$dir" >/dev/null 2>&1; then
        kind="TotallyInvalid"
    fi
    echo "---"
    echo "apiVersion: apps/v1"
    echo "kind: $kind"
    echo "metadata:"
    echo "  name: rapport-app"
}

case "${1:-}" in
version)
    if [ "${2:-}" = "--client" ]; then
        echo "Client Version: v1.31.0"
        exit 0
    fi
    echo "unexpected kubectl version args: $*" >&2
    exit 2
    ;;
kustomize)
    render_kustomize "${2:-.}"
    ;;
*)
    echo "unexpected kubectl args: $*" >&2
    exit 2
    ;;
esac
"""


def kustomize_script() -> str:
    return """#!/bin/sh
set -u

render_kustomize() {
    dir="${1:-.}"
    manifest="$dir/kustomization.yaml"
    if [ ! -f "$manifest" ]; then
        manifest="$dir/kustomization.yml"
    fi
    if [ ! -f "$manifest" ]; then
        echo "error: no kustomization.yaml or kustomization.yml found in $dir" >&2
        exit 1
    fi
    if /usr/bin/grep -q "malformed:" "$manifest"; then
        echo "error: accumulating resources: yaml: line 2: did not find expected node content" >&2
        exit 1
    fi
    if /usr/bin/grep -q "missing.yaml" "$manifest"; then
        echo "Error: accumulating resources: accumulation err='accumulating resources from missing.yaml: no such file or directory'" >&2
        exit 1
    fi
    kind="Deployment"
    if /usr/bin/grep -R "kind: TotallyInvalid" "$dir" >/dev/null 2>&1; then
        kind="TotallyInvalid"
    fi
    echo "---"
    echo "apiVersion: apps/v1"
    echo "kind: $kind"
    echo "metadata:"
    echo "  name: rapport-app"
}

case "${1:-}" in
version)
    echo "v5.4.3"
    ;;
build)
    render_kustomize "${2:-.}"
    ;;
*)
    echo "unexpected kustomize args: $*" >&2
    exit 2
    ;;
esac
"""


def kubeconform_script() -> str:
    return """#!/bin/sh
set -u

if [ "${1:-}" = "-v" ]; then
    echo "v0.6.7"
    exit 0
fi

target="."
for arg in "$@"; do
    target="$arg"
done
if [ "$target" = "-" ]; then
    input="$(/bin/cat)"
    if printf "%s\n" "$input" | /usr/bin/grep "kind: TotallyInvalid" >/dev/null 2>&1; then
        echo "stdin - Deployment rapport-app failed validation: could not find schema for TotallyInvalid" >&2
        exit 1
    fi
elif /usr/bin/grep -R "kind: TotallyInvalid" "$target" >/dev/null 2>&1; then
    echo "deployment.yaml - Deployment rapport-app failed validation: could not find schema for TotallyInvalid" >&2
    exit 1
fi

echo "Summary: 1 resource found in 1 file - Valid: 1, Invalid: 0, Errors: 0, Skipped: 0"
"""


def terraform_script() -> str:
    return """#!/bin/sh
set -u

find_tf_files() {
    /usr/bin/find . -type f -name '*.tf' ! -path './.terraform/*' ! -path './.terragrunt-cache/*' ! -path './.terraform-cache/*' -print | /usr/bin/sort
}

has_format_issue() {
    for file in $(find_tf_files); do
        if /usr/bin/grep -q "bad_format=true" "$file"; then
            return 0
        fi
    done
    return 1
}

print_format_issues() {
    for file in $(find_tf_files); do
        if /usr/bin/grep -q "bad_format=true" "$file"; then
            echo "${file#./}"
        fi
    done
}

fix_format_issue() {
    for file in $(find_tf_files); do
        if /usr/bin/grep -q "bad_format=true" "$file"; then
            /usr/bin/awk '{ gsub("bad_format=true", "bad_format = true"); print }' "$file" > "$file.tmp" && /bin/mv "$file.tmp" "$file"
        fi
    done
}

has_validation_issue() {
    for file in $(find_tf_files); do
        if /usr/bin/grep -q "invalid_reference" "$file"; then
            return 0
        fi
    done
    return 1
}

case "${1:-}" in
version|--version)
    echo "Terraform v1.8.5"
    ;;
fmt)
    check=false
    for arg in "$@"; do
        if [ "$arg" = "-check" ]; then
            check=true
        fi
    done
    if has_format_issue; then
        if [ "$check" = "true" ]; then
            print_format_issues
            exit 3
        fi
        fix_format_issue
        exit 0
    fi
    exit 0
    ;;
validate)
    if has_validation_issue; then
        echo "Error: Reference to undeclared resource" >&2
        echo "A managed resource named invalid_reference has not been declared in the root module." >&2
        exit 1
    fi
    echo "Success! The configuration is valid."
    ;;
*)
    echo "unexpected terraform args: $*" >&2
    exit 2
    ;;
esac
"""


def tflint_script() -> str:
    return """#!/bin/sh
set -u

find_tf_files() {
    /usr/bin/find . -type f -name '*.tf' ! -path './.terraform/*' ! -path './.terragrunt-cache/*' ! -path './.terraform-cache/*' -print | /usr/bin/sort
}

case "${1:-}" in
--version)
    echo "TFLint version 0.51.1"
    ;;
--recursive)
    for file in $(find_tf_files); do
        if /usr/bin/grep -q "bad_tflint" "$file"; then
            echo "${file#./}:1:1: Warning - simulated Terraform lint failure" >&2
            exit 1
        fi
    done
    echo "TFLint passed"
    ;;
*)
    echo "unexpected tflint args: $*" >&2
    exit 2
    ;;
esac
"""


def zola_script() -> str:
    return """#!/bin/sh
set -u

has_broken_template() {
    [ -e templates ] && /usr/bin/grep -R "broken_template" templates >/dev/null 2>&1
}

has_broken_link() {
    for candidate in content templates; do
        if [ -e "$candidate" ] && /usr/bin/grep -R "broken_link" "$candidate" >/dev/null 2>&1; then
            return 0
        fi
    done
    return 1
}

case "${1:-}" in
build)
    if has_broken_template; then
        echo "Error: Failed to render template 'index.html'" >&2
        echo "Reason: template syntax error near broken_template" >&2
        exit 1
    fi
    echo "Zola build completed"
    ;;
check)
    if has_broken_template; then
        echo "Error: Failed to render template 'index.html'" >&2
        echo "Reason: template syntax error near broken_template" >&2
        exit 1
    fi
    if has_broken_link; then
        echo "Error: broken link detected in content/_index.md" >&2
        exit 1
    fi
    echo "Zola check completed"
    ;;
--version|-V)
    echo "zola 0.19.2"
    ;;
*)
    echo "unexpected zola args: $*" >&2
    exit 2
    ;;
esac
"""


def assert_case_state(case: Case, context: RunContext) -> str | None:
    failures: list[str] = []
    for assertion in case.assert_contains:
        path = rooted_path(context, assertion.root) / assertion.path
        try:
            contents = path.read_text()
        except OSError as exc:
            failures.append(f"could not read {path}: {exc}")
            continue
        if assertion.text not in contents:
            failures.append(f"{path} did not contain expected text")

    for assertion in case.assert_absent:
        path = rooted_path(context, assertion.root) / assertion.path
        if path.exists():
            failures.append(f"{path} should not exist")

    if failures:
        return "\n".join(failures)
    return None


def rooted_path(context: RunContext, root: str) -> Path:
    match root:
        case "project":
            return context.project
        case "temp":
            return context.temp_root
        case "cargo_target":
            if context.target is None:
                raise ValueError("case does not have a cargo target directory")
            return context.target
        case "cargo_home":
            if context.cargo_home is None:
                raise ValueError("case does not have a cargo home directory")
            return context.cargo_home
        case "toolchain":
            if context.toolchain is None:
                raise ValueError("case does not have a toolchain directory")
            return context.toolchain
        case _:
            raise ValueError(f"unsupported assertion root: {root}")


def render_snapshot(result: subprocess.CompletedProcess[str]) -> str:
    exit_code = str(result.returncode) if result.returncode >= 0 else "signal"
    return (
        f"exit: {exit_code}\n"
        "stdout:\n---\n"
        f"{normalize_trailing_newline(result.stdout)}"
        "stderr:\n---\n"
        f"{normalize_trailing_newline(result.stderr)}"
    )


def normalize_snapshot(snapshot: str, context: RunContext) -> str:
    text = snapshot.replace("\r\n", "\n").replace("\r", "\n")
    text = re.sub(r"duration: [0-9]+(?:\.[0-9]+)?s", "duration: [duration]", text)
    text = re.sub(r"in [0-9]+(?:\.[0-9]+)?s", "in [duration]", text)
    text = re.sub(
        r"Finished `([^`]+)` profile \[[^\]]+\] target\(s\)",
        r"Finished `\1` profile [cargo-profile] target(s)",
        text,
    )
    text = re.sub(r"rust-[0-9]+\.[0-9]+\.[0-9]+", "rust-[version]", text)
    text = re.sub(r"thread '([^']+)' \([0-9]+\) panicked", r"thread '\1' panicked", text)
    text = re.sub(
        r"could not compile `([^`]+)` \((lib|lib test)\)",
        r"could not compile `\1` ([cargo-target])",
        text,
    )
    text = re.sub(r"/[^ \t\n`']*\.rustup/toolchains/[^ \t\n`']+", "[toolchain]", text)

    replacements = [
        (context.project, "[project]"),
        (context.temp_root, "[tmp]"),
        (REPO_ROOT, "[repo]"),
        (CRATE_ROOT, "[crate]"),
    ]
    if context.target is not None:
        replacements.append((context.target, "[target]"))
    if context.cargo_home is not None:
        replacements.append((context.cargo_home, "[cargo-home]"))
    if context.toolchain is not None:
        replacements.append((context.toolchain, "[tools]"))

    for raw_path, replacement in snapshot_paths(replacements):
        text = text.replace(raw_path, replacement)

    text = re.sub(
        r"\[target\]/debug/deps/([A-Za-z0-9_-]+)-[0-9a-f]+",
        r"[target]/debug/deps/\1-[hash]",
        text,
    )
    return text


def snapshot_paths(paths: list[tuple[Path, str]]) -> list[tuple[str, str]]:
    variants: list[tuple[str, str]] = []
    for path, replacement in paths:
        variants.append((str(path), replacement))
        try:
            resolved = str(path.resolve())
        except OSError:
            resolved = str(path.absolute())
        if resolved != str(path):
            variants.append((resolved, replacement))
    variants.sort(key=lambda item: len(item[0]), reverse=True)
    return variants


def normalize_trailing_newline(s: str) -> str:
    if not s:
        return "(empty)\n"
    if s.endswith("\n"):
        return s
    return f"{s}\n"


def compare_or_update_snapshot(case: Case, actual: str, *, update: bool) -> str | None:
    if update:
        case.snapshot_path.parent.mkdir(parents=True, exist_ok=True)
        case.snapshot_path.write_text(actual)
        return None

    if not case.snapshot_path.exists():
        return f"missing snapshot: {case.snapshot_path}"

    expected = read_snapshot(case.snapshot_path)
    if expected == actual:
        return None

    diff = "".join(
        difflib.unified_diff(
            expected.splitlines(keepends=True),
            actual.splitlines(keepends=True),
            fromfile=str(case.snapshot_path),
            tofile=f"{case.name} actual",
        )
    )
    return f"snapshot mismatch:\n{diff}"


def read_snapshot(path: Path) -> str:
    text = path.read_text()
    lines = text.splitlines(keepends=True)
    if lines and lines[0].strip() == "---":
        for index in range(1, len(lines)):
            if lines[index].strip() == "---":
                return "".join(lines[index + 1 :])
    return text


def safe_name(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "-", name)


if __name__ == "__main__":
    raise SystemExit(main())

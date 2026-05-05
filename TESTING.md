# Testing

Rapport's unit and Rust integration tests stay Rust-native. CLI end-to-end
coverage is script-driven: the e2e harness builds `rapport` once, then runs the
compiled binary against copied fixture projects so Rapport can invoke Cargo or
SwiftPM tools without being nested inside Cargo's test runner.

## Commands

Run the repository's Rust test suite:

```bash
just test
```

Run the CLI end-to-end suite:

```bash
just e2e
```

Run only the Cargo e2e subset:

```bash
just tests/cargo/acceptance
```

Run the same checks GitHub Actions runs:

```bash
just ci
```

`just ci` runs formatting checks, clippy, a workspace build, the workspace Rust
test suite, and the external CLI e2e suite.

The Python harness environment is managed with uv. `just e2e` runs through
`uv run --locked`, so update `uv.lock` whenever Python tooling dependencies
change:

```bash
uv lock
```

## CLI End-to-End Tests

The e2e harness lives in `tests/e2e/run.py`. It discovers TOML case manifests
under:

```text
tests/e2e/cases/**/*.toml
```

Each case names the convention, fixture, verb, expected exit code, and snapshot
file. Cargo, SwiftPM, Fastlane, and Kustomize project fixtures live under:

```text
crates/rapport/tests/fixtures/{cargo,swift,fastlane,kustomize}/...
```

The child-path Cargo discovery fixture remains under `tests/cargo` because that
path is still the backwards-compatible Cargo-only entrypoint.

Snapshots live under:

```text
tests/e2e/snapshots/rapport/
```

Update accepted snapshots with:

```bash
uv run --locked python tests/e2e/run.py --update
```

Review snapshot changes before committing them. They are the command-level
behavior contract for exit codes, stdout, stderr, failure messages, and
next-action hints.

For focused iteration, run one case by name:

```bash
uv run --locked python tests/e2e/run.py --case cargo_ok_basic_crate_build
```

The harness also honors `RAPPORT_BIN=/path/to/rapport` when you want to test a
prebuilt binary instead of rebuilding first.

## Harness Behavior

Every e2e case copies its fixture into a temporary directory and writes a small
`.git` marker so project discovery behaves like it is inside a repository.

Cargo cases use isolated, per-case tool state:

- `CARGO_TARGET_DIR` points at a temp target directory
- `CARGO_HOME` points at a temp cargo home
- `CARGO_TERM_COLOR=never`
- `CARGO_INCREMENTAL=0`
- `CARGO_BUILD_JOBS=1`

The harness also removes ambient Rust configuration such as `RUSTFLAGS`,
`RUSTDOCFLAGS`, `RUSTC_WRAPPER`, and related variables that could change output
on one machine but not another.

SwiftPM cases use generated fake `swift` and `swift-format` tools. Fastlane
cases use a generated fake `bundle` tool that simulates `bundle exec fastlane`.
Kustomize cases use generated fake `kubectl`, standalone `kustomize`, and
`kubeconform` tools. That keeps Swift, Fastlane, and Kubernetes e2e coverage
available on machines and CI runners that do not have those toolchains
installed.

## Snapshot Stability

End-to-end tests should control the environment first, then filter any remaining
volatile output.

The e2e runner normalizes:

- temp project, target, cargo-home, toolchain, repository, and crate paths
- durations
- rustup toolchain paths
- Rust patch versions in rustdoc or clippy URLs
- Cargo test binary hashes
- Cargo target labels that vary by toolchain output

When adding a new e2e convention, use the same pattern: isolate tool caches,
keep generated files inside temp directories, disable color and machine-specific
configuration, then add runner-specific snapshot filters for paths, timestamps,
versions, generated IDs, ports, or cache locations.

## CI

GitHub Actions runs the test suite through `.github/workflows/ci.yml`.

The `CI` workflow runs on pull requests and pushes to `main`. It installs the
stable Rust toolchain, `just`, `nextest`, uv, and the project Python version,
then runs:

```bash
just ci
```

Future e2e conventions should add fixture cases to `tests/e2e/cases` and update
the workflow if they require another external toolchain.

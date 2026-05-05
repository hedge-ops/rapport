# Testing

Rapport's own unit and integration tests are Rust-native. Acceptance coverage is
script-driven: the harness builds `rapport` once, then runs the compiled binary
against fixture projects so Rapport can invoke Cargo without being nested inside
Cargo's test runner.

## Commands

Run the repository's standard test command:

```bash
just test
```

Run the Cargo acceptance fixtures:

```bash
just acceptance
```

Run the Cargo end-to-end integration tests directly:

```bash
cargo test -p rapport --test cargo_e2e -- --test-threads=1
```

Run the Cargo acceptance fixtures directly:

```bash
just tests/cargo/acceptance
```

Run the same checks GitHub Actions runs:

```bash
just ci
```

`just ci` runs formatting checks, clippy, a workspace build, the workspace test
suite, and the script-backed acceptance suite.

## Cargo End-to-End Tests

The Cargo end-to-end harness lives in
`crates/rapport/tests/cargo_e2e.rs`. It is a normal Cargo integration test, so
it is discovered by `cargo test` and `cargo nextest run --workspace`.

The harness uses `assert_cmd` to invoke the compiled `rapport` binary. Each test
copies a fixture project into a fresh temporary directory before running a verb,
so commands like `cargo fmt`, generated `Cargo.lock` files, and `target/`
artifacts never mutate checked-in fixtures.

Cargo fixtures live under:

```text
crates/rapport/tests/fixtures/cargo/{ok,fail}/...
```

Snapshots live under:

```text
crates/rapport/tests/snapshots/
```

Update accepted snapshots with:

```bash
INSTA_UPDATE=always cargo test -p rapport --test cargo_e2e
```

Review snapshot changes before committing them. They are part of the public
behavior contract for exit codes, stdout, stderr, failure messages, and
next-action hints.

## Cargo Acceptance Tests

The Cargo acceptance harness lives in `tests/cargo/acceptance.sh`. It builds the
`rapport` binary once, discovers expectation files, runs each expectation
directly, and prints one pass/fail line per expectation before reporting all
failures at the end.

Acceptance fixtures live under:

```text
tests/cargo/<case>/
```

Each fixture has a `rapport.toml` input path and per-verb expectation files
under `expectations/`, such as `build.ok.toml` or `lint.fail.toml`. Optional
stdout and stderr snapshots sit next to those expectation files.

Run the cargo acceptance suite with:

```bash
just tests/cargo/acceptance
```

The script gives each expectation its own temporary `CARGO_TARGET_DIR` and
`CARGO_HOME`, disables color and ambient Rust flags, and normalizes repository
paths plus durations before comparing snapshots. It intentionally runs outside
Cargo's test harness because the binary under test invokes Cargo itself.

## Snapshot Stability

End-to-end tests should control the environment first, then filter any remaining
volatile output.

The Cargo harness isolates Cargo with temporary directories and deterministic
environment values:

- `CARGO_TARGET_DIR` points at a temp target directory
- `CARGO_HOME` points at a temp cargo home
- `CARGO_TERM_COLOR=never`
- `CARGO_INCREMENTAL=0`
- `CARGO_BUILD_JOBS=1`

It also removes ambient Rust configuration such as `RUSTFLAGS`, `RUSTDOCFLAGS`,
`RUSTC_WRAPPER`, and related variables that could change output on one machine
but not another.

Snapshot filters redact unstable values, including:

- temp project, target, and cargo-home paths
- repository and crate absolute paths
- durations
- rustup toolchain paths
- Rust patch versions in rustdoc/clippy URLs
- Cargo test binary hashes
- Cargo target labels that vary by toolchain output

When adding a new end-to-end harness, use the same pattern: isolate any tool
caches, keep generated files inside temp directories, disable color and
machine-specific configuration, then add runner-specific snapshot filters for
paths, timestamps, versions, generated IDs, ports, or cache locations.

## Future Runner Coverage

Cargo remains the outer test runner for Rapport itself. Future ecosystem
runners should add their own integration test modules and fixtures while keeping
the same shape:

```text
just test runs Rapport's tests
  -> npm_e2e.rs invokes rapport on npm fixtures
  -> go_e2e.rs invokes rapport on Go fixtures
  -> cargo_e2e.rs invokes rapport on Cargo fixtures
```

If the next runner duplicates enough harness code, extract shared helpers under
`crates/rapport/tests/support/` for fixture copying, binary invocation,
environment isolation, and common snapshot filters.

## CI

GitHub Actions runs the test suite through `.github/workflows/ci.yml`.

The `CI` workflow runs on pull requests and pushes to `main`. It installs the
stable Rust toolchain, `just`, and `nextest`, then runs:

```bash
just ci
```

The Cargo acceptance fixtures are part of CI through `just ci`, which invokes
`just acceptance`. If a future runner depends on another toolchain, such as
Node, Go, or Python, the workflow must install that toolchain before `just ci`.

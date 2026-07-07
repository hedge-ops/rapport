# Testing

Rapport uses Rust-native tests.

## Commands

Run the repository's Rust test suite:

```bash
just test
```

Run the same checks GitHub Actions runs:

```bash
just ci
```

`just ci` runs formatting checks, clippy, a workspace build, and the workspace
Rust test suite.

## CI

GitHub Actions runs the test suite through `.github/workflows/ci.yml`.

The `CI` workflow runs on pull requests and pushes to `main`. It installs the
stable Rust toolchain, `just`, and `nextest`, then runs:

```bash
just ci
```

Future command-level coverage should be added around the new `work -> build ->
integrate` CLI surface as it lands.

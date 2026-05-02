# Changelog

All notable changes to `rapport` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this crate
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-02

### Added

- Added the first runnable `rapport` cargo lifecycle CLI.
- Added `fix`, `lint`, `build`, `test`, `validate`, and `audit` verbs.
- Added injected command execution for tests and production cargo runs.
- Added prose-backed success and failure output with required next actions.
- Added captured failure output for failed cargo steps.

### Changed

- Changed `build` to use `cargo check` for the fast compile-verification path.

### Known limitations

- Project discovery is not implemented yet; `rapport` currently assumes cargo
  for any directory path it is given.

## [0.0.1] - 2026-04-27

Name-reservation release. No functionality yet; running the binary prints
a pointer to the workspace.

[Unreleased]: https://github.com/hedge-ops/rapport/compare/rapport-v0.1.0...HEAD
[0.1.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.0.1...rapport-v0.1.0
[0.0.1]: https://github.com/hedge-ops/rapport/releases/tag/rapport-v0.0.1

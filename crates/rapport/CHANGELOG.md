# Changelog

All notable changes to `rapport` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this crate
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added `rapport context signoff add`, `remove`, and `repair` commands that own
  exact GitHub workflows for requesting SHA-bound local signoffs.
- Added byte-for-byte doctor validation for shared and folder-target signoff
  request workflows.

### Changed

- Changed integration to validate the complete signoff contract before any Git
  commit or pull-request side effect and to report folder-qualified statuses.

## [0.3.0] - 2026-07-10

### Added

- Added inherited `signoffs` to folder-owned `context.toml` files.
- Added active-work path resolution that unions applicable signoffs and records
  them as pending integration facts for matching GitHub Actions checks.

### Changed

- Replaced repository-wide `signoffs.toml` command and manual signoffs with
  folder-owned signoff needs. GitHub Actions now own host-specific execution.

## [0.2.0] - 2026-07-09

### Added

- Added the `work`, `build`, `integrate`, `complete`, `init`, `prime`,
  `doctor`, and `context` workflow surface.
- Added GitHub Release archives for supported platforms so
  `cargo binstall rapport` can install prebuilt binaries.
- Added git-root-bounded project discovery for Cargo projects via `Cargo.toml`.

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

[Unreleased]: https://github.com/hedge-ops/rapport/compare/rapport-v0.3.0...HEAD
[0.3.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.2.0...rapport-v0.3.0
[0.2.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.1.0...rapport-v0.2.0
[0.1.0]: https://github.com/hedge-ops/rapport/compare/rapport-v0.0.1...rapport-v0.1.0
[0.0.1]: https://github.com/hedge-ops/rapport/releases/tag/rapport-v0.0.1

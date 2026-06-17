# Changelog

All notable changes to `rapport-temporal` are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.3] - 2026-06-17

### Added

- First-class serde support for `Instant` as a UTC RFC 3339 string. New
  `Instant::to_rfc3339` and `Instant::from_rfc3339` format and parse `Z`-suffixed
  UTC timestamps with optional fractional seconds up to nanosecond precision.
- Reusable `time::rfc3339` and `time::rfc3339::option` serde helper modules for
  required and optional `Instant` fields via `#[serde(with = "...")]`.
- `Error::InvalidInstant` and `Error::NonUtcInstant` variants giving clear,
  distinct errors for malformed and non-UTC timestamps.

## [0.2.2] - 2026-06-16

### Added

- Added a `testing` feature with fixed date and time fixtures for deterministic
  downstream tests.

## [0.2.0] - 2026-05-24

### Changed

- **Breaking:** bumped `facet` from `=0.31` to `=0.44`. A `Facet` impl derived
  under 0.44 does not satisfy a 0.31 trait bound (or vice versa), so any
  consumer relying on the `Facet` impl must also be on facet 0.44.
- Enabled facet's `nonzero` feature so `Interval` (a `NonZeroU16` newtype) can
  derive `Facet`.

### Added

- `RelativeOffset` now derives `facet::Facet`, `serde::Serialize`,
  `serde::Deserialize`, and `Hash`, and is `#[repr(C)]`, so it can be carried
  structurally across a view/event boundary instead of round-tripping its
  `Display` text.
- `Interval` now derives `facet::Facet`, `serde::Serialize`,
  `serde::Deserialize`, and `Hash`.

## [0.1.0] - 2026-04-27

Initial release.

- `Date` for ergonomic date handling without the heat-death-of-the-universe edge cases.
- `time` module for instants.
- `recurrence` for text-driven recurrence rules (daily, weekly, monthly, yearly).
- `offset` for relative-date language (yesterday, tomorrow, a month from now).
- `query` parser turning human/agent expressions into typed values.
- `clock` for testable time.

[Unreleased]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.2.3...HEAD
[0.2.3]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.2.2...rapport-temporal-v0.2.3
[0.2.2]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.2.1...rapport-temporal-v0.2.2
[0.2.0]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.1.0...rapport-temporal-v0.2.0
[0.1.0]: https://github.com/hedge-ops/rapport/releases/tag/rapport-temporal-v0.1.0

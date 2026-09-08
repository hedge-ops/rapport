# Changelog

All notable changes to `rapport-temporal` are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.2] - 2026-09-08

### Added

- `Clock::system_utc()` creates an infallible, real advancing clock with a UTC
  calendar, independent of machine timezone resolution and process environment.

## [0.3.1] - 2026-09-08

### Added

- `Timezone::duration_until_date_change(instant)` for nanosecond-precise daily
  refresh scheduling across repeated or skipped midnights, skipped dates, and
  backward date transitions. Existing strict midnight operations are unchanged.

## [0.3.0] - 2026-09-08

### Added

- Explicit `Timezone::{Utc, Named(chrono_tz::Tz)}` calendar context with validated
  parsing and Serde transport, using bundled IANA rules and system discovery via
  `iana-time-zone`.
- Pure `date_at`, `start_of_day`, and nanosecond-precise
  `duration_until_midnight` operations. Repeated/missing midnight (including
  skipped dates) returns a typed error consistently; invalid ranges never fall
  back to UTC or an unrelated date.
- `Clock::timezone()` and `FakeClock::with_timezone(instant, timezone)`.

### Changed

- **Breaking:** `Clock::System` now holds a `Timezone`; `Clock::system()` resolves
  the machine's OS timezone once and returns `Result<Clock, Error>`. Recreate it
  to refresh zone selection after a machine timezone change.
- **Breaking:** `Clock::today()` returns `Result<Date, Error>` and projects with
  the captured timezone. Fake clocks default to UTC, independent of the host.
- Documented migration examples and preserved legacy host-local conversions.


## [0.2.5] - 2026-08-20

### Added

- Added `Interval::six()` as a named constructor for six-month relative date
  calculations.

## [0.2.4] - 2026-06-17

### Changed

- Bumped `facet` from `=0.44` to `=0.46`. The 0.46 release was API-compatible
  for this crate (no source changes were required), but a `Facet` impl derived
  under 0.46 does not satisfy a 0.44 trait bound, so any consumer relying on the
  `Facet` impl must also be on facet 0.46.

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

[Unreleased]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.3.1...HEAD
[0.3.1]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.3.0...rapport-temporal-v0.3.1
[0.3.0]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.2.5...rapport-temporal-v0.3.0
[0.2.5]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.2.4...rapport-temporal-v0.2.5
[0.2.4]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.2.3...rapport-temporal-v0.2.4
[0.2.3]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.2.2...rapport-temporal-v0.2.3
[0.2.2]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.2.1...rapport-temporal-v0.2.2
[0.2.0]: https://github.com/hedge-ops/rapport/compare/rapport-temporal-v0.1.0...rapport-temporal-v0.2.0
[0.1.0]: https://github.com/hedge-ops/rapport/releases/tag/rapport-temporal-v0.1.0

# AGENTS.md

## Purpose

`rapport-temporal` owns ergonomic date, instant, recurrence, relative offset,
query parsing, and clock primitives for business-facing Rust applications.

## Ownership

- Owns the public temporal types exported from `src/lib.rs`.
- Owns testing-only temporal fixtures behind the `testing` feature.
- Does not own application-specific scheduling policy, persistence, billing, or
  user-interface formatting beyond the generic temporal contracts in this crate.

## Vocabulary

- `Date` is a calendar date with no time-of-day.
- `Instant` is a UTC timestamp-like value suitable for storage and boundaries.
- `Clock` supplies current time; production code uses `Clock::System`, tests use
  `FakeClock`.
- `RecurrenceSchedule` describes repeat cadence; `RecurrenceRule` pairs a
  schedule with a start date.
- `RelativeOffset` describes a user-facing offset from a reference date.

## Standards

- Follow the People Work `/coding`, `/coding/rust.md`, `/coding/comments.md`,
  `/testing`, and `/building` standards when changing this crate.
- Keep date and time logic deterministic in tests. Use injected clocks or
  `rapport_temporal::testing` fixtures instead of current system time.
- Gate helper APIs that exist only for tests behind the `testing` feature.
- Keep dependencies intentional and verify them with the crate-local
  `check-deps` recipe.

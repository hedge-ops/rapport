# rapport-temporal

Deal with time without all the fuss. `chrono` is a great crate but it makes you be a nerd about all the edge cases for date management, which gets in the way of someone writing a business/user-facing application that doesn't take the heat death of the universe into account.

We won't worry about what happens after the sun dies, we'll get stuff done here.

## Installation

Add this to your `Cargo.toml`:

```toml
rapport-temporal = "0.3.0"
```

## Usage

```rust
use rapport_temporal::date::Date;
use rapport_temporal::recurrence::RecurrenceRule;

let today = Date::today();
let rule = RecurrenceRule::parse("weekly on monday", today).unwrap();
let next_monday = rule.next_occurrence_after(today);
```

## Explicit calendar context

`Instant` remains an absolute UTC timestamp and `Date` remains a calendar date.
`Timezone` supplies the calendar interpretation, including daylight-saving rules.
Create the system clock at the application boundary and pass its timezone alongside
its instant into pure application code:

```rust
use rapport_temporal::{clock::Clock, Error};

fn system_context() -> Result<(), Error> {
    let clock = Clock::system()?;
    let timezone = clock.timezone();
    let now = clock.now();
    let today = timezone.date_at(now)?;
    let start = timezone.start_of_day(today)?;
    let until_midnight = timezone.duration_until_midnight(now)?;
    Ok(())
}
```

`Clock::system()` resolves the operating system's IANA timezone once. It does not
follow later machine timezone changes: construct a new system clock and propagate
its new context to refresh. Discovery uses the OS configuration through
[`iana-time-zone`](https://docs.rs/iana-time-zone/), rather than the process `TZ`
override used by the legacy `chrono::Local` APIs. If discovery fails or the name
is unsupported, construction returns an error. Applications can explicitly select
`Clock::System(timezone)` instead. No automatic UTC fallback is applied.

Fake clocks default to UTC, regardless of the host timezone. Supply a named zone
when a test needs a different calendar:

```rust
use rapport_temporal::{clock::{Clock, FakeClock}, time::Instant, Timezone, Error};

fn fake_context() -> Result<(), Error> {
    let now = Instant::from_rfc3339("2026-01-01T00:00:00Z")?;
    let utc_clock = Clock::from(FakeClock::new(now));
    assert_eq!(utc_clock.today()?.into_iso_string(), "2026-01-01");

    let timezone: Timezone = "America/Los_Angeles".parse()?;
    let fake = FakeClock::with_timezone(now, timezone);
    let clock = Clock::from(&fake);
    assert_eq!(clock.today()?.into_iso_string(), "2025-12-31");
    assert_eq!(clock.today()?, timezone.date_at(now)?);
    Ok(())
}
```

Cloning a fake retains shared mutable time and the same immutable timezone.
`add_days` still advances by exactly 24 hours per day; it is elapsed-time movement,
not calendar-day arithmetic. `FakeClock::default()` still initializes its instant
from the current system time, but its timezone is UTC; use `new` with a fixed
instant for deterministic tests.

### Midnight policy and ranges

`Timezone::start_of_day(date)` means exactly local midnight. It returns
`Error::AmbiguousMidnight { date, timezone }` if midnight happens twice, and
`Error::NonexistentMidnight { date, timezone }` if midnight is missing, including
entirely skipped dates. For example, Havana's 2020-11-01 midnight is ambiguous,
São Paulo's 2018-11-04 midnight is missing, and Apia skipped 2011-12-30 entirely.
These errors let the application choose whether to skip a date, use a later time,
or ask for a policy; the library does not choose an arbitrary instant.

`duration_until_midnight(instant)` applies exactly that policy to the **next
calendar date**, even when the input is exactly midnight. It returns a
nanosecond-precise `time::Duration`; a complete ordinary day is 24 hours, while
Los Angeles's 2026 spring/fall transition days are 23/25 hours. Missing or
ambiguous next midnight returns the same typed error as `start_of_day`.
`date_at` can still project instants within a date whose midnight is missing or
ambiguous.

For daily refresh scheduling, use `duration_until_date_change(instant)`. It
returns a strictly positive, nanosecond-precise duration to the earliest subsequent
instant whose local date differs. It selects the first repeated midnight when
approaching from the previous date, ignores a second midnight within the same
date, and handles missing midnight or an entirely skipped date at the actual
transition. Historical backward transitions to an earlier date count too. A call
exactly at a boundary schedules the following date change.

```rust
use rapport_temporal::{Error, time::Instant, Timezone};

let timezone: Timezone = "America/Havana".parse()?;
let now = Instant::from_rfc3339("2020-10-31T12:00:00Z")?;
let delay = timezone.duration_until_date_change(now)?;
assert_eq!(delay.as_secs(), 16 * 3600);
# Ok::<(), Error>(())
```

The calculation visits successive whole seconds in the bundled timezone rules
until the date changes. This preserves even brief backward date changes without
assuming that local dates increase monotonically; input subseconds are retained
in the returned duration. No host timezone lookup is performed.

Explicit operations return `Error::CalendarOutOfRange` for invalid instants,
nanoseconds outside `0..1_000_000_000` (leap-second encodings are unsupported),
unsupported date ranges, or a start of day before the Unix epoch (`Instant` uses
unsigned seconds). They do not use the legacy constructors' saturation or fallback
behavior. This makes `Clock::today()` fallible as well, rather than silently
interpreting invalid fake-clock input as an unrelated date.

### Transport and dependencies

`Timezone` derives Serde with stable representations: `Timezone::Utc` is `"Utc"`
and a named zone is `{"Named":"America/Los_Angeles"}`. Deserialization validates
named zones. These values can travel alongside an instant across shell/core
boundaries without a live clock. `FromStr` also accepts `"UTC"` and validated
IANA names; `Display` emits the name. Prefer Serde when preserving the enum variant
matters: both `Utc` and `Named(chrono_tz::UTC)` display as `UTC`.

Named zones use [`chrono-tz::Tz`](https://docs.rs/chrono-tz/0.10.4/chrono_tz/enum.Tz.html),
a validated enum with bundled IANA transition rules, instead of a fixed offset.
This fits the existing Chrono date/time types and makes pure operations independent
of host timezone lookups. `iana-time-zone` is used only for discovery at the system
clock boundary, with a minimum version of 0.1.65 to include macOS cache refresh
on discovery. Bundled rules update when the dependency is updated and the
application is rebuilt; recreating a clock refreshes the machine's zone selection,
not the bundled rule database. Applications requiring matching cross-process
results should use matching dependency versions.

### Migrating from 0.2

- Replace unit construction `Clock::System` with `Clock::system()?`, or provide an
  explicit zone with `Clock::System(timezone)`. Update enum pattern matches.
- Handle `Clock::today()`'s `Result<Date, Error>` (usually with `?`).
- Fake-clock calendar calculations now default to **UTC**, not the host calendar.
  Use `FakeClock::with_timezone(instant, timezone)` for another calendar.
- Migrate `instant.into_date()` and `Date::from(instant)` to
  `timezone.date_at(instant)?`; migrate `Instant::from(date)` to
  `timezone.start_of_day(date)?`; migrate `instant.duration_until_midnight()` to
  `timezone.duration_until_midnight(instant)?`.
- Existing implicit conversions and `Date::today()` remain available with their
  host-local behavior. Legacy date-to-instant conversion chooses the earliest
  repeated midnight, falls back to UTC for missing midnight, and clamps pre-epoch
  timestamps. Legacy midnight duration also retains its fallback and whole-second
  behavior. They do not acquire context from an injected clock.

Version 0.3.0 marks the clock construction and return-type changes as breaking.
Application-specific shell wiring and scheduling recovery policies remain the
consumer's responsibility.

## RFC 3339 Instants

`Date` serializes as an ISO `YYYY-MM-DD` string out of the box. For `Instant`,
the `time::rfc3339` serde helpers represent a value as a UTC RFC 3339 string with
a `Z` suffix (with optional fractional seconds up to nanosecond precision). Use
them on required and optional fields:

```rust
use rapport_temporal::time::{rfc3339, Instant};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Event {
    #[serde(with = "rfc3339")]
    at: Instant,
    #[serde(with = "rfc3339::option")]
    ended_at: Option<Instant>,
}
```

You can also format and parse directly with `Instant::to_rfc3339` and
`Instant::from_rfc3339`. Parsing rejects malformed timestamps and any non-UTC
offset with a clear error.

## Testing Fixtures

Enable the `testing` feature in test-only dependencies when another crate needs
fixed dates or times:

```toml
rapport-temporal = { version = "0.3.0", features = ["testing"] }
```

```rust
use rapport_temporal::testing::{now, today};

let current_date = today();
let current_time = now();
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

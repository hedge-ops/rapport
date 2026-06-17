# rapport-temporal

Deal with time without all the fuss. `chrono` is a great crate but it makes you be a nerd about all the edge cases for date management, which gets in the way of someone writing a business/user-facing application that doesn't take the heat death of the universe into account.

We won't worry about what happens after the sun dies, we'll get stuff done here.

## Installation

Add this to your `Cargo.toml`:

```toml
rapport-temporal = "0.2.3"
```

## Usage

```rust
use rapport_temporal::date::Date;
use rapport_temporal::recurrence::RecurrenceRule;

let today = Date::today();
let rule = RecurrenceRule::parse("weekly on monday", today).unwrap();
let next_monday = rule.next_occurrence_after(today);
```

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
rapport-temporal = { version = "0.2.2", features = ["testing"] }
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

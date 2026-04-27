# rapport-temporal

Deal with time without all the fuss. `chrono` is a great crate but it makes you be a nerd about all the edge cases for date management, which gets in the way of someone writing a business/user-facing application that doesn't take the heat death of the universe into account.

We won't worry about what happens after the sun dies, we'll get stuff done here.

## Installation

Add this to your `Cargo.toml`:

```toml
rapport-temporal = "0.1.0"
```

## Usage

```rust
use rapport_temporal::date::Date;
use rapport_temporal::recurrence::RecurrenceRule;

let today = Date::today();
let rule = RecurrenceRule::parse("weekly on monday", today).unwrap();
let next_monday = rule.next_occurrence_after(today);
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

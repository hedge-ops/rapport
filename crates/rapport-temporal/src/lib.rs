//! Ergonomic, human friendly way to handle date-based time.
//!
//! ## Motivation
//!
//! `chrono` is a great crate, and this library uses it, but it doesn't easily cover the fact that
//! lots of time humans just want to talk in dates. They might want to do something tomorrow, they
//! want a date to do that, and developers want an easy way to make that happen without a lot of
//! fuss.
//!
//! Also with error handling, yes it might be the heat death date of the universe, but we don't want
//! or need error handling to fail when this is the case. If someone is being weird and giving the
//! library extreme data, we fall back to sensible dates, to make the whole thing ergonomic and
//! delightful to use.
//!
//! ## Modules
//!
//! `date`: simple way of managing simple dates.
//! `time`: simple way to manage what time it is, as an `Instant`.
//! `recurrence`: simple, text-driven recurrence rules, that, yes, don't cover every recurrence but
//! also make it easy for a human to do most things.
//! `offset`: language for relative dates, like yesterday, tomorrow, or a month from now.
//! `query`: parser for human and agent friendly expression of relative dates, into this library's types.
//! `clock`: easily test for time changes with a testable clock.

pub mod clock;
pub mod date;
pub mod offset;
pub mod query;
pub mod recurrence;
pub mod time;

#[cfg(feature = "testing")]
pub mod testing;

pub(crate) trait DisplayExt {
    fn displayed(self) -> String;
}

impl<T: std::fmt::Display> DisplayExt for Option<T> {
    fn displayed(self) -> String {
        self.map_or_else(|| "none".to_string(), |v| v.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("date is not a valid format. Use YYYY-mm-dd format, surrounded by double quotes.")]
    InvalidDate,
    #[error("unable to parse recurrence string: {0}")]
    InvalidRecurrence(String),
    #[error("invalid offset: {0}")]
    InvalidOffset(String),
    #[error(
        "instant is not a valid RFC 3339 timestamp (expected e.g. `2026-06-17T12:00:00Z`): {0}"
    )]
    InvalidInstant(String),
    #[error("instant must be UTC (offset must be zero, e.g. a `Z` suffix or `+00:00`): {0}")]
    NonUtcInstant(String),
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::DisplayExt;

    #[test]
    fn displayed_should_print_inner_value_or_none() {
        assert_eq!(Some(7).displayed(), "7");
        assert_eq!(Option::<u8>::None.displayed(), "none");
    }
}

//! Fixed temporal fixtures for deterministic tests.

use crate::{date::Date, time::Instant};

const fn instant(seconds: u64) -> Instant {
    Instant { seconds, nanos: 0 }
}

/// For testing, a date months ago: 2025-05-01.
#[must_use]
pub fn months_ago() -> Date {
    Date::from_str_unchecked("2025-05-01")
}

/// For testing, a time months ago: 2025-05-01T10:13:44Z.
#[must_use]
pub fn months_ago_time() -> Instant {
    instant(1_746_094_424)
}

/// For testing, yesterday's date: 2025-09-29.
#[must_use]
pub fn yesterday() -> Date {
    Date::from_str_unchecked("2025-09-29")
}

/// For testing, yesterday's time: 2025-09-29T10:07:00Z.
#[must_use]
pub fn yesterday_time() -> Instant {
    instant(1_759_140_420)
}

/// For testing, today's date: 2025-09-30.
#[must_use]
pub fn today() -> Date {
    Date::from_str_unchecked("2025-09-30")
}

/// For testing, the current time: 2025-09-30T10:07:00Z.
#[must_use]
pub fn now() -> Instant {
    instant(1_759_226_820)
}

/// For testing, tomorrow's date: 2025-10-01.
#[must_use]
pub fn tomorrow() -> Date {
    Date::from_str_unchecked("2025-10-01")
}

/// For testing, a date three days from today: 2025-10-03.
#[must_use]
pub fn three_days_from_today() -> Date {
    today().add_days(3)
}

/// For testing, a date next week: 2025-10-06.
#[must_use]
pub fn next_week() -> Date {
    today().next_monday()
}

/// For testing, a date in two weeks: 2025-10-14.
#[must_use]
pub fn in_two_weeks() -> Date {
    today().add_days(14)
}

/// For testing, a date next month: 2025-10-01.
#[must_use]
pub fn next_month() -> Date {
    today().first_of_next_month()
}

/// For testing, a time next month: 2025-10-30T10:07:00Z.
#[must_use]
pub fn next_month_time() -> Instant {
    instant(1_761_818_820)
}

/// For testing, a time in two months: 2025-11-30T10:07:00Z.
#[must_use]
pub fn two_months_time() -> Instant {
    instant(1_764_497_220)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn fixed_dates_match_expected_values() {
        assert_eq!(months_ago().into_iso_string(), "2025-05-01");
        assert_eq!(yesterday().into_iso_string(), "2025-09-29");
        assert_eq!(today().into_iso_string(), "2025-09-30");
        assert_eq!(tomorrow().into_iso_string(), "2025-10-01");
        assert_eq!(three_days_from_today().into_iso_string(), "2025-10-03");
        assert_eq!(next_week().into_iso_string(), "2025-10-06");
        assert_eq!(in_two_weeks().into_iso_string(), "2025-10-14");
        assert_eq!(next_month().into_iso_string(), "2025-10-01");
    }

    #[test]
    fn fixed_times_match_expected_values() {
        assert_eq!(months_ago_time().to_string(), "2025-05-01 10:13:44 UTC");
        assert_eq!(yesterday_time().to_string(), "2025-09-29 10:07:00 UTC");
        assert_eq!(now().to_string(), "2025-09-30 10:07:00 UTC");
        assert_eq!(next_month_time().to_string(), "2025-10-30 10:07:00 UTC");
        assert_eq!(two_months_time().to_string(), "2025-11-30 10:07:00 UTC");
    }
}

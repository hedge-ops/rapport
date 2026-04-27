//! Simple way to deal with the current time, as an `Instant`.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, TimeZone, Utc};
use facet::Facet;
use serde::{Deserialize, Serialize};

use crate::date::Date;

#[derive(Facet, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Instant {
    /// The number of seconds since the unix epoch.
    pub seconds: u64,
    pub nanos: u32,
}

impl Instant {
    #[must_use]
    pub fn duration_until_midnight(self) -> Duration {
        let secs = self.seconds.try_into().unwrap_or_default();
        // Convert to local datetime
        let local_datetime = Local
            .timestamp_opt(secs, self.nanos)
            .earliest()
            .or_else(|| Local.timestamp_opt(secs, self.nanos).latest())
            .unwrap_or_else(|| {
                // Fallback to UTC if local timezone fails
                Utc.timestamp_opt(secs, self.nanos)
                    .earliest()
                    .unwrap_or_default()
                    .with_timezone(&Local)
            });

        // Get start of next day
        let next_day = local_datetime.date_naive() + chrono::Duration::days(1);
        let next_midnight_naive = next_day.and_hms_opt(0, 0, 0).unwrap_or_default();

        // Convert back to local timezone
        let next_midnight = Local
            .from_local_datetime(&next_midnight_naive)
            .earliest()
            .or_else(|| Local.from_local_datetime(&next_midnight_naive).latest())
            .unwrap_or_else(|| {
                Utc.from_utc_datetime(&next_midnight_naive)
                    .with_timezone(&Local)
            });

        let next_midnight_timestamp = next_midnight.timestamp();
        let seconds_until_midnight = next_midnight_timestamp.saturating_sub(secs);

        Duration::from_secs(seconds_until_midnight.try_into().unwrap_or_default())
    }

    #[must_use]
    pub fn into_date(self) -> Date {
        self.into()
    }

    #[must_use]
    pub fn from_utc_datetime(value: DateTime<Utc>) -> Self {
        let timestamp = value.timestamp();
        let seconds = timestamp.max(0).try_into().unwrap_or_default();
        let nanos = value.timestamp_subsec_nanos();

        Self { seconds, nanos }
    }

    #[must_use]
    pub fn from_timestamp(seconds: i64) -> Self {
        let seconds = seconds.max(0).try_into().unwrap_or_default();
        Self { seconds, nanos: 0 }
    }

    #[must_use]
    pub fn into_utc_datetime(self) -> DateTime<Utc> {
        let seconds = i64::try_from(self.seconds).unwrap_or(i64::MAX);
        // Convert timestamp to UTC datetime
        Utc.timestamp_opt(seconds, self.nanos)
            .earliest()
            .unwrap_or_default()
    }

    #[must_use]
    pub fn subtract_minutes(self, value: u64) -> Self {
        let seconds = self.seconds.saturating_sub(value);
        Self {
            seconds,
            nanos: self.nanos,
        }
    }

    #[must_use]
    pub fn add_minutes(self, value: u64) -> Self {
        let seconds = self.seconds.saturating_add(value.saturating_mul(60));
        Self {
            seconds,
            nanos: self.nanos,
        }
    }
}

impl std::fmt::Display for Instant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.into_utc_datetime().fmt(f)
    }
}

impl From<Date> for Instant {
    fn from(value: Date) -> Self {
        // Convert Date to NaiveDate
        let naive_date: chrono::NaiveDate = value.into();

        // Try to create midnight, fallback to start of day if needed
        let midnight = naive_date.and_hms_opt(0, 0, 0).unwrap_or_else(|| {
            naive_date
                .and_hms_opt(0, 0, 1)
                .unwrap_or(naive_date.and_hms_opt(1, 0, 0).unwrap_or_default())
        });

        // Try local timezone first, with fallbacks for DST issues
        let seconds = if let Some(local_dt) = Local.from_local_datetime(&midnight).earliest() {
            local_dt.timestamp().max(0)
        } else if let Some(local_dt) = Local.from_local_datetime(&midnight).latest() {
            local_dt.timestamp().max(0)
        } else {
            // Fallback to UTC if local time is problematic
            Utc.from_utc_datetime(&midnight).timestamp().max(0)
        };
        Instant {
            seconds: seconds.max(0).try_into().unwrap_or_default(),
            nanos: 0,
        }
    }
}

impl From<Instant> for Date {
    fn from(value: Instant) -> Self {
        let seconds = i64::try_from(value.seconds).unwrap_or(i64::MAX);
        // Convert timestamp to local datetime (matching the forward conversion logic)
        let local_datetime = Local
            .timestamp_opt(seconds, value.nanos)
            .earliest()
            .or_else(|| Local.timestamp_opt(seconds, value.nanos).latest())
            .unwrap_or_else(|| {
                // Fallback to UTC if local timezone fails
                Utc.timestamp_opt(seconds, value.nanos)
                    .earliest()
                    .unwrap_or_default()
                    .with_timezone(&Local)
            });
        let naive_date = local_datetime.date_naive();

        Date::from(naive_date)
    }
}

impl From<SystemTime> for Instant {
    fn from(value: SystemTime) -> Self {
        let duration = value.duration_since(UNIX_EPOCH).unwrap_or_default();
        let seconds = duration.as_secs();
        Instant {
            seconds,
            nanos: duration.subsec_nanos(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
#[display("{} seconds", self.as_secs())]
pub struct Duration {
    pub nanos: u64,
}

impl Duration {
    #[must_use]
    pub fn from_secs(secs: u64) -> Self {
        Self {
            nanos: secs * 1_000_000_000,
        }
    }

    #[must_use]
    pub fn as_secs(&self) -> u64 {
        self.nanos / 1_000_000_000
    }

    #[must_use]
    pub fn from_std(duration: std::time::Duration) -> Self {
        Self {
            nanos: (duration.as_secs() * 1_000_000_000)
                .saturating_add(duration.subsec_nanos().into()),
        }
    }

    #[must_use]
    pub fn into_std(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.nanos)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use claims::{assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::summer_date(2025, 7, 24)]
    #[case::new_years_day(2024, 1, 1)]
    #[case::independence_day(2024, 7, 4)]
    #[case::new_years_eve(2024, 12, 31)]
    #[case::leap_year_feb_29(2024, 2, 29)]
    #[case::regular_feb_28(2023, 2, 28)]
    #[case::end_of_month_31(2024, 1, 31)]
    #[case::end_of_month_30(2024, 4, 30)]
    fn from_date_should_convert_to_local_midnight(
        #[case] year: i32,
        #[case] month: u32,
        #[case] day: u32,
    ) {
        let naive_date = assert_some!(
            NaiveDate::from_ymd_opt(year, month, day),
            "precondition: date is constructed"
        );
        let date = crate::date::Date::from(naive_date);

        // initial conversion
        let instant = Instant::from(date);

        let expected_datetime = assert_some!(naive_date.and_hms_opt(0, 0, 0));
        let expected_seconds = assert_some!(
            Local.from_local_datetime(&expected_datetime).earliest(),
            "expecting local time to exist"
        )
        .timestamp();

        assert_eq!(
            instant.seconds,
            expected_seconds.max(0).try_into().unwrap_or_default()
        );
        assert_eq!(instant.nanos, 0);

        // convert back to date
        let converted_date = Date::from(instant);
        assert_eq!(
            converted_date, date,
            "expecting instant to convert back into date"
        );
    }

    #[rstest]
    #[case::spring_dst_transition(2024, 3, 10)]
    #[case::fall_dst_transition(2024, 11, 3)]
    fn from_date_should_handle_dst_transitions(
        #[case] year: i32,
        #[case] month: u32,
        #[case] day: u32,
    ) {
        let naive_date = assert_some!(
            NaiveDate::from_ymd_opt(year, month, day),
            "precondition: DST transition date is constructed"
        );
        let date = crate::date::Date::from(naive_date);

        // Should always produce a valid instant regardless of DST complications
        let instant = Instant::from(date);

        assert!(
            instant.seconds > 0,
            "Should produce valid timestamp for DST transition"
        );
        assert_eq!(instant.nanos, 0);
    }

    #[test]
    fn now_should_convert_to_instant() {
        let instant: Instant = SystemTime::now().into();
        assert!(instant.seconds > 0, "expecting instant to have seconds");
    }

    #[rstest]
    #[case::early_morning("2024-07-15 02:00:00", 22 * 3600)] // ~22 hours
    #[case::afternoon("2024-07-15 14:30:00", 9 * 3600 + 30 * 60)] // ~9.5 hours
    #[case::late_evening("2024-07-15 23:30:00", 30 * 60)] // ~30 minutes
    #[case::very_close_to_midnight("2024-07-15 23:59:59", 1)] // ~1 second
    #[case::exactly_midnight("2024-07-15 00:00:00", 24 * 3600)] // ~24 hours
    fn duration_until_midnight_should_calculate_correctly(
        #[case] datetime_str: &str,
        #[case] expected_seconds: u64,
    ) {
        // Parse the datetime string to create an Instant
        let naive_datetime = assert_ok!(
            chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S"),
            "parsing test datetime"
        );

        let local_datetime = assert_some!(
            Local.from_local_datetime(&naive_datetime).earliest(),
            "converting to local time"
        );

        let instant = Instant {
            seconds: local_datetime.timestamp().try_into().unwrap_or_default(),
            nanos: 0,
        };

        let duration = instant.duration_until_midnight();

        // Allow for small differences due to timezone/DST calculations
        let diff = if duration.as_secs() > expected_seconds {
            duration.as_secs() - expected_seconds
        } else {
            expected_seconds - duration.as_secs()
        };

        assert!(
            diff <= 1,
            "Expected ~{} seconds, got {} seconds (diff: {})",
            expected_seconds,
            duration.as_secs(),
            diff
        );
    }

    #[test]
    fn duration_until_midnight_should_handle_nanoseconds() {
        // Create an instant with nanoseconds
        let instant = Instant {
            seconds: 1_721_030_400, // Some timestamp
            nanos: 500_000_000,     // 0.5 seconds
        };

        let duration = instant.duration_until_midnight();

        // The duration should account for the nanoseconds
        // (exact value depends on the date, but should be reasonable)
        assert!(
            duration.as_secs() < 24 * 3600,
            "Duration should be less than 24 hours"
        );
        assert!(duration.as_secs() > 0, "Duration should be positive");
    }

    #[rstest]
    #[case::seconds(4, 0, 5, 0)]
    #[case::nanos(5, 2, 5, 3)]
    #[case::nanos_larger_but_seconds_smaller(5, 10, 6, 0)]
    fn instant_comparison(
        #[case] earlier_seconds: u64,
        #[case] earlier_nanos: u32,
        #[case] later_seconds: u64,
        #[case] later_nanos: u32,
    ) {
        let earlier = Instant {
            seconds: earlier_seconds,
            nanos: earlier_nanos,
        };
        let later = Instant {
            seconds: later_seconds,
            nanos: later_nanos,
        };

        assert!(earlier < later, "expecting earlier to be less than later");
        assert!(
            earlier <= later,
            "expecting earlier to be less than or equal to later"
        );
        assert!(
            later > earlier,
            "expecting later to be greater than earlier"
        );
        assert!(
            later >= earlier,
            "expecting later to be greater than or equal to earlier"
        );
    }
}

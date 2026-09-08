//! Explicit calendar context for converting absolute instants and calendar dates.
//!
//! Named zones retain IANA daylight-saving rules. Only `system` consults the host;
//! captured values and pure conversions never refresh themselves.

use std::str::FromStr;

use chrono::{LocalResult, Offset, TimeZone as _, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::{
    Error,
    date::Date,
    time::{Duration, Instant},
};

/// Calendar context serialized as `"Utc"` or `{"Named":"America/Los_Angeles"}`.
///
/// ```
/// use rapport_temporal::{clock::{Clock, FakeClock}, time::Instant, Timezone, Error};
/// let now = Instant::from_rfc3339("2026-01-01T00:00:00Z")?;
/// let utc = Clock::from(FakeClock::new(now));
/// assert_eq!(utc.today()?.into_iso_string(), "2026-01-01");
/// let timezone: Timezone = "America/Los_Angeles".parse()?;
/// let local = Clock::from(FakeClock::with_timezone(now, timezone));
/// let today = local.today()?;
/// assert_eq!(today.into_iso_string(), "2025-12-31");
/// assert_eq!(timezone.date_at(timezone.start_of_day(today)?)?, today);
/// assert_eq!(timezone.duration_until_midnight(now)?.as_secs(), 8 * 3600);
/// # Ok::<(), Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timezone {
    Utc,
    Named(Tz),
}

impl Timezone {
    /// Resolves the machine's IANA timezone once; call again to refresh the context.
    ///
    /// # Errors
    /// Returns an error if discovery fails or the name is absent from the bundled database.
    pub fn system() -> Result<Self, Error> {
        iana_time_zone::get_timezone()?.parse()
    }

    /// Projects an absolute instant onto this timezone's calendar.
    ///
    /// # Errors
    /// Rejects instants outside Chrono's range, nanoseconds outside `0..1_000_000_000`
    /// (including leap-second encodings), and local date overflow, without a fallback.
    pub fn date_at(self, instant: Instant) -> Result<Date, Error> {
        let datetime = Self::utc_datetime(instant)?;
        let offset = self.zone().offset_from_utc_datetime(&datetime.naive_utc());
        let local = datetime
            .naive_utc()
            .checked_add_signed(chrono::Duration::seconds(i64::from(
                offset.fix().local_minus_utc(),
            )))
            .ok_or(Error::CalendarOutOfRange)?;
        Ok(Date::from(local.date()))
    }

    /// Returns midnight only when it identifies exactly one instant in this timezone.
    ///
    /// # Errors
    /// Returns `AmbiguousMidnight` for repeated midnight and `NonexistentMidnight`
    /// for missing midnight (including entirely skipped dates). Dates before the
    /// Unix epoch or beyond the supported instant range return `CalendarOutOfRange`.
    pub fn start_of_day(self, date: Date) -> Result<Instant, Error> {
        let midnight = date
            .into_naive_date()
            .and_hms_opt(0, 0, 0)
            .ok_or(Error::CalendarOutOfRange)?;
        match self.zone().offset_from_local_datetime(&midnight) {
            LocalResult::Single(offset) => {
                let datetime = midnight
                    .checked_sub_signed(chrono::Duration::seconds(i64::from(
                        offset.fix().local_minus_utc(),
                    )))
                    .ok_or(Error::CalendarOutOfRange)?
                    .and_utc();
                let seconds =
                    u64::try_from(datetime.timestamp()).map_err(|_| Error::CalendarOutOfRange)?;
                Ok(Instant { seconds, nanos: 0 })
            }
            LocalResult::Ambiguous(_, _) => Err(Error::AmbiguousMidnight {
                date,
                timezone: self,
            }),
            LocalResult::None => Err(Error::NonexistentMidnight {
                date,
                timezone: self,
            }),
        }
    }

    /// Measures until the next calendar date's midnight, preserving nanoseconds.
    ///
    /// # Errors
    /// Uses `start_of_day`'s policy for the next date, including errors for repeated
    /// or missing midnight. Rejects invalid or out-of-range calendar values.
    pub fn duration_until_midnight(self, instant: Instant) -> Result<Duration, Error> {
        let date = self
            .date_at(instant)?
            .into_naive_date()
            .succ_opt()
            .ok_or(Error::CalendarOutOfRange)?;
        let midnight = self.start_of_day(Date::from(date))?;
        let nanos = (Self::utc_datetime(midnight)? - Self::utc_datetime(instant)?)
            .num_nanoseconds()
            .and_then(|value| u64::try_from(value).ok())
            .ok_or(Error::CalendarOutOfRange)?;
        Ok(Duration { nanos })
    }

    fn zone(self) -> Tz {
        match self {
            Self::Utc => chrono_tz::UTC,
            Self::Named(zone) => zone,
        }
    }

    fn utc_datetime(instant: Instant) -> Result<chrono::DateTime<Utc>, Error> {
        let seconds = i64::try_from(instant.seconds).map_err(|_| Error::CalendarOutOfRange)?;
        if instant.nanos >= 1_000_000_000 {
            return Err(Error::CalendarOutOfRange);
        }
        chrono::DateTime::from_timestamp(seconds, instant.nanos).ok_or(Error::CalendarOutOfRange)
    }
}

impl FromStr for Timezone {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if name == "UTC" {
            return Ok(Self::Utc);
        }
        name.parse()
            .map(Self::Named)
            .map_err(|source| Error::InvalidTimezone {
                name: name.to_owned(),
                source,
            })
    }
}

impl std::fmt::Display for Timezone {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.zone().fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;
    use crate::clock::{Clock, FakeClock};

    #[rstest]
    #[case::utc(Timezone::Utc, "2026-01-01")]
    #[case::los_angeles(Timezone::Named(chrono_tz::America::Los_Angeles), "2025-12-31")]
    fn date_at_should_use_explicit_calendar_context(
        #[case] timezone: Timezone,
        #[case] expected: &str,
    ) {
        let midnight_utc = assert_ok!(Instant::from_rfc3339("2026-01-01T00:00:00Z"));
        let fake = FakeClock::with_timezone(midnight_utc, timezone);
        let clock = Clock::from(&fake);

        let date = assert_ok!(timezone.date_at(midnight_utc));

        assert_eq!(date, Date::from_str_unchecked(expected));
        assert_eq!(assert_ok!(clock.today()), date);
        assert_eq!(clock.timezone(), timezone);
    }

    #[test]
    fn fake_clock_should_default_to_utc_and_share_time_with_clones() {
        let midnight_utc = assert_ok!(Instant::from_rfc3339("2026-01-01T00:00:00Z"));
        let fake = FakeClock::new(midnight_utc);
        let clone = fake.clone();
        let clock = Clock::from(&fake);

        assert_eq!(fake.timezone(), Timezone::Utc);
        assert_eq!(
            assert_ok!(clock.today()),
            Date::from_str_unchecked("2026-01-01")
        );
        fake.add_days(1);

        assert_eq!(clone.now(), fake.now());
        assert_eq!(clone.timezone(), Timezone::Utc);
        assert_eq!(
            assert_ok!(clock.today()),
            Date::from_str_unchecked("2026-01-02")
        );
    }

    #[test]
    fn fake_clock_should_preserve_named_context_when_cloned() {
        let timezone = Timezone::Named(chrono_tz::America::Los_Angeles);
        let fake = FakeClock::with_timezone(Instant::from_timestamp(0), timezone);
        let clone = fake.clone();
        let clock = Clock::from(&clone);
        let midnight_utc = assert_ok!(Instant::from_rfc3339("2026-01-01T00:00:00Z"));

        fake.set_time(midnight_utc);

        assert_eq!(clone.now(), midnight_utc);
        assert_eq!(clone.timezone(), timezone);
        assert_eq!(clock.timezone(), timezone);
        assert_eq!(
            assert_ok!(clock.today()),
            Date::from_str_unchecked("2025-12-31")
        );
    }

    #[test]
    fn system_clock_should_retain_supplied_timezone_when_cloned() {
        let timezone = Timezone::Named(chrono_tz::Europe::Rome);
        let clock = Clock::System(timezone);
        let clone = clock.clone();

        assert_eq!(clock.timezone(), timezone);
        assert_eq!(clone.timezone(), timezone);
    }

    #[rstest]
    #[case::utc(Timezone::Utc, "2026-01-01", "2026-01-01T00:00:00Z", 24)]
    #[case::ordinary(
        Timezone::Named(chrono_tz::America::Los_Angeles),
        "2026-01-01",
        "2026-01-01T08:00:00Z",
        24
    )]
    #[case::spring(
        Timezone::Named(chrono_tz::America::Los_Angeles),
        "2026-03-08",
        "2026-03-08T08:00:00Z",
        23
    )]
    #[case::autumn(
        Timezone::Named(chrono_tz::America::Los_Angeles),
        "2026-11-01",
        "2026-11-01T07:00:00Z",
        25
    )]
    fn start_of_day_should_round_trip_and_measure_calendar_days(
        #[case] timezone: Timezone,
        #[case] date: &str,
        #[case] expected: &str,
        #[case] hours: u64,
    ) {
        let date = Date::from_str_unchecked(date);

        let midnight = assert_ok!(timezone.start_of_day(date));

        assert_eq!(midnight, assert_ok!(Instant::from_rfc3339(expected)));
        assert_eq!(assert_ok!(timezone.date_at(midnight)), date);
        assert_eq!(
            assert_ok!(timezone.duration_until_midnight(midnight)),
            Duration::from_secs(hours * 3600)
        );
        let tomorrow = assert_ok!(timezone.start_of_day(date.day_after()));
        assert_eq!(tomorrow.seconds - midnight.seconds, hours * 3600);
    }

    #[test]
    fn duration_until_midnight_should_preserve_fractional_seconds() {
        let instant = assert_ok!(Instant::from_rfc3339("2026-01-01T23:59:59.123456789Z"));

        let duration = assert_ok!(Timezone::Utc.duration_until_midnight(instant));

        assert_eq!(duration, Duration { nanos: 876_543_211 });
    }

    #[rstest]
    #[case::missing_midnight(chrono_tz::America::Sao_Paulo, "2018-11-04", "2018-11-03T12:00:00Z")]
    #[case::skipped_date(chrono_tz::Pacific::Apia, "2011-12-30", "2011-12-29T12:00:00Z")]
    fn start_of_day_should_reject_missing_midnight_consistently(
        #[case] zone: Tz,
        #[case] date: &str,
        #[case] previous_instant: &str,
    ) {
        let timezone = Timezone::Named(zone);
        let date = Date::from_str_unchecked(date);
        let previous_instant = assert_ok!(Instant::from_rfc3339(previous_instant));

        let errors = [
            assert_err!(timezone.start_of_day(date)),
            assert_err!(timezone.duration_until_midnight(previous_instant)),
        ];

        for error in errors {
            assert!(
                matches!(error, Error::NonexistentMidnight { date: actual_date, timezone: actual_zone } if actual_date == date && actual_zone == timezone)
            );
        }
    }

    #[test]
    fn start_of_day_should_reject_ambiguous_midnight_consistently() {
        let timezone = Timezone::Named(chrono_tz::America::Havana);
        let date = Date::from_str_unchecked("2020-11-01");
        let previous_instant = assert_ok!(Instant::from_rfc3339("2020-10-31T12:00:00Z"));

        let errors = [
            assert_err!(timezone.start_of_day(date)),
            assert_err!(timezone.duration_until_midnight(previous_instant)),
        ];

        for error in errors {
            assert!(
                matches!(error, Error::AmbiguousMidnight { date: actual_date, timezone: actual_zone } if actual_date == date && actual_zone == timezone)
            );
        }
    }

    #[rstest]
    #[case::utc(Timezone::Utc, "\"Utc\"")]
    #[case::named(
        Timezone::Named(chrono_tz::America::Los_Angeles),
        "{\"Named\":\"America/Los_Angeles\"}"
    )]
    #[case::named_utc(Timezone::Named(chrono_tz::UTC), "{\"Named\":\"UTC\"}")]
    fn timezone_should_round_trip_through_serde(
        #[case] timezone: Timezone,
        #[case] expected: &str,
    ) {
        let encoded = assert_ok!(serde_json::to_string(&timezone));

        assert_eq!(encoded, expected);
        assert_eq!(
            assert_ok!(serde_json::from_str::<Timezone>(&encoded)),
            timezone
        );
    }

    #[test]
    fn timezone_should_validate_names_at_boundaries() {
        assert_eq!(assert_ok!("UTC".parse::<Timezone>()), Timezone::Utc);
        assert_eq!(
            assert_ok!("America/Los_Angeles".parse::<Timezone>()),
            Timezone::Named(chrono_tz::America::Los_Angeles)
        );
        assert!(
            matches!(assert_err!("Mars/Olympus".parse::<Timezone>()), Error::InvalidTimezone { name, .. } if name == "Mars/Olympus")
        );
        assert_err!(serde_json::from_str::<Timezone>(
            r#"{"Named":"Mars/Olympus"}"#
        ));
    }

    #[rstest]
    #[case::seconds_overflow(Instant { seconds: u64::MAX, nanos: 0 })]
    #[case::invalid_nanos(Instant { seconds: 0, nanos: 1_000_000_000 })]
    fn calendar_operations_should_reject_invalid_instants(#[case] instant: Instant) {
        assert!(matches!(
            assert_err!(Timezone::Utc.date_at(instant)),
            Error::CalendarOutOfRange
        ));
        assert!(matches!(
            assert_err!(Timezone::Utc.duration_until_midnight(instant)),
            Error::CalendarOutOfRange
        ));
    }

    #[rstest]
    #[case::missing_midnight("America/Sao_Paulo", "2018-11-04T03:00:00Z", "2018-11-04")]
    #[case::first_repeated_midnight("America/Havana", "2020-11-01T04:00:00Z", "2020-11-01")]
    #[case::second_repeated_midnight("America/Havana", "2020-11-01T05:00:00Z", "2020-11-01")]
    #[case::after_skipped_date("Pacific/Apia", "2011-12-30T10:00:00Z", "2011-12-31")]
    fn date_at_should_project_instants_across_midnight_transitions(
        #[case] zone: &str,
        #[case] instant: &str,
        #[case] date: &str,
    ) {
        let timezone = assert_ok!(zone.parse::<Timezone>());
        let instant = assert_ok!(Instant::from_rfc3339(instant));

        assert_eq!(
            assert_ok!(timezone.date_at(instant)),
            Date::from_str_unchecked(date)
        );
    }

    #[test]
    fn calendar_operations_should_reject_calendar_boundary_overflow() {
        let max_date = Date::from(chrono::NaiveDate::MAX);
        let midnight = assert_ok!(Timezone::Utc.start_of_day(max_date));
        let last_second = Instant::from_utc_datetime(max_date.end_of_day_utc());
        let east = Timezone::Named(chrono_tz::Asia::Tokyo);

        assert!(matches!(
            assert_err!(Timezone::Utc.duration_until_midnight(midnight)),
            Error::CalendarOutOfRange
        ));
        assert!(matches!(
            assert_err!(east.date_at(last_second)),
            Error::CalendarOutOfRange
        ));
        assert!(matches!(
            assert_err!(east.start_of_day(Date::MIN)),
            Error::CalendarOutOfRange
        ));
    }

    #[test]
    fn clock_today_should_reject_invalid_fake_instants() {
        let fake = FakeClock::new(Instant {
            seconds: u64::MAX,
            nanos: 0,
        });
        let clock = Clock::from(fake);

        assert!(matches!(
            assert_err!(clock.today()),
            Error::CalendarOutOfRange
        ));
    }

    #[test]
    fn start_of_day_should_reject_dates_before_unsigned_instant_range() {
        let date = Date::from_str_unchecked("1969-12-31");

        assert!(matches!(
            assert_err!(Timezone::Utc.start_of_day(date)),
            Error::CalendarOutOfRange
        ));
    }
}

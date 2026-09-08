//! Typed failures for temporal parsing and calendar operations.

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
    #[error("invalid IANA timezone: {name}")]
    InvalidTimezone {
        name: String,
        #[source]
        source: chrono_tz::ParseError,
    },
    #[error("unable to resolve the machine timezone")]
    SystemTimezone(#[from] iana_time_zone::GetTimezoneError),
    #[error("midnight is ambiguous for {date} in {timezone}")]
    AmbiguousMidnight {
        date: crate::date::Date,
        timezone: crate::Timezone,
    },
    #[error("midnight does not exist for {date} in {timezone}")]
    NonexistentMidnight {
        date: crate::date::Date,
        timezone: crate::Timezone,
    },
    #[error("calendar value is outside the supported range")]
    CalendarOutOfRange,
}

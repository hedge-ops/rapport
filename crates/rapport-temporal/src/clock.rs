//! For controlling time in programs, to make them testable over time periods.

use chrono::Utc;
use parking_lot::RwLock;
use std::sync::Arc;

use crate::{Error, Timezone, date::Date, time::Instant};

#[derive(Debug, Clone, derive_more::Display)]
pub enum Clock {
    #[display("system time")]
    System(Timezone),
    #[display("{_0}")]
    Fake(FakeClock),
}

impl Clock {
    /// Captures the machine timezone; construct a new clock to refresh it.
    /// Use [`Self::system_utc`] for an infallible system clock with a UTC calendar.
    ///
    /// ```no_run
    /// use rapport_temporal::{clock::Clock, Error};
    /// let clock = Clock::system()?;
    /// let timezone = clock.timezone();
    /// let today = timezone.date_at(clock.now())?;
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    /// Returns an error if the machine zone cannot be resolved or validated.
    pub fn system() -> Result<Self, Error> {
        Timezone::system().map(Self::System)
    }

    /// Creates a real advancing system clock whose calendar calculations use UTC.
    ///
    /// Unlike [`Self::system`], this is infallible and does not resolve or modify
    /// the machine timezone or process environment. Unlike [`FakeClock::new`],
    /// time advances with the system clock rather than through explicit updates.
    #[must_use]
    pub const fn system_utc() -> Self {
        Self::System(Timezone::Utc)
    }

    /// Returns the captured calendar context, suitable for passing to a pure core.
    #[must_use]
    pub fn timezone(&self) -> Timezone {
        match self {
            Self::System(timezone) => *timezone,
            Self::Fake(clock) => clock.timezone(),
        }
    }

    #[must_use]
    pub fn now(&self) -> Instant {
        match self {
            Clock::System(_) => Instant::from_utc_datetime(Utc::now()),
            Clock::Fake(provider) => provider.now(),
        }
    }

    /// Projects the current instant using this clock's captured timezone.
    ///
    /// # Errors
    /// Returns an error when the instant or local date is outside the supported range.
    pub fn today(&self) -> Result<Date, Error> {
        self.timezone().date_at(self.now())
    }
}

/// A clock with shared mutable time and immutable calendar context for testing.
#[derive(Debug, Clone, derive_more::Display)]
#[display("fake time (for testing), current time: {}", time.read().to_string())]
pub struct FakeClock {
    time: Arc<RwLock<Instant>>,
    timezone: Timezone,
}

impl Default for FakeClock {
    fn default() -> Self {
        // use the system time as a default, in order to not force clients to deal with time
        Self::new(Instant::from_utc_datetime(Utc::now()))
    }
}

impl FakeClock {
    /// Creates a fake whose calendar calculations use UTC, independent of the host.
    #[must_use]
    pub fn new(time: Instant) -> Self {
        Self::with_timezone(time, Timezone::Utc)
    }

    /// Creates a fake with immutable calendar context and shared mutable time.
    #[must_use]
    pub fn with_timezone(time: Instant, timezone: Timezone) -> Self {
        Self {
            time: Arc::new(RwLock::new(time)),
            timezone,
        }
    }

    #[must_use]
    pub fn timezone(&self) -> Timezone {
        self.timezone
    }

    pub fn add_days(&self, value: u32) {
        let now = self.now();
        let add_seconds = value * 24 * 60 * 60;
        let added = Instant {
            seconds: now.seconds.saturating_add(add_seconds.into()),
            nanos: now.nanos,
        };
        self.set_time(added);
    }

    pub fn set_time(&self, time: Instant) {
        *self.time.write() = time;
    }

    #[must_use]
    pub fn now(&self) -> Instant {
        *self.time.read()
    }
}

impl From<FakeClock> for Clock {
    fn from(value: FakeClock) -> Self {
        Self::Fake(value)
    }
}

impl From<&FakeClock> for Clock {
    fn from(value: &FakeClock) -> Self {
        value.clone().into()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn system_utc_should_select_the_system_clock_with_utc_calendar() {
        const CLOCK: Clock = Clock::system_utc();

        assert!(
            matches!(CLOCK, Clock::System(Timezone::Utc)),
            "expecting a system clock with a UTC calendar"
        );
    }

    #[test]
    fn clock_today_should_use_the_current_instant() {
        let fake = FakeClock::new(Instant::from_timestamp(1_759_226_820));
        let clock = Clock::from(&fake);

        let actual = claims::assert_ok!(clock.today());

        assert_eq!(actual, Date::from_str_unchecked("2025-09-30"));
    }

    #[test]
    fn fake_clock_add_days_should_advance_by_whole_days() {
        let fake = FakeClock::new(Instant::from_timestamp(1_759_226_820));

        fake.add_days(2);

        assert_eq!(
            fake.now(),
            crate::time::Instant::from_utc_datetime(claims::assert_some!(
                chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2025, 10, 2, 10, 7, 0).single()
            ))
        );
    }

    #[test]
    fn fake_clock_set_time_should_replace_shared_time() {
        let fake = FakeClock::new(Instant::from_timestamp(1_759_226_820));
        let cloned = fake.clone();

        fake.set_time(Instant::from_timestamp(1_764_497_220));

        assert_eq!(
            cloned.now(),
            crate::time::Instant::from_utc_datetime(claims::assert_some!(
                chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2025, 11, 30, 10, 7, 0).single()
            ))
        );
    }
}

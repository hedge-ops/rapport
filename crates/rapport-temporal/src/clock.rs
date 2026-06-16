//! For controlling time in programs, to make them testable over time periods.

use chrono::Utc;
use parking_lot::RwLock;
use std::sync::Arc;

use crate::{date::Date, time::Instant};

#[derive(Debug, Clone, derive_more::Display)]
pub enum Clock {
    #[display("system time")]
    System,
    #[display("{_0}")]
    Fake(FakeClock),
}

impl Clock {
    #[must_use]
    pub fn now(&self) -> Instant {
        match self {
            Clock::System => Instant::from_utc_datetime(Utc::now()),
            Clock::Fake(provider) => provider.now(),
        }
    }

    #[must_use]
    pub fn today(&self) -> Date {
        self.now().into_date()
    }
}

/// A substute for the real clock for testing purposes, can be cloned but still refer to the same
/// shared time, which can be updated centrally from tests.
#[derive(Debug, Clone, derive_more::Display)]
#[display("fake time (for testing), current time: {}", time.read().to_string())]
pub struct FakeClock {
    time: Arc<RwLock<Instant>>,
}

impl Default for FakeClock {
    fn default() -> Self {
        // use the system time as a default, in order to not force clients to deal with time
        Self::new(Instant::from_utc_datetime(Utc::now()))
    }
}

impl FakeClock {
    #[must_use]
    pub fn new(time: Instant) -> Self {
        Self {
            time: Arc::new(RwLock::new(time)),
        }
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
    fn clock_today_should_use_the_current_instant() {
        let fake = FakeClock::new(Instant::from_timestamp(1_759_226_820));
        let clock = Clock::from(&fake);

        let actual = clock.today();

        assert_eq!(actual.into_iso_string(), "2025-09-30");
    }

    #[test]
    fn fake_clock_add_days_should_advance_by_whole_days() {
        let fake = FakeClock::new(Instant::from_timestamp(1_759_226_820));

        fake.add_days(2);

        assert_eq!(fake.now().to_string(), "2025-10-02 10:07:00 UTC");
    }

    #[test]
    fn fake_clock_set_time_should_replace_shared_time() {
        let fake = FakeClock::new(Instant::from_timestamp(1_759_226_820));
        let cloned = fake.clone();

        fake.set_time(Instant::from_timestamp(1_764_497_220));

        assert_eq!(cloned.now().to_string(), "2025-11-30 10:07:00 UTC");
    }
}

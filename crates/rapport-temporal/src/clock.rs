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

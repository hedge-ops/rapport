use crate::date::Date;

use super::{Interval, Schedule, TemporalUnit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyRecurrenceSchedule {
    interval: Interval,
}

impl DailyRecurrenceSchedule {
    #[must_use]
    pub fn new(interval: impl Into<Interval>) -> Self {
        Self {
            interval: interval.into(),
        }
    }
    pub(super) fn daily() -> Self {
        DailyRecurrenceSchedule {
            interval: Interval::one(),
        }
    }
}

impl Schedule for DailyRecurrenceSchedule {
    fn interval(&self) -> Interval {
        self.interval
    }

    fn next_occurrence_after(&self, from: Date) -> Date {
        from.add_interval_days(self.interval)
    }

    fn into_string(self, _: Date) -> String {
        TemporalUnit::Day.print_interval(self.interval)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::DailyRecurrenceSchedule;
    use crate::recurrence::Interval;

    #[rstest]
    #[case::daily("2025-06-05", Interval::one(), "2025-06-06")]
    #[case::every_three_days("2025-06-05", Interval::three(), "2025-06-08")]
    fn next_occurrence_should_calculate_daily(
        #[case] from: &str,
        #[case] interval: Interval,
        #[case] expected: &str,
    ) {
        use crate::{date::Date, recurrence::Schedule};

        let recurrence = DailyRecurrenceSchedule { interval };

        let from = Date::from_str_unchecked(from);
        let expected_next = Date::from_str_unchecked(expected);

        let actual_next = recurrence.next_occurrence_after(from);

        assert_eq!(actual_next, expected_next);
    }
}

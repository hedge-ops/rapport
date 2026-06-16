use nonempty::NonEmpty;
use strum::EnumCount;

use crate::date::{Date, Weekday};

use super::{Interval, Schedule, TemporalUnit};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeeklyRecurrenceSchedule {
    interval: Interval,
    days: NonEmpty<Weekday>,
}

impl WeeklyRecurrenceSchedule {
    pub fn new(interval: Interval, days: impl Into<NonEmpty<Weekday>>) -> Self {
        let days = days.into();
        Self { interval, days }
    }

    #[must_use]
    pub fn for_weekday(interval: Interval, day: Weekday) -> Self {
        Self::new(interval, NonEmpty::singleton(day))
    }

    /// Creates a schedule that is every two weeks from the starting date.
    ///
    /// # Panics
    ///
    /// Should not panic; panic is used when the internal hard coded value does not fit the invariants.
    #[allow(clippy::expect_used)]
    #[must_use]
    pub fn every_two_weeks(starting: Date) -> Self {
        Self::new(Interval::two(), starting.weekday())
    }

    #[must_use]
    pub fn weekly_on(day: Weekday) -> Self {
        Self::for_weekday(Interval::one(), day)
    }

    pub(crate) fn weekly_on_same_day_as(date: Date) -> WeeklyRecurrenceSchedule {
        Self::weekly_on(date.weekday())
    }

    /// Toggles the weekday to be included in the schedule if it's not included, and excluded if it
    /// is already included. If the given weekday is the last weekday in the schedule, nothing will
    /// hapen.
    #[must_use]
    pub fn with_weekday_toggled(self, weekday: Weekday) -> Self {
        let Self { interval, days } = self;
        let days = super::toggle_value(days, weekday);
        Self { interval, days }
    }
}

impl Schedule for WeeklyRecurrenceSchedule {
    fn interval(&self) -> Interval {
        self.interval
    }

    fn next_occurrence_after(&self, from: Date) -> Date {
        // count next as one interval
        let next = Weekday::next_occurrence_after_days(&self.days, from);
        // remaining intervals
        let remaining_intervals = self.interval.minus_one();
        // remaining days
        let remaining_days = Weekday::COUNT.saturating_mul(remaining_intervals.into());
        next.add_days(remaining_days)
    }

    fn into_string(self, starting: Date) -> String {
        let WeeklyRecurrenceSchedule { interval, days } = self;
        let printed = TemporalUnit::Week.print_interval(interval);
        if days.len() == 1 && days.head == starting.weekday() {
            printed
        } else {
            tracing::debug!(
                "writing out days for days: {} and starting weekday: {}",
                days.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                starting.weekday()
            );
            let days_description = super::print_elements(&days);
            format!(
                "{printed} {} {days_description}",
                super::parser::GrammarToken::On
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    use crate::date::Weekday;
    use claims::assert_some;
    use nonempty::NonEmpty;

    #[test]
    fn interval_should_return_configured_interval() {
        let schedule = WeeklyRecurrenceSchedule::new(Interval::two(), Weekday::Monday);

        let actual = schedule.interval();

        assert_eq!(actual, Interval::two());
    }

    #[rstest]
    #[case::weekly("2025-06-07", Interval::one(), &[Weekday::Saturday], "2025-06-14")]
    #[case::different_day("2025-06-07", Interval::one(), &[Weekday::Tuesday], "2025-06-10")]
    #[case::every_two_weeks("2025-06-07", Interval::two(), &[Weekday::Saturday], "2025-06-21")]
    #[case::every_two_weeks_different_day("2025-06-07", Interval::two(), &[Weekday::Tuesday], "2025-06-17")]
    #[case::multiple_days("2025-06-07", Interval::one(), &[Weekday::Monday, Weekday::Tuesday], "2025-06-09")]
    fn next_occurrence_should_calculate_weekly(
        #[case] from: &str,
        #[case] interval: Interval,
        #[case] days: &[Weekday],
        #[case] expected: &str,
    ) {
        let recurrence =
            WeeklyRecurrenceSchedule::new(interval, assert_some!(NonEmpty::from_slice(days)));

        let from = Date::from_str_unchecked(from);
        let expected_next = Date::from_str_unchecked(expected);

        let actual_next = recurrence.next_occurrence_after(from);

        assert_eq!(actual_next, expected_next);
    }

    #[rstest]
    #[case::weekly_on_single_day("2025-06-07", Interval::one(), &[Weekday::Tuesday], "2025-06-10")]
    #[case::every_two_weeks("2025-06-07", Interval::two(), &[Weekday::Tuesday], "2025-06-17")]
    #[case::multiple_days_this_week("2025-06-07",Interval::one(), &[Weekday::Monday, Weekday::Tuesday], "2025-06-09")]
    #[case::multiple_days_next_week("2025-06-07",Interval::one(), &[Weekday::Friday, Weekday::Thursday], "2025-06-12")]
    #[case::multiple_days_every_two_weeks("2025-06-07",Interval::two(), &[Weekday::Friday, Weekday::Thursday], "2025-06-19")]
    #[case::same_day_included("2025-06-07", Interval::one(),&[Weekday::Saturday, Weekday::Monday], "2025-06-09")]
    #[case::all_next_week("2025-06-07", Interval::one(), &[Weekday::Wednesday, Weekday::Thursday, Weekday::Friday], "2025-06-11")]
    #[case::next_week_is_next_month("2025-06-30", Interval::one(), &[Weekday::Monday], "2025-07-07")]
    #[case::next_week_isnext_year("2025-12-30", Interval::one(), &[Weekday::Monday], "2026-01-05")]
    fn weekly_next_occurrence_after(
        #[case] from: &str,
        #[case] interval: Interval,
        #[case] days: &[Weekday],
        #[case] expected: &str,
    ) {
        let from = Date::from_str_unchecked(from);
        let expected = Date::from_str_unchecked(expected);
        let days = assert_some!(NonEmpty::from_slice(days));

        let recurrence = WeeklyRecurrenceSchedule::new(interval, days);

        let actual = recurrence.next_occurrence_after(from);

        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::weekly(
        WeeklyRecurrenceSchedule::weekly_on(Weekday::Monday),
        "2025-06-09",
        "weekly"
    )]
    #[case::every_three_weeks(
        WeeklyRecurrenceSchedule::new(Interval::three(), NonEmpty::singleton(Weekday::Monday)),
        "2025-06-09",
        "every 3 weeks"
    )]
    #[case::every_three_weeks_on_another_day(
        WeeklyRecurrenceSchedule::weekly_on(Weekday::Tuesday),
        "2025-06-09",
        "weekly on Tuesday"
    )]
    #[case::every_two_weeks(
        WeeklyRecurrenceSchedule::new(Interval::two(), Weekday::Monday),
        "2025-06-09",
        "every 2 weeks"
    )]
    #[case::every_two_weeks_on_another_day(
        WeeklyRecurrenceSchedule::new(Interval::two(), Weekday::Wednesday),
        "2025-06-09",
        "every 2 weeks on Wednesday"
    )]
    fn test_into_string(
        #[case] schedule: WeeklyRecurrenceSchedule,
        #[case] starting: &str,
        #[case] expected: &str,
    ) {
        let starting = Date::from_str_unchecked(starting);

        let actual = schedule.into_string(starting);

        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::toggle_on(&[Weekday::Monday], Weekday::Tuesday, &[Weekday::Monday, Weekday::Tuesday])]
    #[case::toggle_on_from_multiple_days(&[Weekday::Monday, Weekday::Tuesday], Weekday::Wednesday, &[Weekday::Monday, Weekday::Tuesday, Weekday::Wednesday])]
    #[case::toggle_off(&[Weekday::Monday, Weekday::Tuesday], Weekday::Tuesday, &[Weekday::Monday])]
    #[case::toggle_does_nothing_for_singleton(&[Weekday::Monday], Weekday::Monday, &[Weekday::Monday])]
    fn test_toggle_weekday(
        #[case] from: &[Weekday],
        #[case] toggle: Weekday,
        #[case] expected: &[Weekday],
    ) {
        let from = assert_some!(NonEmpty::from_slice(from));
        let expected = assert_some!(NonEmpty::from_slice(expected));

        let schedule = WeeklyRecurrenceSchedule::new(Interval::one(), from);
        let schedule = schedule.with_weekday_toggled(toggle);

        let actual = schedule.days;

        assert_eq!(
            actual, expected,
            "expecting toggle to have the expected effect"
        );
    }
}

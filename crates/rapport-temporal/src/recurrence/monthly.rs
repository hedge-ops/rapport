use nonempty::NonEmpty;

use crate::date::{Date, DayOfMonth};

use super::{DaySpecification, Interval, Ordinal, Schedule, TemporalUnit};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthlyRecurrenceSchedule {
    interval: Interval,
    matching: MonthlySpecification,
}

impl MonthlyRecurrenceSchedule {
    #[must_use]
    pub fn new(interval: Interval, matching: MonthlySpecification) -> Self {
        Self { interval, matching }
    }

    #[must_use]
    pub fn take(self) -> (Interval, MonthlySpecification) {
        (self.interval, self.matching)
    }

    pub(crate) fn monthly(starting: Date) -> MonthlyRecurrenceSchedule {
        Self::monthly_each(starting.day_of_month())
    }

    pub fn monthly_each(days: impl Into<NonEmpty<DayOfMonth>>) -> Self {
        Self::each(Interval::one(), days)
    }

    pub(super) fn each(interval: Interval, days: impl Into<NonEmpty<DayOfMonth>>) -> Self {
        Self::new(interval, MonthlySpecification::Each(days.into()))
    }

    pub(crate) fn quarterly(starting: Date) -> MonthlyRecurrenceSchedule {
        Self::each(Interval::three(), starting.day_of_month())
    }
}

impl Schedule for MonthlyRecurrenceSchedule {
    fn next_occurrence_after(&self, from: Date) -> Date {
        std::iter::successors(Some(from), |&date| {
            Some(self.matching.next_occurrence_after(date))
        })
        .nth(self.interval.get().into())
        .unwrap_or_default()
    }

    fn into_string(self, starting: Date) -> String {
        let Self { interval, matching } = self;
        let printed = TemporalUnit::Month.print_interval(interval);
        match matching {
            MonthlySpecification::Each(days) => {
                if days.len() == 1 && days.head == starting.day_of_month() {
                    printed
                } else {
                    let days_description = super::print_elements(&days);
                    format!(
                        "{printed} {} {} {days_description}",
                        super::GrammarToken::On,
                        super::GrammarToken::The
                    )
                }
            }
            MonthlySpecification::OnThe(ordinal) => {
                let ordinal_description = ordinal.to_string();
                format!(
                    "{printed} {} {} {ordinal_description}",
                    super::GrammarToken::On,
                    super::GrammarToken::The
                )
            }
        }
    }

    fn interval(&self) -> Interval {
        self.interval
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonthlySpecification {
    Each(NonEmpty<DayOfMonth>),
    OnThe(OrdinalMonthlyRecurrence),
}

impl MonthlySpecification {
    pub fn each(days: impl Into<NonEmpty<DayOfMonth>>) -> Self {
        let days = days.into();
        Self::Each(days)
    }

    fn next_occurrence_after(&self, value: Date) -> Date {
        match self {
            MonthlySpecification::Each(days) => {
                next_occurrence_within_month_inclusive(value.day_after(), days).unwrap_or_else(
                    || {
                        next_occurrence_within_month_inclusive(value.first_of_next_month(), days)
                            .unwrap_or_else(|| value.last_day_of_next_month())
                    },
                )
            }
            MonthlySpecification::OnThe(recurrence) => recurrence.next_occurrence_after(value),
        }
    }

    #[must_use]
    pub fn into_each(self) -> Option<NonEmpty<DayOfMonth>> {
        match self {
            MonthlySpecification::Each(value) => Some(value),
            MonthlySpecification::OnThe(_) => None,
        }
    }

    #[must_use]
    pub fn into_on_the(self) -> Option<OrdinalMonthlyRecurrence> {
        match self {
            MonthlySpecification::OnThe(value) => Some(value),
            MonthlySpecification::Each(_) => None,
        }
    }
}

fn next_occurrence_within_month_inclusive(date: Date, days: &NonEmpty<DayOfMonth>) -> Option<Date> {
    let current_day = date.day_of_month().to_value();
    tracing::debug!(
        "evaluting next occurrence inclusive of {} for days {}",
        date.into_iso_string(),
        days.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    days.iter()
        .filter(|d| d.to_value() >= current_day)
        .min()
        .and_then(|d| date.with_day(*d))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, derive_more::Display)]
#[display("{ordinal} {day}")]
pub struct OrdinalMonthlyRecurrence {
    pub ordinal: Ordinal,
    pub day: DaySpecification,
}

impl OrdinalMonthlyRecurrence {
    #[must_use]
    pub fn new(ordinal: Ordinal, day: DaySpecification) -> Self {
        Self { ordinal, day }
    }

    fn next_occurrence_after(self, value: Date) -> Date {
        let mut current_month = value.first_of_month();
        for _ in 0..6 {
            // the fifth of any pattern will be found within 6 months
            if let Some(occurrence) = self.find_occurrence_in_month(current_month)
                && occurrence > value
            {
                return occurrence;
            }
            current_month = current_month.first_of_next_month();
        }
        // fallback
        tracing::error!(
            "next occurrence after {} could not be calculated, falling back to given date",
            value.into_iso_string()
        );
        value
    }

    pub(super) fn find_occurrence_in_month(self, first_day_of_month: Date) -> Option<Date> {
        let mut matching_days = Vec::new();
        let year = first_day_of_month.year();
        let month = first_day_of_month.month();
        // Check each possible day in the month
        for day in DayOfMonth::range() {
            if let Some(date) = Date::from_ymd_opt(year, month.to_month_number(), day)
                && self.day.is_satisfied_by(date)
            {
                matching_days.push(date);
            }
        }

        tracing::debug!(
            "{self} matching days for {}: {}",
            first_day_of_month.into_iso_string(),
            matching_days
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );

        // Select the appropriate occurrence based on the ordinal
        match self.ordinal {
            Ordinal::First => matching_days.first().copied(),
            Ordinal::Second => matching_days.get(1).copied(),
            Ordinal::Third => matching_days.get(2).copied(),
            Ordinal::Fourth => matching_days.get(3).copied(),
            Ordinal::Fifth => matching_days.get(4).copied(),
            Ordinal::NextToLast => matching_days.iter().rev().nth(1).copied(),
            Ordinal::Last => matching_days.last().copied(),
        }
    }

    #[must_use]
    pub fn with_another_ordinal(self, ordinal: Ordinal) -> Self {
        let Self { ordinal: _, day } = self;
        Self { ordinal, day }
    }

    #[must_use]
    pub fn with_another_day_specification(self, day: DaySpecification) -> Self {
        let Self { ordinal, day: _ } = self;
        Self { ordinal, day }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use tracing_test::traced_test;

    use super::MonthlyRecurrenceSchedule;
    use crate::recurrence::MonthlySpecification;
    use crate::{
        date::{DayOfMonth, Weekday},
        recurrence::{DaySpecification, Interval, Ordinal},
    };
    use claims::assert_some;
    use nonempty::NonEmpty;

    #[traced_test]
    #[rstest]
    // First ordinal tests
    #[case::first_specific_sunday(
        "2025-06-02",
        Ordinal::First,
        DaySpecification::Specific(Weekday::Sunday),
        "2025-07-06"
    )]
    #[case::first_any("2025-06-02", Ordinal::First, DaySpecification::Any, "2025-07-01")]
    #[case::first_weekday("2025-06-02", Ordinal::First, DaySpecification::Weekday, "2025-07-01")]
    #[case::first_weekend("2025-06-02", Ordinal::First, DaySpecification::Weekend, "2025-07-05")]
    // Second ordinal tests
    #[case::second_specific_sunday(
        "2025-06-02",
        Ordinal::Second,
        DaySpecification::Specific(Weekday::Sunday),
        "2025-06-08"
    )]
    #[case::second_any("2025-06-02", Ordinal::Second, DaySpecification::Any, "2025-07-02")]
    #[case::second_weekday_should_be_first_weekday_available_that_is_second(
        "2025-06-02",
        Ordinal::Second,
        DaySpecification::Weekday,
        "2025-06-03"
    )]
    #[case::second_weekend_should_be_first_weekend_available_that_is_second(
        "2025-06-02",
        Ordinal::Second,
        DaySpecification::Weekend,
        "2025-06-07"
    )]
    // Third ordinal tests
    #[case::third_specific_sunday(
        "2025-06-02",
        Ordinal::Third,
        DaySpecification::Specific(Weekday::Sunday),
        "2025-06-15"
    )]
    #[case::third_any("2025-06-02", Ordinal::Third, DaySpecification::Any, "2025-06-03")]
    #[case::third_weekday("2025-06-02", Ordinal::Third, DaySpecification::Weekday, "2025-06-04")]
    #[case::third_weekend("2025-06-02", Ordinal::Third, DaySpecification::Weekend, "2025-06-08")]
    // Fourth ordinal tests
    #[case::fourth_specific_sunday(
        "2025-06-02",
        Ordinal::Fourth,
        DaySpecification::Specific(Weekday::Sunday),
        "2025-06-22"
    )]
    #[case::fourth_any("2025-06-02", Ordinal::Fourth, DaySpecification::Any, "2025-06-04")]
    #[case::fourth_weekday("2025-06-02", Ordinal::Fourth, DaySpecification::Weekday, "2025-06-05")]
    #[case::fourth_weekend("2025-06-02", Ordinal::Fourth, DaySpecification::Weekend, "2025-06-14")]
    // Fifth ordinal tests (note: no 5th Sunday in July 2025)
    #[case::fifth_specific_monday(
        "2025-06-02",
        Ordinal::Fifth,
        DaySpecification::Specific(Weekday::Monday),
        "2025-06-30"
    )]
    #[case::fifth_any("2025-06-02", Ordinal::Fifth, DaySpecification::Any, "2025-06-05")]
    #[case::fifth_weekday("2025-06-02", Ordinal::Fifth, DaySpecification::Weekday, "2025-06-06")]
    #[case::fifth_weekend("2025-06-02", Ordinal::Fifth, DaySpecification::Weekend, "2025-06-15")]
    // NextToLast ordinal tests
    #[case::next_to_last_specific_sunday(
        "2025-06-02",
        Ordinal::NextToLast,
        DaySpecification::Specific(Weekday::Sunday),
        "2025-06-22"
    )]
    #[case::next_to_last_any(
        "2025-06-02",
        Ordinal::NextToLast,
        DaySpecification::Any,
        "2025-06-29"
    )]
    #[case::next_to_last_weekday(
        "2025-06-02",
        Ordinal::NextToLast,
        DaySpecification::Weekday,
        "2025-06-27"
    )]
    #[case::next_to_last_weekend(
        "2025-06-02",
        Ordinal::NextToLast,
        DaySpecification::Weekend,
        "2025-06-28"
    )]
    // Last ordinal tests
    #[case::last_specific_sunday(
        "2025-06-02",
        Ordinal::Last,
        DaySpecification::Specific(Weekday::Sunday),
        "2025-06-29"
    )]
    #[case::last_any("2025-06-02", Ordinal::Last, DaySpecification::Any, "2025-06-30")]
    #[case::last_weekday("2025-06-02", Ordinal::Last, DaySpecification::Weekday, "2025-06-30")]
    #[case::last_weekend("2025-06-02", Ordinal::Last, DaySpecification::Weekend, "2025-06-29")]
    #[case::skip_when_fifth_day_does_not_exist(
        "2025-06-02",
        Ordinal::Fifth,
        DaySpecification::Specific(Weekday::Tuesday),
        "2025-07-29"
    )]
    #[case::extreme_fifth_use_case(
        "2025-01-31",
        Ordinal::Fifth,
        DaySpecification::Specific(Weekday::Wednesday),
        "2025-04-30"
    )]
    fn monthly_next_occurrence_ordinal(
        #[case] from: &str,
        #[case] ordinal: Ordinal,
        #[case] day: DaySpecification,
        #[case] expected: &str,
    ) {
        let from = Date::from_str_unchecked(from);
        let expected = Date::from_str_unchecked(expected);

        let recurrence = MonthlyRecurrenceSchedule::new(
            Interval::one(),
            MonthlySpecification::OnThe(OrdinalMonthlyRecurrence { ordinal, day }),
        );

        let actual = recurrence.next_occurrence_after(from);

        assert_eq!(actual, expected);
    }

    #[traced_test]
    #[rstest]
    #[case::same_day("2025-06-07",Interval::one(), &[DayOfMonth::Seventh], "2025-07-07")]
    #[case::next_day("2025-06-07",Interval::one(), &[DayOfMonth::Eighth], "2025-06-08")]
    #[case::next_month("2025-06-07",Interval::one(), &[DayOfMonth::First], "2025-07-01")]
    #[case::skip_months_without_num_days("2025-06-07",Interval::one(), &[DayOfMonth::ThirtyFirst], "2025-07-31")]
    #[case::multiple_days("2025-06-07",Interval::one(), &[DayOfMonth::Tenth, DayOfMonth::Eleventh, DayOfMonth::Twelfth], "2025-06-10")]
    #[case::next_year("2025-12-20", Interval::one(), &[DayOfMonth::Fifth], "2026-01-05")]
    #[case::every_two_months("2025-06-07", Interval::two(), &[DayOfMonth::Fifth], "2025-08-05")]
    #[case::every_three_months("2025-06-07", Interval::three(), &[DayOfMonth::Seventh], "2025-09-07")]
    #[case::day_31_falls_back_to_last_day("2025-08-31", Interval::one(), &[DayOfMonth::ThirtyFirst], "2025-09-30")]
    #[case::quarterly_from_31st("2025-08-31", Interval::three(), &[DayOfMonth::ThirtyFirst], "2025-11-30")]
    fn monthly_next_occurrence_each(
        #[case] from: &str,
        #[case] interval: Interval,
        #[case] days: &[DayOfMonth],
        #[case] expected: &str,
    ) {
        let from = Date::from_str_unchecked(from);
        let expected = Date::from_str_unchecked(expected);
        let days = assert_some!(NonEmpty::from_slice(days));

        let recurrence = MonthlyRecurrenceSchedule::new(interval, MonthlySpecification::Each(days));

        let actual = recurrence.next_occurrence_after(from);

        assert_eq!(actual, expected);
    }

    #[test]
    fn ordinal_monthly_recurrence_should_default() {
        let recurrence = OrdinalMonthlyRecurrence::default();

        assert_eq!(
            recurrence,
            OrdinalMonthlyRecurrence::new(Ordinal::First, DaySpecification::Any),
            "expecting the ordinal recurrence to default to the first day"
        );
    }
}

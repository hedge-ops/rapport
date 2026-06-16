use nonempty::NonEmpty;

use crate::DisplayExt;
use crate::date::{Date, Month, Year};

use super::{FrequencyToken, Interval, OrdinalMonthlyRecurrence, Schedule};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YearlyRecurrenceSchedule {
    interval: Interval,
    months: NonEmpty<Month>,
    ordinal: Option<OrdinalMonthlyRecurrence>,
}

impl YearlyRecurrenceSchedule {
    pub fn new(
        interval: Interval,
        months: impl Into<NonEmpty<Month>>,
        ordinal: Option<OrdinalMonthlyRecurrence>,
    ) -> Self {
        Self {
            interval,
            months: months.into(),
            ordinal,
        }
    }

    fn find_occurrence_in_year_month(
        &self,
        year: Year,
        month: Month,
        reference_date: Date,
    ) -> Option<Date> {
        let month_num = month.to_month_number();
        let reference_day = reference_date.day();

        if let Some(ordinal_recurrence) = &self.ordinal {
            // Use ordinal-based calculation
            let first_of_month = Date::from_ymd_opt(year, month_num, 1)?;
            let value = ordinal_recurrence.find_occurrence_in_month(first_of_month);
            tracing::debug!(
                "ordinal occurrence in year: {year}, month: {month} for ordinal {ordinal_recurrence} is {}",
                value.map(Date::into_iso_string).displayed()
            );
            value
        } else {
            // Use same day of month as reference date
            let value = Date::from_ymd_opt(year, month_num, reference_day);
            tracing::debug!(
                "non-ordinal occurrence in year: {year}, month: {month} for date {} is {}",
                reference_date.into_iso_string(),
                value.map(Date::into_iso_string).displayed()
            );
            value
        }
    }

    pub(super) fn yearly(starting: Date) -> Self {
        Self::yearly_in_month(starting.month())
    }

    pub(super) fn yearly_in_month(month: Month) -> Self {
        Self::new(Interval::one(), NonEmpty::singleton(month), None)
    }
}

impl Schedule for YearlyRecurrenceSchedule {
    fn interval(&self) -> Interval {
        self.interval
    }

    fn next_occurrence_after(&self, value: Date) -> Date {
        let current_year = value.year();

        // Check if there's a valid occurrence in the current year after the given date
        for month in self.months.iter() {
            if let Some(candidate) = self.find_occurrence_in_year_month(current_year, *month, value)
                && candidate > value
            {
                return candidate;
            }
        }

        // Look forward in time for the next valid occurrence
        for year_offset in 1..=100 {
            // reasonable upper bound
            let target_year =
                current_year.saturating_add(year_offset * i32::from(self.interval.get()));

            for month in self.months.iter() {
                if let Some(candidate) =
                    self.find_occurrence_in_year_month(target_year, *month, value)
                {
                    return candidate;
                }
            }
        }

        // Fallback
        value
    }

    fn into_string(self, starting: Date) -> String {
        let interval_text = if self.interval.get() == 1 {
            FrequencyToken::Yearly.to_string()
        } else {
            format!("every {} years", self.interval)
        };
        let month_text = if self.months.len() == 1 && self.months.head == starting.month() {
            tracing::debug!("not printing month since the month specified is the same as starting");
            interval_text
        } else {
            tracing::debug!("printing month(s) since they differ from the starting month");
            let month_text = super::print_elements(&self.months);
            format!("{interval_text} {} {month_text}", super::GrammarToken::In)
        };

        if let Some(ordinal) = self.ordinal {
            format!(
                "{month_text} {} {} {ordinal}",
                super::GrammarToken::On,
                super::GrammarToken::The
            )
        } else {
            month_text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::YearlyRecurrenceSchedule;
    use crate::date::Date;
    use crate::recurrence::Schedule;
    use crate::{
        date::{Month, Weekday},
        recurrence::{DaySpecification, Interval, Ordinal, OrdinalMonthlyRecurrence},
    };
    use claims::assert_some;
    use nonempty::NonEmpty;
    use rstest::rstest;
    use tracing_test::traced_test;

    #[test]
    fn interval_should_return_configured_interval() {
        let schedule = YearlyRecurrenceSchedule::new(Interval::two(), Month::June, None);

        let actual = schedule.interval();

        assert_eq!(actual, Interval::two());
    }

    #[traced_test]
    #[rstest]
    #[case::normal_date(
        "2025-06-07",
        YearlyRecurrenceSchedule::yearly_in_month(Month::June),
        "2026-06-07"
    )]
    #[case::leap_year(
        "2024-02-29",
        YearlyRecurrenceSchedule::yearly_in_month(Month::February),
        "2028-02-29"
    )]
    #[case::multiple_months(
        "2025-06-07",
        YearlyRecurrenceSchedule::new(
            Interval::one(),
            assert_some!(NonEmpty::from_slice(&[Month::April, Month::September])),
            None),
        "2025-09-07")]
    #[case::every_two_years(
        "2025-06-07",
        YearlyRecurrenceSchedule { interval: Interval::two(), months: NonEmpty::singleton(Month::June), ordinal: None },
        "2027-06-07"
    )]
    #[case::every_three_years(
        "2025-03-15",
        YearlyRecurrenceSchedule { interval: Interval::three(), months: NonEmpty::singleton(Month::March), ordinal: None },
        "2028-03-15"
    )]
    #[case::multiple_months_after_all(
        "2025-11-15",
        YearlyRecurrenceSchedule { interval: Interval::one(), months: assert_some!(NonEmpty::from_slice(&[Month::March, Month::July])), ordinal: None },
        "2026-03-15"
    )]
    #[case::multiple_months_before_all(
        "2025-01-15",
        YearlyRecurrenceSchedule { interval: Interval::one(), months: assert_some!(NonEmpty::from_slice(&[Month::June, Month::October])), ordinal: None },
        "2025-06-15"
    )]
    #[case::end_of_year_transition(
        "2025-12-31",
        YearlyRecurrenceSchedule { interval: Interval::one(), months: NonEmpty::singleton(Month::January), ordinal: None },
        "2026-01-31"
    )]
    #[case::leap_year_non_feb_date(
        "2024-07-15",
        YearlyRecurrenceSchedule { interval: Interval::one(), months: NonEmpty::singleton(Month::July), ordinal: None },
        "2025-07-15"
    )]
    #[case::with_ordinal_first_monday(
        "2025-06-07",
        YearlyRecurrenceSchedule {
            interval: Interval::one(),
            months: NonEmpty::singleton(Month::July),
            ordinal: Some(OrdinalMonthlyRecurrence::new(Ordinal::First, DaySpecification::Specific(Weekday::Monday)))
        },
        "2025-07-07"
    )]
    #[case::with_ordinal_last_friday_december(
        "2025-06-07",
        YearlyRecurrenceSchedule {
            interval: Interval::one(),
            months: NonEmpty::singleton(Month::December),
            ordinal: Some(OrdinalMonthlyRecurrence::new(Ordinal::Last, DaySpecification::Specific(Weekday::Friday)))
        },
        "2025-12-26"
    )]
    #[case::with_ordinal_every_two_years_will_choose_next_occurrence(
        "2025-06-07",
        YearlyRecurrenceSchedule {
            interval: Interval::two(),
            months: NonEmpty::singleton(Month::June),
            ordinal: Some(OrdinalMonthlyRecurrence::new(Ordinal::Second, DaySpecification::Specific(Weekday::Tuesday)))
        },
        "2025-06-10" // the second tuesday after the given date is just two days away!
    )]
    #[case::different_day_value(
        "2025-05-01",
        YearlyRecurrenceSchedule { interval: Interval::one(), months: NonEmpty::singleton(Month::May), ordinal: None },
        "2026-05-01"
    )]
    #[case::february_non_leap_to_leap(
        "2025-02-28",
        YearlyRecurrenceSchedule { interval: Interval::one(), months: NonEmpty::singleton(Month::February), ordinal: None },
        "2026-02-28"
    )]
    fn yearly_next_occurrence_after(
        #[case] from: &str,
        #[case] schedule: YearlyRecurrenceSchedule,
        #[case] expected: &str,
    ) {
        let from = Date::from_str_unchecked(from);
        let expected = Date::from_str_unchecked(expected);
        let actual = schedule.next_occurrence_after(from);

        assert_eq!(actual, expected);
    }
}

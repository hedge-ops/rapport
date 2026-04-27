use std::{iter::Peekable, str::FromStr};

use nonempty::NonEmpty;

use super::{
    DailyRecurrenceSchedule, DaySpecification, Interval, MonthlySpecification, Ordinal,
    OrdinalMonthlyRecurrence, RecurrenceSchedule, TemporalUnit, YearlyRecurrenceSchedule,
};
use crate::{
    Error,
    date::Date,
    recurrence::{MonthlyRecurrenceSchedule, WeeklyRecurrenceSchedule},
};

#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display, derive_more::FromStr)]
pub(super) enum FrequencyToken {
    #[display("daily")]
    Daily,
    #[display("weekly")]
    Weekly,
    #[display("monthly")]
    Monthly,
    #[display("yearly")]
    Yearly,
    #[display("every")]
    Every,
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display, derive_more::FromStr)]
pub(super) enum GrammarToken {
    #[display("in")]
    In,
    #[display("on")]
    On,
    #[display("the")]
    The,
}

pub(super) fn parse(value: &str, starting: Date) -> Result<RecurrenceSchedule, Error> {
    tracing::debug!(
        "parsing {value} with starting {} into recurrence",
        starting.into_iso_string()
    );
    let mut parts = value.split(' ').peekable();
    let (unit, interval) = parse_frequency_and_interval(&mut parts)?;
    tracing::debug!("parsed unit: {unit}, interval: {interval}");
    parse_every_x(&mut parts, unit, interval, starting)
}

fn parse_frequency_and_interval<'value>(
    parts: &mut impl Iterator<Item = &'value str>,
) -> Result<(TemporalUnit, Interval), Error> {
    if let Some(first) = parts.next() {
        let first_token = FrequencyToken::from_str(first).map_err(|_| {
            Error::InvalidRecurrence(format!(
                "could not parse first word {first} into a valid frequency value"
            ))
        })?;
        let value = match first_token {
            FrequencyToken::Daily => (TemporalUnit::Day, Interval::one()),
            FrequencyToken::Weekly => (TemporalUnit::Week, Interval::one()),
            FrequencyToken::Monthly => (TemporalUnit::Month, Interval::one()),
            FrequencyToken::Yearly => (TemporalUnit::Year, Interval::one()),
            FrequencyToken::Every => {
                if let Some(second_token) = parts.next() {
                    if let Ok(interval) = Interval::from_str(second_token) {
                        // now parse the third
                        if let Some(third_token) = parts.next() {
                            let unit = TemporalUnit::from_str(third_token)?;
                            (unit, interval)
                        } else {
                            return Err(Error::InvalidRecurrence(format!(
                                "expecting a unit of duration to go after {} (interval)",
                                FrequencyToken::Every
                            )));
                        }
                    } else {
                        return Err(Error::InvalidRecurrence(format!(
                            "expecting the word following {} to be a numeric interval",
                            FrequencyToken::Every
                        )));
                    }
                } else {
                    return Err(Error::InvalidRecurrence(format!(
                        "expecting something to follow `{}`, try an interval",
                        FrequencyToken::Every
                    )));
                }
            }
        };
        Ok(value)
    } else {
        Err(Error::InvalidRecurrence(
            "could not parse empty string into recurrence".to_owned(),
        ))
    }
}

fn rejoin<'value>(parts: impl Iterator<Item = &'value str>) -> String {
    parts.collect::<Vec<_>>().join(" ")
}

pub(super) fn parse_elements<T>(value: &str, description: &str) -> Result<NonEmpty<T>, Error>
where
    T: FromStr + Clone,
{
    let normalized = value.replace(" and ", ", ");
    let values: Result<Vec<_>, _> = normalized
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(T::from_str)
        .collect();
    let values = values.map_err(|_| {
        Error::InvalidRecurrence(format!(
            "could not convert list for {description}, value: {value}"
        ))
    })?;

    NonEmpty::from_slice(&values)
        .ok_or_else(|| Error::InvalidRecurrence("no values found".to_owned()))
}

fn parse_every_x<'value, I>(
    remaining_parts: &mut Peekable<I>,
    unit: TemporalUnit,
    interval: Interval,
    starting: Date,
) -> Result<RecurrenceSchedule, Error>
where
    I: Iterator<Item = &'value str>,
{
    let schedule = match unit {
        TemporalUnit::Day => RecurrenceSchedule::Daily(DailyRecurrenceSchedule::new(interval)),
        TemporalUnit::Week => {
            let weekly_recurrence_schedule =
                if Some(GrammarToken::On.to_string().as_str()) == remaining_parts.next() {
                    parse_every_x_weeks_on(interval, &rejoin(remaining_parts))?
                } else {
                    let weekday = starting.weekday();
                    tracing::debug!(
                        "{} is not modified with {} so it is assumed to be on \
                    the same weekday {weekday} as the starting date",
                        TemporalUnit::Week,
                        GrammarToken::On
                    );
                    WeeklyRecurrenceSchedule::new(interval, NonEmpty::singleton(weekday))
                };
            RecurrenceSchedule::Weekly(weekly_recurrence_schedule)
        }
        TemporalUnit::Month => {
            let monthly_recurrence_schedule = if take_on_the(remaining_parts)? {
                parse_every_x_months_on(&rejoin(remaining_parts), interval)?
            } else {
                let day_of_month = starting.day_of_month();
                tracing::debug!(
                    "{} is not modified with '{} {}' so it is assumed to \
                    be on the same day of the month {day_of_month} as the starting date",
                    TemporalUnit::Month,
                    GrammarToken::On,
                    GrammarToken::The
                );
                MonthlyRecurrenceSchedule::new(interval, MonthlySpecification::each(day_of_month))
            };
            RecurrenceSchedule::Monthly(monthly_recurrence_schedule)
        }
        TemporalUnit::Year => {
            let yearly_recurrence_schedule =
                parse_every_x_years(remaining_parts, interval, starting)?;
            RecurrenceSchedule::Yearly(yearly_recurrence_schedule)
        }
    };
    Ok(schedule)
}

fn take_on_the<'value>(parts: &mut impl Iterator<Item = &'value str>) -> Result<bool, Error> {
    if Some(GrammarToken::On.to_string().as_str()) == parts.next() {
        if Some(GrammarToken::The.to_string().as_str()) == parts.next() {
            Ok(true)
        } else {
            tracing::error!(
                "recurrence string is invalid because it is {}, is modified \
                with {} but not followed by {}",
                TemporalUnit::Month,
                GrammarToken::On,
                GrammarToken::The
            );
            Err(Error::InvalidRecurrence(format!(
                "expecting {} to come after {}",
                GrammarToken::The,
                GrammarToken::On
            )))
        }
    } else {
        Ok(false)
    }
}

fn parse_every_x_weeks_on(
    interval: Interval,
    remaining: &str,
) -> Result<WeeklyRecurrenceSchedule, super::Error> {
    tracing::debug!("parsing days from remaining {remaining}");
    let days = parse_elements(remaining, "weekdays")?;
    Ok(WeeklyRecurrenceSchedule::new(interval, days))
}
fn parse_every_x_months_on(
    remaining: &str,
    interval: Interval,
) -> Result<MonthlyRecurrenceSchedule, Error> {
    tracing::debug!("recurrence is specified on certain days of the month, parsing: {remaining}",);
    let value = if let Some((ordinal, rest)) = Ordinal::try_parse(remaining) {
        tracing::debug!("{rest} is ordinal {ordinal}, so parsing as an ordinal recurrence",);
        let day = DaySpecification::from_str(rest.trim())?;
        MonthlyRecurrenceSchedule::new(
            interval,
            MonthlySpecification::OnThe(OrdinalMonthlyRecurrence::new(ordinal, day)),
        )
    } else {
        tracing::debug!("{remaining} is not ordinal but is only for specific days of the month");
        let days = parse_elements(remaining, "monthly")?;
        MonthlyRecurrenceSchedule::new(interval, MonthlySpecification::Each(days))
    };
    Ok(value)
}

fn parse_every_x_years<'value, I>(
    remaining_parts: &mut Peekable<I>,
    interval: Interval,
    starting: Date,
) -> Result<YearlyRecurrenceSchedule, Error>
where
    I: Iterator<Item = &'value str>,
{
    // peek to see if this is modified by 'in'
    if remaining_parts.peek() == Some(&GrammarToken::In.to_string().as_str()) {
        if let Some(in_token) = remaining_parts.next() {
            if in_token == GrammarToken::In.to_string() {
                let remaining = rejoin(remaining_parts);
                let on_the = format!("{} {}", GrammarToken::On, GrammarToken::The);
                let mut parts = remaining.split(&on_the);

                let months = if let Some(month_part) = parts.next() {
                    tracing::debug!("parsing months clause defined as {month_part}");
                    parse_elements(month_part, "months")?
                } else {
                    return Err(Error::InvalidRecurrence(format!(
                        "expecting month name after {}",
                        GrammarToken::In
                    )));
                };
                let ordinal = parts
                    .next()
                    .map(str::trim)
                    .map(parse_ordinal_clause)
                    .transpose()?;
                Ok(YearlyRecurrenceSchedule::new(interval, months, ordinal))
            } else {
                Err(in_token_expected_error())
            }
        } else {
            Err(in_token_expected_error())
        }
    } else {
        tracing::debug!(
            "months are not specified with the '{}' clause, still checking for \
            the ordinal {} {} clause and assuming the starting date as the month",
            GrammarToken::In,
            GrammarToken::On,
            GrammarToken::The
        );
        let ordinal = if take_on_the(remaining_parts)? {
            let remaining = rejoin(remaining_parts);
            Some(parse_ordinal_clause(&remaining)?)
        } else {
            None
        };
        Ok(YearlyRecurrenceSchedule::new(
            interval,
            NonEmpty::singleton(starting.month()),
            ordinal,
        ))
    }
}

fn in_token_expected_error() -> Error {
    Error::InvalidRecurrence(format!(
        "unexpected state where we peeked for {} but when retrieving didn't get it",
        GrammarToken::In
    ))
}

fn parse_ordinal_clause(value: &str) -> Result<OrdinalMonthlyRecurrence, Error> {
    if let Some((ordinal, rest)) = Ordinal::try_parse(value) {
        tracing::debug!("{rest} is ordinal {ordinal}, so parsing as an ordinal recurrence",);
        let day = DaySpecification::from_str(rest.trim())?;

        Ok(OrdinalMonthlyRecurrence::new(ordinal, day))
    } else {
        Err(Error::InvalidRecurrence(format!(
            "expecting ordinal followed by a date unit but \
                        nothing parsed in text: {value}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::super::RecurrenceSchedule;
    use super::*;
    use crate::{
        date::{DayOfMonth, Month, Weekday},
        recurrence::MonthlySpecification,
    };
    use claims::{assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use tracing_test::traced_test;

    #[traced_test]
    #[rstest]
    // Daily patterns
    #[case::daily_simple("daily", "2025-06-07", "2025-06-08")]
    #[case::daily_interval("every 3 days", "2025-06-07", "2025-06-10")]
    #[case::daily_large_interval("every 7 days", "2025-06-07", "2025-06-14")]
    // Weekly patterns
    #[case::weekly_simple("weekly", "2025-06-07", "2025-06-14")]
    #[case::weekly_interval("every 2 weeks", "2025-06-07", "2025-06-21")]
    #[case::weekly_interval_large("every 4 weeks", "2025-06-07", "2025-07-05")]
    #[case::weekly_single_day("weekly on Monday", "2025-06-07", "2025-06-09")]
    #[case::weekly_two_days("weekly on Monday and Wednesday", "2025-06-07", "2025-06-09")]
    #[case::weekly_multiple_days(
        "weekly on Monday, Wednesday and Friday",
        "2025-06-07",
        "2025-06-09"
    )]
    #[case::weekly_interval_with_days("every 2 weeks on Tuesday", "2025-06-07", "2025-06-17")]
    #[case::weekly_interval_multiple_days(
        "every 3 weeks on Monday and Thursday",
        "2025-06-07",
        "2025-06-23"
    )]
    // Monthly patterns - specific days
    #[case::monthly_simple("monthly", "2025-06-07", "2025-07-07")]
    #[case::monthly_single_day("monthly on the 15th", "2025-06-07", "2025-06-15")]
    #[case::monthly_single_day_next_month("monthly on the 1st", "2025-06-07", "2025-07-01")]
    #[case::monthly_two_days("monthly on the 1st and 15th", "2025-06-07", "2025-06-15")]
    #[case::monthly_multiple_days("monthly on the 5th, 15th and 25th", "2025-06-07", "2025-06-15")]
    #[case::monthly_end_of_month("monthly on the 31st", "2025-06-07", "2025-07-31")]
    // Monthly patterns - ordinal
    #[case::monthly_first_monday("monthly on the first Monday", "2025-06-07", "2025-07-07")]
    #[case::monthly_second_tuesday("monthly on the second Tuesday", "2025-06-07", "2025-06-10")]
    #[case::monthly_third_friday("monthly on the third Friday", "2025-06-07", "2025-06-20")]
    #[case::monthly_fourth_thursday("monthly on the fourth Thursday", "2025-06-07", "2025-06-26")]
    #[case::monthly_fifth_wednesday("monthly on the fifth Wednesday", "2025-06-07", "2025-07-30")]
    #[case::monthly_last_friday("monthly on the last Friday", "2025-06-07", "2025-06-27")]
    #[case::monthly_second_to_last_monday(
        "monthly on the next to last Monday",
        "2025-06-07",
        "2025-06-23"
    )]
    // Monthly patterns - ordinal with day types
    #[case::monthly_first_weekday("monthly on the first weekday", "2025-06-07", "2025-07-01")]
    #[case::monthly_last_weekend_day("monthly on the last weekend day", "2025-06-07", "2025-06-29")]
    #[case::monthly_last_any_day("monthly on the last day", "2025-06-07", "2025-06-30")]
    // Yearly patterns - simple
    #[case::yearly_simple("yearly", "2025-06-07", "2026-06-07")]
    #[case::yearly_interval("every 2 years", "2025-06-07", "2027-06-07")]
    #[case::yearly_interval_large("every 5 years", "2025-06-07", "2030-06-07")]
    // Yearly patterns - with months
    #[case::yearly_single_month("yearly in January", "2025-06-07", "2026-01-07")]
    #[case::yearly_two_months("yearly in March and September", "2025-06-07", "2025-09-07")]
    #[case::yearly_multiple_months(
        "yearly in January, June and December",
        "2025-06-07",
        "2025-12-07"
    )]
    #[case::yearly_interval_with_month("every 2 years in July", "2025-06-07", "2025-07-07")]
    // Yearly patterns - with ordinals
    #[case::yearly_first_monday_december(
        "yearly in December on the first Monday",
        "2025-06-07",
        "2025-12-01"
    )]
    #[case::yearly_last_friday_january(
        "yearly in January on the last Friday",
        "2025-06-07",
        "2026-01-30"
    )]
    #[case::yearly_second_tuesday_march(
        "yearly in March on the second Tuesday",
        "2025-06-07",
        "2026-03-10"
    )]
    #[case::yearly_third_weekend_day_june(
        "yearly in July on the third weekend day",
        "2025-06-07",
        "2025-07-12"
    )]
    #[case::yearly_interval_with_ordinal(
        "every 2 years in November on the fourth Thursday",
        "2025-06-07",
        "2025-11-27"
    )]
    // Edge cases
    #[case::leap_year_handling("yearly", "2024-02-29", "2028-02-29")]
    #[case::month_transition("monthly on the 1st", "2025-01-31", "2025-02-01")]
    #[case::year_transition("monthly", "2025-12-15", "2026-01-15")]
    #[case::weekend_only("weekly on Saturday and Sunday", "2025-06-07", "2025-06-08")]
    #[case::weekday_only(
        "weekly on Monday, Tuesday, Wednesday, Thursday and Friday",
        "2025-06-07",
        "2025-06-09"
    )]
    fn recurrence_schedule_string_round_trip(
        #[case] input: &str,
        #[case] starting: &str,
        #[case] expected: &str,
    ) {
        use crate::recurrence::Schedule;

        tracing::debug!("parsing input '{input}' with starting {starting}");
        let starting = Date::from_str_unchecked(starting);
        let expected = Date::from_str_unchecked(expected);

        let schedule: RecurrenceSchedule = assert_ok!(
            parse(input, starting),
            "expecting {input} to convert to a recurrence schedule"
        );
        tracing::debug!("'{input}' parsed into schedule: {:?}", schedule);
        let actual = schedule.next_occurrence_after(starting);

        assert_eq!(
            actual, expected,
            "expecting schedule for '{input}' to provide expected next date, \
            the fact that it is not the same may mean it did not parse correctly"
        );

        // verify round trip
        let output = schedule.clone().into_string(starting);
        assert_eq!(
            output.as_str(),
            input,
            "expecting schedule to convert to same string that it was read in as: input:'{input}' output:'{output}'",
        );
    }

    #[traced_test]
    #[rstest]
    #[case::invalid_empty("")]
    #[case::invalid_typo("dayly")]
    #[case::invalid_number("every zero days")]
    #[case::invalid_day("weekly on Moonday")]
    #[case::invalid_ordinal("on the zeroth Monday")]
    #[case::invalid_month("yearly in Jamuary")]
    #[case::malformed_interval("every days")]
    #[case::malformed_weekly("weekly on")]
    #[case::malformed_monthly("monthly on the")]
    #[case::malformed_yearly("yearly in")]
    fn recurrence_schedule_invalid_string_parsing(#[case] input: &str) {
        let starting = Date::from_str_unchecked("2025-06-12");
        let result: Result<RecurrenceSchedule, _> = parse(input, starting);
        assert!(
            result.is_err(),
            "Expected parsing to fail for invalid input: '{input}'"
        );
    }

    #[traced_test]
    #[rstest]
    #[case::edge_case_same_day("daily", "2025-06-07", "2025-06-08")]
    #[case::edge_case_past_date("weekly", "2020-01-01", "2020-01-08")]
    #[case::edge_case_far_future("yearly", "2025-06-07", "2026-06-07")]
    fn recurrence_schedule_edge_cases(
        #[case] input: &str,
        #[case] starting: &str,
        #[case] expected: &str,
    ) {
        use crate::recurrence::Schedule;

        let starting = Date::from_str_unchecked(starting);
        let schedule: RecurrenceSchedule = assert_ok!(
            parse(input, starting),
            "expecting '{input}' to parse into a recurrence schedule"
        );
        let expected_date = Date::from_str_unchecked(expected);
        let actual_next = schedule.next_occurrence_after(starting);

        assert_eq!(
            actual_next, expected_date,
            "expecting schedule to have predicated date"
        );

        // Ensure we can still round-trip
        let output = schedule.into_string(starting);
        assert_eq!(
            output, input,
            "expecting the schedule to print the same way it was parsed"
        );
    }

    #[rstest]
    #[case("day", TemporalUnit::Day)]
    #[case("days", TemporalUnit::Day)]
    #[case("week", TemporalUnit::Week)]
    #[case("weeks", TemporalUnit::Week)]
    #[case("month", TemporalUnit::Month)]
    #[case("months", TemporalUnit::Month)]
    #[case("year", TemporalUnit::Year)]
    #[case("years", TemporalUnit::Year)]
    fn unit_token_from_str(#[case] input: &str, #[case] expected: TemporalUnit) {
        let actual = assert_ok!(
            TemporalUnit::from_str(input),
            "expecting {input} to parse into unit token"
        );

        assert_eq!(actual, expected);
    }

    #[traced_test]
    #[rstest]
    #[case::days(
        "",
        TemporalUnit::Day,
        RecurrenceSchedule::Daily(DailyRecurrenceSchedule::new(Interval::three()))
    )]
    #[case::weeks(
        "",
        TemporalUnit::Week,
        RecurrenceSchedule::Weekly(WeeklyRecurrenceSchedule::new(
            Interval::three(),
            Weekday::Monday
        ))
    )]
    #[case::weeks_specifying_days(
        "on Monday and Friday",
        TemporalUnit::Week,
        RecurrenceSchedule::Weekly(WeeklyRecurrenceSchedule::new(
            Interval::three(),
            assert_some!(NonEmpty::from_slice(&[Weekday::Monday, Weekday::Friday]))))
    )]
    #[case::months(
        "",
        TemporalUnit::Month,
        RecurrenceSchedule::Monthly(MonthlyRecurrenceSchedule::new(
            Interval::three(),
            MonthlySpecification::each(DayOfMonth::Ninth)
        ))
    )]
    #[case::months_specifying_day_of_month(
        "on the 1st and 15th",
        TemporalUnit::Month,
        RecurrenceSchedule::Monthly(MonthlyRecurrenceSchedule::new(
            Interval::three(),
            MonthlySpecification::Each(assert_some!(NonEmpty::from_slice(&[DayOfMonth::First, DayOfMonth::Fifteenth])))))
    )]
    #[case::months_specifying_ordinal(
        "on the last Friday",
        TemporalUnit::Month,
        RecurrenceSchedule::Monthly(MonthlyRecurrenceSchedule::new(
            Interval::three(),
            MonthlySpecification::OnThe(OrdinalMonthlyRecurrence::new(
                Ordinal::Last,
                DaySpecification::Specific(Weekday::Friday)
            ))
        ))
    )]
    #[case::years(
        "",
        TemporalUnit::Year,
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::new(
            Interval::three(),
            NonEmpty::singleton(Month::June),
            None
        ))
    )]
    #[case::years_specifying_months(
        "in July",
        TemporalUnit::Year,
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::new(
            Interval::three(),
            NonEmpty::singleton(Month::July),
            None
        ))
    )]
    #[case::years_specifying_ordinal(
        "on the first weekday",
        TemporalUnit::Year,
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::new(
            Interval::three(),
            NonEmpty::singleton(Month::June),
            Some(OrdinalMonthlyRecurrence::new(Ordinal::First, DaySpecification::Weekday))
        ))
    )]
    #[case::years_specifying_month_and_ordinal(
        "in September on the third weekend day",
        TemporalUnit::Year,
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::new(
            Interval::three(),
            NonEmpty::singleton(Month::September),
            Some(OrdinalMonthlyRecurrence::new(Ordinal::Third, DaySpecification::Weekend))
        ))
    )]
    fn test_parse_every_x(
        #[case] remaining: &str,
        #[case] unit: TemporalUnit,
        #[case] expected: RecurrenceSchedule,
    ) {
        tracing::info!("parsing remaining: {remaining} for unit {unit}");
        let starting = Date::from_str_unchecked("2025-06-09");
        let interval = Interval::three();
        // simulate where the iterator would be at this point, which is empty
        // when the test case is an empty string
        let mut remaining_iter = if remaining.is_empty() {
            Box::new(std::iter::empty()) as Box<dyn Iterator<Item = &str>>
        } else {
            Box::new(remaining.split(' ')) as Box<dyn Iterator<Item = &str>>
        }
        .peekable();

        let actual = assert_ok!(
            parse_every_x(&mut remaining_iter, unit, interval, starting),
            "expecting the given input: {remaining} to parse properly"
        );

        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::single_value("Monday", NonEmpty::singleton(Weekday::Monday))]
    #[case::two_values("Monday and Tuesday", assert_some!(NonEmpty::from_slice(&[Weekday::Monday, Weekday::Tuesday])))]
    #[case::three_values("Monday, Tuesday and Wednesday", assert_some!(NonEmpty::from_slice(&[Weekday::Monday, Weekday::Tuesday, Weekday::Wednesday])))]
    fn test_parse_elements(#[case] input: &str, #[case] expected: NonEmpty<Weekday>) {
        let actual = assert_ok!(
            parse_elements(input, "for testing"),
            "expecting elements to be parsed from input {input}"
        );

        assert_eq!(actual, expected);
    }

    #[traced_test]
    #[rstest]
    #[case::single_day(
        "Tuesday",
        WeeklyRecurrenceSchedule::new(Interval::three(), NonEmpty::singleton(Weekday::Tuesday))
    )]
    #[case::two_days(
        "Tuesday and Wednesday",
        WeeklyRecurrenceSchedule::new(
            Interval::three(),
            assert_some!(NonEmpty::from_slice(&[Weekday::Tuesday, Weekday::Wednesday])))
    )]
    #[case::three_days(
        "Tuesday, Wednesday and Thursday",
        WeeklyRecurrenceSchedule::new(
            Interval::three(),
            assert_some!(NonEmpty::from_slice(&[Weekday::Tuesday, Weekday::Wednesday, Weekday::Thursday])))
    )]
    fn test_parse_every_x_weeks(
        #[case] remaining: &str,
        #[case] expected: WeeklyRecurrenceSchedule,
    ) {
        let actual = assert_ok!(super::parse_every_x_weeks_on(Interval::three(), remaining));

        assert_eq!(actual, expected);
    }
}

//! Manage an event that repeats without losing your mind.
mod daily;

mod monthly;
mod parser;
mod weekly;
mod yearly;

pub use daily::DailyRecurrenceSchedule;
pub use monthly::{MonthlyRecurrenceSchedule, MonthlySpecification, OrdinalMonthlyRecurrence};
pub use weekly::WeeklyRecurrenceSchedule;
pub use yearly::YearlyRecurrenceSchedule;

use std::{
    fmt::Display,
    num::{NonZeroU16, ParseIntError, TryFromIntError},
    str::FromStr,
};

use nonempty::NonEmpty;
use serde::{Deserialize, Serialize};
use strum::VariantArray;

use crate::{
    Error,
    date::{Date, Weekday},
};
use parser::{FrequencyToken, GrammarToken};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurrenceRule {
    starting: Date,
    schedule: RecurrenceSchedule,
}

impl RecurrenceRule {
    #[must_use]
    pub fn new(starting: Date, schedule: RecurrenceSchedule) -> Self {
        Self { starting, schedule }
    }

    #[must_use]
    pub fn take(self) -> (Date, RecurrenceSchedule) {
        (self.starting, self.schedule)
    }

    #[must_use]
    pub fn next_occurrence_after(&self, value: Date) -> Date {
        let mut next_value = self.schedule.next_occurrence_after(self.starting);
        while next_value <= value {
            next_value = self.schedule.next_occurrence_after(next_value);
        }
        next_value
    }

    /// Returns the next occurrence from this point forward.
    #[must_use]
    pub fn next_occurrence_from(&self, from: Date) -> Date {
        if self.starting >= from {
            self.starting
        } else {
            self.next_occurrence_after(from.sub_days(1))
        }
    }

    #[must_use]
    pub fn starting(&self) -> Date {
        self.starting
    }

    #[must_use]
    pub fn schedule(&self) -> &RecurrenceSchedule {
        &self.schedule
    }

    #[must_use]
    pub fn into_string(self) -> String {
        let Self { starting, schedule } = self;
        schedule.into_string(starting)
    }

    #[must_use]
    pub fn weekly(starting: Date) -> Self {
        Self {
            starting,
            schedule: RecurrenceSchedule::weekly(starting),
        }
    }

    #[must_use]
    pub fn daily(starting: Date) -> Self {
        Self {
            starting,
            schedule: RecurrenceSchedule::daily(),
        }
    }
}

/// A schedule for calculating a regular recurrence of dates.
pub trait Schedule {
    /// The interval for this schedule
    fn interval(&self) -> Interval;

    /// Provides the next scheduled date on this schedule after the given date.
    fn next_occurrence_after(&self, from: Date) -> Date;

    /// Converts this schedule into a string that can be parsed, given the same starting date, into
    /// the same schedule.
    fn into_string(self, starting: Date) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecurrenceSchedule {
    Daily(DailyRecurrenceSchedule),
    Weekly(WeeklyRecurrenceSchedule),
    Monthly(MonthlyRecurrenceSchedule),
    Yearly(YearlyRecurrenceSchedule),
}

impl RecurrenceSchedule {
    #[must_use]
    pub fn inner(&self) -> &dyn Schedule {
        match self {
            Self::Daily(schedule) => schedule,
            Self::Weekly(schedule) => schedule,
            Self::Monthly(schedule) => schedule,
            Self::Yearly(schedule) => schedule,
        }
    }

    #[must_use]
    pub fn inner_mut(&mut self) -> &mut dyn Schedule {
        match self {
            Self::Daily(schedule) => schedule,
            Self::Weekly(schedule) => schedule,
            Self::Monthly(schedule) => schedule,
            Self::Yearly(schedule) => schedule,
        }
    }

    /// Parses the given string in relation to the given starting date. The starting date is
    /// important in situations where the weekday or day is implied, like `every 2 weeks` will
    /// check the `starting` for the day of the week on which the schedule fallse.
    ///
    /// # Errors
    ///
    /// When given a string not in the expected format.
    pub fn parse(value: &str, starting: Date) -> Result<Self, Error> {
        parser::parse(value, starting)
    }

    /// Validates whether the given string is a valid recurrence schedule.
    #[must_use]
    pub fn validate(value: &str) -> bool {
        // the date does not matter because we are validating the text and the
        // parsing only takes the day of week or month.
        let validation_date = Date::MIN;
        Self::parse(value, validation_date).is_ok()
    }

    /// Creates a schedule that is every two weeks from the starting date.
    ///
    /// # Panics
    ///
    /// Should not panic; panic is used when the internal hard coded value does not fit the invariants.
    #[allow(clippy::expect_used)]
    #[must_use]
    pub fn every_two_weeks(starting: Date) -> Self {
        Self::Weekly(WeeklyRecurrenceSchedule::every_two_weeks(starting))
    }

    #[must_use]
    pub fn daily() -> Self {
        Self::Daily(DailyRecurrenceSchedule::daily())
    }

    #[must_use]
    pub fn weekly(starting: Date) -> Self {
        let day_of_week = starting.weekday();
        Self::weekly_on(day_of_week)
    }

    #[must_use]
    pub fn weekly_on(day: Weekday) -> Self {
        Self::Weekly(WeeklyRecurrenceSchedule::weekly_on(day))
    }

    #[must_use]
    pub fn weekly_on_same_day_as(date: Date) -> Self {
        Self::Weekly(WeeklyRecurrenceSchedule::weekly_on_same_day_as(date))
    }

    #[must_use]
    pub fn monthly(starting: Date) -> Self {
        Self::Monthly(MonthlyRecurrenceSchedule::monthly(starting))
    }

    /// Creates a recurrence schedule that is quarterly after each starting date.
    ///
    /// # Panics
    ///
    /// When the literal values used inside this function aren't allowed as invariants.
    #[allow(clippy::expect_used)]
    #[must_use]
    pub fn quarterly(starting: Date) -> Self {
        Self::Monthly(MonthlyRecurrenceSchedule::quarterly(starting))
    }

    #[must_use]
    pub fn yearly(starting: Date) -> Self {
        Self::Yearly(YearlyRecurrenceSchedule::yearly(starting))
    }

    /// The common set of recurrence schedules based on the given starting date.
    #[must_use]
    pub fn common(starting: Date) -> Vec<RecurrenceSchedule> {
        vec![
            Self::daily(),
            Self::weekly(starting),
            Self::every_two_weeks(starting),
            Self::monthly(starting),
            Self::quarterly(starting),
            Self::yearly(starting),
        ]
    }

    #[must_use]
    pub fn default_for_unit(value: TemporalUnit, starting: Date) -> Self {
        match value {
            TemporalUnit::Day => Self::daily(),
            TemporalUnit::Week => Self::weekly(starting),
            TemporalUnit::Month => Self::monthly(starting),
            TemporalUnit::Year => Self::yearly(starting),
        }
    }

    #[must_use]
    pub fn into_weekly(self) -> Option<WeeklyRecurrenceSchedule> {
        match self {
            Self::Weekly(weekly) => Some(weekly),
            _ => None,
        }
    }
    #[must_use]
    pub fn into_monthly(self) -> Option<MonthlyRecurrenceSchedule> {
        match self {
            Self::Monthly(monthly) => Some(monthly),
            _ => None,
        }
    }
}

impl Schedule for RecurrenceSchedule {
    fn interval(&self) -> Interval {
        self.inner().interval()
    }

    fn next_occurrence_after(&self, from: Date) -> Date {
        self.inner().next_occurrence_after(from)
    }

    fn into_string(self, starting: Date) -> String {
        match self {
            RecurrenceSchedule::Daily(schedule) => schedule.into_string(starting),
            RecurrenceSchedule::Weekly(schedule) => schedule.into_string(starting),
            RecurrenceSchedule::Monthly(schedule) => schedule.into_string(starting),
            RecurrenceSchedule::Yearly(schedule) => schedule.into_string(starting),
        }
    }
}

impl From<WeeklyRecurrenceSchedule> for RecurrenceSchedule {
    fn from(value: WeeklyRecurrenceSchedule) -> Self {
        RecurrenceSchedule::Weekly(value)
    }
}

pub fn toggle_value<T>(from: NonEmpty<T>, value: T) -> NonEmpty<T>
where
    T: PartialEq,
{
    let NonEmpty { mut head, mut tail } = from;

    if head == value {
        // asking for head to be toggled
        if let Some(first) = take_first(&mut tail) {
            // since there is a first element in tail, set it to the new head which replaces
            // the head
            head = first;
        }
        // else do nothing because the head is all there is
    } else if let Some(index) = tail.iter().position(|v| v == &value) {
        // remove the weekday in the tail
        tail.remove(index);
    } else {
        // weekday not found anywhere, add it to tail
        tail.push(value);
    }

    NonEmpty { head, tail }
}

fn take_first<T>(vec: &mut Vec<T>) -> Option<T> {
    if vec.is_empty() {
        None
    } else {
        Some(vec.remove(0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
#[display("{_0}")]
pub struct Interval(NonZeroU16);

impl Interval {
    #[must_use]
    pub fn one() -> Self {
        Self(NonZeroU16::MIN)
    }

    /// An interval of `2`
    ///
    /// # Panics
    ///
    /// This should not panic, because 2 is a hard coded `NonZeroU16`
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn two() -> Self {
        Self(NonZeroU16::new(2).expect("expecting literal 2 to be a valid non zero u16"))
    }

    /// An interval of `3`
    ///
    /// # Panics
    ///
    /// This should not panic, because 3 is a hard coded `NonZeroU16`
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn three() -> Self {
        Self(NonZeroU16::new(3).expect("expecting literal 3 to be a valid non zero u16"))
    }

    /// An interval of `4`
    ///
    /// # Panics
    ///
    /// This should not panic, because 4 is a hard coded `NonZeroU16`
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn four() -> Self {
        Self(NonZeroU16::new(4).expect("expecting literal 4 to be a valid non zero u16"))
    }

    #[must_use]
    pub fn minus_one(self) -> u16 {
        self.0.get() - 1
    }

    #[must_use]
    pub fn get(self) -> u16 {
        self.0.get()
    }

    #[must_use]
    pub fn is_many(self) -> bool {
        self.0 > NonZeroU16::MIN
    }
}

impl From<NonZeroU16> for Interval {
    fn from(value: NonZeroU16) -> Self {
        Self(value)
    }
}

impl From<Interval> for u32 {
    fn from(value: Interval) -> Self {
        value.0.get().into()
    }
}

impl From<Interval> for usize {
    fn from(value: Interval) -> Self {
        value.0.get().into()
    }
}

impl TryFrom<u16> for Interval {
    type Error = TryFromIntError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        let value = NonZeroU16::try_from(value)?;
        Ok(Interval::from(value))
    }
}

impl Default for Interval {
    fn default() -> Self {
        Self::one()
    }
}

impl FromStr for Interval {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, ParseIntError> {
        let value = NonZeroU16::from_str(s)?;
        Ok(Self(value))
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    derive_more::Display,
    derive_more::FromStr,
    strum::VariantArray,
)]
pub enum Ordinal {
    #[display("first")]
    #[default]
    First,
    #[display("second")]
    Second,
    #[display("third")]
    Third,
    #[display("fourth")]
    Fourth,
    #[display("fifth")]
    Fifth,
    /// Second to last occurrence
    #[display("next to last")]
    NextToLast,
    /// Last occurrence
    #[display("last")]
    Last,
}

impl Ordinal {
    fn try_parse(input: &str) -> Option<(Ordinal, &str)> {
        tracing::debug!("parsing `{input}` into ordinal and remaining string");
        Self::VARIANTS.iter().find_map(|&ordinal| {
            let ordinal_str = ordinal.to_string();
            input
                .strip_prefix(&ordinal_str)
                .filter(|remaining| match remaining.chars().next() {
                    None => true,                  // accept the end of string
                    Some(c) => !c.is_alphabetic(), // only accept if at a word boundary
                })
                .inspect(|remaining| {
                    tracing::debug!("parsed ordinal {ordinal} with remaining: {remaining}");
                })
                .map(|remaining| (ordinal, remaining))
        })
    }
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, derive_more::Display,
)]
pub enum DaySpecification {
    #[display("{_0}")]
    Specific(Weekday),
    #[display("day")]
    #[default]
    Any,
    #[display("weekday")]
    Weekday,
    #[display("weekend day")]
    Weekend,
}

impl DaySpecification {
    pub const VARIANTS: [DaySpecification; 10] = [
        DaySpecification::Specific(Weekday::Monday),
        DaySpecification::Specific(Weekday::Tuesday),
        DaySpecification::Specific(Weekday::Wednesday),
        DaySpecification::Specific(Weekday::Thursday),
        DaySpecification::Specific(Weekday::Friday),
        DaySpecification::Specific(Weekday::Saturday),
        DaySpecification::Specific(Weekday::Sunday),
        DaySpecification::Any,
        DaySpecification::Weekday,
        DaySpecification::Weekend,
    ];

    fn is_satisfied_by(self, value: Date) -> bool {
        match self {
            DaySpecification::Specific(weekday) => weekday == value.weekday(),
            DaySpecification::Any => true,
            DaySpecification::Weekday => value.weekday().is_weekday(),
            DaySpecification::Weekend => value.weekday().is_weekend(),
        }
    }
}

impl FromStr for DaySpecification {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Self::VARIANTS
            .iter()
            .find(|v| v.to_string() == s)
            .copied()
            .ok_or_else(|| {
                Error::InvalidRecurrence(format!("{s} is not a valid day specification"))
            })
    }
}

fn print_elements<T: Display>(non_empty: &NonEmpty<T>) -> String {
    let mut iter = non_empty.iter().peekable();
    let mut result = String::new();
    let mut is_first = true;

    while let Some(element) = iter.next() {
        if !is_first {
            match iter.peek() {
                Some(_) => result.push_str(", "),
                None => result.push_str(" and "),
            }
        }

        result.push_str(&element.to_string());
        is_first = false;
    }

    result
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    derive_more::Display,
    strum::VariantArray,
)]
pub enum TemporalUnit {
    #[display("day")]
    Day,
    #[display("week")]
    #[default]
    Week,
    #[display("month")]
    Month,
    #[display("year")]
    Year,
}

impl TemporalUnit {
    #[must_use]
    pub fn print(self, interval: Interval) -> String {
        let mut value = self.to_string();
        if interval.is_many() {
            value.push('s');
        }
        value
    }

    #[must_use]
    pub fn print_interval(self, interval: Interval) -> String {
        if interval == Interval::one() {
            let value = match self {
                Self::Day => FrequencyToken::Daily,
                Self::Week => FrequencyToken::Weekly,
                Self::Month => FrequencyToken::Monthly,
                Self::Year => FrequencyToken::Yearly,
            };
            value.to_string()
        } else {
            format!(
                "{} {interval} {}",
                FrequencyToken::Every,
                self.print(interval)
            )
        }
    }
}

impl FromStr for TemporalUnit {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "day" | "days" => Ok(TemporalUnit::Day),
            "week" | "weeks" => Ok(TemporalUnit::Week),
            "month" | "months" => Ok(TemporalUnit::Month),
            "year" | "years" => Ok(TemporalUnit::Year),
            other => Err(Error::InvalidRecurrence(format!(
                "expecting a unit token (day(s), week(s), month(s), year(s), got: {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::date::{DayOfMonth, Month};
    use claims::{assert_ok, assert_some};
    use nonempty::NonEmpty;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use tracing_test::traced_test;

    #[rstest]
    #[case::from_last_month("2025-06-01", "2025-07-01", RecurrenceSchedule::daily(), "2025-07-02")]
    #[case::years_ago("2020-06-01", "2025-07-01", RecurrenceSchedule::daily(), "2025-07-02")]
    fn recurrence_rule_next_occurrence_after(
        #[case] starting: &str,
        #[case] from: &str,
        #[case] schedule: RecurrenceSchedule,
        #[case] expected: &str,
    ) {
        let starting = Date::from_str_unchecked(starting);
        let from = Date::from_str_unchecked(from);
        let expected = Date::from_str_unchecked(expected);

        let rule = RecurrenceRule { starting, schedule };
        let actual = rule.next_occurrence_after(from);

        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case("2025-06-02", DaySpecification::Any, true)]
    #[case("2025-06-02", DaySpecification::Specific(Weekday::Monday), true)]
    #[case("2025-06-02", DaySpecification::Specific(Weekday::Tuesday), false)]
    #[case("2025-06-02", DaySpecification::Weekday, true)]
    #[case("2025-06-02", DaySpecification::Weekend, false)]
    #[case("2025-06-01", DaySpecification::Any, true)]
    #[case("2025-06-01", DaySpecification::Specific(Weekday::Sunday), true)]
    #[case("2025-06-01", DaySpecification::Specific(Weekday::Wednesday), false)]
    #[case("2025-06-01", DaySpecification::Weekday, false)]
    #[case("2025-06-01", DaySpecification::Weekend, true)]
    fn day_specification_is_satisfied_by(
        #[case] from: &str,
        #[case] day: DaySpecification,
        #[case] expected: bool,
    ) {
        let from = Date::from_str_unchecked(from);

        let actual = day.is_satisfied_by(from);

        assert_eq!(actual, expected);
    }

    #[traced_test]
    #[rstest]
    #[case::monthly(
        RecurrenceSchedule::Monthly(MonthlyRecurrenceSchedule::monthly_each(DayOfMonth::Ninth)),
        "2025-06-09",
        "monthly"
    )]
    #[case::every_two_months(
        RecurrenceSchedule::Monthly(MonthlyRecurrenceSchedule::each(
            Interval::two(),
            DayOfMonth::Ninth
        )),
        "2025-08-09",
        "every 2 months"
    )]
    #[case::monthly_on_alternate_day(
        RecurrenceSchedule::Monthly(MonthlyRecurrenceSchedule::monthly_each(
            DayOfMonth::Fifteenth
        )),
        "2025-06-09",
        "monthly on the 15th"
    )]
    #[case::yearly(
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::yearly_in_month(Month::August)),
        "2025-08-09",
        "yearly"
    )]
    #[case::yearly_with_month_and_ordinal(
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::new(
            Interval::one(),
            Month::January,
            Some(OrdinalMonthlyRecurrence::new(Ordinal::First, DaySpecification::Weekday))
        )),
        "2025-08-09",
        "yearly in January on the first weekday"
    )]
    #[case::every_two_years(
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::new(
            Interval::two(),
            Month::August,
            None
        )),
        "2025-08-09",
        "every 2 years"
    )]
    #[case::every_two_years_alternative_month(
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::new(
            Interval::two(),
            Month::September,
            None
        )),
        "2025-08-09",
        "every 2 years in September"
    )]
    #[case::every_two_years_alternative_multiple_months(
        RecurrenceSchedule::Yearly(YearlyRecurrenceSchedule::new(
            Interval::two(),
            assert_some!(NonEmpty::from_slice(&[Month::September, Month::October])),
            None)),
        "2025-08-09",
        "every 2 years in September and October"
    )]
    fn test_into_string(
        #[case] input: RecurrenceSchedule,
        #[case] starting: &str,
        #[case] expected: &str,
    ) {
        let starting = Date::from_str_unchecked(starting);
        let actual = input.into_string(starting);
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::starting_is_from("2025-07-01", "2025-07-01", RecurrenceSchedule::daily(), "2025-07-01")]
    #[case::starting_is_after_from(
        "2025-07-10",
        "2025-07-01",
        RecurrenceSchedule::daily(),
        "2025-07-10"
    )]
    #[case::from_is_after_starting(
        "2025-06-01",
        "2025-07-01",
        RecurrenceSchedule::daily(),
        "2025-07-01"
    )]
    #[case::weekly_on_starting_day(
        "2025-07-07",
        "2025-07-07",
        RecurrenceSchedule::weekly(Date::from_str_unchecked("2025-07-07")),
        "2025-07-07"
    )]
    #[case::weekly_after_starting(
        "2025-07-07",
        "2025-07-09",
        RecurrenceSchedule::weekly(Date::from_str_unchecked("2025-07-07")),
        "2025-07-14"
    )]
    fn recurrence_rule_next_occurrence_from(
        #[case] starting: &str,
        #[case] from: &str,
        #[case] schedule: RecurrenceSchedule,
        #[case] expected: &str,
    ) {
        let starting = Date::from_str_unchecked(starting);
        let from = Date::from_str_unchecked(from);
        let expected = Date::from_str_unchecked(expected);

        let rule = RecurrenceRule { starting, schedule };
        let actual = rule.next_occurrence_from(from);

        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case(&["apple"], "apple")]
    #[case(&["apple", "banana"], "apple and banana")]
    #[case(&["apple", "banana", "cherry"], "apple, banana and cherry")]
    #[case(&["apple", "banana", "cherry", "date"], "apple, banana, cherry and date")]
    #[case(&["apple", "banana", "cherry", "date", "elderberry"], "apple, banana, cherry, date and elderberry")]
    #[case(&["first", "second", "third", "fourth", "fifth", "sixth"], "first, second, third, fourth, fifth and sixth")]
    fn test_print_elements(#[case] elements: &[&str], #[case] expected: &str) {
        let elements = assert_some!(
            NonEmpty::from_slice(elements),
            "precondition: non empty list of elements to print"
        );
        assert_eq!(super::print_elements(&elements), expected);
    }

    #[traced_test]
    #[rstest]
    #[case::first_monday("first Monday", Some((Ordinal::First, " Monday")))]
    #[case::second_tuesday("second Tuesday", Some((Ordinal::Second, " Tuesday")))]
    #[case::third_friday("third Friday", Some((Ordinal::Third, " Friday")))]
    #[case::fourth_thursday("fourth Thursday", Some((Ordinal::Fourth, " Thursday")))]
    #[case::fifth_wednesday("fifth Wednesday", Some((Ordinal::Fifth, " Wednesday")))]
    #[case::last_friday("last Friday", Some((Ordinal::Last, " Friday")))]
    #[case::next_to_last_monday("next to last Monday", Some((Ordinal::NextToLast, " Monday")))]
    #[case::first_weekday("first weekday", Some((Ordinal::First, " weekday")))]
    #[case::last_weekend("last weekend", Some((Ordinal::Last, " weekend")))]
    #[case::second_any("second any", Some((Ordinal::Second, " any")))]
    #[case::first_only_no_day("first", Some((Ordinal::First, "")))]
    #[case::last_only_no_day("last", Some((Ordinal::Last, "")))]
    #[case::next_to_last_only_no_day("next to last", Some((Ordinal::NextToLast, "")))]
    #[case::numeric_ordinal_no_match("5th Monday", None)]
    #[case::random_word_no_match("random Tuesday", None)]
    #[case::empty_string_no_match("", None)]
    #[case::just_day_no_match("Monday", None)]
    #[case::partial_match_firstly_should_fail("firstly Monday", None)]
    #[case::partial_match_lasting_should_fail("lasting Friday", None)]
    #[case::partial_match_secondary_should_fail("secondary Tuesday", None)]
    #[case::first_with_multiple_spaces("first  Monday", Some((Ordinal::First, "  Monday")))]
    #[case::next_to_last_with_multiple_spaces("next to last  Friday", Some((Ordinal::NextToLast, "  Friday")))]
    fn test_try_parse_ordinal_prefix(
        #[case] input: &str,
        #[case] expected: Option<(Ordinal, &str)>,
    ) {
        let actual = Ordinal::try_parse(input);

        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::monday("Monday", DaySpecification::Specific(Weekday::Monday))]
    #[case::tuesday("Tuesday", DaySpecification::Specific(Weekday::Tuesday))]
    #[case::wednesday("Wednesday", DaySpecification::Specific(Weekday::Wednesday))]
    #[case::thursday("Thursday", DaySpecification::Specific(Weekday::Thursday))]
    #[case::friday("Friday", DaySpecification::Specific(Weekday::Friday))]
    #[case::saturday("Saturday", DaySpecification::Specific(Weekday::Saturday))]
    #[case::sunday("Sunday", DaySpecification::Specific(Weekday::Sunday))]
    #[case::weekday("weekday", DaySpecification::Weekday)]
    #[case::weekend("weekend day", DaySpecification::Weekend)]
    #[case::any("day", DaySpecification::Any)]
    fn test_day_specification_from_str(#[case] input: &str, #[case] expected: DaySpecification) {
        let actual = assert_ok!(DaySpecification::from_str(input));

        assert_eq!(actual, expected);
    }
}

//! Relative date scheduling for follow-ups and deferrals

use std::fmt::{Display, Formatter};

use crate::{Error, date::Date, recurrence::Interval};
use facet::Facet;
use regex_macro::regex;
use serde::{Deserialize, Serialize};

/// A relative offset from a reference date, commonly used for follow-ups and scheduling
// facet's derive emits an `unsafe impl`, which trips `unsafe_derive_deserialize`; deserialization itself is safe.
#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Facet, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(C)]
pub enum RelativeOffset {
    Tomorrow,
    NextBusinessDay,
    Days(Interval),
    NextWeek,
    InWeeks(Interval),
    NextMonth,
    InMonths(Interval),
    NextYear,
    InYears(Interval),
}

impl RelativeOffset {
    /// The common options
    #[must_use]
    #[allow(unused)]
    pub fn common_options() -> [Self; 11] {
        [
            Self::Tomorrow,
            Self::NextBusinessDay,
            Self::Days(Interval::two()),
            Self::Days(Interval::three()),
            Self::NextWeek,
            Self::InWeeks(Interval::one()),
            Self::InWeeks(Interval::two()),
            Self::NextMonth,
            Self::InMonths(Interval::two()),
            Self::InMonths(Interval::three()),
            Self::NextYear,
        ]
    }

    /// Calculate the target date from the given reference date
    #[must_use]
    pub fn resolve_from(self, reference: Date) -> Date {
        match self {
            Self::Tomorrow => reference.add_days(1),
            Self::Days(days) => reference.add_days(days.into()),
            Self::NextBusinessDay => reference.next_business_day(),
            Self::NextWeek => reference.next_monday(),
            Self::InWeeks(weeks) => {
                let weeks = usize::from(weeks);
                let days = weeks.saturating_mul(7);
                reference.add_days(days)
            }
            Self::NextMonth => reference.first_of_next_month(),
            Self::InMonths(months) => reference.add_months(months),
            Self::NextYear => reference.next_january_first(),
            Self::InYears(years) => reference.add_years(years.into()),
        }
    }

    /// Parse natural language expressions like "in 3 days", "next week"
    ///
    /// # Errors
    /// when the given input does not parse into an offset
    pub fn parse_natural(input: impl Into<String>) -> Result<Self, Error> {
        let input = input.into().trim().to_lowercase();

        match input.as_str() {
            "tomorrow" => Ok(Self::Tomorrow),
            "next business day" => Ok(Self::NextBusinessDay),
            "next week" => Ok(Self::NextWeek),
            "next month" => Ok(Self::NextMonth),
            "next year" => Ok(Self::NextYear),
            _ => {
                // Parse "in N days/weeks/months/years" patterns
                if let Some(captures) =
                    regex!(r"^in (\d+) (days?|weeks?|months?|years?)$").captures(&input)
                {
                    let interval: u16 = captures[1]
                        .parse()
                        .map_err(|_| Error::InvalidOffset("Number too large".to_string()))?;
                    let interval = Interval::try_from(interval).map_err(|_| {
                        Error::InvalidOffset(format!("{interval} is an invalid offset"))
                    })?;

                    match &captures[2] {
                        "day" | "days" => Ok(Self::Days(interval)),
                        "week" | "weeks" => Ok(Self::InWeeks(interval)),
                        "month" | "months" => Ok(Self::InMonths(interval)),
                        "year" | "years" => Ok(Self::InYears(interval)),
                        _ => Err(Error::InvalidOffset(format!(
                            "Unknown time unit: {}",
                            &captures[2]
                        ))),
                    }
                } else {
                    Err(Error::InvalidOffset(format!("Cannot parse: {input}")))
                }
            }
        }
    }
}

impl Display for RelativeOffset {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RelativeOffset::Tomorrow => f.write_str("tomorrow"),
            RelativeOffset::NextBusinessDay => f.write_str("next business day"),
            RelativeOffset::Days(interval) => {
                if interval.is_many() {
                    f.write_str(format!("in {interval} days").as_str())
                } else {
                    f.write_str("tomorrow")
                }
            }
            RelativeOffset::NextWeek => f.write_str("next week"),
            RelativeOffset::InWeeks(interval) => {
                if interval.is_many() {
                    f.write_str(format!("in {interval} weeks").as_str())
                } else {
                    f.write_str("in 1 week")
                }
            }
            RelativeOffset::NextMonth => f.write_str("next month"),
            RelativeOffset::InMonths(interval) => {
                if interval.is_many() {
                    f.write_str(format!("in {interval} months").as_str())
                } else {
                    f.write_str("next month")
                }
            }
            RelativeOffset::NextYear => f.write_str("next year"),
            RelativeOffset::InYears(interval) => {
                if interval.is_many() {
                    f.write_str(format!("in {interval} years").as_str())
                } else {
                    f.write_str("next year")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::tomorrow(RelativeOffset::Tomorrow, "2025-01-03", "2025-01-04")]
    #[case::two_days(RelativeOffset::Days(Interval::two()), "2025-01-03", "2025-01-05")]
    #[case::three_days(RelativeOffset::Days(Interval::three()), "2025-01-03", "2025-01-06")]
    #[case::next_business_day_from_friday_to_monday(
        RelativeOffset::NextBusinessDay,
        "2025-01-03",
        "2025-01-06"
    )]
    #[case::next_business_day_monday_to_tuesday(
        RelativeOffset::NextBusinessDay,
        "2025-01-06",
        "2025-01-07"
    )]
    #[case::next_week_friday_to_monday(RelativeOffset::NextWeek, "2025-01-03", "2025-01-06")]
    #[case::next_week_monday_to_next_monday(RelativeOffset::NextWeek, "2025-01-06", "2025-01-13")]
    #[case::one_week(RelativeOffset::InWeeks(Interval::one()), "2025-01-03", "2025-01-10")]
    #[case::two_weeks(RelativeOffset::InWeeks(Interval::two()), "2025-01-03", "2025-01-17")]
    #[case::next_month(RelativeOffset::NextMonth, "2025-01-15", "2025-02-01")]
    #[case::one_month(RelativeOffset::InMonths(Interval::one()), "2025-01-15", "2025-02-15")]
    #[case::three_months(
        RelativeOffset::InMonths(Interval::three()),
        "2025-01-15",
        "2025-04-15"
    )]
    #[case::next_year(RelativeOffset::NextYear, "2025-06-15", "2026-01-01")]
    #[case::one_year(RelativeOffset::InYears(Interval::one()), "2025-06-15", "2026-06-15")]
    fn offset_resolve_from(
        #[case] offset: RelativeOffset,
        #[case] from: &str,
        #[case] expected: &str,
    ) {
        let from = assert_ok!(Date::from_str(from), "precondition: from parses");
        let expected = assert_ok!(Date::from_str(expected), "precondition: to parses");

        let result = offset.resolve_from(from);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::tomorrow("tomorrow", RelativeOffset::Tomorrow)]
    #[case::next_business_day("next business day", RelativeOffset::NextBusinessDay)]
    #[case::next_week("next week", RelativeOffset::NextWeek)]
    #[case::next_month("next month", RelativeOffset::NextMonth)]
    #[case::next_year("next year", RelativeOffset::NextYear)]
    #[case::in_two_days("in 2 days", RelativeOffset::Days(Interval::two()))]
    #[case::in_one_week("in 1 week", RelativeOffset::InWeeks(Interval::one()))]
    #[case::in_three_months("in 3 months", RelativeOffset::InMonths(Interval::three()))]
    #[case::in_one_year("in 1 year", RelativeOffset::InYears(Interval::one()))]
    fn parse_natural_language(#[case] input: &str, #[case] expected: RelativeOffset) {
        let result = assert_ok!(RelativeOffset::parse_natural(input));
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::invalid_format("next tuesday")]
    #[case::unknown_unit("in 2 fortnights")]
    #[case::empty_string("")]
    #[case::malformed("in days 3")]
    fn parse_natural_language_fails(#[case] input: &str) {
        let result = RelativeOffset::parse_natural(input);
        assert_err!(result);
    }

    #[rstest]
    #[case::leap_year_handling(
        "2024-02-29",
        RelativeOffset::InYears(Interval::one()),
        "2025-02-28"
    )]
    #[case::regular_year("2025-02-28", RelativeOffset::InYears(Interval::one()), "2026-02-28")]
    #[case::month_end_edge_case(
        "2025-01-31",
        RelativeOffset::InMonths(Interval::one()),
        "2025-02-28"
    )]
    fn edge_cases_handled_correctly(
        #[case] from: &str,
        #[case] offset: RelativeOffset,
        #[case] expected: &str,
    ) {
        let from = assert_ok!(Date::from_str(from), "precondition: from a valid date");
        let expected = assert_ok!(
            Date::from_str(expected),
            "precondition: expected a valid date"
        );

        let result = offset.resolve_from(from);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::first(RelativeOffset::common_options()[0])]
    #[case::second(RelativeOffset::common_options()[1])]
    #[case::third(RelativeOffset::common_options()[2])]
    #[case::fourth(RelativeOffset::common_options()[3])]
    #[case::fifth(RelativeOffset::common_options()[4])]
    #[case::sixth(RelativeOffset::common_options()[5])]
    #[case::seventh(RelativeOffset::common_options()[6])]
    #[case::eighth(RelativeOffset::common_options()[7])]
    #[case::ninth(RelativeOffset::common_options()[8])]
    #[case::tenth(RelativeOffset::common_options()[9])]
    #[case::eleventh(RelativeOffset::common_options()[10])]
    fn common_options_can_display_then_parse(#[case] option: RelativeOffset) {
        let display = option.to_string();
        let parsed = assert_ok!(
            RelativeOffset::parse_natural(display.clone().as_str()),
            "expecting offset to parse back from {display}"
        );
        assert_eq!(
            parsed, option,
            "expecting {display} to parse back to original option"
        );
    }

    #[test]
    fn common_options_include_one_week_and_next_week() {
        let options = RelativeOffset::common_options();

        assert!(options.contains(&RelativeOffset::InWeeks(Interval::one())));
        assert!(options.contains(&RelativeOffset::NextWeek));
    }

    #[test]
    fn one_week_and_next_week_resolve_differently_when_not_same_day() {
        let today = assert_ok!(Date::from_str("2025-01-03"), "precondition: today parses");

        assert_ne!(
            RelativeOffset::InWeeks(Interval::one()).resolve_from(today),
            RelativeOffset::NextWeek.resolve_from(today)
        );
    }

    // Mirrors the consumer (app_v2) usage: `RelativeOffset` is carried inside a
    // facet-0.46 enum that also derives serde + Hash/Eq and is `#[repr(C)]`. This
    // compiles only if `RelativeOffset` (and the `Interval` it wraps) satisfy all
    // of those bounds, which is the whole point of the derives.
    #[test]
    fn satisfies_facet_0_46_consumer_bounds() {
        #[derive(
            facet::Facet, serde::Serialize, serde::Deserialize, Hash, PartialEq, Eq, Clone, Debug,
        )]
        #[repr(C)]
        #[allow(dead_code, clippy::unsafe_derive_deserialize)]
        enum DateSelection {
            Offset(RelativeOffset),
            None,
        }

        fn assert_consumer_bounds<'a, T>()
        where
            T: facet::Facet<'a>
                + serde::Serialize
                + serde::de::DeserializeOwned
                + std::hash::Hash
                + Eq,
        {
        }

        assert_consumer_bounds::<DateSelection>();
        assert_consumer_bounds::<RelativeOffset>();
        assert_consumer_bounds::<Interval>();
    }
}

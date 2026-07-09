//! Date query parsing for CLI applications.
//!
//! Provides flexible date and period parsing for command-line flags like
//! `--date`, `--from`, `--to`, and `--in`. Designed for AI agent consumption.
//!
//! # Supported Formats
//!
//! ## Single Date (`parse_date`)
//! - ISO dates: `2025-12-31`
//! - Present: `today`
//! - Past: `yesterday`, `last friday`, `7 days ago`, `7d ago`, `1 week ago`
//! - Future: `tomorrow`, `next monday`, `in 3 days`, `in 7d`, `next week`, `next month`
//!
//! ## Period (`parse_period`)
//! - Year-month: `2025-12` (full month range)
//! - Quarter: `2025-Q1`
//! - Natural language: `this week`, `last week`, `this month`, `last month`, `yesterday`

use std::str::FromStr;

use crate::date::{Date, Weekday, YearMonth, YearQuarter};

/// Error returned when a date string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid date format: \"{0}\"")]
pub struct DateParseError(pub String);

/// Error returned when a period string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid period format: \"{0}\"")]
pub struct PeriodParseError(pub String);

/// A date range representing a reporting period.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Period {
    pub from: Date,
    pub to: Date,
}

impl Period {
    #[must_use]
    pub fn new(from: Date, to: Date) -> Self {
        Self { from, to }
    }

    #[must_use]
    pub fn single_day(date: Date) -> Self {
        Self {
            from: date,
            to: date,
        }
    }

    #[must_use]
    pub fn render(&self) -> String {
        if self.from == self.to {
            self.from.to_string()
        } else {
            format!("{} to {}", self.from, self.to)
        }
    }

    /// Check if a date falls within this period (inclusive on both ends).
    #[must_use]
    pub fn contains(self, date: Date) -> bool {
        date >= self.from && date <= self.to
    }
}

/// Try to parse a single date string. Handles:
/// - ISO dates: `2025-12-31`
/// - Present: `today`
/// - Past: `yesterday`, `last friday`, `7 days ago`, `7d ago`, `1 week ago`
/// - Future: `tomorrow`, `next monday`, `in 3 days`, `in 7d`, `next week`, `next month`
///
/// Note: Year-month formats like `2025-12` are periods, not dates. Use `try_parse_period` instead.
///
/// # Errors
///
/// Returns `DateParseError` if the input cannot be parsed.
pub fn try_parse_date(input: &str, today: Date) -> Result<Date, DateParseError> {
    let input = input.trim();

    // Try ISO date format first (YYYY-MM-DD)
    if let Ok(date) = Date::from_str(input) {
        return Ok(date);
    }

    // Try last weekday (e.g., "last friday")
    if let Some(date) = parse_last_weekday(input, today) {
        return Ok(date);
    }

    // Try next weekday (e.g., "next monday")
    if let Some(date) = parse_next_weekday(input, today) {
        return Ok(date);
    }

    // Try relative expressions (e.g., "yesterday", "1 week ago", "in 3 days")
    if let Some(date) = parse_relative(input, today) {
        return Ok(date);
    }

    Err(DateParseError(input.to_owned()))
}

/// Parse a single date string. Handles:
/// - ISO dates: `2025-12-31`
/// - Present: `today`
/// - Past: `yesterday`, `last friday`, `7 days ago`, `7d ago`, `1 week ago`
/// - Future: `tomorrow`, `next monday`, `in 3 days`, `in 7d`, `next week`, `next month`
///
/// Note: Year-month formats like `2025-12` are periods, not dates. Use `parse_period` instead.
///
/// Returns `today` if the input cannot be parsed.
#[must_use]
pub fn parse_date(input: &str, today: Date) -> Date {
    try_parse_date(input, today).unwrap_or(today)
}

/// Try to parse period strings for `--in` flag:
/// - Year-month: `2025-12`
/// - Quarter: `2025-Q1`
/// - Natural language: `last week`, `this week`, `last month`, `this month`, `yesterday`
///
/// # Errors
///
/// Returns `PeriodParseError` if the input cannot be parsed.
pub fn try_parse_period(input: &str, today: Date) -> Result<Period, PeriodParseError> {
    let input = input.trim();

    // Try year-month (YYYY-MM)
    if let Some(ym) = YearMonth::parse(input) {
        return Ok(Period::new(ym.first_day(), ym.last_day()));
    }

    // Try quarter (YYYY-Q#)
    if let Some(quarter) = YearQuarter::parse(input) {
        return Ok(Period::new(quarter.first_day(), quarter.last_day()));
    }

    // Natural language periods
    match input.to_lowercase().as_str() {
        "last week" => {
            let week_start = today.latest_sunday().sub_days(6);
            let week_end = today.latest_sunday();
            Ok(Period::new(week_start, week_end))
        }
        "this week" => {
            let week_start = today.latest_sunday().add_days(1);
            let week_end = today.latest_sunday().add_days(7);
            Ok(Period::new(week_start, week_end))
        }
        "last month" => {
            let last_month_date = today.sub_months(1u32);
            let month_start = last_month_date.first_of_month();
            let month_end = month_start.last_day_of_month();
            Ok(Period::new(month_start, month_end))
        }
        "this month" => {
            let month_start = today.first_of_month();
            let month_end = month_start.last_day_of_month();
            Ok(Period::new(month_start, month_end))
        }
        "yesterday" => {
            let yesterday = today.sub_days(1);
            Ok(Period::single_day(yesterday))
        }
        _ => Err(PeriodParseError(input.to_owned())),
    }
}

/// Parse period strings for `--in` flag:
/// - Year-month: `2025-12`
/// - Quarter: `2025-Q1`
/// - Natural language: `last week`, `this week`, `last month`, `this month`, `yesterday`
///
/// Returns a single-day period of `today` if the input cannot be parsed.
#[must_use]
pub fn parse_period(input: &str, today: Date) -> Period {
    try_parse_period(input, today).unwrap_or_else(|_| Period::single_day(today))
}

/// Parse "last WEEKDAY" expressions (e.g., "last friday").
fn parse_last_weekday(input: &str, today: Date) -> Option<Date> {
    let input = input.trim().to_lowercase();

    if !input.starts_with("last ") {
        return None;
    }

    let weekday_str = &input[5..]; // Remove "last "
    let weekday = parse_weekday_name(weekday_str)?;
    Some(today.last_weekday(weekday))
}

/// Parse "next WEEKDAY" expressions (e.g., "next monday").
fn parse_next_weekday(input: &str, today: Date) -> Option<Date> {
    let input = input.trim().to_lowercase();

    if !input.starts_with("next ") {
        return None;
    }

    let weekday_str = &input[5..]; // Remove "next "
    let weekday = parse_weekday_name(weekday_str)?;
    Some(today.next_weekday(weekday))
}

/// Parse a weekday name string into a Weekday.
fn parse_weekday_name(name: &str) -> Option<Weekday> {
    match name {
        "monday" => Some(Weekday::Monday),
        "tuesday" => Some(Weekday::Tuesday),
        "wednesday" => Some(Weekday::Wednesday),
        "thursday" => Some(Weekday::Thursday),
        "friday" => Some(Weekday::Friday),
        "saturday" => Some(Weekday::Saturday),
        "sunday" => Some(Weekday::Sunday),
        _ => None,
    }
}

/// Direction of a duration offset.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Direction {
    Forward,
    Backward,
}

/// Time unit for duration parsing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TimeUnit {
    Days,
    Weeks,
    Months,
    Years,
}

impl TimeUnit {
    fn parse(unit: &str) -> Option<Self> {
        match unit {
            "d" | "day" | "days" => Some(Self::Days),
            "w" | "week" | "weeks" => Some(Self::Weeks),
            "month" | "months" => Some(Self::Months),
            "y" | "year" | "years" => Some(Self::Years),
            _ => None,
        }
    }
}

/// A parsed duration with direction, unit, and count.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct ParsedDuration {
    direction: Direction,
    unit: TimeUnit,
    count: usize,
}

impl ParsedDuration {
    fn apply(self, today: Date) -> Date {
        let count_u32 = u32::try_from(self.count).unwrap_or(u32::MAX);
        match (self.direction, self.unit) {
            (Direction::Forward, TimeUnit::Days) => today.add_days(self.count),
            (Direction::Forward, TimeUnit::Weeks) => today.add_days(self.count * 7),
            (Direction::Forward, TimeUnit::Months) => today.add_months(count_u32),
            (Direction::Forward, TimeUnit::Years) => today.add_years(count_u32),
            (Direction::Backward, TimeUnit::Days) => today.sub_days(self.count),
            (Direction::Backward, TimeUnit::Weeks) => today.sub_days(self.count * 7),
            (Direction::Backward, TimeUnit::Months) => today.sub_months(count_u32),
            (Direction::Backward, TimeUnit::Years) => today.sub_years(count_u32),
        }
    }
}

/// Parse a duration expression like "in 3 days" or "7d ago".
///
/// Returns `Some(ParsedDuration)` if input has "in " prefix or " ago" suffix.
/// Returns `None` if neither marker is present.
fn parse_duration(input: &str) -> Option<ParsedDuration> {
    let input = input.trim().to_lowercase();

    let (direction, middle) = if let Some(rest) = input.strip_prefix("in ") {
        (Direction::Forward, rest.trim())
    } else {
        let rest = input.strip_suffix(" ago")?;
        (Direction::Backward, rest.trim())
    };

    let (count, unit) = parse_count_and_unit(middle)?;

    Some(ParsedDuration {
        direction,
        unit,
        count,
    })
}

/// Parse count and unit from strings like "7 days" or "7d".
fn parse_count_and_unit(input: &str) -> Option<(usize, TimeUnit)> {
    let parts: Vec<&str> = input.split_whitespace().collect();

    if parts.len() == 2 {
        // "7 days" format
        let count: usize = parts[0].parse().ok()?;
        let unit = TimeUnit::parse(parts[1])?;
        Some((count, unit))
    } else if parts.len() == 1 {
        // "7d" format - extract number and suffix
        let s = parts[0];
        let num_end = s.find(|c: char| !c.is_ascii_digit())?;
        let count: usize = s[..num_end].parse().ok()?;
        let unit_str = &s[num_end..];
        if unit_str.is_empty() {
            return None;
        }
        let unit = TimeUnit::parse(unit_str)?;
        Some((count, unit))
    } else {
        None
    }
}

/// Parse relative time expressions.
///
/// Past: `yesterday`, `7 days ago`, `7d ago`, `1 week ago`, `1 month ago`, `1 year ago`
/// Present: `today`
/// Future: `tomorrow`, `in 3 days`, `in 7d`, `in 2 weeks`, `next week`, `next month`
fn parse_relative(input: &str, today: Date) -> Option<Date> {
    let input = input.trim().to_lowercase();

    // Handle "today", "yesterday" and "tomorrow"
    if input == "today" {
        return Some(today);
    }
    if input == "yesterday" {
        return Some(today.sub_days(1));
    }
    if input == "tomorrow" {
        return Some(today.add_days(1));
    }

    // Handle "next week" and "next month"
    if input == "next week" {
        return Some(today.add_days(7));
    }
    if input == "next month" {
        return Some(today.add_months(1u32));
    }

    // Handle duration expressions like "in 3 days" or "7 days ago"
    if let Some(duration) = parse_duration(&input) {
        return Some(duration.apply(today));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use crate::date::Weekday;

    const TODAY: &str = "2026-01-13"; // A Tuesday

    fn today() -> Date {
        Date::from_str_unchecked(TODAY)
    }

    // ==========================================================================
    // Period
    // ==========================================================================

    #[test]
    fn period_render_single_day() {
        let period = Period::single_day(today());
        assert_eq!(period.render(), "2026-01-13");
    }

    #[test]
    fn period_render_range() {
        let from = Date::from_str_unchecked("2026-01-01");
        let to = Date::from_str_unchecked("2026-01-31");
        let period = Period::new(from, to);
        assert_eq!(period.render(), "2026-01-01 to 2026-01-31");
    }

    #[rstest]
    #[case::inside_range("2026-01-15", true)]
    #[case::at_from_edge("2026-01-01", true)]
    #[case::at_to_edge("2026-01-31", true)]
    #[case::before_range("2025-12-31", false)]
    #[case::after_range("2026-02-01", false)]
    fn period_contains(#[case] date: &str, #[case] expected: bool) {
        let period = Period::new(
            Date::from_str_unchecked("2026-01-01"),
            Date::from_str_unchecked("2026-01-31"),
        );
        let date = Date::from_str_unchecked(date);
        assert_eq!(period.contains(date), expected);
    }

    // ==========================================================================
    // parse_date
    // ==========================================================================

    #[rstest]
    #[case::iso_date("2025-12-31", "2025-12-31")]
    // Present
    #[case::today("today", "2026-01-13")]
    // Backward dates
    #[case::yesterday("yesterday", "2026-01-12")]
    #[case::one_week_ago("1 week ago", "2026-01-06")]
    #[case::two_weeks_ago("2 weeks ago", "2025-12-30")]
    #[case::one_month_ago("1 month ago", "2025-12-13")]
    #[case::two_months_ago("2 months ago", "2025-11-13")]
    #[case::one_year_ago("1 year ago", "2025-01-13")]
    #[case::two_years_ago("2 years ago", "2024-01-13")]
    #[case::one_y_ago("1y ago", "2025-01-13")]
    #[case::one_day_ago("1 day ago", "2026-01-12")]
    #[case::three_days_ago("3 days ago", "2026-01-10")]
    #[case::seven_d_ago("7d ago", "2026-01-06")]
    #[case::thirty_d_ago("30d ago", "2025-12-14")]
    // Forward dates
    #[case::tomorrow("tomorrow", "2026-01-14")]
    #[case::in_3_days("in 3 days", "2026-01-16")]
    #[case::in_7d("in 7d", "2026-01-20")]
    #[case::in_2_weeks("in 2 weeks", "2026-01-27")]
    #[case::in_1_year("in 1 year", "2027-01-13")]
    #[case::in_2y("in 2y", "2028-01-13")]
    #[case::next_week("next week", "2026-01-20")]
    #[case::next_month("next month", "2026-02-13")]
    fn parse_date_formats(#[case] input: &str, #[case] expected: &str) {
        let expected = Date::from_str_unchecked(expected);
        assert_eq!(parse_date(input, today()), expected);
    }

    #[rstest]
    #[case::bare_7d("7d")]
    #[case::bare_30d("30d")]
    #[case::bare_1d("1d")]
    #[case::year_month("2025-12")] // Year-month is a period, not a date
    fn try_parse_date_returns_error(#[case] input: &str) {
        assert!(try_parse_date(input, today()).is_err());
    }

    #[rstest]
    // "last friday" from Tuesday = 4 days ago (2026-01-09)
    #[case::last_friday("last friday", "2026-01-09")]
    // "last monday" from Tuesday = 1 day ago (2026-01-12)
    #[case::last_monday("last monday", "2026-01-12")]
    // "last sunday" from Tuesday = 2 days ago (2026-01-11)
    #[case::last_sunday("last sunday", "2026-01-11")]
    // "last saturday" from Tuesday = 3 days ago (2026-01-10)
    #[case::last_saturday("last saturday", "2026-01-10")]
    // "last tuesday" from Tuesday = 7 days ago (not today!)
    #[case::last_tuesday("last tuesday", "2026-01-06")]
    // "last wednesday" from Tuesday = 6 days ago (2026-01-07)
    #[case::last_wednesday("last wednesday", "2026-01-07")]
    // "last thursday" from Tuesday = 5 days ago (2026-01-08)
    #[case::last_thursday("last thursday", "2026-01-08")]
    fn parse_date_last_weekday(#[case] input: &str, #[case] expected: &str) {
        let expected = Date::from_str_unchecked(expected);
        assert_eq!(parse_date(input, today()), expected);
    }

    #[rstest]
    // "next friday" from Tuesday = 3 days ahead (2026-01-16)
    #[case::next_friday("next friday", "2026-01-16")]
    // "next monday" from Tuesday = 6 days ahead (2026-01-19)
    #[case::next_monday("next monday", "2026-01-19")]
    // "next sunday" from Tuesday = 5 days ahead (2026-01-18)
    #[case::next_sunday("next sunday", "2026-01-18")]
    // "next saturday" from Tuesday = 4 days ahead (2026-01-17)
    #[case::next_saturday("next saturday", "2026-01-17")]
    // "next tuesday" from Tuesday = 7 days ahead (not today!)
    #[case::next_tuesday("next tuesday", "2026-01-20")]
    // "next wednesday" from Tuesday = 1 day ahead (2026-01-14)
    #[case::next_wednesday("next wednesday", "2026-01-14")]
    // "next thursday" from Tuesday = 2 days ahead (2026-01-15)
    #[case::next_thursday("next thursday", "2026-01-15")]
    fn parse_date_next_weekday(#[case] input: &str, #[case] expected: &str) {
        let expected = Date::from_str_unchecked(expected);
        assert_eq!(parse_date(input, today()), expected);
    }

    #[test]
    fn parse_date_invalid_returns_today() {
        assert_eq!(parse_date("invalid", today()), today());
        assert_eq!(parse_date("", today()), today());
    }

    // ==========================================================================
    // parse_period
    // ==========================================================================

    #[rstest]
    #[case::year_month("2025-12", "2025-12-01", "2025-12-31")]
    #[case::year_month_feb("2025-02", "2025-02-01", "2025-02-28")]
    #[case::year_month_feb_leap("2024-02", "2024-02-01", "2024-02-29")]
    fn parse_period_year_month(
        #[case] input: &str,
        #[case] expected_from: &str,
        #[case] expected_to: &str,
    ) {
        let period = parse_period(input, today());
        assert_eq!(period.from, Date::from_str_unchecked(expected_from));
        assert_eq!(period.to, Date::from_str_unchecked(expected_to));
    }

    #[rstest]
    #[case::q1("2025-Q1", "2025-01-01", "2025-03-31")]
    #[case::q2("2025-Q2", "2025-04-01", "2025-06-30")]
    #[case::q3("2025-Q3", "2025-07-01", "2025-09-30")]
    #[case::q4("2025-Q4", "2025-10-01", "2025-12-31")]
    #[case::lowercase("2025-q1", "2025-01-01", "2025-03-31")]
    fn parse_period_quarter(
        #[case] input: &str,
        #[case] expected_from: &str,
        #[case] expected_to: &str,
    ) {
        let period = parse_period(input, today());
        assert_eq!(period.from, Date::from_str_unchecked(expected_from));
        assert_eq!(period.to, Date::from_str_unchecked(expected_to));
    }

    #[test]
    fn parse_period_this_month() {
        // Today is 2026-01-13
        let period = parse_period("this month", today());
        assert_eq!(period.from, Date::from_str_unchecked("2026-01-01"));
        assert_eq!(period.to, Date::from_str_unchecked("2026-01-31"));
    }

    #[test]
    fn parse_period_last_month() {
        // Today is 2026-01-13, last month is December 2025
        let period = parse_period("last month", today());
        assert_eq!(period.from, Date::from_str_unchecked("2025-12-01"));
        assert_eq!(period.to, Date::from_str_unchecked("2025-12-31"));
    }

    #[test]
    fn parse_period_this_week() {
        // Today is 2026-01-13 (Tuesday)
        // Latest Sunday is 2026-01-11
        // This week: Mon 2026-01-12 to Sun 2026-01-18
        let period = parse_period("this week", today());
        assert_eq!(period.from, Date::from_str_unchecked("2026-01-12"));
        assert_eq!(period.to, Date::from_str_unchecked("2026-01-18"));
    }

    #[test]
    fn parse_period_last_week() {
        // Today is 2026-01-13 (Tuesday)
        // Latest Sunday is 2026-01-11
        // Last week: Mon 2026-01-05 to Sun 2026-01-11
        let period = parse_period("last week", today());
        assert_eq!(period.from, Date::from_str_unchecked("2026-01-05"));
        assert_eq!(period.to, Date::from_str_unchecked("2026-01-11"));
    }

    #[test]
    fn parse_period_yesterday() {
        let period = parse_period("yesterday", today());
        assert_eq!(period.from, Date::from_str_unchecked("2026-01-12"));
        assert_eq!(period.to, Date::from_str_unchecked("2026-01-12"));
    }

    #[test]
    fn parse_period_invalid_returns_today() {
        let period = parse_period("invalid", today());
        assert_eq!(period.from, today());
        assert_eq!(period.to, today());
    }

    // ==========================================================================
    // Date::last_day_of_month (testing via the method we added)
    // ==========================================================================

    #[rstest]
    #[case::january("2025-01-15", "2025-01-31")]
    #[case::february_non_leap("2025-02-10", "2025-02-28")]
    #[case::february_leap("2024-02-10", "2024-02-29")]
    #[case::march("2025-03-01", "2025-03-31")]
    #[case::april("2025-04-30", "2025-04-30")]
    #[case::june("2025-06-15", "2025-06-30")]
    #[case::december("2025-12-01", "2025-12-31")]
    #[case::already_last_day("2025-01-31", "2025-01-31")]
    fn last_day_of_month(#[case] input: &str, #[case] expected: &str) {
        let date = Date::from_str_unchecked(input);
        let expected = Date::from_str_unchecked(expected);
        assert_eq!(date.last_day_of_month(), expected);
    }

    // ==========================================================================
    // Date::last_weekday (testing via the method we added)
    // ==========================================================================

    #[rstest]
    // "last friday" from Tuesday = 4 days ago (2026-01-09)
    #[case::last_friday(Weekday::Friday, "2026-01-09")]
    // "last monday" from Tuesday = 1 day ago (2026-01-12)
    #[case::last_monday(Weekday::Monday, "2026-01-12")]
    // "last sunday" from Tuesday = 2 days ago (2026-01-11)
    #[case::last_sunday(Weekday::Sunday, "2026-01-11")]
    // "last saturday" from Tuesday = 3 days ago (2026-01-10)
    #[case::last_saturday(Weekday::Saturday, "2026-01-10")]
    // "last tuesday" from Tuesday = 7 days ago (not today!)
    #[case::last_tuesday(Weekday::Tuesday, "2026-01-06")]
    // "last wednesday" from Tuesday = 6 days ago (2026-01-07)
    #[case::last_wednesday(Weekday::Wednesday, "2026-01-07")]
    // "last thursday" from Tuesday = 5 days ago (2026-01-08)
    #[case::last_thursday(Weekday::Thursday, "2026-01-08")]
    fn last_weekday(#[case] weekday: Weekday, #[case] expected: &str) {
        let expected = Date::from_str_unchecked(expected);
        assert_eq!(today().last_weekday(weekday), expected);
    }

    // ==========================================================================
    // Date::sub_months (testing via the method we added)
    // ==========================================================================

    #[rstest]
    #[case::one_month("2026-01-13", 1, "2025-12-13")]
    #[case::two_months("2026-01-13", 2, "2025-11-13")]
    #[case::twelve_months("2026-01-13", 12, "2025-01-13")]
    #[case::across_year("2025-03-15", 4, "2024-11-15")]
    // Edge case: Jan 31 - 1 month = Dec 31 (not Jan 31 of prev year)
    #[case::month_end("2025-01-31", 1, "2024-12-31")]
    // Edge case: Mar 31 - 1 month = Feb 28 (Feb doesn't have 31 days)
    #[case::overflow_to_feb("2025-03-31", 1, "2025-02-28")]
    #[case::overflow_to_feb_leap("2024-03-31", 1, "2024-02-29")]
    fn sub_months(#[case] input: &str, #[case] months: u32, #[case] expected: &str) {
        let date = Date::from_str_unchecked(input);
        let expected = Date::from_str_unchecked(expected);
        assert_eq!(date.sub_months(months), expected);
    }

    #[rstest]
    #[case::one_year("2026-01-13", 1, "2025-01-13")]
    #[case::two_years("2026-01-13", 2, "2024-01-13")]
    #[case::ten_years("2026-01-13", 10, "2016-01-13")]
    // Edge case: Feb 29 - 1 year = Feb 28 (non-leap year)
    #[case::leap_day_to_non_leap("2024-02-29", 1, "2023-02-28")]
    // Edge case: Feb 29 - 4 years = Feb 29 (still a leap year)
    #[case::leap_day_to_leap("2024-02-29", 4, "2020-02-29")]
    fn sub_years(#[case] input: &str, #[case] years: u32, #[case] expected: &str) {
        let date = Date::from_str_unchecked(input);
        let expected = Date::from_str_unchecked(expected);
        assert_eq!(date.sub_years(years), expected);
    }
}

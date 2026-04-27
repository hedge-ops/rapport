//! Invariants related to dates are used by the `NaiveDate` which is extended to treat the ISO
//! format such as `2025-01-23` as a date representation in string value.

use std::str::FromStr;

use crate::{Error, recurrence::Interval};

pub use chrono::NaiveDate;
use chrono::{Datelike, Days, Months, TimeZone, Utc};
use nonempty::NonEmpty;
use serde::{Deserialize, Serialize};
use strum::{EnumCount, VariantArray};

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::Display,
)]
#[display("{}", _0)]
#[serde(transparent)]
pub struct Date(NaiveDate);

impl Date {
    #[must_use]
    pub fn into_iso_string(self) -> String {
        self.0.format(ISO_DATE_FORMAT).to_string()
    }

    #[must_use]
    pub fn weekday(self) -> Weekday {
        Weekday::from(Datelike::weekday(&self.0))
    }

    #[must_use]
    pub fn month(self) -> Month {
        Month::from(self.0)
    }

    /// Provides the day of month for this dates
    ///
    /// # Panics
    ///
    /// Panics when `DayOfMonth` is outside of a valid `DayOfMonth` value, which should never happen.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn day_of_month(self) -> DayOfMonth {
        DayOfMonth::from_value(Datelike::day(&self.0))
            .expect("expecting all dates to have a day in a month")
    }

    #[must_use]
    pub fn year(self) -> Year {
        Year(self.0.year())
    }

    #[must_use]
    pub fn day(self) -> u32 {
        self.0.day()
    }

    #[must_use]
    pub fn add_interval_days(self, value: Interval) -> Date {
        self.add_days(value.get() as usize)
    }

    #[must_use]
    pub fn add_days(self, value: usize) -> Date {
        Self(
            self.0
                .checked_add_days(Days::new(value as u64))
                .unwrap_or(self.0),
        )
    }

    #[must_use]
    pub fn sub_days(self, value: usize) -> Date {
        Self(
            self.0
                .checked_sub_days(Days::new(value as u64))
                .unwrap_or(self.0),
        )
    }

    #[must_use]
    pub const fn from_ymd_opt(year: Year, month: u32, day: u32) -> Option<Date> {
        let year: i32 = year.into_i32();
        let inner = NaiveDate::from_ymd_opt(year, month, day);
        match inner {
            Some(inner) => Some(Self(inner)),
            None => None,
        }
    }

    #[must_use]
    pub fn day_after(self) -> Date {
        self.0.succ_opt().map_or(self, Self)
    }

    #[must_use]
    pub fn next_month(self) -> Date {
        self.0.checked_add_months(Months::new(1)).map_or(self, Self)
    }

    #[must_use]
    pub fn first_of_next_month(self) -> Self {
        self.first_of_month()
            .0
            .checked_add_months(Months::new(1))
            .map_or(self, Self)
    }

    #[must_use]
    pub fn first_of_month(self) -> Self {
        self.0.with_day(DayOfMonth::MIN).map_or(self, Self)
    }

    #[must_use]
    pub fn first_of_year(self) -> Self {
        self.first_of_month().0.with_month(1).map_or(self, Self)
    }

    #[must_use]
    pub fn with_day(self, value: DayOfMonth) -> Option<Date> {
        self.0.with_day(value.to_value()).map(Self)
    }

    pub const MIN: Self = Self(NaiveDate::MIN);

    #[must_use]
    pub fn is_valid_date_str(value: &str) -> bool {
        Self::from_str(value).is_ok()
    }

    /// Converts the given string which is expected to be in ISO format (YYYY-mm-dd) into a Date.
    ///
    /// # Panics
    ///
    /// If the given &str is not in the valid format
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn from_str_unchecked(value: &str) -> Date {
        Self::from_str(value)
            .expect("expecting value {value} to be in iso format for it to be parsed")
    }

    /// Returns the absolute number of days between two dates
    #[must_use]
    pub fn days_between(self, other: Date) -> usize {
        let duration = self.0.signed_duration_since(other.0);
        duration.num_days().abs().try_into().unwrap_or_default()
    }

    /// Returns signed difference in days (positive = self is in future, negative = self is in past)
    #[must_use]
    pub fn signed_days_from(self, reference: Date) -> i64 {
        (self.0 - reference.0).num_days()
    }

    /// Get the next business day (Monday-Friday) after this date
    #[must_use]
    pub fn next_business_day(self) -> Self {
        let next_day = self.add_days(1);
        match next_day.weekday() {
            Weekday::Saturday => next_day.add_days(2), // Skip to Monday
            Weekday::Sunday => next_day.add_days(1),   // Skip to Monday
            _ => next_day,                             // Weekday, so use it
        }
    }

    #[must_use]
    pub fn latest_sunday(self) -> Self {
        match self.weekday() {
            Weekday::Monday => self.sub_days(1),
            Weekday::Tuesday => self.sub_days(2),
            Weekday::Wednesday => self.sub_days(3),
            Weekday::Thursday => self.sub_days(4),
            Weekday::Friday => self.sub_days(5),
            Weekday::Saturday => self.sub_days(6),
            Weekday::Sunday => self,
        }
    }

    #[must_use]
    pub fn next_sunday(self) -> Self {
        let days_until_sunday = match self.weekday() {
            Weekday::Monday => 6,
            Weekday::Tuesday => 5,
            Weekday::Wednesday => 4,
            Weekday::Thursday => 3,
            Weekday::Friday => 2,
            Weekday::Saturday => 1,
            Weekday::Sunday => 7, // If today is Sunday, get next Sunday
        };
        self.add_days(days_until_sunday)
    }

    #[must_use]
    pub fn next_saturday(self) -> Self {
        let days_until_saturday = match self.weekday() {
            Weekday::Monday => 5,
            Weekday::Tuesday => 4,
            Weekday::Wednesday => 3,
            Weekday::Thursday => 2,
            Weekday::Friday => 1,
            Weekday::Saturday => 7, // If today is Saturday, get next Saturday
            Weekday::Sunday => 6,
        };
        self.add_days(days_until_saturday)
    }

    #[must_use]
    pub fn soonest_saturday(self) -> Self {
        if self.weekday() == Weekday::Saturday {
            self
        } else {
            self.next_saturday()
        }
    }

    /// Get the next Monday after this date (or next Monday if today is Monday)
    #[must_use]
    pub fn next_monday(self) -> Self {
        let days_until_monday = match self.weekday() {
            Weekday::Monday => 7, // If today is Monday, get next Monday
            Weekday::Tuesday => 6,
            Weekday::Wednesday => 5,
            Weekday::Thursday => 4,
            Weekday::Friday => 3,
            Weekday::Saturday => 2,
            Weekday::Sunday => 1,
        };
        self.add_days(days_until_monday)
    }

    /// Add the specified number of months to this date
    #[must_use]
    pub fn add_months(self, months: impl Into<u32>) -> Self {
        use chrono::Months;

        let naive_date: chrono::NaiveDate = self.into();
        let result = naive_date
            .checked_add_months(Months::new(months.into()))
            .unwrap_or(naive_date);

        Self::from(result)
    }

    /// Add the specified number of years to this date
    #[must_use]
    pub fn add_years(self, years: u32) -> Self {
        let years = years.try_into().unwrap_or(i32::MAX);
        let new_year = self.year().saturating_add(years);
        Self::from_ymd_opt(new_year, self.month().to_month_number(), self.day()).unwrap_or_else(
            || {
                // Handle Feb 29 on non-leap years by going to Feb 28
                Self::from_ymd_opt(new_year, self.month().to_month_number(), 28).unwrap_or(self)
            },
        )
    }

    /// Subtract the specified number of years from this date
    #[must_use]
    pub fn sub_years(self, years: u32) -> Self {
        let years = years.try_into().unwrap_or(i32::MAX);
        let new_year = self.year().saturating_sub(years);
        Self::from_ymd_opt(new_year, self.month().to_month_number(), self.day()).unwrap_or_else(
            || {
                // Handle Feb 29 on non-leap years by going to Feb 28
                Self::from_ymd_opt(new_year, self.month().to_month_number(), 28).unwrap_or(self)
            },
        )
    }

    /// Subtract the specified number of months from this date.
    #[must_use]
    pub fn sub_months(self, months: u32) -> Self {
        let naive_date: NaiveDate = self.into();
        let result = naive_date
            .checked_sub_months(Months::new(months))
            .unwrap_or(naive_date);
        Self::from(result)
    }

    /// Returns the most recent occurrence of the given weekday.
    /// If today is that weekday, returns the *previous* occurrence (not today).
    #[must_use]
    pub fn last_weekday(self, weekday: Weekday) -> Self {
        let today_weekday = self.weekday();
        let days_back = if today_weekday == weekday {
            7 // If today is the target weekday, go back a full week
        } else {
            let today_num = today_weekday.num_days_from_monday();
            let target_num = weekday.num_days_from_monday();
            if today_num > target_num {
                today_num - target_num
            } else {
                7 - (target_num - today_num)
            }
        };
        self.sub_days(days_back)
    }

    /// Returns the next occurrence of the given weekday.
    /// If today is that weekday, returns the *next* occurrence (not today).
    #[must_use]
    pub fn next_weekday(self, weekday: Weekday) -> Self {
        weekday.next_occurrence_after(self)
    }

    /// Returns the last day of the current month.
    #[must_use]
    pub fn last_day_of_month(self) -> Self {
        self.first_of_next_month().sub_days(1)
    }

    /// Returns the last day of the next month.
    #[must_use]
    pub fn last_day_of_next_month(self) -> Self {
        self.first_of_next_month().last_day_of_month()
    }

    /// Get January 1st of the next year
    #[must_use]
    pub fn next_january_first(self) -> Self {
        let next_year = self.year().next();
        Self::from_ymd_opt(next_year, 1, 1).unwrap_or(self)
    }

    /// January first of this year.
    #[must_use]
    pub fn january_first(year: Year) -> Option<Self> {
        Self::from_ymd_opt(year, 1, 1)
    }

    #[must_use]
    pub fn into_naive_date(self) -> NaiveDate {
        self.0
    }

    #[must_use]
    pub fn from_naive_date(value: NaiveDate) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn year_month(self) -> YearMonth {
        YearMonth::new(self.year(), self.month())
    }

    #[must_use]
    pub fn end_of_day_utc(self) -> chrono::DateTime<Utc> {
        Utc.from_utc_datetime(&self.0.and_hms_opt(23, 59, 59).unwrap_or_default())
    }

    #[must_use]
    pub fn start_of_day_utc(self) -> chrono::DateTime<Utc> {
        Utc.from_utc_datetime(&self.0.and_hms_opt(0, 0, 0).unwrap_or_default())
    }

    #[must_use]
    pub fn today() -> Self {
        Self::from(chrono::Local::now().date_naive())
    }

    #[must_use]
    pub fn last_day_of_previous_month(self) -> Self {
        self.first_of_month().sub_days(1)
    }
}

impl FromStr for Date {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        NaiveDate::parse_from_str(s, ISO_DATE_FORMAT)
            .map(Self)
            .map_err(|_| Error::InvalidDate)
    }
}

impl From<NaiveDate> for Date {
    fn from(value: NaiveDate) -> Self {
        Self(value)
    }
}

impl From<Date> for NaiveDate {
    fn from(value: Date) -> Self {
        value.into_naive_date()
    }
}

impl std::ops::Sub for Date {
    type Output = crate::time::Duration;

    fn sub(self, other: Date) -> Self::Output {
        let chrono_duration = self.0.signed_duration_since(other.0);
        let abs_duration = chrono_duration.abs();

        // Try to get nanoseconds first for precision
        if let Some(nanos) = abs_duration.num_nanoseconds() {
            crate::time::Duration {
                nanos: nanos.abs().try_into().unwrap_or_default(),
            }
        } else {
            // For very large durations, fall back to seconds
            let secs = abs_duration
                .num_seconds()
                .abs()
                .try_into()
                .unwrap_or_default();
            crate::time::Duration::from_secs(secs)
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::Display,
)]
#[serde(transparent)]
#[display("{_0}")]
pub struct Year(i32);

impl Year {
    pub(crate) fn saturating_add(self, value: i32) -> Self {
        Self(self.0.saturating_add(value))
    }

    pub(crate) fn saturating_sub(self, value: i32) -> Self {
        Self(self.0.saturating_sub(value))
    }

    #[must_use]
    pub fn previous(self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    pub(crate) fn next(self) -> Self {
        self.saturating_add(1)
    }

    pub(crate) const fn into_i32(self) -> i32 {
        self.0
    }
}

impl From<i32> for Year {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<Year> for i32 {
    fn from(value: Year) -> Self {
        value.0
    }
}

const ISO_DATE_FORMAT: &str = "%Y-%m-%d";

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    derive_more::Display,
    derive_more::FromStr,
    strum::VariantArray,
    strum::EnumCount,
)]
pub enum Weekday {
    #[display("Monday")]
    Monday,
    #[display("Tuesday")]
    Tuesday,
    #[display("Wednesday")]
    Wednesday,
    #[display("Thursday")]
    Thursday,
    #[display("Friday")]
    Friday,
    #[display("Saturday")]
    Saturday,
    #[display("Sunday")]
    Sunday,
}

impl Weekday {
    pub(crate) fn next_occurrence_after(self, date: Date) -> Date {
        let after_weekday = date.weekday();

        let days_to_add = if after_weekday == self {
            // If today is the target day, return next week's occurrence
            Self::COUNT
        } else {
            let after_num_days = after_weekday.num_days_from_monday();
            let self_num_days = self.num_days_from_monday();

            if self_num_days > after_num_days {
                // target is later this week
                self_num_days - after_num_days
            } else {
                // target is next week
                Self::COUNT - after_num_days + self_num_days
            }
        };

        date.add_days(days_to_add)
    }

    /// Returns the number of days from Monday (Monday = 0, Sunday = 6).
    #[must_use]
    pub fn num_days_from_monday(self) -> usize {
        match self {
            Weekday::Monday => 0,
            Weekday::Tuesday => 1,
            Weekday::Wednesday => 2,
            Weekday::Thursday => 3,
            Weekday::Friday => 4,
            Weekday::Saturday => 5,
            Weekday::Sunday => 6,
        }
    }

    pub(crate) fn next_occurrence_after_days(days: &NonEmpty<Self>, date: Date) -> Date {
        days.iter()
            .map(|day| day.next_occurrence_after(date))
            .min()
            .unwrap_or(date)
    }

    pub(crate) fn is_weekend(self) -> bool {
        self == Weekday::Saturday || self == Weekday::Sunday
    }

    pub(crate) fn is_weekday(self) -> bool {
        !self.is_weekend()
    }

    #[must_use]
    pub fn first_initial(&self) -> String {
        let initial = match self {
            Weekday::Monday => "M",
            Weekday::Tuesday | Weekday::Thursday => "T",
            Weekday::Wednesday => "W",
            Weekday::Friday => "F",
            Weekday::Saturday | Weekday::Sunday => "S",
        };
        initial.to_owned()
    }
}

impl From<Weekday> for NonEmpty<Weekday> {
    fn from(value: Weekday) -> Self {
        NonEmpty::singleton(value)
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::Display,
    derive_more::FromStr,
    strum::VariantArray,
)]
pub enum Month {
    #[display("January")]
    January,
    #[display("February")]
    February,
    #[display("March")]
    March,
    #[display("April")]
    April,
    #[display("May")]
    May,
    #[display("June")]
    June,
    #[display("July")]
    July,
    #[display("August")]
    August,
    #[display("September")]
    September,
    #[display("October")]
    October,
    #[display("November")]
    November,
    #[display("December")]
    December,
}

impl Month {
    #[must_use]
    pub fn to_month_number(self) -> u32 {
        match self {
            Month::January => 1,
            Month::February => 2,
            Month::March => 3,
            Month::April => 4,
            Month::May => 5,
            Month::June => 6,
            Month::July => 7,
            Month::August => 8,
            Month::September => 9,
            Month::October => 10,
            Month::November => 11,
            Month::December => 12,
        }
    }

    /// Convert month number (1-12) to Month enum.
    #[must_use]
    pub fn from_number(num: u32) -> Option<Self> {
        match num {
            1 => Some(Month::January),
            2 => Some(Month::February),
            3 => Some(Month::March),
            4 => Some(Month::April),
            5 => Some(Month::May),
            6 => Some(Month::June),
            7 => Some(Month::July),
            8 => Some(Month::August),
            9 => Some(Month::September),
            10 => Some(Month::October),
            11 => Some(Month::November),
            12 => Some(Month::December),
            _ => None,
        }
    }

    #[must_use]
    pub fn short_description(&self) -> &'static str {
        match self {
            Month::January => "Jan",
            Month::February => "Feb",
            Month::March => "Mar",
            Month::April => "Apr",
            Month::May => "May",
            Month::June => "Jun",
            Month::July => "Jul",
            Month::August => "Aug",
            Month::September => "Sep",
            Month::October => "Oct",
            Month::November => "Nov",
            Month::December => "Dec",
        }
    }

    #[must_use]
    pub fn months_until(value: Self) -> Vec<Self> {
        let mut months = Vec::new();
        for month in Month::VARIANTS {
            months.push(*month);
            if month == &value {
                break;
            }
        }
        months
    }

    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Month::January => Month::February,
            Month::February => Month::March,
            Month::March => Month::April,
            Month::April => Month::May,
            Month::May => Month::June,
            Month::June => Month::July,
            Month::July => Month::August,
            Month::August => Month::September,
            Month::September => Month::October,
            Month::October => Month::November,
            Month::November => Month::December,
            Month::December => Month::January,
        }
    }
}

impl From<Month> for NonEmpty<Month> {
    fn from(value: Month) -> Self {
        NonEmpty::singleton(value)
    }
}

impl From<NaiveDate> for Month {
    fn from(value: NaiveDate) -> Self {
        match value.month0() {
            0 => Month::January,
            1 => Month::February,
            2 => Month::March,
            3 => Month::April,
            4 => Month::May,
            5 => Month::June,
            6 => Month::July,
            7 => Month::August,
            8 => Month::September,
            9 => Month::October,
            10 => Month::November,
            11 => Month::December,
            _ => unreachable!("Invalid month index from NaiveDate"),
        }
    }
}

impl From<Weekday> for chrono::Weekday {
    fn from(value: Weekday) -> chrono::Weekday {
        match value {
            Weekday::Monday => chrono::Weekday::Mon,
            Weekday::Tuesday => chrono::Weekday::Tue,
            Weekday::Wednesday => chrono::Weekday::Wed,
            Weekday::Thursday => chrono::Weekday::Thu,
            Weekday::Friday => chrono::Weekday::Fri,
            Weekday::Saturday => chrono::Weekday::Sat,
            Weekday::Sunday => chrono::Weekday::Sun,
        }
    }
}

impl From<chrono::Weekday> for Weekday {
    fn from(value: chrono::Weekday) -> Self {
        match value {
            chrono::Weekday::Mon => Self::Monday,
            chrono::Weekday::Tue => Self::Tuesday,
            chrono::Weekday::Wed => Self::Wednesday,
            chrono::Weekday::Thu => Self::Thursday,
            chrono::Weekday::Fri => Self::Friday,
            chrono::Weekday::Sat => Self::Saturday,
            chrono::Weekday::Sun => Self::Sunday,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    derive_more::Display,
    strum::VariantArray,
)]
pub enum DayOfMonth {
    #[display("1st")]
    First,
    #[display("2nd")]
    Second,
    #[display("3rd")]
    Third,
    #[display("4th")]
    Fourth,
    #[display("5th")]
    Fifth,
    #[display("6th")]
    Sixth,
    #[display("7th")]
    Seventh,
    #[display("8th")]
    Eighth,
    #[display("9th")]
    Ninth,
    #[display("10th")]
    Tenth,
    #[display("11th")]
    Eleventh,
    #[display("12th")]
    Twelfth,
    #[display("13th")]
    Thirteenth,
    #[display("14th")]
    Fourteenth,
    #[display("15th")]
    Fifteenth,
    #[display("16th")]
    Sixteenth,
    #[display("17th")]
    Seventeenth,
    #[display("18th")]
    Eighteenth,
    #[display("19th")]
    Nineteenth,
    #[display("20th")]
    Twentieth,
    #[display("21st")]
    TwentyFirst,
    #[display("22nd")]
    TwentySecond,
    #[display("23rd")]
    TwentyThird,
    #[display("24th")]
    TwentyFourth,
    #[display("25th")]
    TwentyFifth,
    #[display("26th")]
    TwentySixth,
    #[display("27th")]
    TwentySeventh,
    #[display("28th")]
    TwentyEighth,
    #[display("29th")]
    TwentyNinth,
    #[display("30th")]
    Thirtieth,
    #[display("31st")]
    ThirtyFirst,
}

impl DayOfMonth {
    pub(crate) const MIN: u32 = 1;
    const MAX: u32 = 31;

    pub(crate) fn from_value(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::First),
            2 => Some(Self::Second),
            3 => Some(Self::Third),
            4 => Some(Self::Fourth),
            5 => Some(Self::Fifth),
            6 => Some(Self::Sixth),
            7 => Some(Self::Seventh),
            8 => Some(Self::Eighth),
            9 => Some(Self::Ninth),
            10 => Some(Self::Tenth),
            11 => Some(Self::Eleventh),
            12 => Some(Self::Twelfth),
            13 => Some(Self::Thirteenth),
            14 => Some(Self::Fourteenth),
            15 => Some(Self::Fifteenth),
            16 => Some(Self::Sixteenth),
            17 => Some(Self::Seventeenth),
            18 => Some(Self::Eighteenth),
            19 => Some(Self::Nineteenth),
            20 => Some(Self::Twentieth),
            21 => Some(Self::TwentyFirst),
            22 => Some(Self::TwentySecond),
            23 => Some(Self::TwentyThird),
            24 => Some(Self::TwentyFourth),
            25 => Some(Self::TwentyFifth),
            26 => Some(Self::TwentySixth),
            27 => Some(Self::TwentySeventh),
            28 => Some(Self::TwentyEighth),
            29 => Some(Self::TwentyNinth),
            30 => Some(Self::Thirtieth),
            31 => Some(Self::ThirtyFirst),
            _ => None,
        }
    }

    #[must_use]
    pub fn to_value(self) -> u32 {
        match self {
            Self::First => 1,
            Self::Second => 2,
            Self::Third => 3,
            Self::Fourth => 4,
            Self::Fifth => 5,
            Self::Sixth => 6,
            Self::Seventh => 7,
            Self::Eighth => 8,
            Self::Ninth => 9,
            Self::Tenth => 10,
            Self::Eleventh => 11,
            Self::Twelfth => 12,
            Self::Thirteenth => 13,
            Self::Fourteenth => 14,
            Self::Fifteenth => 15,
            Self::Sixteenth => 16,
            Self::Seventeenth => 17,
            Self::Eighteenth => 18,
            Self::Nineteenth => 19,
            Self::Twentieth => 20,
            Self::TwentyFirst => 21,
            Self::TwentySecond => 22,
            Self::TwentyThird => 23,
            Self::TwentyFourth => 24,
            Self::TwentyFifth => 25,
            Self::TwentySixth => 26,
            Self::TwentySeventh => 27,
            Self::TwentyEighth => 28,
            Self::TwentyNinth => 29,
            Self::Thirtieth => 30,
            Self::ThirtyFirst => 31,
        }
    }

    pub(crate) fn range() -> std::ops::RangeInclusive<u32> {
        Self::MIN..=Self::MAX
    }
}

impl From<DayOfMonth> for NonEmpty<DayOfMonth> {
    fn from(value: DayOfMonth) -> Self {
        NonEmpty::singleton(value)
    }
}

impl std::str::FromStr for DayOfMonth {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1st" => Ok(Self::First),
            "2nd" => Ok(Self::Second),
            "3rd" => Ok(Self::Third),
            "4th" => Ok(Self::Fourth),
            "5th" => Ok(Self::Fifth),
            "6th" => Ok(Self::Sixth),
            "7th" => Ok(Self::Seventh),
            "8th" => Ok(Self::Eighth),
            "9th" => Ok(Self::Ninth),
            "10th" => Ok(Self::Tenth),
            "11th" => Ok(Self::Eleventh),
            "12th" => Ok(Self::Twelfth),
            "13th" => Ok(Self::Thirteenth),
            "14th" => Ok(Self::Fourteenth),
            "15th" => Ok(Self::Fifteenth),
            "16th" => Ok(Self::Sixteenth),
            "17th" => Ok(Self::Seventeenth),
            "18th" => Ok(Self::Eighteenth),
            "19th" => Ok(Self::Nineteenth),
            "20th" => Ok(Self::Twentieth),
            "21st" => Ok(Self::TwentyFirst),
            "22nd" => Ok(Self::TwentySecond),
            "23rd" => Ok(Self::TwentyThird),
            "24th" => Ok(Self::TwentyFourth),
            "25th" => Ok(Self::TwentyFifth),
            "26th" => Ok(Self::TwentySixth),
            "27th" => Ok(Self::TwentySeventh),
            "28th" => Ok(Self::TwentyEighth),
            "29th" => Ok(Self::TwentyNinth),
            "30th" => Ok(Self::Thirtieth),
            "31st" => Ok(Self::ThirtyFirst),
            _ => Err(crate::Error::InvalidDate),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::Display,
)]
#[display("{year}-{month}")]
pub struct YearMonth {
    year: Year,
    month: Month,
}

impl YearMonth {
    pub fn new(year: impl Into<Year>, month: Month) -> Self {
        let year = year.into();
        Self { year, month }
    }

    #[must_use]
    pub fn year(self) -> Year {
        self.year
    }

    #[must_use]
    pub fn month(self) -> Month {
        self.month
    }

    #[must_use]
    pub fn first_day(self) -> Date {
        Date::from_ymd_opt(self.year, self.month.to_month_number(), 1).unwrap_or_default()
    }

    /// Last day of this month.
    #[must_use]
    pub fn last_day(self) -> Date {
        // Go to first of next month, then subtract one day
        self.first_day().first_of_next_month().sub_days(1)
    }

    #[must_use]
    pub fn to_numeric_string(self) -> String {
        format!(
            "{}-{:02}",
            i32::from(self.year),
            self.month.to_month_number()
        )
    }

    /// Parse a year-month string like "2025-01" or "2025-12".
    #[must_use]
    pub fn parse(input: &str) -> Option<YearMonth> {
        let input = input.trim();
        let parts: Vec<&str> = input.split('-').collect();
        if parts.len() != 2 {
            return None;
        }

        let year: i32 = parts[0].parse().ok()?;
        let month_num: u32 = parts[1].parse().ok()?;
        let month = Month::from_number(month_num)?;

        Some(YearMonth::new(year, month))
    }
}

impl From<Date> for YearMonth {
    fn from(value: Date) -> Self {
        value.year_month()
    }
}

/// A calendar quarter (Q1-Q4).
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::Display,
    strum::VariantArray,
)]
pub enum Quarter {
    #[display("Q1")]
    Q1,
    #[display("Q2")]
    Q2,
    #[display("Q3")]
    Q3,
    #[display("Q4")]
    Q4,
}

impl Quarter {
    /// The starting month of this quarter.
    #[must_use]
    pub fn first_month(self) -> Month {
        match self {
            Quarter::Q1 => Month::January,
            Quarter::Q2 => Month::April,
            Quarter::Q3 => Month::July,
            Quarter::Q4 => Month::October,
        }
    }

    /// The ending month of this quarter.
    #[must_use]
    pub fn last_month(self) -> Month {
        match self {
            Quarter::Q1 => Month::March,
            Quarter::Q2 => Month::June,
            Quarter::Q3 => Month::September,
            Quarter::Q4 => Month::December,
        }
    }

    /// Parse a quarter number (1-4) into a Quarter.
    #[must_use]
    pub fn from_number(num: u8) -> Option<Self> {
        match num {
            1 => Some(Quarter::Q1),
            2 => Some(Quarter::Q2),
            3 => Some(Quarter::Q3),
            4 => Some(Quarter::Q4),
            _ => None,
        }
    }

    /// Get the quarter number (1-4).
    #[must_use]
    pub fn to_number(self) -> u8 {
        match self {
            Quarter::Q1 => 1,
            Quarter::Q2 => 2,
            Quarter::Q3 => 3,
            Quarter::Q4 => 4,
        }
    }
}

/// A calendar quarter in a specific year.
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::Display,
)]
#[display("{year}-{quarter}")]
pub struct YearQuarter {
    year: Year,
    quarter: Quarter,
}

impl YearQuarter {
    #[must_use]
    pub fn new(year: impl Into<Year>, quarter: Quarter) -> Self {
        Self {
            year: year.into(),
            quarter,
        }
    }

    #[must_use]
    pub fn year(self) -> Year {
        self.year
    }

    #[must_use]
    pub fn quarter(self) -> Quarter {
        self.quarter
    }

    /// First day of the quarter.
    #[must_use]
    pub fn first_day(self) -> Date {
        Date::from_ymd_opt(self.year, self.quarter.first_month().to_month_number(), 1)
            .unwrap_or_default()
    }

    /// Last day of the quarter.
    #[must_use]
    pub fn last_day(self) -> Date {
        let (month, day) = match self.quarter {
            Quarter::Q1 => (3, 31),  // March 31
            Quarter::Q2 => (6, 30),  // June 30
            Quarter::Q3 => (9, 30),  // September 30
            Quarter::Q4 => (12, 31), // December 31
        };
        Date::from_ymd_opt(self.year, month, day).unwrap_or_default()
    }

    /// Parse a year-quarter string like "2025-Q1" or "2025-q1".
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        let parts: Vec<&str> = input.split('-').collect();
        if parts.len() != 2 {
            return None;
        }

        let year: i32 = parts[0].parse().ok()?;
        let quarter_str = parts[1].to_lowercase();
        let quarter_num: u8 = quarter_str.strip_prefix('q')?.parse().ok()?;
        let quarter = Quarter::from_number(quarter_num)?;

        Some(Self::new(year, quarter))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use tracing_test::traced_test;

    const SAMPLE_DATE_STR: &str = "2025-01-30";
    static SAMPLE_DATE: Date = assert_some!(Date::from_ymd_opt(Year(2025), 1, 30));

    #[test]
    fn test_to_iso_string() {
        assert_eq!(SAMPLE_DATE.into_iso_string(), SAMPLE_DATE_STR);
        assert!(Date::is_valid_date_str(SAMPLE_DATE_STR));
    }

    #[test]
    fn test_try_from_iso_string_valid() {
        let result = assert_ok!(Date::from_str(SAMPLE_DATE_STR));
        assert_eq!(result, SAMPLE_DATE);
    }

    #[test]
    fn test_try_from_iso_string_invalid() {
        let invalid_date_str = "January 1, 2025"; // not the iso format
        assert_err!(Date::from_str(invalid_date_str));
        assert!(!Date::is_valid_date_str(invalid_date_str));
    }

    #[rstest]
    #[case::same_day("2025-06-07", Weekday::Saturday, "2025-06-14")]
    #[case::next_day("2025-06-07", Weekday::Sunday, "2025-06-08")]
    #[case::monday("2025-06-07", Weekday::Monday, "2025-06-09")]
    #[case::tuesday("2025-06-07", Weekday::Tuesday, "2025-06-10")]
    #[case::wednesday("2025-06-07", Weekday::Wednesday, "2025-06-11")]
    #[case::thursday("2025-06-07", Weekday::Thursday, "2025-06-12")]
    #[case::friday("2025-06-07", Weekday::Friday, "2025-06-13")]
    #[case::next_month("2025-06-30", Weekday::Friday, "2025-07-04")]
    #[case::next_year("2025-12-29", Weekday::Friday, "2026-01-02")]
    fn weekday_next_occurrence_after(
        #[case] from: &str,
        #[case] day: Weekday,
        #[case] expected: &str,
    ) {
        let from = Date::from_str_unchecked(from);
        let expected = Date::from_str_unchecked(expected);
        let actual = day.next_occurrence_after(from);

        assert_eq!(actual, expected);
    }

    #[traced_test]
    #[rstest]
    #[case::monday(Weekday::Monday, true)]
    #[case::tuesday(Weekday::Tuesday, true)]
    #[case::wednesday(Weekday::Wednesday, true)]
    #[case::thursday(Weekday::Thursday, true)]
    #[case::friday(Weekday::Friday, true)]
    #[case::saturday(Weekday::Saturday, false)]
    #[case::sunday(Weekday::Sunday, false)]
    fn weekday_is_weekday(#[case] input: Weekday, #[case] expected: bool) {
        let actual_weekday = input.is_weekday();
        let actual_weekend = input.is_weekend();
        assert_eq!(
            actual_weekday, expected,
            "expecting {input} weekday to be {expected}"
        );
        assert_eq!(
            actual_weekend, !expected,
            "expecting {input} is weekend to {}",
            !expected
        );
    }

    #[rstest]
    #[case::january("2025-01-15", Month::January)]
    #[case::february("2025-02-28", Month::February)]
    #[case::march("2025-03-10", Month::March)]
    #[case::april("2025-04-01", Month::April)]
    #[case::may("2025-05-31", Month::May)]
    #[case::june("2025-06-15", Month::June)]
    #[case::july("2025-07-04", Month::July)]
    #[case::august("2025-08-20", Month::August)]
    #[case::september("2025-09-30", Month::September)]
    #[case::october("2025-10-12", Month::October)]
    #[case::november("2025-11-25", Month::November)]
    #[case::december("2025-12-31", Month::December)]
    #[case::january_different_year("2024-01-01", Month::January)]
    #[case::february_leap_year("2024-02-29", Month::February)]
    #[case::december_different_year("2026-12-01", Month::December)]
    fn date_has_month(#[case] date: &str, #[case] expected: Month) {
        let date = Date::from_str_unchecked(date);

        let actual = date.month();

        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::first("1st", DayOfMonth::First)]
    #[case::second("2nd", DayOfMonth::Second)]
    #[case::third("3rd", DayOfMonth::Third)]
    #[case::fourth("4th", DayOfMonth::Fourth)]
    #[case::fifth("5th", DayOfMonth::Fifth)]
    #[case::sixth("6th", DayOfMonth::Sixth)]
    #[case::seventh("7th", DayOfMonth::Seventh)]
    #[case::eighth("8th", DayOfMonth::Eighth)]
    #[case::ninth("9th", DayOfMonth::Ninth)]
    #[case::tenth("10th", DayOfMonth::Tenth)]
    #[case::eleventh("11th", DayOfMonth::Eleventh)]
    #[case::twelfth("12th", DayOfMonth::Twelfth)]
    #[case::thirteenth("13th", DayOfMonth::Thirteenth)]
    #[case::fourteenth("14th", DayOfMonth::Fourteenth)]
    #[case::fifteenth("15th", DayOfMonth::Fifteenth)]
    #[case::sixteenth("16th", DayOfMonth::Sixteenth)]
    #[case::seventeenth("17th", DayOfMonth::Seventeenth)]
    #[case::eighteenth("18th", DayOfMonth::Eighteenth)]
    #[case::nineteenth("19th", DayOfMonth::Nineteenth)]
    #[case::twentieth("20th", DayOfMonth::Twentieth)]
    #[case::twenty_first("21st", DayOfMonth::TwentyFirst)]
    #[case::twenty_second("22nd", DayOfMonth::TwentySecond)]
    #[case::twenty_third("23rd", DayOfMonth::TwentyThird)]
    #[case::twenty_fourth("24th", DayOfMonth::TwentyFourth)]
    #[case::twenty_fifth("25th", DayOfMonth::TwentyFifth)]
    #[case::twenty_sixth("26th", DayOfMonth::TwentySixth)]
    #[case::twenty_seventh("27th", DayOfMonth::TwentySeventh)]
    #[case::twenty_eighth("28th", DayOfMonth::TwentyEighth)]
    #[case::twenty_ninth("29th", DayOfMonth::TwentyNinth)]
    #[case::thirtieth("30th", DayOfMonth::Thirtieth)]
    #[case::thirty_first("31st", DayOfMonth::ThirtyFirst)]
    fn day_of_month_parses_and_prints(#[case] input: &str, #[case] expected: DayOfMonth) {
        let actual = assert_ok!(DayOfMonth::from_str(input));

        assert_eq!(
            actual, expected,
            "expecting {input} to parse as expected {expected}"
        );
        assert_eq!(
            expected.to_string(),
            input,
            "expecting day of month to print back out to the same as it was entered in"
        );
    }

    #[rstest]
    #[case::same_date("2025-01-30", "2025-01-30", 0)]
    #[case::one_day_apart("2025-01-31", "2025-01-30", 24 * 60 * 60)] // 1 day in seconds
    #[case::three_days_apart("2025-02-02", "2025-01-30", 3 * 24 * 60 * 60)] // 3 days in seconds
    #[case::reversed_order("2025-01-30", "2025-02-02", 3 * 24 * 60 * 60)] // should be absolute difference
    fn date_subtraction_yields_duration(
        #[case] date1: &str,
        #[case] date2: &str,
        #[case] expected_seconds: u64,
    ) {
        let date1 = Date::from_str_unchecked(date1);
        let date2 = Date::from_str_unchecked(date2);

        let duration = date1 - date2;

        assert_eq!(
            duration.as_secs(),
            expected_seconds,
            "expected duration of {} seconds between {} and {}",
            expected_seconds,
            date1,
            date2
        );
    }

    #[rstest]
    #[case("2025-01-01", "2025-01-01")]
    #[case("2025-01-02", "2025-01-01")]
    #[case("2025-06-30", "2025-01-01")]
    #[case("2025-12-31", "2025-01-01")]
    #[case("2026-01-01", "2026-01-01")]
    fn first_of_year(#[case] date: &str, #[case] expected: &str) {
        let date = Date::from_str_unchecked(date);
        let expected = Date::from_str_unchecked(expected);
        let actual = date.first_of_year();
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::january(Month::January, vec![Month::January])]
    #[case::february(Month::February, vec![Month::January, Month::February])]
    #[case::march(Month::March, vec![Month::January, Month::February, Month::March])]
    #[case::april(Month::April, vec![Month::January, Month::February, Month::March, Month::April])]
    #[case::may(Month::May, vec![Month::January, Month::February, Month::March, Month::April, Month::May])]
    #[case::june(Month::June, vec![Month::January, Month::February, Month::March, Month::April, Month::May, Month::June])]
    #[case::july(Month::July, vec![Month::January, Month::February, Month::March, Month::April, Month::May, Month::June, Month::July])]
    #[case::august(Month::August, vec![Month::January, Month::February, Month::March, Month::April, Month::May, Month::June, Month::July, Month::August])]
    #[case::september(Month::September, vec![Month::January, Month::February, Month::March, Month::April, Month::May, Month::June, Month::July, Month::August, Month::September])]
    #[case::october(Month::October, vec![Month::January, Month::February, Month::March, Month::April, Month::May, Month::June, Month::July, Month::August, Month::September, Month::October])]
    #[case::november(Month::November, vec![Month::January, Month::February, Month::March, Month::April, Month::May, Month::June, Month::July, Month::August, Month::September, Month::October, Month::November])]
    #[case::december(Month::December, vec![Month::January, Month::February, Month::March, Month::April, Month::May, Month::June, Month::July, Month::August, Month::September, Month::October, Month::November, Month::December])]
    fn months_until(#[case] value: Month, #[case] expected: Vec<Month>) {
        let actual = Month::months_until(value);
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::january(YearMonth::new(Year(2025), Month::January), "2025-01-01")]
    #[case::february(YearMonth::new(Year(2025), Month::February), "2025-02-01")]
    #[case::march(YearMonth::new(Year(2025), Month::March), "2025-03-01")]
    #[case::april(YearMonth::new(Year(2025), Month::April), "2025-04-01")]
    #[case::may(YearMonth::new(Year(2025), Month::May), "2025-05-01")]
    #[case::june(YearMonth::new(Year(2025), Month::June), "2025-06-01")]
    #[case::july(YearMonth::new(Year(2025), Month::July), "2025-07-01")]
    #[case::august(YearMonth::new(Year(2025), Month::August), "2025-08-01")]
    #[case::september(YearMonth::new(Year(2025), Month::September), "2025-09-01")]
    #[case::october(YearMonth::new(Year(2025), Month::October), "2025-10-01")]
    #[case::november(YearMonth::new(Year(2025), Month::November), "2025-11-01")]
    #[case::december(YearMonth::new(Year(2025), Month::December), "2025-12-01")]
    fn first_day_of_month(#[case] year_month: YearMonth, #[case] expected: &str) {
        let actual = year_month.first_day();
        assert_eq!(actual.into_iso_string(), expected);

        let actual = actual.year_month();
        assert_eq!(actual, year_month);
    }

    #[test]
    fn next_year() {
        let starting = Year(2024);
        let expected = Year(2025);
        let actual = starting.next();
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::january(Month::January, Month::February)]
    #[case::february(Month::February, Month::March)]
    #[case::march(Month::March, Month::April)]
    #[case::april(Month::April, Month::May)]
    #[case::may(Month::May, Month::June)]
    #[case::june(Month::June, Month::July)]
    #[case::july(Month::July, Month::August)]
    #[case::august(Month::August, Month::September)]
    #[case::september(Month::September, Month::October)]
    #[case::october(Month::October, Month::November)]
    #[case::november(Month::November, Month::December)]
    #[case::december(Month::December, Month::January)]
    fn next_month(#[case] starting: Month, #[case] expected: Month) {
        let actual = starting.next();
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::same_date("2026-02-02", "2026-02-02", 0)]
    #[case::one_day_future("2026-02-03", "2026-02-02", 1)]
    #[case::two_days_future("2026-02-04", "2026-02-02", 2)]
    #[case::one_day_past("2026-02-01", "2026-02-02", -1)]
    #[case::two_weeks_past("2026-01-19", "2026-02-02", -14)]
    #[case::two_weeks_future("2026-02-16", "2026-02-02", 14)]
    #[case::across_years("2027-02-02", "2026-02-02", 365)]
    fn signed_days_from(#[case] date: &str, #[case] reference: &str, #[case] expected: i64) {
        let date = Date::from_str_unchecked(date);
        let reference = Date::from_str_unchecked(reference);

        let actual = date.signed_days_from(reference);

        assert_eq!(actual, expected);
    }

    #[test]
    fn end_of_day_utc_should_return_23_59_59() {
        let date = Date::from_str_unchecked("2026-03-15");

        let result = date.end_of_day_utc();

        assert_eq!(result.to_string(), "2026-03-15 23:59:59 UTC");
    }

    #[test]
    fn start_of_day_utc_should_return_00_00_00() {
        let date = Date::from_str_unchecked("2026-03-15");

        let result = date.start_of_day_utc();

        assert_eq!(result.to_string(), "2026-03-15 00:00:00 UTC");
    }

    #[rstest]
    #[case::mid_month("2026-03-15", "2026-02-28")]
    #[case::first_of_month("2026-03-01", "2026-02-28")]
    #[case::january_wraps_to_december("2026-01-10", "2025-12-31")]
    #[case::leap_year("2024-03-15", "2024-02-29")]
    fn last_day_of_previous_month_should_return_correct_date(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        let date = Date::from_str_unchecked(input);

        let actual = date.last_day_of_previous_month();

        assert_eq!(actual, Date::from_str_unchecked(expected));
    }

    #[rstest]
    #[case::january(2025, Month::January, "2025-01")]
    #[case::september(2025, Month::September, "2025-09")]
    #[case::december(2025, Month::December, "2025-12")]
    fn year_month_to_numeric_string_should_zero_pad(
        #[case] year: i32,
        #[case] month: Month,
        #[case] expected: &str,
    ) {
        let ym = YearMonth::new(year, month);

        let actual = ym.to_numeric_string();

        assert_eq!(actual, expected);
    }
}

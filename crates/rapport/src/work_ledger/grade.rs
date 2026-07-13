//! Review quality grades recorded by the Work ledger.
//!
//! This module owns parsing, ordering, display, and serialization for the
//! grades used by Review tasks. Policy Context only supplies the configured
//! minimum as text at the boundary.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ReviewGrade(u8);

impl ReviewGrade {
    const DEFAULT_MINIMUM: Self = Self(12);

    pub(super) const fn meets(self, minimum: Self) -> bool {
        self.0 >= minimum.0
    }

    fn label(self) -> &'static str {
        match self.0 {
            14 => "A+",
            13 => "A",
            12 => "A-",
            11 => "B+",
            10 => "B",
            9 => "B-",
            8 => "C+",
            7 => "C",
            6 => "C-",
            5 => "D+",
            4 => "D",
            3 => "D-",
            2 => "F+",
            1 => "F",
            _ => "F-",
        }
    }
}

impl Default for ReviewGrade {
    fn default() -> Self {
        Self::DEFAULT_MINIMUM
    }
}

impl fmt::Display for ReviewGrade {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

impl FromStr for ReviewGrade {
    type Err = ReviewGradeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let score = match value.trim().to_ascii_uppercase().as_str() {
            "A+" => 14,
            "A" => 13,
            "A-" => 12,
            "B+" => 11,
            "B" => 10,
            "B-" => 9,
            "C+" => 8,
            "C" => 7,
            "C-" => 6,
            "D+" => 5,
            "D" => 4,
            "D-" => 3,
            "F+" => 2,
            "F" => 1,
            "F-" => 0,
            _ => {
                return Err(ReviewGradeError {
                    value: value.to_owned(),
                });
            }
        };
        Ok(Self(score))
    }
}

impl Serialize for ReviewGrade {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for ReviewGrade {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid review grade `{value}`; expected A through F with optional + or -")]
pub(super) struct ReviewGradeError {
    value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades_preserve_quality_order() {
        let passing = "A-".parse::<ReviewGrade>().expect("valid passing grade");
        let failing = "B+".parse::<ReviewGrade>().expect("valid failing grade");

        assert!(passing.meets(failing));
        assert!(!failing.meets(passing));
    }
}

pub mod clock;
pub mod date;
pub mod offset;
pub mod query;
pub mod recurrence;
pub mod time;

pub(crate) trait DisplayExt {
    fn displayed(self) -> String;
}

impl<T: std::fmt::Display> DisplayExt for Option<T> {
    fn displayed(self) -> String {
        self.map_or_else(|| "none".to_string(), |v| v.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("date is not a valid format. Use YYYY-mm-dd format, surrounded by double quotes.")]
    InvalidDate,
    #[error("unable to parse recurrence string: {0}")]
    InvalidRecurrence(String),
    #[error("invalid offset: {0}")]
    InvalidOffset(String),
}

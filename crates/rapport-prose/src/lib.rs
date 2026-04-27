//! Output markdown friendly to humans and agents.
//!
//! Provides `OutputBuilder` for structured key-value and heading output,
//! and `ReportTable` / `Column` for tabular data.

mod builder;
mod table;

pub use builder::OutputBuilder;
pub use table::{Column, ReportTable, truncate};

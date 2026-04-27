//! Table rendering for CLI reports.

use std::fmt::Write;

/// Column definition: header text and width.
#[derive(Debug)]
pub struct Column {
    pub header: &'static str,
    pub width: usize,
}

impl Column {
    #[must_use]
    pub fn new(header: &'static str, width: usize) -> Self {
        Self { header, width }
    }
}

/// A structured table for CLI report output.
#[derive(Debug)]
pub struct ReportTable {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
}

impl ReportTable {
    #[must_use]
    pub fn new(columns: Vec<Column>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
        }
    }

    pub fn push_row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    #[must_use]
    pub fn render(self) -> String {
        let mut output = String::new();

        // Header
        for col in &self.columns {
            write!(output, "{:<width$} ", col.header, width = col.width).ok();
        }
        writeln!(output).ok();

        // Separator
        let total_width: usize = self.columns.iter().map(|c| c.width + 1).sum();
        writeln!(output, "{}", "─".repeat(total_width)).ok();

        // Rows
        for row in self.rows {
            for (cell, col) in row.into_iter().zip(&self.columns) {
                let truncated = truncate(&cell, col.width.saturating_sub(2));
                write!(output, "{:<width$} ", truncated, width = col.width).ok();
            }
            writeln!(output).ok();
        }

        output
    }
}

/// Truncate a string to fit within `max_len`, adding ellipsis if needed.
/// Uses character count for UTF-8 safety.
#[must_use]
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars()
                .take(max_len.saturating_sub(1))
                .collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn empty_table() {
        let table = ReportTable::new(vec![Column::new("Name", 20), Column::new("Value", 10)]);

        let output = table.render();

        assert!(output.contains("Name"));
        assert!(output.contains("Value"));
        assert!(output.contains("─"));
    }

    #[test]
    fn single_row() {
        let mut table = ReportTable::new(vec![Column::new("Name", 20), Column::new("Value", 10)]);
        table.push_row(vec!["Alice".to_string(), "100".to_string()]);

        let output = table.render();

        assert!(output.contains("Alice"));
        assert!(output.contains("100"));
    }

    #[test]
    fn long_text_is_truncated() {
        let mut table = ReportTable::new(vec![Column::new("Name", 10)]);
        table.push_row(vec!["This is a very long name".to_string()]);

        let output = table.render();

        assert!(output.contains("This is…"));
        assert!(!output.contains("This is a very long name"));
    }

    #[test]
    fn truncate_handles_utf8() {
        let result = truncate("こんにちは世界", 5);

        assert_eq!(result, "こんにち…");
    }

    #[test]
    fn truncate_short_string_unchanged() {
        let result = truncate("Hi", 10);

        assert_eq!(result, "Hi");
    }

    #[test]
    fn truncate_exact_length_unchanged() {
        let result = truncate("Hello", 5);

        assert_eq!(result, "Hello");
    }
}

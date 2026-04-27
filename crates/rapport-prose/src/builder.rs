//! Generic builder for constructing CLI output with consistent formatting.

use std::fmt::{Display, Write};

const NONE: &str = "(none)";

/// Builder for constructing CLI output with consistent formatting.
#[derive(Debug, Default)]
pub struct OutputBuilder {
    output: String,
}

impl OutputBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a level 1 heading (# Title) followed by a blank line.
    #[must_use]
    pub fn h1(mut self, text: impl Display) -> Self {
        writeln!(self.output, "# {text}").ok();
        self.blank()
    }

    /// Adds a level 1 heading with a count (# Title (N)) followed by a blank line.
    #[must_use]
    pub fn h1_with_count(mut self, text: &str, count: usize) -> Self {
        writeln!(self.output, "# {text} ({count})").ok();
        self.blank()
    }

    /// Adds a level 2 heading (## Title) followed by a blank line.
    #[must_use]
    pub fn h2(mut self, text: &str) -> Self {
        writeln!(self.output, "## {text}").ok();
        self.blank()
    }

    /// Adds a level 2 heading with a count (## Title (N)) followed by a blank line.
    #[must_use]
    pub fn h2_with_count(mut self, text: &str, count: usize) -> Self {
        writeln!(self.output, "## {text} ({count})").ok();
        self.blank()
    }

    /// Adds a level 3 heading (### Title) followed by a blank line.
    #[must_use]
    pub fn h3(mut self, text: &str) -> Self {
        writeln!(self.output, "### {text}").ok();
        self.blank()
    }

    /// Adds a key-value field (Key: value).
    #[must_use]
    pub fn field(mut self, key: &str, value: impl Display) -> Self {
        writeln!(self.output, "{key}: {value}").ok();
        self
    }

    /// Adds a key-value field only if the condition is true.
    #[must_use]
    pub fn field_if(self, condition: bool, key: &str, value: impl Display) -> Self {
        if condition {
            self.field(key, value)
        } else {
            self
        }
    }

    /// Adds a key-value field only if the value is Some.
    #[must_use]
    pub fn field_opt(mut self, key: &str, value: Option<impl Display>) -> Self {
        if let Some(v) = value {
            writeln!(self.output, "{key}: {v}").ok();
        }
        self
    }

    /// Adds a key-value field, showing "(none)" if the value is None.
    #[must_use]
    pub fn field_or_none(mut self, key: &str, value: Option<impl Display>) -> Self {
        match value {
            Some(v) => writeln!(self.output, "{key}: {v}").ok(),
            None => writeln!(self.output, "{key}: {NONE}").ok(),
        };
        self
    }

    /// Adds text, or "(none)" if the value is None.
    #[must_use]
    pub fn text_or_none(self, value: Option<impl Display>) -> Self {
        match value {
            Some(v) => self.text(v),
            None => self.text(NONE),
        }
    }

    /// Adds a blank line.
    #[must_use]
    pub fn blank(mut self) -> Self {
        writeln!(self.output).ok();
        self
    }

    /// Adds plain text (trailing whitespace is trimmed).
    #[must_use]
    pub fn text(mut self, text: impl Display) -> Self {
        let s = text.to_string();
        writeln!(self.output, "{}", s.trim_end()).ok();
        self
    }

    /// Adds a list of items using a formatter, or "(none)" if empty.
    #[must_use]
    pub fn list_or_none<T, I, F>(mut self, items: I, formatter: F) -> Self
    where
        I: IntoIterator<Item = T>,
        F: Fn(T) -> String,
    {
        let items: Vec<_> = items.into_iter().collect();
        if items.is_empty() {
            writeln!(self.output, "(none)").ok();
        } else {
            for item in items {
                writeln!(self.output, "{}", formatter(item)).ok();
            }
        }
        self
    }

    /// Adds a numbered list of items using a formatter, or "(none)" if empty.
    #[must_use]
    pub fn numbered_list_or_none<T, I, F>(mut self, items: I, formatter: F) -> Self
    where
        I: IntoIterator<Item = T>,
        F: Fn(T) -> String,
    {
        let items: Vec<_> = items.into_iter().collect();
        if items.is_empty() {
            writeln!(self.output, "(none)").ok();
        } else {
            for (i, item) in items.into_iter().enumerate() {
                writeln!(self.output, "{}. {}", i + 1, formatter(item)).ok();
            }
        }
        self
    }

    /// Iterates over records using a builder closure, or shows "(none)" if empty.
    ///
    /// Unlike `list_or_none` where each item produces a single line, this allows
    /// multi-line output per record by threading the builder through the closure.
    #[must_use]
    pub fn with_records_or_none<T, I, F>(self, items: I, formatter: F) -> Self
    where
        I: IntoIterator<Item = T>,
        F: Fn(Self, T) -> Self,
    {
        let items: Vec<_> = items.into_iter().collect();
        if items.is_empty() {
            self.text(NONE)
        } else {
            items.into_iter().fold(self, formatter)
        }
    }

    /// Consumes the builder and returns the final output string (trailing whitespace trimmed).
    #[must_use]
    pub fn build(mut self) -> String {
        let trimmed_len = self.output.trim_end().len();
        self.output.truncate(trimmed_len);
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn h1() {
        let output = OutputBuilder::new().h1("Title").build();

        assert_eq!(output, "# Title");
    }

    #[test]
    fn h1_with_count() {
        let output = OutputBuilder::new().h1_with_count("Comments", 3).build();

        assert_eq!(output, "# Comments (3)");
    }

    #[test]
    fn h2() {
        let output = OutputBuilder::new().h2("Section").build();

        assert_eq!(output, "## Section");
    }

    #[test]
    fn field() {
        let output = OutputBuilder::new()
            .field("Name", "Alice")
            .field("Age", 30)
            .build();

        assert_eq!(
            output,
            indoc! {"
            Name: Alice
            Age: 30"}
        );
    }

    #[test]
    fn field_opt() {
        let output = OutputBuilder::new()
            .field_opt("Present", Some("yes"))
            .field_opt("Missing", None::<&str>)
            .field_opt("Also", Some("here"))
            .build();

        assert_eq!(
            output,
            indoc! {"
            Present: yes
            Also: here"}
        );
    }

    #[test]
    fn list_or_none() {
        let output = OutputBuilder::new()
            .list_or_none(vec!["apple", "banana"], ToOwned::to_owned)
            .build();

        assert_eq!(
            output,
            indoc! {"
            apple
            banana"}
        );
    }

    #[test]
    fn test_list_or_none_empty() {
        let output = OutputBuilder::new()
            .list_or_none(Vec::<&str>::new(), ToOwned::to_owned)
            .build();

        assert_eq!(output, "(none)");
    }

    #[test]
    fn blank() {
        let output = OutputBuilder::new()
            .field("A", "1")
            .blank()
            .field("B", "2")
            .build();

        assert_eq!(
            output,
            indoc! {"
            A: 1

            B: 2"}
        );
    }

    #[test]
    fn text() {
        let output = OutputBuilder::new().text("Hello world").build();

        assert_eq!(output, "Hello world");
    }

    #[test]
    fn with_records_or_none_formats_multi_line_records() {
        let items = vec![("Alice", "hello"), ("Bob", "world")];
        let output = OutputBuilder::new()
            .with_records_or_none(items, |builder, (name, msg)| {
                builder.text(format!("from {name}:")).text(msg).blank()
            })
            .build();

        assert_eq!(
            output,
            indoc! {"
            from Alice:
            hello

            from Bob:
            world"}
        );
    }

    #[test]
    fn with_records_or_none_empty() {
        let output = OutputBuilder::new()
            .with_records_or_none(Vec::<&str>::new(), |builder, _| builder)
            .build();

        assert_eq!(output, "(none)");
    }

    #[test]
    fn text_trims_trailing_whitespace() {
        let output = OutputBuilder::new().text("hello\n\n").build();

        assert_eq!(output, "hello");
    }
}

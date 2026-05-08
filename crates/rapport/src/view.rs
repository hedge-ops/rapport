//! Domain output primitives for rapport.
//!
//! Renderers compose a [`View`] from these primitives; the type-state
//! shape (no `build()` until [`ViewBuilder::next_actions`]) enforces
//! that every view ends with a non-empty next-actions node. The
//! markdown layer underneath is `rapport-prose`; this module is the
//! only place that talks to it.

use nonempty::NonEmpty;
use rapport_prose::OutputBuilder;
use std::fmt::Display;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Pass,
    Fail,
}

impl Outcome {
    fn status_word(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunHint {
    command: String,
}

impl RunHint {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
        }
    }
}

impl Display for RunHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "└ run {}", self.command)
    }
}

#[derive(Debug, Default)]
pub struct ViewBuilder {
    inner: OutputBuilder,
    has_separator: bool,
}

impl ViewBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn title(mut self, text: impl Display) -> Self {
        self.inner = self.inner.h1(text);
        self.has_separator = true;
        self
    }

    #[must_use]
    pub fn paragraph(mut self, line: impl Display) -> Self {
        self.inner = self.inner.text(line);
        self.has_separator = false;
        self
    }

    #[must_use]
    pub fn section<F>(mut self, title: &str, body: F) -> Self
    where
        F: FnOnce(SectionBuilder) -> SectionBuilder,
    {
        self.inner = self.inner.h2(title);
        let section = body(SectionBuilder { inner: self.inner });
        self.inner = section.inner.blank();
        self.has_separator = true;
        self
    }

    #[must_use]
    pub fn status(mut self, outcome: Outcome, duration: Duration) -> Self {
        self.inner = self
            .inner
            .field("status", outcome.status_word())
            .field("duration", format!("{:.2}s", duration.as_secs_f64()));
        self.has_separator = false;
        self
    }

    #[must_use]
    pub fn next_actions(mut self, hints: NonEmpty<RunHint>) -> View {
        if !self.has_separator {
            self.inner = self.inner.blank();
        }
        self.inner = self.inner.h2("Next");
        for hint in hints {
            self.inner = self.inner.text(hint);
        }
        View { inner: self.inner }
    }
}

#[derive(Debug)]
pub struct SectionBuilder {
    inner: OutputBuilder,
}

impl SectionBuilder {
    #[must_use]
    pub fn usage<I, S>(mut self, lines: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Display,
    {
        self.inner = self.inner.text("```");
        for line in lines {
            self.inner = self.inner.text(line);
        }
        self.inner = self.inner.text("```");
        self
    }

    #[must_use]
    pub fn entries<K, V, I>(mut self, iter: I) -> Self
    where
        K: Display,
        V: Display,
        I: IntoIterator<Item = (K, V)>,
    {
        for (k, v) in iter {
            self.inner = self.inner.text(format!("- `{k}` — {v}"));
        }
        self
    }

    #[must_use]
    pub fn items<I, S>(mut self, iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Display,
    {
        for item in iter {
            self.inner = self.inner.text(format!("- {item}"));
        }
        self
    }

    #[must_use]
    pub fn captured(mut self, text: impl Display) -> Self {
        let s = text.to_string();
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            self.inner = self.inner.text("```").text(trimmed).text("```");
        }
        self
    }
}

#[derive(Debug)]
pub struct View {
    inner: OutputBuilder,
}

impl View {
    #[must_use]
    pub fn build(self) -> String {
        self.inner.build()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use indoc::indoc;
    use nonempty::nonempty;
    use pretty_assertions::assert_eq;

    #[test]
    fn status_pass_with_next_actions() {
        let view = ViewBuilder::new()
            .status(Outcome::Pass, Duration::from_millis(1234))
            .next_actions(nonempty![RunHint::new("rapport test .")])
            .build();
        assert_eq!(
            view,
            indoc! {"
            status: pass
            duration: 1.23s

            ## Next

            └ run rapport test ."}
        );
    }

    #[test]
    fn status_fail_with_captured_section() {
        let view = ViewBuilder::new()
            .section("Output", |b| b.captured("error: something went wrong"))
            .status(Outcome::Fail, Duration::from_millis(420))
            .next_actions(nonempty![RunHint::new("rapport build .")])
            .build();
        assert_eq!(
            view,
            indoc! {"
            ## Output

            ```
            error: something went wrong
            ```

            status: FAIL
            duration: 0.42s

            ## Next

            └ run rapport build ."}
        );
    }

    #[test]
    fn captured_omits_empty_blocks() {
        let view = ViewBuilder::new()
            .section("Output", |b| b.captured("   \n  "))
            .status(Outcome::Fail, Duration::from_secs(0))
            .next_actions(nonempty![RunHint::new("rapport help")])
            .build();
        assert_eq!(
            view,
            indoc! {"
            ## Output


            status: FAIL
            duration: 0.00s

            ## Next

            └ run rapport help"}
        );
    }

    #[test]
    fn help_view_with_title_usage_and_entries() {
        let view = ViewBuilder::new()
            .title("rapport — workspace command runner")
            .section("Usage", |b| {
                b.usage(["rapport <verb> <path>", "rapport help [<verb>]"])
            })
            .section("Verbs", |b| {
                b.entries([("build", "Verify the code compiles"), ("test", "Run tests")])
            })
            .next_actions(nonempty![RunHint::new("rapport help build")])
            .build();
        assert_eq!(
            view,
            indoc! {"
            # rapport — workspace command runner

            ## Usage

            ```
            rapport <verb> <path>
            rapport help [<verb>]
            ```

            ## Verbs

            - `build` — Verify the code compiles
            - `test` — Run tests

            ## Next

            └ run rapport help build"}
        );
    }

    #[test]
    fn error_view_uses_paragraphs() {
        let view = ViewBuilder::new()
            .paragraph("You ran: rapport build /nope")
            .paragraph("/nope does not exist or is not a directory.")
            .next_actions(nonempty![RunHint::new("rapport help build")])
            .build();
        assert_eq!(
            view,
            indoc! {"
            You ran: rapport build /nope
            /nope does not exist or is not a directory.

            ## Next

            └ run rapport help build"}
        );
    }

    #[test]
    fn run_hint_renders_with_lead_marker() {
        assert_eq!(RunHint::new("ls .").to_string(), "└ run ls .");
    }
}

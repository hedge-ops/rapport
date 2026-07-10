use crate::context::{Clock, CommandContext};
use crate::telemetry::{CommandEvent, CommandEventOutcome, TelemetryError, TelemetryWriter};
use crate::{RunHint, ViewBuilder};
use nonempty::nonempty;
use rapport_files::FileSystem;
use std::io::Write;
use std::process::ExitCode;

const SUCCESS: u8 = 0;

pub fn run<F, C, O, E>(
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let _ = writeln!(context.out, "{}", render_prime());
    finish("prime", arguments, context, CommandResult::success())
}

#[derive(Debug, Clone, Copy)]
struct CommandResult {
    outcome: CommandEventOutcome,
    exit_code: u8,
}

impl CommandResult {
    fn success() -> Self {
        Self {
            outcome: CommandEventOutcome::Success,
            exit_code: SUCCESS,
        }
    }
}

fn finish<F, C, O, E>(
    command: &'static str,
    arguments: Vec<String>,
    context: &mut CommandContext<'_, F, C, O, E>,
    result: CommandResult,
) -> ExitCode
where
    F: FileSystem,
    C: Clock,
    O: Write,
    E: Write,
{
    let event = CommandEvent::new(
        context.clock.now_rfc3339(),
        arguments,
        command,
        result.outcome,
        result.exit_code,
    );
    match TelemetryWriter::new(context.paths.clone()).append(context.fs, &event) {
        Ok(()) => ExitCode::from(result.exit_code),
        Err(error) => {
            let _ = writeln!(context.err, "{}", render_telemetry_error(&error));
            ExitCode::FAILURE
        }
    }
}

fn render_prime() -> String {
    ViewBuilder::new()
        .title("rapport prime")
        .section("Purpose", |b| {
            b.items([
                "Use Rapport before planning, coding, testing, building, reviewing, or integrating code.",
                "Rapport records active work, resolves repository rules, runs validation, and carries local work into GitHub.",
            ])
        })
        .section("Loop", |b| {
            b.items([
                "`rapport work start --title \"...\" --ticket <ticket> --objective \"...\" --path <path>` - create active work state",
                "`rapport work status` - inspect current work, paths, and recent facts",
                "`rapport context show <path>` - read folder purpose, ownership, boundaries, and applicable benchmarks",
                "`rapport work rules list` - read repository rules for active work paths",
                "`rapport doctor` - verify Git and GitHub prerequisites before integration",
                "`rapport build` - run applicable typed build operations for active work",
                "`rapport review` - emit host-neutral adversarial review requests; use `--result <file>` to record structured results",
                "`rapport integrate --summary \"...\" --message \"...\"` - commit active changes and open a PR",
                "`rapport work complete --summary \"...\"` - archive completed work and clear local state",
            ])
        })
        .section("Boundaries", |b| {
            b.items([
                "Keep `.rapport/work.toml` local; it is working memory, not project source.",
                "Prefer repository tools and rules discovered by Rapport over ad hoc workflow guesses.",
                "When changing Rapport itself, run an installed or copied Rapport binary for dogfooding builds.",
            ])
        })
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

fn render_telemetry_error(error: &TelemetryError) -> String {
    ViewBuilder::new()
        .title("rapport telemetry")
        .paragraph("Command completed, but telemetry could not be written.")
        .paragraph(error)
        .next_actions(nonempty![RunHint::new("rapport work status")])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prime_view_includes_the_core_workflow() {
        let view = render_prime();

        assert!(view.contains("planning, coding, testing, building, reviewing"));
        assert!(view.contains("rapport work start"));
        assert!(view.contains("rapport context show"));
        assert!(view.contains("rapport work rules list"));
        assert!(view.contains("rapport doctor"));
        assert!(view.contains("rapport build"));
        assert!(view.contains("rapport review"));
        assert!(view.contains("rapport integrate"));
        assert!(view.contains("rapport work complete"));
    }
}
